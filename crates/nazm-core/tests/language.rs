//! The v0.1 language, exercised through the public `run` entry point.
//!
//! Each test names the rule in `docs/spec.md` it pins. A semantic decision that is only
//! written down is one a refactor can quietly reverse.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::{run, Outcome, Value};
use nazm_diag::codes;

fn value(src: &str) -> Value {
    match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    }
}

/// The single diagnostic code a program fails with. Asserts there is exactly one, so a
/// test cannot pass on an error it did not mean to provoke.
fn sole_error(src: &str) -> nazm_diag::Code {
    match run(src) {
        Outcome::CheckFailed(ds) => {
            assert_eq!(ds.len(), 1, "expected one error, got {ds:#?}");
            ds[0].code.clone()
        }
        Outcome::RuntimeFailed(d) => d.code,
        Outcome::Value(v) => panic!("expected an error, got {v}"),
    }
}

fn errors(src: &str) -> Vec<nazm_diag::Code> {
    match run(src) {
        Outcome::CheckFailed(ds) => ds.into_iter().map(|d| d.code).collect(),
        Outcome::RuntimeFailed(d) => vec![d.code],
        Outcome::Value(v) => panic!("expected errors, got {v}"),
    }
}

// --- the positive example ---------------------------------------------------------

#[test]
fn a_function_call_and_a_conditional() {
    let src = r"
        fn max(a: Int, b: Int) -> Int { if a > b { a } else { b } }
        fn main() -> Int { let x = 17; let y = 25; max(x, y) }
    ";
    assert_eq!(value(src), Value::Int(25));
}

#[test]
fn recursion_works_without_loops() {
    // v0.1 has no loops on purpose, so recursion has to carry the weight.
    let src = r"
        fn fact(n: Int) -> Int { if n <= 1 { 1 } else { n * fact(n - 1) } }
        fn main() -> Int { fact(10) }
    ";
    assert_eq!(value(src), Value::Int(3_628_800));
}

// --- the settled semantics --------------------------------------------------------

#[test]
fn integer_overflow_stops_the_program_rather_than_wrapping() {
    // The same in every build. A bug that reproduces only where it cannot be debugged is
    // the worst kind to be handed.
    let src = "fn main() -> Int { 9223372036854775807 + 1 }";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn the_one_division_that_overflows_is_caught_too() {
    // i64::MIN / -1 has no representable result. It is not a zero check, and a plain
    // `/` would panic rather than diagnose.
    let src = "fn main() -> Int { let a = -9223372036854775807 - 1; a / (0 - 1) }";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn division_by_zero_is_an_error_not_a_value() {
    assert_eq!(
        sole_error("fn main() -> Int { let d = 0; 10 / d }"),
        codes::DIVISION_BY_ZERO
    );
    assert_eq!(
        sole_error("fn main() -> Int { let d = 0; 10 % d }"),
        codes::DIVISION_BY_ZERO
    );
}

#[test]
fn shadowing_is_refused_in_the_same_scope_and_allowed_in_a_nested_one() {
    // Same scope: a later mention of `x` would require scanning upward to know which
    // binding it means, and a diff inserting one changes code it does not touch.
    assert_eq!(
        sole_error("fn main() -> Int { let x = 1; let x = 2; x }"),
        codes::ALREADY_DEFINED_IN_SCOPE
    );
    // Nested: the boundary is visible, so the reader can see it.
    assert_eq!(
        value("fn main() -> Int { let x = 1; if true { let x = 2; x } else { x } }"),
        Value::Int(2)
    );
}

#[test]
fn a_function_body_is_its_final_expression() {
    assert_eq!(
        value("fn main() -> Int { let a = 1; let b = 2; a + b }"),
        Value::Int(3)
    );
}

#[test]
fn arithmetic_is_left_associative() {
    // `10 - 3 - 2` must be `(10 - 3) - 2` = 5, not `10 - (3 - 2)` = 9. Silent for `+`,
    // wrong for `-`.
    assert_eq!(value("fn main() -> Int { 10 - 3 - 2 }"), Value::Int(5));
    assert_eq!(value("fn main() -> Int { 100 / 5 / 2 }"), Value::Int(10));
}

#[test]
fn multiplication_binds_tighter_than_addition_and_comparison_loosest() {
    assert_eq!(value("fn main() -> Int { 2 + 3 * 4 }"), Value::Int(14));
    assert_eq!(value("fn main() -> Bool { 1 + 1 == 2 }"), Value::Bool(true));
}

#[test]
fn a_function_sees_its_parameters_and_nothing_from_its_caller() {
    // Lexical scope, not dynamic. `hidden` is in main's scope, not callee's.
    let src = r"
        fn callee() -> Int { hidden }
        fn main() -> Int { let hidden = 5; callee() }
    ";
    assert_eq!(sole_error(src), codes::UNDEFINED_VARIABLE);
}

// --- the negative cases -----------------------------------------------------------

#[test]
fn an_undefined_variable_is_named() {
    assert_eq!(
        sole_error("fn main() -> Int { x }"),
        codes::UNDEFINED_VARIABLE
    );
}

#[test]
fn calling_a_function_with_the_wrong_number_of_arguments_is_refused() {
    let src = "fn add(a: Int, b: Int) -> Int { a + b } fn main() -> Int { add(1) }";
    assert_eq!(sole_error(src), codes::WRONG_ARGUMENT_COUNT);
}

#[test]
fn there_is_no_truthiness() {
    assert_eq!(
        sole_error("fn main() -> Int { if 1 { 1 } else { 2 } }"),
        codes::CONDITION_NOT_BOOL
    );
    assert_eq!(
        sole_error(r#"fn main() -> Int { if "x" { 1 } else { 2 } }"#),
        codes::CONDITION_NOT_BOOL
    );
}

#[test]
fn arithmetic_on_non_numbers_is_refused() {
    assert_eq!(
        sole_error("fn main() -> Int { let f = true; f + 1 }"),
        codes::INVALID_OPERANDS
    );
    assert_eq!(
        sole_error(r#"fn main() -> Str { "a" + "b" }"#),
        codes::INVALID_OPERANDS
    );
}

#[test]
fn equality_works_within_a_type_but_not_across_two() {
    assert_eq!(
        value(r#"fn main() -> Bool { "a" == "a" }"#),
        Value::Bool(true)
    );
    assert_eq!(
        value("fn main() -> Bool { true != false }"),
        Value::Bool(true)
    );
    assert_eq!(
        sole_error(r#"fn main() -> Bool { 1 == "1" }"#),
        codes::INVALID_OPERANDS
    );
}

#[test]
fn both_branches_of_an_if_must_agree() {
    let src = "fn main() -> Int { if true { 1 } else { false } }";
    assert!(
        errors(src).contains(&codes::BRANCH_TYPES_DIFFER),
        "{:?}",
        errors(src)
    );
}

#[test]
fn a_body_that_does_not_match_the_signature_is_refused() {
    assert_eq!(
        sole_error("fn main() -> Int { true }"),
        codes::TYPE_MISMATCH
    );
}

#[test]
fn an_unknown_type_is_named_rather_than_assumed() {
    let src = "fn f(x: Float) -> Float { x } fn main() -> Int { 1 }";
    assert!(
        errors(src).contains(&codes::UNKNOWN_TYPE),
        "{:?}",
        errors(src)
    );
}

#[test]
fn a_program_without_main_says_so() {
    assert_eq!(sole_error("fn other() -> Int { 1 }"), codes::MISSING_MAIN);
}

#[test]
fn a_duplicate_function_points_at_the_first_definition() {
    let src = "fn f() -> Int { 1 } fn f() -> Int { 2 } fn main() -> Int { f() }";
    match run(src) {
        Outcome::CheckFailed(ds) => {
            let d = ds
                .iter()
                .find(|d| d.code == codes::DUPLICATE_FUNCTION)
                .expect("no duplicate error");
            assert!(
                !d.secondary.is_empty(),
                "should point at the first definition"
            );
        }
        other => panic!("expected a check failure, got {other:?}"),
    }
}

// --- unsupported constructs are named, not misparsed -------------------------------

#[test]
fn absent_features_are_refused_by_name() {
    for (src, word) in [
        ("fn main() -> Int { while true { 1 } }", "while"),
        ("fn main() -> Int { return 1 }", "return"),
        ("fn main() -> Int { let mut x = 1; x }", "mut"),
        ("fn main() -> Int { match 1 { } }", "match"),
        ("fn main() -> Int { print(1) }", "print"),
    ] {
        match run(src) {
            Outcome::CheckFailed(ds) => {
                let d = ds
                    .iter()
                    .find(|d| d.code == codes::UNSUPPORTED_CONSTRUCT)
                    .unwrap_or_else(|| panic!("`{word}` was not refused by name: {ds:#?}"));
                assert!(d.message.contains(word), "{}", d.message);
                assert!(d.help.is_some(), "should say what the supported subset is");
            }
            other => panic!("`{word}` should be refused, got {other:?}"),
        }
    }
}

// --- reporting more than one error at a time ---------------------------------------

#[test]
fn three_mistakes_produce_three_errors_not_one() {
    // The whole argument for this language rests on not making an agent fix one mistake
    // per compile.
    let src =
        "fn main() -> Int { let a = undefined_one; let b = undefined_two; a + undefined_three }";
    let found = errors(src);
    assert_eq!(
        found
            .iter()
            .filter(|c| **c == codes::UNDEFINED_VARIABLE)
            .count(),
        3,
        "got {found:?}"
    );
}

// --- negation ---------------------------------------------------------------------

#[test]
fn negative_numbers_can_be_written() {
    // Added because this suite could not otherwise express `i64::MIN`. A language where
    // `-1` has to be spelled `0 - 1` is not finished.
    assert_eq!(value("fn main() -> Int { -1 }"), Value::Int(-1));
    assert_eq!(value("fn main() -> Int { let x = 5; -x }"), Value::Int(-5));
}

#[test]
fn negation_binds_tighter_than_multiplication() {
    // `-a * b` is `(-a) * b`, not `-(a * b)`. They agree numerically here, so the test
    // pins the parse rather than the arithmetic.
    assert_eq!(value("fn main() -> Int { -2 * 3 }"), Value::Int(-6));
    assert_eq!(value("fn main() -> Int { 10 - -3 }"), Value::Int(13));
}

#[test]
fn negating_the_smallest_int_overflows() {
    // The one negation with no representable result.
    let src = "fn main() -> Int { let min = -9223372036854775807 - 1; -min }";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn negation_applies_only_to_numbers() {
    assert_eq!(
        sole_error("fn main() -> Int { -true }"),
        codes::INVALID_OPERANDS
    );
}

#[test]
fn one_bad_operand_produces_one_error_not_a_cascade() {
    // `"a" + "b"` is invalid, and the expression then has no honest type. Reporting `Int`
    // for it would add a second error about a `Str` mismatch the author never wrote.
    assert_eq!(
        sole_error(r#"fn main() -> Str { "a" + "b" }"#),
        codes::INVALID_OPERANDS
    );
}
