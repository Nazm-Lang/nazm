//! N37: capabilities — authority, checked separately from effects, within one module.
//!
//! `docs/spec.md`, *Capabilities*, is the law. What these tests hold:
//!
//! * a function that declares an effect set holds only the authority of the capability
//!   values its scope reaches — parameters and `let`s of type `IoCap` or `SpawnCap` — and
//!   every built-in, `spawn` and call to an undeclared function needs the authority for
//!   what it does;
//! * missing authority (`N0369`) and a missing effect (`N0366`) are two errors, reported in
//!   that order when both apply, and neither implies the other;
//! * holding, passing and storing a capability is not exercising one, and adds no effect;
//! * nothing a program writes constructs a capability (`N0370`), and `main`'s parameters —
//!   the root of authority — can be nothing else (`N0371`);
//! * a function with no declared set is the compatibility bridge: it exercises its
//!   caller's authority, so a caller with a declared set must hold what it needs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::sema::{Effect, EffectSet};
use nazm_core::{check_source, run, Outcome, Value};
use nazm_diag::codes;

fn codes_of(src: &str) -> Vec<String> {
    check_source(src)
        .into_iter()
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

fn accepted(src: &str) {
    let d = check_source(src);
    assert!(d.is_empty(), "{d:#?}\n{src}");
}

/// The span of the first `needle` in `src`.
fn at(src: &str, needle: &str) -> (u32, u32) {
    let start = src.find(needle).unwrap_or_else(|| panic!("no {needle:?}"));
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

/// Exactly one diagnostic, of `code`, whose primary span is `span`.
fn refused_at(src: &str, code: nazm_diag::Code, span: (u32, u32)) -> nazm_diag::Diagnostic {
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:#?}\n{src}");
    let d = d.into_iter().next().unwrap();
    assert_eq!(d.code, code, "{d:#?}");
    assert_eq!((d.primary.span.start, d.primary.span.end), span, "{d:#?}");
    d
}

fn inferred(src: &str, name: &str) -> EffectSet {
    let src = src.to_owned();
    let name = name.to_owned();
    std::thread::Builder::new()
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(move || {
            let (map, graph) = nazm_core::one_file(&src);
            let a = nazm_core::analyse(&map, &graph, nazm_core::Entry::Absent);
            a.resolution
                .functions()
                .iter()
                .find(|f| f.module == graph.root() && f.name == name)
                .and_then(|f| f.inferred_effects)
                .unwrap_or_else(|| panic!("no inferred effects for {name}"))
        })
        .unwrap()
        .join()
        .unwrap()
}

fn value(src: &str) -> i64 {
    match run(src) {
        Outcome::Value(Value::Int(n)) => n,
        other => panic!("{other:?}"),
    }
}

const IO: EffectSet = EffectSet::of(Effect::Io);

#[test]
fn authority_passed_from_main_through_helpers_recursion_and_storage_is_accepted() {
    let src = "struct Ctx { io: IoCap, n: Int, }\n\
        enum Grant { None, Some(cap: IoCap), }\n\
        fn show(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }\n\
        fn mid(io: IoCap, n: Int) -> Int ! { io } { show(io, n) }\n\
        fn walk(io: IoCap, n: Int) -> Int ! { io } { if n == 0 { 0 } else { show(io, n); walk(io, n - 1) } }\n\
        fn ping(io: IoCap, n: Int) -> Int ! { io } { if n == 0 { 0 } else { pong(io, n - 1) } }\n\
        fn pong(io: IoCap, n: Int) -> Int ! { io } { show(io, n); ping(io, n) }\n\
        fn keep(io: IoCap) -> IoCap ! {} { io }\n\
        fn carry(io: IoCap, n: Int) -> Int ! {} { str_len(int_to_str(n)) }\n\
        fn from_record(c: Ctx) -> Int ! { io } { let io = c.io; show(io, c.n) }\n\
        fn from_vec(v: Vec[IoCap]) -> Int ! { io } { let io = vec_get(v, 0); show(io, 7) }\n\
        fn from_enum(g: Grant) -> Int ! { io } { match g { Grant.None() => 0, Grant.Some(cap: io) => show(io, 8), } }\n\
        fn main(io: IoCap) -> Int ! { io } {\n\
            mid(io, 1); walk(keep(io), 2); ping(io, 2); carry(io, 3);\n\
            from_record(Ctx(io: io, n: 6));\n\
            let v = vec_new[IoCap](); vec_push(v, io); from_vec(v);\n\
            from_enum(Grant.Some(cap: io));\n\
            0\n\
        }\n";
    accepted(src);
    // Effects propagate as N36 says, unchanged by the capabilities alongside them.
    for f in [
        "show",
        "mid",
        "walk",
        "ping",
        "pong",
        "from_record",
        "from_vec",
        "main",
    ] {
        assert_eq!(inferred(src, f), IO, "{f}");
    }
    // Holding, returning and storing a capability exercises nothing — not even at a call
    // made while holding one.
    assert_eq!(inferred(src, "keep"), EffectSet::EMPTY);
    assert_eq!(inferred(src, "carry"), EffectSet::EMPTY);
    assert_eq!(value(src), 0);
}

#[test]
fn an_effect_declared_without_authority_is_an_authority_error_and_not_an_effect_one() {
    // The standard streams ask for the narrower `OutCap` (N79); an `IoCap` held gives it.
    for (call, needle, kind) in [
        ("print(\"x\")", "print", "`OutCap`"),
        ("eprint(\"x\")", "eprint", "`OutCap`"),
        ("args_count()", "args_count", "`IoCap`"),
        ("str_len(arg(0))", "arg", "`IoCap`"),
        ("str_len(read_file(\"f\"))", "read_file", "`IoCap`"),
        ("write_file(\"f\", \"d\")", "write_file", "`IoCap`"),
        (
            "if file_exists(\"f\") { 1 } else { 0 }",
            "file_exists",
            "`IoCap`",
        ),
        ("exit_with(0)", "exit_with", "`IoCap`"),
    ] {
        let src = format!("fn f() -> Int ! {{ io }} {{ {call} }}\nfn main() -> Int {{ f() }}\n");
        let d = refused_at(&src, codes::MISSING_AUTHORITY, at(&src, needle));
        assert!(d.message.contains(kind), "{}", d.message);
        // Threading authority changes every caller: never an automatic fix.
        assert!(d.fixes.is_empty(), "{:?}", d.fixes);
        // Holding one is the whole difference.
        accepted(
            &src.replace("fn f() -> Int", "fn f(io: IoCap) -> Int")
                .replace(
                    "fn main() -> Int { f() }",
                    "fn main(io: IoCap) -> Int { f(io) }",
                ),
        );
    }
}

#[test]
fn authority_without_the_effect_is_an_effect_error_and_both_missing_is_both_in_order() {
    // Holds an `IoCap`, declares `! {}`: the effect is what is wrong.
    let src = "fn f(io: IoCap) -> Int ! {} { print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    refused_at(src, codes::UNDECLARED_EFFECT, at(src, "print"));
    // Neither: authority, then the effect, whatever order the calls are in.
    let src = "fn f() -> Int ! {} { let c = chan_new(1); print(\"x\"); scope { spawn w(c); } 0 }\n\
        fn w(c: Chan) -> Int ! {} { 0 }\nfn main() -> Int { f() }\n";
    assert_eq!(
        codes_of(src),
        ["N0369", "N0369", "N0366"],
        "{:#?}",
        check_source(src)
    );
    let d = check_source(src);
    assert!(d[0].message.contains("`OutCap`") && d[1].message.contains("`SpawnCap`"));
    assert_eq!(codes_of(src), codes_of(src));
}

#[test]
fn starting_a_task_needs_spawn_authority_and_the_task_brings_none_of_its_own() {
    let ok = "fn w(io: IoCap, c: Chan) -> Int ! { io } { print(\"t\"); 0 }\n\
        fn run(s: SpawnCap, io: IoCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn w(io, c); } 0 }\n\
        fn main(s: SpawnCap, io: IoCap) -> Int { run(s, io) }\n";
    accepted(ok);
    // No `SpawnCap`: the `spawn` itself is refused.
    let src = ok
        .replace("fn run(s: SpawnCap, io: IoCap)", "fn run(io: IoCap)")
        .replace("{ run(s, io) }", "{ run(io) }");
    let w = at(&src, "w(io, c)").0;
    let d = refused_at(&src, codes::MISSING_AUTHORITY, (w, w + 1));
    assert!(d.message.contains("`SpawnCap`"), "{}", d.message);
    // A task whose function declares nothing has only what it is handed, as any function
    // (N104): its own body is refused, not the spawner's.
    let src = "fn w(c: Chan) -> Int { print(\"t\"); 0 }\n\
        fn run(s: SpawnCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn w(c); } 0 }\n\
        fn main(s: SpawnCap) -> Int { run(s) }\n";
    let d = refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    assert!(d.message.contains("`OutCap`"), "{}", d.message);
    // A task with a contract has only the authority it is handed: its own body is checked.
    let src = "fn w(c: Chan) -> Int ! { io } { print(\"t\"); 0 }\n\
        fn run(s: SpawnCap, io: IoCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn w(c); } 0 }\n\
        fn main(s: SpawnCap, io: IoCap) -> Int { run(s, io) }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
}

#[test]
fn an_undeclared_function_holds_what_it_exercises() {
    // N104: no function exercises its caller's authority. `inner` prints and holds nothing,
    // so it alone is refused, at the `print`; `helper` and `f` call it and need nothing at
    // the call, because a function's authority is its parameters.
    let src = "fn inner() -> Int { print(\"deep\") }\n\
        fn helper() -> Int { inner() }\n\
        fn f(io: IoCap) -> Int ! { io } { helper() }\n\
        fn main(io: IoCap) -> Int { f(io) }\n";
    let d = refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    assert!(
        d.message.contains("`inner` holds no `OutCap`"),
        "{}",
        d.message
    );
    // Threaded from `main`, as a parameter at every level, it is accepted.
    accepted(
        "fn inner(out: OutCap) -> Int { print(\"deep\") }\n\
         fn helper(out: OutCap) -> Int { inner(out) }\n\
         fn f(io: IoCap) -> Int ! { io } { helper(io) }\n\
         fn main(io: IoCap) -> Int { f(io) }\n",
    );
    // `main` holds nothing it is not given, beyond the `out: OutCap` it is given when it
    // writes neither an `OutCap` nor an `IoCap` (R1, 5b.1): it may print, and nothing more.
    accepted("fn main() -> Int { print(\"x\") }\n");
    let src = "fn main() -> Int { write_file(\"f\", \"d\") }\n";
    let d = refused_at(src, codes::MISSING_AUTHORITY, at(src, "write_file"));
    assert!(d.message.contains("`IoCap`"), "{}", d.message);
}

#[test]
fn authority_is_the_bindings_a_name_reaches_and_nothing_else() {
    // Shadowed: the capability is no longer reachable.
    let src = "fn f(io: IoCap) -> Int ! { io } { let io = 3; print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    // Inside a record it holds nothing until it is bound.
    let src = "struct Ctx { io: IoCap, }\nfn f(c: Ctx) -> Int ! { io } { print(\"x\") }\nfn main(io: IoCap) -> Int { f(Ctx(io: io)) }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    // A type parameter is not a capability, whatever it is instantiated with.
    let src =
        "fn g[T](x: T) -> Int ! { io } { print(\"x\") }\nfn main(io: IoCap) -> Int { g(io) }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    // Out of the block that bound it, it is out of scope.
    let src = "struct Ctx { io: IoCap, }\nfn f(c: Ctx) -> Int ! { io } { if true { let io = c.io; 0 } else { 0 }; print(\"x\") }\nfn main(io: IoCap) -> Int { f(Ctx(io: io)) }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "print"));
    // The wrong kind is the wrong type.
    let src =
        "fn f(io: IoCap) -> Int ! { io } { print(\"x\") }\nfn main(s: SpawnCap) -> Int { f(s) }\n";
    assert_eq!(codes_of(src), ["N0300"]);
    // A capability required and not supplied is a missing argument.
    let src = "fn f(io: IoCap) -> Int ! { io } { print(\"x\") }\nfn main() -> Int { f() }\n";
    assert_eq!(codes_of(src), ["N0303"]);
}

#[test]
fn nothing_a_program_writes_constructs_a_capability() {
    for (src, needle) in [
        ("fn main() -> Int { let c = IoCap(x: 1); 0 }\n", "IoCap"),
        ("fn main() -> Int { let c = SpawnCap(); 0 }\n", "SpawnCap"),
        ("fn main() -> Int { IoCap(); 0 }\n", "IoCap"),
    ] {
        refused_at(src, codes::CAPABILITY_CONSTRUCTED, at(src, needle));
    }
    // Its name is the compiler's: no module may declare a type, or a parameter, of it.
    assert_eq!(
        codes_of("struct IoCap { x: Int, }\nfn main() -> Int { 0 }\n"),
        ["N0331"]
    );
    assert_eq!(
        codes_of("enum SpawnCap { A, }\nfn main() -> Int { 0 }\n"),
        ["N0331"]
    );
    // No equality, so no two can be compared — and none derived for what contains one.
    assert_eq!(
        codes_of("fn main(io: IoCap) -> Int { if io == io { 1 } else { 0 } }\n"),
        ["N0304"]
    );
    assert_eq!(
        codes_of("struct C { io: IoCap, }\nfn main(io: IoCap) -> Int { if C(io: io) == C(io: io) { 1 } else { 0 } }\n"),
        ["N0304"]
    );
}

#[test]
fn main_is_the_root_of_authority_and_takes_nothing_else() {
    let src = "fn main(x: Int) -> Int { 0 }\n";
    refused_at(src, codes::MAIN_PARAMETER, at(src, "x: Int"));
    let src = "fn main(io: IoCap, n: Str) -> Int { 0 }\n";
    refused_at(src, codes::MAIN_PARAMETER, at(src, "n: Str"));
    // Any capabilities, in any order, and more than one of a kind.
    accepted("fn main(s: SpawnCap, io: IoCap, again: IoCap) -> Int { 0 }\n");
    // A `main` with a contract has exactly the authority it is handed: the `OutCap` option C
    // gives it (R1, 5b.1) prints, and writes no file.
    accepted("fn main() -> Int ! { io } { print(\"x\") }\n");
    let src = "fn main() -> Int ! { io } { write_file(\"f\", \"d\") }\n";
    refused_at(src, codes::MISSING_AUTHORITY, at(src, "write_file"));
    accepted("fn main(io: IoCap) -> Int ! { io } { write_file(\"f\", \"d\") }\n");
    // The interpreter hands `main` one of each it declares, and the program runs.
    assert_eq!(
        value("fn main(io: IoCap, s: SpawnCap) -> Int ! { io } { print(\"\"); 5 }\n"),
        5
    );
}

#[test]
fn a_call_is_authorised_by_what_it_resolved_to() {
    // A program's own function cannot be named `print`, so what a call named `print`
    // reaches is the built-in; a function of any name that calls nothing needs nothing.
    accepted("fn read_file_safely(s: Str) -> Int ! {} { str_len(s) }\nfn main() -> Int ! {} { read_file_safely(\"x\") }\n");
    assert_eq!(
        codes_of("fn print(s: Str) -> Int { 0 }\nfn main() -> Int { 0 }\n").len(),
        1
    );
    // Channels are values, not authority: a pure function may use one it is handed.
    accepted("fn f(c: Chan) -> Int ! {} { chan_send(c, 1); chan_close(c); 0 }\nfn main() -> Int ! {} { f(chan_new(1)) }\n");
}

#[test]
fn a_large_program_threading_authority_checks_quickly_and_the_same_twice() {
    let mut src = String::new();
    let n = 1500;
    for i in 0..n {
        src.push_str(&format!(
            "fn c{i}(io: IoCap, k: Int) -> Int ! {{ io }} {{ let a = k; let b = a + 1; c{}(io, b) }}\n",
            i + 1
        ));
    }
    src.push_str(&format!(
        "fn c{n}(io: IoCap, k: Int) -> Int ! {{ io }} {{ print(int_to_str(k)) }}\n"
    ));
    src.push_str("fn main(io: IoCap) -> Int ! { io } { c0(io, 0) }\n");
    let start = std::time::Instant::now();
    accepted(&src);
    assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    let broken = src
        .replace("fn c700(io: IoCap, k: Int)", "fn c700(k: Int)")
        .replace("c700(io, b)", "c700(b)");
    let d = check_source(&broken);
    assert_eq!(d, check_source(&broken));
    // `c700` holds nothing, and its call to `c701` needs an argument it does not have.
    assert!(
        d.iter().any(|d| d.code == codes::UNDEFINED_VARIABLE),
        "{d:#?}"
    );
}
