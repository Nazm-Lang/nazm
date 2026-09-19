//! The independent acceptance runner.
//!
//! Grades a **frozen submission** using tests the agent never saw, in a directory the
//! agent never had write access to. Three properties, each corresponding to a way a
//! benchmark can report a confident wrong answer:
//!
//! * **The tests come from the corpus, not the workspace.** An agent that edits the visible
//!   tests changes what *it* sees passing, and nothing about the grade.
//! * **The graded bytes are the recorded bytes.** The submission digest is re-verified
//!   before grading, so a lingering process editing the tree produces a void result rather
//!   than a quietly different one.
//! * **Evaluator failure is not candidate failure — and candidate failure is not void.**
//!   A database that would not start is the evaluator's fault and voids the attempt;
//!   scoring it as "the arm failed" would penalise whichever arm ran while the machine was
//!   unwell. But a candidate that crashes, emits nothing, or blows a resource limit is a
//!   *failure*, and it stays in the denominator. Voiding it would delete an arm's worst
//!   attempts and make an unreliable arm look good.

use serde::{Deserialize, Serialize};

use crate::outcome::Outcome;
use crate::workspace::{Submission, SubmissionDrift};

/// What the acceptance runner concluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Every independent test passed.
    Accepted,
    /// The submission ran and failed its independent tests.
    Rejected { failing: Vec<String> },
    /// The candidate crashed, produced nothing usable, or blew a resource limit. This is a
    /// **failure and stays in the denominator** — it is a fact about the arm.
    CandidateFault { reason: String },
    /// The evaluator could not produce a judgement. Void, and not attributable to the arm.
    EvaluatorFault { reason: String },
    /// The frozen submission is not the one recorded. Void.
    Void { reason: String },
}

impl Verdict {
    /// Map to the outcome recorded for the attempt.
    ///
    /// `visible_passed` is what the agent's own test run reported. The interesting case is
    /// `visible_passed && Rejected`: a solution fitted to the tests it could see, which is
    /// the single most informative failure this harness can detect.
    #[must_use]
    pub fn into_outcome(self, visible_passed: bool) -> Outcome {
        match self {
            Self::Accepted => Outcome::Accepted,
            Self::Rejected { failing } if visible_passed => {
                Outcome::VisiblePassedAcceptanceFailed { failing }
            }
            Self::Rejected { .. } => Outcome::WrongAnswer,
            // Counted. A crash is a result about the arm.
            Self::CandidateFault { .. } => Outcome::Crash { signal: None },
            Self::EvaluatorFault { reason } | Self::Void { reason } => {
                Outcome::HarnessError { reason }
            }
        }
    }

    /// Whether this verdict leaves the analysis entirely.
    #[must_use]
    pub fn is_void(&self) -> bool {
        matches!(self, Self::EvaluatorFault { .. } | Self::Void { .. })
    }
}

/// Where the independent tests live. Deliberately a distinct type from anything the agent
/// can reach, so "grade against the workspace" cannot be written by accident.
/// One independent check. Real checks will shell out to a test process; the signature is
/// what matters — it takes a *frozen* submission and nothing else, so a check cannot reach
/// the live workspace even by mistake.
pub type Check = fn(&Submission) -> CheckResult;

#[derive(Debug, Clone)]
pub struct AcceptanceSuite {
    task_id: String,
    checks: Vec<(String, Check)>,
}

/// Why a check did not pass — and, critically, **whose fault it was**.
///
/// An earlier version had a single `CouldNotRun` that mapped to evaluator infrastructure
/// failure, which made a candidate producing no output a *void* attempt. That deletes an
/// arm's worst attempts from the denominator: an arm that crashes or emits garbage on hard
/// tasks would look better than one that merely gets them wrong. The distinction below is
/// the whole point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    Pass,
    /// Ran, produced a wrong answer.
    Fail,
    /// The candidate is at fault: it crashed, produced no output, emitted something
    /// unparseable, or exceeded a resource limit. **A failure, not a void attempt.**
    CandidateFault(String),
    /// The evaluator is at fault: the database would not start, the test runner itself
    /// crashed, the fixture is missing. **Void** — not attributable to the arm.
    EvaluatorFault(String),
}

impl AcceptanceSuite {
    #[must_use]
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            checks: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_check(
        mut self,
        name: impl Into<String>,
        f: fn(&Submission) -> CheckResult,
    ) -> Self {
        self.checks.push((name.into(), f));
        self
    }

    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Grade a frozen submission.
    ///
    /// Takes `&Submission`, not a workspace path: the type makes it impossible to grade a
    /// live tree the agent may still be writing to.
    #[must_use]
    pub fn grade(&self, submission: &Submission) -> Verdict {
        // The recorded bytes must still be the bytes on disk. A process that outlived its
        // timeout and kept writing shows up here rather than in the score.
        if let Err(drift) = submission.verify_unchanged() {
            return match drift {
                SubmissionDrift::Changed { .. } => Verdict::Void {
                    reason: drift.to_string(),
                },
                SubmissionDrift::Unreadable(_) => Verdict::EvaluatorFault {
                    reason: drift.to_string(),
                },
            };
        }

        let mut failing = Vec::new();
        for (name, check) in &self.checks {
            match check(submission) {
                CheckResult::Pass => {}
                CheckResult::Fail => failing.push(name.clone()),
                // The candidate broke. That is a failure and it stays counted — deleting
                // it would remove an arm's worst attempts from the denominator.
                CheckResult::CandidateFault(reason) => {
                    return Verdict::CandidateFault {
                        reason: format!("check `{name}`: {reason}"),
                    };
                }
                // The evaluator broke. Reporting "3 of 4 passed" when the fourth never ran
                // would be a judgement the runner did not make, so the whole grade voids.
                CheckResult::EvaluatorFault(reason) => {
                    return Verdict::EvaluatorFault {
                        reason: format!("check `{name}` could not run: {reason}"),
                    };
                }
            }
        }

        if failing.is_empty() {
            Verdict::Accepted
        } else {
            Verdict::Rejected { failing }
        }
    }
}
