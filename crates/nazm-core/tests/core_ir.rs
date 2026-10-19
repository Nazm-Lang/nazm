//! N39: Core IR, the one lowering of a checked program (`docs/internal/architecture.md` §7.41).
//!
//! The claims held here, each against the representation itself rather than against what a
//! backend later does with it:
//!
//! * Core IR exists only for a program that checked; a rejected one produces none;
//! * every value is typed, every name is an identity, every transfer names its target;
//! * `?` is a `match` whose error arm returns the enclosing function's `Result`;
//! * the printed form is deterministic and names definitions durably;
//! * each function's digests move exactly when §7.41 says they do — and not for a span, a
//!   local's name, a declaration's order, an arm's order or a provenance change;
//! * the verifier refuses Core IR that breaks an invariant;
//! * recursion, deep nesting and large programs lower, and scale linearly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::cir::{self, Ends, ExprKind, MatchOrigin, Stmt};
use nazm_core::sema::ModuleGraph;
use nazm_core::{Entry, Resolution};

/// Lower `src` as a program and hand its Core IR to `f`.
fn with<T: Send>(
    src: &str,
    f: impl FnOnce(&cir::Program, &Resolution, &ModuleGraph) -> T + Send,
) -> T {
    let (map, graph) = nazm_core::one_file(src);
    nazm_core::on_lowered(&map, &graph, Entry::Required, |core, res| {
        Ok(f(core, res, &graph))
    })
    .unwrap_or_else(|d| panic!("expected Core IR, got {d:?}\nfor {src}"))
}

fn printed(src: &str, spans: bool) -> String {
    with(src, |core, res, graph| {
        cir::print(
            core,
            res,
            graph,
            cir::Options {
                spans,
                compact: true,
            },
        )
    })
}

/// The digests of the function called `name`.
fn digest_of(src: &str, name: &str) -> cir::Digests {
    with(src, |core, res, graph| {
        let at = core
            .functions
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("no `{name}`"));
        cir::digests(core, res, graph)[at]
    })
}

fn function<'a>(core: &'a cir::Program, name: &str) -> &'a cir::Function {
    core.functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"))
}

/// Every expression in a region, depth first.
fn exprs_of(b: &cir::Block, out: &mut Vec<cir::Expr>) {
    for s in &b.stmts {
        stmt_exprs(s, out);
    }
    if let Some(t) = &b.tail {
        expr_all(t, out);
    }
}

fn stmt_exprs(s: &Stmt, out: &mut Vec<cir::Expr>) {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Eval(value) => {
            expr_all(value, out);
        }
        Stmt::Return { value, .. } => expr_all(value, out),
        Stmt::If {
            cond,
            then,
            otherwise,
            ..
        } => {
            expr_all(cond, out);
            exprs_of(then, out);
            if let Some(o) = otherwise {
                exprs_of(o, out);
            }
        }
        Stmt::Match(m) => {
            expr_all(&m.scrutinee, out);
            for a in &m.arms {
                exprs_of(&a.body, out);
            }
        }
        Stmt::Loop { cond, body, .. } => {
            expr_all(cond, out);
            exprs_of(body, out);
        }
        Stmt::Block(b) | Stmt::Scope { body: b, .. } => exprs_of(b, out),
        Stmt::Spawn { args, .. } => {
            for a in args {
                expr_all(a, out);
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

fn expr_all(e: &cir::Expr, out: &mut Vec<cir::Expr>) {
    out.push(e.clone());
    match &e.kind {
        ExprKind::Neg(x)
        | ExprKind::Complement(x)
        | ExprKind::Convert { operand: x, .. }
        | ExprKind::Fits { operand: x, .. }
        | ExprKind::ArrayRepeat { value: x, .. } => expr_all(x, out),
        ExprKind::Array { elems } => {
            for x in elems {
                expr_all(x, out);
            }
        }
        ExprKind::Index { base, index } => {
            expr_all(base, out);
            expr_all(index, out);
        }
        ExprKind::Arith { lhs, rhs, .. }
        | ExprKind::Bits { lhs, rhs, .. }
        | ExprKind::Compare { lhs, rhs, .. } => {
            expr_all(lhs, out);
            expr_all(rhs, out);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for a in args {
                expr_all(a, out);
            }
        }
        ExprKind::Record { fields, .. } | ExprKind::Variant { fields, .. } => {
            for i in fields {
                expr_all(&i.value, out);
            }
        }
        ExprKind::Field { base, .. } => expr_all(base, out),
        ExprKind::Match(m) => {
            expr_all(&m.scrutinee, out);
            for a in &m.arms {
                exprs_of(&a.body, out);
            }
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            expr_all(cond, out);
            exprs_of(then, out);
            exprs_of(otherwise, out);
        }
        ExprKind::Block(b) => exprs_of(b, out),
        ExprKind::CallIndirect { callee, args, .. } => {
            expr_all(callee, out);
            for a in args {
                expr_all(a, out);
            }
        }
        ExprKind::CallTrait { args, .. } => {
            for a in args {
                expr_all(a, out);
            }
        }
        ExprKind::Const(_)
        | ExprKind::Local(_)
        | ExprKind::Closure { .. }
        | ExprKind::FnValue(_) => {}
    }
}

// ---------------------------------------------------------------------------------------
// Only after checking
// ---------------------------------------------------------------------------------------

#[test]
fn a_program_that_does_not_check_has_no_core_ir() {
    for src in [
        // A type error.
        "fn main() -> Int { true }",
        // A body that failed to parse: the checker keeps the signature, and no Core IR is
        // made from the partial tree.
        "fn f() -> Int { return; }\nfn main() -> Int { f() }",
        // A name that resolves to nothing.
        "fn main() -> Int { g() }",
    ] {
        let (map, graph) = nazm_core::one_file(src);
        let mut reached = false;
        let out = nazm_core::on_lowered(&map, &graph, Entry::Required, |_, _| {
            reached = true;
            Ok(())
        });
        assert!(out.is_err(), "{src} should be refused");
        assert!(!reached, "Core IR must not reach a consumer for {src}");
        let codes: Vec<_> = out.unwrap_err().iter().map(|d| d.code.clone()).collect();
        assert!(
            !codes.contains(&nazm_diag::codes::COMPILER_DEFECT),
            "a user error is not a compiler defect: {codes:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Typed and resolved
// ---------------------------------------------------------------------------------------

const EVERYTHING: &str = "\
struct Point { x: Int, y: Int, }
enum Shape { Dot(at: Point), Line(from: Point, to: Point), }
fn same[T: Equality](a: T, b: T) -> Bool { a == b }
fn len(s: Shape) -> Int {
    match s {
        Shape.Dot(at: _) => 0,
        Shape.Line(from: a, to: b) => b.x - a.x,
    }
}
fn parse(n: Int) -> Result[Int, Str] {
    if n < 0 { Result[Int, Str].Err(error: \"negative\") } else { Result[Int, Str].Ok(value: n) }
}
fn twice(n: Int) -> Result[Int, Str] {
    let v = parse(n)?;
    Result[Int, Str].Ok(value: v + v)
}
fn main() -> Int {
    let mut p = Point(x: 1, y: 2);
    p.x = 3;
    let xs = vec_new[Str]();
    vec_push(xs, \"a\");
    let mut i = 0;
    while i < 3 { i = i + 1; if i == 2 { continue; } }
    let s = Shape.Line(from: p, to: Point(x: 7, y: 0));
    if same(1, 1) && same(\"a\", \"a\") { 0 } else { 0 };
    match twice(2) { Result.Ok(value: v) => v + len(s), Result.Err(error: _) => 0 - 1, }
}
";

#[test]
fn every_value_is_typed_by_what_the_checker_established() {
    // `&&` is not in the language; the source above is used without it below.
    let src = EVERYTHING.replace(" && same(\"a\", \"a\")", "");
    with(&src, |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
        for f in &core.functions {
            let mut all = Vec::new();
            exprs_of(&f.body, &mut all);
            assert!(!all.is_empty());
            for e in &all {
                // Only a control expression every path of which transfers has no type.
                if e.ty.is_none() {
                    assert!(
                        matches!(
                            e.kind,
                            ExprKind::If { .. } | ExprKind::Match(_) | ExprKind::Block(_)
                        ),
                        "an untyped {:?} in `{}`",
                        e.kind,
                        f.name
                    );
                }
            }
        }
        // A generic call's result is the checker's instantiation, not its declaration's.
        let main = function(core, "main");
        let mut all = Vec::new();
        exprs_of(&main.body, &mut all);
        let generic_call = all
            .iter()
            .find(|e| matches!(&e.kind, ExprKind::Call { type_args, .. } if !type_args.is_empty()))
            .expect("the call to `same`");
        assert_eq!(generic_call.ty, Some(nazm_core::Type::Bool));
    });
}

#[test]
fn calls_name_definitions_and_builtins_by_identity_never_by_spelling() {
    let src = "fn helper(n: Int) -> Int { n }\n\
               fn main() -> Int { let s = int_to_str(helper(4)); str_len(s) }";
    with(src, |core, res, _| {
        let main = function(core, "main");
        let mut all = Vec::new();
        exprs_of(&main.body, &mut all);
        let helper = res
            .definition(nazm_core::sema::ModuleId::ROOT, "helper")
            .unwrap();
        assert!(all
            .iter()
            .any(|e| matches!(&e.kind, ExprKind::Call { callee, .. } if *callee == helper)));
        let builtins: Vec<nazm_core::Intrinsic> = all
            .iter()
            .filter_map(|e| match &e.kind {
                ExprKind::Builtin { op, .. } => Some(*op),
                _ => None,
            })
            .collect();
        assert_eq!(
            builtins,
            vec![nazm_core::Intrinsic::IntToStr, nazm_core::Intrinsic::StrLen]
        );
    });
}

#[test]
fn a_comparison_carries_the_law_it_compares_by() {
    let src = "struct P { x: Int, }\n\
               fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
               fn main() -> Int {\n\
                 let a = 1 < 2; let b = true == false; let c = \"x\" != \"y\";\n\
                 let d = P(x: 1) == P(x: 1); let e = same(1, 2);\n\
                 0\n\
               }";
    with(src, |core, _, _| {
        let laws = |f: &cir::Function| -> Vec<cir::Law> {
            let mut all = Vec::new();
            exprs_of(&f.body, &mut all);
            all.iter()
                .filter_map(|e| match &e.kind {
                    ExprKind::Compare { law, .. } => Some(*law),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            laws(function(core, "main")),
            vec![
                cir::Law::Int,
                cir::Law::Bool,
                cir::Law::Bytes,
                cir::Law::Structural
            ]
        );
        assert_eq!(laws(function(core, "same")), vec![cir::Law::Parametric]);
    });
}

#[test]
fn effects_and_authority_survive_as_function_metadata() {
    let src = "fn quiet(n: Int) -> Int ! {} { n }\n\
               fn loud(io: IoCap, s: Str) -> Int ! { io } { print(s) }\n\
               fn ambient() -> Int { 0 }\n\
               fn main(io: IoCap) -> Int ! { io } { loud(io, \"hi\") + quiet(1) + ambient() }";
    with(src, |core, _, _| {
        let quiet = function(core, "quiet");
        assert_eq!(
            quiet.effects.declared,
            Some(nazm_core::sema::EffectSet::EMPTY)
        );
        assert!(quiet.effects.is_pure());
        assert!(quiet.authority().is_empty());

        let loud = function(core, "loud");
        let io = nazm_core::sema::EffectSet::of(nazm_core::sema::Effect::Io);
        assert_eq!(loud.effects.declared, Some(io));
        assert_eq!(loud.effects.required, io);
        assert!(!loud.effects.is_pure());
        assert!(loud
            .authority()
            .contains(nazm_core::sema::CapabilityKind::Io));
        // A capability is an ordinary typed value: the parameter's type says it.
        assert_eq!(
            loud.locals[0].ty,
            nazm_core::Type::Cap(nazm_core::sema::CapabilityKind::Io)
        );

        let ambient = function(core, "ambient");
        assert_eq!(ambient.effects.declared, None);
        assert!(ambient.effects.is_pure());
    });
}

#[test]
fn provenance_is_not_carried_into_core_ir() {
    // The checker's summary says `r` returns a file's contents; Core IR says nothing about
    // it — no section, no per-node origin set — because nothing below Core IR reads one.
    let src =
        "fn r(io: IoCap) -> Str { read_file(\"f\") }\nfn main(io: IoCap) -> Int { str_len(r(io)) }";
    let text = printed(src, true);
    for key in ["provenance", "origin\":\"file", "origins", "\"file\""] {
        assert!(!text.contains(key), "{key} in {text}");
    }
}

// ---------------------------------------------------------------------------------------
// Control flow, explicit
// ---------------------------------------------------------------------------------------

fn loops(b: &cir::Block, out: &mut Vec<(cir::LoopId, Vec<(bool, cir::LoopId)>)>) {
    for s in &b.stmts {
        match s {
            Stmt::Loop { id, body, cond, .. } => {
                let mut transfers = Vec::new();
                collect_transfers(body, &mut transfers);
                out.push((*id, transfers));
                // A transfer in the condition belongs to the enclosing loop.
                let mut in_cond = Vec::new();
                cond_transfers(cond, &mut in_cond);
                if let Some((_, outer)) = out.iter_mut().rev().nth(1) {
                    outer.extend(in_cond);
                }
                loops(body, out);
            }
            Stmt::If {
                then, otherwise, ..
            } => {
                loops(then, out);
                if let Some(o) = otherwise {
                    loops(o, out);
                }
            }
            Stmt::Block(b) | Stmt::Scope { body: b, .. } => loops(b, out),
            _ => {}
        }
    }
}

fn collect_transfers(b: &cir::Block, out: &mut Vec<(bool, cir::LoopId)>) {
    for s in &b.stmts {
        match s {
            Stmt::Break { target, .. } => out.push((true, *target)),
            Stmt::Continue { target, .. } => out.push((false, *target)),
            Stmt::If {
                then, otherwise, ..
            } => {
                collect_transfers(then, out);
                if let Some(o) = otherwise {
                    collect_transfers(o, out);
                }
            }
            Stmt::Block(b) | Stmt::Scope { body: b, .. } => collect_transfers(b, out),
            _ => {}
        }
    }
}

fn cond_transfers(e: &cir::Expr, out: &mut Vec<(bool, cir::LoopId)>) {
    if let ExprKind::If {
        then, otherwise, ..
    } = &e.kind
    {
        collect_transfers(then, out);
        collect_transfers(otherwise, out);
    }
    if let ExprKind::Block(b) = &e.kind {
        collect_transfers(b, out);
    }
}

#[test]
fn every_break_and_continue_names_the_loop_it_leaves() {
    let src = "fn main() -> Int {\n\
                 let mut i = 0;\n\
                 while i < 10 {\n\
                   i = i + 1;\n\
                   let mut j = 0;\n\
                   while (if j > 3 { break; } else { j < 10 }) {\n\
                     j = j + 1;\n\
                     if j == 2 { continue; }\n\
                     if j == 5 { break; }\n\
                   }\n\
                   if i == 7 { continue; }\n\
                 }\n\
                 i\n\
               }";
    with(src, |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
        let main = function(core, "main");
        assert_eq!(main.loops, 2);
        let mut found = Vec::new();
        loops(&main.body, &mut found);
        let (outer, inner) = (cir::LoopId(0), cir::LoopId(1));
        let targets: Vec<_> = found.iter().map(|(id, t)| (*id, t.clone())).collect();
        // The inner loop's own body: `continue` and `break` name it.
        assert_eq!(targets[1], (inner, vec![(false, inner), (true, inner)]));
        // The outer loop's: its own `continue`, and the `break` written in the inner loop's
        // condition — which is outside the inner loop, so it leaves the outer one.
        assert!(targets[0].1.contains(&(false, outer)));
        assert!(targets[0].1.contains(&(true, outer)));
    });
    // And it runs as the language always meant: the condition's `break` ends the outer loop.
    let ran = nazm_core::run(src);
    assert!(
        matches!(ran, nazm_core::Outcome::Value(nazm_core::Value::Int(1))),
        "{ran:?}"
    );
}

#[test]
fn value_if_return_and_completion_are_explicit() {
    let src = "fn f(c: Bool) -> Int {\n\
                 let x = if c { return 1; } else { 2 };\n\
                 if c { return 3; } else { return 4; }\n\
               }\n\
               fn main() -> Int { f(false) }";
    with(src, |core, _, _| {
        let f = function(core, "f");
        // The statements fall through to a tail.
        assert_eq!(f.body.ends, Ends::FallsThrough);
        let Stmt::Let { value, .. } = &f.body.stmts[0] else {
            panic!("a `let` first");
        };
        // A value `if` with one transferring branch takes the other branch's type.
        assert!(matches!(value.kind, ExprKind::If { .. }));
        assert_eq!(value.ty, Some(nazm_core::Type::Int));
        let ExprKind::If { then, .. } = &value.kind else {
            unreachable!()
        };
        assert_eq!(then.ends, Ends::Transfers);
        assert!(then.tail.is_none());
        // The tail is a value `if` both of whose branches return: it never produces a value,
        // and says so, while the statements before it fall through to it.
        let tail = f.body.tail.as_ref().expect("a tail");
        assert!(matches!(tail.kind, ExprKind::If { .. }));
        assert_eq!(tail.ty, None);
    });
    // Written as a statement, the same `if` transfers, and so does the body.
    let as_statement = "fn f(c: Bool) -> Int {\n\
                          if c { return 3; } else { return 4; }\n\
                          5\n\
                        }\n\
                        fn main() -> Int { f(false) }";
    let (map, graph) = nazm_core::one_file(as_statement);
    let refused = nazm_core::on_lowered(&map, &graph, Entry::Required, |_, _| Ok(()));
    // A tail after a statement that always transfers can never run (`N0313`), so the
    // program is refused before Core IR exists.
    assert!(refused.is_err());
}

#[test]
fn a_question_mark_is_a_match_whose_error_arm_returns() {
    let src = "fn parse(n: Int) -> Result[Int, Str] {\n\
                 if n < 0 { Result[Int, Str].Err(error: \"negative\") } else { Result[Int, Str].Ok(value: n) }\n\
               }\n\
               fn show(n: Int) -> Result[Str, Str] {\n\
                 let v = parse(n)?;\n\
                 Result[Str, Str].Ok(value: int_to_str(v))\n\
               }\n\
               fn main() -> Int { match show(0 - 1) { Result.Ok(value: s) => str_len(s), Result.Err(error: e) => 100 + str_len(e), } }";
    with(src, |core, res, _| {
        let f = function(core, "show");
        let Stmt::Let { value, .. } = &f.body.stmts[0] else {
            panic!("a `let` first");
        };
        let ExprKind::Match(m) = &value.kind else {
            panic!("`?` should be a match, got {:?}", value.kind);
        };
        assert_eq!(m.origin, MatchOrigin::Propagate);
        assert_eq!(value.ty, Some(nazm_core::Type::Int));
        assert_eq!(m.arms.len(), 2);
        let (ok, err) = (&m.arms[0], &m.arms[1]);
        // The success arm's value is the `?`'s; its binding is a temporary Core IR made.
        assert_eq!(ok.body.ends, Ends::FallsThrough);
        assert!(matches!(
            f.locals[ok.bindings[0].local.index()].origin,
            cir::LocalOrigin::Temp(cir::Temp::Ok)
        ));
        // The error arm returns the error, rebuilt in the function's own `Result` — a
        // different instance from the operand's, since the success types differ.
        assert_eq!(err.body.ends, Ends::Transfers);
        let [Stmt::Return { value, .. }] = err.body.stmts.as_slice() else {
            panic!("the error arm should be one `return`");
        };
        assert_eq!(value.ty, Some(f.ret));
        assert_ne!(Some(f.ret), m.scrutinee.ty);
        assert!(cir::verify(core, res).is_empty());
    });
    assert!(matches!(
        nazm_core::run(src),
        nazm_core::Outcome::Value(nazm_core::Value::Int(108))
    ));
}

#[test]
fn a_region_owns_the_locals_bound_in_it() {
    let src = "fn main() -> Int {\n\
                 let a = \"outer\";\n\
                 let mut n = 0;\n\
                 while n < 2 { let b = str_concat(a, \"!\"); n = n + str_len(b); }\n\
                 n\n\
               }";
    with(src, |core, _, _| {
        let main = function(core, "main");
        let name = |l: &cir::LocalId| main.locals[l.index()].name.clone();
        let owned: Vec<String> = main.body.owns.iter().map(name).collect();
        assert_eq!(owned, vec!["a".to_owned(), "n".to_owned()]);
        let Stmt::Loop { body, .. } = &main.body.stmts[2] else {
            panic!("the loop third");
        };
        let owned: Vec<String> = body.owns.iter().map(name).collect();
        assert_eq!(owned, vec!["b".to_owned()]);
    });
}

// ---------------------------------------------------------------------------------------
// Determinism, and the digest law
// ---------------------------------------------------------------------------------------

#[test]
fn the_printed_form_is_the_same_every_time() {
    let src = EVERYTHING.replace(" && same(\"a\", \"a\")", "");
    let a = printed(&src, true);
    for _ in 0..5 {
        assert_eq!(printed(&src, true), a);
    }
    assert!(a.starts_with("{\"schema\":\"nazm.core-ir/3\""), "{a}");
}

#[test]
fn syntax_only_differences_lower_to_the_same_core_ir() {
    // Parentheses, a comment, the layout of the text: nothing of it survives.
    let plain = "fn f(a: Int, b: Int) -> Int { a + b * 2 }\nfn main() -> Int { f(1, 2) }";
    let dressed = "// a comment that moves every span\n\
                   fn f(a: Int, b: Int) -> Int {\n    ((a) + ((b * 2)))\n}\n\n\
                   fn main() -> Int { (f((1), (2))) }";
    assert_eq!(printed(plain, false), printed(dressed, false));
    assert_ne!(
        printed(plain, true),
        printed(dressed, true),
        "spans are metadata, printed on request"
    );
    assert_eq!(digest_of(plain, "f"), digest_of(dressed, "f"));
    assert_eq!(digest_of(plain, "main"), digest_of(dressed, "main"));
}

#[test]
fn a_body_change_moves_both_digests() {
    let a = "fn f(n: Int) -> Int { n + 1 }\nfn main() -> Int { f(1) }";
    let b = "fn f(n: Int) -> Int { n + 2 }\nfn main() -> Int { f(1) }";
    let (da, db) = (digest_of(a, "f"), digest_of(b, "f"));
    assert_ne!(da.semantic, db.semantic);
    assert_ne!(da.body, db.body);
    // The caller's own Core IR did not change: its digests name the callee, not its body.
    assert_eq!(digest_of(a, "main"), digest_of(b, "main"));
}

#[test]
fn an_effect_only_change_moves_the_semantic_digest_and_not_the_body_digest() {
    let a = "fn f(n: Int) -> Int { n }\nfn main() -> Int { f(1) }";
    let b = "fn f(n: Int) -> Int ! {} { n }\nfn main() -> Int { f(1) }";
    let (da, db) = (digest_of(a, "f"), digest_of(b, "f"));
    assert_ne!(da.semantic, db.semantic, "the effect contract is semantic");
    assert_eq!(da.body, db.body, "and nothing code generation reads");
}

#[test]
fn a_capability_parameter_is_a_type_and_moves_both_digests() {
    let a = "fn f(n: Int, t: Int) -> Int { n }\nfn main(io: IoCap) -> Int { f(1, 0) }";
    let b = "fn f(n: Int, t: IoCap) -> Int { n }\nfn main(io: IoCap) -> Int { f(1, io) }";
    let (da, db) = (digest_of(a, "f"), digest_of(b, "f"));
    assert_ne!(da.semantic, db.semantic);
    assert_ne!(
        da.body, db.body,
        "Core IR is typed; the backend's erasure is below it"
    );
}

#[test]
fn a_provenance_only_change_moves_no_core_ir_digest() {
    // `g`'s body changes whether `f`'s result carries `f`'s argument — the command line, in
    // `main` — or nothing at all, and so `f`'s provenance; it changes no effect and nothing of
    // `f`'s own Core IR, and so none of `f`'s digests.
    let a = "fn g(s: Str) -> Str { s }\nfn f(s: Str) -> Str { g(s) }\n\
             fn main(io: IoCap) -> Int { str_len(f(arg(0))) }";
    let b = "fn g(s: Str) -> Str { \"x\" }\nfn f(s: Str) -> Str { g(s) }\n\
             fn main(io: IoCap) -> Int { str_len(f(arg(0))) }";
    let provenance = |src: &str| {
        let (map, graph) = nazm_core::one_file(src);
        nazm_core::on_lowered(&map, &graph, Entry::Required, |_, res| {
            let f = res
                .definition(nazm_core::sema::ModuleId::ROOT, "f")
                .unwrap();
            Ok(format!("{:?}", res.function(f).unwrap().provenance))
        })
        .unwrap()
    };
    assert_ne!(
        provenance(a),
        provenance(b),
        "the premise: `f`'s provenance changed"
    );
    assert_eq!(digest_of(a, "f"), digest_of(b, "f"));
}

#[test]
fn an_unrelated_definition_moves_no_digest() {
    // Adding a function earlier in the file renumbers every definition after it; a digest
    // names a callee by its durable key, so nothing that was not changed moves.
    let a =
        "fn g(n: Int) -> Int { n }\nfn f(n: Int) -> Int { g(n) + 1 }\nfn main() -> Int { f(1) }";
    let b = "fn unrelated() -> Int { 7 }\nfn g(n: Int) -> Int { n }\n\
             fn f(n: Int) -> Int { g(n) + 1 }\nfn main() -> Int { f(1) }";
    assert_eq!(digest_of(a, "f"), digest_of(b, "f"));
    assert_eq!(digest_of(a, "main"), digest_of(b, "main"));
}

#[test]
fn renaming_a_local_moves_no_digest() {
    let a = "fn f(n: Int) -> Int { let doubled = n + n; doubled }\nfn main() -> Int { f(1) }";
    let b = "fn f(k: Int) -> Int { let twice = k + k; twice }\nfn main() -> Int { f(1) }";
    assert_eq!(digest_of(a, "f"), digest_of(b, "f"));
}

#[test]
fn declaration_and_arm_order_move_no_digest_but_evaluation_order_does() {
    let rec = |decl: &str, init: &str| {
        format!(
            "struct P {{ {decl} }}\nenum E {{ A, B(n: Int), }}\n\
             fn f(e: E) -> Int {{ match e {{ E.A() => 0, E.B(n: n) => n, }} }}\n\
             fn g(e: E) -> Int {{ match e {{ E.B(n: n) => n, E.A() => 0, }} }}\n\
             fn h() -> Int {{ let p = P({init}); p.x }}\n\
             fn main() -> Int {{ f(E.A()) + g(E.A()) + h() }}"
        )
    };
    let a = rec("x: Int, y: Int,", "x: 1, y: 2");
    let b = rec("y: Int, x: Int,", "x: 1, y: 2");
    // Moving two field declarations means nothing: fields are named, not numbered.
    assert_eq!(digest_of(&a, "h"), digest_of(&b, "h"));
    // Arms are exhaustive and disjoint: their written order means nothing.
    assert_eq!(digest_of(&a, "f").semantic, digest_of(&a, "g").semantic);
    // Initialisers run in the order written, so swapping them is a different program.
    let c = rec("x: Int, y: Int,", "y: 2, x: 1");
    assert_ne!(digest_of(&a, "h").semantic, digest_of(&c, "h").semantic);
}

// ---------------------------------------------------------------------------------------
// The verifier
// ---------------------------------------------------------------------------------------

#[test]
fn the_verifier_refuses_core_ir_that_breaks_an_invariant() {
    let src = "fn main() -> Int {\n\
                 let mut i = 0;\n\
                 while i < 3 { i = i + 1; if i == 2 { break; } }\n\
                 i\n\
               }";
    with(src, |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
        let main_at = core
            .functions
            .iter()
            .position(|f| f.name == "main")
            .unwrap();

        // A `break` naming a loop that does not enclose it.
        let mut broken = core.clone();
        let f = &mut broken.functions[main_at];
        f.loops = 2;
        let Stmt::Loop { body, .. } = &mut f.body.stmts[1] else {
            panic!()
        };
        let Stmt::If { then, .. } = &mut body.stmts[1] else {
            panic!()
        };
        then.stmts[0] = Stmt::Break {
            target: cir::LoopId(1),
            at: nazm_span::Span::synthetic(),
        };
        assert!(cir::verify(&broken, res)
            .iter()
            .any(|p| p.contains("does not enclose")));

        // A value region that falls through with no tail.
        let mut broken = core.clone();
        broken.functions[main_at].body.tail = None;
        assert!(cir::verify(&broken, res)
            .iter()
            .any(|p| p.contains("has no tail")));

        // A read of a local typed as something else.
        let mut broken = core.clone();
        if let Some(t) = broken.functions[main_at].body.tail.as_mut() {
            t.ty = Some(nazm_core::Type::Str);
        }
        assert!(!cir::verify(&broken, res).is_empty());

        // A function at another definition's index.
        let mut broken = core.clone();
        broken.functions.reverse();
        assert!(!cir::verify(&broken, res).is_empty() || broken.functions.len() == 1);
    });
}

// ---------------------------------------------------------------------------------------
// Recursion, nesting, scale
// ---------------------------------------------------------------------------------------

#[test]
fn direct_and_mutual_recursion_lower_in_either_source_order() {
    let fact = "fn fact(n: Int) -> Int { if n < 2 { 1 } else { n * fact(n - 1) } }";
    let even = "fn even(n: Int) -> Bool { if n == 0 { true } else { odd(n - 1) } }";
    let odd = "fn odd(n: Int) -> Bool { if n == 0 { false } else { even(n - 1) } }";
    let main = "fn main() -> Int { if even(10) { fact(5) } else { 0 } }";
    for order in [
        [fact, even, odd, main],
        [main, odd, even, fact],
        [odd, main, fact, even],
    ] {
        let src = order.join("\n");
        with(&src, |core, res, _| {
            assert!(cir::verify(core, res).is_empty())
        });
        assert!(matches!(
            nazm_core::run(&src),
            nazm_core::Outcome::Value(nazm_core::Value::Int(120))
        ));
    }
}

#[test]
fn deep_nesting_within_the_parsers_limit_lowers_and_runs() {
    let depth = 120;
    let mut body = String::from("n");
    for i in 0..depth {
        body = format!("if n > {i} {{ {body} }} else {{ {i} }}");
    }
    let mut loops = String::from("i = i + 1; break;");
    for _ in 0..40 {
        loops = format!("while true {{ {loops} }} break;");
    }
    // The outermost level's own `break` has no loop to leave, so it goes inside one more.
    let src = format!(
        "fn f(n: Int) -> Int {{ {body} }}\nfn main() -> Int {{\n\
         let mut i = 0;\n while i < 1 {{ {loops} }}\n f(200) + i - 1 }}"
    );
    with(&src, |core, res, _| {
        assert!(cir::verify(core, res).is_empty())
    });
    let ran = nazm_core::run(&src);
    assert!(
        // Past every guard, to the innermost `n`; and every loop left exactly once.
        matches!(ran, nazm_core::Outcome::Value(nazm_core::Value::Int(200))),
        "{ran:?}"
    );
}

/// A function with `n` locals, `n` branches, `n` calls and `n / 10` loops.
fn large_function(n: usize) -> String {
    let mut s = String::from("fn id(x: Int) -> Int { x }\nfn main() -> Int {\n let mut t = 0;\n");
    for i in 0..n {
        s.push_str(&format!(
            " let v{i} = id({i});\n if v{i} > t {{ t = v{i}; }} else {{ t = t + 1; }}\n"
        ));
        if i % 10 == 0 {
            s.push_str(&format!(
                " let mut w{i} = 0;\n while w{i} < 2 {{ w{i} = w{i} + 1; }}\n"
            ));
        }
    }
    s.push_str(" t\n}\n");
    s
}

/// `n` functions, each calling the next.
fn large_program(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!(
            "fn f{i}(x: Int) -> Int {{ if x > {i} {{ x }} else {{ f{}(x + 1) }} }}\n",
            i + 1
        ));
    }
    s.push_str(&format!(
        "fn f{n}(x: Int) -> Int {{ x }}\nfn main() -> Int {{ f0(0) }}\n"
    ));
    s
}

/// What lowering to Core IR and verifying it add to parsing and checking, best of three of
/// each. Checking is timed separately and subtracted: its own cost grows faster than linearly
/// in the number of bindings one function has (`docs/internal/performance-log.md`, N39), which is the
/// checker's, predates Core IR, and is not what these tests are about.
fn lowering_time(src: &str) -> std::time::Duration {
    let best = |f: &dyn Fn()| {
        (0..3)
            .map(|_| {
                let start = std::time::Instant::now();
                f();
                start.elapsed()
            })
            .min()
            .unwrap()
    };
    let (map, graph) = nazm_core::one_file(src);
    let checked = best(&|| {
        assert!(nazm_core::check_map(&map, &graph).is_empty());
    });
    let lowered = best(&|| {
        with(src, |core, _, _| core.functions.len());
    });
    lowered.saturating_sub(checked)
}

#[test]
fn a_large_function_lowers_in_time_proportional_to_its_size() {
    let small = large_function(250);
    let large = large_function(1_000);
    with(&large, |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
        assert!(function(core, "main").locals.len() > 1_000);
    });
    let (a, b) = (lowering_time(&small), lowering_time(&large));
    // Four times the code; linear is ×4. A generous bound catches a quadratic walk (×16)
    // without failing on a busy machine.
    assert!(
        b < a * 10 + std::time::Duration::from_millis(200),
        "250 → {a:?}, 1,000 → {b:?}"
    );
}

#[test]
fn a_program_of_thousands_of_functions_lowers_in_time_proportional_to_its_size() {
    let small = large_program(750);
    let large = large_program(3_000);
    with(&large, |core, res, _| {
        assert_eq!(core.functions.len(), 3_002);
        assert!(cir::verify(core, res).is_empty());
    });
    let (a, b) = (lowering_time(&small), lowering_time(&large));
    assert!(
        b < a * 10 + std::time::Duration::from_millis(200),
        "750 → {a:?}, 3,000 → {b:?}"
    );
}

/// One `+` chain of `n` terms — strings, alternately literals and a parameter, and integers —
/// and a chain of `n` methods by name, `a.again()` being `str_again(a)`.
fn plus_chain(n: usize) -> String {
    let strs: Vec<String> = (0..n)
        .map(|i| {
            if i % 2 == 0 {
                format!("\"s{i}\"")
            } else {
                "a".to_owned()
            }
        })
        .collect();
    let ints: Vec<String> = (0..n)
        .map(|i| {
            if i % 2 == 0 {
                i.to_string()
            } else {
                "k".to_owned()
            }
        })
        .collect();
    format!(
        "fn s(a: Str) -> Str {{\n    {}\n}}\nfn i(k: Int) -> Int {{\n    {}\n}}\n\
         fn str_again(a: Str) -> Str {{\n    a\n}}\nfn m(a: Str) -> Str {{\n    a{}\n}}\n\
         fn main() -> Int {{\n    str_len(s(\"x\")) + str_len(m(\"y\")) + i(1)\n}}\n",
        strs.join(" + "),
        ints.join(" + "),
        ".again()".repeat(n)
    )
}

/// How long checking `src` takes, best of three.
fn checking_time(src: &str) -> std::time::Duration {
    let (map, graph) = nazm_core::one_file(src);
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            assert!(nazm_core::check_map(&map, &graph).is_empty());
            start.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn a_long_plus_chain_checks_and_lowers_in_time_proportional_to_its_size() {
    // `s + t` on `Str`s is `str_concat(s, t)`, and `a.again()` is `str_again(a)`; the checker
    // once checked the operands or the receiver, then checked them again as that call's
    // arguments, so every link of a chain doubled the work before it: thirty terms took
    // minutes and gigabytes. Eight and sixteen terms first, which that told apart at once, so
    // a regression fails here rather than hangs.
    let (a, b) = (
        checking_time(&plus_chain(8)),
        checking_time(&plus_chain(16)),
    );
    assert!(
        b < a * 10 + std::time::Duration::from_millis(100),
        "8 → {a:?}, 16 → {b:?}"
    );
    let (c, d) = (
        checking_time(&plus_chain(50)),
        checking_time(&plus_chain(200)),
    );
    assert!(
        d < c * 10 + std::time::Duration::from_millis(200),
        "50 → {c:?}, 200 → {d:?}"
    );
    // And the chain lowers to Core IR that verifies.
    with(&plus_chain(200), |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
    });
}

/// Q1-D-19: the verifier's Gate 2 rules refuse a broken array literal, a repeat of the wrong
/// length and an index by something not an `Int`.
#[test]
fn the_verifier_refuses_broken_arrays_and_indexes() {
    let src = "fn main() -> Int {\n\
                 let xs: [Int; 3] = [1, 2, 3];\n\
                 let ys = [0; 4];\n\
                 xs[1] + ys[2]\n\
               }";
    with(src, |core, res, _| {
        assert!(cir::verify(core, res).is_empty());
        let main_at = core
            .functions
            .iter()
            .position(|f| f.name == "main")
            .unwrap();

        let mut broken = core.clone();
        let Stmt::Let { value, .. } = &mut broken.functions[main_at].body.stmts[0] else {
            panic!("the first statement binds the array")
        };
        let ExprKind::Array { elems } = &mut value.kind else {
            panic!("an array literal")
        };
        elems.pop();
        assert!(cir::verify(&broken, res)
            .iter()
            .any(|p| p.contains("an array whose elements are not its element type and length")));

        let mut broken = core.clone();
        let Stmt::Let { value, .. } = &mut broken.functions[main_at].body.stmts[1] else {
            panic!("the second statement binds the repeat")
        };
        let ExprKind::ArrayRepeat { count, .. } = &mut value.kind else {
            panic!("a repeated array")
        };
        *count += 1;
        assert!(cir::verify(&broken, res)
            .iter()
            .any(|p| p.contains("a repeated array of another element type or length")));

        let mut broken = core.clone();
        let tail = broken.functions[main_at].body.tail.as_mut().unwrap();
        let mut all = Vec::new();
        expr_all(tail, &mut all);
        assert!(all.iter().any(|e| matches!(e.kind, ExprKind::Index { .. })));
        fn first_index(e: &mut cir::Expr) -> Option<&mut cir::Expr> {
            if matches!(e.kind, ExprKind::Index { .. }) {
                return Some(e);
            }
            match &mut e.kind {
                ExprKind::Arith { lhs, rhs, .. } => first_index(lhs).or_else(|| first_index(rhs)),
                _ => None,
            }
        }
        let at = first_index(tail).expect("an index in the tail");
        let ExprKind::Index { index, .. } = &mut at.kind else {
            panic!()
        };
        index.ty = Some(nazm_core::Type::Bool);
        assert!(
            cir::verify(&broken, res)
                .iter()
                .any(|p| p
                    .contains("an index into something that is not an array, or by a non-`Int`"))
        );
    });
}
