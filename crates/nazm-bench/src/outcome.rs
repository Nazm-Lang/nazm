//! Attempt outcomes, and the categories that explain where post-success cost went.
//!
//! Two rules from `docs/evaluation.md` are encoded here:
//!
//! * The harness must distinguish *every* outcome class, including the one that matters
//!   most — a solution that passes the visible tests but fails the independent acceptance
//!   tests. A harness that cannot tell those apart cannot detect overfitting to the suite.
//! * Post-success activity is categorised to explain cost, **never** to exclude it. Some
//!   reverts repair real regressions; treating all post-success work as waste would be a
//!   measurement decision dressed up as an analysis label.

use serde::{Deserialize, Serialize};

/// How one attempt ended.
///
/// `Accepted` is the only variant that counts toward the acceptance rate, and it requires
/// the *independent* acceptance tests to pass — not the tests the agent could see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Independent acceptance tests passed.
    Accepted,
    /// The visible suite passed but the independent acceptance tests did not. The most
    /// informative failure the harness can record: it is the signature of a solution
    /// fitted to the tests rather than the requirement.
    VisiblePassedAcceptanceFailed { failing: Vec<String> },
    /// Produced output, wrong answer.
    WrongAnswer,
    /// Did not compile or type-check.
    CompileFailure { first_error_code: Option<String> },
    /// Compiled, then crashed.
    Crash { signal: Option<String> },
    /// Hit the wall-clock or turn cap fixed in the pre-registration.
    Timeout { limit: Limit },
    /// The attempt is void and must not enter the analysis as a failure: the harness
    /// itself broke. Kept distinct so that instrument defects cannot be mistaken for
    /// arm performance.
    HarnessError { reason: String },
    /// The agent tampered with the evaluation — edited tests, invoked a different binary.
    /// Void like `HarnessError`, but reported separately because it is a property of the
    /// agent, not the instrument.
    ProtocolViolation { detail: String },
}

impl Outcome {
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Whether this attempt contributes to the analysis at all.
    ///
    /// Void attempts are excluded from both numerator and denominator and reported as a
    /// separate count — if that count is not small, the run is not usable.
    #[must_use]
    pub fn is_void(&self) -> bool {
        matches!(
            self,
            Self::HarnessError { .. } | Self::ProtocolViolation { .. }
        )
    }

    /// A stable label for grouping in reports.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::VisiblePassedAcceptanceFailed { .. } => "visible_passed_acceptance_failed",
            Self::WrongAnswer => "wrong_answer",
            Self::CompileFailure { .. } => "compile_failure",
            Self::Crash { .. } => "crash",
            Self::Timeout { .. } => "timeout",
            Self::HarnessError { .. } => "harness_error",
            Self::ProtocolViolation { .. } => "protocol_violation",
        }
    }
}

/// Which cap was hit. They are different experimental constraints and the pre-registration
/// names which ones bind, so the harness records which one actually stopped the attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    WallClock,
    Turns,
    Tokens,
    Dollars,
}

/// What an agent did after its solution already passed.
///
/// These are **analysis labels**, not a filter. Every category's cost stays in the
/// denominator; the breakdown explains where the money went, and only `Cosmetic` and
/// `Revert` are prima facie waste — and even a revert may be repairing a regression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostSuccessCategory {
    /// Formatting, renaming, comment churn. Structurally preventable by a canonical
    /// formatter, which is why arm 2 includes one.
    Cosmetic,
    /// Re-emitting a previously tried state. Detectable, and the thing repeat-edit
    /// detection targets.
    Revert,
    /// Optimisation work. Real value if the task asked for it.
    Performance,
    /// Hardening, input validation, authorization. Frequently legitimate.
    Security,
    /// Handling a case absent from the visible suite — which the independent acceptance
    /// tests may well reward.
    EdgeCase,
}

impl PostSuccessCategory {
    /// Whether this category is *prima facie* waste. Deliberately not called
    /// `is_waste`: the judgement is contestable per attempt, and this flag only shapes
    /// how a report is sorted, never what enters the cost denominator.
    #[must_use]
    pub fn is_prima_facie_waste(self) -> bool {
        matches!(self, Self::Cosmetic | Self::Revert)
    }

    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Cosmetic,
            Self::Revert,
            Self::Performance,
            Self::Security,
            Self::EdgeCase,
        ]
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn only_independent_acceptance_counts_as_accepted() {
        let overfitted = Outcome::VisiblePassedAcceptanceFailed {
            failing: vec!["tenant_isolation".to_owned()],
        };
        assert!(!overfitted.is_accepted());
        assert!(
            !overfitted.is_void(),
            "overfitting is a real result, not a void run"
        );
    }

    #[test]
    fn instrument_and_agent_faults_are_void_but_distinguishable() {
        let harness = Outcome::HarnessError {
            reason: "container never started".to_owned(),
        };
        let cheating = Outcome::ProtocolViolation {
            detail: "modified test file".to_owned(),
        };
        assert!(harness.is_void() && cheating.is_void());
        assert_ne!(harness.label(), cheating.label());
    }

    #[test]
    fn every_outcome_has_a_distinct_label() {
        let all = [
            Outcome::Accepted,
            Outcome::VisiblePassedAcceptanceFailed { failing: vec![] },
            Outcome::WrongAnswer,
            Outcome::CompileFailure {
                first_error_code: None,
            },
            Outcome::Crash { signal: None },
            Outcome::Timeout {
                limit: Limit::WallClock,
            },
            Outcome::HarnessError {
                reason: String::new(),
            },
            Outcome::ProtocolViolation {
                detail: String::new(),
            },
        ];
        let mut labels: Vec<_> = all.iter().map(Outcome::label).collect();
        labels.sort_unstable();
        let before = labels.len();
        labels.dedup();
        assert_eq!(
            labels.len(),
            before,
            "labels must be unique to group reports correctly"
        );
    }

    #[test]
    fn post_success_categories_do_not_filter_cost() {
        // Three of five categories are legitimate work. A design that discarded
        // post-success cost wholesale would misreport every arm.
        let legitimate = PostSuccessCategory::all()
            .into_iter()
            .filter(|c| !c.is_prima_facie_waste())
            .count();
        assert_eq!(legitimate, 3);
    }

    #[test]
    fn outcomes_round_trip_through_json() {
        // Run records are read back by the analysis, which is not written in Rust.
        let o = Outcome::CompileFailure {
            first_error_code: Some("E0502".to_owned()),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("compile_failure"));
        assert_eq!(serde_json::from_str::<Outcome>(&json).unwrap(), o);
    }
}
