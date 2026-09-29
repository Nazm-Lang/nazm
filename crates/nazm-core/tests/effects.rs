//! N36: typed effects, within one module.
//!
//! `docs/spec.md`, *Effects*, is the law. What these tests hold:
//!
//! * an effect comes only from a resolved built-in that exercises it or from a `spawn`, and
//!   reaches every caller, through any number of calls and any recursion, as the least fixed
//!   point — never from a spelling, a `Result`, arithmetic, a record, an enum or a sequence;
//! * a declared set is a contract: a body that needs more is refused at the first thing that
//!   needs it, with one shortest witness, and a wider one is allowed and is what callers see;
//! * the empty set is purity, and a purity assertion fails transitively;
//! * a name that is not an effect, or one written twice, is refused once, with no cascade;
//! * order, repetition and source order change nothing, and nothing changes what runs.
//!
//! Since N37 a function that declares a set holds only the authority of the capabilities
//! in its scope, so every function here that declares one and reaches `io` or `spawn` is
//! handed the capability for it: these are effect laws, and each test's only diagnostic is
//! the effect one. The authority laws are `tests/capabilities.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::sema::{Effect, EffectSet, EffectSource};
use nazm_core::{check_source, run, Outcome, Value};
use nazm_diag::codes;

fn diagnostics(src: &str) -> Vec<(String, u32, u32)> {
    check_source(src)
        .into_iter()
        .map(|d| {
            (
                d.code.as_str().to_owned(),
                d.primary.span.start,
                d.primary.span.end,
            )
        })
        .collect()
}

/// The span of `name` at the start of the first `context` in `src`: a callee's name.
fn name(src: &str, context: &str, name: &str) -> (u32, u32) {
    let start = src
        .find(context)
        .unwrap_or_else(|| panic!("no {context:?}"));
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + name.len()).unwrap(),
    )
}

/// The span of the first `needle` in `src`.
fn at(src: &str, needle: &str) -> (u32, u32) {
    let start = src.find(needle).unwrap_or_else(|| panic!("no {needle:?}"));
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

fn accepted(src: &str) {
    let d = check_source(src);
    assert!(d.is_empty(), "{d:#?}\n{src}");
}

fn refused_at(src: &str, code: nazm_diag::Code, span: (u32, u32)) {
    assert_eq!(
        diagnostics(src),
        vec![(code.as_str().to_owned(), span.0, span.1)],
        "\n{src}"
    );
}

/// Every function of the root module's inferred effects, by name.
fn effects(src: &str) -> Vec<(String, Option<EffectSet>, Option<EffectSet>)> {
    let src = src.to_owned();
    // The checker's stack, as every real consumer runs it.
    std::thread::Builder::new()
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(move || {
            let (map, graph) = nazm_core::one_file(&src);
            let a = nazm_core::analyse(&map, &graph, nazm_core::Entry::Absent);
            let root = graph.root();
            let mut out: Vec<_> = a
                .resolution
                .functions()
                .iter()
                .filter(|f| f.module == root)
                .map(|f| (f.name.clone(), f.declared_effects, f.inferred_effects))
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        })
        .unwrap()
        .join()
        .unwrap()
}

fn inferred(src: &str, name: &str) -> EffectSet {
    effects(src)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .and_then(|(_, _, i)| i)
        .unwrap_or_else(|| panic!("no inferred effects for {name}"))
}

const IO: EffectSet = EffectSet::of(Effect::Io);
const SPAWN: EffectSet = EffectSet::of(Effect::Spawn);

#[test]
fn ordinary_computation_is_pure_and_asserting_it_is_accepted() {
    let src = "struct P { x: Int, y: Int, }\n\
        enum Shape { Dot, Line(length: Int), }\n\
        fn area(p: P) -> Int ! {} { p.x * p.y + 1 }\n\
        fn len(s: Shape) -> Int ! {} { match s { Shape.Dot() => 0, Shape.Line(length: l) => l, } }\n\
        fn first[A, B](a: A, b: B) -> A ! {} { a }\n\
        fn same[T: Equality](a: T, b: T) -> Bool ! {} { a == b }\n\
        fn text(n: Int) -> Str ! {} { str_concat(int_to_str(n), \"!\") }\n\
        fn seq() -> Int ! {} { let v = vec_new[Int](); vec_push(v, 1); let s = ints_new(); ints_push(s, 2); vec_len(v) + ints_pop(s) }\n\
        fn chans() -> Int ! {} { let c = chan_new(1); chan_send(c, 1); chan_close(c); 0 }\n\
        fn halve(n: Int) -> Result[Int, Str] ! {} { if n % 2 == 0 { Result[Int, Str].Ok(value: n / 2) } else { Result[Int, Str].Err(error: \"odd\") } }\n\
        fn quarter(n: Int) -> Result[Int, Str] ! {} { let h = halve(n)?; halve(h) }\n\
        fn loops(n: Int) -> Int ! {} { let mut i = 0; let mut t = 0; while i < n { t = t + i; i = i + 1; } t }\n\
        fn main() -> Int { area(P(x: 1, y: 2)) + len(Shape.Dot()) + first(1, \"x\") + str_len(text(3)) + seq() + chans() + loops(3) + if same(1, 1) { 1 } else { 0 } }\n";
    accepted(src);
    for (name, _, inferred) in effects(src) {
        assert_eq!(inferred, Some(EffectSet::EMPTY), "{name}");
    }
}

#[test]
fn every_outside_world_built_in_requires_io_and_a_pure_function_may_call_none_of_them() {
    for (call, needle) in [
        ("print(\"x\")", "print"),
        ("eprint(\"x\")", "eprint"),
        ("args_count()", "args_count"),
        ("str_len(arg(0))", "arg"),
        ("str_len(read_file(\"f\"))", "read_file"),
        ("write_file(\"f\", \"d\")", "write_file"),
        ("if file_exists(\"f\") { 1 } else { 0 }", "file_exists"),
        ("exit_with(0)", "exit_with"),
    ] {
        let src = format!(
            "fn f(io: IoCap) -> Int ! {{}} {{ {call} }}\nfn main(io: IoCap) -> Int {{ f(io) }}\n"
        );
        refused_at(&src, codes::UNDECLARED_EFFECT, at(&src, needle));
        let src = src.replace("! {}", "! { io }");
        accepted(&src);
        assert_eq!(inferred(&src, "f"), IO, "{call}");
    }
}

#[test]
fn a_spawn_requires_spawn_and_every_effect_of_the_task_it_starts() {
    let src = "fn quiet(c: Chan) -> Int { 0 }\n\
        fn loud(c: Chan) -> Int { print(\"t\") }\n\
        fn a(s: SpawnCap) -> Int { let c = chan_new(1); scope { spawn quiet(c); } 0 }\n\
        fn b() -> Int { let c = chan_new(1); scope { spawn loud(c); } 0 }\n\
        fn through() -> Int { b() }\n\
        fn main(s: SpawnCap) -> Int { a(s) + through() }\n";
    accepted(src);
    assert_eq!(inferred(src, "quiet"), EffectSet::EMPTY);
    assert_eq!(inferred(src, "a"), SPAWN);
    assert_eq!(inferred(src, "b"), IO.union(SPAWN));
    assert_eq!(inferred(src, "through"), IO.union(SPAWN));
    let narrow = src.replace(
        "fn a(s: SpawnCap) -> Int {",
        "fn a(s: SpawnCap) -> Int ! { io } {",
    );
    refused_at(
        &narrow,
        codes::UNDECLARED_EFFECT,
        name(&narrow, "quiet(c); }", "quiet"),
    );
}

#[test]
fn a_set_is_the_same_in_any_order_and_a_wider_contract_is_allowed() {
    let a = "fn f(io: IoCap) -> Int ! { io, spawn } { print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    let b = "fn f(io: IoCap) -> Int ! { spawn, io } { print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    accepted(a);
    accepted(b);
    assert_eq!(effects(a), effects(b));
    let (_, declared, inferred) = effects(a).into_iter().find(|e| e.0 == "f").unwrap();
    assert_eq!(declared, Some(IO.union(SPAWN)));
    assert_eq!(
        inferred,
        Some(IO),
        "a wider contract does not widen what the body needs"
    );
    // Callers read the contract, not the body: `g` needs `spawn` because `f` may.
    let src = "fn f(io: IoCap) -> Int ! { io, spawn } { print(\"x\") }\nfn g(io: IoCap) -> Int ! { io } { f(io) }\nfn main(io: IoCap) -> Int { g(io) }\n";
    refused_at(src, codes::UNDECLARED_EFFECT, name(src, "f(io) }", "f"));
}

#[test]
fn a_purity_assertion_fails_through_every_level_and_names_a_shortest_witness() {
    let src = "fn leaf() -> Int { print(\"x\") }\n\
        fn mid() -> Int { leaf() }\n\
        fn top() -> Int { mid() }\n\
        fn detour() -> Int { top() }\n\
        fn pure(io: IoCap) -> Int ! {} { if true { detour() } else { mid() } }\n\
        fn main(io: IoCap) -> Int { pure(io) }\n";
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:#?}");
    let d = &d[0];
    assert_eq!(d.code, codes::UNDECLARED_EFFECT);
    // `mid` is two calls from `print`, `detour` four: the shorter chain is the witness,
    // though `detour` is called first.
    let (s, _) = at(src, "mid() } }");
    assert_eq!((d.primary.span.start, d.primary.span.end), (s, s + 3));
    let hops: Vec<(u32, u32)> = d
        .secondary
        .iter()
        .map(|l| (l.span.start, l.span.end))
        .collect();
    let leaf_call = at(src, "leaf() }");
    let print_call = at(src, "print");
    assert!(hops.contains(&(leaf_call.0, leaf_call.0 + 4)), "{hops:?}");
    assert!(hops.contains(&print_call), "{hops:?}");
    assert!(
        d.message.contains("`pure` declares `! {}`"),
        "{}",
        d.message
    );
    // One fix, for review: widening a contract may break its own callers.
    assert_eq!(d.fixes.len(), 1);
    assert_eq!(d.fixes[0].replacement, "! { io }");
    assert_eq!(
        d.fixes[0].applicability,
        nazm_diag::Applicability::NeedsReview
    );
}

#[test]
fn recursion_reaches_the_least_fixed_point_and_injects_nothing() {
    // A cycle of pure functions is pure: recursion is not an effect.
    let pure = "fn even(n: Int) -> Bool ! {} { if n == 0 { true } else { odd(n - 1) } }\n\
        fn odd(n: Int) -> Bool ! {} { if n == 0 { false } else { even(n - 1) } }\n\
        fn count(n: Int) -> Int ! {} { if n == 0 { 0 } else { 1 + count(n - 1) } }\n\
        fn depth[T](x: T, n: Int) -> Int ! {} { if n == 0 { 0 } else { depth(x, n - 1) } }\n\
        fn main() -> Int { if even(4) { count(3) + depth(1, 2) } else { 0 } }\n";
    accepted(pure);
    // One `print` anywhere on a cycle reaches all of it, and a pure caller of the cycle.
    let src = "fn a(n: Int) -> Int { if n == 0 { 0 } else { b(n - 1) } }\n\
        fn b(n: Int) -> Int { if n == 0 { 0 } else { c(n - 1) } }\n\
        fn c(n: Int) -> Int { if n == 7 { print(\"x\") } else { a(n - 1) } }\n\
        fn self_(n: Int) -> Int { if n == 0 { eprint(\"x\") } else { self_(n - 1) } }\n\
        fn gen[T](x: T, n: Int) -> Int { if n == 0 { print(\"g\") } else { gen(x, n - 1) } }\n\
        fn pure(io: IoCap, n: Int) -> Int ! {} { a(n) }\n\
        fn main(io: IoCap) -> Int { pure(io, 3) + self_(2) + gen(1, 2) }\n";
    for f in ["a", "b", "c", "self_", "gen"] {
        assert_eq!(inferred(src, f), IO, "{f}");
    }
    refused_at(src, codes::UNDECLARED_EFFECT, name(src, "a(n) }", "a"));
    // The same, written in any order.
    let mut lines: Vec<&str> = src.lines().collect();
    lines.reverse();
    let reversed = lines.join("\n");
    assert_eq!(effects(src), effects(&reversed));
}

#[test]
fn an_unknown_or_repeated_effect_is_refused_once_and_does_not_cascade() {
    let src = "fn f() -> Int ! { io, network } { print(\"x\") }\nfn g(io: IoCap) -> Int ! {} { f() }\nfn main(io: IoCap) -> Int { g(io) }\n";
    // `network` is refused, and `f`'s set is then unwritten: its body is not checked
    // against a contract nobody can read, and `g` sees what `f` actually needs.
    let d = diagnostics(src);
    assert_eq!(
        d,
        vec![
            (
                "N0367".to_owned(),
                at(src, "network").0,
                at(src, "network").1
            ),
            (
                "N0366".to_owned(),
                at(src, "f() }").0,
                at(src, "f() }").0 + 1
            ),
        ]
    );
    let src = "fn f(io: IoCap) -> Int ! { io, io } { print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].code, codes::DUPLICATE_EFFECT);
    // At the second `io`, the last in the source.
    let s = u32::try_from(src.find("io } {").unwrap()).unwrap();
    assert_eq!((d[0].primary.span.start, d[0].primary.span.end), (s, s + 2));
    // The fix removes the repetition exactly, and what is left checks.
    let fix = &d[0].fixes[0];
    assert_eq!(fix.applicability, nazm_diag::Applicability::Automatic);
    let fixed = format!(
        "{}{}{}",
        &src[..fix.span.start as usize],
        fix.replacement,
        &src[fix.span.end as usize..]
    );
    assert_eq!(
        fixed,
        "fn f(io: IoCap) -> Int ! { io } { print(\"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n"
    );
    accepted(&fixed);
}

#[test]
fn effect_names_are_their_own_namespace_and_a_call_is_decided_by_what_it_resolved_to() {
    // A function named `io`, and a local named `spawn` is impossible (a keyword), so a
    // parameter named `io`: neither is the effect, and the effect is not either.
    let src = "fn io(io: Int) -> Int ! {} { io + 1 }\nfn main(cap: IoCap) -> Int ! { io } { io(1) + print(\"x\") }\n";
    accepted(src);
    assert_eq!(inferred(src, "io"), EffectSet::EMPTY);
    // A function whose name is a built-in's cannot exist, so a call spelled `print` is the
    // built-in: identity, not spelling, is what the checker used — as a program's own
    // `say` that calls nothing is pure however it is named.
    let src = "fn say(s: Str) -> Int ! {} { str_len(s) }\nfn main() -> Int { say(\"x\") }\n";
    accepted(src);
}

#[test]
fn a_body_that_did_not_parse_adds_nothing_to_the_error_it_is() {
    let src = "fn broken() -> Int ! {} { let = ; }\nfn pure() -> Int ! {} { broken() }\nfn main() -> Int { pure() }\n";
    let d = check_source(src);
    assert!(
        d.iter().all(|d| d.code != codes::UNDECLARED_EFFECT),
        "{d:#?}"
    );
}

#[test]
fn the_analysis_is_the_same_twice_and_its_witness_is_recorded() {
    let src = "fn leaf() -> Int { print(\"x\") }\nfn mid() -> Int { leaf() }\nfn main() -> Int { mid() }\n";
    assert_eq!(effects(src), effects(src));
    assert_eq!(check_source(src), check_source(src));
    let src2 = src.to_owned();
    let reasons = std::thread::Builder::new()
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(move || {
            let (map, graph) = nazm_core::one_file(&src2);
            let a = nazm_core::analyse(&map, &graph, nazm_core::Entry::Absent);
            let f = a
                .resolution
                .functions()
                .iter()
                .find(|f| f.name == "mid")
                .unwrap()
                .clone();
            let leaf = a
                .resolution
                .functions()
                .iter()
                .position(|f| f.name == "leaf")
                .unwrap();
            (f.effect_reasons, leaf)
        })
        .unwrap()
        .join()
        .unwrap();
    let (reasons, leaf) = reasons;
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].effect, Effect::Io);
    assert!(matches!(reasons[0].source, EffectSource::Inferred(id) if id.index() == leaf));
}

#[test]
fn a_large_call_graph_settles_without_recursion_or_explosion() {
    // A long chain, a wide fan-out and a long cycle, with one `print` at the far end of each.
    let mut src = String::new();
    let n = 2000;
    for i in 0..n {
        src.push_str(&format!("fn c{i}() -> Int {{ c{}() }}\n", i + 1));
    }
    src.push_str(&format!("fn c{n}() -> Int {{ print(\"end\") }}\n"));
    let fan = 300;
    // Flat statements, not a 300-deep `+` chain: the fan-out is wide, not nested.
    src.push_str("fn wide() -> Int { let mut t = 0;");
    for i in 0..fan {
        src.push_str(&format!(" t = t + w{i}();"));
    }
    src.push_str(" t }\n");
    for i in 0..fan {
        let body = if i == fan - 1 {
            "eprint(\"w\")".to_owned()
        } else {
            "1".to_owned()
        };
        src.push_str(&format!("fn w{i}() -> Int {{ {body} }}\n"));
    }
    let ring = 500;
    for i in 0..ring {
        let next = (i + 1) % ring;
        let body = if i == 250 {
            format!("if true {{ print(\"r\") }} else {{ r{next}() }}")
        } else {
            format!("r{next}()")
        };
        src.push_str(&format!("fn r{i}() -> Int {{ {body} }}\n"));
    }
    src.push_str("fn main(io: IoCap) -> Int ! { io } { c0() + wide() + r0() }\n");
    let start = std::time::Instant::now();
    let all = effects(&src);
    assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    for (name, _, inferred) in &all {
        let expect = if name.starts_with('w') && name != "wide" && name != &format!("w{}", fan - 1)
        {
            EffectSet::EMPTY
        } else {
            IO
        };
        assert_eq!(inferred.unwrap(), expect, "{name}");
    }
    accepted(&src);
}

#[test]
fn effects_change_nothing_that_runs() {
    let plain = "fn say(io: IoCap, n: Int) -> Int { print(int_to_str(n)) }\nfn sum(n: Int) -> Int { if n == 0 { 0 } else { n + sum(n - 1) } }\nfn main(io: IoCap) -> Int { say(io, sum(4)); sum(3) }\n";
    let typed = "fn say(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }\nfn sum(n: Int) -> Int ! {} { if n == 0 { 0 } else { n + sum(n - 1) } }\nfn main(io: IoCap) -> Int ! { io } { say(io, sum(4)); sum(3) }\n";
    let value = |src: &str| match run(src) {
        Outcome::Value(Value::Int(n)) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!(value(plain), 6);
    assert_eq!(value(typed), value(plain));
}
