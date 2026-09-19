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
//! * **Infrastructure failure is not candidate failure.** A database that would not start
//!   is a void attempt. Scoring it as "the arm failed" would silently penalise whichever
//!   arm happened to run when the machine was unwell.

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
    /// The runner itself could not produce a judgement. **Not** a candidate failure.
    Infrastructure { reason: String },
    /// The frozen submission is not the one recorded.
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
            Self::Infrastructure { reason } => Outcome::HarnessError { reason },
            Self::Void { reason } => Outcome::HarnessError { reason },
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    Pass,
    Fail,
    /// The check could not run. Propagates as `Infrastructure`, never as a failure.
    CouldNotRun(String),
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
                SubmissionDrift::Unreadable(_) => Verdict::Infrastructure {
                    reason: drift.to_string(),
                },
            };
        }

        let mut failing = Vec::new();
        for (name, check) in &self.checks {
            match check(submission) {
                CheckResult::Pass => {}
                CheckResult::Fail => failing.push(name.clone()),
                // One unrunnable check voids the whole grade. Reporting "3 of 4 passed"
                // when the fourth never ran would be a judgement the runner did not make.
                CheckResult::CouldNotRun(reason) => {
                    return Verdict::Infrastructure {
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
