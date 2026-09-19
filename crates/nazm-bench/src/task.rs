//! Tasks, arms, and the three task sets.
//!
//! The holdout set is the part worth encoding in types rather than documenting. Hiding
//! acceptance tests from the agent is necessary but not sufficient: if the language, its
//! diagnostics, and its wrappers are tuned against the same tasks for a year, the final
//! benchmark rewards specialisation to that set. `docs/evaluation.md` §3 therefore splits
//! tasks three ways and gates the holdout behind an implementation freeze.
//!
//! A comment saying "don't open the holdout early" is not a control. [`Freeze`] is.

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
/// Carries the git tag everything under evaluation was fixed at. There is no way to
/// construct one without naming that tag, and [`TaskSpec::open`] will not hand back a
/// holdout task without it — so reading the holdout early requires deliberately forging a
/// freeze, which is a thing a reviewer can see in a diff.
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
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FreezeError {
    #[error("a freeze must name the git tag it froze")]
    MissingTag,
    #[error("a freeze must name the commit it froze")]
    MissingCommit,
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
