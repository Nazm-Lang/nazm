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

mod common;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// How long any subprocess here may take before the test kills it and fails.
///
/// Generous, because this is not a performance assertion — it is the thing that stops a
/// **compiled Nazm program from hanging the test suite**. `nazm build` emits no iteration
/// budget (see `docs/spec.md`, *The iteration and depth limits*), so a
/// miscompiled loop does not fail, it runs forever; without a deadline the suite would
/// stop reporting rather than report a defect.
const DEADLINE: Duration = Duration::from_secs(30);

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
    let out = run_bounded(
        Command::new(NAZM)
            .args(["build", src.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .arg("--opt-level")
            .arg(opt.to_string()),
        DEADLINE,
    )
    .expect("`nazm build` did not terminate within the deadline");
    if out.code == Some(0) {
        Ok(Built { dir, exe })
    } else {
        Err(out)
    }
}

/// Run a command, killing it if it outlives `limit`.
///
/// `None` means the deadline was reached — **a distinct outcome from any exit status**,
/// never folded into one. A compiled program that does not terminate has not "failed with
/// no output"; it has not finished, and the two want different words in a failure message.
///
/// The child is killed *and reaped* on that path, so no test leaves a process behind for
/// the next one to trip over. Output is piped and read once the child is gone; every
/// program here prints at most one short line, well inside a pipe buffer, so there is no
/// deadlock to arrange around.
fn run_bounded(cmd: &mut Command, limit: Duration) -> Option<Ran> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("collect output");
                return Some(Ran {
                    code: status.code(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                });
            }
            None if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Run a built executable, failing the test if it outlives [`DEADLINE`].
fn run_exe(b: &Built) -> Ran {
    let _ = &b.dir;
    run_exe_within(b, DEADLINE).unwrap_or_else(|| {
        panic!(
            "the compiled program did not terminate within {DEADLINE:?}. A timeout is a \
             failure, not a result — `nazm build` emits no iteration budget, so a \
             miscompiled loop runs forever instead of reporting."
        )
    })
}

/// Run a built executable with an explicit deadline, and say plainly whether it finished.
fn run_exe_within(b: &Built, limit: Duration) -> Option<Ran> {
    let _ = &b.dir;
    run_bounded(&mut common::bounded_exe(&b.exe), limit)
}

fn interpret(body: &str) -> Ran {
    let (dir, src) = write_program(body);
    let ran = run_bounded(
        Command::new(NAZM).args(["run", src.to_str().unwrap()]),
        DEADLINE,
    );
    let _ = dir;
    ran.expect("`nazm run` did not terminate within the deadline")
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
    // `docs/spec.md`: `%` has exactly one failure case, a zero divisor. `Int::MIN % -1`
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
    // `if` and the comparisons left this list in M1b; `let mut`, assignment and `while`
    // in M1d; `break`, `continue` and statement-position `if` in M1e; `return` in M1f.
    // What is left is what the backend still cannot lower.
    let cases: &[(&str, &str)] = &[
        // `main` must still return `Int`, even though `Str` is otherwise native: the
        // platform wrapper prints an integer, and what a `Str` result would mean for an
        // exit status is a decision nobody has made.
        (
            "fn main() -> Str {\n    \"hi\"\n}\n",
            "a `main` that does not return `Int`",
        ),
        (
            "fn main() -> Int {\n    { 1 }\n}\n",
            "a block used as an expression",
        ),
        // A bare block used as an expression is the one construct left outside the
        // subset that is not recursion. Expression statements compile since B2: a
        // language with no unit type otherwise forces a binding at every mutation.
        (
            "fn main() -> Int {\n    { let y = 2; };\n    3\n}\n",
            "a block used as an expression",
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
    let (dir, src) = write_program("fn main() -> Int {\n    { 1 }\n}\n");
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
        ("examples/pipeline.nz", "42925"),
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
    // Shadowing an outer name inside a branch. `docs/spec.md` allows this in a nested
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
            "fn main() -> Int {\n    if true { { 1 } } else { 0 }\n}\n",
            "a block used as an expression",
        ),
        (
            "fn main() -> Int {\n    if true { { let y = 2; }; 1 } else { 0 }\n}\n",
            "a block used as an expression",
        ),
        (
            "fn f(n: Int) -> Int { if n < 1 { 0 } else { f(n - 1) } }\n\
             fn main() -> Int {\n    if true { f(3) } else { 0 }\n}\n",
            "from inside itself",
        ),
        (
            "fn main() -> Int {\n    if true { { 1 } } else { 0 }\n}\n",
            "a block used as an expression",
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
fn a_statement_position_if_compiles_but_other_expression_statements_do_not() {
    // M1e adds the statement `if` and **only** that. The other expression statements —
    // a bare block, a discarded call, a bare arithmetic expression — are separate
    // constructs with their own questions, and they stay refused so that adding one did
    // not quietly add the rest.
    expect_value("    let mut x = 1;\n    if x > 0 { x = 5; }\n    x", "5");
    // With no `else` and a false condition, the statement still falls through.
    expect_value("    let mut x = 1;\n    if x > 9 { x = 5; }\n    x", "1");

    expect_value("    1 + 1;\n    3", "3");

    for (src, expected) in [
        (
            "fn main() -> Int {\n    { let y = 2; };\n    3\n}\n",
            "a block used as an expression",
        ),
        (
            "fn main() -> Int {\n    { 1 }\n}\n",
            "a block used as an expression",
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
        assert!(stderr.contains(expected), "expected {expected:?}: {stderr}");
    }
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
    let ran = run_bounded(&mut common::bounded_exe(&exe), DEADLINE)
        .expect("the 511-term chain did not terminate within the deadline");
    assert_eq!(ran.code, Some(0));
    assert_eq!(ran.stdout.trim(), "511");

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
            "fn f(n: Int) -> Int { { 1 } }\nfn main() -> Int { f(1) }\n",
            "a block used as an expression",
        ),
        (
            "fn f(n: Int) -> Int {\n    let mut t = n;\n    while t < 10 { { t = t + 1; }; }\n    t\n}\n\
             fn main() -> Int { f(1) }\n",
            "a block used as an expression",
        ),
        (
            "fn f(n: Int) -> Int { { let y = 2; }; n }\nfn main() -> Int { f(1) }\n",
            "a block used as an expression",
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
    // The C entry point takes `argc`/`argv` since B3, so that `arg(i)` has something
    // to read. It takes them whether or not the program asks: the signature is fixed,
    // and only the globals and the stores are conditional.
    assert!(
        ir.contains("define i32 @main(i32 %argc, ptr %argv) {"),
        "{ir}"
    );
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

// ---------------------------------------------------------------------------
// M1d — mutable locals, assignment, and `while`
//
// Every case here terminates inside the interpreter's iteration budget, which is what
// makes differential comparison possible at all: `nazm build` emits **no** iteration
// budget, so the two implementations agree on what a terminating program produces and do
// not agree on what a non-terminating one does. See `docs/spec.md`, *The iteration and depth limits*. *The native
// execution contract for loops*. Nothing below infers a resource guarantee from a passing
// test, and every subprocess runs under a deadline whose expiry is a failure.
// ---------------------------------------------------------------------------

/// A runtime failure whose reported position is checked exactly, in both implementations.
///
/// `expect_runtime_error` only requires the file name to appear. Inside a loop that is too
/// weak: the interesting failures are in a *condition* and in a *body*, which are
/// different lines of the same statement, and a position that merely mentions the file
/// would be satisfied by either.
fn expect_runtime_error_at(body: &str, code: &str, message: &str, position: &str) {
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
    assert!(
        interp.stderr.contains(position),
        "the expected position {position} is not where the interpreter points: {}",
        interp.stderr
    );

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}: {ran:?}");
        assert!(ran.stderr.contains(code), "-O{opt}: {}", ran.stderr);
        assert!(ran.stderr.contains(message), "-O{opt}: {}", ran.stderr);
        assert!(
            ran.stderr.contains(position),
            "-O{opt}: expected {position}: {}",
            ran.stderr
        );
        assert!(ran.stdout.is_empty(), "-O{opt}: nothing should be printed");
    }
}

#[test]
fn a_loop_runs_zero_one_or_many_times() {
    // Zero. The condition is false on entry, so the body never runs and the loop is not
    // an error — the one case a `do/while` shape would get wrong.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 0 { n = n + 1; i = i + 1; }\n    n",
        "0",
    );
    // Exactly one.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 1 { n = n + 7; i = i + 1; }\n    n",
        "7",
    );
    // Many, with a count that a fencepost error would move by one in either direction.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 5 { n = n + 1; i = i + 1; }\n    n",
        "5",
    );
    // And a condition that is false on entry because a *mutable* local says so, rather
    // than because a literal does.
    expect_value(
        "    let mut go = false;\n    let mut n = 0;\n    while go { n = n + 1; }\n    n",
        "0",
    );
}

#[test]
fn sum_to_n_and_factorial_stay_inside_int() {
    // Sum to 100: 5050, a number wrong by a visible amount if the loop runs once too
    // often or too seldom.
    expect_value(
        "    let mut i = 1;\n    let mut total = 0;\n    while i <= 100 { total = total + i; i = i + 1; }\n    total",
        "5050",
    );
    // 20! is 2,432,902,008,176,640,000 — the largest factorial that fits in `Int`, so
    // this is also the boundary case for the checked multiply not firing early.
    expect_value(
        "    let mut k = 1;\n    let mut f = 1;\n    while k <= 20 { f = f * k; k = k + 1; }\n    f",
        "2432902008176640000",
    );
}

#[test]
fn nested_loops_agree_with_the_interpreter() {
    // A rectangle: 4 × 6 iterations of the inner body.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 4 {\n        let mut j = 0;\n        while j < 6 { n = n + 1; j = j + 1; }\n        i = i + 1;\n    }\n    n",
        "24",
    );
    // A triangle, where the inner bound depends on the outer variable — so the inner
    // condition has to be re-read against a value the outer body changed.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 5 {\n        let mut j = 0;\n        while j < i { n = n + 1; j = j + 1; }\n        i = i + 1;\n    }\n    n",
        "10",
    );
    // Three deep, so the back edge of the middle loop is itself inside a loop body.
    expect_value(
        "    let mut a = 0;\n    let mut n = 0;\n    while a < 3 {\n        let mut b = 0;\n        while b < 3 {\n            let mut c = 0;\n            while c < 3 { n = n + 1; c = c + 1; }\n            b = b + 1;\n        }\n        a = a + 1;\n    }\n    n",
        "27",
    );
}

#[test]
fn a_bool_local_changed_by_the_body_ends_the_loop() {
    // The condition is a plain `Bool` load, so the only thing that can stop this loop is
    // the body's store being visible to the next evaluation of the condition.
    expect_value(
        "    let mut go = true;\n    let mut n = 0;\n    while go {\n        n = n + 1;\n        go = n < 4;\n    }\n    n",
        "4",
    );
    // The same, with the flag set from an `if`/`else` rather than a comparison.
    expect_value(
        "    let mut go = true;\n    let mut n = 0;\n    while go {\n        n = n + 3;\n        go = if n > 8 { false } else { true };\n    }\n    n",
        "9",
    );
}

#[test]
fn helper_functions_are_callable_from_a_condition_and_a_body() {
    let src = "fn below(n: Int, limit: Int) -> Bool { n < limit }\n\
               fn twice(n: Int) -> Int { n * 2 }\n\
               fn main() -> Int {\n\
               \x20   let mut i = 1;\n\
               \x20   let mut steps = 0;\n\
               \x20   while below(i, 100) {\n\
               \x20       i = twice(i);\n\
               \x20       steps = steps + 1;\n\
               \x20   }\n\
               \x20   i + steps\n\
               }\n";
    // 1 → 2 → 4 → … → 128 is seven doublings.
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "135");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), "135", "-O{opt}");
    }
}

#[test]
fn an_assignment_value_may_be_an_if_else_expression() {
    // The value-position `if` from M1b, now on the right of an `=`. Both branches of the
    // `if` are inside the loop, so the join is reached once per iteration.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 6 {\n        s = s + if i % 2 == 0 { i } else { 0 };\n        i = i + 1;\n    }\n    s",
        "6",
    );
    // Nested, and with the `if` deciding the loop variable itself — a store whose value
    // came from a branch, read back by the condition.
    expect_value(
        "    let mut i = 0;\n    let mut hops = 0;\n    while i < 20 {\n        i = if i < 5 { i + 1 } else { if i < 10 { i + 2 } else { i + 5 } };\n        hops = hops + 1;\n    }\n    hops",
        "10",
    );
}

#[test]
fn a_loop_local_is_initialised_on_every_iteration() {
    // The slot for `acc` is allocated once and reused; its *value* must not be. If the
    // initialiser were hoisted out of the loop — storage reuse becoming value reuse —
    // `acc` would accumulate across iterations and this would be 306 rather than 303.
    expect_value(
        "    let mut i = 0;\n    let mut total = 0;\n    while i < 3 {\n        let mut acc = 100;\n        acc = acc + i;\n        total = total + acc;\n        i = i + 1;\n    }\n    total",
        "303",
    );
    // An immutable loop-local, which has the same obligation for a different reason: it
    // is a fresh binding each time round, not one binding read three times.
    expect_value(
        "    let mut i = 0;\n    let mut total = 0;\n    while i < 4 {\n        let doubled = i * 2;\n        total = total + doubled;\n        i = i + 1;\n    }\n    total",
        "12",
    );
}

#[test]
fn shadowing_inside_a_loop_leaves_the_outer_binding_alone() {
    // `x` is assigned before the inner `let mut x`, so that assignment reaches the outer
    // binding; the one after it reaches the inner. The inner binding is new on every
    // iteration and its value is discarded at the end of each.
    expect_value(
        "    let mut x = 1;\n    let mut i = 0;\n    while i < 3 {\n        x = x + 10;\n        let mut x = 100;\n        x = x + 1;\n        i = i + 1;\n    }\n    x",
        "31",
    );
    // The inner binding may even have a different type, which is only possible because
    // it really is a separate binding rather than a reassignment.
    expect_value(
        "    let mut n = 5;\n    let mut i = 0;\n    while i < 2 {\n        let mut n = true;\n        n = false;\n        i = i + 1;\n    }\n    n",
        "5",
    );
    // A nested loop's body shadowing the outer loop's counter, while the outer counter
    // keeps advancing — the case where a shared slot would hang the program rather than
    // produce a wrong number.
    expect_value(
        "    let mut i = 0;\n    let mut seen = 0;\n    while i < 3 {\n        let mut j = 0;\n        while j < 2 {\n            let i = 99;\n            seen = seen + i;\n            j = j + 1;\n        }\n        i = i + 1;\n    }\n    seen",
        "594",
    );
}

#[test]
fn mutable_locals_in_different_functions_are_independent() {
    // `count` exists in two functions at once, with different values, while one is on the
    // other's stack. Sharing a slot space between functions would make the inner call
    // overwrite the outer's counter.
    let src = "fn inner(n: Int) -> Int {\n\
               \x20   let mut count = 0;\n\
               \x20   let mut total = 0;\n\
               \x20   while count < n { total = total + 1; count = count + 1; }\n\
               \x20   total\n\
               }\n\
               fn main() -> Int {\n\
               \x20   let mut count = 0;\n\
               \x20   let mut total = 0;\n\
               \x20   while count < 4 {\n\
               \x20       total = total + inner(count);\n\
               \x20       count = count + 1;\n\
               \x20   }\n\
               \x20   total * 100 + count\n\
               }\n";
    // inner(0)+inner(1)+inner(2)+inner(3) = 0+1+2+3 = 6, and `main`'s own count reaches 4.
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "604");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "604", "-O{opt}");
    }
}

#[test]
fn a_failure_in_the_condition_reports_the_conditions_position() {
    // `10 / d` with `d` reaching 0 — the failure is in the condition, on the `while` line,
    // and not on any line of the body.
    expect_runtime_error_at(
        "    let mut d = 2;\n    let mut n = 0;\n    while 10 / d > 0 {\n        n = n + 1;\n        d = d - 1;\n    }\n    n",
        "N0401",
        "division by zero",
        "prog.nz:4:14",
    );
}

#[test]
fn a_failure_in_the_body_reports_the_bodys_position() {
    expect_runtime_error_at(
        "    let m = 9223372036854775807;\n    let mut i = 0;\n    let mut x = 0;\n    while i < 3 {\n        x = m + 1;\n        i = i + 1;\n    }\n    x",
        "N0400",
        "this addition overflows `Int`",
        "prog.nz:6:15",
    );
}

#[test]
fn a_loop_can_fail_on_a_later_iteration_rather_than_the_first() {
    // The first iterations succeed and only a later one fails, so this is not satisfied
    // by a compiler that evaluates the body once. `10 / (3 - i)` is fine for i = 0, 1, 2
    // and divides by zero on the fourth pass.
    expect_runtime_error_at(
        "    let mut i = 0;\n    let mut q = 0;\n    while i < 6 {\n        q = q + 10 / (3 - i);\n        i = i + 1;\n    }\n    q",
        "N0401",
        "division by zero",
        "prog.nz:5:20",
    );
    // Multiplication that only overflows after about forty iterations, which also shows
    // the guard is inside the loop rather than hoisted out of it.
    expect_runtime_error_at(
        "    let mut x = 1;\n    let mut i = 0;\n    while i < 100 {\n        x = x * 3;\n        i = i + 1;\n    }\n    x",
        "N0400",
        "this multiplication overflows `Int`",
        "prog.nz:5:15",
    );
}

#[test]
fn assignment_and_loop_condition_errors_belong_to_the_checker() {
    // A backend limitation must never be reported as a language error, and the reverse is
    // equally wrong. These three are the language's rules and `N0101` must not appear.
    for (src, code) in [
        (
            "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n",
            "N0310",
        ),
        (
            "fn main() -> Int {\n    let mut b = true;\n    b = 1;\n    0\n}\n",
            "N0300",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i { i = i + 1; }\n    i\n}\n",
            "N0301",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; missing = 1; }\n    i\n}\n",
            "N0200",
        ),
        // A loop body whose tail produces a value is the checker's `N0300`, not the
        // backend's refusal of a tail — so a reader is told the value is discarded rather
        // than that the compiler cannot do loops.
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; i }\n    i\n}\n",
            "N0300",
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
        assert_eq!(out.status.code(), Some(1), "{code}: {stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a language error was reported as a backend limit: {stderr}"
        );
        assert!(!exe.exists(), "{code}: produced an executable");
    }
}

#[test]
fn unsupported_control_flow_inside_a_loop_is_refused_and_nothing_is_published() {
    // What is left after B2. `break`, `continue`, statement `if`, `return`, `Str` and
    // discarded expressions all compile now, so what remains inside a loop is a bare
    // block used as an expression, and recursion.
    for (src, expected) in [
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; { 1 }; }\n    i\n}\n",
            "a block used as an expression",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { { 1 }; i = i + 1; }\n    i\n}\n",
            "a block used as an expression",
        ),
        // Recursion, reached from inside a loop. Still refused, and still for a reason
        // about the stack rather than about loops.
        (
            "fn f(n: Int) -> Int { if n < 1 { return 0; } return f(n - 1); }\n\
             fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1 + f(2); }\n    i\n}\n",
            "from inside itself",
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
        assert!(
            stderr.contains("nazm run"),
            "{expected}: a refusal should say what still works: {stderr}"
        );
        assert!(!exe.exists(), "{expected}: produced an executable");

        // Refused, not invalid: the interpreter runs every one of these.
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
fn a_working_loop_executable_survives_a_later_refused_build() {
    // The output is published only after `clang` succeeds, so a build that is refused for
    // being outside the subset must leave the previous executable exactly as it was —
    // the failure mode that costs someone a working artefact at the worst moment.
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    let exe = dir.path().join("prog");

    std::fs::write(
        &src,
        "fn main() -> Int {\n    let mut i = 0;\n    while i < 5 { i = i + 1; }\n    i\n}\n",
    )
    .expect("write");
    let first = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let before = std::fs::read(&exe).expect("read the executable");

    // Now edit the source to something the backend refuses.
    std::fs::write(
        &src,
        "fn main() -> Int {\n    let mut i = 0;\n    while i < 5 { i = i + 1; { 1 }; }\n    i\n}\n",
    )
    .expect("write");
    let second = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("N0101"));

    assert_eq!(
        std::fs::read(&exe).expect("read again"),
        before,
        "the refused build replaced a working executable"
    );
    let built = Built {
        dir,
        exe: exe.clone(),
    };
    let ran = run_exe(&built);
    assert_eq!(ran.code, Some(0));
    assert_eq!(
        ran.stdout.trim(),
        "5",
        "the surviving executable still works"
    );
}

#[test]
fn many_iterations_with_body_locals_allocate_once_in_the_entry_block() {
    // Two claims, and they are separate.
    //
    // The first is structural: **every `alloca` is in the entry block**, read out of the
    // emitted IR. A slot belongs to a binding, not to an execution of it, so a body-local
    // declaration allocates once however many times the loop goes round. An `alloca` on
    // the loop path would grow the stack by a cell per iteration.
    //
    // The second is that a bounded run with such a body completes and produces the right
    // number. That is *not* a whole-program memory bound: it says this program at this
    // iteration count did not fail, and nothing about any other.
    let src = "fn main() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   let mut total = 0;\n\
               \x20   while i < 20000 {\n\
               \x20       let mut a = 1;\n\
               \x20       let b = i % 7;\n\
               \x20       let mut c = 0;\n\
               \x20       a = a + b;\n\
               \x20       c = a;\n\
               \x20       total = total + c;\n\
               \x20       i = i + 1;\n\
               \x20   }\n\
               \x20   total\n\
               }\n";

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("emitted IR");
    let body: &str = ir
        .split("define internal i64 @nz.f0()")
        .nth(1)
        .expect("main's definition");

    // Walk the definition, tracking which block each line is in. Every `alloca` must be
    // in `entry`, and `entry` is the block before the first label.
    let mut block = "entry".to_owned();
    let mut allocas = 0usize;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "}" {
            break;
        }
        if let Some(label) = trimmed.strip_suffix(':') {
            if !label.contains(char::is_whitespace) {
                block = label.to_owned();
                continue;
            }
        }
        if trimmed.contains("alloca") {
            allocas += 1;
            assert_eq!(
                block, "entry",
                "an alloca outside the entry block, in {block}: {trimmed}"
            );
        }
    }
    // i, total, a, b, c.
    assert_eq!(
        allocas, 5,
        "one slot per binding, no more and no fewer:\n{ir}"
    );

    // The loop really does go round: some branch targets a block that is already open
    // above it. Phrased as the property rather than as a literal label, because the
    // label's number is an emitter detail — it changed once when block labels stopped
    // sharing a counter with SSA temporaries, and a test that pins it fails for a reason
    // that has nothing to do with control flow.
    let mut seen: Vec<&str> = Vec::new();
    let mut back_edge = None;
    for line in ir.lines() {
        let trimmed = line.trim();
        if let Some(label) = trimmed.strip_suffix(':') {
            if !label.contains(char::is_whitespace) {
                seen.push(label);
                continue;
            }
        }
        if let Some(target) = trimmed.strip_prefix("br label %") {
            let target = target.split_whitespace().next().unwrap_or(target);
            if seen.contains(&target) {
                back_edge = Some(target.to_owned());
                break;
            }
        }
    }
    assert!(
        back_edge.is_some(),
        "no back edge: every `br label` goes forward, so the loop cannot repeat:\n{ir}"
    );

    // 20000 iterations of `(i % 7) + 1`.
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    let built = Built { dir, exe };
    let ran = run_exe(&built);
    assert_eq!(ran.code, Some(0), "{ran:?}");
    assert_eq!(
        ran.stdout.trim(),
        interp.stdout.trim(),
        "the native program and the interpreter disagree"
    );
    assert_eq!(ran.stdout.trim(), "79997");
}

#[test]
fn the_condition_is_reevaluated_on_every_iteration() {
    // The discriminating test for the loop's one load-bearing property. A back edge that
    // jumped to the *body* instead of the condition block would evaluate the condition
    // once, find it true, and never look again — so this program would never terminate.
    //
    // Deliberately given its own short deadline and asserted on directly, because the
    // failure that defect produces is **non-termination, not a wrong value**. A test that
    // only compared numbers would hang instead of failing.
    let src = program(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 3 {\n        n = n + 1;\n        i = i + 1;\n    }\n    n",
    );
    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        let ran = run_exe_within(&built, Duration::from_secs(10)).unwrap_or_else(|| {
            panic!(
                "-O{opt}: the loop did not terminate within 10s — the condition is not \
                 being re-evaluated. A timeout here is a failure, never a result."
            )
        });
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), "3", "-O{opt}");
    }

    // The other half of the same property: the condition must see what the body stored.
    // A loop whose condition read a stale copy would run forever here too.
    expect_value(
        "    let mut go = true;\n    let mut n = 0;\n    while go { n = n + 1; go = n < 10; }\n    n",
        "10",
    );
}

#[test]
fn a_loop_inside_a_branch_is_the_joins_predecessor() {
    // The predecessor rule, which a loop is the newest way to break. The `phi` at the
    // join must name the block the branch *ended* in — for a branch containing a loop
    // that is the loop's **exit** block, not the label the branch was opened with. Naming
    // the entry label produces IR clang rejects, or worse, one it accepts with the wrong
    // incoming value.
    expect_value(
        "    let mut n = 0;\n    let v = if n < 1 {\n        let mut i = 0;\n        let mut s = 0;\n        while i < 4 { s = s + i; i = i + 1; }\n        s\n    } else { 99 };\n    n = v + 1;\n    n",
        "7",
    );
    // The other branch, so neither side is the only one exercised.
    expect_value(
        "    let mut n = 5;\n    let v = if n < 1 {\n        99\n    } else {\n        let mut i = 0;\n        while i < 3 { i = i + 1; }\n        i\n    };\n    v",
        "3",
    );
    // A loop whose *condition* contains an `if`, so the conditional branch is emitted in
    // the join block the `if` opened rather than in the loop's own head block.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while if n < 2 { i < 10 } else { false } {\n        n = n + 1;\n        i = i + 1;\n    }\n    i * 100 + n",
        "202",
    );
    // And a loop in both branches of an `if` that is itself inside a loop body.
    expect_value(
        "    let mut outer = 0;\n    let mut total = 0;\n    while outer < 3 {\n        total = total + if outer % 2 == 0 {\n            let mut a = 0;\n            let mut x = 0;\n            while a < 2 { x = x + 10; a = a + 1; }\n            x\n        } else {\n            let mut b = 0;\n            let mut y = 0;\n            while b < 3 { y = y + 1; b = b + 1; }\n            y\n        };\n        outer = outer + 1;\n    }\n    total",
        "43",
    );
}

// ---------------------------------------------------------------------------
// M1e — `break`, `continue`, and statement-position `if`
//
// Every case terminates inside the interpreter's budget, for the reason M1d records: a
// compiled program has no iteration budget, so the two implementations agree on what a
// terminating program produces and nothing more. Deadlines still apply to every
// subprocess, and a timeout is a failure.
// ---------------------------------------------------------------------------

#[test]
fn a_loop_can_break_immediately() {
    expect_value(
        "    let mut i = 0;\n    while i < 10 { break; }\n    i",
        "0",
    );
    // With work before the break, so "immediately" means the first iteration rather than
    // no iteration.
    expect_value(
        "    let mut i = 0;\n    while i < 10 { i = i + 7; break; }\n    i",
        "7",
    );
}

#[test]
fn continue_skips_the_rest_of_the_iteration_and_rechecks_the_condition() {
    // `s` must miss exactly the skipped values, which a `continue` that fell through to
    // the rest of the body would not, and one that jumped to the body rather than the
    // condition would never terminate.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 6 {\n        i = i + 1;\n        if i == 3 { continue; }\n        s = s + i;\n    }\n    s",
        "18",
    );
    // The condition is re-read after a `continue`, including a change the body made to
    // it immediately before continuing.
    expect_value(
        "    let mut i = 0;\n    let mut n = 0;\n    while i < 4 {\n        i = i + 1;\n        n = n + 1;\n        continue;\n    }\n    i * 10 + n",
        "44",
    );
    // A condition with a call and checked arithmetic in it, re-evaluated on the path a
    // `continue` takes — the case a jump straight to the body would skip.
    let src = "fn limit() -> Int { 5 }\n\
               fn main() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   let mut n = 0;\n\
               \x20   while i * 1 < limit() {\n\
               \x20       i = i + 1;\n\
               \x20       n = n + 1;\n\
               \x20       continue;\n\
               \x20   }\n\
               \x20   n\n\
               }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "5", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "5", "-O{opt}");
    }
}

#[test]
fn break_and_continue_may_be_conditional() {
    // Conditional break: stops at the first multiple of 7 above 20.
    expect_value(
        "    let mut n = 20;\n    while true {\n        n = n + 1;\n        if n % 7 == 0 { break; }\n    }\n    n",
        "21",
    );
    // Conditional continue with an `else`, so both branches are written out.
    expect_value(
        "    let mut i = 0;\n    let mut odd = 0;\n    while i < 10 {\n        i = i + 1;\n        if i % 2 == 0 { continue; } else { odd = odd + i; }\n    }\n    odd",
        "25",
    );
}

#[test]
fn a_statement_if_reaches_its_continuation_from_whichever_branches_fall_through() {
    // Neither branch transfers: both reach the continuation.
    expect_value(
        "    let mut x = 0;\n    let mut i = 0;\n    while i < 4 {\n        i = i + 1;\n        if i % 2 == 0 { x = x + 10; } else { x = x + 1; }\n        x = x + 100;\n    }\n    x",
        "422",
    );
    // Only the `else` falls through: the `then` breaks, so the continuation runs on
    // exactly the iterations that took the `else`.
    expect_value(
        "    let mut x = 0;\n    let mut i = 0;\n    while i < 9 {\n        i = i + 1;\n        if i == 3 { break; } else { x = x + 1; }\n        x = x + 100;\n    }\n    x",
        "202",
    );
    // Only the `then` falls through.
    expect_value(
        "    let mut x = 0;\n    let mut i = 0;\n    while i < 9 {\n        i = i + 1;\n        if i < 3 { x = x + 1; } else { break; }\n        x = x + 100;\n    }\n    x",
        "202",
    );
    // **Both** branches transfer: there is no continuation at all, so the statement
    // after the `if` can never run — and the checker says so before the backend does.
    expect_value(
        "    let mut i = 0;\n    let mut x = 0;\n    while i < 9 {\n        i = i + 1;\n        if i > 3 { break; } else { continue; }\n    }\n    i * 10 + x",
        "40",
    );
}

#[test]
fn a_statement_if_without_else_always_falls_through() {
    // Whatever the branch does. Even when it always transfers, the condition may be
    // false, so the continuation is reachable and must be emitted.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        if i == 2 { continue; }\n        s = s + i;\n    }\n    s",
        "13",
    );
    // A branch that always breaks, with a condition that is false for a while.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 100 {\n        i = i + 1;\n        if i == 4 { break; }\n        s = s + i;\n    }\n    s",
        "6",
    );
    // And nested: an `if` with no `else` inside an `if` with no `else`.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 8 {\n        i = i + 1;\n        if i > 2 { if i % 2 == 1 { continue; } }\n        s = s + i;\n    }\n    s",
        "21",
    );
}

#[test]
fn an_inner_break_leaves_only_the_inner_loop() {
    // The outer loop must keep going, and the statement after the inner loop must run —
    // a target stack that let the inner loop overwrite the outer's would leave both.
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 4 {\n        let mut j = 0;\n        while j < 10 {\n            if j == 2 { break; }\n            s = s + 1;\n            j = j + 1;\n        }\n        s = s + 100;\n        o = o + 1;\n    }\n    s",
        "408",
    );
    // Reaching the *outer* loop's own control after the inner loop finishes: the outer
    // `continue` must target the outer condition, not the inner one, which by then has
    // been popped.
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 5 {\n        o = o + 1;\n        let mut j = 0;\n        while j < 3 { j = j + 1; }\n        if o % 2 == 0 { continue; }\n        s = s + j;\n    }\n    s",
        "9",
    );
    // An outer `break` reached after an inner loop completed.
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 100 {\n        o = o + 1;\n        let mut j = 0;\n        while j < 2 { j = j + 1; s = s + 1; }\n        if o == 3 { break; }\n    }\n    o * 100 + s",
        "306",
    );
}

#[test]
fn an_inner_continue_reevaluates_the_inner_condition() {
    // If the inner `continue` jumped to the inner body, `j` would never be re-tested and
    // this would not terminate; if it targeted the *outer* loop, `s` would be 0.
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 3 {\n        o = o + 1;\n        let mut j = 0;\n        while j < 5 {\n            j = j + 1;\n            if j % 2 == 0 { continue; }\n            s = s + 1;\n        }\n    }\n    s",
        "9",
    );
}

#[test]
fn a_transfer_in_a_loop_condition_targets_the_enclosing_loop() {
    // The rule taken from the existing implementations rather than assumed: a loop's
    // condition is *not* inside that loop. `check.rs` raises `loop_depth` around the
    // body alone, and `eval.rs` lets a `Flow::Break` raised in a condition leave
    // `while_loop` — so the enclosing loop consumes it.
    //
    // These two numbers are only explicable under that rule, which is why they are the
    // assertion: 102 means the outer loop stopped, and 204 means the outer loop skipped
    // the rest of an iteration and then ran another one.
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 3 {\n        o = o + 1;\n        let mut j = 0;\n        while (if o == 2 { break; } else { j < 2 }) { j = j + 1; s = s + 1; }\n        s = s + 100;\n    }\n    s",
        "102",
    );
    expect_value(
        "    let mut o = 0;\n    let mut s = 0;\n    while o < 3 {\n        o = o + 1;\n        let mut j = 0;\n        while (if o == 2 { continue; } else { j < 2 }) { j = j + 1; s = s + 1; }\n        s = s + 100;\n    }\n    s",
        "204",
    );
}

#[test]
fn control_transfers_out_of_every_value_position_the_frontend_accepts() {
    // The five compositions named in the M1e scope table. Each was confirmed to check
    // and run before any of this was implemented.

    // 1. A `let` initialiser.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        let x = if i == 3 { break; } else { i };\n        s = s + x;\n    }\n    s",
        "3",
    );
    // 2. An assignment right-hand side.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        s = if i == 3 { break; } else { s + i };\n    }\n    s",
        "3",
    );
    // 3. An arithmetic operand.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        s = s + (if i == 3 { break; } else { i });\n    }\n    s",
        "3",
    );
    // 4. A call argument.
    let src = "fn dbl(n: Int) -> Int { n * 2 }\n\
               fn main() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   let mut s = 0;\n\
               \x20   while i < 5 {\n\
               \x20       i = i + 1;\n\
               \x20       s = s + dbl(if i == 3 { break; } else { i });\n\
               \x20   }\n\
               \x20   s\n\
               }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "6", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "6", "-O{opt}");
    }
    // 5. A nested `if`'s condition.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        s = s + if (if i == 3 { break; } else { true }) { 10 } else { 0 };\n    }\n    s",
        "20",
    );
    // Both branches transferring, in a `let` initialiser: the binding is never written
    // and nothing after the statement runs.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        let x = if i == 3 { break; } else { continue; };\n        s = s + x;\n    }\n    s",
        "0",
    );
}

#[test]
fn an_interrupted_expression_keeps_earlier_effects_and_skips_later_ones() {
    // A failure *before* the transfer still happens: `10 / z` is the left operand and
    // divides by zero on the first iteration, so the `break` to its right is never
    // reached.
    expect_runtime_error_at(
        "    let z = 0;\n    let mut i = 0;\n    while i < 3 {\n        i = (10 / z) + (if true { break; } else { 1 });\n    }\n    i",
        "N0401",
        "division by zero",
        "prog.nz:5:17",
    );
    // The same two operands the other way round: the transfer comes first, so the
    // division is **not** evaluated and the program succeeds. Nothing but evaluation
    // order distinguishes these two.
    expect_value(
        "    let z = 0;\n    let mut i = 0;\n    let mut s = 7;\n    while i < 3 {\n        s = (if true { break; } else { 1 }) + (10 / z);\n    }\n    s",
        "7",
    );
    // An interrupted assignment performs no store: `s` keeps the value it had before the
    // iteration that broke.
    expect_value(
        "    let mut i = 0;\n    let mut s = 100;\n    while i < 5 {\n        i = i + 1;\n        s = if i == 2 { break; } else { s + 1 };\n    }\n    s",
        "101",
    );
    // A call with an interrupted argument is never entered. `boom(0)` would divide by
    // zero, and a compiler that called it with whatever was lying around would say so.
    let src = "fn boom(n: Int) -> Int { 10 / n }\n\
               fn main() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   let mut s = 0;\n\
               \x20   while i < 3 {\n\
               \x20       i = i + 1;\n\
               \x20       s = s + boom(if i == 2 { break; } else { i });\n\
               \x20   }\n\
               \x20   s\n\
               }\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "10");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), "10", "-O{opt}");
    }
    // Statements already executed keep their effects; statements after the transfer do
    // not run at all.
    expect_value(
        "    let mut i = 0;\n    let mut s = 0;\n    while i < 5 {\n        i = i + 1;\n        s = s + 1;\n        if i == 2 { break; }\n        s = s + 100;\n    }\n    s",
        "102",
    );
}

#[test]
fn loop_locals_and_shadowing_survive_break_and_continue() {
    // A body-local is re-initialised on every iteration, including the iterations a
    // `continue` cut short.
    expect_value(
        "    let mut i = 0;\n    let mut total = 0;\n    while i < 5 {\n        i = i + 1;\n        let mut acc = 10;\n        if i % 2 == 0 { continue; }\n        acc = acc + i;\n        total = total + acc;\n    }\n    total",
        "39",
    );
    // Shadowing inside a branch of a statement `if`, with the outer binding untouched.
    expect_value(
        "    let mut x = 1;\n    let mut i = 0;\n    while i < 3 {\n        i = i + 1;\n        if i < 3 { let mut x = 100; x = x + 1; continue; }\n        x = x + 10;\n    }\n    x",
        "11",
    );
}

#[test]
fn misplaced_transfers_and_unreachable_code_belong_to_the_checker() {
    for (src, code) in [
        ("fn main() -> Int {\n    break;\n    1\n}\n", "N0312"),
        ("fn main() -> Int {\n    continue;\n    1\n}\n", "N0312"),
        // A `break` in the *outermost* loop's condition has no enclosing loop to affect,
        // which is the checker's rule and the reason the backend pushes targets around
        // the body alone.
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while (if i > 1 { break; } else { true }) { i = i + 1; }\n    i\n}\n",
            "N0312",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { break; i = i + 1; }\n    i\n}\n",
            "N0313",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { if i == 1 { break; } else { continue; } i = i + 1; }\n    i\n}\n",
            "N0313",
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
        assert_eq!(out.status.code(), Some(1), "{code}: {stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a language error was reported as a backend limit: {stderr}"
        );
        assert!(!exe.exists(), "{code}: produced an executable");
    }
}

/// Structural facts about a generated module, read out of the text.
///
/// clang accepting the module is the real check and it runs on every case above; this
/// adds *which* shape was emitted. It exists because the two ways control flow goes wrong
/// here are both things a reader would have to hand-trace otherwise: an instruction
/// appended after a terminator, and a `phi` naming a block that does not branch to it.
///
/// Returns the number of blocks inspected, so a test cannot pass by finding nothing.
fn assert_control_flow_is_well_formed(ir: &str) -> usize {
    let terminator = |l: &str| {
        l.starts_with("br ")
            || l.starts_with("ret ")
            || l == "unreachable"
            || l.starts_with("switch ")
    };

    let mut blocks = 0usize;
    for function in ir.split("\ndefine ").skip(1) {
        let body = function.split("\n}").next().unwrap_or_default();

        // Pass one: the instructions of each block, in order. A function may open with
        // an explicit `entry:` label or fall straight into instructions, so the first
        // block is created by whichever comes first rather than seeded.
        let mut named: Vec<(String, Vec<String>)> = Vec::new();
        for line in body.lines().skip(1) {
            let t = line.split(';').next().unwrap_or_default().trim().to_owned();
            if t.is_empty() {
                continue;
            }
            if let Some(label) = t.strip_suffix(':') {
                if !label.contains(char::is_whitespace) {
                    named.push((label.to_owned(), Vec::new()));
                    continue;
                }
            }
            if named.is_empty() {
                named.push(("entry".to_owned(), Vec::new()));
            }
            if let Some((_, instrs)) = named.last_mut() {
                instrs.push(t);
            }
        }

        for (label, instrs) in &named {
            blocks += 1;
            let ends = instrs.iter().rposition(|l| terminator(l));
            assert_eq!(
                ends,
                Some(instrs.len().saturating_sub(1)),
                "block %{label} does not end in a terminator:\n{instrs:#?}"
            );
            let count = instrs.iter().filter(|l| terminator(l)).count();
            assert_eq!(
                count, 1,
                "block %{label} has {count} terminators, so something was appended after \
                 one:\n{instrs:#?}"
            );
        }

        // Pass two: every `phi` incoming must name a block that really branches here.
        for (label, instrs) in &named {
            for instr in instrs {
                let Some(rest) = instr.split(" = phi ").nth(1) else {
                    continue;
                };
                for incoming in rest.split('[').skip(1) {
                    let inside = incoming.split(']').next().unwrap_or_default();
                    let from = inside
                        .split(',')
                        .nth(1)
                        .unwrap_or_default()
                        .trim()
                        .trim_start_matches('%')
                        .to_owned();
                    let source = named.iter().find(|(n, _)| *n == from);
                    let (_, source) = source.unwrap_or_else(|| {
                        panic!(
                            "phi in %{label} names %{from}, which is not a block in this function"
                        )
                    });
                    assert!(
                        source
                            .iter()
                            .any(|l| terminator(l) && l.contains(&format!("label %{label}"))),
                        "phi in %{label} names %{from}, but %{from} does not branch there: \
                         {source:#?}"
                    );
                }
            }
        }
    }
    blocks
}

#[test]
fn the_emitted_ir_for_nested_control_flow_is_well_formed() {
    // One program carrying every shape M1e adds at once: nested loops, `break` and
    // `continue` at both depths, a statement `if` with and without `else`, a
    // value-position `if` whose branches disagree about transferring, checked arithmetic
    // inside a condition, and a call.
    let src = "fn keep(n: Int) -> Bool { n % 4 != 0 }\n\
               fn main() -> Int {\n\
               \x20   let mut o = 0;\n\
               \x20   let mut total = 0;\n\
               \x20   while o < 6 {\n\
               \x20       o = o + 1;\n\
               \x20       if o == 5 { break; }\n\
               \x20       let mut j = 0;\n\
               \x20       while j * 1 < o {\n\
               \x20           j = j + 1;\n\
               \x20           if keep(j) { total = total + (if j > 2 { continue; } else { j }); } else { break; }\n\
               \x20       }\n\
               \x20       if o % 2 == 0 { continue; } else { total = total + 100; }\n\
               \x20   }\n\
               \x20   total\n\
               }\n";

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "clang rejected the generated IR: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("emitted IR");
    let blocks = assert_control_flow_is_well_formed(&ir);
    assert!(
        blocks > 15,
        "only {blocks} blocks — the shape is too simple to mean much"
    );

    // A `phi` with a single incoming is the value-position `if` whose other branch
    // transferred: the value comes from the branch that actually reaches the join.
    let single = ir
        .lines()
        .filter(|l| l.contains(" = phi "))
        .filter(|l| l.matches('[').count() == 1)
        .count();
    assert_eq!(
        single, 1,
        "expected exactly one single-incoming phi, for the branch whose `else` \
         continues:\n{ir}"
    );

    // And the whole thing produces the same number as the interpreter.
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    let built = Built { dir, exe };
    let ran = run_exe(&built);
    assert_eq!(ran.code, Some(0), "{ran:?}");
    assert_eq!(
        ran.stdout.trim(),
        interp.stdout.trim(),
        "the native program and the interpreter disagree"
    );
}

// ---------------------------------------------------------------------------
// M1f — native `return`
//
// One `ret` per `return`, not a shared exit block: nothing needs cleanup on the way out
// yet.
// ---------------------------------------------------------------------------

#[test]
fn a_body_where_every_path_returns_needs_no_tail() {
    // The shape that has no fall-through at all. `check_function` accepts it by treating
    // `Completion::Diverges` as satisfying the signature, so lowering must produce a
    // body with no result and emission must write **no** terminator of its own — the
    // `ret` is already there. A stray `unreachable` after it would be a second
    // terminator and clang would reject the module.
    let src = "fn sign(n: Int) -> Int {\n\
               \x20   if n < 0 { return 0 - 1; } else { return 1; }\n\
               }\n\
               fn main() -> Int { sign(0 - 5) * 10 + sign(3) }\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "-9");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), "-9", "-O{opt}");
    }

    // A body that is nothing but a `return`, so the entry block itself ends in `ret`.
    expect_value("    return 42;", "42");
    // And one where the last statement returns after other work.
    expect_value(
        "    let mut i = 0;\n    while i < 3 { i = i + 1; }\n    return i * 7;",
        "21",
    );
}

#[test]
fn an_early_return_leaves_the_function_not_the_loop() {
    // `return` inside a loop skips the rest of the function, not just the iteration —
    // the distinction `break` exists for. `first_div(91)` is 7, and 97 is prime so the
    // loop finishes and the tail runs.
    let src = "fn first_div(n: Int) -> Int {\n\
               \x20   let mut d = 2;\n\
               \x20   while d < n {\n\
               \x20       if n % d == 0 { return d; }\n\
               \x20       d = d + 1;\n\
               \x20   }\n\
               \x20   n\n\
               }\n\
               fn main() -> Int { first_div(91) * 100 + first_div(97) }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "797", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "797", "-O{opt}");
    }

    // From inside a *nested* loop, leaving both at once — where `break` would leave one.
    let src = "fn meet(target: Int) -> Int {\n\
               \x20   let mut a = 0;\n\
               \x20   while a < 5 {\n\
               \x20       let mut b = 0;\n\
               \x20       while b < 5 {\n\
               \x20           if a * b == target { return a * 10 + b; }\n\
               \x20           b = b + 1;\n\
               \x20       }\n\
               \x20       a = a + 1;\n\
               \x20   }\n\
               \x20   0 - 1\n\
               }\n\
               fn main() -> Int { meet(6) * 1000 + meet(99) }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "22999", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "22999", "-O{opt}");
    }
}

#[test]
fn a_return_in_main_is_still_printed_only_by_the_wrapper() {
    // `main` is an ordinary `@nz.f<N>`; the platform wrapper prints what it returns. A
    // `return` from a *helper* must therefore print nothing of its own.
    expect_program(
        "fn shout() -> Int { return 5; }\n\
         fn main() -> Int {\n\
         \x20   let mut i = 0;\n\
         \x20   while i < 10 { if i == 4 { return i * 11 + shout(); } i = i + 1; }\n\
         \x20   0\n\
         }\n",
        "49",
    );
}

#[test]
fn return_reaches_every_value_position_the_frontend_accepts() {
    // The five positions M1e established, now with `return` rather than `break`.
    // 1. A `let` initialiser.
    let src = "fn f(n: Int) -> Int {\n\
               \x20   let x = if n > 10 { return 999; } else { n * 2 };\n\
               \x20   x + 1\n\
               }\n\
               fn main() -> Int { f(3) * 1000 + f(20) }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "7999", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "7999", "-O{opt}");
    }

    // 2. An assignment right-hand side, 3. an arithmetic operand, 4. a call argument,
    // 5. a nested `if` condition — each returning early from a helper.
    let src = "fn twice(n: Int) -> Int { n * 2 }\n\
               fn rhs(n: Int) -> Int { let mut s = 1; s = if n > 0 { return 7; } else { 2 }; s }\n\
               fn operand(n: Int) -> Int { 100 + (if n > 0 { return 8; } else { 3 }) }\n\
               fn argument(n: Int) -> Int { twice(if n > 0 { return 9; } else { 4 }) }\n\
               fn condition(n: Int) -> Int { if (if n > 0 { return 10; } else { true }) { 5 } else { 6 } }\n\
               fn main() -> Int {\n\
               \x20   rhs(1) * 10000 + operand(1) * 1000 + argument(1) * 100 + condition(1) * 10\n\
               \x20       + rhs(0 - 1) + operand(0 - 1) + argument(0 - 1) + condition(0 - 1)\n\
               }\n";
    // Positive arguments take the `return`: 7, 8, 9, 10. Negative ones fall through:
    // 2 + 103 + 8 + 5 = 118.
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    let expected = interp.stdout.trim().to_owned();
    assert_eq!(
        expected, "79118",
        "the expectation disagrees with the interpreter"
    );
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), expected, "-O{opt}");
    }
}

#[test]
fn a_transferring_operand_beats_the_return_it_sits_in() {
    // `return if c { break; } else { … }` — the `break` wins and no `ret` is written,
    // which is what the interpreter's `take!` does with the same program. The loop is
    // left, the function's tail runs, and the returned value is never produced.
    let src = "fn h() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   while i < 5 {\n\
               \x20       i = i + 1;\n\
               \x20       return if i == 3 { break; } else { i * 100 };\n\
               \x20   }\n\
               \x20   i\n\
               }\n\
               fn main() -> Int { h() }\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    // The first iteration returns 100 outright, so the `break` is never reached — the
    // point is that the compiled program agrees about which one happens.
    assert_eq!(interp.stdout.trim(), "100");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "100", "-O{opt}");
    }

    // And the shape where the `break` does win, because the transfer is guarded.
    let src = "fn h() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   while i < 5 {\n\
               \x20       i = i + 1;\n\
               \x20       return if i < 3 { continue; } else { i * 100 };\n\
               \x20   }\n\
               \x20   i\n\
               }\n\
               fn main() -> Int { h() }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "300", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "300", "-O{opt}");
    }
}

#[test]
fn an_early_return_skips_the_operands_and_stores_after_it() {
    // A store that never happens: the assignment's right-hand side returns first.
    expect_value(
        "    let mut kept = 11;\n    let mut i = 0;\n    while i < 3 {\n        i = i + 1;\n        if i == 2 { return kept; }\n        kept = kept + 100;\n    }\n    0",
        "111",
    );
    // A failure *after* a `return` never fires, and the same failure before it does.
    let src = "fn safe(n: Int) -> Int { if n > 0 { return 3; } 10 / n }\n\
               fn main() -> Int { safe(1) }\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "3");
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), "3", "-O{opt}");
    }
    // The same helper with a zero argument reaches the division and fails there, at the
    // division's own position rather than the `return`'s.
    expect_runtime_error_at(
        "    let z = 0;\n    z / z",
        "N0401",
        "division by zero",
        "prog.nz:3:7",
    );
}

#[test]
fn return_carries_bool_as_well_as_int() {
    let src = "fn even(n: Int) -> Bool {\n\
               \x20   if n % 2 == 0 { return true; }\n\
               \x20   return false;\n\
               }\n\
               fn main() -> Int { if even(4) { if even(7) { 0 } else { 42 } } else { 0 } }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "42", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "42", "-O{opt}");
    }
}

#[test]
fn a_callee_returning_early_does_not_disturb_its_caller() {
    // The caller's locals and loop keep going across a callee that left early.
    let src = "fn clamp(n: Int) -> Int {\n\
               \x20   if n > 10 { return 10; }\n\
               \x20   if n < 0 { return 0; }\n\
               \x20   n\n\
               }\n\
               fn main() -> Int {\n\
               \x20   let mut i = 0 - 3;\n\
               \x20   let mut s = 0;\n\
               \x20   while i < 14 { s = s + clamp(i); i = i + 1; }\n\
               \x20   s\n\
               }\n";
    let interp = interpret(src);
    assert_eq!(interp.stdout.trim(), "85", "{}", interp.stderr);
    for opt in [0u8, 2] {
        let built = build_at(src, opt).unwrap_or_else(|e| panic!("-O{opt}: {e:?}"));
        assert_eq!(run_exe(&built).stdout.trim(), "85", "-O{opt}");
    }
}

#[test]
fn the_emitted_ir_for_a_return_heavy_function_is_well_formed() {
    // The structural check, aimed at the shape M1f adds: several `ret`s in one function,
    // one of them ending the entry block's successor, and a body with no tail at all.
    // Exactly one terminator per block is what catches a stray `unreachable` written
    // after a `ret`.
    let src = "fn classify(n: Int) -> Int {\n\
               \x20   if n < 0 { return 0 - 1; }\n\
               \x20   let mut i = 0;\n\
               \x20   while i < n {\n\
               \x20       if i * i == n { return i; }\n\
               \x20       if i > 100 { break; }\n\
               \x20       i = i + 1;\n\
               \x20   }\n\
               \x20   return 0;\n\
               }\n\
               fn main() -> Int { classify(49) * 100 + classify(0 - 2) + classify(5) }\n";

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "clang rejected the generated IR: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("emitted IR");
    let blocks = assert_control_flow_is_well_formed(&ir);
    assert!(blocks > 10, "only {blocks} blocks");

    // Three `ret`s in `classify` plus one in `main` plus the wrapper's `ret i32`, and
    // now one failure exit each in `classify` and `main` plus `@nz.fail`'s `ret void`.
    // A failure leaves a function by an ordinary `ret` — the emitted code has no other
    // way out of one — so a function that can fail has one more return site than it has
    // `return`s. `@nz.report` is not counted: it ends in `unreachable` after `@exit`.
    let rets = ir.lines().filter(|l| l.trim().starts_with("ret ")).count();
    assert_eq!(rets, 8, "expected one `ret` per return site:\n{ir}");
    // The specific regression: a terminator written *after* a `ret`. Checked as that
    // adjacency rather than by looking for `unreachable` anywhere — the `@nz.fail`
    // helper ends in one legitimately, and an assertion that cannot tell the two apart
    // fails on correct output.
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    for pair in lines.windows(2) {
        let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        assert!(
            !(first.starts_with("ret ") && *second == "unreachable"),
            "a terminator was written after the body's own `ret`:\n{ir}"
        );
    }

    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    let built = Built { dir, exe };
    let ran = run_exe(&built);
    assert_eq!(ran.stdout.trim(), interp.stdout.trim());
    assert_eq!(ran.stdout.trim(), "699");
}

#[test]
fn a_wrongly_typed_return_belongs_to_the_checker() {
    for (src, code) in [
        (
            "fn f() -> Int { return true; }\nfn main() -> Int { f() }\n",
            "N0300",
        ),
        (
            "fn f() -> Bool { return 1; }\nfn main() -> Int { if f() { 1 } else { 0 } }\n",
            "N0300",
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
        assert_eq!(out.status.code(), Some(1), "{code}: {stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a language error was reported as a backend limit: {stderr}"
        );
        assert!(!exe.exists());
    }
}

// ---------------------------------------------------------------------------
// B1 — `Str` as bytes, natively
//
// `Str` is `{ pointer, byte length }` by value. Slices share storage; concatenation and
// `int_to_str` allocate from a region that is never freed. See
// `docs/bootstrap.md` §1, *the memory model*.
// ---------------------------------------------------------------------------

/// A program returning `Int`, checked against an expectation and the interpreter, at
/// both optimisation levels. Like `expect_value` but for a whole multi-function program.
fn expect_whole(src: &str, expected: &str) {
    let interp = interpret(src);
    assert_eq!(
        interp.code,
        Some(0),
        "the interpreter rejected it: {}",
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
    }
}

#[test]
fn str_len_and_str_byte_are_measured_in_bytes() {
    expect_value(
        "    str_len(\"hello\") + str_len(\"\") + str_len(\"é\")",
        "7",
    );
    // 'a' is 97; the first byte of "é" is 0xC3 = 195, **zero**-extended. A sign-extended
    // byte would make every value above 127 negative.
    expect_value(
        "    str_byte(\"abc\", 0) * 1000 + str_byte(\"é\", 0)",
        "97195",
    );
    // v0.1 string literals have **no escapes**, so a byte that cannot be typed is
    // built with `str_from_byte`. This is also the only way to test the specification's
    // "an embedded NUL is an ordinary byte" clause: the string below is three bytes
    // long and its middle byte is zero.
    expect_value(
        "    let s = str_concat(str_concat(\"a\", str_from_byte(0)), \"b\");\n    str_len(s) * 1000 + str_byte(s, 1) * 10 + str_byte(s, 2) - 97",
        "3001",
    );
    expect_value("    str_byte(str_from_byte(255), 0)", "255");
}

#[test]
fn a_slice_shares_storage_and_may_split_a_character() {
    expect_value(
        "    let s = str_slice(\"hello world\", 6, 11);\n    str_len(s) * 1000 + str_byte(s, 0)",
        "5119",
    );
    // The case the interpreter's representation could not hold until `Value::Str` became
    // bytes: half of a two-byte character is a one-byte `Str` whose byte is 195.
    expect_value("    str_byte(str_slice(\"é\", 0, 1), 0)", "195");
    // Empty slices at both ends, and a slice of the whole string.
    expect_value(
        "    str_len(str_slice(\"abc\", 0, 0)) + str_len(str_slice(\"abc\", 3, 3)) + str_len(str_slice(\"abc\", 0, 3))",
        "3",
    );
}

#[test]
fn concatenation_and_equality_compare_bytes_not_buffers() {
    // Two strings built different ways compare equal, which is the property a buffer
    // comparison would get wrong.
    expect_value(
        "    let joined = str_concat(str_slice(\"xabcx\", 1, 3), \"cd\");\n    if joined == \"abcd\" { 1 } else { 0 }",
        "1",
    );
    // Empty strings compare equal, which needs a valid pointer to hand to `memcmp`.
    expect_value("    if \"\" == \"\" { 1 } else { 0 }", "1");
    expect_value(
        "    if str_concat(\"\", \"\") == \"\" { 1 } else { 0 }",
        "1",
    );
    // Different lengths are unequal without reading past either buffer.
    expect_value("    if \"ab\" == \"abc\" { 1 } else { 0 }", "0");
    expect_value("    if \"ab\" != \"abc\" { 1 } else { 0 }", "1");
    // No case folding and no normalisation.
    expect_value("    if \"A\" == \"a\" { 1 } else { 0 }", "0");
}

#[test]
fn int_to_str_covers_the_whole_int_range() {
    expect_value("    str_len(int_to_str(0))", "1");
    expect_value("    if int_to_str(0) == \"0\" { 1 } else { 0 }", "1");
    expect_value("    if int_to_str(0 - 42) == \"-42\" { 1 } else { 0 }", "1");
    // 19 and 20 bytes respectively — the widest results, and what the buffer is sized
    // for.
    expect_value(
        "    if int_to_str(9223372036854775807) == \"9223372036854775807\" { 1 } else { 0 }",
        "1",
    );
    expect_value(
        "    if int_to_str(-9223372036854775808) == \"-9223372036854775808\" { 1 } else { 0 }",
        "1",
    );
    expect_value("    str_len(int_to_str(-9223372036854775808))", "20");
}

#[test]
fn str_travels_through_parameters_returns_locals_loops_and_joins() {
    // The shape a tokeniser is made of, and the shape a `phi` of `{ptr, i64}` is made of.
    expect_whole(
        "fn count(s: Str, want: Int) -> Int {\n\
         \x20   let mut i = 0;\n\
         \x20   let mut n = 0;\n\
         \x20   while i < str_len(s) {\n\
         \x20       if str_byte(s, i) == want { n = n + 1; }\n\
         \x20       i = i + 1;\n\
         \x20   }\n\
         \x20   n\n\
         }\n\
         fn repeat(s: Str, times: Int) -> Str {\n\
         \x20   let mut out = \"\";\n\
         \x20   let mut i = 0;\n\
         \x20   while i < times { out = str_concat(out, s); i = i + 1; }\n\
         \x20   out\n\
         }\n\
         fn pick(c: Bool) -> Str { if c { \"yes\" } else { \"no\" } }\n\
         fn main() -> Int {\n\
         \x20   count(repeat(\"ab\", 5), 97) * 100 + str_len(pick(true)) * 10 + str_len(pick(false))\n\
         }\n",
        "532",
    );
    // A `Str` returned early, and one returned from a body with no tail.
    expect_whole(
        "fn name_of(n: Int) -> Str {\n\
         \x20   if n < 0 { return \"negative\"; }\n\
         \x20   return \"other\";\n\
         }\n\
         fn main() -> Int { str_len(name_of(0 - 1)) * 10 + str_len(name_of(1)) }\n",
        "85",
    );
}

#[test]
fn every_string_bound_fails_at_its_own_position() {
    expect_runtime_error_at(
        "    str_byte(\"abc\", 3)",
        "N0405",
        "this byte index is outside the `Str`",
        "prog.nz:2:5",
    );
    expect_runtime_error_at(
        "    str_byte(\"abc\", 0 - 1)",
        "N0405",
        "this byte index is outside the `Str`",
        "prog.nz:2:5",
    );
    expect_runtime_error_at(
        "    str_len(str_from_byte(256))",
        "N0405",
        "this is not a byte value",
        "prog.nz:2:13",
    );
    expect_runtime_error_at(
        "    str_len(str_slice(\"abc\", 2, 1))",
        "N0405",
        "this slice is not inside the `Str`",
        "prog.nz:2:13",
    );
    expect_runtime_error_at(
        "    str_len(str_slice(\"abc\", 0, 4))",
        "N0405",
        "this slice is not inside the `Str`",
        "prog.nz:2:13",
    );
    // A bound reached only on a later loop iteration, so the guard is inside the loop.
    expect_runtime_error_at(
        "    let s = \"abc\";\n    let mut i = 0;\n    let mut n = 0;\n    while i < 10 { n = n + str_byte(s, i); i = i + 1; }\n    n",
        "N0405",
        "this byte index is outside the `Str`",
        "prog.nz:5:28",
    );
}

#[test]
fn every_front_end_builtin_is_lowered_or_refused_by_name() {
    // `nazm-lir` keeps its own table of the built-ins it can lower, because it may not
    // depend on `nazm-core` and because backend coverage is its own statement. The risk
    // that creates is drift: a built-in the front end gains and the backend never hears
    // about. This test is the guard — it sees both crates, and requires every front-end
    // built-in to either compile or be refused **by its own name**, never to fail some
    // other way.
    for i in nazm_core::Intrinsic::ALL {
        let args: Vec<&str> = i
            .params()
            .iter()
            .map(|t| match t {
                nazm_core::Type::Int => "0",
                nazm_core::Type::Bool => "true",
                nazm_core::Type::Str => "\"ab\"",
                nazm_core::Type::Ints => "ints_new()",
                nazm_core::Type::Strs => "strs_new()",
                nazm_core::Type::Chan => "chan_new(1)",
            })
            .collect();
        // Every built-in currently returns `Int` or `Str`; wrap the `Str` ones so `main`
        // still returns `Int`.
        let call = format!("{}({})", i.name(), args.join(", "));
        // Wrapped so `main` still returns `Int`, whatever the built-in produces. The
        // program is only built, never run, so an index that would be out of bounds at
        // run time is fine here — this test is about lowering coverage.
        let body = match i.ret() {
            nazm_core::Type::Int => call,
            nazm_core::Type::Str => format!("str_len({call})"),
            nazm_core::Type::Bool => format!("if {call} {{ 1 }} else {{ 0 }}"),
            nazm_core::Type::Ints => format!("ints_len({call})"),
            nazm_core::Type::Strs => format!("strs_len({call})"),
            // No `chan_len`, and none is wanted: a channel's length is not a thing a
            // program may observe without racing. Sending into it is the shortest way
            // to reach an `Int` from one.
            nazm_core::Type::Chan => format!("if chan_send({call}, 0) {{ 1 }} else {{ 0 }}"),
        };
        let src = format!("fn main() -> Int {{ {body} }}\n");

        let (dir, path) = write_program(&src);
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(dir.path().join("prog"))
            .output()
            .expect("run");
        if out.status.success() {
            continue;
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("the built-in `{}`", i.name())),
            "`{}` neither compiles nor is refused by name — the backend's built-in table \
             has drifted from the front end's: {stderr}",
            i.name()
        );
    }
}

#[test]
fn the_emitted_ir_for_strings_is_well_formed() {
    let src = "fn describe(s: Str) -> Str {\n\
               \x20   if str_len(s) == 0 { return \"empty\"; }\n\
               \x20   str_concat(\"len=\", int_to_str(str_len(s)))\n\
               }\n\
               fn main() -> Int {\n\
               \x20   let a = describe(\"\");\n\
               \x20   let b = describe(str_slice(\"hello\", 1, 4));\n\
               \x20   if a == \"empty\" { str_len(b) } else { 0 }\n\
               }\n";

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "clang rejected the generated IR: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("emitted IR");
    assert_control_flow_is_well_formed(&ir);
    // A `Str` is an aggregate, so equality is a branch and a `phi`, never an `icmp` on
    // the pair — comparing buffers instead of bytes is the defect that would hide here.
    assert!(
        !ir.contains("icmp eq { ptr, i64 }"),
        "two strings were compared as pairs rather than as bytes:\n{ir}"
    );
    assert!(
        ir.contains("@memcmp"),
        "no byte comparison was emitted:\n{ir}"
    );
    // Every literal gets a constant pair, so nothing is assembled from `undef` for a
    // value known at compile time.
    assert!(
        ir.contains(".v = private unnamed_addr constant { ptr, i64 }"),
        "{ir}"
    );

    let interp = interpret(src);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    let built = Built { dir, exe };
    assert_eq!(run_exe(&built).stdout.trim(), interp.stdout.trim());
    assert_eq!(run_exe(&built).stdout.trim(), "5");
}

// ---------------------------------------------------------------------------
// B2 — sequences, natively
//
// A handle is a pointer to a `{ len, cap, data }` header, so growth rewrites the header
// in place and every holder sees it. See `docs/spec.md`, *Sequences are shared, mutable,
// and never freed*.
// ---------------------------------------------------------------------------

#[test]
fn a_native_sequence_is_shared_storage() {
    expect_whole(
        "fn fill(a: Ints, n: Int) -> Int {\n\
         \x20   let mut i = 0;\n\
         \x20   while i < n { ints_push(a, i * i); i = i + 1; }\n\
         \x20   ints_len(a)\n\
         }\n\
         fn total(a: Ints) -> Int {\n\
         \x20   let mut i = 0;\n\
         \x20   let mut s = 0;\n\
         \x20   while i < ints_len(a) { s = s + ints_get(a, i); i = i + 1; }\n\
         \x20   s\n\
         }\n\
         fn main() -> Int {\n\
         \x20   let a = ints_new();\n\
         \x20   let b = a;\n\
         \x20   fill(a, 5);\n\
         \x20   ints_push(b, 100);\n\
         \x20   ints_set(a, 0, 7);\n\
         \x20   total(b) * 100 + ints_len(a)\n\
         }\n",
        "13706",
    );
}

#[test]
fn growth_survives_many_pushes_and_keeps_every_element() {
    // Crosses the doubling boundary repeatedly, so a `realloc` that lost data or a
    // header that was not rewritten would show up as a wrong sum rather than a crash.
    expect_whole(
        "fn main() -> Int {\n\
         \x20   let a = ints_new();\n\
         \x20   let mut i = 0;\n\
         \x20   while i < 1000 { ints_push(a, i); i = i + 1; }\n\
         \x20   let mut s = 0;\n\
         \x20   let mut j = 0;\n\
         \x20   while j < ints_len(a) { s = s + ints_get(a, j); j = j + 1; }\n\
         \x20   s * 10 + ints_len(a) / 1000\n\
         }\n",
        "4995001",
    );
}

#[test]
fn strs_sequences_carry_strings_by_value() {
    expect_whole(
        "fn main() -> Int {\n\
         \x20   let words = strs_new();\n\
         \x20   strs_push(words, \"alpha\");\n\
         \x20   strs_push(words, str_concat(\"be\", \"ta\"));\n\
         \x20   let first = strs_get(words, 0);\n\
         \x20   strs_set(words, 0, \"x\");\n\
         \x20   let mut chars = 0;\n\
         \x20   let mut i = 0;\n\
         \x20   while i < strs_len(words) { chars = chars + str_len(strs_get(words, i)); i = i + 1; }\n\
         \x20   str_len(first) * 100 + chars\n\
         }\n",
        "505",
    );
}

#[test]
fn every_sequence_bound_fails_at_its_own_position() {
    expect_runtime_error_at(
        "    let a = ints_new();\n    ints_get(a, 0)",
        "N0405",
        "this index is outside the sequence",
        "prog.nz:3:5",
    );
    expect_runtime_error_at(
        "    let a = ints_new();\n    ints_push(a, 1);\n    ints_set(a, 2, 5)",
        "N0405",
        "this index is outside the sequence",
        "prog.nz:4:5",
    );
    expect_runtime_error_at(
        "    let a = strs_new();\n    str_len(strs_get(a, 0 - 1))",
        "N0405",
        "this index is outside the sequence",
        "prog.nz:3:13",
    );
}

#[test]
fn a_program_without_sequences_carries_none_of_their_runtime() {
    // The helpers are emitted only where they are reached. Dead scaffolding in every
    // program would be the easy mistake here.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, "fn main() -> Int { 1 + 1 }\n").expect("write");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("IR");
    assert!(
        !ir.contains("@nz.arr_new"),
        "unused sequence runtime emitted:\n{ir}"
    );
    assert!(
        !ir.contains("@realloc"),
        "unused declaration emitted:\n{ir}"
    );

    // And a program that does use one carries it, with well-formed control flow.
    let path2 = dir.path().join("seq.nz");
    std::fs::write(
        &path2,
        "fn main() -> Int { let a = ints_new(); ints_push(a, 5); ints_get(a, 0) }\n",
    )
    .expect("write");
    let out = Command::new(NAZM)
        .args(["build", path2.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(dir.path().join("seq"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir = std::fs::read_to_string(dir.path().join("seq.ll")).expect("IR");
    assert!(ir.contains("@nz.arr_new"), "{ir}");
    assert!(ir.contains("@nz.arr_grow"), "{ir}");
    assert_control_flow_is_well_formed(&ir);
}

#[test]
fn a_call_may_be_a_statement_but_a_bare_block_may_not() {
    expect_value(
        "    let a = ints_new();\n    ints_push(a, 1);\n    ints_push(a, 2);\n    ints_len(a)",
        "2",
    );
    expect_value("    1 + 1;\n    3", "3");

    let (dir, path) = write_program("fn main() -> Int {\n    { let y = 2; };\n    3\n}\n");
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("a block used as an expression"), "{stderr}");
}

// ---------------------------------------------------------------------------
// B3 — arguments, files, output, exit
//
// Authority is ambient and the specification says so. What is tested here is that the
// two implementations agree, not that anything is confined.
// ---------------------------------------------------------------------------

/// Build a program, run it with arguments, and compare everything with the interpreter.
fn expect_same_run(src: &str, args: &[&str], stdout: &str, stderr: &str, code: i32) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");

    let mut interp_cmd = Command::new(NAZM);
    interp_cmd
        .args(["run", path.to_str().unwrap()])
        .arg("--")
        .args(args);
    let interp = run_bounded(&mut interp_cmd, DEADLINE).expect("the interpreter hung");
    assert_eq!(interp.code, Some(code), "interpreter: {interp:?}");
    assert_eq!(interp.stdout, stdout, "interpreter stdout");
    assert!(
        interp.stderr.contains(stderr),
        "interpreter stderr {:?} lacks {stderr:?}",
        interp.stderr
    );

    for opt in [0u8, 2] {
        let exe = dir.path().join(format!("prog{opt}"));
        let built = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .arg("--opt-level")
            .arg(opt.to_string())
            .output()
            .expect("build");
        assert!(
            built.status.success(),
            "-O{opt}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let mut cmd = common::bounded_exe(&exe);
        cmd.args(args);
        let ran = run_bounded(&mut cmd, DEADLINE).expect("the compiled program hung");
        assert_eq!(ran.code, Some(code), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout, stdout, "-O{opt} stdout");
        assert!(
            ran.stderr.contains(stderr),
            "-O{opt} stderr {:?} lacks {stderr:?}",
            ran.stderr
        );
    }
}

#[test]
fn arguments_exclude_the_program_name_in_both_implementations() {
    // The reason they are excluded: a compiled `argv[0]` is a path and an interpreted
    // program has no equivalent, so including it would make one program mean two
    // things. Excluded, the two agree exactly — which is what this asserts.
    let src = "fn main() -> Int {\n\
               \x20   let mut i = 0;\n\
               \x20   let mut total = 0;\n\
               \x20   while i < args_count() {\n\
               \x20       print(arg(i));\n\
               \x20       print(str_from_byte(10));\n\
               \x20       total = total + str_len(arg(i));\n\
               \x20       i = i + 1;\n\
               \x20   }\n\
               \x20   total\n\
               }\n";
    // `main`'s value is printed by the platform wrapper; the exit status is 0. Those
    // are different things and the helper keeps them apart.
    expect_same_run(src, &["alpha", "beta"], "alpha\nbeta\n9\n", "", 0);
    expect_same_run(src, &[], "0\n", "", 0);
}

#[test]
fn an_argument_past_the_end_fails_at_its_own_position() {
    let src = "fn main() -> Int { str_len(arg(0)) }\n";
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).expect("write");
    let interp = run_bounded(
        Command::new(NAZM).args(["run", path.to_str().unwrap()]),
        DEADLINE,
    )
    .expect("hung");
    assert_eq!(interp.code, Some(2), "{interp:?}");
    assert!(interp.stderr.contains("N0405"), "{}", interp.stderr);
    assert!(
        interp
            .stderr
            .contains("this index is outside the arguments"),
        "{}",
        interp.stderr
    );

    let exe = dir.path().join("prog");
    assert!(Command::new(NAZM)
        .args(["build", path.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("build")
        .status
        .success());
    let ran = run_bounded(&mut common::bounded_exe(&exe), DEADLINE).expect("hung");
    assert_eq!(ran.code, Some(2), "{ran:?}");
    assert!(ran.stderr.contains("N0405"), "{}", ran.stderr);
    assert!(
        ran.stderr.contains("this index is outside the arguments"),
        "{}",
        ran.stderr
    );
}

#[test]
fn a_file_round_trips_through_both_implementations() {
    // Written by one run and read back by the same run, with the path supplied as an
    // argument so the program has no compiled-in knowledge of where it is.
    let src = "fn main() -> Int {\n\
               \x20   let path = arg(0);\n\
               \x20   let payload = str_concat(\"bytes: \", int_to_str(1234));\n\
               \x20   write_file(path, payload);\n\
               \x20   let back = read_file(path);\n\
               \x20   if back == payload { str_len(back) } else { 0 - 1 }\n\
               }\n";
    let dir = tempfile::tempdir().expect("tempdir");
    for (i, _) in [0u8, 1, 2].iter().enumerate() {
        let target = dir.path().join(format!("out{i}.txt"));
        let _ = std::fs::remove_file(&target);
    }
    expect_same_run(
        src,
        &[dir.path().join("roundtrip.txt").to_str().unwrap()],
        "11\n",
        "",
        0,
    );
    // The file really is on disk with the bytes the program wrote.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("roundtrip.txt")).expect("the file"),
        "bytes: 1234"
    );
}

#[test]
fn a_missing_file_is_reported_and_a_present_one_is_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let there = dir.path().join("present.txt");
    std::fs::write(&there, "x").expect("write");
    let src = "fn main() -> Int {\n\
               \x20   if file_exists(arg(0)) { 1 } else { 0 }\n\
               }\n";
    expect_same_run(src, &[there.to_str().unwrap()], "1\n", "", 0);
    expect_same_run(
        src,
        &[dir.path().join("absent.txt").to_str().unwrap()],
        "0\n",
        "",
        0,
    );

    // Reading one that is not there stops the program, in both.
    let failing = "fn main() -> Int { str_len(read_file(arg(0))) }\n";
    expect_same_run(
        failing,
        &[dir.path().join("absent.txt").to_str().unwrap()],
        "",
        "this file could not be read",
        2,
    );
}

#[test]
fn exit_with_ends_the_program_and_nothing_after_it_runs() {
    // Also the only built-in that transfers control: everything after it is unreachable,
    // and `main`'s value is never printed because `main` never returns.
    let src = "fn main() -> Int {\n\
               \x20   eprint(\"before\");\n\
               \x20   exit_with(7);\n\
               \x20   print(\"after\");\n\
               \x20   0\n\
               }\n";
    // Nothing on standard output at all: `main` never returns, so the wrapper never
    // prints, and `print("after")` is past a terminator.
    expect_same_run(src, &[], "", "before", 7);
}

#[test]
fn print_and_eprint_go_to_different_descriptors_and_add_nothing() {
    // No trailing newline is added by either — `str_from_byte(10)` is how a program
    // asks for one, and a compiler emitting IR depends on that being exact.
    let src = "fn main() -> Int {\n\
               \x20   print(\"out\");\n\
               \x20   eprint(\"err\");\n\
               \x20   print(str_from_byte(10));\n\
               \x20   0\n\
               }\n";
    expect_same_run(src, &[], "out\n0\n", "err", 0);
}

#[test]
fn a_program_without_io_carries_none_of_its_runtime() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, "fn main() -> Int { 1 + 1 }\n").expect("write");
    assert!(Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run")
        .status
        .success());
    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("IR");
    for absent in ["@nz.read_file", "@nz.cstr", "@fopen", "@nz.argv"] {
        assert!(!ir.contains(absent), "unused {absent} emitted:\n{ir}");
    }
}

// ---------------------------------------------------------------------------
// B4 — modules
// ---------------------------------------------------------------------------

/// Write a tree of files and return its root.
fn write_tree(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, text).expect("write");
    }
    let root = dir.path().join(files.first().expect("a root file").0);
    (dir, root)
}

fn run_root(root: &Path) -> Ran {
    run_bounded(
        Command::new(NAZM).args(["run", root.to_str().unwrap()]),
        DEADLINE,
    )
    .expect("the interpreter hung")
}

fn build_root(dir: &Path, root: &Path) -> Result<PathBuf, Ran> {
    let exe = dir.join("prog");
    let out = run_bounded(
        Command::new(NAZM)
            .args(["build", root.to_str().unwrap()])
            .arg("-o")
            .arg(&exe),
        DEADLINE,
    )
    .expect("the build hung");
    if out.code == Some(0) {
        Ok(exe)
    } else {
        Err(out)
    }
}

#[test]
fn a_program_can_span_several_files_and_a_diamond_loads_once() {
    // `util.nz` is reached twice — directly and through `sub/deep.nz`. Loading it twice
    // would make every function in it a duplicate definition.
    let (dir, root) = write_tree(&[
        (
            "main.nz",
            "use \"util.nz\";\nuse \"sub/deep.nz\";\n\nfn main() -> Int { quad(5) * 100 + str_len(greet()) + twice(1) }\n",
        ),
        (
            "util.nz",
            "fn twice(n: Int) -> Int { n * 2 }\nfn greet() -> Str { \"hello\" }\n",
        ),
        (
            "sub/deep.nz",
            "use \"../util.nz\";\nfn quad(n: Int) -> Int { twice(twice(n)) }\n",
        ),
    ]);

    let interp = run_root(&root);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "2007");

    let exe = build_root(dir.path(), &root).expect("build");
    let ran = run_bounded(&mut common::bounded_exe(&exe), DEADLINE).expect("hung");
    assert_eq!(ran.code, Some(0), "{ran:?}");
    assert_eq!(ran.stdout.trim(), "2007", "the compiled program disagrees");
}

#[test]
fn an_import_cycle_terminates_and_is_not_an_error() {
    // Nothing depends on load order — signatures are collected before any body — so a
    // cycle has no meaning to get wrong, and refusing it would refuse a working program.
    let (dir, root) = write_tree(&[
        ("a.nz", "use \"b.nz\";\nfn main() -> Int { from_b() + 1 }\n"),
        ("b.nz", "use \"a.nz\";\nfn from_b() -> Int { 41 }\n"),
    ]);
    let interp = run_root(&root);
    assert_eq!(interp.code, Some(0), "{}", interp.stderr);
    assert_eq!(interp.stdout.trim(), "42");
    let exe = build_root(dir.path(), &root).expect("build");
    assert_eq!(
        run_bounded(&mut common::bounded_exe(&exe), DEADLINE)
            .expect("hung")
            .stdout
            .trim(),
        "42"
    );
}

#[test]
fn a_diagnostic_in_an_imported_file_names_that_file_and_its_own_line() {
    // The property the merged buffer exists to keep: a reader opens the file the message
    // names and finds the line it gives, not a line number into a concatenation.
    let (_dir, root) = write_tree(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn main() -> Int { helper() }\n",
        ),
        (
            "lib.nz",
            "fn unused() -> Int { 1 }\nfn helper() -> Int { \"oops\" }\n",
        ),
    ]);
    let out = Command::new(NAZM)
        .args(["check", root.to_str().unwrap()])
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("N0300"), "{stderr}");
    assert!(
        stderr.contains("lib.nz:2:22"),
        "wrong file or line: {stderr}"
    );
}

#[test]
fn a_name_defined_in_two_files_is_reported_with_both() {
    let (_dir, root) = write_tree(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn helper() -> Int { 2 }\nfn main() -> Int { helper() }\n",
        ),
        ("lib.nz", "fn helper() -> Int { 1 }\n"),
    ]);
    let out = Command::new(NAZM)
        .args(["check", root.to_str().unwrap()])
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("N0203"), "{stderr}");
    // Both files named: a line number with no file is unusable across a program.
    assert!(stderr.contains("lib.nz:1:4"), "{stderr}");
    assert!(stderr.contains("main.nz:2:4"), "{stderr}");
}

#[test]
fn a_missing_import_points_at_the_path_that_named_it() {
    let (_dir, root) = write_tree(&[("main.nz", "use \"nope.nz\";\nfn main() -> Int { 1 }\n")]);
    let out = Command::new(NAZM)
        .args(["check", root.to_str().unwrap()])
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("N0205"), "{stderr}");
    assert!(stderr.contains("main.nz:1:5"), "{stderr}");
    assert!(stderr.contains("cannot find `nope.nz`"), "{stderr}");
}

#[test]
fn imports_resolve_relative_to_the_importing_file_not_the_working_directory() {
    // Run from a directory that contains a *different* `util.nz`, and check the program
    // still gets its own. A build that depends on where it was started is not
    // reproducible, which is the whole point of the rule.
    let (dir, root) = write_tree(&[
        (
            "app/main.nz",
            "use \"util.nz\";\nfn main() -> Int { answer() }\n",
        ),
        ("app/util.nz", "fn answer() -> Int { 42 }\n"),
        ("util.nz", "fn answer() -> Int { 0 - 1 }\n"),
    ]);
    let out = Command::new(NAZM)
        .args(["run", root.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

/// `examples/worker-pool.nz` is the spec's concurrency example, and it terminates with 90.
///
/// It is here rather than in the interpreted-examples list because that list runs
/// `.output()` with no deadline, and a concurrency example that deadlocks would hang the
/// suite rather than fail it — which is exactly what the previous version of this example
/// did. Every run below is bounded.
///
/// The first assertion is that `docs/spec.md` still shows this file. A specification whose
/// example is not the program anybody runs is how the old one came to deadlock unnoticed:
/// nothing extracted it, so nothing executed it.
#[test]
fn the_spec_worker_pool_example_terminates_and_answers_ninety() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let src = std::fs::read_to_string(root.join("examples/worker-pool.nz"))
        .expect("examples/worker-pool.nz");

    let spec = std::fs::read_to_string(root.join("docs/spec.md")).expect("docs/spec.md");
    const FENCE: &str = "```nazm\nfn worker(jobs: Chan, results: Chan) -> Int {";
    let at = spec
        .find(FENCE)
        .expect("docs/spec.md no longer shows the worker-pool example");
    let after = &spec[at + "```nazm\n".len()..];
    let block = &after[..after.find("```").expect("unclosed fence")];
    assert_eq!(
        block.trim_end(),
        src.trim_end(),
        "docs/spec.md and examples/worker-pool.nz have drifted. The spec is meant to show \
         the program that is actually run."
    );

    let ran = interpret(&src);
    assert_eq!(ran.code, Some(0), "interpreted: {}", ran.stderr);
    assert_eq!(ran.stdout.trim(), "90", "interpreted");

    for opt in [0_u8, 2] {
        let built =
            build_at(&src, opt).unwrap_or_else(|r| panic!("-O{opt} did not compile: {}", r.stderr));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {}", ran.stderr);
        assert_eq!(ran.stdout.trim(), "90", "-O{opt}");
    }
}
