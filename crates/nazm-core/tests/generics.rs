//! N11: first-order generics and `Vec[T]`, as the checker and the interpreter see them.
//!
//! `docs/spec.md`, *Generics*, is the law. These tests hold three of its claims in
//! particular: a generic definition is checked **once, parametrically** — whether it is
//! valid never depends on how it is used; type arguments come from the **arguments** and
//! from what is written, never from the expected result; and a type that could own
//! itself through a `Vec` is refused with a reason distinct from an impossible layout.
//!
//! Every refusal names its code **and its span**, because the Nazm-written checker is held
//! to the same pair and a test that checked only the code would let the two drift.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::{check_source, run, Outcome, Value};
use nazm_diag::codes;

fn value(src: &str) -> Value {
    match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    }
}

/// Every diagnostic, as `(code, start, end)`.
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

/// The span of the `n`th occurrence (from 0) of `needle` in `src`.
fn at(src: &str, needle: &str, n: usize) -> (u32, u32) {
    let mut from = 0;
    for _ in 0..n {
        from = src[from..].find(needle).unwrap() + from + 1;
    }
    let start = src[from..]
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?}"))
        + from;
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

/// The program fails with exactly one diagnostic, this code, at this text.
fn refused(src: &str, code: nazm_diag::Code, needle: &str, n: usize) {
    let got = diagnostics(src);
    let (start, end) = at(src, needle, n);
    assert_eq!(
        got,
        vec![(code.as_str().to_owned(), start, end)],
        "for {src}"
    );
}

const MAYBE: &str = "enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n";

// --- the language, working ----------------------------------------------------------------

#[test]
fn the_milestone_example_runs() {
    let src = "struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
               enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
               fn identity[T](value: T) -> T {\n    value\n}\n\
               fn main() -> Int {\n    let xs = vec_new[Pair[Int, Str]]();\n    \
               vec_push(xs, Pair[Int, Str](first: 7, second: \"nazm\"));\n    \
               let p = vec_get(xs, 0);\n    identity(p.first)\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
    assert_eq!(value(src), Value::Int(7));
}

#[test]
fn type_arguments_are_inferred_through_the_structure_of_a_parameter() {
    // `Vec[T]` against a `Vec[Point]` gives `T = Point`, and the result is a `Point`.
    let src = "struct Point { x: Int, }\n\
               fn first[T](xs: Vec[T]) -> T { vec_get(xs, 0) }\n\
               fn main() -> Int {\n    let ps = vec_new[Point]();\n    \
               vec_push(ps, Point(x: 41));\n    first(ps).x + 1\n}\n";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn explicit_type_arguments_fix_a_call() {
    let src = "fn identity[T](v: T) -> T { v }\n\
               fn main() -> Int { identity[Int](5) + str_len(identity[Str](\"abc\")) }\n";
    assert_eq!(value(src), Value::Int(8));
}

#[test]
fn a_generic_enum_is_matched_by_its_name_and_its_arguments_come_from_the_scrutinee() {
    let src = format!(
        "{MAYBE}fn unwrap_or[T](m: Maybe[T], d: T) -> T {{\n    match m {{\n        \
         Maybe.None() => d,\n        Maybe.Some(value: v) => v,\n    }}\n}}\n\
         fn main() -> Int {{\n    unwrap_or(Maybe[Int].Some(value: 40), 0) + \
         unwrap_or(Maybe[Int].None(), 2) + str_len(unwrap_or(Maybe[Str].None(), \"\"))\n}}\n"
    );
    assert_eq!(value(&src), Value::Int(42));
}

#[test]
fn a_generic_function_may_call_another_with_its_own_parameter() {
    let src = "struct Box[T] { value: T, }\n\
               fn wrap[T](x: T) -> Box[T] { Box[T](value: x) }\n\
               fn twice[T](x: T) -> Box[Box[T]] { wrap(wrap(x)) }\n\
               fn main() -> Int { twice(21).value.value * 2 }\n";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn one_application_is_one_type_wherever_it_is_written() {
    // `Pair[Int, Str]` written in a signature and again in a body is the same type: the
    // value passes between them with no conversion, because there is nothing to convert.
    let src = "struct Pair[A, B] { a: A, b: B, }\n\
               fn take(p: Pair[Int, Str]) -> Int { p.a + str_len(p.b) }\n\
               fn main() -> Int { let p = Pair[Int, Str](a: 1, b: \"xy\"); take(p) }\n";
    assert_eq!(value(src), Value::Int(3));
}

#[test]
fn renaming_a_parameter_changes_nothing_a_program_can_see() {
    let a = "fn pick[A, B](x: A, y: B) -> A { x }\nfn main() -> Int { pick(3, \"s\") }\n";
    let b = "fn pick[Left, Right](x: Left, y: Right) -> Left { x }\n\
             fn main() -> Int { pick(3, \"s\") }\n";
    assert_eq!(value(a), value(b));
}

#[test]
fn a_parameter_may_shadow_a_type_of_the_same_name_inside_its_declaration() {
    // Lexical: inside `id[Point]`, `Point` is the parameter.
    let src = "struct Point { x: Int, }\n\
               fn id[Point](p: Point) -> Point { p }\n\
               fn main() -> Int { id(5) }\n";
    assert_eq!(value(src), Value::Int(5));
}

#[test]
fn a_generic_definition_nothing_calls_is_still_checked() {
    // Parametric checking means a body is valid for every type or refused — and so it is
    // checked whether or not anything instantiates it.
    let src = "fn broken[T](x: T) -> Int { x + 1 }\nfn main() -> Int { 0 }\n";
    refused(src, codes::INVALID_OPERANDS, "+", 0);
}

#[test]
fn a_generic_body_is_checked_once_whatever_calls_it() {
    // Three calls at three types, one mistake in the body: one diagnostic, at the body.
    let src = "fn broken[T](x: T) -> T { let y = x.field; x }\n\
               fn main() -> Int { broken(1); broken(\"s\"); broken(true); 0 }\n";
    refused(src, codes::INVALID_PROJECTION, "field", 0);
}

// --- what an unconstrained `T` may not do ----------------------------------------------------

#[test]
fn a_type_parameter_supports_no_equality() {
    // No constraint says "types with `==`", so a definition that needs one is refused
    // where it is written — not accepted because every current caller passes an `Int`.
    let src = "fn equal[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }\n";
    refused(src, codes::INVALID_OPERANDS, "==", 0);
}

#[test]
fn a_type_parameter_supports_no_arithmetic_projection_match_or_condition() {
    refused(
        "fn f[T](a: T) -> Int { a * 2 }\nfn main() -> Int { 0 }\n",
        codes::INVALID_OPERANDS,
        "*",
        0,
    );
    refused(
        "fn f[T](value: T) -> Int { match value { } }\nfn main() -> Int { 0 }\n",
        codes::MATCH_ON_NON_ENUM,
        "value",
        1,
    );
    refused(
        "fn f[T](value: T) -> Int { if value { 1 } else { 0 } }\nfn main() -> Int { 0 }\n",
        codes::CONDITION_NOT_BOOL,
        "value",
        1,
    );
}

#[test]
fn a_type_parameter_can_reach_a_task_only_as_a_mismatch() {
    // A task runs a non-generic function, whose parameters cannot name a type parameter —
    // so a `T` can never be given to one. The rule that a parameter is not task-safe is
    // held by construction here, and a generic body may still spawn ordinary work.
    let src = "fn w2(n: Int) -> Int { 0 }\n\
               fn g[T](held: T, tasks: SpawnCap) -> Int { scope { spawn w2(0); } 0 }\n\
               fn main(tasks: SpawnCap) -> Int { g(\"s\", tasks) }\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
    // The argument is the wrong type for `take`, and it is also a value no task may be
    // given: both rules apply to it, exactly as they do to an `Ints` handed to an `Int`
    // parameter — the capture rule reads the argument's type, not the parameter's.
    let spawn_t = "fn run[T](held: T, tasks: SpawnCap) -> Int { scope { spawn take(held); } 0 }\n\
                   fn take(n: Int) -> Int { 0 }\n\
                   fn main() -> Int { 0 }\n";
    let (start, end) = at(spawn_t, "held", 1);
    assert_eq!(
        diagnostics(spawn_t),
        vec![
            ("N0300".to_owned(), start, end),
            (codes::ILLEGAL_TASK_CAPTURE.as_str().to_owned(), start, end)
        ]
    );
}

#[test]
fn a_task_runs_a_non_generic_function() {
    let src = "fn work[T](x: T) -> Int { 0 }\n\
               fn main(tasks: SpawnCap) -> Int { scope { spawn work(1); } 0 }\n";
    refused(src, codes::UNSUPPORTED_CONSTRUCT, "work", 1);
}

#[test]
fn a_vec_never_crosses_into_a_task_whatever_it_holds() {
    let src = "fn w(x: Vec[Str]) -> Int { 0 }\n\
               fn main(tasks: SpawnCap) -> Int { let held = vec_new[Str](); scope { spawn w(held); } 0 }\n";
    refused(src, codes::ILLEGAL_TASK_CAPTURE, "held", 1);
    // And a record holding one inherits that, through an application.
    let wrapped = "struct Env[T] { value: T, }\n\
                   fn w(x: Env[Vec[Int]]) -> Int { 0 }\n\
                   fn main(tasks: SpawnCap) -> Int { let held = Env[Vec[Int]](value: vec_new[Int]()); \
                   scope { spawn w(held); } 0 }\n";
    refused(wrapped, codes::ILLEGAL_TASK_CAPTURE, "held", 1);
    // A generic record of task-safe fields is task-safe.
    let fine = "struct Env[T] { value: T, }\n\
                fn w(x: Env[Str]) -> Int { 0 }\n\
                fn main(tasks: SpawnCap) -> Int { scope { spawn w(Env[Str](value: \"s\")); } 0 }\n";
    assert!(diagnostics(fine).is_empty(), "{:?}", diagnostics(fine));
}

#[test]
fn a_vec_supports_no_equality() {
    let src = "fn main() -> Int { let a = vec_new[Int](); if a == a { 1 } else { 0 } }\n";
    refused(src, codes::INVALID_OPERANDS, "==", 0);
}

// --- declaring parameters -----------------------------------------------------------------

#[test]
fn a_parameter_list_may_not_name_one_parameter_twice() {
    let src = "fn f[T, T](x: T) -> T { x }\nfn main() -> Int { 0 }\n";
    refused(src, codes::DUPLICATE_TYPE_PARAMETER, "T", 1);
}

#[test]
fn a_parameter_may_not_be_a_built_in_types_name_or_vec() {
    refused(
        "fn f[Int](x: Int) -> Int { x }\nfn main() -> Int { 0 }\n",
        codes::BUILTIN_TYPE_REDEFINED,
        "Int",
        0,
    );
    refused(
        "struct B[Vec] { v: Int, }\nfn main() -> Int { 0 }\n",
        codes::BUILTIN_TYPE_REDEFINED,
        "Vec",
        0,
    );
}

#[test]
fn a_parameter_is_in_scope_only_in_its_own_declaration() {
    let src = "fn a[T](x: T) -> T { x }\nfn b(y: T) -> Int { 0 }\nfn main() -> Int { 0 }\n";
    refused(src, codes::UNKNOWN_TYPE, "T", 3);
}

#[test]
fn generic_main_is_refused() {
    refused(
        "fn main[T]() -> Int { 0 }\n",
        codes::GENERIC_MAIN,
        "main",
        0,
    );
}

// --- applying types ----------------------------------------------------------------------

#[test]
fn type_arguments_on_a_definition_that_takes_none_are_refused() {
    refused(
        "struct P { x: Int, }\nfn f(p: P[Int]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::TYPE_ARGUMENTS_ON_NON_GENERIC,
        "P[Int]",
        0,
    );
    refused(
        "fn f(p: Int[Str]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::TYPE_ARGUMENTS_ON_NON_GENERIC,
        "Int[Str]",
        0,
    );
    refused(
        "fn f[T](x: T[Int]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::TYPE_ARGUMENTS_ON_NON_GENERIC,
        "T[Int]",
        0,
    );
    refused(
        "fn g(x: Int) -> Int { x }\nfn main() -> Int { g[Int](1) }\n",
        codes::TYPE_ARGUMENTS_ON_NON_GENERIC,
        "g",
        1,
    );
}

#[test]
fn a_generic_type_written_without_its_arguments_is_refused() {
    refused(
        "struct B[T] { v: T, }\nfn f(p: B) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::MISSING_TYPE_ARGUMENTS,
        "B",
        1,
    );
    refused(
        "fn f(p: Vec) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::MISSING_TYPE_ARGUMENTS,
        "Vec",
        0,
    );
    // A construction infers nothing: its arguments are written.
    let construct = "struct B[T] { v: T, }\nfn main() -> Int { let b = B(v: 1); 0 }\n";
    refused(construct, codes::MISSING_TYPE_ARGUMENTS, "B", 1);
}

#[test]
fn the_wrong_number_of_type_arguments_is_one_error_wherever_it_is_written() {
    refused(
        "struct P[A, B] { a: A, b: B, }\nfn f(p: P[Int]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        "P[Int]",
        0,
    );
    refused(
        "struct P[A, B] { a: A, b: B, }\n\
         fn f(p: P[Int, Str, Bool]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        "P[Int, Str, Bool]",
        0,
    );
    refused(
        "fn f(p: Vec[]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        "Vec[]",
        0,
    );
    refused(
        "fn f(p: Vec[Int, Str]) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        "Vec[Int, Str]",
        0,
    );
    refused(
        "fn id[T](x: T) -> T { x }\nfn main() -> Int { id[Int, Str](1) }\n",
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        "id",
        1,
    );
}

#[test]
fn a_written_type_may_nest_thirty_two_deep() {
    let deep = |n: usize| format!("{}Int{}", "Vec[".repeat(n), "]".repeat(n));
    let fits = format!(
        "fn f(x: {}) -> Int {{ 0 }}\nfn main() -> Int {{ 0 }}\n",
        deep(31)
    );
    assert!(diagnostics(&fits).is_empty(), "{:?}", diagnostics(&fits));
    let written = deep(32);
    let over = format!("fn f(x: {written}) -> Int {{ 0 }}\nfn main() -> Int {{ 0 }}\n");
    refused(&over, codes::TYPE_NESTING_TOO_DEEP, &written, 0);
}

// --- inference ---------------------------------------------------------------------------

#[test]
fn the_expected_result_never_supplies_a_type_argument() {
    // `vec_new()` cannot learn `T` from what its result is bound to, and neither can any
    // other generic function whose parameter appears only in its result.
    refused(
        "fn main() -> Int { let v = vec_new(); 0 }\n",
        codes::CANNOT_INFER_TYPE_ARGUMENT,
        "vec_new",
        0,
    );
    refused(
        "fn make[T](n: Int) -> Vec[T] { vec_new[T]() }\n\
         fn main() -> Int { vec_len(make(3)) }\n",
        codes::CANNOT_INFER_TYPE_ARGUMENT,
        "make",
        1,
    );
}

#[test]
fn two_arguments_that_disagree_about_a_parameter_are_a_conflict_not_a_choice() {
    let src = "fn same[T](a: T, b: T) -> T { a }\nfn main() -> Int { same(1, \"x\") }\n";
    refused(src, codes::CONFLICTING_TYPE_ARGUMENTS, "\"x\"", 0);
    // Through structure too: the first argument says `Int` and the second's element says
    // `Str`.
    let nested = "fn same[T](a: T, b: Vec[T]) -> T { a }\n\
                  fn main() -> Int { same(1, vec_new[Str]()) }\n";
    refused(
        nested,
        codes::CONFLICTING_TYPE_ARGUMENTS,
        "vec_new[Str]()",
        0,
    );
}

#[test]
fn an_argument_that_disagrees_with_written_type_arguments_is_an_ordinary_mismatch() {
    let src = "fn id[T](x: T) -> Int { 0 }\nfn main() -> Int { id[Str](1) }\n";
    refused(src, codes::TYPE_MISMATCH, "1", 0);
}

#[test]
fn an_argument_of_the_wrong_shape_is_a_mismatch_against_what_is_known() {
    let src = "fn first[T](xs: Vec[T]) -> T { vec_get(xs, 0) }\nfn main() -> Int { first(3) }\n";
    refused(src, codes::TYPE_MISMATCH, "3", 0);
}

// --- generic records and enums -----------------------------------------------------------

#[test]
fn two_generic_records_with_one_shape_are_two_types() {
    let src = "struct A[T] { value: T, }\nstruct B[T] { value: T, }\n\
               fn take(a: A[Int]) -> Int { a.value }\n\
               fn main() -> Int { take(B[Int](value: 1)) }\n";
    refused(src, codes::TYPE_MISMATCH, "B[Int](value: 1)", 0);
}

#[test]
fn a_pattern_names_the_scrutinees_own_generic_enum() {
    let src = "enum A[T] { X(v: T), }\nenum B[T] { X(v: T), }\n\
               fn main() -> Int { match A[Int].X(v: 1) { B.X(v: n) => n, } }\n";
    refused(src, codes::WRONG_ENUM_IN_PATTERN, "B", 1);
}

#[test]
fn every_variant_of_a_generic_enum_gets_an_arm_at_every_instantiation() {
    let src =
        format!("{MAYBE}fn main() -> Int {{ match Maybe[Str].None() {{ Maybe.None() => 0, }} }}\n");
    let got = diagnostics(&src);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "N0343");
}

#[test]
fn a_public_generic_signature_may_not_expose_a_private_type_in_an_argument() {
    let src = "struct Secret { x: Int, }\npub struct Box[T] { v: T, }\n\
               pub fn f(b: Box[Secret]) -> Int { 0 }\nfn main() -> Int { 0 }\n";
    refused(src, codes::PRIVATE_TYPE_EXPOSED, "Box[Secret]", 0);
}

// --- ownership cycles --------------------------------------------------------------------

#[test]
fn a_type_that_can_own_itself_through_a_vec_is_refused() {
    refused(
        "struct Node { children: Vec[Node], }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "Node",
        0,
    );
    refused(
        "struct A { bs: Vec[B], }\nstruct B { as: Vec[A], }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "A",
        0,
    );
    refused(
        "enum Tree { Leaf, Node(kids: Vec[Tree]), }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "Tree",
        0,
    );
    refused(
        "struct Tree[T] { kids: Vec[Tree[T]], v: T, }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "Tree",
        0,
    );
    // Through a generic record whose parameter a `Vec` holds.
    refused(
        "struct Holder[T] { items: Vec[T], }\nstruct Node { h: Holder[Node], }\n\
         fn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "Node",
        0,
    );
    // Through a record, an enum and a `Vec`.
    refused(
        "struct R { e: E, }\nenum E { Many(rs: Vec[R]), None, }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "R",
        0,
    );
    // Expanding without bound, which is also a cycle: refused, and never an endless
    // family of instances.
    refused(
        "struct Strange[T] { next: Vec[Strange[Vec[T]]], }\nfn main() -> Int { 0 }\n",
        codes::OWNERSHIP_CYCLE,
        "Strange",
        0,
    );
}

#[test]
fn an_inline_cycle_through_a_generic_type_is_still_an_impossible_layout() {
    refused(
        "struct Box[T] { v: T, }\nstruct Loop { a: Box[Loop], }\nfn main() -> Int { 0 }\n",
        codes::RECURSIVE_RECORD,
        "Loop",
        0,
    );
}

#[test]
fn acyclic_nested_containers_are_not_refused() {
    let src = "struct Point { x: Int, }\nstruct Pair[A, B] { a: A, b: B, }\n\
               struct Grid { rows: Vec[Vec[Int]], names: Vec[Vec[Str]], ps: Vec[Point], }\n\
               enum E { Words(ws: Vec[Str]), Pairs(ps: Vec[Pair[Int, Str]]), }\n\
               struct Tag[T] { n: Int, }\nstruct R { t: Tag[R], }\n\
               fn main() -> Int { 0 }\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}

// --- `Vec[T]`, interpreted ---------------------------------------------------------------

#[test]
fn a_vec_is_a_handle_and_an_alias_sees_every_push() {
    let src = "fn main() -> Int {\n    let a = vec_new[Int]();\n    let b = a;\n    \
               vec_push(a, 1);\n    vec_push(b, 2);\n    vec_len(a) * 10 + vec_get(a, 1)\n}\n";
    assert_eq!(value(src), Value::Int(22));
}

#[test]
fn the_vec_operations_return_what_the_sequence_operations_return() {
    // `vec_push` the new length, `vec_set` the index written, `vec_pop` the element.
    let src = "fn main() -> Int {\n    let xs = vec_new[Int]();\n    \
               let n = vec_push(xs, 5) + vec_push(xs, 6);\n    let i = vec_set(xs, 1, 9);\n    \
               let p = vec_pop(xs);\n    n * 100 + i * 10 + p + vec_len(xs)\n}\n";
    assert_eq!(value(src), Value::Int(320));
}

#[test]
fn a_vec_index_outside_it_fails_with_the_sequence_code() {
    for (src, needle) in [
        (
            "fn main() -> Int { vec_get(vec_new[Int](), 0) }\n",
            "vec_get",
        ),
        (
            "fn main() -> Int { vec_set(vec_new[Int](), 0, 1) }\n",
            "vec_set",
        ),
        ("fn main() -> Int { vec_pop(vec_new[Int]()) }\n", "vec_pop"),
    ] {
        match run(src) {
            Outcome::RuntimeFailed(d) => {
                assert_eq!(d.code, codes::INDEX_OUT_OF_BOUNDS, "{src}");
                let (start, _) = at(src, needle, 0);
                assert_eq!(d.primary.span.start, start, "{src}");
            }
            other => panic!("expected a failure from {src}, got {other:?}"),
        }
    }
}

#[test]
fn a_type_named_vec_cannot_be_declared() {
    // A type written `Vec` is always the built-in, so a record declared under that name
    // could be built and never named — refused as every built-in type's name is.
    refused(
        "struct Vec { x: Int, }\nfn main() -> Int { 0 }",
        codes::BUILTIN_TYPE_REDEFINED,
        "Vec",
        0,
    );
    refused(
        "enum Vec { A, }\nfn main() -> Int { 0 }",
        codes::BUILTIN_TYPE_REDEFINED,
        "Vec",
        0,
    );
}

#[test]
fn inference_does_not_unify_two_generic_definitions_of_one_shape() {
    // Found by mutation: an `A[Int]` passed where `B[T]` is declared settled `T = Int` and
    // was accepted, because nothing compared the definitions — two generic records with the
    // same fields and arity are two types, before and after inference.
    refused(
        "struct A[T] { v: T, }\nstruct B[T] { v: T, }\n\
         fn un[T](b: B[T]) -> T { b.v }\n\
         fn main() -> Int { un(A[Int](v: 1)) }",
        codes::TYPE_MISMATCH,
        "A[Int](v: 1)",
        0,
    );
}
