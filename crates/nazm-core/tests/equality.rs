//! N13: built-in equality, derived structurally for the value types that can have it.
//!
//! `docs/spec.md`, *Equality is derived*, is the law. What these tests hold:
//!
//! * `Int`, `Bool` and `Str` keep exactly the equality they had — a `Str` by its bytes;
//! * a record has `==` exactly when every field does, and an enum exactly when every
//!   payload field of **every** variant does — decided from the type, never from the
//!   variant a value holds;
//! * a record compares field by field, and an enum compares its variant and then only that
//!   variant's payload;
//! * a concrete generic instance is answered after substitution, so `Box[Int]` compares
//!   and `Box[Vec[Int]]` does not, while an unconstrained `T` never does;
//! * `Option` and `Result` get it the same way as any generic enum, by no rule of their own;
//! * two types of one shape are still two types, and `Vec`, `Ints`, `Strs` and `Chan`
//!   have no equality at all.
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

fn value(src: &str) -> Value {
    match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}\n{src}"),
    }
}

fn int(src: &str) -> i64 {
    match value(src) {
        Value::Int(n) => n,
        other => panic!("expected an Int, got {other:?}"),
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

/// The program fails with exactly one diagnostic, `N0304`, at the `n`th `op`.
fn refused(src: &str, op: &str, n: usize) {
    let (start, end) = at(src, op, n);
    assert_eq!(
        diagnostics(src),
        vec![(codes::INVALID_OPERANDS.as_str().to_owned(), start, end)],
        "for {src}"
    );
}

/// The message of the program's one diagnostic.
fn message(src: &str) -> String {
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:?}");
    d[0].message.clone()
}

fn accepted(src: &str) {
    assert!(diagnostics(src).is_empty(), "{:?}\n{src}", diagnostics(src));
}

/// `1` when the comparison `expr` holds, `0` when it does not, after `decls`.
fn holds(decls: &str, expr: &str) -> i64 {
    int(&format!(
        "{decls}\nfn main() -> Int {{ if {expr} {{ 1 }} else {{ 0 }} }}\n"
    ))
}

const POINT: &str = "struct Point { x: Int, y: Int, }";
const MAYBE_INT: &str = "enum MaybeInt { None, Some(value: Int), }";

// --- primitives are unchanged ---------------------------------------------------------------

#[test]
fn primitive_equality_is_what_it_was() {
    assert_eq!(holds("", "3 == 3"), 1);
    assert_eq!(holds("", "3 == 4"), 0);
    assert_eq!(holds("", "3 != 4"), 1);
    assert_eq!(holds("", "true == true"), 1);
    assert_eq!(holds("", "true != false"), 1);
    // A `Str` is its bytes: a slice sharing a backing and a concatenation that does not
    // are equal when their bytes are, and a shared backing makes nothing equal.
    assert_eq!(
        holds("", "str_slice(\"xabx\", 1, 3) == str_concat(\"a\", \"b\")"),
        1
    );
    assert_eq!(
        holds("", "str_slice(\"abab\", 0, 2) == str_slice(\"abab\", 2, 4)"),
        1
    );
    assert_eq!(
        holds("", "str_slice(\"abc\", 0, 2) == str_slice(\"abc\", 1, 3)"),
        0
    );
    assert_eq!(holds("", "\"a\" == \"ab\""), 0);
    assert_eq!(holds("", "str_from_byte(0) == str_from_byte(0)"), 1);
    assert_eq!(holds("", "\"\" == str_slice(\"x\", 1, 1)"), 1);
}

// --- records -------------------------------------------------------------------------------

#[test]
fn a_record_is_equal_when_every_field_is() {
    assert_eq!(holds(POINT, "Point(x: 1, y: 2) == Point(x: 1, y: 2)"), 1);
    // Unequal early and unequal late: a comparison that stopped after one field, or
    // looked at only the last, gets one of these wrong.
    assert_eq!(holds(POINT, "Point(x: 1, y: 2) == Point(x: 9, y: 2)"), 0);
    assert_eq!(holds(POINT, "Point(x: 1, y: 2) == Point(x: 1, y: 9)"), 0);
    assert_eq!(holds(POINT, "Point(x: 1, y: 2) != Point(x: 1, y: 2)"), 0);
    assert_eq!(holds(POINT, "Point(x: 1, y: 2) != Point(x: 1, y: 9)"), 1);
    let named = "struct Named { name: Str, count: Int, flag: Bool, }";
    assert_eq!(
        holds(named, "Named(name: str_concat(\"a\", \"b\"), count: 2, flag: true) == Named(flag: true, count: 2, name: \"ab\")"),
        1
    );
    assert_eq!(
        holds(named, "Named(name: \"ab\", count: 2, flag: true) == Named(name: \"ab\", count: 2, flag: false)"),
        0
    );
}

#[test]
fn a_record_has_no_identity() {
    // Two records built separately, one copied and one changed after the copy.
    let src = format!(
        "{POINT}\nfn main() -> Int {{\n    let a = Point(x: 1, y: 2);\n    let mut b = a;\n    \
         let c = Point(x: 1, y: 2);\n    let mut n = 0;\n    if a == c {{ n = n + 1; }}\n    \
         b.y = 3;\n    if a != b {{ n = n + 10; }}\n    b.y = 2;\n    if a == b {{ n = n + 100; }}\n    n\n}}\n"
    );
    assert_eq!(int(&src), 111);
}

#[test]
fn a_record_has_equality_exactly_when_every_field_does() {
    let a = "struct A { name: Str, count: Int, }";
    accepted(&format!(
        "{a}\nfn main() -> Int {{ if A(name: \"x\", count: 1) == A(name: \"x\", count: 1) {{ 1 }} else {{ 0 }} }}"
    ));
    let c = format!("{a}\nstruct C {{ inner: A, }}");
    assert_eq!(
        holds(
            &c,
            "C(inner: A(name: \"x\", count: 1)) == C(inner: A(name: \"x\", count: 2))"
        ),
        0
    );
    for field in ["Vec[Int]", "Ints", "Strs", "Chan"] {
        let src = format!(
            "struct B {{ ok: Int, values: {field}, }}\nfn f(a: B, b: B) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}\n"
        );
        refused(&src, "==", 0);
        // And a record that contains one, however deep.
        let src = format!(
            "struct B {{ values: {field}, }}\nstruct D {{ n: Int, inner: B, }}\n\
             fn f(a: D, b: D) -> Bool {{ a != b }}\nfn main() -> Int {{ 0 }}\n"
        );
        refused(&src, "!=", 0);
    }
}

#[test]
fn a_refusal_names_the_component_that_has_no_equality() {
    let src = "struct B { n: Int, values: Vec[Int], }\nstruct Outer { inner: B, }\n\
               fn f(a: Outer, b: Outer) -> Bool { a == b }\nfn main() -> Int { 0 }\n";
    let m = message(src);
    assert!(m.contains("`Outer`"), "{m}");
    assert!(m.contains("`inner.values`"), "{m}");
    assert!(m.contains("`Vec[Int]`"), "{m}");
    // Through a variant, the variant is named on the way.
    let src = "enum W { Empty, Held(items: Ints), }\nstruct Outer { work: W, }\n\
               fn f(a: Outer, b: Outer) -> Bool { a == b }\nfn main() -> Int { 0 }\n";
    let m = message(src);
    assert!(m.contains("`work.Held.items`"), "{m}");
    // A handle itself has no path to name.
    let m = message("fn main() -> Int { let a = ints_new(); if a == a { 1 } else { 0 } }");
    assert!(m.contains("does not apply to `Ints`"), "{m}");
}

#[test]
fn two_types_of_one_shape_are_never_compared() {
    refused(
        "struct A { x: Int, }\nstruct B { x: Int, }\nfn main() -> Int { if A(x: 1) == B(x: 1) { 1 } else { 0 } }",
        "==",
        0,
    );
    refused(
        "enum A { V, }\nenum B { V, }\nfn main() -> Int { if A.V() == B.V() { 1 } else { 0 } }",
        "==",
        0,
    );
    let boxes = "struct Box[T] { value: T, }\nstruct Crate[T] { value: T, }";
    refused(
        &format!("{boxes}\nfn main() -> Int {{ if Box[Int](value: 1) == Box[Str](value: \"1\") {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
    refused(
        &format!("{boxes}\nfn main() -> Int {{ if Box[Int](value: 1) == Crate[Int](value: 1) {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
    refused(
        "fn main() -> Int { if Option[Int].None() == Option[Str].None() { 1 } else { 0 } }",
        "==",
        0,
    );
}

// --- enums ---------------------------------------------------------------------------------

#[test]
fn an_enum_compares_its_variant_and_then_that_variant_s_payload() {
    assert_eq!(holds(MAYBE_INT, "MaybeInt.None() == MaybeInt.None()"), 1);
    assert_eq!(
        holds(MAYBE_INT, "MaybeInt.None() == MaybeInt.Some(value: 0)"),
        0
    );
    assert_eq!(
        holds(MAYBE_INT, "MaybeInt.Some(value: 0) == MaybeInt.None()"),
        0
    );
    assert_eq!(
        holds(
            MAYBE_INT,
            "MaybeInt.Some(value: 1) == MaybeInt.Some(value: 1)"
        ),
        1
    );
    assert_eq!(
        holds(
            MAYBE_INT,
            "MaybeInt.Some(value: 1) == MaybeInt.Some(value: 2)"
        ),
        0
    );
    assert_eq!(
        holds(
            MAYBE_INT,
            "MaybeInt.Some(value: 1) != MaybeInt.Some(value: 2)"
        ),
        1
    );
    assert_eq!(holds(MAYBE_INT, "MaybeInt.None() != MaybeInt.None()"), 0);
    // Two variants with identical payloads are still two variants.
    let twins = "enum Twins { Left(n: Int), Right(n: Int), }";
    assert_eq!(holds(twins, "Twins.Left(n: 1) == Twins.Right(n: 1)"), 0);
    assert_eq!(holds(twins, "Twins.Right(n: 1) == Twins.Right(n: 1)"), 1);
    // Several payload fields: every one of them counts.
    let pair = "enum P { Empty, Pair(left: Int, right: Str), }";
    assert_eq!(
        holds(
            pair,
            "P.Pair(left: 1, right: \"a\") == P.Pair(right: \"a\", left: 1)"
        ),
        1
    );
    assert_eq!(
        holds(
            pair,
            "P.Pair(left: 1, right: \"a\") == P.Pair(left: 2, right: \"a\")"
        ),
        0
    );
    assert_eq!(
        holds(
            pair,
            "P.Pair(left: 1, right: \"a\") == P.Pair(left: 1, right: \"b\")"
        ),
        0
    );
}

#[test]
fn an_enum_has_equality_only_when_every_variant_does() {
    // Decided from the type, not from the variant a value holds: `Empty` owns nothing,
    // and `Work.Empty() == Work.Empty()` is still refused.
    let work = "enum Work { Empty, Values(value: Vec[Int]), Count(n: Int), }";
    refused(
        &format!(
            "{work}\nfn main() -> Int {{ if Work.Empty() == Work.Empty() {{ 1 }} else {{ 0 }} }}"
        ),
        "==",
        0,
    );
    // The blocking variant need not be the first or the last written.
    let middle = "enum M { A(n: Int), B(c: Chan), C(s: Str), }";
    refused(
        &format!("{middle}\nfn main() -> Int {{ if M.A(n: 1) == M.A(n: 1) {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
    let last = "enum L { A(n: Int), B(s: Str), C(xs: Strs), }";
    refused(
        &format!("{last}\nfn main() -> Int {{ if L.A(n: 1) != L.A(n: 1) {{ 1 }} else {{ 0 }} }}"),
        "!=",
        0,
    );
    accepted("enum E { Empty, Text(value: Str), }\nfn main() -> Int { if E.Empty() == E.Text(value: \"\") { 1 } else { 0 } }");
}

#[test]
fn a_single_variant_enum_compares_its_payload_or_is_always_equal() {
    assert_eq!(
        holds(
            "enum One { Only(n: Int), }",
            "One.Only(n: 1) == One.Only(n: 1)"
        ),
        1
    );
    assert_eq!(
        holds(
            "enum One { Only(n: Int), }",
            "One.Only(n: 1) == One.Only(n: 2)"
        ),
        0
    );
    assert_eq!(
        holds("enum Solo { Only, }", "Solo.Only() == Solo.Only()"),
        1
    );
    assert_eq!(
        holds("enum Solo { Only, }", "Solo.Only() != Solo.Only()"),
        0
    );
}

// --- Option and Result, as the ordinary enums they are --------------------------------------

#[test]
fn option_and_result_have_equality_through_their_arguments() {
    assert_eq!(holds("", "Option[Int].None() == Option[Int].None()"), 1);
    assert_eq!(
        holds(
            "",
            "Option[Int].Some(value: 1) == Option[Int].Some(value: 1)"
        ),
        1
    );
    assert_eq!(
        holds(
            "",
            "Option[Int].Some(value: 1) == Option[Int].Some(value: 2)"
        ),
        0
    );
    assert_eq!(
        holds("", "Option[Int].Some(value: 1) == Option[Int].None()"),
        0
    );
    assert_eq!(
        holds(
            "",
            "Option[Str].Some(value: str_concat(\"a\", \"b\")) == Option[Str].Some(value: \"ab\")"
        ),
        1
    );
    assert_eq!(holds(POINT, "Option[Point].Some(value: Point(x: 1, y: 2)) != Option[Point].Some(value: Point(x: 1, y: 3))"), 1);
    assert_eq!(
        holds(
            "",
            "Result[Int, Str].Ok(value: 1) == Result[Int, Str].Ok(value: 1)"
        ),
        1
    );
    assert_eq!(
        holds(
            "",
            "Result[Int, Str].Ok(value: 1) == Result[Int, Str].Err(error: \"1\")"
        ),
        0
    );
    assert_eq!(
        holds(
            "",
            "Result[Int, Str].Err(error: \"e\") == Result[Int, Str].Err(error: \"e\")"
        ),
        1
    );
    let code = "enum ErrorCode { Missing, Bad(n: Int), }";
    assert_eq!(
        holds(&format!("{POINT}\n{code}"), "Result[Point, ErrorCode].Err(error: ErrorCode.Bad(n: 1)) == Result[Point, ErrorCode].Err(error: ErrorCode.Bad(n: 1))"),
        1
    );
}

#[test]
fn option_and_result_of_a_type_without_equality_have_none() {
    refused(
        "fn main() -> Int { if Option[Vec[Int]].None() == Option[Vec[Int]].None() { 1 } else { 0 } }",
        "==",
        0,
    );
    refused(
        "struct Diag { code: Str, }\n\
         fn main() -> Int { if Result[Int, Vec[Diag]].Ok(value: 1) == Result[Int, Vec[Diag]].Ok(value: 1) { 1 } else { 0 } }",
        "==",
        0,
    );
}

#[test]
fn a_user_enum_shaped_like_option_is_answered_by_the_same_rule_and_is_not_option() {
    // Were `Option` recognised by name, a user enum of its shape could disagree with it;
    // were it recognised by shape, the last case here would compare. Neither is so.
    let maybe = "enum Maybe[T] { None, Some(value: T), }";
    assert_eq!(
        holds(
            maybe,
            "Maybe[Int].Some(value: 3) == Maybe[Int].Some(value: 3)"
        ),
        1
    );
    refused(
        &format!("{maybe}\nfn main() -> Int {{ if Maybe[Vec[Int]].None() == Maybe[Vec[Int]].None() {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
    refused(
        &format!("{maybe}\nfn main() -> Int {{ if Maybe[Int].None() == Option[Int].None() {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
}

// --- generics ------------------------------------------------------------------------------

#[test]
fn a_generic_instance_has_equality_after_substitution() {
    let b = format!("struct Box[T] {{ value: T, }}\n{POINT}");
    assert_eq!(holds(&b, "Box[Int](value: 1) == Box[Int](value: 1)"), 1);
    assert_eq!(holds(&b, "Box[Int](value: 1) == Box[Int](value: 2)"), 0);
    assert_eq!(
        holds(&b, "Box[Str](value: \"a\") != Box[Str](value: \"b\")"),
        1
    );
    assert_eq!(
        holds(
            &b,
            "Box[Point](value: Point(x: 1, y: 2)) == Box[Point](value: Point(x: 1, y: 2))"
        ),
        1
    );
    assert_eq!(
        holds(
            &b,
            "Box[Box[Int]](value: Box[Int](value: 4)) == Box[Box[Int]](value: Box[Int](value: 5))"
        ),
        0
    );
    refused(
        &format!("{b}\nfn main() -> Int {{ let a = Box[Vec[Int]](value: vec_new[Int]()); if a == a {{ 1 }} else {{ 0 }} }}"),
        "==",
        0,
    );
    refused(
        &format!("{b}\nfn f(a: Box[Box[Chan]], b: Box[Box[Chan]]) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}"),
        "==",
        0,
    );
    // Two arguments, each of which counts.
    let pair = "struct Pair[A, B] { first: A, second: B, }";
    assert_eq!(
        holds(
            pair,
            "Pair[Int, Str](first: 1, second: \"x\") == Pair[Int, Str](first: 1, second: \"x\")"
        ),
        1
    );
    refused(
        &format!("{pair}\nfn f(a: Pair[Int, Ints], b: Pair[Int, Ints]) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}"),
        "==",
        0,
    );
    refused(
        &format!("{pair}\nfn f(a: Pair[Strs, Int], b: Pair[Strs, Int]) -> Bool {{ a == b }}\nfn main() -> Int {{ 0 }}"),
        "==",
        0,
    );
}

#[test]
fn an_unconstrained_type_parameter_still_has_no_equality() {
    refused(
        "fn same[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }\n",
        "==",
        0,
    );
    // Nor does a generic type applied to one: `Box[T]` is not known to compare, and
    // no call decides it later.
    refused(
        "struct Box[T] { value: T, }\n\
         fn same_box[T](a: Box[T], b: Box[T]) -> Bool { a == b }\n\
         fn main() -> Int { if same_box(Box[Int](value: 1), Box[Int](value: 1)) { 1 } else { 0 } }\n",
        "==",
        0,
    );
    refused(
        "fn same[T](a: Option[T], b: Option[T]) -> Bool { a != b }\nfn main() -> Int { 0 }\n",
        "!=",
        0,
    );
    // A generic body may still compare what its parameters do not reach.
    accepted(
        "struct Box[T] { value: T, tag: Int, }\n\
         fn same_tag[T](a: Box[T], b: Box[T]) -> Bool { a.tag == b.tag }\nfn main() -> Int { 0 }\n",
    );
}

// --- nested composites ---------------------------------------------------------------------

#[test]
fn equality_composes_through_every_nesting() {
    let decls = "struct Name { text: Str, }\n\
                 struct Person { name: Name, age: Int, }\n\
                 enum Shape { Dot, Circle(centre: Person, r: Int), }\n\
                 struct Drawing { shape: Shape, }\n\
                 enum Layer { Hidden, Shown(top: Drawing), }\n\
                 struct Box[T] { value: T, }\n";
    let p = |t: &str, age: i64| format!("Person(name: Name(text: \"{t}\"), age: {age})");
    // record → record → Str
    assert_eq!(holds(decls, &format!("{} == {}", p("a", 1), p("a", 1))), 1);
    assert_eq!(holds(decls, &format!("{} == {}", p("a", 1), p("b", 1))), 0);
    // record → enum → record
    let d = |t: &str| format!("Drawing(shape: Shape.Circle(centre: {}, r: 2))", p(t, 1));
    assert_eq!(holds(decls, &format!("{} == {}", d("a"), d("a"))), 1);
    assert_eq!(holds(decls, &format!("{} == {}", d("a"), d("z"))), 0);
    // enum → record → enum
    let l = |t: &str| format!("Layer.Shown(top: {})", d(t));
    assert_eq!(holds(decls, &format!("{} == {}", l("a"), l("a"))), 1);
    assert_eq!(holds(decls, &format!("{} != {}", l("a"), l("q"))), 1);
    assert_eq!(holds(decls, &format!("{} == Layer.Hidden()", l("a"))), 0);
    // Option[record], Result[enum, record], generic record[enum]
    assert_eq!(
        holds(
            decls,
            &format!(
                "Option[Person].Some(value: {}) == Option[Person].Some(value: {})",
                p("a", 1),
                p("a", 1)
            )
        ),
        1
    );
    assert_eq!(
        holds(decls, &format!("Result[Shape, Person].Err(error: {}) == Result[Shape, Person].Ok(value: Shape.Dot())", p("a", 1))),
        0
    );
    assert_eq!(
        holds(
            decls,
            "Box[Shape](value: Shape.Dot()) == Box[Shape](value: Shape.Dot())"
        ),
        1
    );
}

#[test]
fn a_type_without_equality_anywhere_inside_blocks_the_whole() {
    // record → enum → record → Vec[Int]
    refused(
        "struct Leaf { xs: Vec[Int], }\nenum Mid { None, Has(leaf: Leaf), }\nstruct Top { mid: Mid, }\n\
         fn f(a: Top, b: Top) -> Bool { a == b }\nfn main() -> Int { 0 }\n",
        "==",
        0,
    );
    // enum → Option[Vec[Int]]
    refused(
        "enum E { A, B(maybe: Option[Vec[Int]]), }\nfn main() -> Int { if E.A() == E.A() { 1 } else { 0 } }\n",
        "==",
        0,
    );
}

// --- evaluation --------------------------------------------------------------------------------

#[test]
fn both_operands_are_evaluated_once_left_first_and_neither_is_consumed() {
    let src = format!(
        "{POINT}\nfn mark(log: Ints, n: Int) -> Point {{ ints_push(log, n); Point(x: n, y: 0) }}\n\
         fn main() -> Int {{\n    let log = ints_new();\n    let same = mark(log, 1) == mark(log, 2);\n    \
         let a = Point(x: 3, y: 4);\n    let b = a;\n    let again = a == b;\n    \
         let mut n = ints_get(log, 0) * 10 + ints_get(log, 1) + ints_len(log) * 100;\n    \
         if same {{ n = n + 1000; }}\n    if again {{ n = n + 10000; }}\n    n + a.x + b.y\n}}\n"
    );
    // log = [1, 2]: 12 + 200; `same` is false; `again` is true; 3 + 4.
    assert_eq!(int(&src), 10_219);
}

#[test]
fn an_operand_that_leaves_leaves_no_comparison_behind() {
    // The left side returns: the right is never evaluated. The right side returns: the
    // left was, and nothing compares.
    let src = format!(
        "{POINT}\nfn mark(log: Ints, n: Int) -> Point {{ ints_push(log, n); Point(x: n, y: 0) }}\n\
         fn left(log: Ints) -> Int {{ if Point(x: {{ return 7; }}, y: 0) == mark(log, 1) {{ 1 }} else {{ 2 }} }}\n\
         fn right(log: Ints) -> Int {{ if mark(log, 2) == Point(x: {{ return 8; }}, y: 0) {{ 1 }} else {{ 2 }} }}\n\
         fn main() -> Int {{ let log = ints_new(); let a = left(log); let b = right(log); \
         a * 1000 + b * 100 + ints_len(log) * 10 + ints_get(log, 0) }}\n"
    );
    assert_eq!(int(&src), 7812);
}

#[test]
fn equality_works_inside_a_value_block_and_after_propagation() {
    let src = "enum E { A(value: Str), B, }\n\
               fn expected() -> Str { str_concat(\"x\", \"y\") }\n\
               fn found(ok: Bool) -> Result[Option[Str], Int] { if ok { Result[Option[Str], Int].Ok(value: Option[Str].Some(value: \"xy\")) } else { Result[Option[Str], Int].Err(error: 3) } }\n\
               fn compare(ok: Bool) -> Result[Int, Int] {\n    let got = found(ok)?;\n    \
               if got == Option[Str].Some(value: expected()) { Result[Int, Int].Ok(value: 1) } else { Result[Int, Int].Ok(value: 0) }\n}\n\
               fn main() -> Int {\n    let e = E.A(value: \"xy\");\n    let same = match e {\n        \
               E.A(value: x) => {\n            let want = expected();\n            x == want\n        },\n        \
               E.B() => {\n            false\n        },\n    };\n    \
               let one = if same { 1 } else { 0 };\n    \
               let ten = match compare(true) { Result.Ok(value: v) => 10 * v, Result.Err(error: _) => 0, };\n    \
               let hundred = match compare(false) { Result.Ok(value: _) => 0, Result.Err(error: c) => 100 * c, };\n    \
               one + ten + hundred\n}\n";
    assert_eq!(int(src), 311);
}

// --- what stays absent ---------------------------------------------------------------------------

#[test]
fn handles_still_have_no_equality() {
    refused(
        "fn main() -> Int { let a = vec_new[Int](); if a == a { 1 } else { 0 } }",
        "==",
        0,
    );
    refused(
        "fn main() -> Int { let a = ints_new(); if a == a { 1 } else { 0 } }",
        "==",
        0,
    );
    refused(
        "fn main() -> Int { let a = strs_new(); if a != a { 1 } else { 0 } }",
        "!=",
        0,
    );
    refused(
        "fn main() -> Int { let a = chan_new(1); if a == a { 1 } else { 0 } }",
        "==",
        0,
    );
    // Ordering is derived since R1a (L6), but not through a handle.
    let src = "struct H { v: Vec[Int], }\nfn main() -> Int { let h = H(v: vec_new[Int]()); if h < h { 1 } else { 0 } }";
    refused(src, "<", 0);
}
