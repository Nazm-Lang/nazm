//! N14: a generic definition may require equality of a type parameter.
//!
//! `docs/spec.md`, *A type parameter may require equality*, is the law. What these tests
//! hold:
//!
//! * `==` on an unconstrained `T` is still refused, and on a `T` whose declaration says
//!   `T: Equality` it is accepted — and so is every composite whose equality derives from
//!   it (`Box[T]`, `Option[T]`, `Result[T, E]` with both required), by N13's one derivation;
//! * every type argument, written or inferred, is checked against the requirement by that
//!   same derivation, so a concrete type satisfies it exactly when it has equality, and a
//!   caller's own parameter exactly when its declaration requires it;
//! * the body is checked once, the requirement is static, and it gives no new meaning to
//!   `==` — two parameters are still two types, and handles still have no equality.
//!
//! Every refusal names its code **and its span**, the pair the Nazm-written checker is held
//! to.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::{check_source, run, Outcome, Value};
use nazm_diag::codes;

fn int(src: &str) -> i64 {
    match run(src) {
        Outcome::Value(Value::Int(n)) => n,
        other => panic!("expected an Int, got {other:?}\n{src}"),
    }
}

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

/// The span of `needle` at the start of the first `prefix` + `needle` in `src`.
fn after(src: &str, prefix: &str, needle: &str) -> (u32, u32) {
    let start = src
        .find(&format!("{prefix}{needle}"))
        .unwrap_or_else(|| panic!("no {prefix:?}{needle:?}"))
        + prefix.len();
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

/// The program fails with exactly one diagnostic, this code, at this span.
fn refused_at(src: &str, code: nazm_diag::Code, span: (u32, u32)) {
    assert_eq!(
        diagnostics(src),
        vec![(code.as_str().to_owned(), span.0, span.1)],
        "for {src}"
    );
}

/// The program fails with exactly one diagnostic, this code, at the `n`th `needle`.
fn refused(src: &str, code: nazm_diag::Code, needle: &str, n: usize) {
    let (start, end) = at(src, needle, n);
    assert_eq!(
        diagnostics(src),
        vec![(code.as_str().to_owned(), start, end)],
        "for {src}"
    );
}

fn accepted(src: &str) {
    assert!(diagnostics(src).is_empty(), "{:?}\n{src}", diagnostics(src));
}

fn message(src: &str) -> String {
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:?}");
    d[0].message.clone()
}

const SAME: &str = "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n";
const BOX: &str = "struct Box[T] { value: T, }\n";
const POINT: &str = "struct Point { x: Int, y: Int, }\n";

/// `1` when `expr` holds, after `decls` and `SAME`.
fn holds(decls: &str, expr: &str) -> i64 {
    int(&format!(
        "{SAME}{decls}\nfn main() -> Int {{ if {expr} {{ 1 }} else {{ 0 }} }}\n"
    ))
}

// --- the body ------------------------------------------------------------------------------------

#[test]
fn an_unconstrained_parameter_still_has_no_equality() {
    let src = "fn same[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }\n";
    refused(src, codes::INVALID_OPERANDS, "==", 0);
    // And a caller that only ever passes an `Int` does not change what the body may do.
    let src = "fn same[T](a: T, b: T) -> Bool { a == b }\n\
               fn main() -> Int { if same(1, 1) { 1 } else { 0 } }\n";
    refused(src, codes::INVALID_OPERANDS, "==", 0);
    // The refusal says how to lift it.
    let d = check_source("fn same[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }\n");
    let help = d[0].help.clone().unwrap_or_default();
    assert!(help.contains("T: Equality"), "{help}");
}

#[test]
fn a_required_parameter_has_equality_in_its_body() {
    accepted(&format!("{SAME}fn main() -> Int {{ 0 }}\n"));
    accepted("fn differ[T: Equality](a: T, b: T) -> Bool { a != b }\nfn main() -> Int { 0 }\n");
}

#[test]
fn a_composite_of_a_required_parameter_has_equality_too() {
    // Derived by N13's rule under what the body may assume: `Box[T]` has equality because
    // its one field is a `T` that does.
    accepted(&format!(
        "{BOX}fn box_same[T: Equality](a: Box[T], b: Box[T]) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}\n"
    ));
    accepted("fn opt[T: Equality](a: Option[T], b: Option[T]) -> Bool { a == b }\nfn main() -> Int { 0 }\n");
    accepted("fn res[T: Equality, E: Equality](a: Result[T, E], b: Result[T, E]) -> Bool { a == b }\nfn main() -> Int { 0 }\n");
    accepted(&format!(
        "{BOX}struct Outer[T] {{ inner: Box[T], tag: Result[T, Str], }}\n\
         enum Layer[T] {{ Empty, Held(b: Box[Option[T]]), }}\n\
         fn deep[T: Equality](a: Layer[T], b: Layer[T], c: Outer[T], d: Outer[T], \
         e: Result[Box[T], Str], f: Result[Box[T], Str]) -> Bool {{ if a == b {{ c == d }} else {{ e == f }} }}\n\
         fn main() -> Int {{ 0 }}\n"
    ));
}

#[test]
fn a_composite_needs_every_parameter_it_contains_to_be_required() {
    // `E` is unconstrained, so `Result[T, E]` has no equality — nothing is inferred from
    // the composite.
    let src = "fn f[T: Equality, E](a: Result[T, E], b: Result[T, E]) -> Bool { a == b }\nfn main() -> Int { 0 }\n";
    refused(src, codes::INVALID_OPERANDS, "==", 0);
    assert!(message(src).contains("`Err.error`"), "{}", message(src));
    let src = format!(
        "{BOX}fn f[T](a: Box[Option[T]], b: Box[Option[T]]) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}\n"
    );
    refused(&src, codes::INVALID_OPERANDS, "==", 0);
    // A handle stays a handle whatever it holds.
    refused(
        "fn f[T: Equality](a: Vec[T], b: Vec[T]) -> Bool { a == b }\nfn main() -> Int { 0 }\n",
        codes::INVALID_OPERANDS,
        "==",
        0,
    );
}

#[test]
fn two_required_parameters_are_still_two_types() {
    refused(
        "fn wrong[A: Equality, B: Equality](a: A, b: B) -> Bool { a == b }\nfn main() -> Int { 0 }\n",
        codes::INVALID_OPERANDS,
        "==",
        0,
    );
    // Each on its own, as many as the definition has.
    accepted(
        "fn both[A: Equality, B: Equality](a1: A, a2: A, b1: B, b2: B) -> Bool { if a1 == a2 { b1 == b2 } else { false } }\n\
         fn main() -> Int { 0 }\n",
    );
}

// --- the arguments -------------------------------------------------------------------------------

#[test]
fn a_concrete_argument_satisfies_the_requirement_exactly_when_it_has_equality() {
    let decls = format!("{BOX}{POINT}enum ErrorCode {{ Missing, Bad(n: Int), }}\n");
    assert_eq!(holds(&decls, "same(1, 1)"), 1);
    assert_eq!(holds(&decls, "same[Int](1, 2)"), 0);
    assert_eq!(holds(&decls, "same(str_concat(\"a\", \"b\"), \"ab\")"), 1);
    assert_eq!(
        holds(&decls, "same(Point(x: 1, y: 2), Point(x: 1, y: 2))"),
        1
    );
    assert_eq!(
        holds(
            &decls,
            "same(Option[Int].Some(value: 1), Option[Int].None())"
        ),
        0
    );
    assert_eq!(
        holds(&decls, "same(Box[Int](value: 3), Box[Int](value: 3))"),
        1
    );
    assert_eq!(holds(&decls, "same(Option[Point].Some(value: Point(x: 1, y: 1)), Option[Point].Some(value: Point(x: 1, y: 1)))"), 1);
    assert_eq!(
        holds(
            &decls,
            "same(Result[Int, Str].Ok(value: 1), Result[Int, Str].Ok(value: 1))"
        ),
        1
    );
    assert_eq!(
        holds(&decls, "same(Result[Point, ErrorCode].Err(error: ErrorCode.Bad(n: 1)), Result[Point, ErrorCode].Err(error: ErrorCode.Bad(n: 2)))"),
        0
    );
}

#[test]
fn a_concrete_argument_without_equality_is_refused_where_it_was_given() {
    // Inferred: at the argument it was inferred from.
    for (decls, make) in [
        ("", "vec_new[Int]()"),
        ("", "ints_new()"),
        ("", "strs_new()"),
        ("", "chan_new(1)"),
        (BOX, "Box[Vec[Int]](value: vec_new[Int]())"),
        ("", "Option[Vec[Int]].None()"),
        (
            "struct Diag { code: Str, }\n",
            "Result[Int, Vec[Diag]].Ok(value: 1)",
        ),
        ("struct Held { ch: Chan, }\n", "Held(ch: chan_new(1))"),
        (
            "enum Work { Empty, Values(value: Vec[Int]), }\n",
            "Work.Empty()",
        ),
    ] {
        let src = format!(
            "{SAME}{decls}fn main() -> Int {{ let a = {make}; if same(a, a) {{ 1 }} else {{ 0 }} }}\n"
        );
        refused_at(
            &src,
            codes::UNSATISFIED_REQUIREMENT,
            after(&src, "if same(", "a"),
        );
    }
}

#[test]
fn a_written_argument_without_equality_is_refused_at_the_callee() {
    let src = format!(
        "{SAME}{BOX}fn main() -> Int {{ let a = Box[Vec[Int]](value: vec_new[Int]()); if same[Box[Vec[Int]]](a, a) {{ 1 }} else {{ 0 }} }}\n"
    );
    refused(&src, codes::UNSATISFIED_REQUIREMENT, "same", 1);
    let m = message(&src);
    assert!(
        m.contains("`value`") && m.contains("`Ints`"), /* `Vec[Int]`, R1 5b.18 */
        "{m}"
    );
}

#[test]
fn a_requirement_is_satisfied_by_a_type_never_by_a_value() {
    // `Work.Empty()` holds no `Vec`, and `Work` still has none of equality.
    let src = format!(
        "{SAME}enum Work {{ Empty, Values(value: Vec[Int]), }}\n\
         fn main() -> Int {{ if same(Work.Empty(), Work.Empty()) {{ 1 }} else {{ 0 }} }}\n"
    );
    refused(&src, codes::UNSATISFIED_REQUIREMENT, "Work.Empty()", 0);
}

#[test]
fn every_required_parameter_is_checked_not_only_the_first() {
    let src = "fn both[A: Equality, B: Equality](a: A, b: B) -> Int { 0 }\n\
               fn main() -> Int { both(1, vec_new[Int]()) }\n";
    refused(src, codes::UNSATISFIED_REQUIREMENT, "vec_new[Int]()", 0);
    // An unconstrained parameter beside a required one takes anything.
    accepted("fn pick[A: Equality, B](a: A, b: B) -> Int { 0 }\nfn main() -> Int { pick(1, vec_new[Int]()) }\n");
}

// --- forwarding ---------------------------------------------------------------------------------

#[test]
fn a_generic_caller_may_pass_on_only_what_it_requires_itself() {
    let ok = format!(
        "{SAME}fn wrapper[T: Equality](a: T, b: T) -> Bool {{ same(a, b) }}\n\
         fn main() -> Int {{ if wrapper(Option[Int].Some(value: 2), Option[Int].Some(value: 2)) {{ 1 }} else {{ 0 }} }}\n"
    );
    assert_eq!(int(&ok), 1);
    let bad = format!(
        "{SAME}fn wrapper[T](a: T, b: T) -> Bool {{ same(a, b) }}\nfn main() -> Int {{ 0 }}\n"
    );
    refused_at(
        &bad,
        codes::UNSATISFIED_REQUIREMENT,
        after(&bad, "{ same(", "a"),
    );
    assert!(
        message(&bad).contains("type parameter"),
        "{}",
        message(&bad)
    );
    // Forwarding a composite of a required parameter is forwarding what it derives from.
    let boxed = format!(
        "{SAME}{BOX}fn wrap_box[T: Equality](a: Box[T]) -> Bool {{ same(a, a) }}\nfn main() -> Int {{ 0 }}\n"
    );
    accepted(&boxed);
    let unboxed = format!(
        "{SAME}{BOX}fn wrap_box[T](a: Box[T]) -> Bool {{ same(a, a) }}\nfn main() -> Int {{ 0 }}\n"
    );
    refused_at(
        &unboxed,
        codes::UNSATISFIED_REQUIREMENT,
        after(&unboxed, "{ same(", "a"),
    );
}

// --- what may be written ------------------------------------------------------------------------

#[test]
fn equality_is_the_only_capability_and_only_a_function_may_require_it() {
    refused(
        "fn f[T: Ord](a: T) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::UNKNOWN_REQUIREMENT,
        "Ord",
        0,
    );
    // A record named `Equality` is a type, and a requirement is not a type position.
    refused(
        "struct Eq { x: Int, }\nfn f[T: Eq](a: T) -> Int { 0 }\nfn main() -> Int { 0 }\n",
        codes::UNKNOWN_REQUIREMENT,
        "Eq",
        1,
    );
    accepted("struct Equality { x: Int, }\nfn f[T: Equality](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { Equality(x: 1).x }\n");
    refused(
        "struct B[T: Equality] { v: T, }\nfn main() -> Int { 0 }\n",
        codes::UNSUPPORTED_CONSTRUCT,
        "Equality",
        0,
    );
    refused(
        "enum E[T: Equality] { A(v: T), }\nfn main() -> Int { 0 }\n",
        codes::UNSUPPORTED_CONSTRUCT,
        "Equality",
        0,
    );
}

#[test]
fn a_requirement_belongs_to_the_position_not_the_name() {
    // Renamed, the same program means the same thing.
    let a = format!("{SAME}fn main() -> Int {{ if same(1, 1) {{ 1 }} else {{ 0 }} }}\n");
    let b = a.replace(
        "[T: Equality](a: T, b: T)",
        "[Value: Equality](a: Value, b: Value)",
    );
    assert_ne!(a, b);
    assert_eq!(int(&a), int(&b));
    // And a parameter elsewhere with the same name is not required by this one's bound.
    let src =
        format!("{SAME}fn other[T](a: T, b: T) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}\n");
    refused(&src, codes::INVALID_OPERANDS, "==", 1);
}

#[test]
fn a_bounded_call_evaluates_as_ordinary_equality() {
    let src = format!(
        "{SAME}fn mark(log: Ints, n: Int) -> Int {{ ints_push(log, n); n }}\n\
         fn main() -> Int {{ let log = ints_new(); let s = same(mark(log, 1), mark(log, 2)); \
         let n = if s {{ 100 }} else {{ 0 }}; n + ints_get(log, 0) * 10 + ints_get(log, 1) }}\n"
    );
    assert_eq!(int(&src), 12);
}
