//! `nazm build`, end to end: source → executable → run it.
//!
//! Every case here compiles a real `.nz` file with the real `nazm` binary and executes
//! the result. A hand-written `.ll` proves `clang` exists and nothing about this compiler.
//!
//! # Two oracles, not one
//!
//! Each arithmetic case is checked against **an explicitly written expected result** and
//! against **the interpreter**. Either alone is weak in a different way: an expectation
//! can be written wrong, and differential agreement is satisfied by a bug both
//! implementations share — which, since one was written from the other's semantics, is the
//! likely kind.
//!
//! # Both optimisation levels
//!
//! Everything arithmetic is built at `-O0` and `-O2`. The specified behaviour must not
//! depend on the optimiser, and the way to get that wrong is `nsw`: it does not check for
//! overflow, it asserts overflow cannot happen, so `-O2` would be entitled to delete the
//! very branch meant to catch it. Running both levels is what would catch that.
//!
//! At `-O0` the locals are genuinely stored to and loaded from stack slots, so the
//! arithmetic executes on loaded values rather than being folded. At `-O2` LLVM may fold
//! it — and the *outcome must still be the specified one*, which is the stronger claim of
//! the two. M1a has no way to produce a value unknown at compile time: there are no
//! parameters and no input. That arrives with M1b.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

struct Built {
    dir: tempfile::TempDir,
    exe: PathBuf,
}

/// What running something produced.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn write_program(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    std::fs::write(&src, body).expect("write source");
    (dir, src)
}

fn build_at(body: &str, opt: u8) -> Result<Built, Ran> {
    let (dir, src) = write_program(body);
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .arg("--opt-level")
        .arg(opt.to_string())
        .output()
        .expect("run nazm build");
    if out.status.success() {
        Ok(Built { dir, exe })
    } else {
        Err(Ran {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

fn run_exe(b: &Built) -> Ran {
    let out = Command::new(&b.exe).output().expect("run the executable");
    let _ = &b.dir;
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn interpret(body: &str) -> Ran {
    let (dir, src) = write_program(body);
    let out = Command::new(NAZM)
        .args(["run", src.to_str().unwrap()])
        .output()
        .expect("run nazm run");
    let _ = dir;
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn program(body: &str) -> String {
    format!("fn main() -> Int {{\n{body}\n}}\n")
}

/// A program that must succeed, checked against its expected value and the interpreter,
/// at both optimisation levels.
fn expect_value(body: &str, expected: &str) {
    let src = program(body);
    let interp = interpret(&src);
    assert_eq!(
        interp.code,
        Some(0),
        "the interpreter did not accept this program: {}",
        interp.stderr
    );
    assert_eq!(
        interp.stdout.trim(),
        expected,
        "the *expectation* disagrees with the interpreter — one of them is wrong, and \
         differential agreement alone would have hidden it"
    );

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), expected, "-O{opt}");
        assert_eq!(
            ran.stdout.trim(),
            interp.stdout.trim(),
            "-O{opt}: the native program and the interpreter disagree"
        );
    }
}

/// A program that must fail at run time with a given code, at both optimisation levels.
fn expect_runtime_error(body: &str, code: &str, message: &str) {
    let src = program(body);
    let interp = interpret(&src);
    assert_eq!(
        interp.code,
        Some(2),
        "the interpreter did not fail: {interp:?}"
    );
    assert!(
        interp.stderr.contains(code) && interp.stderr.contains(message),
        "the expectation disagrees with the interpreter: {}",
        interp.stderr
    );

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(
            ran.code,
            Some(2),
            "-O{opt}: a specified runtime failure must exit 2, not {:?}. stderr: {}",
            ran.code,
            ran.stderr
        );
        assert!(
            ran.stderr.contains(code),
            "-O{opt}: missing diagnostic code {code}: {}",
            ran.stderr
        );
        assert!(
            ran.stderr.contains(message),
            "-O{opt}: missing message {message:?}: {}",
            ran.stderr
        );
        assert!(
            ran.stderr.contains("prog.nz:"),
            "-O{opt}: a runtime error must say where: {}",
            ran.stderr
        );
        assert!(ran.stdout.is_empty(), "-O{opt}: nothing should be printed");
    }
}

// ---------------------------------------------------------------------------
// Successful arithmetic
// ---------------------------------------------------------------------------

#[test]
fn ordinary_arithmetic_agrees_with_the_interpreter() {
    expect_value("    1 + 2 * 3 - 4", "3");
    expect_value("    (1 + 2) * (3 - 4)", "-3");
    expect_value("    100 / 7", "14");
    expect_value("    100 % 7", "2");
    expect_value("    -(3 * 4)", "-12");
}

#[test]
fn boundary_values_survive_the_round_trip() {
    // The full 64-bit range has to reach standard output. An exit status is 8 bits, so a
    // compiler that encoded the result there would be wrong for almost every `Int`.
    expect_value("    9223372036854775807", "9223372036854775807");
    expect_value("    -9223372036854775808", "-9223372036854775808");
    expect_value("    9223372036854775807 - 1", "9223372036854775806");
    expect_value("    -9223372036854775808 + 1", "-9223372036854775807");
}

#[test]
fn negative_division_and_remainder_truncate_toward_zero() {
    // The behaviour Nazm specifies, and the one that differs between languages — Python
    // floors. Written out rather than derived, so the expectation is a claim about the
    // specification and not about Rust.
    expect_value("    -7 / 2", "-3");
    expect_value("    -7 % 2", "-1");
    expect_value("    7 / -2", "-3");
    expect_value("    7 % -2", "1");
    expect_value("    -7 / -2", "3");
    expect_value("    -7 % -2", "-1");
}

#[test]
fn the_minimum_divided_into_by_minus_one_is_zero_for_remainder() {
    // `docs/user/spec.md`: `%` has exactly one failure case, a zero divisor. `Int::MIN % -1`
    // is mathematically 0 and 0 is representable, so it succeeds — even though the
    // hardware instruction that computes it faults on this exact pair, because it
    // computes the quotient. A backend that reached `srem` here would die.
    expect_value(
        "    let a = -9223372036854775808;\n    let b = -1;\n    a % b",
        "0",
    );
}

#[test]
fn local_bindings_resolve_and_nest() {
    expect_value("    let x = 10;\n    let y = 32;\n    x + y", "42");
    // Later bindings may read earlier ones.
    expect_value(
        "    let a = 2;\n    let b = a * 3;\n    let c = b + a;\n    c",
        "8",
    );
    // A binding's initialiser sees the *outer* binding of the same name, which is what
    // the checker says and what a slot-based resolver could easily get wrong.
    expect_value("    let x = 1;\n    let y = x + 10;\n    y", "11");
}

// ---------------------------------------------------------------------------
// Each exceptional case, separately
// ---------------------------------------------------------------------------

#[test]
fn addition_overflow_traps() {
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    m + 1",
        "N0400",
        "this addition overflows `Int`",
    );
}

#[test]
fn subtraction_overflow_traps() {
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    m - 1",
        "N0400",
        "this subtraction overflows `Int`",
    );
}

#[test]
fn multiplication_overflow_traps() {
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    m * 2",
        "N0400",
        "this multiplication overflows `Int`",
    );
}

#[test]
fn negating_the_minimum_traps() {
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    -m",
        "N0400",
        "this negation overflows `Int`",
    );
}

#[test]
fn division_by_zero_traps() {
    expect_runtime_error("    let z = 0;\n    7 / z", "N0401", "division by zero");
}

#[test]
fn remainder_by_zero_traps() {
    expect_runtime_error("    let z = 0;\n    7 % z", "N0401", "division by zero");
}

#[test]
fn the_minimum_divided_by_minus_one_traps() {
    // The quotient is one past the largest `Int`, so it is an overflow rather than a
    // division error — and the instruction would fault in hardware if it were reached.
    expect_runtime_error(
        "    let a = -9223372036854775808;\n    let b = -1;\n    a / b",
        "N0400",
        "this division overflows `Int`",
    );
}

#[test]
fn the_leftmost_failure_is_the_one_reported() {
    // Evaluation order is specified left to right, and here it is observable: both
    // operands would fail, and which diagnostic appears is the evidence that the order
    // was honoured.
    let src = program("    let z = 0;\n    let m = 9223372036854775807;\n    (m + 1) / z");
    let interp = interpret(&src);
    assert!(interp.stderr.contains("N0400"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).expect("build");
        let ran = run_exe(&built);
        assert!(
            ran.stderr.contains("N0400"),
            "-O{opt}: the right operand's failure was reported first: {}",
            ran.stderr
        );
    }
}

#[test]
fn the_two_implementations_report_the_same_source_position() {
    // A runtime error from a compiled program should point where the same error from
    // `nazm run` points. The compiled one has no syntax tree to consult — the position
    // is resolved at compile time and baked in as a constant — so agreement here is a
    // real check on that arithmetic, not a tautology.
    let src = "fn main() -> Int {\n    let z = 0;\n    7 / z\n}\n";

    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(
        interp.stderr.contains("prog.nz:3:7"),
        "the interpreter moved: {}",
        interp.stderr
    );

    let built = build_at(src, 0).expect("build");
    let ran = run_exe(&built);
    assert!(
        ran.stderr.contains("prog.nz:3:7"),
        "the compiled program points elsewhere: {}",
        ran.stderr
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Everything outside the subset, refused by name and with nothing written.
#[test]
fn constructs_outside_the_subset_are_refused_and_nothing_is_emitted() {
    // `if` and the comparison operators were on this list in M1a and are not any more:
    // M1b compiles them. What is left is what the backend still cannot lower.
    let cases: &[(&str, &str)] = &[
        (
            "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    x\n}\n",
            "`let mut`",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; }\n    i\n}\n",
            "`while`",
        ),
        ("fn main() -> Int {\n    return 1;\n}\n", "`return`"),
        // The signature is refused before the body is looked at, so this names the
        // return type — one mistake, one error, rather than also complaining about the
        // value that return type made necessary.
        (
            "fn main() -> Str {\n    \"hi\"\n}\n",
            "a function returning `Str`",
        ),
        // A `Str` reached from a body whose signature is fine.
        (
            "fn main() -> Int {\n    let s = \"x\";\n    1\n}\n",
            "a `Str` value",
        ),
        (
            "fn main() -> Int {\n    { 1 }\n}\n",
            "a block used as an expression",
        ),
        (
            "fn main() -> Int {\n    if true { 1 } else { 2 };\n    3\n}\n",
            "an expression statement",
        ),
    ];

    for (src, expected) in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prog.nz");
        std::fs::write(&path, src).expect("write");
        let exe = dir.path().join("prog");

        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(1), "{expected}: {stderr}");
        assert!(stderr.contains("N0101"), "{expected}: {stderr}");
        assert!(
            stderr.contains(expected),
            "expected {expected:?} in: {stderr}"
        );
        assert!(
            stderr.contains("nazm run"),
            "{expected}: a refusal should say what still works: {stderr}"
        );
        assert!(
            !exe.exists(),
            "{expected}: an unsupported program produced an executable"
        );

        // And the same program still runs under the interpreter, which is the point of
        // calling it unsupported rather than invalid.
        let ran = Command::new(NAZM)
            .args(["run", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{expected}: `nazm run` should still accept it: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
}

#[test]
fn a_program_that_does_not_check_is_a_check_failure_not_a_subset_failure() {
    // The distinction matters: one says "your program is wrong", the other says "this
    // compiler cannot do that yet". Reporting the first as the second would be confusing
    // and the reverse would be a lie.
    let (dir, src) = write_program("fn main() -> Int {\n    undefined_name\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("N0200"), "{stderr}");
    assert!(
        !stderr.contains("N0101"),
        "reported as unsupported: {stderr}"
    );
}

#[test]
fn build_diagnostics_are_available_as_json() {
    let (dir, src) = write_program("fn main() -> Int {\n    let mut x = 1;\n    x\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--json"])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let first = stderr.lines().next().unwrap_or_default();
    assert!(first.starts_with('{'), "not JSON: {first}");
    assert!(first.contains("N0101"), "{first}");
}

// ---------------------------------------------------------------------------
// The toolchain, and the output path
// ---------------------------------------------------------------------------

#[test]
fn a_missing_clang_is_reported_as_a_toolchain_problem_not_a_program_problem() {
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        // Nothing on PATH, so `clang` cannot be found.
        .env("PATH", "")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(4),
        "a missing tool must not be reported as a bad program: {stderr}"
    );
    assert!(stderr.contains("clang"), "{stderr}");
    assert!(
        stderr.contains("nazm run"),
        "it should say what still works without clang: {stderr}"
    );
}

#[test]
fn a_failing_clang_is_reported_with_its_own_output() {
    // A stub earlier on PATH than the real one. This exercises the path where the
    // toolchain runs and refuses, which is different from it being absent.
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let stub = bin.join("clang");
    std::fs::write(&stub, "#!/bin/sh\necho 'stub: refusing' >&2\nexit 1\n").expect("stub");
    set_executable(&stub);

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .env("PATH", &bin)
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(4), "{stderr}");
    assert!(
        stderr.contains("stub: refusing"),
        "clang's own output was lost: {stderr}"
    );
    assert!(
        stderr.contains("bug in `nazm build`"),
        "IR the compiler generated being rejected is a compiler bug, and should say so: {stderr}"
    );
}

#[test]
fn an_existing_executable_survives_a_failed_build() {
    // The failure that costs someone a working artefact: deleting the output, then
    // failing. Everything is built under a temporary name and moved into place last.
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let exe = dir.path().join("prog");
    std::fs::write(&exe, "an earlier, working build").expect("seed");

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .env("PATH", "")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(
        std::fs::read_to_string(&exe).expect("read"),
        "an earlier, working build",
        "a failed build destroyed the previous executable"
    );
}

#[test]
fn the_output_may_not_overwrite_the_input() {
    let (_dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let before = std::fs::read_to_string(&src).expect("read");

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&src)
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_ne!(out.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("own source"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&src).expect("read"),
        before,
        "the build overwrote its own source"
    );
}

#[test]
fn the_generated_ir_can_be_kept_for_inspection() {
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(0));

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("the IR");
    assert!(ir.contains("@llvm.sadd.with.overflow.i64"), "{ir}");
    assert!(!ir.contains(" nsw "), "emitted nsw: {ir}");
    assert!(!ir.contains("llvm.trap"), "emitted llvm.trap: {ir}");
}

#[test]
fn run_and_check_are_unchanged_by_the_new_subcommand() {
    // M1a must not alter either. The examples are the same ones the README quotes.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    for (file, expected) in [
        ("examples/max.nz", "25"),
        ("examples/sum.nz", "6074"),
        ("examples/primes.nz", "1060"),
        ("examples/search.nz", "1723"),
    ] {
        let path = root.join(file);
        let out = Command::new(NAZM)
            .args(["run", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(out.status.code(), Some(0), "{file}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            expected,
            "{file}"
        );

        let out = Command::new(NAZM)
            .args(["check", path.to_str().unwrap()])
            .output()
            .expect("check");
        assert_eq!(out.status.code(), Some(0), "{file}");
    }
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let mut p = std::fs::metadata(path).expect("metadata").permissions();
    p.set_mode(0o755);
    std::fs::set_permissions(path, p).expect("chmod");
}

/// The native toolchain is present, asserted separately and first.
///
/// Without this, a machine with no `clang` fails every compiling test in this file, and a
/// mutation run reads that as *every mutant caught* — a coverage claim manufactured by a
/// missing tool. Asserted on its own so the baseline check in `cargo xtask mutate` refuses
/// to start instead: "tests already fail before any mutation" is the truth, and "caught"
/// is not.
#[test]
fn the_native_toolchain_is_available() {
    let out = Command::new("clang").arg("--version").output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!("`clang --version` exited {:?}", o.status.code()),
        Err(e) => panic!(
            "`clang` is not usable ({e}). Every compiling test in this file needs it, and \
             a run without it measures the toolchain rather than the compiler."
        ),
    }
}

// ---------------------------------------------------------------------------
// M1b: Bool, comparisons, and value-position if/else
// ---------------------------------------------------------------------------

#[test]
fn every_comparison_operator_agrees_with_the_interpreter() {
    // Each of the six, with operands on either side of the answer and on it.
    for (op, a, b, expect) in [
        ("<", 1, 2, 1),
        ("<", 2, 2, 0),
        ("<", 3, 2, 0),
        ("<=", 1, 2, 1),
        ("<=", 2, 2, 1),
        ("<=", 3, 2, 0),
        (">", 3, 2, 1),
        (">", 2, 2, 0),
        (">", 1, 2, 0),
        (">=", 3, 2, 1),
        (">=", 2, 2, 1),
        (">=", 1, 2, 0),
        ("==", 2, 2, 1),
        ("==", 2, 3, 0),
        ("!=", 2, 3, 1),
        ("!=", 2, 2, 0),
    ] {
        expect_value(
            &format!("    if {a} {op} {b} {{ 1 }} else {{ 0 }}"),
            &expect.to_string(),
        );
    }
}

#[test]
fn comparisons_are_signed_at_the_boundaries() {
    // The case an unsigned predicate gets backwards: as bit patterns, `-1` is the
    // largest 64-bit value. `Int` is signed, so `-1 < 1`.
    expect_value("    if -1 < 1 { 1 } else { 0 }", "1");
    expect_value(
        "    if -9223372036854775808 < 9223372036854775807 { 1 } else { 0 }",
        "1",
    );
    expect_value("    if -9223372036854775808 >= 0 { 1 } else { 0 }", "0");
    // Equality at the extremes, where a truncating comparison would also go wrong.
    expect_value(
        "    let m = -9223372036854775808;\n    if m == -9223372036854775808 { 1 } else { 0 }",
        "1",
    );
}

#[test]
fn bool_locals_and_bool_equality_work() {
    expect_value("    let t = true;\n    if t { 7 } else { 8 }", "7");
    expect_value("    let f = false;\n    if f { 7 } else { 8 }", "8");
    // `==` on two `Bool`s: the checker admits equality on any single type.
    expect_value("    if true == true { 1 } else { 0 }", "1");
    expect_value("    if true != false { 1 } else { 0 }", "1");
    expect_value(
        "    let a = 1 < 2;\n    let b = 3 < 2;\n    if a == b { 1 } else { 0 }",
        "0",
    );
}

#[test]
fn both_branch_outcomes_are_reachable() {
    expect_value(
        "    let n = 5;\n    if n > 0 { n * 2 } else { 0 - n }",
        "10",
    );
    expect_value(
        "    let n = 0 - 5;\n    if n > 0 { n * 2 } else { 0 - n }",
        "5",
    );
}

#[test]
fn conditionals_nest_in_conditions_and_in_branches() {
    // An `if` producing a `Bool`, used as another `if`'s condition — a `Bool`
    // intermediate result that never becomes the program's value.
    expect_value(
        "    let a = 12;\n    let b = 5;\n    \
         if if a > b { a % b == 2 } else { false } { 1 } else { 0 }",
        "1",
    );
    // And nested in a branch.
    expect_value(
        "    let n = 7;\n    if n > 0 { if n > 5 { 2 } else { 1 } } else { 0 }",
        "2",
    );
    expect_value(
        "    let n = 3;\n    if n > 0 { if n > 5 { 2 } else { 1 } } else { 0 }",
        "1",
    );
    // Three deep, so the phi in the middle has a nested join as its predecessor.
    expect_value(
        "    let n = 9;\n    \
         if n > 0 { if n > 5 { if n > 8 { 3 } else { 2 } } else { 1 } } else { 0 }",
        "3",
    );
}

#[test]
fn branch_local_bindings_and_shadowing() {
    // A binding made inside a branch, used only there.
    expect_value(
        "    let n = 4;\n    if n > 0 { let double = n * 2; double + 1 } else { 0 }",
        "9",
    );
    // Shadowing an outer name inside a branch. `docs/user/spec.md` allows this in a nested
    // scope and refuses it in the same one, so the branch is exactly where it is legal.
    expect_value(
        "    let x = 1;\n    if true { let x = 100; x } else { x }",
        "100",
    );
    // And the outer binding is unaffected on the other path.
    expect_value(
        "    let x = 1;\n    if false { let x = 100; x } else { x }",
        "1",
    );
    // Both branches may bind the same name without colliding: distinct slots.
    expect_value(
        "    let c = true;\n    if c { let v = 10; v } else { let v = 20; v }",
        "10",
    );
}

#[test]
fn conditionals_compose_with_arithmetic_and_comparisons() {
    // An `if` as an operand, which is where a wrong phi type or a missing join shows up.
    expect_value("    1 + if true { 10 } else { 20 }", "11");
    expect_value("    (if false { 10 } else { 20 }) * 2", "40");
    expect_value(
        "    if (if true { 3 } else { 4 }) < 4 { 1 } else { 0 }",
        "1",
    );
    // Arithmetic inside a branch, so the branch ends in a guard's continuation block
    // rather than in the block it started in — the phi-predecessor case.
    expect_value(
        "    let a = 6;\n    if a > 0 { a * 7 + 1 } else { 0 }",
        "43",
    );
}

#[test]
fn a_failure_in_the_branch_not_taken_does_not_run() {
    // The property a conditional branch gives for free, and the one most worth pinning:
    // only the selected branch is evaluated.
    expect_value("    if true { 1 } else { 1 / 0 }", "1");
    expect_value("    if false { 1 / 0 } else { 2 }", "2");
    expect_value(
        "    let m = 9223372036854775807;\n    if false { m + 1 } else { 5 }",
        "5",
    );
    expect_value(
        "    let z = 0;\n    let m = -9223372036854775808;\n    \
         if 1 > 2 { m / -1 } else { if 2 > 1 { 42 } else { 7 / z } }",
        "42",
    );
}

#[test]
fn a_failure_in_the_branch_taken_reports_correctly() {
    expect_runtime_error(
        "    if true { 1 / 0 } else { 1 }",
        "N0401",
        "division by zero",
    );
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    if 1 < 2 { m + 1 } else { 0 }",
        "N0400",
        "this addition overflows `Int`",
    );
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    if false { 0 } else { m / -1 }",
        "N0400",
        "this division overflows `Int`",
    );
}

#[test]
fn the_failing_branch_reports_its_own_source_position() {
    // Not the `if`'s position, and not the other branch's: the operator that failed.
    let src = "fn main() -> Int {\n    let z = 0;\n    if true {\n        7 / z\n    } else {\n        1\n    }\n}\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(interp.stderr.contains("prog.nz:4:11"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(src, opt).expect("build");
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}: {ran:?}");
        assert!(
            ran.stderr.contains("prog.nz:4:11"),
            "-O{opt}: wrong position: {}",
            ran.stderr
        );
    }
}

#[test]
fn among_evaluated_operands_the_first_failure_still_wins() {
    // Left-to-right, and here both operands are on the taken path. The condition is
    // evaluated before either branch, so a failing condition beats a failing branch too.
    let src = program(
        "    let z = 0;\n    let m = 9223372036854775807;\n    \
         if (m + 1) > 0 { 7 / z } else { 0 }",
    );
    let interp = interpret(&src);
    assert!(interp.stderr.contains("N0400"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).expect("build");
        let ran = run_exe(&built);
        assert!(
            ran.stderr.contains("N0400"),
            "-O{opt}: the branch's failure was reported before the condition's: {}",
            ran.stderr
        );
    }
}

#[test]
fn the_condition_is_evaluated_exactly_once() {
    // A condition that fails must fail once, and reaching the branch at all would mean
    // it had been evaluated twice or not at all. Observable here because the failure is
    // in the condition and both branches are fine.
    expect_runtime_error(
        "    let z = 0;\n    if (7 / z) > 0 { 1 } else { 2 }",
        "N0401",
        "division by zero",
    );
}

#[test]
fn front_end_type_errors_are_not_reported_as_backend_limits() {
    // A non-`Bool` condition and mismatched branch types are the checker's business.
    // Reporting them as "not in the subset" would tell the user to wait for a release
    // that will never make their program correct.
    for (src, code) in [
        (
            "fn main() -> Int {\n    if 1 { 2 } else { 3 }\n}\n",
            "N0301",
        ),
        (
            "fn main() -> Int {\n    if true { 1 } else { false }\n}\n",
            "N0302",
        ),
    ] {
        let (dir, path) = write_program(src);
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(dir.path().join("prog"))
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a type error was reported as a backend limit: {stderr}"
        );
    }
}

#[test]
fn unsupported_constructs_inside_a_branch_are_refused_by_the_backend() {
    let cases: &[(&str, &str)] = &[
        (
            "fn main() -> Int {\n    if true { let mut x = 1; x = 2; x } else { 0 }\n}\n",
            "`let mut`",
        ),
        (
            "fn main() -> Int {\n    if true { return 1; } else { 0 }\n}\n",
            "`return`",
        ),
        (
            "fn f(n: Int) -> Int { if n < 1 { 0 } else { f(n - 1) } }\n\
             fn main() -> Int {\n    if true { f(3) } else { 0 }\n}\n",
            "from inside itself",
        ),
        (
            "fn main() -> Int {\n    if \"a\" == \"b\" { 1 } else { 0 }\n}\n",
            "a `Str` value",
        ),
    ];

    for (src, expected) in cases {
        let (dir, path) = write_program(src);
        let exe = dir.path().join("prog");
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(1), "{expected}: {stderr}");
        assert!(stderr.contains("N0101"), "{expected}: {stderr}");
        assert!(stderr.contains(expected), "expected {expected:?}: {stderr}");
        assert!(
            !exe.exists(),
            "{expected}: an unsupported program produced an executable"
        );
    }
}

#[test]
fn a_statement_position_if_is_still_refused() {
    // The one `if` this backend compiles is the one in value position. A statement `if`
    // arrives as an expression statement, which stays refused — deliberately, so that
    // adding value-position `if` did not quietly also add the other one.
    let src = "fn main() -> Int {\n    let x = 1;\n    if x > 0 { let y = 2; }\n    x\n}\n";
    let (dir, path) = write_program(src);
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("an expression statement"), "{stderr}");

    // And `nazm run` still executes it, which is what makes this a backend limit.
    let ran = Command::new(NAZM)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("run");
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

#[test]
fn the_emitted_ir_for_a_conditional_is_well_formed() {
    // clang accepting the module is the real check — it verifies the IR, including that
    // every phi names actual predecessors. These assertions add *which* construct was
    // used, which a successful compile alone does not say.
    let (dir, src) =
        write_program("fn main() -> Int {\n    let a = 6;\n    if a > 5 { a * 7 } else { 0 }\n}\n");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("the IR");
    assert!(ir.contains("icmp sgt i64"), "{ir}");
    assert!(ir.contains("br i1 %"), "{ir}");
    assert!(ir.contains("phi i64"), "{ir}");
    assert!(!ir.contains(" nsw "), "{ir}");
    assert!(!ir.contains("llvm.trap"), "{ir}");
}

// ---------------------------------------------------------------------------
// Stack safety, out of process
// ---------------------------------------------------------------------------

/// Deep-but-accepted and over-limit syntax, through the real binary.
///
/// The failure mode being guarded against is **the process dying**, which cannot be
/// observed from inside the process it kills — so this runs `nazm` as a child and asserts
/// on its exit status and output.
///
/// An earlier version only required `status.code().is_some()` and then `code == 0 || 1`.
/// That is far too weak: it would have passed if the accepted program had been *refused*,
/// and it said nothing about which diagnostic an over-limit program produced. Each case
/// below now asserts the one outcome it should have, and a panic or an unrelated failure
/// fails the test rather than satisfying a disjunction.
///
/// Exercised on: `aarch64-apple-darwin`, against the child's main thread. Other platforms
/// and other thread stack sizes are unverified.
#[test]
fn deep_syntax_produces_exactly_the_expected_outcome() {
    // A long binary chain is the deep shape the compiled subset can actually express: it
    // builds one tree level per operator while the parser never recurses.
    let chain = |n: usize| {
        format!(
            "fn main() -> Int {{\n    {}\n}}\n",
            vec!["1"; n].join(" + ")
        )
    };

    // Accepted, and it must *work*: check passes, build produces an executable, and that
    // executable prints the right number.
    let (dir, src) = write_program(&chain(511));
    let checked = nazm(&["check", src.to_str().unwrap()], &dir);
    assert_ok(&checked, "`nazm check` on a 511-term chain");

    let exe = dir.path().join("prog");
    let built = nazm(
        &["build", src.to_str().unwrap(), "-o", exe.to_str().unwrap()],
        &dir,
    );
    assert_ok(&built, "`nazm build` on a 511-term chain");
    assert!(exe.exists(), "the accepted program produced no executable");
    let ran = Command::new(&exe).output().expect("run");
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "511");

    // Over the limit: refused, with the nesting diagnostic and nothing else.
    for cmd in ["check", "build"] {
        let (dir, src) = write_program(&chain(600));
        let exe = dir.path().join("prog");
        let args: Vec<&str> = if cmd == "build" {
            vec![cmd, src.to_str().unwrap(), "-o", exe.to_str().unwrap()]
        } else {
            vec![cmd, src.to_str().unwrap()]
        };
        let out = nazm(&args, &dir);
        assert_diagnostic(&out, "N0102", &format!("`nazm {cmd}` on a 600-term chain"));
        assert!(!exe.exists(), "a refused program produced an executable");
    }
}

/// Deep nesting that only the checker can reach, and the backend's refusal of it.
///
/// Nested blocks are the most expensive shape per level. They are also **accepted by the
/// language and unsupported by this backend**, which makes them the case that
/// distinguishes a nesting diagnostic from a backend limitation — two different refusals
/// that an `is_some()` assertion would have conflated.
#[test]
fn deeply_nested_blocks_check_but_are_refused_by_the_backend() {
    let nest = |n: usize| {
        format!(
            "fn main() -> Int {{\n{}1{}\n}}\n",
            "{ ".repeat(n),
            " }".repeat(n)
        )
    };

    // Within the limit: the checker accepts it.
    let (dir, src) = write_program(&nest(255));
    assert_ok(
        &nazm(&["check", src.to_str().unwrap()], &dir),
        "`nazm check` on 255 nested blocks",
    );

    // And the backend refuses it — as a *backend limitation*, not a nesting error.
    let exe = dir.path().join("prog");
    let built = nazm(
        &["build", src.to_str().unwrap(), "-o", exe.to_str().unwrap()],
        &dir,
    );
    assert_diagnostic(&built, "N0101", "`nazm build` on 255 nested blocks");
    assert!(
        built.stderr.contains("a block used as an expression"),
        "the backend limitation should name the construct: {}",
        built.stderr
    );
    assert!(!exe.exists());

    // Past the limit: the nesting diagnostic, from the parser, before anything else.
    let (dir, src) = write_program(&nest(400));
    assert_diagnostic(
        &nazm(&["check", src.to_str().unwrap()], &dir),
        "N0102",
        "`nazm check` on 400 nested blocks",
    );
}

/// Run `nazm`, keeping the temporary directory alive for the call.
fn nazm(args: &[&str], dir: &tempfile::TempDir) -> Ran {
    let out = Command::new(NAZM).args(args).output().expect("run nazm");
    let _ = dir;
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Exit 0, and no sign of a panic or a signal.
fn assert_ok(r: &Ran, what: &str) {
    assert_no_panic(r, what);
    assert_eq!(
        r.code,
        Some(0),
        "{what} should have succeeded: {}",
        r.stderr
    );
}

/// Exit 1 carrying exactly this diagnostic code, and no sign of a panic or a signal.
fn assert_diagnostic(r: &Ran, code: &str, what: &str) {
    assert_no_panic(r, what);
    assert_eq!(
        r.code,
        Some(1),
        "{what} should have been refused with a diagnostic, not exited {:?}: {}",
        r.code,
        r.stderr
    );
    assert!(
        r.stderr.contains(code),
        "{what} should have reported {code}: {}",
        r.stderr
    );
}

/// A signal death or a Rust panic is never an acceptable outcome, whatever the exit code.
fn assert_no_panic(r: &Ran, what: &str) {
    assert!(
        r.code.is_some(),
        "{what} was killed by a signal — a depth bound that aborts is not a bound"
    );
    assert!(
        !r.stderr.contains("panicked at") && !r.stderr.contains("RUST_BACKTRACE"),
        "{what} panicked: {}",
        r.stderr
    );
    assert_ne!(r.code, Some(101), "{what} exited with a panic status");
}

/// `nazm check` must not need the native toolchain.
///
/// The behavioural half of the emission-reach gate in `xtask/src/rules.rs`: that gate is a
/// source-level check on where `clang` may be mentioned, and this is the part no source
/// scan can establish — that the check-only path does not actually invoke it.
#[test]
fn check_does_not_need_clang() {
    let (_dir, src) = write_program("fn main() -> Int {\n    if 1 < 2 { 3 } else { 4 }\n}\n");
    let out = Command::new(NAZM)
        .args(["check", src.to_str().unwrap()])
        .env("PATH", "")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`nazm check` failed with no toolchain on PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// M1c: several functions, and non-recursive direct calls
// ---------------------------------------------------------------------------

/// A whole program, checked against an expected value and the interpreter at both levels.
///
/// Unlike `expect_value`, the caller supplies every function rather than a `main` body.
fn expect_program(src: &str, expected: &str) {
    let interp = interpret(src);
    assert_eq!(
        interp.code,
        Some(0),
        "the interpreter did not accept this program: {}",
        interp.stderr
    );
    assert_eq!(
        interp.stdout.trim(),
        expected,
        "the expectation disagrees with the interpreter"
    );

    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), expected, "-O{opt}");
        // Only the platform wrapper prints. A helper that wrote anything would show up
        // here as an extra line.
        assert_eq!(
            ran.stdout.lines().count(),
            1,
            "-O{opt}: something other than the entry wrapper produced output: {:?}",
            ran.stdout
        );
        assert!(
            ran.stderr.is_empty(),
            "-O{opt}: unexpected stderr: {}",
            ran.stderr
        );
    }
}

#[test]
fn functions_of_zero_one_and_several_parameters() {
    expect_program(
        "fn answer() -> Int { 42 }\nfn main() -> Int { answer() }\n",
        "42",
    );
    expect_program(
        "fn double(n: Int) -> Int { n * 2 }\nfn main() -> Int { double(21) }\n",
        "42",
    );
    expect_program(
        "fn clamp(v: Int, lo: Int, hi: Int) -> Int {\n    \
         if v < lo { lo } else { if v > hi { hi } else { v } }\n}\n\
         fn main() -> Int { clamp(150, 0, 100) }\n",
        "100",
    );
}

#[test]
fn arguments_are_not_swapped() {
    // Noncommutative on purpose. `sub(10, 3)` and `sub(3, 10)` differ in sign, and
    // division and remainder differ in magnitude too — so a backend that transposed two
    // same-typed arguments could not produce these numbers.
    expect_program(
        "fn sub(a: Int, b: Int) -> Int { a - b }\nfn main() -> Int { sub(10, 3) }\n",
        "7",
    );
    expect_program(
        "fn div(a: Int, b: Int) -> Int { a / b }\nfn main() -> Int { div(100, 7) }\n",
        "14",
    );
    expect_program(
        "fn rem(a: Int, b: Int) -> Int { a % b }\nfn main() -> Int { rem(100, 7) }\n",
        "2",
    );
    // Three parameters, so a rotation as well as a swap would be caught.
    expect_program(
        "fn mix(a: Int, b: Int, c: Int) -> Int { a * 100 + b * 10 + c }\n\
         fn main() -> Int { mix(1, 2, 3) }\n",
        "123",
    );
}

#[test]
fn forward_references_and_shared_helpers() {
    // `main` calls a helper declared *after* it: signatures are collected before any body
    // is lowered, so declaration order does not matter.
    expect_program(
        "fn main() -> Int { later(4) }\nfn later(n: Int) -> Int { n * 11 }\n",
        "44",
    );
    // Two callers sharing one helper, and one of them reached through another function.
    expect_program(
        "fn main() -> Int { twice(3) + indirect(5) }\n\
         fn twice(n: Int) -> Int { n * 2 }\n\
         fn indirect(n: Int) -> Int { twice(n) + 1 }\n",
        "17",
    );
}

#[test]
fn calls_nest_and_compose() {
    expect_program(
        "fn inc(n: Int) -> Int { n + 1 }\n\
         fn main() -> Int { inc(inc(inc(0))) }\n",
        "3",
    );
    // A call inside arithmetic, inside a comparison, and inside a condition.
    expect_program(
        "fn f(n: Int) -> Int { n * 3 }\n\
         fn main() -> Int {\n    \
             if f(2) > 5 { f(1) + f(2) } else { 0 }\n}\n",
        "9",
    );
    // A call supplying an argument to another call.
    expect_program(
        "fn add(a: Int, b: Int) -> Int { a + b }\n\
         fn one() -> Int { 1 }\n\
         fn main() -> Int { add(one(), add(one(), one())) }\n",
        "3",
    );
}

#[test]
fn bool_travels_through_parameters_and_returns() {
    expect_program(
        "fn is_negative(n: Int) -> Bool { n < 0 }\n\
         fn main() -> Int { if is_negative(0 - 3) { 1 } else { 0 } }\n",
        "1",
    );
    expect_program(
        "fn pick(flag: Bool, a: Int, b: Int) -> Int { if flag { a } else { b } }\n\
         fn main() -> Int { pick(false, 10, 20) }\n",
        "20",
    );
    // A `Bool` computed by one function, passed to another, used as a condition.
    expect_program(
        "fn gt(a: Int, b: Int) -> Bool { a > b }\n\
         fn both(x: Bool, y: Bool) -> Bool { if x { y } else { false } }\n\
         fn main() -> Int { if both(gt(3, 1), gt(5, 4)) { 7 } else { 8 } }\n",
        "7",
    );
}

#[test]
fn calls_in_selected_and_unselected_branches() {
    // A call in the branch not taken must not run — including one that would fail.
    expect_program(
        "fn boom(n: Int) -> Int { n / 0 }\n\
         fn main() -> Int { if false { boom(1) } else { 5 } }\n",
        "5",
    );
    expect_program(
        "fn ok(n: Int) -> Int { n + 1 }\n\
         fn boom(n: Int) -> Int { n / 0 }\n\
         fn main() -> Int { if 2 > 1 { ok(1) } else { boom(1) } }\n",
        "2",
    );
    // And a call *as* the condition.
    expect_program(
        "fn positive(n: Int) -> Bool { n > 0 }\n\
         fn main() -> Int { if positive(3) { 1 } else { 0 } }\n",
        "1",
    );
}

#[test]
fn each_function_owns_its_names() {
    // The same spelling in a parameter, a caller's local and a callee's local. If any two
    // shared storage the answers would collide.
    expect_program(
        "fn helper(n: Int) -> Int {\n    let total = n * 2;\n    total\n}\n\
         fn main() -> Int {\n    let total = 100;\n    let n = 3;\n    total + helper(n)\n}\n",
        "106",
    );
    // A local may shadow a *parameter* in a nested scope, which the checker allows.
    expect_program(
        "fn f(x: Int) -> Int {\n    let x = x + 1;\n    x\n}\n\
         fn main() -> Int { f(1) }\n",
        "2",
    );
    expect_program(
        "fn f(x: Int) -> Int { if true { let x = 100; x } else { x } }\n\
         fn main() -> Int { f(1) }\n",
        "100",
    );
    // A local and a function may share a name: the checker keeps them in separate
    // namespaces, so `g` is both a value and a callee here. Reproducing that, rather than
    // inventing a shadowing rule, is the point.
    expect_program(
        "fn g(n: Int) -> Int { n }\n\
         fn main() -> Int {\n    let g = 5;\n    g + g(1)\n}\n",
        "6",
    );
}

#[test]
fn boundary_values_survive_a_parameter() {
    expect_program(
        "fn id(n: Int) -> Int { n }\n\
         fn main() -> Int { id(-9223372036854775808) }\n",
        "-9223372036854775808",
    );
    expect_program(
        "fn id(n: Int) -> Int { n }\n\
         fn main() -> Int { id(9223372036854775807) }\n",
        "9223372036854775807",
    );
    expect_program(
        "fn neg(n: Int) -> Int { 0 - n }\n\
         fn main() -> Int { neg(-9223372036854775807) }\n",
        "9223372036854775807",
    );
}

#[test]
fn a_failure_inside_a_callee_reports_the_callees_own_position() {
    // Not the call site's line. The callee's guard carries the span of *its* operator,
    // which is the only position that tells the reader where to look.
    let src = "fn bad(n: Int) -> Int {\n    let z = 0;\n    n / z\n}\n\
               fn main() -> Int {\n    bad(7)\n}\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(interp.stderr.contains("prog.nz:3:7"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(src, opt).expect("build");
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}: {ran:?}");
        assert!(ran.stderr.contains("N0401"), "-O{opt}: {}", ran.stderr);
        assert!(
            ran.stderr.contains("prog.nz:3:7"),
            "-O{opt}: reported the call site instead of the failing operator: {}",
            ran.stderr
        );
        assert!(
            ran.stdout.is_empty(),
            "-O{opt}: a failing program printed a result"
        );
    }
}

#[test]
fn an_earlier_failing_argument_stops_everything_after_it() {
    // The first argument fails. The second argument must not be evaluated, and the callee
    // body must not run — both of which would produce a *different* diagnostic if they
    // did, which is what makes this observable.
    let src = "fn bad(n: Int) -> Int {\n    let z = 0;\n    n % z\n}\n\
               fn two(a: Int, b: Int) -> Int { a + b }\n\
               fn main() -> Int {\n    let z = 0;\n    let m = 9223372036854775807;\n    \
               two(m + 1, 7 / z)\n}\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(
        interp.stderr.contains("N0400"),
        "the first argument's overflow should win: {}",
        interp.stderr
    );

    for opt in [0u8, 2] {
        let built = build_at(src, opt).expect("build");
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}");
        assert!(
            ran.stderr.contains("N0400"),
            "-O{opt}: a later argument was evaluated first: {}",
            ran.stderr
        );
        assert!(
            !ran.stderr.contains("N0401"),
            "-O{opt}: the second argument ran despite the first failing: {}",
            ran.stderr
        );
    }
}

#[test]
fn a_failing_argument_stops_the_callee_body() {
    // The body would fail too, with a different code. Seeing the argument's code proves
    // the body was never entered.
    let src = "fn bad(n: Int) -> Int {\n    let z = 0;\n    n / z\n}\n\
               fn main() -> Int {\n    let m = 9223372036854775807;\n    bad(m + 1)\n}\n";
    for opt in [0u8, 2] {
        let built = build_at(src, opt).expect("build");
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}");
        assert!(ran.stderr.contains("N0400"), "-O{opt}: {}", ran.stderr);
        assert!(
            !ran.stderr.contains("N0401"),
            "-O{opt}: the callee body ran despite a failing argument: {}",
            ran.stderr
        );
    }
    assert!(interpret(src).stderr.contains("N0400"));
}

#[test]
fn recursion_is_refused_by_the_backend_and_still_runs_interpreted() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "direct",
            "fn f(n: Int) -> Int { if n < 1 { 0 } else { f(n - 1) } }\n\
             fn main() -> Int { f(3) }\n",
            "from inside itself",
        ),
        (
            "mutual",
            "fn a(n: Int) -> Int { if n < 1 { 0 } else { b(n - 1) } }\n\
             fn b(n: Int) -> Int { a(n) }\n\
             fn main() -> Int { a(2) }\n",
            "call cycle",
        ),
        (
            // The documented policy: every declared function is validated, reachable or
            // not. Checking only what `main` reaches would make deleting a call elsewhere
            // silently change whether this is diagnosed.
            "unreachable",
            "fn x(n: Int) -> Int { y(n) }\nfn y(n: Int) -> Int { x(n) }\n\
             fn main() -> Int { 7 }\n",
            "call cycle",
        ),
    ];

    for (what, src, phrase) in cases {
        let (dir, path) = write_program(src);
        let exe = dir.path().join("prog");
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(1), "{what}: {stderr}");
        assert!(stderr.contains("N0101"), "{what}: {stderr}");
        assert!(
            stderr.contains(phrase),
            "{what}: expected {phrase:?}: {stderr}"
        );
        assert!(
            !exe.exists(),
            "{what}: a refused program produced an executable"
        );

        // And the interpreter still runs it — which is what makes this a backend limit
        // rather than the program being wrong.
        let ran = Command::new(NAZM)
            .args(["run", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{what}: `nazm run` should still accept it: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
}

#[test]
fn call_errors_belong_to_the_checker_not_the_backend() {
    // Arity, argument type, unknown callee and duplicate definition are all language
    // errors. Reporting any of them as a backend limitation would tell the user to wait
    // for a release that will never make their program correct.
    for (src, code) in [
        (
            "fn f(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { f(1) }\n",
            "N0303",
        ),
        (
            "fn f(a: Int) -> Int { a }\nfn main() -> Int { f(true) }\n",
            "N0300",
        ),
        ("fn main() -> Int { nope(1) }\n", "N0201"),
        (
            "fn f() -> Int { 1 }\nfn f() -> Int { 2 }\nfn main() -> Int { f() }\n",
            "N0203",
        ),
    ] {
        let (dir, path) = write_program(src);
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(dir.path().join("prog"))
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{code}: {stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a language error was reported as a backend limit: {stderr}"
        );
    }
}

#[test]
fn unsupported_signatures_and_bodies_are_refused_by_name() {
    for (src, expected) in [
        (
            "fn f(s: Str) -> Int { 1 }\nfn main() -> Int { f(\"x\") }\n",
            "a parameter of type `Str`",
        ),
        (
            "fn f(n: Int) -> Str { \"x\" }\nfn main() -> Int { 1 }\n",
            "a function returning `Str`",
        ),
        (
            "fn f(n: Int) -> Int {\n    let mut t = n;\n    t = t + 1;\n    t\n}\n\
             fn main() -> Int { f(1) }\n",
            "`let mut`",
        ),
        (
            "fn f(n: Int) -> Int { return n; }\nfn main() -> Int { f(1) }\n",
            "`return`",
        ),
    ] {
        let (dir, path) = write_program(src);
        let exe = dir.path().join("prog");
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{expected}: {stderr}");
        assert!(stderr.contains("N0101"), "{expected}: {stderr}");
        assert!(stderr.contains(expected), "expected {expected:?}: {stderr}");
        assert!(!exe.exists(), "{expected}: produced an executable");
    }
}

#[test]
fn the_emitted_ir_for_a_multi_function_program_is_well_formed() {
    // clang accepting the module is the real check — it verifies signatures and call
    // types. These assertions add *which* shape was emitted, which a successful compile
    // alone does not say.
    let (dir, src) = write_program(
        "fn helper(n: Int, flag: Bool) -> Int { if flag { n * 2 } else { n } }\n\
         fn main() -> Int { helper(21, true) }\n",
    );
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("the IR");
    // Nazm functions are internal and carry generated symbols; the C entry point is a
    // separate, external one. Asserted by shape rather than by which id each function
    // happened to get — that depends on declaration order and is not a property worth
    // pinning.
    assert!(ir.contains("define i32 @main() {"), "{ir}");
    let defs: Vec<&str> = ir
        .lines()
        .filter(|l| l.starts_with("define internal i64 @nz.f"))
        .collect();
    assert_eq!(defs.len(), 2, "expected two Nazm functions:\n{ir}");
    assert!(
        defs.iter()
            .any(|d| d.contains("(i64 %p0, i1 %p1)") && d.ends_with("; helper")),
        "the two-parameter signature is wrong:\n{ir}"
    );
    assert!(
        defs.iter()
            .any(|d| d.contains("()") && d.ends_with("; main")),
        "nazm's main should take no parameters:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @nz.f") && ir.contains("(i64 %"),
        "no typed call emitted:\n{ir}"
    );
    // The runtime helper lives in the same reserved space, so nothing can collide.
    assert!(ir.contains("@nz.fail"), "{ir}");
    // Exactly one print, in the wrapper.
    assert_eq!(ir.matches("call i32 (ptr, ...) @printf").count(), 1, "{ir}");
    assert!(!ir.contains(" nsw "), "{ir}");
}
