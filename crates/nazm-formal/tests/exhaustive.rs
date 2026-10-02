//! The formal core's properties (`docs/formal-core.md`), checked over **every** program of the
//! core up to [`BOUND`] nodes — a bounded model check, not a proof.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_formal::{arith, enumerate, eval, exact, to_source, type_of, Expr, Outcome, Ty, Value};

/// Every program up to this many nodes is checked. Five is every literal, operator, `if` and
/// `let` composed to a depth where every rule meets every other at least once.
const BOUND: usize = 5;

fn programs() -> Vec<Expr> {
    (1..=BOUND).flat_map(|n| enumerate(n, &[])).collect()
}

fn typed() -> Vec<(Expr, Ty)> {
    programs()
        .into_iter()
        .filter_map(|e| type_of(&e, &[]).map(|t| (e, t)))
        .collect()
}

#[test]
fn p1_a_well_typed_program_is_never_stuck_and_has_its_type() {
    let all = typed();
    assert!(
        all.len() > 10_000,
        "the bound covers too little: {}",
        all.len()
    );
    for (e, t) in &all {
        let r = eval(e, &[]).unwrap_or_else(|| panic!("stuck: {e:?}"));
        if let Outcome::Value(v) = r {
            let has = match v {
                Value::Int(_) => Ty::Int,
                Value::Bool(_) => Ty::Bool,
            };
            assert_eq!(has, *t, "{e:?}");
        }
    }
}

#[test]
fn p2_evaluation_is_deterministic() {
    for (e, _) in typed() {
        assert_eq!(eval(&e, &[]), eval(&e, &[]), "{e:?}");
    }
}

#[test]
fn p3_checked_arithmetic_is_the_mathematical_result_or_a_trap() {
    for op in nazm_formal::Op::ALL {
        for m in nazm_formal::INTS {
            for k in nazm_formal::INTS {
                match (arith(op, m, k), exact(op, m, k)) {
                    (Outcome::Value(Value::Int(n)), Some(x)) => assert_eq!(i128::from(n), x),
                    (Outcome::Trap(c), Some(x)) => {
                        assert_eq!(c, nazm_formal::OVERFLOW);
                        assert!(i64::try_from(x).is_err(), "{op:?} {m} {k}: {x} fits");
                    }
                    (Outcome::Trap(c), None) => assert_eq!(c, nazm_formal::DIVIDE_BY_ZERO),
                    other => panic!("{op:?} {m} {k}: {other:?}"),
                }
            }
        }
    }
}

/// What Nazm's interpreter says of a program, in the core's terms.
fn nazm_says(source: &str) -> Option<Outcome> {
    match nazm_core::run(source) {
        nazm_core::Outcome::Value(nazm_core::eval::Value::Int(n)) => {
            Some(Outcome::Value(Value::Int(n)))
        }
        nazm_core::Outcome::Value(nazm_core::eval::Value::Bool(b)) => {
            Some(Outcome::Value(Value::Bool(b)))
        }
        nazm_core::Outcome::RuntimeFailed(d) => {
            let code: &'static str = match d.code.as_str() {
                "N0400" => nazm_formal::OVERFLOW,
                "N0401" => nazm_formal::DIVIDE_BY_ZERO,
                other => panic!("an unexpected failure {other}: {source}"),
            };
            Some(Outcome::Trap(code))
        }
        nazm_core::Outcome::CheckFailed(_) => None,
        nazm_core::Outcome::Value(_) => panic!("a value of another type: {source}"),
    }
}

#[test]
fn p4_the_interpreter_means_what_the_semantics_says() {
    let mut checked = 0usize;
    for (e, t) in typed() {
        let src = to_source(&e, t);
        let formal = eval(&e, &[]).unwrap();
        let nazm = nazm_says(&src).unwrap_or_else(|| panic!("refused a well-typed program: {src}"));
        assert_eq!(nazm, formal, "{src}");
        checked += 1;
    }
    assert!(checked > 10_000, "{checked}");
}

#[test]
fn p5_the_checker_accepts_exactly_what_the_rules_type() {
    for e in programs() {
        let formal = type_of(&e, &[]);
        // An ill-typed program is printed as `Int`; the checker must refuse it whatever its
        // declared result, and accept a typed one at its type.
        let src = to_source(&e, formal.unwrap_or(Ty::Int));
        let accepted = nazm_core::check_source(&src).is_empty();
        assert_eq!(accepted, formal.is_some(), "{src}");
    }
}

#[test]
fn the_bound_covers_what_it_says() {
    // The counts are part of the claim: a change to the enumeration that covered less would
    // otherwise pass every property vacuously.
    assert_eq!(programs().len(), 94_352);
    assert_eq!(typed().len(), 27_680);
}

#[test]
fn p4_holds_where_a_right_operand_can_trap() {
    // Beyond the bound, one family where evaluation order is observable: every `a && b` whose
    // left side is a boolean of at most two nodes and whose right side is a boolean of five — the smallest that can trap —
    // among them every `false && (1 / 0 == 0)`-like program, which short-circuiting decides.
    let lefts: Vec<Expr> = (1..=2)
        .flat_map(|n| enumerate(n, &[]))
        .filter(|e| type_of(e, &[]) == Some(Ty::Bool))
        .collect();
    let rights: Vec<Expr> = enumerate(5, &[])
        .into_iter()
        .filter(|e| type_of(e, &[]) == Some(Ty::Bool))
        .collect();
    let mut traps_on_the_right = 0usize;
    for a in &lefts {
        for b in &rights {
            if matches!(eval(b, &[]), Some(Outcome::Trap(_))) {
                traps_on_the_right += 1;
            }
            let e = Expr::And(Box::new(a.clone()), Box::new(b.clone()));
            let src = to_source(&e, Ty::Bool);
            assert_eq!(nazm_says(&src), eval(&e, &[]), "{src}");
        }
    }
    assert!(traps_on_the_right > 100, "{traps_on_the_right}");
}
