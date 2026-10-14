//! R1b, L10: closures in generic functions, and a `let mut` binding a closure shares
//! (`docs/spec.md`, *Function values and closures*).
//!
//! A closure written in a generic function is instantiated with it, and so is a named function's
//! value made there; a captured `let mut` binding is one variable the function and the closure
//! share, kept in a cell. The tests ask every implementation — the interpreter, the LIR oracle,
//! LLVM at `-O0` and `-O2`, and Cranelift — for what the rules work out by hand, with every
//! allocation reclaimed; a generic closure built across modules; and provenance through a shared
//! binding.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The program under the interpreter and the LIR oracle, and built and run by each backend,
/// each with the memory report asked for.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

fn nothing_live(r: &Ran) -> bool {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"))
        .split_whitespace()
        .filter(|w| w.starts_with("live="))
        .all(|w| w == "live=0")
}

fn holds(src: &str, expected: &str) {
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(expected), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

const GENERIC: &str = "fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

fn inc(x: Int) -> Int { x + 1 }

fn map_all[T](xs: Vec[T], f: fn(T) -> T) -> Vec[T] {
    let out = vec_new[T]();
    for x in xs { vec_push(out, f(x)); }
    out
}

// A closure over the function's own parameter type, calling a function value it captured.
fn twice_each[T](xs: Vec[T], g: fn(T) -> T) -> Vec[T] {
    map_all(xs, fn(x: T) -> T { g(g(x)) })
}

// A closure nested in a closure, both in a generic function, and a named function's value.
fn nested[T](x: T, f: fn(T) -> Int) -> Int {
    let outer = fn() -> Int {
        let inner = fn(y: T) -> Int { f(y) * 10 };
        inner(x)
    };
    let step = inc;
    step(outer())
}

fn main(out: OutCap) -> Int ! {io} {
    let xs = vec_new[Int]();
    vec_push(xs, 1); vec_push(xs, 2); vec_push(xs, 3);
    let ys = twice_each(xs, fn(x: Int) -> Int { x * 3 });
    show(out, num_to_str(vec_get(ys, 0) + vec_get(ys, 1) + vec_get(ys, 2)));
    let ss = vec_new[Str]();
    vec_push(ss, \"a\"); vec_push(ss, \"bb\");
    let ts = twice_each(ss, fn(s: Str) -> Str { str_concat(s, \"!\") });
    show(out, str_concat(vec_get(ts, 0), vec_get(ts, 1)));
    show(out, num_to_str(nested(\"abc\", fn(s: Str) -> Int { str_len(s) })));
    show(out, num_to_str(nested(4, fn(n: Int) -> Int { n })));
    0
}
";

#[test]
fn a_closure_in_a_generic_function_means_the_same_everywhere_and_is_reclaimed() {
    holds(GENERIC, "54\na!!bb!!\n31\n41\n");
}

const SHARED: &str = "struct P { x: Int, y: Int, }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

fn each(xs: Vec[Int], f: fn(Int) -> Int) -> Int {
    for x in xs { f(x); }
    0
}

fn count_where[T](xs: Vec[T], p: fn(T) -> Bool) -> Int {
    let mut n = 0;
    for x in xs { let hit = fn() -> Int { if p(x) { n = n + 1; } 0 }; hit(); }
    n
}

fn main(out: OutCap) -> Int ! {io} {
    let xs = vec_new[Int]();
    vec_push(xs, 1); vec_push(xs, 2); vec_push(xs, 3);
    // A closure updates what its function then reads.
    let mut total = 0;
    let mut seen = \"\";
    each(xs, fn(x: Int) -> Int { total = total + x; seen = str_concat(seen, num_to_str(x)); x });
    show(out, str_concat(seen, num_to_str(total)));
    // A field of a shared record, written through the closure.
    let mut p = P(x: 1, y: 2);
    let swap = fn() -> Int { p.x = p.y; 0 };
    swap();
    show(out, num_to_str(p.x * 10 + p.y));
    // The function writes after the closure is made, and the closure sees it.
    let mut k = 1;
    let read = fn() -> Int { k };
    k = 7;
    show(out, num_to_str(read()));
    // Nested closures share one variable; a generic function's too.
    let mut n = 0;
    let outer = fn() -> Int { let inner = fn() -> Int { n = n + 10; n }; inner() + inner() };
    show(out, num_to_str(outer() + n));
    show(out, num_to_str(count_where(xs, fn(x: Int) -> Bool { x > 1 })));
    0
}
";

#[test]
fn a_shared_binding_is_one_variable_everywhere_and_is_reclaimed() {
    holds(SHARED, "1236\n22\n7\n50\n2\n");
}

#[test]
fn a_generic_closure_and_a_function_value_are_built_across_modules() {
    // The generic function and its closure are in one module; their instances, and the thunk
    // of the named function the generic body makes a value of, are reached from another.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("seq.nz"),
        "fn bump(x: Int) -> Int { x + 1 }\n\
         pub fn fold[T](xs: Vec[T], f: fn(T) -> Int) -> Int {\n\
             let mut acc = 0;\n\
             let add = fn(x: T) -> Int { acc = acc + f(x); 0 };\n\
             for x in xs { add(x); }\n\
             let b = bump;\n\
             b(acc)\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"seq.nz\";\n\
         fn main() -> Int {\n\
             let xs = vec_new[Int](); vec_push(xs, 10); vec_push(xs, 20);\n\
             let ss = vec_new[Str](); vec_push(ss, \"abc\");\n\
             fold(xs, fn(x: Int) -> Int { x }) + fold(ss, fn(s: Str) -> Int { str_len(s) })\n\
         }\n",
    )
    .unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--backend",
                backend,
                "main.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let r = run_bounded(&mut common::bounded_exe(&exe), backend);
        assert!(r.stdout.starts_with("35"), "{backend}: {r:?}");
    }
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "--interpret", "main.nz"]),
        "nazm run",
    );
    assert!(r.stdout.starts_with("35"), "{r:?}");
}

/// `nazm check` on `src`: every error code it reports, in order.
fn codes(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    r.stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect()
}

#[test]
fn what_a_shared_binding_holds_carries_its_provenance() {
    // A file's contents written into a shared binding by the closure reach the function's read,
    // and the other way round (`N0372`); a literal the same way does not.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let mut p = \"out.txt\"; \
             let f = fn() -> Int ! { io } { p = read_file(\"cfg\"); 0 }; f(); write_file(p, \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let mut p = read_file(\"cfg\"); \
             let f = fn() -> Int ! { io } { write_file(p, \"x\") }; f() }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let mut p = \"a.txt\"; \
             let f = fn() -> Int { p = \"b.txt\"; 0 }; f(); write_file(p, \"x\") }\n"
        ),
        Vec::<String>::new()
    );
}
