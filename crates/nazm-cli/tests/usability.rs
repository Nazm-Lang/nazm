//! N49: string escapes, the logical operators, native recursion and qualified names.
//!
//! `docs/architecture.md` §7.51 and the spec sections it names. What only a whole run shows:
//! that each mechanism means the same in the interpreter and in both native backends — the
//! same stdout, exit status and memory report — that refusals are refusals before anything
//! runs, and that the formatter keeps what was written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every program here finishes in well under a second; one that does not fails by name.
const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_bounded(cmd: &mut Command, what: &str) -> Ran {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            let o = child.wait_with_output().expect("collect output");
            return Ran {
                code: o.status.code(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            };
        }
        if start.elapsed() >= DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not terminate within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program at `dir/main` under the interpreter, then built and run by each backend at
/// each optimisation level, with the memory report on.
fn everywhere(dir: &Path, main: &str) -> Vec<(String, Ran)> {
    let mut out = vec![("interpreter".to_owned(), nazm(dir, &["run", main]))];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let b = nazm(
            dir,
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                main,
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected` and exits with `code`; every native build
/// reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let p = d.path().join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
    for (label, r) in everywhere(d.path(), files[0].0) {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        if label != "interpreter" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {}", r.stderr));
            for kind in ["sequences", "strings", "channels"] {
                let part = line.split(kind).nth(1).unwrap_or("");
                assert!(
                    part.contains("live=0"),
                    "{label}: {kind} not all reclaimed: {line}"
                );
            }
        }
    }
}

fn agrees(src: &str, expected: &str) {
    agrees_on(&[("prog.nz", src)], expected, 0);
}

fn check_codes(files: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let r = nazm(d.path(), &["check", "--json", files[0].0]);
    let mut codes: Vec<String> = Vec::new();
    for line in r.stdout.lines().chain(r.stderr.lines()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(c) = v["code"].as_str() {
            codes.push(c.to_owned());
        }
        if let Some(ds) = v["diagnostics"].as_array() {
            for d in ds {
                if let Some(c) = d["code"].as_str() {
                    codes.push(c.to_owned());
                }
            }
        }
    }
    codes
}

// --- string escapes --------------------------------------------------------------------

#[test]
fn escapes_are_the_same_bytes_in_every_implementation() {
    // Each byte is printed as a number, so the comparison is of bytes, not of a terminal's
    // rendering of them.
    let src = r#"
fn bytes(s: Str) -> Int {
    let mut i = 0;
    while i < str_len(s) {
        print(str_concat(int_to_str(str_byte(s, i)), "\n"));
        i = i + 1;
    }
    str_len(s)
}
fn main() -> Int {
    let n = bytes("a\nb\t\r\0\\\"\x41\x7f");
    print("line one\nline two\n");
    print("é\x21\n");
    if "\x41" == "A" { n } else { 0 - 1 }
}
"#;
    agrees(
        src,
        "97\n10\n98\n9\n13\n0\n92\n34\n65\n127\nline one\nline two\né!\n10",
    );
}

#[test]
fn unicode_escapes_are_the_same_bytes_in_every_implementation() {
    // N76: `\u{…}` is the UTF-8 encoding of a scalar value — the bytes the character
    // written directly would be — in the interpreter and every native build alike.
    let src = r#"
fn bytes(s: Str) -> Int {
    let mut i = 0;
    while i < str_len(s) {
        print(str_concat(int_to_str(str_byte(s, i)), " "));
        i = i + 1;
    }
    print("\n");
    str_len(s)
}
fn main() -> Int {
    let a = bytes("\u{41}\u{e9}\u{20AC}\u{1F600}");
    let same = "\u{e9}" == "é";
    let b = bytes("\u{0}\u{7F}\u{80}");
    if same { a * 10 + b } else { 0 - 1 }
}
"#;
    agrees(
        src,
        "65 195 169 226 130 172 240 159 152 128 \n0 127 194 128 \n104",
    );
}

#[test]
fn a_refused_unicode_escape_names_its_bytes() {
    for bad in ["\\u{D800}", "\\u{110000}", "\\u{}", "\\u{1234567}", "\\u41"] {
        assert_eq!(
            check_codes(&[(
                "prog.nz",
                &format!("fn main() -> Int {{ str_len(\"{bad}\") }}\n")
            )]),
            ["N0004"],
            "{bad}"
        );
    }
}

#[test]
fn a_refused_escape_stops_the_program_before_anything_runs() {
    assert_eq!(
        check_codes(&[("prog.nz", "fn main() -> Int { str_len(\"a\\qb\") }\n")]),
        ["N0004"]
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main() -> Int { print(\"ran\"); str_len(\"\\x80\") }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert_eq!(r.code, Some(1), "{}", r.stderr);
    assert!(!r.stdout.contains("ran"), "{}", r.stdout);
    assert!(r.stderr.contains("N0004"), "{}", r.stderr);
}

#[test]
fn the_formatter_keeps_an_escape_as_it_was_written() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn main()->Int{str_len(\"a\\n\\x41\\\\\")}\n";
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let formatted = std::fs::read_to_string(d.path().join("prog.nz")).unwrap();
    assert!(formatted.contains("\"a\\n\\x41\\\\\""), "{formatted}");
    let again = nazm(d.path(), &["fmt", "--check", "prog.nz"]);
    assert_eq!(again.code, Some(0), "idempotent: {}", again.stderr);
}

// --- logical operators -----------------------------------------------------------------

#[test]
fn logical_operators_short_circuit_and_bind_as_specified_everywhere() {
    // `seen` prints when it is evaluated, so the output is a trace of which operands ran.
    // `boom` divides by zero: reached, it would fail the program, so its absence from the
    // trace and the program's success are two observations of the same skipped operand.
    let src = r#"
fn seen(tag: Str, v: Bool) -> Bool { print(str_concat(tag, "\n")); v }
fn boom() -> Bool { 1 / (1 - 1) == 0 }
fn b(v: Bool) -> Int { if v { 1 } else { 0 } }
fn main() -> Int {
    let mut n = 0;
    n = n * 2 + b(seen("a", false) && seen("never-1", true));
    n = n * 2 + b(seen("b", true) || seen("never-2", true));
    n = n * 2 + b(seen("c", true) && seen("d", true));
    n = n * 2 + b(seen("e", false) || seen("f", false));
    n = n * 2 + b(false && boom());
    n = n * 2 + b(true || boom());
    // `||` loosest, then `&&`, then comparison; `!` as tight as `-`.
    n = n * 2 + b(1 == 2 && 3 < 4 || 5 > 4);
    n = n * 2 + b(!(1 == 1) || !false && 2 < 1);
    n = n * 2 + b(!!true);
    n = n * 2 + b(true || false && false);
    n
}
"#;
    // a b c d e f, and the bits 0 1 1 0 0 1 1 0 1 1 = 411.
    agrees(src, "a\nb\nc\nd\ne\nf\n411");
}

#[test]
fn a_logical_operand_that_is_not_bool_is_refused() {
    for src in [
        "fn main() -> Int { if 1 && true { 1 } else { 0 } }\n",
        "fn main() -> Int { if true || 0 { 1 } else { 0 } }\n",
        "fn main() -> Int { if !1 { 1 } else { 0 } }\n",
    ] {
        assert_eq!(check_codes(&[("prog.nz", src)]), ["N0304"], "{src}");
    }
}

#[test]
fn the_formatter_spaces_the_logical_operators_and_keeps_the_effect_marker() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn f(a: Bool, b: Bool)->Bool ! {} {!a&&b||!(a)}\nfn main()->Int{if f(true,false){1}else{0}}\n";
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let out = std::fs::read_to_string(d.path().join("prog.nz")).unwrap();
    assert!(out.contains("!a && b || !(a)"), "{out}");
    assert!(out.contains("-> Bool ! {}"), "{out}");
}

// --- native recursion ------------------------------------------------------------------

#[test]
fn direct_mutual_generic_and_cross_module_recursion_compile_and_agree() {
    let main = r#"
use "parity.nz";
struct Node { value: Int, label: Str, }
fn fact(n: Int) -> Int { if n <= 1 { 1 } else { n * fact(n - 1) } }
fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
fn count[T](items: Vec[T], i: Int) -> Int { if i >= vec_len(items) { 0 } else { 1 + count(items, i + 1) } }
fn depth(n: Int) -> Int { if n == 0 { 0 } else { 1 + depth(n - 1) } }
fn labels(n: Int) -> Str { if n == 0 { "" } else { str_concat(Node(value: n, label: int_to_str(n)).label, labels(n - 1)) } }
fn main() -> Int {
    let v = vec_new[Str]();
    vec_push(v, "a");
    vec_push(v, "b");
    vec_push(v, "c");
    print(str_concat(labels(5), "\n"));
    let e = if is_even(10) { 1 } else { 0 };
    fact(10) % 1000 + fib(15) + count(v, 0) + depth(400) + e
}
"#;
    let parity = r#"
pub fn is_even(n: Int) -> Bool { if n == 0 { true } else { is_odd(n - 1) } }
pub fn is_odd(n: Int) -> Bool { if n == 0 { false } else { is_even(n - 1) } }
"#;
    // 10! % 1000 = 800, fib(15) = 610, 3, 400, 1 — a depth inside the interpreter's own
    // budget of 512, which is that implementation's limit and not the language's.
    agrees_on(
        &[("main.nz", main), ("parity.nz", parity)],
        "54321\n1814",
        0,
    );
}

/// Recursion with no base case, built by each backend; it must end with `N0408` at the
/// function entered, exit status 2, and its frames released — never a signal.
fn exhausts(src: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    for (backend, opt) in [("llvm", "0"), ("llvm", "2"), ("cranelift", "0")] {
        let exe = d.path().join(format!("{backend}{opt}"));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let mut c = common::bounded_exe_within(&exe, common::TASKS_ADDRESS_SPACE_KB);
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
        let r = run_bounded(&mut c, backend);
        assert_eq!(r.code, Some(2), "{backend} -O{opt}: {r:?}");
        assert!(
            r.stderr
                .contains("error[N0408]: the native call stack is exhausted at prog.nz:"),
            "{backend} -O{opt}: {}",
            r.stderr
        );
        let line = r
            .stderr
            .lines()
            .find(|l| l.starts_with("nazm-memory:"))
            .unwrap();
        assert!(
            line.contains("strings allocated=") && !line.contains("live=1"),
            "{line}"
        );
    }
}

#[test]
fn native_recursion_goes_as_deep_as_the_stack_allows() {
    // Past the interpreter's default budget by two orders of magnitude, and inside a
    // thread's stack budget at either optimisation level.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn depth(n: Int) -> Int { if n == 0 { 0 } else { 1 + depth(n - 1) } }\n\
         fn main() -> Int { depth(50000) }\n",
    )
    .unwrap();
    for (backend, opt) in [("llvm", "0"), ("llvm", "2"), ("cranelift", "0")] {
        let exe = d.path().join(format!("{backend}{opt}"));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let r = run_bounded(&mut common::bounded_exe(&exe), backend);
        assert_eq!(
            (r.code, r.stdout.trim()),
            (Some(0), "50000"),
            "{backend} -O{opt}: {r:?}"
        );
    }
}

#[test]
fn unbounded_recursion_reports_n0408_instead_of_crashing() {
    // A string per frame, so the unwinding has something to release at every level.
    exhausts(
        "fn down(n: Int, s: Str) -> Int { let t = str_concat(s, \"x\"); down(n + 1, str_slice(t, 0, 1)) + str_len(t) }\n\
         fn main() -> Int { down(0, \"a\") }\n",
    );
    // Mutual, and through a non-recursive helper between the two checks.
    exhausts(
        "fn helper(n: Int) -> Int { ping(n) + 1 }\n\
         fn ping(n: Int) -> Int { pong(n + 1) }\n\
         fn pong(n: Int) -> Int { helper(n + 1) }\n\
         fn main() -> Int { ping(0) }\n",
    );
}

#[test]
fn a_task_that_exhausts_its_stack_fails_its_scope_after_the_join() {
    exhausts(
        "fn down(n: Int) -> Int { down(n + 1) + 1 }\n\
         fn job(c: Chan) -> Int { chan_send(c, 1); down(0) }\n\
         fn main(t: SpawnCap) -> Int { let c = chan_new(4); scope { spawn job(c); spawn job(c); } 0 }\n",
    );
}

#[test]
fn the_interpreter_keeps_its_own_limit() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn down(n: Int) -> Int { down(n + 1) + 1 }\nfn main() -> Int { down(0) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert!(r.stderr.contains("N0403"), "{}", r.stderr);
}

// --- qualified names -------------------------------------------------------------------

const GEOMETRY: &str = "pub struct Point { x: Int, y: Int, }\n\
pub enum Shape { Dot(at: Point), Square(side: Int), }\n\
pub fn area(s: Shape) -> Int { match s { Shape.Dot(at: _) => 0, Shape.Square(side: n) => n * n, } }\n\
pub fn parse(s: Str) -> Int { str_len(s) }\n";

const OTHER: &str = "pub fn parse(s: Str) -> Int { 100 + str_len(s) }\n";

#[test]
fn qualified_names_resolve_to_the_definitions_an_import_would_and_agree_everywhere() {
    // Two modules export `parse`: unqualified that is N0207, qualified it is two names.
    let main = r#"
use "geometry.nz" as g;
use "other.nz" as o;
use "@std/text" as t;
fn corner(p: g::Point) -> Int { p.x + p.y }
fn measure(s: g::Shape) -> Int {
    match s { g::Shape.Dot(at: p) => corner(p), g::Shape.Square(side: n) => n, }
}
fn main() -> Int {
    let p = g::Point(x: 2, y: 3);
    let sq = g::Shape.Square(side: 4);
    print(str_concat(t::text_trim("  hi  "), "\n"));
    g::area(sq) + measure(g::Shape.Dot(at: p)) + g::parse("ab") + o::parse("ab")
}
"#;
    // 16 + 5 + 2 + 102.
    agrees_on(
        &[
            ("main.nz", main),
            ("geometry.nz", GEOMETRY),
            ("other.nz", OTHER),
        ],
        "hi\n125",
        0,
    );
}

#[test]
fn a_module_imported_both_ways_is_one_module_seen_both_ways() {
    let main = "use \"geometry.nz\";\nuse \"geometry.nz\" as g;\n\
                fn main() -> Int { area(g::Shape.Square(side: 3)) + g::area(Shape.Square(side: 2)) }\n";
    agrees_on(&[("main.nz", main), ("geometry.nz", GEOMETRY)], "13", 0);
}

#[test]
fn an_import_under_a_name_brings_nothing_into_scope_unqualified() {
    let main = "use \"geometry.nz\" as g;\nfn main() -> Int { parse(\"a\") }\n";
    let codes = check_codes(&[("main.nz", main), ("geometry.nz", GEOMETRY)]);
    assert!(codes.contains(&"N0201".to_owned()), "{codes:?}");
}

#[test]
fn an_alias_that_already_means_something_is_refused() {
    for main in [
        "use \"geometry.nz\" as main;\nfn main() -> Int { 0 }\n",
        "use \"geometry.nz\" as Item;\nstruct Item { v: Int, }\nfn main() -> Int { 0 }\n",
        "use \"geometry.nz\" as g;\nuse \"other.nz\" as g;\nfn main() -> Int { 0 }\n",
    ] {
        let codes = check_codes(&[
            ("main.nz", main),
            ("geometry.nz", GEOMETRY),
            ("other.nz", OTHER),
        ]);
        assert_eq!(codes, ["N0209"], "{main}");
    }
}

#[test]
fn a_qualified_name_that_names_nothing_is_refused_by_what_is_missing() {
    for main in [
        "use \"geometry.nz\" as g;\nfn main() -> Int { h::area(g::Shape.Square(side: 1)) }\n",
        "use \"geometry.nz\" as g;\nfn main() -> Int { g::nope(1) }\n",
        "use \"geometry.nz\" as g;\nfn f(p: g::Nope) -> Int { 0 }\nfn main() -> Int { 0 }\n",
    ] {
        let codes = check_codes(&[("main.nz", main), ("geometry.nz", GEOMETRY)]);
        assert!(codes.contains(&"N0210".to_owned()), "{main}: {codes:?}");
    }
    // A qualified name is never a value: Nazm has no module-level ones.
    let codes = check_codes(&[
        (
            "main.nz",
            "use \"geometry.nz\" as g;\nfn main() -> Int { let a = g::area; 0 }\n",
        ),
        ("geometry.nz", GEOMETRY),
    ]);
    assert!(
        codes.contains(&"N0001".to_owned()) || !codes.is_empty(),
        "{codes:?}"
    );
}

#[test]
fn the_formatter_glues_a_qualified_name() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("geometry.nz"), GEOMETRY).unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"geometry.nz\"   as   g;\nfn main()->Int{g :: area(g::Shape.Square(side:1))}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["fmt", "main.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let out = std::fs::read_to_string(d.path().join("main.nz")).unwrap();
    assert!(out.contains("use \"geometry.nz\" as g;"), "{out}");
    assert!(out.contains("g::area(g::Shape.Square(side: 1))"), "{out}");
}
