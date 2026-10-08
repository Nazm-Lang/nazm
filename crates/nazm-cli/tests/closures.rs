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
mod differential;

use common::{run_bounded, Ran};
use differential::{agrees, agrees_on, check_codes, codes, nazm};

fn closures_made(line: &str) -> u64 {
    let part = line.split("closures").nth(1).unwrap_or("");
    part.split_whitespace()
        .next()
        .and_then(|w| w.strip_prefix("allocated="))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no closure count in: {line}"))
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
fn main(out: OutCap) -> Int {
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
                fn main(out: OutCap) -> Int { print(int_to_str(apply_twice(scaler(3), 2))); 0 }\n";
    agrees_on(&[("main.nz", main), ("lib.nz", lib)], "180", 0);

    // The interface publishes the function type, so `main` is reused on a second check. The
    // library makes a closure, so it files no entry and is checked again every time (Q1-B-C1: a
    // reused module has no closures, and facts naming one made the next run refuse what a cold
    // check accepts) — declined, in the report's words.
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
    assert_eq!(report(&first)["declined"].as_u64(), Some(1), "{first:?}");
    let again = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let r = report(&again);
    assert_eq!(r["checked"].as_u64(), Some(1), "{r}");
    assert_eq!(r["declined"].as_u64(), Some(1), "{r}");
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
            "fn loud(x: Int, out: OutCap) -> Int { print(\"a\"); x }\n\
             fn main(io: IoCap) -> Int ! { io } { let f = loud; f(1, io) }\n"
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
fn a_path_through_a_function_value_carries_what_the_values_it_may_be_carry() {
    // An indirect call may run every function value of its type the program makes (N80):
    // its result carries what they return, and a closure's parameter what such calls hand it.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let p = fn(x: Int) -> Str { \"out.txt\" }; write_file(p(1), \"x\") }\n"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let p = fn(c: IoCap) -> Str ! { io } { read_file(\"cfg\") }; write_file(p(io), \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let w = fn(c: IoCap, p: Str) -> Int ! { io } { write_file(p, \"x\") }; w(io, \"o\") }\n"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let w = fn(c: IoCap, p: Str) -> Int ! { io } { write_file(p, \"x\") }; w(io, read_file(\"cfg\")) }\n"
        ),
        ["N0372"]
    );
    // A captured binding carries its own origins: a literal path is not restricted, and a
    // file's contents captured are, at the closure that captures them.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let path = \"out.txt\"; let w = fn(c: IoCap) -> Int ! { io } { write_file(path, \"x\") }; w(io) }\n"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } \
             { let path = read_file(\"cfg\"); let w = fn(c: IoCap) -> Int ! { io } { write_file(path, \"x\") }; w(io) }\n"
        ),
        ["N0372"]
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

// --- Gate 1-C1: a closure's environment in the ownership graph --------------------------
//
// `docs/spec.md`, *Cycles*: a closure owns what it captures, so capturing a value that can hold
// a function value behind a counted handle is the one way an environment could come to own
// itself. Refused by the captured type (`N0616`), where the closure is written.

/// Every shape of the cycle Gate 1 found, refused before anything runs — not by the spelling
/// of one variable: an alias, a mutual pair, the container inside a record, an enum or an
/// `Option`, records with a function field in a `Vec`, a nested closure, a `Vec` of `Vec`s.
/// Each is refused once, at the capture, and with nothing else.
#[test]
fn a_closure_capturing_a_container_of_function_values_is_refused_in_every_shape() {
    let shapes: [(&str, &str); 9] = [
        (
            "direct",
            "fn main() -> Int {\n    let fs = vec_new[fn() -> Int]();\n    let f = fn() -> Int { vec_len(fs) };\n    vec_push(fs, f);\n    vec_len(fs)\n}\n",
        ),
        (
            "alias",
            "fn main() -> Int {\n    let fs = vec_new[fn() -> Int]();\n    let alias = fs;\n    let f = fn() -> Int { vec_len(alias) };\n    vec_push(fs, f);\n    0\n}\n",
        ),
        (
            "nested",
            "fn main() -> Int {\n    let fs = vec_new[fn() -> Int]();\n    let outer = fn() -> Int {\n        let inner = fn() -> Int { vec_len(fs) + vec_len(fs) };\n        inner()\n    };\n    vec_push(fs, outer);\n    0\n}\n",
        ),
        (
            "record",
            "struct Handlers {\n    hs: Vec[fn() -> Int],\n}\n\nfn main() -> Int {\n    let h = Handlers(hs: vec_new[fn() -> Int]());\n    let f = fn() -> Int { vec_len(h.hs) };\n    vec_push(h.hs, f);\n    0\n}\n",
        ),
        (
            "records in a vec",
            "struct Job {\n    run: fn() -> Int,\n}\n\nfn main() -> Int {\n    let jobs = vec_new[Job]();\n    let f = fn() -> Int { vec_len(jobs) };\n    vec_push(jobs, Job(run: f));\n    0\n}\n",
        ),
        (
            "enum",
            "enum Slot {\n    Empty,\n    Full(hs: Vec[fn() -> Int]),\n}\n\nfn main() -> Int {\n    let hs = vec_new[fn() -> Int]();\n    let s = Slot.Full(hs: hs);\n    let f = fn() -> Int {\n        match s {\n            Slot.Empty() => 0,\n            Slot.Full(hs: v) => vec_len(v),\n        }\n    };\n    vec_push(hs, f);\n    0\n}\n",
        ),
        (
            "option",
            "fn main() -> Int {\n    let o = Option[Vec[fn() -> Int]].None();\n    let f = fn() -> Int {\n        match o {\n            Option.None() => 0,\n            Option.Some(value: v) => vec_len(v),\n        }\n    };\n    f()\n}\n",
        ),
        (
            "vec of vecs",
            "fn main() -> Int {\n    let fss = vec_new[Vec[fn() -> Int]]();\n    let f = fn() -> Int { vec_len(fss) };\n    f()\n}\n",
        ),
        (
            "made in a helper",
            "fn counter(fs: Vec[fn() -> Int]) -> fn() -> Int {\n    fn() -> Int { vec_len(fs) }\n}\n\nfn main() -> Int {\n    let fs = vec_new[fn() -> Int]();\n    vec_push(fs, counter(fs));\n    0\n}\n",
        ),
    ];
    for (shape, src) in shapes {
        assert_eq!(codes(src), ["N0616"], "{shape}");
    }
    // A mutual pair: each capture is its own refusal.
    let mutual = "fn main() -> Int {\n    let a = vec_new[fn() -> Int]();\n    let b = vec_new[fn() -> Int]();\n    let fa = fn() -> Int { vec_len(b) };\n    let fb = fn() -> Int { vec_len(a) };\n    vec_push(a, fa);\n    vec_push(b, fb);\n    0\n}\n";
    assert_eq!(codes(mutual), ["N0616", "N0616"]);
    // And nothing is run: `nazm run` refuses it as `nazm check` does.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), shapes[0].1).unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert_eq!(r.code, Some(1), "{}", r.stderr);
    assert!(r.stderr.contains("error[N0616]"), "{}", r.stderr);
    assert!(
        r.stderr.contains("`fs`") && r.stderr.contains("Vec[fn() -> Int]"),
        "{}",
        r.stderr
    );
}

/// What stays accepted, and is reclaimed everywhere: a named function in a `Vec`; closures
/// capturing an `Int`, a `Str`, a `Vec[Int]` and another closure; a record holding a function
/// value, captured; a closure handed through a generic function; and a function value stored
/// by a function that cannot know what it captured — every function value satisfies the rule
/// where it was made, so storing one is never refused.
#[test]
fn acyclic_closures_in_containers_are_accepted_and_reclaimed_everywhere() {
    let line = agrees(
        r#"
struct Tagged {
    tag: Int,
    f: fn(Int) -> Int,
}

fn double(x: Int) -> Int { x * 2 }

fn keep(fs: Vec[fn(Int) -> Int], f: fn(Int) -> Int) -> Int { vec_push(fs, f) }

fn ident[T](x: T) -> T { x }

fn run_all(fs: Vec[fn(Int) -> Int], x: Int) -> Int {
    let mut acc = 0;
    let mut i = 0;
    while i < vec_len(fs) {
        let g = vec_get(fs, i);
        acc = acc + g(x);
        i = i + 1;
    }
    acc
}

fn main(out: OutCap) -> Int {
    let fs = vec_new[fn(Int) -> Int]();
    let others = vec_new[fn(Int) -> Int]();
    let n = 10;
    let word = str_concat("ab", "c");
    let data = vec_new[Int]();
    vec_push(data, 5);
    let add_n = fn(x: Int) -> Int { x + n };
    let len_w = fn(x: Int) -> Int { x + str_len(word) };
    let sum_d = fn(x: Int) -> Int { x + vec_get(data, 0) };
    let twice_add = fn(x: Int) -> Int { add_n(add_n(x)) };
    let t = Tagged(tag: 7, f: double);
    let tagged = fn(x: Int) -> Int { let g = t.f; g(x) + t.tag };
    vec_push(fs, double);
    vec_push(fs, add_n);
    keep(fs, len_w);
    vec_push(fs, ident(sum_d));
    vec_push(others, twice_add);
    vec_push(others, tagged);
    vec_push(data, 100);
    print(int_to_str(run_all(fs, 1)));
    print("\n");
    run_all(others, 1)
}
"#,
        "23\n30",
    );
    assert_eq!(closures_made(&line), 7, "{line}");
}

/// A closure made in another module and stored here: accepted, because the rule held where it
/// was written; and one written there capturing a container of function values is refused
/// there.
#[test]
fn the_rule_holds_where_a_closure_is_written_in_any_module() {
    let line = agrees_on(
        &[
            (
                "main.nz",
                "use \"lib.nz\";\n\nfn main(out: OutCap) -> Int {\n    let fs = vec_new[fn() -> Int]();\n    vec_push(fs, adder(4));\n    vec_push(fs, adder(5));\n    let a = vec_get(fs, 0);\n    let b = vec_get(fs, 1);\n    a() + b()\n}\n",
            ),
            (
                "lib.nz",
                "pub fn adder(k: Int) -> fn() -> Int {\n    let data = vec_new[Int]();\n    vec_push(data, k);\n    fn() -> Int { vec_get(data, 0) * 10 }\n}\n",
            ),
        ],
        "90",
        0,
    );
    assert_eq!(closures_made(&line), 2, "{line}");
    let refused = check_codes(&[
        (
            "main.nz",
            "use \"lib.nz\";\n\nfn main() -> Int {\n    0\n}\n",
        ),
        (
            "lib.nz",
            "pub fn counter(fs: Vec[fn() -> Int]) -> fn() -> Int {\n    fn() -> Int { vec_len(fs) }\n}\n",
        ),
    ]);
    assert!(refused.contains(&"N0616".to_owned()), "{refused:?}");
}
