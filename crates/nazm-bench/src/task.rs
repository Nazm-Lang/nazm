//! Tasks, arms, and the three task sets.
//!
//! The holdout set is the part worth encoding in types rather than documenting. Hiding
//! acceptance tests from the agent is necessary but not sufficient: if the language, its
//! diagnostics, and its wrappers are tuned against the same tasks for a year, the final
//! benchmark rewards specialisation to that set. `docs/evaluation.md` §3 therefore splits
//! tasks three ways and gates the holdout behind an implementation freeze.
//!
//! # What [`Freeze`] does and does not do
//!
//! It prevents **accidental early access through this API**. That is worth having and it
//! is all it is. It is a workflow control, not an isolation boundary:
//!
//! * anyone who can call [`Freeze::new`] can construct one, so it stops mistakes, not
//!   intent — though [`Freeze::verify_against_repo`] at least makes a forged one fail
//!   against the actual repository;
//! * it does nothing about the filesystem. An agent, a developer, or any other tool with
//!   read access to the corpus directory can read holdout fixtures without going through
//!   [`TaskSpec::open`] at all;
//! * it says nothing about repository history, logs, or CI artifacts, all of which can
//!   leak fixture content.
//!
//! **Real isolation for the gate run requires the holdout corpus to live outside the
//! agent's accessible environment entirely** — a separate host or credential-gated store,
//! fetched by the harness after the freeze and never mounted where the agent can reach it.
//! That is an operational requirement recorded in `docs/evaluation.md` §3, not something
//! this module can enforce.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which arm an attempt belongs to.
///
/// The primary comparison is A3 against **A2**, never against A1. A2 is the control that
/// separates "better tooling helps" from "a new language helps", and without it every
/// improvement would be attributed to the language by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    /// TypeScript, baseline tooling.
    TypescriptBaseline,
    /// TypeScript plus structured diagnostics, canonical formatter, repeat-edit detection.
    TypescriptTooled,
    /// Nazm with tooling of comparable capability.
    Nazm,
}

impl Arm {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::TypescriptBaseline => "A1",
            Self::TypescriptTooled => "A2",
            Self::Nazm => "A3",
        }
    }

    /// The arm each comparison is made against. `None` for the baseline itself.
    ///
    /// Encoded rather than left to the analysis script because getting it wrong — comparing
    /// A3 to A1 — is precisely the error the control exists to prevent, and it would
    /// inflate the apparent language benefit by the entire tooling contribution.
    #[must_use]
    pub fn control(self) -> Option<Self> {
        match self {
            Self::TypescriptBaseline => None,
            Self::TypescriptTooled | Self::Nazm => Some(Self::TypescriptBaseline),
        }
    }

    /// The arm the **gate** compares against. Distinct from [`Arm::control`]: A3's gate
    /// comparison is A2, not A1.
    #[must_use]
    pub fn gate_comparator(self) -> Option<Self> {
        match self {
            Self::Nazm => Some(Self::TypescriptTooled),
            Self::TypescriptBaseline | Self::TypescriptTooled => None,
        }
    }

    #[must_use]
    pub fn all() -> [Self; 3] {
        [Self::TypescriptBaseline, Self::TypescriptTooled, Self::Nazm]
    }
}

impl fmt::Display for Arm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which pool a task belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSet {
    /// Opened freely: language design, diagnostics, wrappers, debugging.
    Development,
    /// Variance estimation, power analysis, protocol rehearsal.
    Calibration,
    /// Gate evaluation only, after the freeze. Never opened before.
    Holdout,
}

impl TaskSet {
    /// Whether tasks in this set may be read without a [`Freeze`].
    #[must_use]
    pub fn is_freely_readable(self) -> bool {
        matches!(self, Self::Development | Self::Calibration)
    }
}

/// Evidence that the implementation freeze has happened.
///
/// Carries the git tag and commit everything under evaluation was fixed at.
/// [`TaskSpec::open`] will not hand back a holdout fixture path without one, so opening
/// the holdout early through this API requires deliberately forging a freeze — visible in
/// a diff. See the module docs for what this does **not** protect against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freeze {
    tag: String,
    commit: String,
}

impl Freeze {
    /// Both fields are required and must be non-empty: a freeze that cannot name what it
    /// froze is not a freeze.
    pub fn new(tag: impl Into<String>, commit: impl Into<String>) -> Result<Self, FreezeError> {
        let tag = tag.into();
        let commit = commit.into();
        if tag.trim().is_empty() {
            return Err(FreezeError::MissingTag);
        }
        if commit.trim().is_empty() {
            return Err(FreezeError::MissingCommit);
        }
        Ok(Self { tag, commit })
    }

    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Check the freeze against the actual repository: the tag must exist and must resolve
    /// to the recorded commit.
    ///
    /// A `Freeze` built by hand claims a freeze happened. This is what turns the claim
    /// into something checkable, so that a run record naming a tag nobody ever created —
    /// or a tag that has since been moved — fails rather than being believed.
    ///
    /// It still does not make the holdout unreadable by other means; see the module docs.
    pub fn verify_against_repo(&self, repo: &std::path::Path) -> Result<(), FreezeError> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-list", "-n", "1"])
            .arg(&self.tag)
            .output()
            .map_err(|e| FreezeError::RepoUnreadable(e.to_string()))?;

        if !out.status.success() {
            return Err(FreezeError::UnknownTag {
                tag: self.tag.clone(),
            });
        }

        let resolved = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        // Accept an abbreviated commit, as long as it is a genuine prefix of the full one.
        if resolved.starts_with(&self.commit) {
            Ok(())
        } else {
            Err(FreezeError::TagMovedOrMismatched {
                tag: self.tag.clone(),
                recorded: self.commit.clone(),
                actual: resolved,
            })
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FreezeError {
    #[error("a freeze must name the git tag it froze")]
    MissingTag,
    #[error("a freeze must name the commit it froze")]
    MissingCommit,
    #[error("could not read the repository to verify the freeze: {0}")]
    RepoUnreadable(String),
    #[error("freeze names tag {tag}, which does not exist in the repository")]
    UnknownTag { tag: String },
    #[error(
        "freeze names tag {tag} at commit {recorded}, but the tag resolves to {actual}. \
         Either the tag moved or the record is wrong; neither may be evaluated."
    )]
    TagMovedOrMismatched {
        tag: String,
        recorded: String,
        actual: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AccessError {
    #[error(
        "task {id} is in the holdout set and cannot be opened before the implementation \
         freeze (docs/evaluation.md §3)"
    )]
    HoldoutBeforeFreeze { id: String },
}

/// One benchmark task.
///
/// The body is private and reachable only through [`TaskSpec::open`], so the holdout check
/// cannot be bypassed by reading a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    id: String,
    family: Family,
    set: TaskSet,
    /// Path to the task fixture, relative to the corpus root.
    fixture: String,
}

impl TaskSpec {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        family: Family,
        set: TaskSet,
        fixture: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            family,
            set,
            fixture: fixture.into(),
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn family(&self) -> Family {
        self.family
    }

    #[must_use]
    pub fn set(&self) -> TaskSet {
        self.set
    }

    /// Obtain the fixture path, subject to the holdout rule.
    ///
    /// `freeze` is `Option` rather than two separate methods so that the check happens on
    /// every access rather than at one call site a future refactor might route around.
    /// Note this returns a *path*, and holds no authority over that path: anything with
    /// filesystem access can read it directly. See the module docs.
    pub fn open(&self, freeze: Option<&Freeze>) -> Result<&str, AccessError> {
        if self.set.is_freely_readable() || freeze.is_some() {
            Ok(&self.fixture)
        } else {
            Err(AccessError::HoldoutBeforeFreeze {
                id: self.id.clone(),
            })
        }
    }
}

/// The task families from `docs/evaluation.md` §3. Each is instantiated into several
/// concrete tasks; the family is what per-category results are grouped by, and the gate's
/// "no category collapse" condition is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    ValidatedCrudApi,
    TransactionalReservation,
    TenantIsolatedApi,
    IdempotentWebhook,
    CancellableJob,
    /// Maintenance on an existing service. The category that predicts real use, and the
    /// one the gate requires not to regress.
    ExistingServiceChange,
    /// Secondary domains, sampled to test whether backend is the right first choice.
    EtlSample,
    CliSample,
}

impl Family {
    #[must_use]
    pub fn is_primary_domain(self) -> bool {
        !matches!(self, Self::EtlSample | Self::CliSample)
    }

    /// The gate treats this family specially: acceptance must not fall and regressions
    /// must not rise relative to A2.
    #[must_use]
    pub fn is_gate_critical(self) -> bool {
        matches!(self, Self::ExistingServiceChange)
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

    fn holdout_task() -> TaskSpec {
        TaskSpec::new(
            "tenant-iso-07",
            Family::TenantIsolatedApi,
            TaskSet::Holdout,
            "holdout/tenant-iso-07",
        )
    }

    #[test]
    fn holdout_is_unreadable_before_the_freeze() {
        let task = holdout_task();
        let err = task.open(None).unwrap_err();
        assert_eq!(
            err,
            AccessError::HoldoutBeforeFreeze {
                id: "tenant-iso-07".to_owned()
            }
        );
    }

    #[test]
    fn holdout_opens_once_frozen() {
        let freeze = Freeze::new("gate-v1", "abc1234").unwrap();
        assert_eq!(
            holdout_task().open(Some(&freeze)).unwrap(),
            "holdout/tenant-iso-07"
        );
    }

    #[test]
    fn development_and_calibration_need_no_freeze() {
        for set in [TaskSet::Development, TaskSet::Calibration] {
            let task = TaskSpec::new("crud-01", Family::ValidatedCrudApi, set, "dev/crud-01");
            assert!(task.open(None).is_ok());
        }
    }

    #[test]
    fn a_forged_freeze_fails_against_the_real_repository() {
        // `Freeze::new` accepts anything, which is why it is a workflow control and not a
        // boundary. Verification is what makes a fabricated one detectable.
        let forged = Freeze::new("gate-v1-that-never-existed", "deadbeef").unwrap();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        match forged.verify_against_repo(repo) {
            Err(FreezeError::UnknownTag { tag }) => {
                assert_eq!(tag, "gate-v1-that-never-existed");
            }
            // A machine without git, or a non-repo checkout, is not a test failure.
            Err(FreezeError::RepoUnreadable(_)) => {}
            other => panic!("a forged freeze must not verify: {other:?}"),
        }
    }

    #[test]
    fn opening_a_holdout_returns_only_a_path_not_protection() {
        // Documents the limit in an executable form: `open` hands back a path, and the
        // path is readable by anything with filesystem access. Real isolation for the gate
        // run is operational, not type-level.
        let freeze = Freeze::new("gate-v1", "abc1234").unwrap();
        let task = holdout_task();
        let path = task.open(Some(&freeze)).unwrap();
        assert_eq!(path, "holdout/tenant-iso-07");
    }

    #[test]
    fn a_freeze_must_name_what_it_froze() {
        assert_eq!(Freeze::new("", "abc").unwrap_err(), FreezeError::MissingTag);
        assert_eq!(
            Freeze::new("gate-v1", "  ").unwrap_err(),
            FreezeError::MissingCommit
        );
    }

    #[test]
    fn the_gate_compares_nazm_against_the_tooled_arm_not_the_baseline() {
        // Comparing A3 to A1 would credit the language with the entire tooling
        // contribution. This is the single most consequential wiring mistake available.
        assert_eq!(Arm::Nazm.gate_comparator(), Some(Arm::TypescriptTooled));
        assert_ne!(Arm::Nazm.gate_comparator(), Some(Arm::TypescriptBaseline));
    }

    #[test]
    fn only_nazm_has_a_gate_comparator() {
        assert!(Arm::TypescriptBaseline.gate_comparator().is_none());
        assert!(Arm::TypescriptTooled.gate_comparator().is_none());
    }

    #[test]
    fn maintenance_is_the_gate_critical_family() {
        let critical: Vec<_> = [
            Family::ValidatedCrudApi,
            Family::TransactionalReservation,
            Family::TenantIsolatedApi,
            Family::IdempotentWebhook,
            Family::CancellableJob,
            Family::ExistingServiceChange,
        ]
        .into_iter()
        .filter(|f| f.is_gate_critical())
        .collect();
        assert_eq!(critical, vec![Family::ExistingServiceChange]);
    }
}
