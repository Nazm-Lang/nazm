//! N50: function values and closures.
//!
//! `docs/architecture.md` §7.52 and the spec section *Function values and closures*. What only
//! a whole run shows: that a closure, a named function as a value and a call through one mean
//! the same in the interpreter and in both native backends — the same stdout, exit status and
//! memory report, closure objects included — that what a closure captures is reclaimed exactly
//! once on every exit, and that each refusal is a refusal before anything runs.

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

fn closures_made(line: &str) -> u64 {
    let part = line.split("closures").nth(1).unwrap_or("");
    part.split_whitespace()
        .next()
        .and_then(|w| w.strip_prefix("allocated="))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no closure count in: {line}"))
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

// --- values and calls ------------------------------------------------------------------

#[test]
fn closures_named_functions_and_indirect_calls_agree_everywhere() {
    let line = agrees(
        r#"
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }
fn inc(x: Int) -> Int { x + 1 }
fn make_adder(n: Int) -> fn(Int) -> Int {
    fn(x: Int) -> Int { x + n }
}
fn main() -> Int {
    let s = str_concat("hel", "lo");
    let k = 10;
    let add = make_adder(5);
    let g = fn(x: Int) -> Int { x * k + str_len(s) };
    let h = inc;
    let nested = fn(y: Int) -> Int {
        let inner = fn(z: Int) -> Int { z + k + y };
        inner(1)
    };
    print(int_to_str(twice(add, 1)));
    print(int_to_str(twice(g, 2)));
    print(int_to_str(twice(h, 3)));
    print(int_to_str(nested(100)));
    0
}
"#,
        // Each print, then `main`'s result.
        "1125551110",
    );
    // `add`, `g`, `h`, `nested`, and `inner` once: one object each, every one reclaimed.
    assert_eq!(closures_made(&line), 5, "{line}");
}

#[test]
fn a_capture_is_a_copy_taken_when_the_closure_is_made() {
    // A name bound again in an inner scope after the closure is made changes nothing it
    // holds; a sequence is a handle, so the copy is of the handle and a push through either
    // is seen by both.
    agrees(
        r#"
fn main() -> Int {
    let n = 1;
    let xs = ints_new();
    let f = fn(x: Int) -> Int { x + n + ints_len(xs) };
    if true {
        let n = 100;
        ints_push(xs, 7);
        print(int_to_str(f(0) + n));
    }
    0
}
"#,
        "1020",
    );
}

#[test]
fn function_values_live_in_records_and_vecs_and_compose() {
    agrees(
        r#"
struct Op { name: Str, apply: fn(Int) -> Int, }
fn compose(f: fn(Int) -> Int, g: fn(Int) -> Int) -> fn(Int) -> Int {
    fn(x: Int) -> Int { g(f(x)) }
}
fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }
fn fact(n: Int) -> Int { if n <= 1 { 1 } else { n * apply(fact, n - 1) } }
fn main() -> Int {
    let tag = str_concat("ab", "cd");
    let xs = ints_new();
    ints_push(xs, 3);
    let len_plus = fn(x: Int) -> Int { x + str_len(tag) + ints_get(xs, 0) };
    let op = Op(name: tag, apply: len_plus);
    let fs = vec_new[fn(Int) -> Int]();
    vec_push(fs, len_plus);
    vec_push(fs, compose(len_plus, op.apply));
    let both = vec_get(fs, 1);
    let ap = op.apply;
    print(int_to_str(both(1) + ap(0) + fact(5)));
    0
}
"#,
        "1420",
    );
}

#[test]
fn a_closure_with_managed_results_and_arguments_agrees() {
    agrees(
        r#"
struct Pair { a: Str, b: Int, }
fn main() -> Int {
    let sep = str_concat(", ", "");
    let join = fn(p: Pair, more: Str) -> Pair { Pair(a: str_concat(str_concat(p.a, sep), more), b: p.b + 1) };
    let mut p = Pair(a: "x", b: 0);
    let mut i = 0;
    while i < 3 { p = join(p, int_to_str(i)); i = i + 1; }
    print(p.a);
    p.b - 3
}
"#,
        "x, 0, 1, 20",
    );
}

// --- cleanup ---------------------------------------------------------------------------

#[test]
fn a_failure_inside_a_closure_reclaims_its_captures_and_its_callers() {
    let line = agrees_on(
        &[(
            "prog.nz",
            r#"
fn main() -> Int {
    let d = 0;
    let s = str_concat("x", "y");
    let f = fn(x: Int) -> Int { x / d + str_len(s) };
    let ys = ints_new();
    ints_push(ys, 1);
    let r = f(ints_len(ys));
    print("not reached");
    r
}
"#,
        )],
        "",
        2,
    );
    assert_eq!(closures_made(&line), 1, "{line}");
}

#[test]
fn an_early_return_and_a_loop_release_the_closures_they_made() {
    let line = agrees(
        r#"
fn pick(n: Int) -> Int {
    let mut i = 0;
    let mut total = 0;
    while i < n {
        let s = str_concat("v", int_to_str(i));
        let f = fn(x: Int) -> Int { x + str_len(s) };
        if i == 7 { return total + f(0); }
        total = total + f(i);
        i = i + 1;
    }
    total
}
fn main() -> Int {
    print(int_to_str(pick(10)));
    0
}
"#,
        "370",
    );
    assert_eq!(closures_made(&line), 8, "{line}");
}

#[test]
fn a_returned_closure_keeps_what_it_captured_alive() {
    // The maker's bindings go when it returns; the closure's own references keep the storage.
    // Allocating after the maker returns would reuse storage a closure had not retained, so a
    // closure that only borrowed reads someone else's value here.
    let line = agrees(
        r#"
fn make(k: Int) -> fn(Int) -> Str {
    let xs = ints_new();
    ints_push(xs, k);
    let tag = str_concat("tag-", int_to_str(k));
    fn(i: Int) -> Str { str_concat(tag, int_to_str(ints_get(xs, i))) }
}
fn main() -> Int {
    let g = make(5);
    let ys = ints_new();
    ints_push(ys, 99);
    let other = str_concat("xxxx-", int_to_str(7));
    let zs = ints_new();
    ints_push(zs, 98);
    print(g(0));
    print(other);
    ints_len(ys) + ints_len(zs) - 2
}
"#,
        "tag-55xxxx-70",
    );
    assert_eq!(closures_made(&line), 1, "{line}");
}

// --- recursion through a value -----------------------------------------------------------

#[test]
fn recursion_through_a_function_value_checks_the_stack_natively() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n\
         fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + apply(down, n - 1) } }\n\
         fn main() -> Int { down(100000000) }\n",
    )
    .unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let r = run_bounded(&mut common::bounded_exe(&exe), backend);
        assert!(r.stderr.contains("N0408"), "{backend}: {r:?}");
        assert_ne!(r.code, None, "{backend} was killed by a signal: {r:?}");
    }
}

// --- the standard library ---------------------------------------------------------------

#[test]
fn map_filter_and_fold_work_over_records_and_enums() {
    agrees(
        r#"
use "@std/seq";
struct Item { name: Str, cost: Int, }
enum Size { Small, Large(by: Int), }
fn size_of(i: Item) -> Size { if i.cost > 10 { Size.Large(by: i.cost - 10) } else { Size.Small() } }
fn extra(acc: Int, s: Size) -> Int {
    match s { Size.Small() => acc, Size.Large(by: n) => acc + n, }
}
fn main() -> Int {
    let items = vec_new[Item]();
    vec_push(items, Item(name: "a", cost: 4));
    vec_push(items, Item(name: "b", cost: 15));
    vec_push(items, Item(name: "c", cost: 30));
    let limit = 5;
    let dear = vec_filter(items, fn(i: Item) -> Bool { i.cost > limit });
    let sizes = vec_map(dear, size_of);
    let names = vec_map(dear, fn(i: Item) -> Str { i.name });
    let total = vec_fold(sizes, 0, extra);
    let joined = vec_fold(names, "", fn(acc: Str, s: Str) -> Str { str_concat(acc, s) });
    print(joined);
    print(int_to_str(total));
    0
}
"#,
        "bc250",
    );
}

// --- across modules ---------------------------------------------------------------------

#[test]
fn a_function_type_crosses_a_module_boundary_and_its_interface() {
    let lib = "pub fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int ! {} { f(f(x)) }\n\
               pub fn scaler(k: Int) -> fn(Int) -> Int ! {} { fn(x: Int) -> Int { x * k } }\n";
    let main = "use \"lib.nz\";\n\
                fn main() -> Int { print(int_to_str(apply_twice(scaler(3), 2))); 0 }\n";
    agrees_on(&[("main.nz", main), ("lib.nz", lib)], "180", 0);

    // The interface publishes the function type, so a second check reuses both modules
    // rather than declining to file the library.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("main.nz"), main).unwrap();
    std::fs::write(d.path().join("lib.nz"), lib).unwrap();
    let report = |r: &Ran| -> serde_json::Value {
        r.stdout
            .lines()
            .find(|l| l.contains("nazm.check-report/1"))
            .map(|l| serde_json::from_str(l).unwrap())
            .unwrap_or_else(|| panic!("no cache report: {r:?}"))
    };
    let first = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(first.code, Some(0), "{first:?}");
    assert_eq!(report(&first)["declined"].as_u64(), Some(0), "{first:?}");
    let again = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let r = report(&again);
    assert_eq!(r["checked"].as_u64(), Some(0), "{r}");
    assert!(r["reused"].as_u64().unwrap() >= 2, "{r}");
}

// --- refusals ---------------------------------------------------------------------------

#[test]
fn a_closure_may_not_capture_a_mutable_binding() {
    assert_eq!(
        codes("fn main() -> Int { let mut n = 1; let f = fn(x: Int) -> Int { x + n }; f(1) }\n"),
        ["N0386"]
    );
    // A capability may be captured since N51 (`effect_params.rs`).
    assert_eq!(
        codes(
            "fn use_it(c: IoCap) -> Int ! {} { 0 }\n\
             fn main(io: IoCap) -> Int ! { io } { let f = fn(x: Int) -> Int { use_it(io) }; f(1) }\n"
        ),
        Vec::<String>::new()
    );
    // Assigning a captured binding inside the closure is assigning an immutable one.
    assert_eq!(
        codes("fn main() -> Int { let n = 1; let f = fn(x: Int) -> Int { n = 2; x }; f(1) }\n"),
        ["N0310"]
    );
}

#[test]
fn only_a_monomorphic_nazm_function_is_a_value() {
    assert_eq!(
        codes("fn id[T](x: T) -> T { x }\nfn main() -> Int { let f = id; 0 }\n"),
        ["N0387"]
    );
    assert_eq!(
        codes("fn main() -> Int { let f = str_len; 0 }\n"),
        ["N0387"]
    );
    assert_eq!(
        codes(
            "extern \"C\" fn labs(x: Int) -> Int = \"labs\";\n\
             fn main() -> Int { let f = labs; 0 }\n"
        ),
        ["N0387"]
    );
    assert_eq!(
        codes(
            "fn g[T](x: T) -> T { let f = fn(y: Int) -> Int { y }; x }\n\
             fn main() -> Int { g(1) }\n"
        ),
        ["N0387"]
    );
    assert_eq!(
        codes(
            "fn inc(y: Int) -> Int { y + 1 }\n\
             fn g[T](x: T) -> T { let f = inc; x }\n\
             fn main() -> Int { g(1) }\n"
        ),
        ["N0387"]
    );
}

#[test]
fn a_function_value_carries_its_effects_and_takes_its_authority_as_parameters() {
    // A closure declares `! {}` unless it writes a set.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let f = fn(c: IoCap, x: Int) -> Int { print(\"a\"); x }; f(io, 1) }\n"
        ),
        ["N0366"]
    );
    // Its authority is what it holds: since N51 a capability visible where it is written
    // too, so with none visible it holds none.
    assert_eq!(
        codes(
            "fn helper() -> Int ! { io } { let f = fn(x: Int) -> Int ! { io } { print(\"a\"); x }; f(1) }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0369"]
    );
    // An undeclared function used as a value is held to the none its type assumed.
    assert_eq!(
        codes(
            "fn loud(x: Int) -> Int { print(\"a\"); x }\n\
             fn main(io: IoCap) -> Int ! { io } { let f = loud; f(1) }\n"
        ),
        ["N0366"]
    );
    // A call through a value requires what its type says.
    assert_eq!(
        codes(
            "fn run(f: fn(Int) -> Int ! { io }, x: Int) -> Int ! {} { f(x) }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0366"]
    );
    agrees(
        r#"
fn run(c: IoCap, f: fn(IoCap, Int) -> Int ! { io }, x: Int) -> Int ! { io } { f(c, x) }
fn main(io: IoCap) -> Int ! { io } {
    run(io, fn(c: IoCap, x: Int) -> Int ! { io } { print("hi"); x - 4 }, 4)
}
"#,
        "hi0",
    );
}

#[test]
fn a_function_value_has_no_equality_and_never_crosses_into_a_task() {
    assert_eq!(
        codes("fn main() -> Bool { let f = fn(x: Int) -> Int { x }; f == f }\n"),
        ["N0304"]
    );
    assert_eq!(
        codes(
            "fn w(f: fn(Int) -> Int) -> Int { f(1) }\n\
             fn main(sp: SpawnCap) -> Int ! { spawn } \
             { let f = fn(x: Int) -> Int { x }; scope { spawn w(f); } 0 }\n"
        ),
        ["N0321"]
    );
}

#[test]
fn an_indirect_call_is_checked_against_the_values_type() {
    assert_eq!(
        codes("fn main() -> Int { let f = fn(x: Int) -> Int { x }; f(1, 2) }\n"),
        ["N0303"]
    );
    assert_eq!(
        codes("fn main() -> Int { let f = fn(x: Int) -> Int { x }; f(\"a\") }\n"),
        ["N0300"]
    );
    assert_eq!(
        codes("fn main() -> Int { let f = fn(x: Int) -> Int { \"a\" }; f(1) }\n"),
        ["N0300"]
    );
    assert_eq!(
        codes(
            "fn run(f: fn(Int) -> Bool) -> Bool { f(1) }\n\
             fn main() -> Int { if run(fn(x: Int) -> Int { x }) { 1 } else { 0 } }\n"
        ),
        ["N0300"]
    );
}

// --- provenance -------------------------------------------------------------------------

#[test]
fn a_path_through_a_function_value_is_of_unknown_origin() {
    // Which function an indirect call reaches is not known where it is, so neither is where
    // its result came from; nor where a closure's parameter came from.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let p = fn(x: Int) -> Str { \"out.txt\" }; write_file(p(1), \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let w = fn(c: IoCap, p: Str) -> Int ! { io } { write_file(p, \"x\") }; w(io, \"o\") }\n"
        ),
        ["N0372"]
    );
    // A captured binding carries its own origins: a literal path is not restricted.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let path = \"out.txt\"; let w = fn(c: IoCap) -> Int ! { io } { write_file(path, \"x\") }; w(io) }\n"
        ),
        Vec::<String>::new()
    );
}

// --- the formatter ----------------------------------------------------------------------

#[test]
fn the_formatter_keeps_function_types_and_closures() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn twice(f: fn(Int) -> Int ! {}, x: Int) -> Int {\n    f(f(x))\n}\n\n\
               fn main() -> Int {\n    let k = 2;\n    twice(fn(x: Int) -> Int { x * k }, 1)\n}\n";
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "--check", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
}
