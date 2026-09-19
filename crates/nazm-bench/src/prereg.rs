//! Pre-registration records, bound to chronology rather than to consistency.
//!
//! An earlier version of the `xtask` gate checked only that the documentation and the
//! pre-registration file agreed with each other. That is satisfiable *after* looking at
//! results: examine the numbers, then write both files in one commit, and the gate passes.
//! Consistency is not chronology.
//!
//! Git ancestry is the first half of the answer, and an earlier version of this module
//! overstated it. Requiring the registration commit to be a strict ancestor of the run
//! commit establishes **ordering within repository history** — it does not establish that
//! the registration preceded the *experiment*. Someone can run the experiment, then create
//! the registration commit and the run commit in the required order. History is authored,
//! not observed.
//!
//! The second half is [`Preflight`]: before an agent is launched, the runner verifies the
//! registration and the freeze, captures what is being evaluated, refuses to proceed from
//! a dirty tree, and writes a start record. That record is what ties the registration to a
//! specific execution rather than to a commit graph.
//!
//! Neither half is an independent timestamp. Publication-grade chronology needs an
//! externally recorded registration event — a push whose time the forge records, or a
//! timestamping service — and this module does not provide one. It must not be described
//! as if it did.
//!
//! What this binds, per `docs/evaluation.md` §6:
//!
//! * the protocol and analysis rules (the content of the registration commit)
//! * the evaluated implementation and configuration (the freeze tag)
//! * task-set identifiers (the corpus digest)
//! * the ordering: registration before evaluation
//!
//! A later protocol change creates a **new** registration. Earlier runs keep pointing at
//! the one they were run under, and stay interpretable under the rules that applied to
//! them.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// An immutable reference to a pre-registration, recorded on every run.
// No `Eq`: `max_void_rate` is an f64. `PartialEq` is what comparisons here need, and
// deriving `Eq` on a float would be a lie about the type rather than a convenience.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreRegistration {
    /// Commit containing the registered protocol and analysis rules.
    pub commit: String,
    /// Version, so a superseding registration does not silently reinterpret old runs.
    pub version: u32,
    /// Digest of the task corpus the rules were registered against.
    pub corpus_digest: String,
    /// The implementation freeze tag this registration expects to be evaluated at.
    pub freeze_tag: String,
    /// The void rate above which an arm's results are not usable.
    ///
    /// Evaluator faults stay outside the *acceptance-rate* denominator under the
    /// registered policy, but that licence has a limit: past some rate the comparison is
    /// no longer between two arms, it is between one arm and a partly-broken measurement
    /// of another. Registered in advance so the threshold is not chosen after seeing which
    /// arm breached it.
    pub max_void_rate: f64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChronologyError {
    #[error("could not read the repository: {0}")]
    RepoUnreadable(String),
    #[error("pre-registration commit {commit} does not exist in this repository")]
    UnknownCommit { commit: String },
    #[error(
        "run commit {run} is not a descendant of pre-registration {prereg}. The rules were \
         not registered before this evaluation, so the run cannot be reported under them."
    )]
    RegistrationNotAncestor { prereg: String, run: String },
    #[error(
        "run commit {run} is the pre-registration commit itself. The registration must \
         precede the evaluation, not accompany it."
    )]
    RegistrationIsRunCommit { run: String },
}

impl PreRegistration {
    /// Whether an arm's void rate is within the registered tolerance.
    ///
    /// `None` when there were no attempts to rate. Registered in advance so the threshold
    /// is not chosen after seeing which arm breached it.
    #[must_use]
    pub fn void_rate_acceptable(&self, observed: Option<f64>) -> Option<bool> {
        observed.map(|r| r <= self.max_void_rate)
    }

    /// Verify that this registration genuinely precedes `run_commit`.
    ///
    /// Uses `git merge-base --is-ancestor`, which answers exactly the question that
    /// matters and cannot be satisfied by editing files.
    pub fn verify_precedes(&self, repo: &Path, run_commit: &str) -> Result<(), ChronologyError> {
        if run_commit == self.commit {
            return Err(ChronologyError::RegistrationIsRunCommit {
                run: run_commit.to_owned(),
            });
        }

        let exists = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["cat-file", "-e"])
            .arg(format!("{}^{{commit}}", self.commit))
            .status()
            .map_err(|e| ChronologyError::RepoUnreadable(e.to_string()))?;

        if !exists.success() {
            return Err(ChronologyError::UnknownCommit {
                commit: self.commit.clone(),
            });
        }

        let is_ancestor = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["merge-base", "--is-ancestor", &self.commit, run_commit])
            .status()
            .map_err(|e| ChronologyError::RepoUnreadable(e.to_string()))?;

        if is_ancestor.success() {
            Ok(())
        } else {
            Err(ChronologyError::RegistrationNotAncestor {
                prereg: self.commit.clone(),
                run: run_commit.to_owned(),
            })
        }
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

    fn prereg(commit: &str) -> PreRegistration {
        PreRegistration {
            commit: commit.to_owned(),
            version: 1,
            corpus_digest: "sha256:deadbeef".to_owned(),
            freeze_tag: "gate-v1".to_owned(),
            max_void_rate: 0.05,
        }
    }

    #[test]
    fn a_registration_cannot_be_its_own_run_commit() {
        // The "write both files together after seeing results" defect, in its most direct
        // form: registering and evaluating in one commit establishes no ordering.
        let p = prereg("abc123");
        let err = p.verify_precedes(Path::new("."), "abc123").unwrap_err();
        assert_eq!(
            err,
            ChronologyError::RegistrationIsRunCommit {
                run: "abc123".to_owned()
            }
        );
    }

    #[test]
    fn a_fabricated_registration_commit_does_not_verify() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let p = prereg("0000000000000000000000000000000000000000");
        match p.verify_precedes(repo, "HEAD") {
            Err(ChronologyError::UnknownCommit { .. } | ChronologyError::RepoUnreadable(_)) => {}
            other => panic!("a nonexistent registration must not verify: {other:?}"),
        }
    }

    fn preflight(prereg_for: &PreRegistration) -> Preflight {
        Preflight {
            preregistration: prereg_for.clone(),
            run_commit: "ffffff".to_owned(),
            freeze_tag: prereg_for.freeze_tag.clone(),
            corpus_digest: prereg_for.corpus_digest.clone(),
            configuration: "model=x;effort=high;turns=40".to_owned(),
            dependency_digest: "sha256:lock".to_owned(),
            uncommitted: None,
            started_at: "2026-09-19T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_dirty_tree_blocks_a_run_by_default() {
        let p = prereg("aaa");
        let mut pf = preflight(&p);
        pf.uncommitted = Some("M src/lib.rs".to_owned());
        assert!(matches!(
            pf.validate(false),
            Err(PreflightError::DirtyTree { .. })
        ));
        // Allowed explicitly, it proceeds but can never be reported.
        assert!(pf.validate(true).is_ok());
        assert!(!pf.is_publishable());
    }

    #[test]
    fn a_corpus_the_rules_were_not_registered_against_is_refused() {
        let p = prereg("aaa");
        let mut pf = preflight(&p);
        pf.corpus_digest = "sha256:some-other-tasks".to_owned();
        assert!(matches!(
            pf.validate(false),
            Err(PreflightError::CorpusMismatch { .. })
        ));
    }

    #[test]
    fn a_run_at_the_wrong_freeze_is_refused() {
        let p = prereg("aaa");
        let mut pf = preflight(&p);
        pf.freeze_tag = "gate-v2".to_owned();
        assert!(matches!(
            pf.validate(false),
            Err(PreflightError::FreezeMismatch { .. })
        ));
    }

    #[test]
    fn a_clean_matching_preflight_is_publishable() {
        let p = prereg("aaa");
        let pf = preflight(&p);
        assert!(pf.validate(false).is_ok());
        assert!(pf.is_publishable());
    }

    #[test]
    fn the_void_rate_tolerance_is_registered_in_advance() {
        // Choosing the threshold after seeing which arm breached it is the thing this
        // field exists to prevent.
        let p = prereg("aaa");
        assert_eq!(p.void_rate_acceptable(Some(0.04)), Some(true));
        assert_eq!(p.void_rate_acceptable(Some(0.20)), Some(false));
        assert_eq!(p.void_rate_acceptable(None), None);
    }

    #[test]
    fn a_registration_survives_being_superseded() {
        // Two registrations differ by version, so a run reported under v1 is not silently
        // reinterpreted when v2 changes the rules.
        let v1 = prereg("aaa");
        let mut v2 = prereg("bbb");
        v2.version = 2;
        assert_ne!(v1, v2);
        assert_ne!(v1.version, v2.version);
    }
}

/// What the runner captures before launching an agent.
///
/// Ancestry proves ordering in the commit graph; this proves what was actually executed.
/// Together they are much harder to fabricate after the fact than either alone — but see
/// the module docs: neither is an independent timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preflight {
    pub preregistration: PreRegistration,
    /// Commit of the implementation under evaluation, resolved at launch.
    pub run_commit: String,
    /// The freeze tag, verified to resolve to `run_commit`.
    pub freeze_tag: String,
    /// Digest of the task bundle actually loaded, not the one the registration names —
    /// so a mismatch is visible rather than assumed away.
    pub corpus_digest: String,
    /// Model versions, reasoning settings, caps. The pre-registration fixes these; the
    /// record captures what was really used.
    pub configuration: String,
    /// Lockfile digest. "Same configuration" is not the same run if a dependency moved.
    pub dependency_digest: String,
    /// Uncommitted changes at launch. Never silently ignored: either the run is refused,
    /// or the diff is captured here and the run is marked unpublishable.
    pub uncommitted: Option<String>,
    /// When the runner started, by the runner's clock. Self-reported, and therefore
    /// evidence of sequence within a run, not proof of chronology against the world.
    pub started_at: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PreflightError {
    #[error(
        "the working tree has uncommitted changes, so what would be evaluated is not any \
         commit. Commit them, stash them, or pass the explicit allow-dirty flag, which \
         marks the run unpublishable."
    )]
    DirtyTree { diff: String },
    #[error(
        "the task bundle loaded ({loaded}) is not the one the pre-registration names \
         ({registered}). The rules were registered against different tasks."
    )]
    CorpusMismatch { registered: String, loaded: String },
    #[error("the pre-registration expects freeze tag {expected}, but the run is at {actual}")]
    FreezeMismatch { expected: String, actual: String },
}

impl Preflight {
    /// Check the run against what was registered, before any agent is launched.
    ///
    /// `allow_dirty` exists because refusing outright would push people to commit noise
    /// mid-experiment; it records the diff and the caller is required to mark the run
    /// unpublishable.
    pub fn validate(&self, allow_dirty: bool) -> Result<(), PreflightError> {
        if let Some(diff) = &self.uncommitted {
            if !allow_dirty {
                return Err(PreflightError::DirtyTree { diff: diff.clone() });
            }
        }
        if self.corpus_digest != self.preregistration.corpus_digest {
            return Err(PreflightError::CorpusMismatch {
                registered: self.preregistration.corpus_digest.clone(),
                loaded: self.corpus_digest.clone(),
            });
        }
        if self.freeze_tag != self.preregistration.freeze_tag {
            return Err(PreflightError::FreezeMismatch {
                expected: self.preregistration.freeze_tag.clone(),
                actual: self.freeze_tag.clone(),
            });
        }
        Ok(())
    }

    /// Whether results from this run may be published.
    ///
    /// A dirty tree does not invalidate a debugging run; it does mean the run is not
    /// attributable to a commit, so it cannot be reported.
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.uncommitted.is_none()
    }
}
