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

/// An iteration budget small enough that a runaway loop reports in test time.
fn limits(max_iterations: u64) -> nazm_core::eval::Limits {
    nazm_core::eval::Limits {
        max_iterations,
        ..Default::default()
    }
}

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
    // And `print` is no longer one of them: it is a built-in now, checked by the
    // ordinary call rule, so a wrong argument is a type error rather than a refusal.
    assert_eq!(
        sole_error("fn main(out: OutCap) -> Int { print(1) }"),
        codes::TYPE_MISMATCH
    );

    for (src, word) in [
        ("fn main() -> Int { loop { 1 } }", "loop"),
        // `for x in y` is a loop since R1a; a `for` of neither form is still refused by name.
        ("fn main() -> Int { for x { 1 } }", "for"),
        // `print` was on this list until B3, `struct` until N9, `enum` and `match` until
        // N10, and `impl` until N78 — each stopped being a name held for a feature that did not exist and
        // became the feature. A reserved word earns its place by naming something absent;
        // once it arrives, reserving it would be refusing the thing it was reserving.
        // What `struct` and `enum` in a *body* get instead is the case below.
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

#[test]
fn match_is_no_longer_refused_by_name_and_is_refused_by_meaning() {
    // The other half of the note above, checked rather than asserted: `match` reaches the
    // checker now, and what refuses `match 1 { }` is the rule that a `match` needs an
    // enum — not a reserved word.
    assert_eq!(
        sole_error("fn main() -> Int { match 1 { } }"),
        codes::MATCH_ON_NON_ENUM
    );
}

#[test]
fn a_record_declared_inside_a_function_is_refused_where_it_was_written() {
    // `struct` is no longer absent, so it is no longer refused by name — but a
    // declaration in a body is still wrong, and "expected an expression" would be true
    // and useless. The message says where it belongs.
    match run("fn main() -> Int { struct S { x: Int, } 0 }") {
        Outcome::CheckFailed(ds) => {
            let d = &ds[0];
            assert_eq!(d.code, codes::UNEXPECTED_TOKEN);
            assert!(d.message.contains("top level"), "{}", d.message);
            assert!(d.help.is_some(), "should say where a record goes");
        }
        other => panic!("expected a refusal, got {other:?}"),
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

// --- v0.2: mutation --------------------------------------------------------------

#[test]
fn a_mutable_binding_can_be_reassigned() {
    assert_eq!(
        value("fn main() -> Int { let mut n = 1; n = n + 41; n }"),
        Value::Int(42)
    );
}

#[test]
fn an_immutable_binding_cannot_be() {
    // Immutable by default: mutable-by-default with an opt-out would put the annotation
    // on the rarer case and leave the common one silent.
    assert_eq!(
        sole_error("fn main() -> Int { let x = 1; x = 2; x }"),
        codes::ASSIGN_TO_IMMUTABLE
    );
}

#[test]
fn the_immutability_error_suggests_the_fix() {
    match run("fn main() -> Int { let x = 1; x = 2; x }") {
        Outcome::CheckFailed(ds) => {
            let d = &ds[0];
            assert!(!d.fixes.is_empty(), "should suggest `mut`");
            assert!(d.fixes[0].replacement.contains("mut"), "{:?}", d.fixes);
        }
        other => panic!("expected a check failure, got {other:?}"),
    }
}

#[test]
fn parameters_are_never_mutable() {
    // A mutable parameter is a local copy, which reads as an out-parameter to anyone who
    // has not checked the calling convention.
    let src = "fn f(mut n: Int) -> Int { n } fn main() -> Int { f(1) }";
    assert!(
        errors(src).contains(&codes::MUTABLE_PARAMETER),
        "{:?}",
        errors(src)
    );
}

#[test]
fn assignment_cannot_change_a_bindings_type() {
    // A variable whose type depends on how far through the function you are is exactly
    // what a reader cannot predict.
    let src = "fn main() -> Int { let mut n = 1; n = true; n }";
    assert!(
        errors(src).contains(&codes::TYPE_MISMATCH),
        "{:?}",
        errors(src)
    );
}

#[test]
fn assigning_to_an_undefined_name_is_not_a_declaration() {
    // `n = 1;` does not introduce `n`; only `let` does.
    assert_eq!(
        sole_error("fn main() -> Int { n = 1; 0 }"),
        codes::UNDEFINED_VARIABLE
    );
}

#[test]
fn every_use_of_an_undefined_name_is_reported() {
    // Two sites, two errors. Not a cascade: each is a real place needing an edit, unlike
    // an invented follow-on error of a different kind.
    let found = errors("fn main() -> Int { n = 1; n }");
    assert_eq!(
        found
            .iter()
            .filter(|c| **c == codes::UNDEFINED_VARIABLE)
            .count(),
        2,
        "{found:?}"
    );
}

#[test]
fn assignment_is_a_statement_so_it_cannot_be_a_condition() {
    // `if (x = 1)` is one of the oldest C bugs; nothing is gained by reproducing it.
    let src = "fn main() -> Int { let mut x = 1; if (x = 1) { 1 } else { 2 } }";
    assert!(
        !errors(src).is_empty(),
        "an assignment in a condition must not parse"
    );
}

// --- v0.2: loops -----------------------------------------------------------------

#[test]
fn a_while_loop_accumulates() {
    let src = r"
        fn main() -> Int {
            let mut total = 0;
            let mut i = 1;
            while i <= 100 { total = total + i; i = i + 1; }
            total
        }
    ";
    assert_eq!(value(src), Value::Int(5050));
}

#[test]
fn a_loop_body_assigns_to_the_outer_binding_not_a_shadow() {
    // If the body created a fresh binding each iteration, every accumulator in the
    // language would silently do nothing and still compile.
    let src = r"
        fn main() -> Int {
            let mut n = 0;
            while n < 3 { n = n + 1; }
            n
        }
    ";
    assert_eq!(value(src), Value::Int(3));
}

#[test]
fn a_loop_body_needs_no_tail_expression() {
    // A `Block` must end in an expression because it is one; a loop body has no value to
    // give, so requiring a tail would force every loop to end with something pointless.
    assert_eq!(
        value("fn main() -> Int { let mut i = 0; while i < 2 { i = i + 1; } i }"),
        Value::Int(2)
    );
}

#[test]
fn a_while_condition_must_be_bool() {
    let src = "fn main() -> Int { let mut i = 0; while i { i = i + 1; } i }";
    assert_eq!(sole_error(src), codes::CONDITION_NOT_BOOL);
}

#[test]
fn a_loop_that_never_ends_reports_rather_than_hanging() {
    // The guard is the interpreter's, not the language's. Without it a non-terminating
    // program produces nothing at all, and a tool that hangs is worse than one that says
    // what happened.
    let src = "fn main() -> Int { let mut i = 0; while i >= 0 { i = i + 1; } i }";
    match nazm_core::run_with_limits(src, limits(1000)) {
        Outcome::RuntimeFailed(d) => assert_eq!(d.code, codes::ITERATION_LIMIT),
        other => panic!("expected the iteration limit, got {other:?}"),
    }
}

#[test]
fn a_terminating_loop_is_unaffected_by_the_guard() {
    let src = "fn main() -> Int { let mut i = 0; while i < 10 { i = i + 1; } i }";
    match nazm_core::run_with_limits(src, limits(1000)) {
        Outcome::Value(v) => assert_eq!(v, Value::Int(10)),
        other => panic!("expected 10, got {other:?}"),
    }
}

#[test]
fn a_let_inside_a_loop_body_does_not_leak_between_iterations() {
    let src = r"
        fn main() -> Int {
            let mut n = 0;
            while n < 3 { let step = 1; n = n + step; }
            n
        }
    ";
    assert_eq!(value(src), Value::Int(3));
}

#[test]
fn loops_nest() {
    let src = r"
        fn main() -> Int {
            let mut count = 0;
            let mut i = 0;
            while i < 3 {
                let mut j = 0;
                while j < 4 { count = count + 1; j = j + 1; }
                i = i + 1;
            }
            count
        }
    ";
    assert_eq!(value(src), Value::Int(12));
}

// --- v0.3: break and continue -----------------------------------------------------

#[test]
fn break_leaves_the_loop() {
    let src = r"
        fn main() -> Int {
            let mut i = 0;
            while i < 100 {
                if i == 7 { break; }
                i = i + 1;
            }
            i
        }
    ";
    assert_eq!(value(src), Value::Int(7));
}

#[test]
fn continue_starts_the_next_iteration_and_re_evaluates_the_condition() {
    // Sums only the odd numbers below 10, by skipping the even ones.
    let src = r"
        fn main() -> Int {
            let mut total = 0;
            let mut i = 0;
            while i < 10 {
                i = i + 1;
                if i % 2 == 0 { continue; }
                total = total + i;
            }
            total
        }
    ";
    assert_eq!(value(src), Value::Int(25));
}

#[test]
fn break_affects_only_the_innermost_loop() {
    // There are no labels, so the outer loop must keep going. If `break` escaped to it,
    // this would be 1 rather than 3.
    let src = r"
        fn main() -> Int {
            let mut outer = 0;
            let mut rounds = 0;
            while outer < 3 {
                let mut inner = 0;
                while inner < 10 {
                    if inner == 1 { break; }
                    inner = inner + 1;
                }
                rounds = rounds + 1;
                outer = outer + 1;
            }
            rounds
        }
    ";
    assert_eq!(value(src), Value::Int(3));
}

#[test]
fn continue_affects_only_the_innermost_loop() {
    let src = r"
        fn main() -> Int {
            let mut count = 0;
            let mut i = 0;
            while i < 3 {
                i = i + 1;
                let mut j = 0;
                while j < 4 {
                    j = j + 1;
                    if j == 2 { continue; }
                    count = count + 1;
                }
            }
            count
        }
    ";
    assert_eq!(value(src), Value::Int(9));
}

#[test]
fn break_outside_a_loop_says_so() {
    assert_eq!(
        sole_error("fn main() -> Int { let mut i = 0; if i == 0 { break; } i }"),
        codes::NOT_IN_A_LOOP
    );
    assert_eq!(
        sole_error("fn main() -> Int { let mut i = 0; if i == 0 { continue; } i }"),
        codes::NOT_IN_A_LOOP
    );
}

#[test]
fn break_has_no_value_so_it_cannot_be_used_as_one() {
    // Rust gives `break` the never type and makes this work. A never type is a real
    // addition with consequences for every inference rule, and not worth adding so that
    // `break` can sit somewhere it has no business being.
    let src = "fn main() -> Int { let mut i = 0; while i < 2 { let x = break; i = i + x; } i }";
    assert_eq!(sole_error(src), codes::CONTROL_FLOW_AS_VALUE);
}

#[test]
fn code_after_break_is_refused() {
    let src = "fn main() -> Int { let mut i = 0; while i < 2 { break; i = i + 1; } i }";
    assert_eq!(sole_error(src), codes::UNREACHABLE_STATEMENT);
}

#[test]
fn code_after_continue_is_refused_too() {
    let src = "fn main() -> Int { let mut i = 0; while i < 2 { i = i + 1; continue; i = 99; } i }";
    assert_eq!(sole_error(src), codes::UNREACHABLE_STATEMENT);
}

// --- v0.3: the two forms of `if` ---------------------------------------------------

#[test]
fn a_statement_if_needs_no_else() {
    // The statement form has no value, so there is nothing for a missing branch to fail
    // to produce.
    let src = r"
        fn main() -> Int {
            let mut n = 0;
            if true { n = 1; }
            n
        }
    ";
    assert_eq!(value(src), Value::Int(1));
}

#[test]
fn an_if_expression_still_requires_else() {
    // Without `else` it would have no value when the condition is false.
    let src = "fn main() -> Int { let x = if true { 1 }; x }";
    assert!(
        !errors(src).is_empty(),
        "an `if` used as a value needs both branches"
    );
}

#[test]
fn an_if_at_the_end_of_a_block_is_its_value() {
    // The same syntax means the expression form here, because nothing follows it.
    assert_eq!(
        value("fn main() -> Int { if true { 1 } else { 2 } }"),
        Value::Int(1)
    );
}

#[test]
fn an_if_followed_by_more_is_a_statement() {
    // ...and the statement form here, because something does. Both branches assign, and
    // neither produces a value.
    let src = r"
        fn main() -> Int {
            let mut n = 0;
            if false { n = 1; } else { n = 2; }
            n
        }
    ";
    assert_eq!(value(src), Value::Int(2));
}

#[test]
fn a_statement_if_condition_must_still_be_bool() {
    let src = "fn main() -> Int { let mut n = 0; if n { n = 1; } n }";
    assert_eq!(sole_error(src), codes::CONDITION_NOT_BOOL);
}

#[test]
fn a_statement_if_gets_its_own_scope() {
    // A `let` inside it must not leak out, or the binding would exist only when the
    // condition happened to be true.
    let src = "fn main() -> Int { if true { let hidden = 1; } hidden }";
    assert_eq!(sole_error(src), codes::UNDEFINED_VARIABLE);
}

// ---------------------------------------------------------------------------
// v0.3 — `return`
//
// The table below is the one the design review asked for. Each row is a rule that
// distinguishes `return` from a tail expression; a language that gets any of them wrong
// is not one an agent can reason about locally.
// ---------------------------------------------------------------------------

#[test]
fn a_return_value_must_match_the_declared_return_type() {
    let src = r#"
        fn f(n: Int) -> Int {
            if n > 0 {
                return "positive";
            }
            0
        }
        fn main() -> Int { f(1) }
    "#;
    assert_eq!(sole_error(src), codes::TYPE_MISMATCH);
}

#[test]
fn return_leaves_the_function_not_merely_the_loop() {
    // The distinction `break` does not make. A `return` inside nested loops inside an
    // `if` must skip everything after them, including the outer loop's remaining work.
    let src = r"
        fn find(limit: Int) -> Int {
            let mut i = 0;
            while i < limit {
                let mut j = 0;
                while j < limit {
                    if i * j == 12 {
                        return i * 100 + j;
                    }
                    j = j + 1;
                }
                i = i + 1;
            }
            -1
        }
        fn main() -> Int { find(10) }
    ";
    // 2 * 6, reached before 3 * 4: the first hit wins, so the loops really did stop.
    assert_eq!(value(src), Value::Int(206));
}

#[test]
fn return_in_a_callee_does_not_return_from_its_caller() {
    // The bug this pins is a `Flow::Return` escaping one call frame into the next. The
    // caller must see a value and keep going.
    let src = r"
        fn early() -> Int {
            return 1;
        }
        fn main() -> Int {
            let a = early();
            let b = early();
            a + b + early()
        }
    ";
    assert_eq!(value(src), Value::Int(3));
}

#[test]
fn a_body_where_every_branch_returns_needs_no_tail_expression() {
    // Demanding one would mean writing an expression after code that cannot reach it.
    let src = r"
        fn sign(n: Int) -> Int {
            if n > 0 {
                return 1;
            } else {
                return 0 - 1;
            }
        }
        fn main() -> Int { sign(5) + sign(0 - 5) }
    ";
    assert_eq!(value(src), Value::Int(0));
}

#[test]
fn when_only_one_branch_returns_the_other_must_still_produce_the_value() {
    let src = r"
        fn f(n: Int) -> Int {
            if n > 0 {
                return 1;
            }
        }
        fn main() -> Int { f(1) }
    ";
    assert_eq!(sole_error(src), codes::TYPE_MISMATCH);
}

#[test]
fn one_branch_returning_and_the_other_yielding_a_value_is_accepted() {
    // The companion to the case above: `Diverges` on one side must take the other side's
    // type rather than forcing a mismatch.
    let src = r"
        fn f(n: Int) -> Int {
            if n > 0 {
                return 1;
            }
            2
        }
        fn main() -> Int { f(1) * 10 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(12));
}

#[test]
fn a_branch_that_returns_takes_the_other_branchs_type() {
    // Inside an `if`/`else` used as a value: one side leaves the function, so the other
    // side alone decides what the `if` produces. Treating the two as "different branch
    // types" would reject the exact shape `return` exists for.
    let src = r"
        fn f(n: Int) -> Int {
            if n > 0 {
                return 1;
            } else {
                2
            }
        }
        fn main() -> Int { f(1) * 10 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(12));
}

#[test]
fn a_statement_after_an_unconditional_return_is_unreachable() {
    // Same policy as `break` and `continue`: refused, not warned about. A statement that
    // cannot run is a mistake about control flow, and reporting it is the whole point.
    let src = r"
        fn f() -> Int {
            return 1;
            let x = 2;
            x
        }
        fn main() -> Int { f() }
    ";
    assert_eq!(sole_error(src), codes::UNREACHABLE_STATEMENT);
}

#[test]
fn a_bare_return_is_refused_until_valueless_returns_are_defined() {
    // Nazm has no unit value to give it, so accepting `return;` would mean inventing one
    // silently. Refusing by name keeps the decision open and visible.
    let src = r"
        fn f() -> Int {
            return;
        }
        fn main() -> Int { f() }
    ";
    assert_eq!(sole_error(src), codes::UNSUPPORTED_CONSTRUCT);
}

#[test]
fn the_tail_expression_still_returns() {
    // `return` is an addition, not a replacement.
    let src = r"
        fn f() -> Int { 7 }
        fn main() -> Int { f() }
    ";
    assert_eq!(value(src), Value::Int(7));
}

#[test]
fn return_is_not_an_expression() {
    // `let x = return 1;` would need a type for something that never produces a value.
    let src = r"
        fn f() -> Int {
            let x = return 1;
            x
        }
        fn main() -> Int { f() }
    ";
    assert!(
        !errors(src).is_empty(),
        "`return` in value position must fail"
    );
}

// ---------------------------------------------------------------------------
// Call depth
//
// The iteration budget bounds loops and nothing else. Recursion needs its own guard, or
// the process dies by stack overflow with no diagnostic — the one failure mode a tool
// cannot explain to anyone.
// ---------------------------------------------------------------------------

// Unbounded recursion is tested from `nazm-cli/tests/cli.rs`, not here. If the guard were
// ever removed, those programs would abort the process — and an in-process test cannot
// survive to report that. Run as a subprocess, the abort is just a child exit the test
// reads. The cases below stay in-process because each one stops well inside the stack
// whether the guard fires or not.

#[test]
fn the_default_depth_is_reached_before_the_stack_is() {
    // The claim `DEFAULT_MAX_DEPTH` rests on, tested rather than asserted: recursing to
    // the bound on `EVAL_STACK_BYTES` must produce a diagnostic. If a future construct
    // makes each level dear enough to exhaust the stack first, this test aborts the
    // process — which is exactly the report we want, and far better than a user finding
    // out the same way.
    let depth = nazm_core::eval::DEFAULT_MAX_DEPTH;
    let src = format!(
        r"
        fn down(n: Int) -> Int {{
            if n == 0 {{ 0 }} else {{ down(n - 1) }}
        }}
        fn main() -> Int {{ down({}) }}
    ",
        depth + 1
    );
    match run(&src) {
        Outcome::RuntimeFailed(d) => assert_eq!(d.code, codes::CALL_DEPTH_LIMIT),
        other => panic!("expected the call-depth limit, got {other:?}"),
    }
}

#[test]
fn a_smaller_depth_budget_is_honoured() {
    // Deep enough to exceed a 16-call budget, shallow enough that removing the guard
    // entirely would return a value rather than exhaust the stack — so this reports the
    // guard's absence instead of dying of it.
    let src = r"
        fn down(n: Int) -> Int {
            if n == 0 { 0 } else { down(n - 1) }
        }
        fn main() -> Int { down(100) }
    ";
    let budget = nazm_core::eval::Limits {
        max_depth: 16,
        ..Default::default()
    };
    match nazm_core::run_with_limits(src, budget) {
        Outcome::RuntimeFailed(d) => assert_eq!(d.code, codes::CALL_DEPTH_LIMIT),
        other => panic!("expected the call-depth limit, got {other:?}"),
    }
}

#[test]
fn mutual_recursion_counts_against_the_same_budget() {
    // Counting frames per function rather than overall would miss this entirely.
    let src = r"
        fn ping(n: Int) -> Int { if n == 0 { 0 } else { pong(n - 1) } }
        fn pong(n: Int) -> Int { if n == 0 { 0 } else { ping(n - 1) } }
        fn main() -> Int { ping(100) }
    ";
    let budget = nazm_core::eval::Limits {
        max_depth: 16,
        ..Default::default()
    };
    match nazm_core::run_with_limits(src, budget) {
        Outcome::RuntimeFailed(d) => assert_eq!(d.code, codes::CALL_DEPTH_LIMIT),
        other => panic!("expected the call-depth limit, got {other:?}"),
    }
}

#[test]
fn recursion_within_the_budget_is_unaffected() {
    // A guard that refused legitimate recursion would be worse than none: it would push
    // people to write loops where recursion is the clearer statement.
    let src = r"
        fn sum_to(n: Int) -> Int {
            if n == 0 { 0 } else { n + sum_to(n - 1) }
        }
        fn main() -> Int { sum_to(100) }
    ";
    assert_eq!(value(src), Value::Int(5050));
}

#[test]
fn depth_is_released_when_a_call_returns() {
    // Many sequential calls are not deep ones. Failing to decrement on the way out would
    // make a long `main` hit the limit for no reason.
    let src = r"
        fn one() -> Int { 1 }
        fn main() -> Int {
            let mut total = 0;
            let mut i = 0;
            while i < 5000 {
                total = total + one();
                i = i + 1;
            }
            total
        }
    ";
    assert_eq!(value(src), Value::Int(5000));
}

// ---------------------------------------------------------------------------
// Integer boundaries
//
// Unary negation made these writable; it did not make them correct. `docs/spec.md` says
// overflow traps identically in every build, and the two's-complement asymmetry is where
// that promise is easiest to break.
// ---------------------------------------------------------------------------

#[test]
fn the_smallest_int_can_be_written() {
    // Its magnitude is one greater than the largest `Int`, so the sign has to fold into
    // the literal. Lexing the digits alone and negating afterwards would overflow, and a
    // language whose smallest value is unwritable has an arbitrary hole in its range.
    assert_eq!(
        value("fn main() -> Int { -9223372036854775808 }"),
        Value::Int(i64::MIN)
    );
}

#[test]
fn the_largest_int_can_be_written() {
    assert_eq!(
        value("fn main() -> Int { 9223372036854775807 }"),
        Value::Int(i64::MAX)
    );
}

#[test]
fn a_literal_past_the_largest_int_is_refused() {
    assert_eq!(
        sole_error("fn main() -> Int { 9223372036854775808 }"),
        codes::INTEGER_TOO_LARGE
    );
}

#[test]
fn a_negative_literal_past_the_smallest_int_is_refused() {
    // One past `i64::MIN`. The fold must check the magnitude, not assume any digit string
    // following `-` fits.
    assert_eq!(
        sole_error("fn main() -> Int { -9223372036854775809 }"),
        codes::INTEGER_TOO_LARGE
    );
}

#[test]
fn negating_the_smallest_int_traps() {
    // `-i64::MIN` has no representation. In Rust this wraps in release and panics in
    // debug; the spec says Nazm must do the same thing in every build, and trapping is
    // that thing.
    let src = r"
        fn main() -> Int {
            let smallest = -9223372036854775808;
            0 - smallest
        }
    ";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn unary_negation_of_the_smallest_int_traps_too() {
    // The same value reached through the prefix operator rather than subtraction. These
    // are different code paths and only one of them was exercised above.
    let src = r"
        fn negate(n: Int) -> Int { -n }
        fn main() -> Int { negate(-9223372036854775808) }
    ";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn dividing_the_smallest_int_by_minus_one_traps() {
    // The quotient is one past `i64::MAX`. On x86 this faults in hardware, so a checked
    // division is the only way it reports rather than killing the process.
    let src = r"
        fn main() -> Int {
            let smallest = -9223372036854775808;
            smallest / (0 - 1)
        }
    ";
    assert_eq!(sole_error(src), codes::INTEGER_OVERFLOW);
}

#[test]
fn the_remainder_of_the_smallest_int_by_minus_one_is_zero() {
    // Mathematically zero, and zero is representable — but the hardware instruction that
    // computes it faults, and Rust's `checked_rem` reports `None`. Nazm answers 0, so
    // that `%` has one failure case (a zero divisor) rather than two.
    let src = r"
        fn main() -> Int {
            let smallest = -9223372036854775808;
            smallest % (0 - 1)
        }
    ";
    assert_eq!(value(src), Value::Int(0));
}

// ---------------------------------------------------------------------------
// Stack safety for the paths that are not call depth
//
// Deeply nested expressions and blocks recurse through the parser, the checker, the
// evaluator and the destructor of the tree, without adding a single call frame. The
// call-depth budget does not see them at all.
// ---------------------------------------------------------------------------

/// `fn main() -> Int { <depth> nested blocks around 1 }` — the costliest shape.
fn deeply_nested(depth: usize) -> String {
    format!(
        "fn main() -> Int {{ {}1{} }}",
        "{ ".repeat(depth),
        " }".repeat(depth)
    )
}

#[test]
fn a_deeply_nested_program_runs_end_to_end() {
    // Parse, check, evaluate and drop, all on the interpreter's stack. Nothing here
    // increases call depth, so `--max-depth` would never have caught it.
    assert_eq!(value(&deeply_nested(200)), Value::Int(1));
}

#[test]
fn a_long_flat_function_is_not_deep_nesting() {
    // One statement after another is not nesting. A counter that went up on the way in
    // and not back down on the way out would refuse this at the 512th `let`, which is an
    // ordinary length for generated code.
    let body: String = (0..1000).map(|i| format!("let v{i} = {i};\n")).collect();
    let src = format!("fn main() -> Int {{\n{body}v999\n}}");
    assert_eq!(value(&src), Value::Int(999));
}

#[test]
fn nesting_past_the_parser_limit_reports_rather_than_crashing() {
    assert_eq!(
        sole_error(&deeply_nested(nazm_syntax::MAX_NESTING + 1)),
        codes::NESTING_TOO_DEEP
    );
}

// A raised depth budget being given the stack to match, and absurd nesting being
// refused, are both tested from `nazm-cli/tests/cli.rs`. Take either guarantee away and
// those programs abort the process — which an in-process test cannot survive to report.
// The cases that stay here all finish well inside the stack either way.

#[test]
fn a_raised_budget_still_runs_what_fits_inside_it() {
    // The companion: the budget is raised and the program stays within it, so it must
    // produce its value rather than trip a limit the stack was resized to accommodate.
    // The recursion is kept well inside the base reservation so that this test reports a
    // wrong answer rather than aborting when the sizing is what has broken. It was 1,000
    // levels until N39's campaign showed a debug build does not fit that in the base 24 MiB:
    // with the sizing taken away this binary aborted, and the mutant was a crash rather than
    // a catch. 200 levels fit on any build, and `nazm-cli/tests/cli.rs` holds the raised
    // budget itself out of process, where an abort is an exit code.
    let src = r"
        fn sum_to(n: Int) -> Int {
            if n == 0 { 0 } else { n + sum_to(n - 1) }
        }
        fn main() -> Int { sum_to(200) }
    ";
    let raised = nazm_core::eval::Limits {
        max_depth: 5_000,
        ..Default::default()
    };
    match nazm_core::run_with_limits(src, raised) {
        Outcome::Value(v) => assert_eq!(v, Value::Int(20_100)),
        other => panic!("expected a value, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// A function whose body did not parse
// ---------------------------------------------------------------------------

#[test]
fn a_function_with_no_body_is_never_run_as_an_empty_one() {
    // Keeping the signature must not turn a body that failed to parse into a body that
    // does nothing. Until N39 this drove the interpreter directly over the partial tree;
    // the interpreter now runs Core IR, and Core IR exists only for a program that checked,
    // so there is no route by which such a body could run — the run is refused at checking
    // and nothing is evaluated.
    let src = "fn f() -> Int { return; }\nfn main() -> Int { f() }";
    let (program, errors) = nazm_syntax::parse_source(src);
    assert!(!errors.is_empty(), "the body should have failed to parse");
    assert!(
        program
            .functions
            .iter()
            .find(|f| f.name == "f")
            .is_some_and(|f| f.body.is_none()),
        "the signature should survive, and the body should be absent, not empty"
    );
    match nazm_core::run(src) {
        Outcome::CheckFailed(ds) => {
            assert!(
                ds.iter().any(|d| d.code == codes::UNSUPPORTED_CONSTRUCT),
                "{ds:?}"
            );
        }
        other => panic!("a function with no body must not run, got {other:?}"),
    }
}

#[test]
fn a_body_that_failed_to_parse_is_not_type_checked() {
    // Nothing is said about statements that were never built. A "must produce an Int"
    // error here would be a claim about the parser's guess, not about the program — and
    // it would be the second error for one mistake.
    let src = "fn f() -> Int { return; }\nfn main() -> Int { f() }";
    assert_eq!(sole_error(src), codes::UNSUPPORTED_CONSTRUCT);
}

// ---------------------------------------------------------------------------
// `return` from inside an expression
//
// `return` is a statement, but a block is an expression — so a `return` can sit inside
// anything used as a value. The checker allows it (a diverging branch takes the other
// branch's type), which means the evaluator has to as well.
// ---------------------------------------------------------------------------

#[test]
fn return_works_from_a_let_initializer() {
    let src = r"
        fn f(n: Int) -> Int {
            let x = if n > 0 { return 1; } else { 2 };
            x + 10
        }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(112));
}

#[test]
fn return_works_from_a_binary_operand() {
    // And the left-to-right rule still holds: a flow raised on the left leaves before
    // the right operand is evaluated at all.
    let src = r"
        fn f(n: Int) -> Int { 1 + if n > 0 { return 5; } else { 2 } }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(503));
}

#[test]
fn return_works_from_a_call_argument() {
    let src = r"
        fn g(n: Int) -> Int { n }
        fn f(n: Int) -> Int { g(if n > 0 { return 7; } else { 2 }) }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(702));
}

#[test]
fn return_works_from_a_condition() {
    // A condition is an expression like any other.
    let src = r"
        fn f(n: Int) -> Int {
            if (if n > 0 { return 3; } else { false }) { 1 } else { 2 }
        }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(302));
}

#[test]
fn return_works_from_a_loop_condition() {
    let src = r"
        fn f(n: Int) -> Int {
            let mut i = 0;
            while (if n > 0 { return 4; } else { i < 3 }) {
                i = i + 1;
            }
            i
        }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(403));
}

#[test]
fn return_works_from_an_assigned_value() {
    let src = r"
        fn f(n: Int) -> Int {
            let mut x = 0;
            x = if n > 0 { return 6; } else { 9 };
            x
        }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(609));
}

#[test]
fn a_nested_return_wins_over_the_one_it_is_inside() {
    // `return (if c { return 1; } else { 2 });` — the inner one leaves first, so the
    // outer never gets a value to return.
    let src = r"
        fn f(n: Int) -> Int {
            return if n > 0 { return 1; } else { 2 };
        }
        fn main() -> Int { f(1) * 100 + f(0) }
    ";
    assert_eq!(value(src), Value::Int(102));
}

#[test]
fn break_works_from_inside_an_expression_too() {
    // The same mechanism, and the case that would have been missed by fixing `return`
    // alone: `break` inside a value position leaves the loop rather than erroring.
    let src = r"
        fn main() -> Int {
            let mut total = 0;
            let mut i = 0;
            while i < 10 {
                total = total + if i == 3 { break; } else { i };
                i = i + 1;
            }
            total
        }
    ";
    // 0 + 1 + 2, then the fourth iteration leaves before adding anything.
    assert_eq!(value(src), Value::Int(3));
}

// ---------------------------------------------------------------------------
// Long binary chains
//
// `1 + 1 + 1` is `((1 + 1) + 1)`: the parse loop does not recurse, but the tree it
// builds nests one level per operator, and every pass over it does.
// ---------------------------------------------------------------------------

#[test]
fn a_long_binary_chain_is_counted_as_the_nesting_it_is() {
    // Four times the limit. The chain that could only abort — 100,000 terms — is in the
    // CLI suite, for the same reason the deepest trees are.
    let chain = vec!["1"; nazm_syntax::MAX_NESTING * 4].join(" + ");
    assert_eq!(
        sole_error(&format!("fn main() -> Int {{ {chain} }}")),
        codes::NESTING_TOO_DEEP
    );
}

#[test]
fn a_chain_within_the_limit_still_evaluates() {
    let chain = vec!["1"; 400].join(" + ");
    assert_eq!(
        value(&format!("fn main() -> Int {{ {chain} }}")),
        Value::Int(400)
    );
}

#[test]
fn chains_in_sequence_are_not_cumulative() {
    // Each statement's tree is walked on its own, so a budget that never came back down
    // between them would refuse an ordinary function.
    let chain = vec!["1"; 400].join(" + ");
    let body: String = (0..20).map(|i| format!("let v{i} = {chain};\n")).collect();
    assert_eq!(
        value(&format!("fn main() -> Int {{\n{body}v19\n}}")),
        Value::Int(400)
    );
}

// ---------------------------------------------------------------------------
// Strings as bytes (B1)
// ---------------------------------------------------------------------------

#[test]
fn str_len_counts_bytes_not_characters() {
    assert_eq!(
        value("fn main() -> Int { str_len(\"hello\") }").to_string(),
        "5"
    );
    assert_eq!(value("fn main() -> Int { str_len(\"\") }").to_string(), "0");
    // Two bytes, one character. The specification says bytes, so this is 2.
    assert_eq!(
        value("fn main() -> Int { str_len(\"é\") }").to_string(),
        "2"
    );
    // Four bytes, one character.
    assert_eq!(
        value("fn main() -> Int { str_len(\"😀\") }").to_string(),
        "4"
    );
}

#[test]
fn str_byte_reads_one_byte_and_refuses_to_leave_the_string() {
    assert_eq!(
        value("fn main() -> Int { str_byte(\"abc\", 0) }").to_string(),
        "97"
    );
    assert_eq!(
        value("fn main() -> Int { str_byte(\"abc\", 2) }").to_string(),
        "99"
    );
    // The first byte of a two-byte sequence, as a byte: 0xC3.
    assert_eq!(
        value("fn main() -> Int { str_byte(\"é\", 0) }").to_string(),
        "195"
    );

    assert_eq!(
        sole_error("fn main() -> Int { str_byte(\"abc\", 3) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_byte(\"abc\", 0 - 1) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_byte(\"\", 0) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
}

#[test]
fn str_slice_is_half_open_and_may_split_a_character() {
    assert_eq!(
        value("fn main() -> Int { str_len(str_slice(\"hello\", 1, 4)) }").to_string(),
        "3"
    );
    // Empty slices at both ends are legal, including one at the very end.
    assert_eq!(
        value("fn main() -> Int { str_len(str_slice(\"hello\", 2, 2)) }").to_string(),
        "0"
    );
    assert_eq!(
        value("fn main() -> Int { str_len(str_slice(\"hello\", 5, 5)) }").to_string(),
        "0"
    );
    assert_eq!(
        value("fn main() -> Int { str_len(str_slice(\"hello\", 0, 5)) }").to_string(),
        "5"
    );

    // Splitting a multi-byte sequence is allowed and produces a one-byte `Str`.
    assert_eq!(
        value("fn main() -> Int { str_byte(str_slice(\"é\", 0, 1), 0) }").to_string(),
        "195"
    );

    assert_eq!(
        sole_error("fn main() -> Int { str_len(str_slice(\"abc\", 2, 1)) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_len(str_slice(\"abc\", 0, 4)) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_len(str_slice(\"abc\", 0 - 1, 2)) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
}

#[test]
fn str_concat_and_equality_compare_bytes() {
    assert_eq!(
        value("fn main() -> Int { if str_concat(\"ab\", \"cd\") == \"abcd\" { 1 } else { 0 } }")
            .to_string(),
        "1"
    );
    assert_eq!(
        value("fn main() -> Int { if str_concat(\"\", \"x\") == \"x\" { 1 } else { 0 } }")
            .to_string(),
        "1"
    );
    // No normalisation and no case folding.
    assert_eq!(
        value("fn main() -> Int { if \"A\" == \"a\" { 1 } else { 0 } }").to_string(),
        "0"
    );
    assert_eq!(
        value("fn main() -> Int { if \"ab\" != \"abc\" { 1 } else { 0 } }").to_string(),
        "1"
    );
}

#[test]
fn int_to_str_round_trips_the_whole_range() {
    assert_eq!(
        value("fn main() -> Int { str_len(int_to_str(0)) }").to_string(),
        "1"
    );
    assert_eq!(
        value("fn main() -> Int { if int_to_str(0) == \"0\" { 1 } else { 0 } }").to_string(),
        "1"
    );
    assert_eq!(
        value("fn main() -> Int { if int_to_str(0 - 42) == \"-42\" { 1 } else { 0 } }").to_string(),
        "1"
    );
    assert_eq!(value("fn main() -> Int { if int_to_str(9223372036854775807) == \"9223372036854775807\" { 1 } else { 0 } }").to_string(), "1");
    // Written with unary minus: `9223372036854775808` on its own is `N0003`, and the
    // lexer is right to say so — only the negated form is in range.
    assert_eq!(value("fn main() -> Int { if int_to_str(-9223372036854775808) == \"-9223372036854775808\" { 1 } else { 0 } }").to_string(), "1");
}

#[test]
fn a_builtin_may_not_be_redefined_or_miscalled() {
    assert_eq!(
        sole_error("fn str_len(s: Str) -> Int { 0 }\nfn main() -> Int { 1 }"),
        codes::DUPLICATE_FUNCTION
    );
    // The ordinary call rule applies to built-ins: arity and argument types.
    assert_eq!(
        sole_error("fn main() -> Int { str_len(1) }"),
        codes::TYPE_MISMATCH
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_len(\"a\", \"b\") }"),
        codes::WRONG_ARGUMENT_COUNT
    );
    assert_eq!(
        sole_error("fn main() -> Int { str_byte(\"a\") }"),
        codes::WRONG_ARGUMENT_COUNT
    );
    // `str_slice` returns `Str`, so using it where an `Int` is wanted is a type error.
    assert_eq!(
        sole_error("fn main() -> Int { str_slice(\"abc\", 0, 1) }"),
        codes::TYPE_MISMATCH
    );
    // Strings order by their bytes since R1a (L6); a string and a number still do not compare.
    assert_eq!(
        sole_error("fn main() -> Int { if \"a\" < 1 { 1 } else { 0 } }"),
        codes::INVALID_OPERANDS
    );
}

#[test]
fn strings_travel_through_parameters_locals_and_loops() {
    // The shape a tokeniser is made of: walk a string byte by byte, accumulate.
    assert_eq!(
        value(
            "fn count(s: Str, want: Int) -> Int {\n\
         \x20   let mut i = 0;\n\
         \x20   let mut n = 0;\n\
         \x20   while i < str_len(s) {\n\
         \x20       if str_byte(s, i) == want { n = n + 1; }\n\
         \x20       i = i + 1;\n\
         \x20   }\n\
         \x20   n\n\
         }\n\
         fn main() -> Int { count(\"banana\", 97) }"
        )
        .to_string(),
        "3"
    );
}

// ---------------------------------------------------------------------------
// Sequences (B2)
// ---------------------------------------------------------------------------

#[test]
fn a_sequence_is_a_handle_to_shared_storage() {
    // The property everything else rests on: two bindings, one sequence.
    assert_eq!(
        value(
            "fn main() -> Int {\n\
             \x20   let a = ints_new();\n\
             \x20   let b = a;\n\
             \x20   ints_push(a, 1);\n\
             \x20   ints_push(b, 2);\n\
             \x20   ints_len(a) * 10 + ints_len(b)\n\
             }"
        )
        .to_string(),
        "22"
    );
    // And across a function boundary: the callee mutates the caller's sequence.
    assert_eq!(
        value(
            "fn fill(a: Ints, n: Int) -> Int {\n\
             \x20   let mut i = 0;\n\
             \x20   while i < n { ints_push(a, i); i = i + 1; }\n\
             \x20   ints_len(a)\n\
             }\n\
             fn main() -> Int {\n\
             \x20   let a = ints_new();\n\
             \x20   fill(a, 4);\n\
             \x20   ints_len(a)\n\
             }"
        )
        .to_string(),
        "4"
    );
}

#[test]
fn sequence_reads_and_writes_are_bounds_checked() {
    assert_eq!(
        value("fn main() -> Int { let a = ints_new(); ints_push(a, 7); ints_get(a, 0) }")
            .to_string(),
        "7"
    );
    assert_eq!(
        value(
            "fn main() -> Int { let a = ints_new(); ints_push(a, 7); ints_set(a, 0, 9); ints_get(a, 0) }"
        )
        .to_string(),
        "9"
    );
    assert_eq!(
        sole_error("fn main() -> Int { let a = ints_new(); ints_get(a, 0) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { let a = ints_new(); ints_push(a, 1); ints_get(a, 1) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error(
            "fn main() -> Int { let a = ints_new(); ints_push(a, 1); ints_set(a, 0 - 1, 2) }"
        ),
        codes::INDEX_OUT_OF_BOUNDS
    );
}

#[test]
fn strs_holds_strings_by_value() {
    // A `Str` read out of a sequence is a value: changing the sequence afterwards does
    // not change the string already read.
    assert_eq!(
        value(
            "fn main() -> Int {\n\
             \x20   let a = strs_new();\n\
             \x20   strs_push(a, \"hello\");\n\
             \x20   let first = strs_get(a, 0);\n\
             \x20   strs_set(a, 0, \"x\");\n\
             \x20   str_len(first) * 10 + str_len(strs_get(a, 0))\n\
             }"
        )
        .to_string(),
        "51"
    );
}

#[test]
fn sequences_are_not_comparable() {
    // `==` would have to mean identity or contents, and the language has chosen
    // neither. Refused rather than answered arbitrarily.
    assert_eq!(
        sole_error("fn main() -> Int { let a = ints_new(); let b = ints_new(); if a == b { 1 } else { 0 } }"),
        codes::INVALID_OPERANDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { let a = strs_new(); if a != a { 1 } else { 0 } }"),
        codes::INVALID_OPERANDS
    );
    // Nor mixable with anything else.
    assert_eq!(
        sole_error("fn main() -> Int { let a = ints_new(); ints_len(a) + a }"),
        codes::INVALID_OPERANDS
    );
}

#[test]
fn an_expression_may_be_a_statement() {
    // Without this, every sequence mutation would need a binding nobody wants.
    assert_eq!(
        value("fn main() -> Int { let a = ints_new(); ints_push(a, 1); ints_push(a, 2); ints_len(a) }")
            .to_string(),
        "2"
    );
    assert_eq!(value("fn main() -> Int { 1 + 1; 3 }").to_string(), "3");
}

// ---------------------------------------------------------------------------
// Codes that were reachable and unasserted
//
// A diagnostic code is part of the machine interface — `docs/diagnostics.md` says a code
// never changes meaning and is never reused, which makes each one a promise. Checking
// which codes any test actually asserts found four that none did. Two of them are
// provoked by four lines of Nazm, so they are asserted here; the other two need the
// machine to run out of something, and `crates/nazm-cli/tests/codes.rs` records them as
// unprovokable by name rather than leaving the gap silent.
// ---------------------------------------------------------------------------

#[test]
fn a_syntax_error_reports_n0100_and_says_what_it_wanted() {
    let found = errors("fn main() -> Int { let = 1; 0 }");
    assert!(
        found.contains(&codes::UNEXPECTED_TOKEN),
        "expected N0100 among {found:?}"
    );
}

#[test]
fn a_file_that_cannot_be_read_reports_n0407() {
    // The path is one no tree contains, and the failure is the language's, not the
    // harness's: `read_file` is an ordinary built-in and this is what it does when the
    // outside world says no.
    assert_eq!(
        sole_error(
            r#"fn main(io: IoCap) -> Int { str_len(read_file("/no/such/file/at/all.txt")) }"#
        ),
        codes::IO_FAILED
    );
}

#[test]
fn popping_an_empty_sequence_reports_rather_than_inventing_a_value() {
    // Both sequence types, because they are two code paths and the `Strs` one had no
    // test of its own until the built-in coverage check asked which built-ins were
    // never named in one.
    assert_eq!(
        sole_error("fn main() -> Int { let a = ints_new(); ints_pop(a) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
    assert_eq!(
        sole_error("fn main() -> Int { let a = strs_new(); str_len(strs_pop(a)) }"),
        codes::INDEX_OUT_OF_BOUNDS
    );
}

#[test]
fn a_pop_returns_the_last_item_and_shortens_the_sequence() {
    assert_eq!(
        value(
            "fn main() -> Int { let a = strs_new(); strs_push(a, \"ab\"); \
             strs_push(a, \"cde\"); let last = strs_pop(a); \
             str_len(last) * 10 + strs_len(a) }"
        )
        .to_string(),
        "31"
    );
}
