//! The unit of evidence: one attempt at one task by one arm.
//!
//! Every field is required. A record that cannot say which arm, which task, which
//! pre-registration, or what it cost is not a weaker record — it is not a record, and the
//! type system is where that is cheapest to enforce.

use serde::{Deserialize, Serialize};

use crate::cost::Cost;
use crate::outcome::Outcome;
use crate::prereg::PreRegistration;
use crate::task::Arm;

/// One attempt, as written to the run log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub arm: Arm,
    pub task_id: String,
    /// Which task family, so per-category results can be computed without re-reading the
    /// corpus — the gate's "no category collapse" condition needs this.
    pub family: crate::task::Family,
    pub outcome: Outcome,
    pub cost: Cost,
    /// How many times the agent was restarted on this task. Retries are attempts and their
    /// cost is in `cost`; this records that the sequence happened.
    pub retries: u32,
    /// The rules this attempt is reportable under. Recorded per attempt rather than per
    /// run so that a log merged from several runs stays interpretable.
    pub preregistration: PreRegistration,
    /// Commit of the implementation under evaluation.
    pub run_commit: String,
}

impl Attempt {
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        self.outcome.is_accepted()
    }

    /// Void attempts are excluded from both numerator and denominator and counted
    /// separately: instrument defects and agent protocol violations are not arm
    /// performance. If the void count is not small, the run is not usable.
    #[must_use]
    pub fn is_void(&self) -> bool {
        self.outcome.is_void()
    }
}
