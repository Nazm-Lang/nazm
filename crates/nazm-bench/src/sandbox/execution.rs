//! One attempt, end to end: candidate work → freeze → fresh acceptance state → grade →
//! record → verified cleanup.
//!
//! # Why the grade is provisional until cleanup verifies
//!
//! Grading and cleanup answer different questions, and the second can invalidate the
//! first. A submission that passes every acceptance test is still not a usable result if
//! teardown left a container running, because the *next* attempt then starts from state
//! this one left behind — and an independence violation cannot be detected after the
//! fact. So the grade is recorded as provisional, cleanup runs, and only a verified-clean
//! teardown finalises it.
//!
//! What must survive a failed cleanup is the accounting. The money was spent and the
//! diagnostics were gathered whether or not the containers went away, and discarding them
//! would hide real cost behind an infrastructure problem.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::acceptance::Verdict;
use crate::cost::{Cost, CostComponent};
use crate::engine::{ContainerEngine, ResourceId, ResourceKind};
use crate::outcome::{Limit, Outcome};
use crate::sandbox::attempt::{Disposition, Sandbox};

/// What one attempt produced, and whether it can be used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub attempt_id: String,
    pub outcome: Outcome,
    pub cost: Cost,
    /// The digest of the exact bytes that were graded.
    pub submission_digest: Option<String>,
    /// Container wall-clock, per role, in milliseconds. `None` where it could not be
    /// read — never silently zero, because a zero runtime is a claim and an unreadable
    /// clock is not.
    pub durations_ms: Vec<(String, Option<u64>)>,
    /// Whether teardown verified clean.
    pub cleanup: CleanupOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CleanupOutcome {
    Verified,
    /// Cleanup failed or could not be verified. The grade stays provisional and the run
    /// must not continue to another attempt.
    Failed {
        reason: String,
        surviving: Vec<String>,
    },
}

impl AttemptRecord {
    /// Whether this record may be reported.
    ///
    /// A grade obtained in an attempt whose cleanup did not verify is not wrong, but it
    /// cannot be distinguished from one contaminated by a previous attempt's leftovers.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self.cleanup, CleanupOutcome::Verified)
    }
}

/// A submission the harness can grade, plus how it was produced.
///
/// Deliberately not a workspace path: grading reads the frozen bytes, and the type is
/// what stops a live tree the candidate may still be writing to from being graded.
#[derive(Debug, Clone)]
pub struct Graded {
    pub verdict: Verdict,
    pub digest: String,
    pub visible_passed: bool,
}

/// Stop the candidate before anything is frozen.
///
/// SIGKILL of the container, not of a client: once PID 1 of the namespace is gone the
/// kernel signals every remaining task in it, which is what reaches processes the
/// candidate detached. Nothing can be frozen while something can still write.
pub fn stop_candidate<E: ContainerEngine>(
    sandbox: &Sandbox<'_, E>,
    grace: Duration,
) -> Result<StopOutcome, crate::engine::EngineError> {
    let id = ResourceId::new(ResourceKind::Container, &sandbox.names().candidate);

    sandbox.kill_candidate()?;

    // Confirm it actually stopped rather than assuming the kill took effect.
    match sandbox.engine().wait_until_exit(&id, grace)? {
        Some(state) => Ok(StopOutcome {
            attempt: sandbox.names().attempt.clone(),
            exit_code: state.exit_code,
            oom_killed: state.oom_killed,
            started_at: state.started_at,
            finished_at: state.finished_at,
        }),
        // The container is still running after a SIGKILL and a grace period. That is an
        // evaluator problem, not a candidate one: nothing the candidate can do prevents
        // a namespace teardown, so this means the daemon is wedged.
        None => Err(crate::engine::EngineError::RemovalInProgress { id }),
    }
}

impl StopOutcome {
    /// The evidence that the workspace is quiet, for [`crate::workspace::Workspace::freeze`].
    ///
    /// Available only from a `StopOutcome`, and one of those exists only when the
    /// candidate container was killed and then *observed* to have exited — see
    /// [`stop_candidate`], which returns an error rather than a `StopOutcome` when the
    /// container is still running after the grace period.
    ///
    /// The container is the only writer: the work volume is mounted writable into the
    /// candidate and nowhere else, and killing the container takes its descendants with
    /// it, detached ones included.
    #[must_use]
    pub fn writers_stopped(&self) -> crate::workspace::WritersStopped {
        crate::workspace::WritersStopped::confirmed(
            &self.attempt,
            format!(
                "candidate container observed exited (code {:?}, oom {})",
                self.exit_code, self.oom_killed
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopOutcome {
    /// Which attempt's candidate this is the stop of.
    pub attempt: String,
    pub exit_code: Option<i32>,
    pub oom_killed: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Classify how the candidate's own run ended, from engine state only.
///
/// `logs` is passed for the record, not for the decision. Candidate-controlled output can
/// say "Killed" or "out of memory" whenever it likes, and classifying from it would let a
/// submission choose its own outcome.
#[must_use]
pub fn classify_candidate(stop: &StopOutcome, timed_out: bool) -> Option<Outcome> {
    if timed_out {
        return Some(Outcome::Timeout {
            limit: Limit::WallClock,
        });
    }
    if stop.oom_killed {
        return Some(Outcome::ResourceExhausted {
            limit: Limit::Memory,
        });
    }
    // Anything else is not yet an outcome: the grade comes from the acceptance tests, and
    // a non-zero exit only means the last thing the container ran returned non-zero.
    None
}

/// Assemble the record, then let cleanup decide whether it is final.
///
/// Takes the disposition rather than running teardown itself, so a caller cannot forget
/// to do it and still get a record back.
#[must_use]
pub fn finalise(
    attempt_id: &str,
    outcome: Outcome,
    cost: Cost,
    submission_digest: Option<String>,
    durations_ms: Vec<(String, Option<u64>)>,
    disposition: &Disposition,
) -> AttemptRecord {
    let cleanup = match disposition {
        Disposition::Clean => CleanupOutcome::Verified,
        Disposition::Quarantine { surviving, reason } => CleanupOutcome::Failed {
            reason: reason.clone(),
            surviving: surviving.iter().map(ToString::to_string).collect(),
        },
    };

    AttemptRecord {
        attempt_id: attempt_id.to_owned(),
        outcome,
        // Kept whatever cleanup did. The money was spent either way, and dropping it
        // would hide real cost behind an infrastructure problem.
        cost,
        submission_digest,
        durations_ms,
        cleanup,
    }
}

/// Infrastructure cost from container wall-clock, or an explicit absence.
///
/// Docker's zero time (`0001-01-01…`) means the container never ran. Parsed naively it
/// yields a large negative duration that a `max(0, …)` would turn into a silent zero, so
/// it is rejected and the line records that the measurement is missing.
#[must_use]
pub fn infrastructure_cost(
    durations_ms: &[(String, Option<u64>)],
    micros_per_ms: i64,
) -> CostComponent {
    let mut total: i64 = 0;
    for (role, ms) in durations_ms {
        let Some(ms) = ms else {
            return CostComponent::missing(format!("container `{role}` timing unavailable"));
        };
        let Ok(ms) = i64::try_from(*ms) else {
            return CostComponent::missing(format!("container `{role}` duration out of range"));
        };
        let Some(add) = ms.checked_mul(micros_per_ms) else {
            return CostComponent::missing(format!("container `{role}` cost overflowed"));
        };
        let Some(sum) = total.checked_add(add) else {
            return CostComponent::missing("infrastructure cost overflowed".to_owned());
        };
        total = sum;
    }
    CostComponent::recorded(total)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn paid() -> Cost {
        Cost::new(
            CostComponent::recorded(1_000_000),
            CostComponent::recorded(5_000),
            CostComponent::recorded(0),
        )
    }

    #[test]
    fn a_passing_grade_is_not_final_until_cleanup_verifies() {
        let dirty = Disposition::Quarantine {
            surviving: vec![ResourceId::new(ResourceKind::Container, "leftover")],
            reason: "removal failed".to_owned(),
        };
        let r = finalise(
            "a1",
            Outcome::Accepted,
            paid(),
            Some("d".to_owned()),
            Vec::new(),
            &dirty,
        );

        assert_eq!(r.outcome, Outcome::Accepted, "the grade itself stands");
        assert!(!r.is_final(), "but it cannot be reported");
        assert!(matches!(r.cleanup, CleanupOutcome::Failed { .. }));
    }

    #[test]
    fn costs_and_diagnostics_survive_a_failed_cleanup() {
        // The money was spent whether or not the containers went away. Dropping it would
        // hide real cost behind an infrastructure problem.
        let dirty = Disposition::Quarantine {
            surviving: vec![ResourceId::new(ResourceKind::Volume, "v")],
            reason: "still referenced".to_owned(),
        };
        let r = finalise(
            "a1",
            Outcome::WrongAnswer,
            paid(),
            Some("digest-abc".to_owned()),
            vec![("cand".to_owned(), Some(1200))],
            &dirty,
        );

        assert_eq!(r.cost, paid());
        assert_eq!(r.submission_digest.as_deref(), Some("digest-abc"));
        assert_eq!(r.durations_ms, vec![("cand".to_owned(), Some(1200))]);
        match &r.cleanup {
            CleanupOutcome::Failed { surviving, .. } => assert_eq!(surviving.len(), 1),
            CleanupOutcome::Verified => panic!("should not be verified"),
        }
    }

    #[test]
    fn a_verified_cleanup_finalises_the_record() {
        let r = finalise(
            "a1",
            Outcome::Accepted,
            paid(),
            None,
            Vec::new(),
            &Disposition::Clean,
        );
        assert!(r.is_final());
    }

    #[test]
    fn a_timeout_outranks_the_exit_code() {
        let stop = StopOutcome {
            attempt: "test-attempt".to_owned(),
            exit_code: Some(0),
            oom_killed: false,
            started_at: None,
            finished_at: None,
        };
        assert_eq!(
            classify_candidate(&stop, true),
            Some(Outcome::Timeout {
                limit: Limit::WallClock
            })
        );
    }

    #[test]
    fn oom_comes_from_engine_state_not_from_the_candidates_output() {
        let stop = StopOutcome {
            attempt: "test-attempt".to_owned(),
            exit_code: Some(137),
            oom_killed: true,
            started_at: None,
            finished_at: None,
        };
        assert_eq!(
            classify_candidate(&stop, false),
            Some(Outcome::ResourceExhausted {
                limit: Limit::Memory
            })
        );

        // Exit 137 alone is not OOM: it is also what any SIGKILL produces, including the
        // harness's own stop. Classifying from it would invent resource exhaustion every
        // time an attempt was stopped normally.
        let killed_normally = StopOutcome {
            oom_killed: false,
            ..stop
        };
        assert_eq!(classify_candidate(&killed_normally, false), None);
    }

    #[test]
    fn a_missing_duration_makes_the_infrastructure_line_missing_not_zero() {
        let c = infrastructure_cost(&[("db".to_owned(), None)], 10);
        assert!(c.is_missing(), "an unreadable clock is not a zero runtime");

        let c = infrastructure_cost(
            &[
                ("db".to_owned(), Some(1000)),
                ("cand".to_owned(), Some(500)),
            ],
            10,
        );
        assert_eq!(c.value().map(crate::cost::Money::as_micros), Some(15_000));
    }
}
