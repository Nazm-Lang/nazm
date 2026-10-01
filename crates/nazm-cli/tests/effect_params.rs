//! N51: effect parameters, captured authority and the bounded bridge.
//!
//! `docs/architecture.md` §7.53 and the spec section *Effect parameters and captured
//! authority*. That an effect-polymorphic function does what the function it is handed does
//! — in the interpreter and both backends alike — that its callers are held to that, through
//! a wrapper and across a module's interface, that a profile sees it, and that every refusal
//! is one before anything runs.

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

fn memory_line(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {}", r.stderr))
        .to_owned()
}

/// The program `files[0]` under the interpreter and built and run by each backend at each
/// optimisation level, every one with the memory report on.
fn everywhere(files: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let main = files[0].0;
    let mut interpreted = Command::new(NAZM);
    interpreted
        .current_dir(d.path())
        .args(["run", main])
        .env(nazm_lir::MEMORY_VARIABLE, "1");
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(&mut interpreted, "nazm run"),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = nazm(
            d.path(),
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

/// Every implementation prints `expected`, exits with `code`, writes the same memory report
/// — closure objects counted with everything else — and reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) -> String {
    let runs = everywhere(files);
    let first = memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
    first
}

fn agrees(src: &str, expected: &str) -> String {
    agrees_on(&[("prog.nz", src)], expected, 0)
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

fn codes(src: &str) -> Vec<String> {
    check_codes(&[("prog.nz", src)])
}

// --- polymorphism ------------------------------------------------------------------------

#[test]
fn an_effect_polymorphic_wrapper_does_what_its_function_does_everywhere() {
    agrees(
        r#"
use "@std/seq";
fn main(io: IoCap) -> Int ! { io } {
    let xs = vec_new[Int]();
    vec_push(xs, 1);
    vec_push(xs, 2);
    let shown = vec_map(xs, fn(x: Int) -> Int ! { io } { print(int_to_str(x)); x * 10 });
    let pure = vec_map(xs, fn(x: Int) -> Int { x + 1 });
    let kept = vec_filter(shown, fn(x: Int) -> Bool ! { io } { print("?"); x > 10 });
    let total = vec_fold(kept, 0, fn(a: Int, x: Int) -> Int { a + x });
    print(int_to_str(total + vec_len(pure)));
    0
}
"#,
        "12??220",
    );
}

#[test]
fn a_caller_is_held_to_what_the_function_it_passes_does() {
    let pure_main = "use \"@std/seq\";\n\
        fn main(io: IoCap) -> Int ! {} { let xs = vec_new[Int](); \
        vec_len(vec_map(xs, fn(x: Int) -> Int ! { io } { print(\"a\"); x })) }\n";
    assert_eq!(codes(pure_main), ["N0366"]);
    // The same through a callee of this module, whose contract is read from its declaration.
    assert_eq!(
        codes(
            "fn apply[effects E](f: fn() -> Int ! { E }) -> Int ! { E } { f() }\n\
             fn main(io: IoCap) -> Int ! {} { apply(fn() -> Int ! { io } { print(\"a\"); 1 }) }\n"
        ),
        ["N0366"]
    );
    // A pure function passed to the same wrapper asks nothing of its caller.
    assert_eq!(
        codes(
            "use \"@std/seq\";\n\
             fn main() -> Int ! {} { let xs = vec_new[Int](); \
             vec_len(vec_map(xs, fn(x: Int) -> Int { x })) }\n"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_function_may_forward_its_own_effect_parameter() {
    agrees(
        r#"
use "@std/seq";
fn run[effects F](f: fn(Int) -> Int ! { F }) -> Int ! { F } {
    let xs = vec_new[Int]();
    vec_push(xs, 1);
    vec_push(xs, 2);
    vec_len(vec_map(xs, f))
}
fn twice[T, effects E](f: fn(T) -> T ! { E }, x: T) -> T ! { E } { f(f(x)) }
fn main(io: IoCap) -> Int ! { io } {
    let n = run(fn(x: Int) -> Int ! { io } { print(int_to_str(x)); x });
    let m = twice(fn(s: Str) -> Str { str_concat(s, "!") }, "hi");
    print(m);
    n - 2
}
"#,
        "12hi!!0",
    );
    // Forwarding performs the parameter, so the forwarder declares it.
    assert_eq!(
        codes(
            "use \"@std/seq\";\n\
             fn run[effects F](f: fn(Int) -> Int ! { F }) -> Int ! {} \
             { let xs = vec_new[Int](); vec_len(vec_map(xs, f)) }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0366"]
    );
}

#[test]
fn a_result_type_carries_the_binding() {
    agrees(
        r#"
fn wrap[effects E](f: fn() -> Int ! { E }) -> fn() -> Int ! { E } ! { E } { f }
fn main(io: IoCap) -> Int ! { io } {
    let g = wrap(fn() -> Int ! { io } { print("w"); 0 });
    g()
}
"#,
        "w0",
    );
}

#[test]
fn one_parameter_is_one_set_of_effects() {
    assert_eq!(
        codes(
            "fn both[effects E](f: fn() -> Int ! { E }, g: fn() -> Int ! { E }) -> Int ! { E } \
             { f() + g() }\n\
             fn main(io: IoCap) -> Int ! { io } \
             { both(fn() -> Int ! { io } { print(\"a\"); 1 }, fn() -> Int { 2 }) }\n"
        ),
        ["N0389"]
    );
    // A parameter's named effects are its floor: an argument without them does not fit.
    assert_eq!(
        codes(
            "fn logged[effects E](f: fn() -> Int ! { io, E }) -> Int ! { io, E } { f() }\n\
             fn main() -> Int { logged(fn() -> Int { 1 }) }\n"
        ),
        ["N0300"]
    );
}

#[test]
fn a_body_that_performs_its_parameter_declares_it_by_name() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn apply[effects E](f: fn() -> Int ! { E }) -> Int ! {} { f() }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "prog.nz"]);
    assert!(r.stderr.contains("N0366"), "{r:?}");
    assert!(r.stderr.contains("requires `E`"), "{r:?}");
    assert!(
        r.stderr.contains("! { E }"),
        "the fix names the parameter: {r:?}"
    );
}

#[test]
fn an_effect_parameter_is_allowed_only_where_it_means_something() {
    for (src, code) in [
        (
            "fn f[effects E, effects F](g: fn() -> Int ! { E }) -> Int ! { E } { g() }\n\
             fn main() -> Int { 0 }\n",
            "N0388",
        ),
        (
            "fn f[effects E](g: fn() -> Int ! { E }) -> Int { g() }\nfn main() -> Int { 0 }\n",
            "N0388",
        ),
        ("fn main[effects E]() -> Int ! { E } { 0 }\n", "N0388"),
        (
            "fn f[effects E](xs: Vec[fn() -> Int ! { E }]) -> Int ! { E } { 0 }\n\
             fn main() -> Int { 0 }\n",
            "N0388",
        ),
        (
            "struct R[effects E] { x: Int, }\nfn main() -> Int { 0 }\n",
            "N0100",
        ),
        (
            "fn apply[effects E](f: fn() -> Int ! { E }) -> Int ! { E } { f() }\n\
             fn main() -> Int { let g = apply; 0 }\n",
            "N0387",
        ),
    ] {
        assert_eq!(codes(src), [code], "{src}");
    }
}

// --- captured authority -----------------------------------------------------------------

#[test]
fn a_closure_holds_the_capabilities_it_captures() {
    agrees(
        r#"
fn helper(c: IoCap) -> Int ! { io } { print("h"); 0 }
fn main(io: IoCap) -> Int ! { io } {
    let f = fn(x: Int) -> Int ! { io } { print("c"); helper(io) + x };
    let g = f;
    g(0)
}
"#,
        "ch0",
    );
    // Holding authority is not doing anything: a closure that performs `io` still says so.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let f = fn(x: Int) -> Int { print(\"c\"); x }; f(0) }\n"
        ),
        ["N0366"]
    );
}

// --- the bridge -------------------------------------------------------------------------

#[test]
fn a_value_of_an_undeclared_import_needs_the_authority_a_call_would() {
    let lib = "pub fn loud(x: Int) -> Int { print(\"x\"); x }\n";
    let without = "use \"lib.nz\";\n\
        fn main(io: IoCap) -> Int ! { io, spawn } { let f = loud; f(1) }\n";
    let codes = check_codes(&[("main.nz", without), ("lib.nz", lib)]);
    assert!(codes.contains(&"N0369".to_owned()), "{codes:?}");
    let with = "use \"lib.nz\";\n\
        fn main(io: IoCap, sp: SpawnCap) -> Int ! { io, spawn } { let f = loud; f(1) }\n";
    assert_eq!(
        check_codes(&[("main.nz", with), ("lib.nz", lib)]),
        Vec::<String>::new()
    );
}

// --- across a module, and its interface --------------------------------------------------

#[test]
fn an_effect_parameter_crosses_a_module_and_its_cached_interface() {
    let lib =
        "pub fn apply[effects E](f: fn(Int) -> Int ! { E }, x: Int) -> Int ! { E } { f(x) }\n";
    let loud = "use \"lib.nz\";\n\
        fn main(io: IoCap) -> Int ! { io } \
        { print(int_to_str(apply(fn(x: Int) -> Int ! { io } { print(\"in\"); x + 1 }, 4))); 0 }\n";
    agrees_on(&[("main.nz", loud), ("lib.nz", lib)], "in50", 0);
    // Checked twice, the second time from the library's cached interface: the binding is
    // read from what the interface published, and still holds the caller to it.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("lib.nz"), lib).unwrap();
    let pure = "use \"lib.nz\";\n\
        fn main(io: IoCap) -> Int ! {} \
        { apply(fn(x: Int) -> Int ! { io } { print(\"in\"); x }, 4) }\n";
    std::fs::write(d.path().join("main.nz"), loud).unwrap();
    let first = nazm(d.path(), &["check", "main.nz"]);
    assert_eq!(first.code, Some(0), "{first:?}");
    std::fs::write(d.path().join("main.nz"), pure).unwrap();
    let again = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert!(again.stderr.contains("N0366"), "{again:?}");
    assert!(again.stdout.contains("\"reused\":"), "{again:?}");
}

// --- profiles ---------------------------------------------------------------------------

#[test]
fn a_profile_sees_an_effect_that_only_a_callback_performs() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "use \"@std/seq\";\n\
         fn main(io: IoCap) -> Int ! { io } { let xs = vec_new[Int](); \
         vec_len(vec_map(xs, fn(x: Int) -> Int ! { io } { print(\"a\"); x })) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "embedded", "prog.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0510"), "{r:?}");
    assert!(r.stderr.contains("no-io"), "{r:?}");
}
