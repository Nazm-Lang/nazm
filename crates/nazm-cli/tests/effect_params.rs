//! N51: effect parameters, captured authority and the bounded bridge.
//!
//! `docs/internal/architecture.md` §7.53 and the spec section *Effect parameters and captured
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

use crate::differential;

use differential::{agrees, agrees_on, check_codes, codes, nazm};

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
fn a_value_of_an_undeclared_import_needs_no_authority_of_its_maker() {
    // N104: the function holds what it exercises, as a parameter, so making a value of it
    // needs nothing; calling the value passes the authority like any argument. The value's
    // type still carries every effect, since the import declares none.
    let lib = "pub fn loud(x: Int, out: OutCap) -> Int { print(\"x\"); x }\n";
    let ok = "use \"lib.nz\";\n\
        fn main(io: IoCap) -> Int ! { io, spawn } { let f = loud; f(1, io) }\n";
    assert_eq!(
        check_codes(&[("main.nz", ok), ("lib.nz", lib)]),
        Vec::<String>::new()
    );
    let narrow = "use \"lib.nz\";\n\
        fn main(io: IoCap) -> Int ! { io } { let f = loud; f(1, io) }\n";
    let codes = check_codes(&[("main.nz", narrow), ("lib.nz", lib)]);
    assert!(codes.contains(&"N0366".to_owned()), "{codes:?}");
    assert!(!codes.contains(&"N0369".to_owned()), "{codes:?}");
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
