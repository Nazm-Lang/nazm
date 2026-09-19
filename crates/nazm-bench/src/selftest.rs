//! End-to-end known-answer tests for the measurement path.
//!
//! `docs/evaluation.md` §8 requires the harness to be validated **by construction** rather
//! than by agreeing with published results. That means feeding it attempts whose correct
//! aggregate is known by hand, and checking the numbers that come out — acceptance counts,
//! denominators, totals, and comparison estimates — not merely that it runs.
//!
//! These are deliberately fixtures, not live runs. A live run cannot tell you the harness
//! is right, because you do not independently know the answer.
//!
//! Every case here exists because it is a way the instrument could report a wrong number
//! while looking healthy.

use crate::cost::{Cost, CostComponent};
use crate::outcome::{Limit, Outcome};
use crate::prereg::PreRegistration;
use crate::record::Attempt;
use crate::report;
use crate::task::{Arm, Family};

/// Result of one self-test case.
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

fn prereg() -> PreRegistration {
    PreRegistration {
        commit: "0000000".to_owned(),
        version: 1,
        corpus_digest: "sha256:fixture".to_owned(),
        freeze_tag: "fixture".to_owned(),
    }
}

fn attempt(arm: Arm, family: Family, outcome: Outcome, cost: Cost) -> Attempt {
    Attempt {
        arm,
        task_id: "fixture".to_owned(),
        family,
        outcome,
        cost,
        retries: 0,
        preregistration: prereg(),
        run_commit: "1111111".to_owned(),
    }
}

fn paid(micros: i64) -> Cost {
    Cost::new(
        CostComponent::recorded(micros),
        CostComponent::recorded(0),
        CostComponent::recorded(0),
    )
}

fn check(name: &'static str, passed: bool, detail: impl Into<String>) -> CaseResult {
    CaseResult {
        name,
        passed,
        detail: detail.into(),
    }
}

/// Run every case. Returns them all so the caller can report failures rather than
/// stopping at the first.
#[must_use]
pub fn run() -> Vec<CaseResult> {
    vec![
        failure_costs_stay_in_the_cost_numerator(),
        acceptance_rate_divides_by_every_counted_attempt(),
        the_two_metrics_use_different_denominators(),
        void_attempts_leave_both_numerator_and_denominator(),
        a_timeout_is_a_failure_not_a_void(),
        overfitting_is_not_acceptance(),
        one_missing_cost_invalidates_the_arm(),
        retries_are_counted_and_their_cost_kept(),
        record_count_does_not_invert_arm_ordering(),
        the_comparison_is_against_the_tooled_arm(),
        category_collapse_is_detected(),
        no_verdict_is_produced_here(),
        void_spend_is_reported_not_discarded(),
        holdout_is_refused_before_the_freeze(),
        a_forged_freeze_fails_against_the_repository(),
    ]
}

/// Three failures and one acceptance: cost per accepted is the whole spend, not a quarter
/// of it. An arm that fails cheaply and often is not cheap.
///
/// Named for the *numerator*, deliberately. An earlier version called this "failed
/// attempts stay in the denominator", which conflated two metrics with two different
/// denominators:
///
/// ```text
/// cost per accepted = cost of EVERY attempt / count of ACCEPTED attempts
/// acceptance rate   = count of ACCEPTED    / count of EVERY counted attempt
/// ```
///
/// Failures belong in the cost **numerator** and in the acceptance-rate **denominator** —
/// never in the cost-per-accepted denominator. The two are exercised separately below,
/// because a mutation in one does not necessarily show up in the other.
fn failure_costs_stay_in_the_cost_numerator() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);
    let got = s.cost_per_accepted.map(|m| m.as_micros());
    check(
        "failure costs stay in the cost numerator",
        got == Some(4_000_000) && s.accepted == 1 && s.attempts == 4,
        format!(
            "expected $4.00 per accepted over 4 attempts, got {got:?} over {}",
            s.attempts
        ),
    )
}

/// A harness crash is not the arm's failure. Void attempts are excluded from both sides
/// and counted separately, so an instrument defect cannot look like poor performance.
fn void_attempts_leave_both_numerator_and_denominator() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::HarnessError {
                reason: "container never started".to_owned(),
            },
            paid(500_000),
        ),
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::ProtocolViolation {
                detail: "edited the test file".to_owned(),
            },
            paid(500_000),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);
    check(
        "void attempts leave both numerator and denominator",
        s.attempts == 1
            && s.void == 2
            && s.cost_per_accepted.map(|m| m.as_micros()) == Some(1_000_000),
        format!(
            "expected 1 counted / 2 void / $1.00, got {} / {} / {:?}",
            s.attempts,
            s.void,
            s.cost_per_accepted.map(|m| m.as_micros())
        ),
    )
}

/// A timeout is a real result about the arm, so it counts. Confusing it with a void
/// attempt would quietly delete the most expensive failures from the denominator.
fn a_timeout_is_a_failure_not_a_void() -> CaseResult {
    let attempts = vec![attempt(
        Arm::Nazm,
        Family::IdempotentWebhook,
        Outcome::Timeout {
            limit: Limit::WallClock,
        },
        paid(9_000_000),
    )];
    let s = report::summarise(Arm::Nazm, &attempts);
    check(
        "a timeout is a failure, not a void attempt",
        s.attempts == 1 && s.void == 0 && s.accepted == 0,
        format!(
            "expected 1 counted / 0 void / 0 accepted, got {} / {} / {}",
            s.attempts, s.void, s.accepted
        ),
    )
}

/// Passing the visible suite while failing independent acceptance is the signature of a
/// solution fitted to the tests. It must not count as acceptance.
fn overfitting_is_not_acceptance() -> CaseResult {
    let attempts = vec![attempt(
        Arm::Nazm,
        Family::TenantIsolatedApi,
        Outcome::VisiblePassedAcceptanceFailed {
            failing: vec!["cross_tenant_read".to_owned()],
        },
        paid(1_000_000),
    )];
    let s = report::summarise(Arm::Nazm, &attempts);
    check(
        "passing visible tests is not acceptance",
        s.accepted == 0 && s.attempts == 1 && s.void == 0,
        format!(
            "expected 0 accepted over 1 counted attempt, got {} over {}",
            s.accepted, s.attempts
        ),
    )
}

/// The failure mode that can manufacture a winner: one dropped usage record must
/// invalidate the arm's cost figure rather than shrinking it.
fn one_missing_cost_invalidates_the_arm() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            Cost::new(
                CostComponent::missing("provider returned no usage block"),
                CostComponent::recorded(0),
                CostComponent::recorded(0),
            ),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);
    check(
        "one missing cost invalidates the arm's figure",
        s.cost_per_accepted.is_none() && !s.cost.is_complete() && s.accepted == 2,
        format!(
            "expected no cost figure despite 2 acceptances, got {:?}",
            s.cost_per_accepted.map(|m| m.as_micros())
        ),
    )
}

/// Retries are attempts. Their cost stays in the total and the retry count is preserved,
/// so a run that succeeded only after five restarts cannot read like a clean success.
fn retries_are_counted_and_their_cost_kept() -> CaseResult {
    let mut a = attempt(
        Arm::Nazm,
        Family::CancellableJob,
        Outcome::Accepted,
        paid(6_000_000),
    );
    a.retries = 5;
    let s = report::summarise(Arm::Nazm, &[a.clone()]);
    check(
        "retries are counted and their cost kept",
        a.retries == 5 && s.cost_per_accepted.map(|m| m.as_micros()) == Some(6_000_000),
        format!(
            "expected the full $6.00 retained, got {:?}",
            s.cost_per_accepted.map(|m| m.as_micros())
        ),
    )
}

/// The reviewer's truncation counterexample, end to end. Two records of $0.60 must remain
/// more expensive than one record of $1.00; per-record rounding would invert them.
fn record_count_does_not_invert_arm_ordering() -> CaseResult {
    let a2 = vec![
        attempt(
            Arm::TypescriptTooled,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(600_000),
        ),
        attempt(
            Arm::TypescriptTooled,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(600_000),
        ),
    ];
    let a3 = vec![attempt(
        Arm::Nazm,
        Family::ValidatedCrudApi,
        Outcome::Accepted,
        paid(1_000_000),
    )];

    let s2 = report::summarise(Arm::TypescriptTooled, &a2);
    let s3 = report::summarise(Arm::Nazm, &a3);
    let (c2, c3) = (
        s2.cost.lower_bound_with_gaps(),
        s3.cost.lower_bound_with_gaps(),
    );
    check(
        "record count does not invert arm ordering",
        c2.as_micros() == 1_200_000 && c3.as_micros() == 1_000_000 && c2 > c3,
        format!("expected A2 $1.20 > A3 $1.00, got {c2} vs {c3}"),
    )
}

/// Comparing Nazm against the baseline would credit the language with the entire tooling
/// contribution. The comparator must be the tooled arm.
fn the_comparison_is_against_the_tooled_arm() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::TypescriptBaseline,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(10_000_000),
        ),
        attempt(
            Arm::TypescriptTooled,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(2_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
    ];
    let summaries: Vec<_> = Arm::all()
        .iter()
        .map(|a| report::summarise(*a, &attempts))
        .collect();
    let nazm = summaries.iter().find(|s| s.arm == Arm::Nazm).cloned();
    let Some(nazm) = nazm else {
        return check(
            "the comparison is against the tooled arm",
            false,
            "no Nazm summary",
        );
    };
    let Some(cmp) = report::compare(&nazm, &summaries) else {
        return check(
            "the comparison is against the tooled arm",
            false,
            "no comparison produced",
        );
    };
    // $1.00 against A2's $2.00 is a ratio of 0.5. Against A1's $10.00 it would be 0.1 —
    // the difference between a real result and a flattering one.
    let ratio_ok = cmp.cost_ratio.is_some_and(|r| (r - 0.5).abs() < 1e-9);
    check(
        "the comparison is against the tooled arm",
        cmp.comparator == Arm::TypescriptTooled && ratio_ok,
        format!(
            "expected comparator A2 and ratio 0.5, got {:?} and {:?}",
            cmp.comparator, cmp.cost_ratio
        ),
    )
}

/// A single family where Nazm collapses must surface, even when the overall numbers look
/// acceptable — that is exactly the shape of result the condition exists to catch.
fn category_collapse_is_detected() -> CaseResult {
    let mut attempts = Vec::new();
    for _ in 0..10 {
        attempts.push(attempt(
            Arm::TypescriptTooled,
            Family::TenantIsolatedApi,
            Outcome::Accepted,
            paid(1_000_000),
        ));
        attempts.push(attempt(
            Arm::TypescriptTooled,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ));
        attempts.push(attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ));
        // Nazm fails the tenant-isolation family outright.
        attempts.push(attempt(
            Arm::Nazm,
            Family::TenantIsolatedApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ));
    }
    let summaries: Vec<_> = Arm::all()
        .iter()
        .map(|a| report::summarise(*a, &attempts))
        .collect();
    let Some(nazm) = summaries.iter().find(|s| s.arm == Arm::Nazm).cloned() else {
        return check("category collapse is detected", false, "no Nazm summary");
    };
    let Some(cmp) = report::compare(&nazm, &summaries) else {
        return check(
            "category collapse is detected",
            false,
            "no comparison produced",
        );
    };
    check(
        "category collapse is detected",
        cmp.collapsed_families.contains(&Family::TenantIsolatedApi),
        format!(
            "expected TenantIsolatedApi flagged, got {:?}",
            cmp.collapsed_families
        ),
    )
}

/// The harness must not emit a verdict. The gate needs confidence bounds from a
/// mixed-effects model, and a point estimate dressed as a decision is worse than none.
fn no_verdict_is_produced_here() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::TypescriptTooled,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(2_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
    ];
    let summaries: Vec<_> = Arm::all()
        .iter()
        .map(|a| report::summarise(*a, &attempts))
        .collect();
    let Some(nazm) = summaries.iter().find(|s| s.arm == Arm::Nazm).cloned() else {
        return check("no verdict is produced here", false, "no Nazm summary");
    };
    let Some(cmp) = report::compare(&nazm, &summaries) else {
        return check(
            "no verdict is produced here",
            false,
            "no comparison produced",
        );
    };
    check(
        "no verdict is produced here",
        cmp.requires_external_analysis,
        "the comparison must declare that the decision needs the external analysis",
    )
}

/// Deliberate holdout-access violation: the harness API must refuse a holdout fixture
/// before the implementation freeze.
///
/// Note the limit this case does *not* cover, stated in `task`'s module docs: the refusal
/// is a workflow control over this API, not filesystem isolation. Nothing here stops a
/// process that reads the corpus directory directly.
fn holdout_is_refused_before_the_freeze() -> CaseResult {
    use crate::task::{AccessError, TaskSet, TaskSpec};

    let task = TaskSpec::new(
        "held-01",
        Family::TenantIsolatedApi,
        TaskSet::Holdout,
        "holdout/held-01",
    );
    let refused = matches!(
        task.open(None),
        Err(AccessError::HoldoutBeforeFreeze { .. })
    );

    let dev = TaskSpec::new(
        "dev-01",
        Family::ValidatedCrudApi,
        TaskSet::Development,
        "dev/dev-01",
    );
    let allowed = dev.open(None).is_ok();

    check(
        "holdout is refused before the freeze",
        refused && allowed,
        format!("expected holdout refused and development allowed, got {refused} and {allowed}"),
    )
}

/// A `Freeze` can be constructed by anyone, so the value alone proves nothing. Verified
/// against the repository, a fabricated one must fail.
fn a_forged_freeze_fails_against_the_repository() -> CaseResult {
    use crate::task::{Freeze, FreezeError};

    let Ok(forged) = Freeze::new("tag-that-was-never-created", "deadbeef") else {
        return check(
            "a forged freeze fails against the repository",
            false,
            "could not build fixture",
        );
    };
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let outcome = forged.verify_against_repo(repo);
    let rejected = matches!(
        outcome,
        // A machine without git is not a harness defect, but it is also not a pass, so it
        // is reported rather than silently treated as success.
        Err(FreezeError::UnknownTag { .. } | FreezeError::RepoUnreadable(_))
    );
    check(
        "a forged freeze fails against the repository",
        rejected,
        format!("expected a fabricated freeze to be rejected, got {outcome:?}"),
    )
}

/// The acceptance-rate denominator is every counted attempt, not just the acceptances.
///
/// This case exists because it was missing, and the gap was demonstrated rather than
/// assumed: a mutation dividing acceptances by acceptances — pinning the rate at 1.0 for
/// every arm — passed the entire self-test suite. Δp is a primary gate estimand and the
/// absolute 60% usability floor reads this number directly, so an arm could have looked
/// perfect on both while failing three attempts in four.
fn acceptance_rate_divides_by_every_counted_attempt() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Timeout {
                limit: Limit::Turns,
            },
            paid(1_000_000),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);
    let rate = s.acceptance_rate;
    check(
        "acceptance rate divides by every counted attempt",
        rate.is_some_and(|r| (r - 0.25).abs() < 1e-9),
        format!("expected 1 of 4 counted attempts = 0.25, got {rate:?}"),
    )
}

/// The two metrics must not share a denominator.
///
/// ```text
/// cost per accepted = cost of EVERY attempt / count of ACCEPTED attempts
/// acceptance rate   = count of ACCEPTED     / count of EVERY counted attempt
/// ```
///
/// Constructed so a single shared denominator cannot satisfy both: the cost figure is
/// $4.00 only when divided by 1 acceptance, and the rate is 0.25 only when divided by 4
/// counted attempts. Any implementation using one denominator for both fails here,
/// whichever one it picked.
fn the_two_metrics_use_different_denominators() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::CancellableJob,
            Outcome::WrongAnswer,
            paid(1_000_000),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);
    let cost = s.cost_per_accepted.map(|m| m.as_micros());
    let rate = s.acceptance_rate;
    check(
        "the two metrics use different denominators",
        cost == Some(4_000_000) && rate.is_some_and(|r| (r - 0.25).abs() < 1e-9),
        format!("expected cost $4.00 (/1) and rate 0.25 (/4), got {cost:?} and {rate:?}"),
    )
}

/// Void attempts cost real money, and a run that voids constantly must not look cheap.
///
/// Their spend stays out of `cost_per_accepted` — void work is not attributable to the arm
/// — but it is reported, and the void rate with it. An arm voiding far more often than its
/// comparator means the comparison is partly a measurement artifact, which is a finding in
/// its own right rather than something to average away.
fn void_spend_is_reported_not_discarded() -> CaseResult {
    let attempts = vec![
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::Accepted,
            paid(1_000_000),
        ),
        attempt(
            Arm::Nazm,
            Family::ValidatedCrudApi,
            Outcome::HarnessError {
                reason: "test database refused connections".to_owned(),
            },
            paid(3_000_000),
        ),
    ];
    let s = report::summarise(Arm::Nazm, &attempts);

    let counted_cost = s.cost_per_accepted.map(|m| m.as_micros());
    let void_cost = s.void_cost.believable().map(|m| m.as_micros());
    let rate = s.void_rate;

    check(
        "void spend is reported, not discarded",
        counted_cost == Some(1_000_000) && void_cost == Some(3_000_000)
            && rate.is_some_and(|r| (r - 0.5).abs() < 1e-9),
        format!(
            "expected $1.00 counted, $3.00 void, rate 0.5; got {counted_cost:?}, {void_cost:?}, {rate:?}"
        ),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    #[test]
    fn every_selftest_case_passes() {
        let failures: Vec<_> = super::run().into_iter().filter(|c| !c.passed).collect();
        assert!(failures.is_empty(), "self-test failures: {failures:#?}");
    }

    #[test]
    fn the_selftest_covers_every_outcome_class() {
        // A self-test that silently stopped exercising a case would be worse than none.
        assert!(super::run().len() >= 15);
    }
}
