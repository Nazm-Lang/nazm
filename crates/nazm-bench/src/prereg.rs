//! Pre-registration records, bound to chronology rather than to consistency.
//!
//! An earlier version of the `xtask` gate checked only that the documentation and the
//! pre-registration file agreed with each other. That is satisfiable *after* looking at
//! results: examine the numbers, then write both files in one commit, and the gate passes.
//! Consistency is not chronology.
//!
//! What makes a registration credible is that the registration event provably preceded the
//! evaluation. Git gives that for free, if it is actually checked: the registration is a
//! commit, every run record names it, and the run's own commit must be a **descendant** of
//! it. A registration written afterwards cannot be an ancestor of a run that already
//! happened, so the ordering is not a matter of trust.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreRegistration {
    /// Commit containing the registered protocol and analysis rules.
    pub commit: String,
    /// Version, so a superseding registration does not silently reinterpret old runs.
    pub version: u32,
    /// Digest of the task corpus the rules were registered against.
    pub corpus_digest: String,
    /// The implementation freeze tag this registration expects to be evaluated at.
    pub freeze_tag: String,
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
