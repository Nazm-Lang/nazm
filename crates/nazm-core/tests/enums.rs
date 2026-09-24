//! Enums and `match`: what the checker accepts, what it refuses, and by which name.
//!
//! The acceptance question N10 exists to answer is whether the ownership model N7–N9 built
//! holds when **which fields exist is a runtime fact**. These are the front end's half of
//! the evidence; `crates/nazm-cli/tests/memory.rs` is the back end's, and
//! `crates/nazm-cli/tests/visibility.rs` the module system's.
//!
//! Each test names the rule in `docs/spec.md` it pins.

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

/// The single diagnostic a program fails with, code and message.
fn sole(src: &str) -> (nazm_diag::Code, String) {
    match run(src) {
        Outcome::CheckFailed(ds) => {
            assert_eq!(ds.len(), 1, "expected one error, got {ds:#?}");
            (ds[0].code.clone(), ds[0].message.clone())
        }
        Outcome::RuntimeFailed(d) => (d.code, d.message),
        Outcome::Value(v) => panic!("expected an error, got {v}"),
    }
}

fn code(src: &str) -> nazm_diag::Code {
    sole(src).0
}

const TOKEN: &str = r"
    enum Token {
        Eof,
        Number(value: Int),
        Name(text: Str),
        Pair(left: Int, right: Int),
    }
";

// --- construction and matching -------------------------------------------------------

#[test]
fn a_variant_is_built_by_naming_its_enum_and_eliminated_by_match() {
    let src = format!(
        "{TOKEN}
        fn size(t: Token) -> Int {{
            match t {{
                Token.Eof() => 0,
                Token.Number(value: n) => n,
                Token.Name(text: s) => str_len(s),
                Token.Pair(left: a, right: b) => a + b,
            }}
        }}
        fn main() -> Int {{
            size(Token.Eof()) + size(Token.Number(value: 30))
                + size(Token.Name(text: \"abcd\")) + size(Token.Pair(left: 3, right: 5))
        }}"
    );
    assert_eq!(value(&src), Value::Int(42));
}

#[test]
fn a_zero_payload_variant_is_written_with_parentheses_everywhere_but_its_declaration() {
    // `docs/spec.md`: the declaration has no field list to write, and construction and
    // patterns write the parentheses because `Token.Eof` alone is a field projection.
    let src = r"
        enum E { A, }
        fn main() -> Int { match E.A() { E.A() => 7, } }
    ";
    assert_eq!(value(src), Value::Int(7));
}

#[test]
fn a_single_variant_enum_is_a_sum_type_rather_than_a_record() {
    // Allowed, and it is still eliminated by naming the variant: `docs/spec.md` refuses to
    // special-case it into a record, so adding a second variant later is an ordinary edit.
    let src = r"
        enum Wrapper { Value(data: Int), }
        fn main() -> Int { match Wrapper.Value(data: 42) { Wrapper.Value(data: d) => d, } }
    ";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn payload_initialisers_may_be_written_in_any_order_and_mean_the_same_value() {
    let a = r"
        enum P { V(left: Int, right: Int), }
        fn main() -> Int { match P.V(left: 1, right: 2) { P.V(left: l, right: r) => l * 10 + r, } }
    ";
    let b = r"
        enum P { V(left: Int, right: Int), }
        fn main() -> Int { match P.V(right: 2, left: 1) { P.V(right: r, left: l) => l * 10 + r, } }
    ";
    assert_eq!(value(a), Value::Int(12));
    assert_eq!(value(b), Value::Int(12));
}

#[test]
// The expectation is written as the program computes it — the first entry times 100, the
// second times 10 — and `1 * 10` is that second term, not an operation to simplify away.
#[allow(clippy::identity_op)]
fn payload_initialisers_are_evaluated_exactly_once_in_the_order_written() {
    // `docs/spec.md` fixes the order as the order written, whatever order the payload is
    // stored in. The sequence records which ran first.
    let src = r"
        enum P { V(left: Int, right: Int), }
        fn note(xs: Ints, n: Int) -> Int { ints_push(xs, n); n }
        fn main() -> Int {
            let log = ints_new();
            let v = P.V(right: note(log, 2), left: note(log, 1));
            let sum = match v { P.V(left: l, right: r) => l + r, };
            ints_get(log, 0) * 100 + ints_get(log, 1) * 10 + ints_len(log) + sum
        }
    ";
    // `2` ran first, then `1`; two entries; the payload still sums to 3.
    assert_eq!(value(src), Value::Int(2 * 100 + 1 * 10 + 2 + 3));
}

#[test]
fn a_variant_of_two_different_enums_may_share_a_name() {
    let src = r"
        enum A { Ready, }
        enum B { Ready, }
        fn main() -> Int {
            let a = match A.Ready() { A.Ready() => 1, };
            let b = match B.Ready() { B.Ready() => 2, };
            a * 10 + b
        }
    ";
    assert_eq!(value(src), Value::Int(12));
}

#[test]
fn two_enums_with_identical_variants_are_different_types() {
    let src = r"
        enum A { V(x: Int), }
        enum B { V(x: Int), }
        fn takes(a: A) -> Int { match a { A.V(x: v) => v, } }
        fn main() -> Int { takes(B.V(x: 1)) }
    ";
    assert_eq!(code(src), codes::TYPE_MISMATCH);
}

#[test]
fn a_match_is_an_expression_and_composes_where_one_may_appear() {
    let src = r"
        enum E { A, B(x: Int), }
        fn pick(e: E) -> Int { return match e { E.A() => 1, E.B(x: v) => v, }; }
        fn main() -> Int {
            let direct = match E.B(x: 5) { E.A() => 0, E.B(x: v) => v, };
            direct + pick(E.A()) + match E.A() { E.A() => 10, E.B(x: v) => v, }
        }
    ";
    assert_eq!(value(src), Value::Int(16));
}

#[test]
fn an_arm_body_may_be_a_block_with_its_own_bindings() {
    let src = r#"
        enum E { A(text: Str), B, }
        fn main() -> Int {
            match E.A(text: "hello") {
                E.A(text: s) => {
                    let n = str_len(s);
                    n * 2
                },
                E.B() => 0,
            }
        }
    "#;
    assert_eq!(value(src), Value::Int(10));
}

#[test]
fn an_arm_that_transfers_control_constrains_no_type() {
    // The same rule an `if` branch follows: a diverging arm stands in for whatever the
    // context wanted, because control never reaches the point where its value would be.
    // `return` is a statement in this language, so the arm is a block that contains one.
    let src = r"
        enum E { A, B, }
        fn f(e: E) -> Int { match e { E.A() => { return 3; }, E.B() => 4, } }
        fn main() -> Int { f(E.A()) * 10 + f(E.B()) }
    ";
    assert_eq!(value(src), Value::Int(34));
}

// --- exhaustiveness and arm shape ----------------------------------------------------

#[test]
fn a_match_that_misses_a_variant_is_refused_and_names_it() {
    let (c, message) = sole(
        r"
        enum E { A, B(x: Int), C, }
        fn main() -> Int { match E.A() { E.A() => 1, } }
        ",
    );
    assert_eq!(c, codes::NON_EXHAUSTIVE_MATCH);
    assert!(message.contains("`B`"), "{message}");
    assert!(message.contains("`C`"), "{message}");
}

#[test]
fn a_match_that_names_one_variant_twice_is_refused() {
    assert_eq!(
        code(
            r"
            enum E { A, B, }
            fn main() -> Int { match E.A() { E.A() => 1, E.A() => 2, E.B() => 3, } }
            "
        ),
        codes::DUPLICATE_MATCH_ARM
    );
}

#[test]
fn exhaustiveness_is_membership_rather_than_declaration_order() {
    // The arms are written in the reverse of the declaration order, and cover the set.
    let src = r"
        enum E { A, B, C, }
        fn main() -> Int { match E.B() { E.C() => 3, E.B() => 2, E.A() => 1, } }
    ";
    assert_eq!(value(src), Value::Int(2));
}

#[test]
fn a_pattern_naming_another_enums_variant_is_refused() {
    assert_eq!(
        code(
            r"
            enum A { Ready, }
            enum B { Ready, }
            fn main() -> Int { match A.Ready() { B.Ready() => 1, } }
            "
        ),
        codes::WRONG_ENUM_IN_PATTERN
    );
}

#[test]
fn a_pattern_naming_a_variant_the_enum_does_not_have_is_refused() {
    let (c, message) = sole(
        r"
        enum E { A, B, }
        fn main() -> Int { match E.A() { E.A() => 1, E.Missing() => 2, E.B() => 3, } }
        ",
    );
    assert_eq!(c, codes::UNKNOWN_VARIANT);
    assert!(message.contains("`Missing`"), "{message}");
}

#[test]
fn a_construction_naming_a_variant_the_enum_does_not_have_is_refused() {
    assert_eq!(
        code(
            r"
            enum E { A, }
            fn main() -> Int { match E.Nope() { E.A() => 1, } }
            "
        ),
        codes::UNKNOWN_VARIANT
    );
}

#[test]
fn a_pattern_must_name_every_payload_field() {
    let (c, message) = sole(
        r"
        enum E { V(left: Int, right: Int), }
        fn main() -> Int { match E.V(left: 1, right: 2) { E.V(left: a) => a, } }
        ",
    );
    assert_eq!(c, codes::MISSING_FIELD);
    assert!(message.contains("`right`"), "{message}");
}

#[test]
fn a_pattern_naming_a_payload_field_that_does_not_exist_is_refused() {
    // Two diagnostics, as a record construction naming an unknown field also gives: the
    // field does not exist, and the one that does was not named. The first is the one this
    // rule is about.
    let src = r"
        enum E { V(x: Int), }
        fn main() -> Int { match E.V(x: 1) { E.V(y: a) => a, } }
    ";
    let Outcome::CheckFailed(ds) = run(src) else {
        panic!("expected a refusal")
    };
    assert_eq!(ds[0].code, codes::UNKNOWN_FIELD, "{ds:#?}");
}

#[test]
fn a_pattern_naming_one_payload_field_twice_is_refused() {
    let (c, _) = sole(
        r"
        enum E { V(x: Int), }
        fn main() -> Int { match E.V(x: 1) { E.V(x: a, x: b) => a, } }
        ",
    );
    assert_eq!(c, codes::DUPLICATE_FIELD);
}

#[test]
fn two_bindings_of_one_name_in_one_pattern_are_refused() {
    assert_eq!(
        code(
            r"
            enum E { V(x: Int, y: Int), }
            fn main() -> Int { match E.V(x: 1, y: 2) { E.V(x: a, y: a) => a, } }
            "
        ),
        codes::ALREADY_DEFINED_IN_SCOPE
    );
}

#[test]
fn a_discard_names_nothing_and_two_of_them_are_fine() {
    let src = r"
        enum E { V(x: Int, y: Int), }
        fn main() -> Int { match E.V(x: 1, y: 2) { E.V(x: _, y: _) => 9, } }
    ";
    assert_eq!(value(src), Value::Int(9));
}

#[test]
fn a_binding_does_not_escape_its_arm() {
    assert_eq!(
        code(
            r"
            enum E { A(x: Int), B, }
            fn main() -> Int { let n = match E.B() { E.A(x: v) => v, E.B() => 0, }; v }
            "
        ),
        codes::UNDEFINED_VARIABLE
    );
}

#[test]
fn a_binding_from_one_arm_is_not_visible_in_another() {
    assert_eq!(
        code(
            r"
            enum E { A(x: Int), B, }
            fn main() -> Int { match E.B() { E.A(x: v) => v, E.B() => v, } }
            "
        ),
        codes::UNDEFINED_VARIABLE
    );
}

#[test]
fn arms_that_produce_different_types_are_refused() {
    assert_eq!(
        code(
            r#"
            enum E { A, B, }
            fn main() -> Int { let n = match E.A() { E.A() => 1, E.B() => "two", }; n }
            "#
        ),
        codes::BRANCH_TYPES_DIFFER
    );
}

// --- declaration rules ---------------------------------------------------------------

#[test]
fn an_enum_with_no_variants_is_refused_by_name() {
    assert_eq!(
        code("enum Never {} fn main() -> Int { 0 }"),
        codes::EMPTY_ENUM
    );
}

#[test]
fn a_variant_declared_with_empty_brackets_is_refused() {
    // One concept, one spelling: a variant with no payload has no field list. The
    // brackets are written at a construction and in a pattern, where they are what
    // separates a variant from a field read — and nowhere else.
    let (c, message) = sole("enum E { A(), B, } fn main() -> Int { 0 }");
    assert_eq!(c, codes::UNSUPPORTED_CONSTRUCT);
    assert!(message.contains("`A`"), "{message}");
}

#[test]
fn two_variants_of_one_name_are_refused() {
    assert_eq!(
        code("enum E { A, A, } fn main() -> Int { 0 }"),
        codes::DUPLICATE_VARIANT
    );
}

#[test]
fn two_payload_fields_of_one_name_are_refused() {
    assert_eq!(
        code("enum E { V(x: Int, x: Int), } fn main() -> Int { 0 }"),
        codes::DUPLICATE_FIELD
    );
}

#[test]
fn a_record_and_an_enum_of_one_name_collide_in_the_type_namespace() {
    assert_eq!(
        code("struct Item { x: Int, } enum Item { A, } fn main() -> Int { 0 }"),
        codes::DUPLICATE_TYPE
    );
}

#[test]
fn an_enum_may_not_take_a_built_in_types_name() {
    assert_eq!(
        code("enum Int { A, } fn main() -> Int { 0 }"),
        codes::BUILTIN_TYPE_REDEFINED
    );
}

#[test]
fn an_enum_and_a_function_of_one_name_do_not_collide() {
    // Types and functions are separate namespaces, and an enum is a type.
    let src = r"
        enum State { Ready, }
        fn State() -> Int { 4 }
        fn main() -> Int { State() + match State.Ready() { State.Ready() => 5, } }
    ";
    assert_eq!(value(src), Value::Int(9));
}

// --- refusals that are about the shape of the language --------------------------------

#[test]
fn match_on_something_that_is_not_an_enum_is_refused() {
    assert_eq!(
        code("fn main() -> Int { match 42 { } }"),
        codes::MATCH_ON_NON_ENUM
    );
    assert_eq!(
        code("struct P { x: Int, } fn main() -> Int { match P(x: 1) { } }"),
        codes::MATCH_ON_NON_ENUM
    );
}

#[test]
fn an_enums_payload_cannot_be_projected_out_of_it() {
    let (c, _) = sole("enum E { V(x: Int), } fn main() -> Int { E.V(x: 1).x }");
    assert_eq!(c, codes::INVALID_PROJECTION);
}

#[test]
fn equality_is_offered_on_an_enum_only_when_every_variant_has_it() {
    // N13. Decided over every variant, never the one a value holds: `E.A()` owns nothing,
    // and in the second program it is still not comparable.
    let src = r"
        enum E { A, }
        fn main() -> Int { let a = E.A(); let b = E.A(); if a == b { 1 } else { 0 } }
    ";
    assert!(check_source(src).is_empty(), "{:?}", check_source(src));
    let src = r"
        enum E { A, B(xs: Strs), }
        fn main() -> Int { let a = E.A(); let b = E.A(); if a == b { 1 } else { 0 } }
    ";
    assert_eq!(code(src), codes::INVALID_OPERANDS);
}

#[test]
fn a_payload_value_of_the_wrong_type_is_refused() {
    assert_eq!(
        code(
            r#"
            enum E { V(x: Int), }
            fn main() -> Int { match E.V(x: "no") { E.V(x: v) => v, } }
            "#
        ),
        codes::TYPE_MISMATCH
    );
}

#[test]
fn a_construction_that_omits_a_payload_field_is_refused() {
    assert_eq!(
        code(
            r"
            enum E { V(x: Int, y: Int), }
            fn main() -> Int { match E.V(x: 1) { E.V(x: a, y: b) => a + b, } }
            "
        ),
        codes::MISSING_FIELD
    );
}

#[test]
fn a_construction_naming_an_unknown_payload_field_is_refused() {
    assert_eq!(
        code(
            r"
            enum E { V(x: Int), }
            fn main() -> Int { match E.V(x: 1, z: 2) { E.V(x: a) => a, } }
            "
        ),
        codes::UNKNOWN_FIELD
    );
}

#[test]
fn a_construction_giving_one_payload_field_twice_is_refused() {
    assert_eq!(
        code(
            r"
            enum E { V(x: Int), }
            fn main() -> Int { match E.V(x: 1, x: 2) { E.V(x: a) => a, } }
            "
        ),
        codes::DUPLICATE_FIELD
    );
}

// --- layout ---------------------------------------------------------------------------

#[test]
fn an_enum_that_contains_itself_is_refused_as_an_impossible_layout() {
    assert_eq!(
        code("enum List { End, Next(rest: List), } fn main() -> Int { 0 }"),
        codes::RECURSIVE_RECORD
    );
}

#[test]
fn a_cycle_through_a_record_and_an_enum_is_refused_too() {
    // N9's rule stated over every inline user-defined value type. A walk that knew only
    // about records would have run forever on this.
    assert_eq!(
        code("struct A { b: B, } enum B { V(a: A), } fn main() -> Int { 0 }"),
        codes::RECURSIVE_RECORD
    );
    assert_eq!(
        code("enum A { V(b: B), } enum B { V(a: A), } fn main() -> Int { 0 }"),
        codes::RECURSIVE_RECORD
    );
}

#[test]
fn an_enum_may_contain_a_record_and_a_record_an_enum() {
    let src = r"
        struct Point { x: Int, y: Int, }
        enum Shape { Dot, At(where: Point), }
        struct Event { shape: Shape, }
        fn main() -> Int {
            let e = Event(shape: Shape.At(where: Point(x: 3, y: 4)));
            match e.shape { Shape.Dot() => 0, Shape.At(where: p) => p.x * 10 + p.y, }
        }
    ";
    assert_eq!(value(src), Value::Int(34));
}

#[test]
fn an_enum_may_contain_an_enum() {
    let src = r"
        enum Inner { A(n: Int), B, }
        enum Outer { Wrapped(value: Inner), Empty, }
        fn f(o: Outer) -> Int {
            match o {
                Outer.Empty() => -1,
                Outer.Wrapped(value: v) => match v { Inner.A(n: n) => n, Inner.B() => 0, },
            }
        }
        fn main() -> Int { f(Outer.Wrapped(value: Inner.A(n: 8))) + f(Outer.Empty()) }
    ";
    assert_eq!(value(src), Value::Int(7));
}

// --- ownership, as the interpreter sees it ---------------------------------------------

#[test]
fn a_payload_returned_from_an_arm_outlives_the_value_it_came_from() {
    let src = r#"
        enum E { Text(value: Str), Empty, }
        fn take(e: E) -> Str { match e { E.Text(value: s) => s, E.Empty() => "", } }
        fn main() -> Int { str_len(take(E.Text(value: str_concat("he", "llo")))) }
    "#;
    assert_eq!(value(src), Value::Int(5));
}

#[test]
fn a_matched_sequence_handle_still_reaches_the_storage_it_names() {
    // *Immutable binding* is a statement about the name, never about the storage.
    let src = r"
        enum E { Values(items: Ints), }
        fn main() -> Int {
            let xs = ints_new();
            ints_push(xs, 1);
            let e = E.Values(items: xs);
            match e { E.Values(items: ys) => ints_push(ys, 2), };
            ints_len(xs) * 10 + ints_get(xs, 1)
        }
    ";
    assert_eq!(value(src), Value::Int(22));
}

#[test]
fn matching_does_not_consume_the_value_it_reads() {
    let src = r#"
        enum E { Text(value: Str), }
        fn main() -> Int {
            let e = E.Text(value: str_concat("ab", "cd"));
            let a = match e { E.Text(value: s) => str_len(s), };
            let b = match e { E.Text(value: s) => str_len(s), };
            a + b
        }
    "#;
    assert_eq!(value(src), Value::Int(8));
}

#[test]
fn a_whole_enum_may_be_replaced_across_variants() {
    let src = r#"
        enum E { Text(value: Str), Count(n: Int), }
        fn size(e: E) -> Int { match e { E.Text(value: s) => str_len(s), E.Count(n: n) => n, } }
        fn main() -> Int {
            let mut e = E.Text(value: str_concat("abc", "de"));
            let first = size(e);
            e = E.Count(n: 7);
            let second = size(e);
            e = E.Text(value: str_concat("x", "y"));
            first * 100 + second * 10 + size(e)
        }
    "#;
    assert_eq!(value(src), Value::Int(572));
}

#[test]
fn a_failed_payload_initialiser_leaves_no_value_behind() {
    let src = r"
        enum E { V(a: Str, b: Int), }
        fn main() -> Int { match E.V(a: int_to_str(1), b: 1 / 0) { E.V(a: s, b: n) => n, } }
    ";
    let (c, _) = sole(src);
    assert_eq!(c, codes::DIVISION_BY_ZERO);
}
