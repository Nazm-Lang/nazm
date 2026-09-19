//! Per-attempt workspace management, and the submission freeze.
//!
//! # What this is not
//!
//! **This is not an access-control boundary, and the module was originally misdescribed as
//! one.** Giving each attempt its own directory and validating paths inside the harness
//! constrains *the harness's own* file operations. It does nothing to a process the agent
//! spawns: `cat /tmp/nazm-attempt-xyz/secret.txt` works, and so does reading the
//! acceptance fixtures or the harness binary itself.
//!
//! Real isolation needs enforcement the candidate process cannot decline — an OS sandbox
//! (`sandbox-exec` on macOS, namespaces plus seccomp on Linux), or a container per
//! attempt, with the acceptance corpus outside the mount namespace entirely. **None of
//! that is implemented.** Until it is, the tests in `tests/execution_boundary.rs` are
//! workspace-management tests, and this module's guarantee is "the harness will not
//! itself leak state between attempts", not "attempts are isolated".
//!
//! For backend tasks the same gap applies beyond the filesystem: each attempt also needs
//! its own database state and scoped credentials, or attempts share a Postgres instance
//! and one attempt's rows are another's starting conditions.
//!
//! # What it does hold
//!
//! 1. **The harness will not itself carry state between attempts.** Each attempt gets a
//!    fresh tree, removed when the attempt ends, and harness-side path resolution refuses
//!    absolute paths, `..`, and symlinks resolving outside the root.
//! 2. **Acceptance evaluates a frozen submission, not a live tree.** An agent whose
//!    process outlived its timeout — or whose children did — must not be able to keep
//!    editing what is being graded. The freeze takes a snapshot, records its digest, and
//!    grading happens against the snapshot in a directory the agent never had.
//!
//! The digest is the link between the two. A record naming digest X and an evaluation of
//! digest Y is not a result; it is a bug, and it is detectable.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// An isolated tree for one attempt.
///
/// Dropping it removes the tree, so a run cannot slowly accumulate the state of every
/// attempt that came before it.
#[derive(Debug)]
pub struct Workspace {
    root: tempfile::TempDir,
    attempt_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("could not create the attempt workspace: {0}")]
    Create(std::io::Error),
    #[error("could not stage {path}: {source}")]
    Stage {
        path: String,
        source: std::io::Error,
    },
    #[error(
        "path {path} escapes the workspace. A task fixture that can write outside its \
         attempt tree can reach another attempt, or the acceptance tests."
    )]
    Escapes { path: String },
    #[error("could not freeze the submission: {0}")]
    Freeze(std::io::Error),
}

impl Workspace {
    /// Create an empty isolated workspace for one attempt.
    pub fn create(attempt_id: impl Into<String>) -> Result<Self, WorkspaceError> {
        let root = tempfile::Builder::new()
            .prefix("nazm-attempt-")
            .tempdir()
            .map_err(WorkspaceError::Create)?;
        Ok(Self {
            root,
            attempt_id: attempt_id.into(),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    #[must_use]
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    /// Resolve a path inside the workspace, refusing anything that escapes it.
    ///
    /// Checked lexically *and* against the canonical root, because `..` is only the
    /// obvious case — a symlink planted by a previous step is the interesting one.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let candidate = self.root.path().join(relative);

        if relative.starts_with('/') || relative.split('/').any(|c| c == "..") {
            return Err(WorkspaceError::Escapes {
                path: relative.to_owned(),
            });
        }

        // If the path exists, canonicalise and confirm containment; symlinks resolve here.
        if candidate.exists() {
            let real = candidate
                .canonicalize()
                .map_err(|e| WorkspaceError::Stage {
                    path: relative.to_owned(),
                    source: e,
                })?;
            let root_real = self
                .root
                .path()
                .canonicalize()
                .map_err(|e| WorkspaceError::Stage {
                    path: relative.to_owned(),
                    source: e,
                })?;
            if !real.starts_with(&root_real) {
                return Err(WorkspaceError::Escapes {
                    path: relative.to_owned(),
                });
            }
        }

        Ok(candidate)
    }

    /// Write a file into the workspace, creating parent directories.
    pub fn write(&self, relative: &str, contents: &str) -> Result<(), WorkspaceError> {
        let path = self.resolve(relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| WorkspaceError::Stage {
                path: relative.to_owned(),
                source: e,
            })?;
        }
        fs::write(&path, contents).map_err(|e| WorkspaceError::Stage {
            path: relative.to_owned(),
            source: e,
        })
    }

    pub fn read(&self, relative: &str) -> Result<String, WorkspaceError> {
        let path = self.resolve(relative)?;
        fs::read_to_string(&path).map_err(|e| WorkspaceError::Stage {
            path: relative.to_owned(),
            source: e,
        })
    }

    /// Snapshot the submission into a directory outside this workspace.
    ///
    /// After this returns, further writes to the workspace — by a lingering agent process,
    /// an orphaned child, anything — cannot change what gets graded.
    pub fn freeze(&self, into: &Path) -> Result<Submission, WorkspaceError> {
        fs::create_dir_all(into).map_err(WorkspaceError::Freeze)?;
        let mut files = BTreeMap::new();
        copy_tree(self.root.path(), self.root.path(), into, &mut files)
            .map_err(WorkspaceError::Freeze)?;

        Ok(Submission {
            attempt_id: self.attempt_id.clone(),
            path: into.to_path_buf(),
            digest: digest_of(&files),
            files,
        })
    }
}

/// A frozen submission: the exact bytes that will be graded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub attempt_id: String,
    pub path: PathBuf,
    /// Digest over every path and its content hash, in sorted order — so the same tree
    /// always digests the same way regardless of how it was walked.
    pub digest: String,
    /// Per-file content hashes, kept so a mismatch can say *which* file moved.
    pub files: BTreeMap<String, String>,
}

impl Submission {
    /// Re-digest the frozen tree and compare. Detects a snapshot modified after freezing,
    /// which would mean the record and the graded artifact had drifted apart.
    pub fn verify_unchanged(&self) -> Result<(), SubmissionDrift> {
        let mut files = BTreeMap::new();
        copy_tree_digest_only(&self.path, &self.path, &mut files)
            .map_err(|e| SubmissionDrift::Unreadable(e.to_string()))?;
        let now = digest_of(&files);
        if now == self.digest {
            Ok(())
        } else {
            Err(SubmissionDrift::Changed {
                recorded: self.digest.clone(),
                actual: now,
            })
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubmissionDrift {
    #[error("the frozen submission could not be read: {0}")]
    Unreadable(String),
    #[error(
        "the frozen submission changed after it was recorded (recorded {recorded}, now \
         {actual}). The evaluated artifact is not the recorded one, so the result is void."
    )]
    Changed { recorded: String, actual: String },
}

/// FNV-1a over the sorted (path, content-hash) pairs.
///
/// Not cryptographic and not claimed to be: this detects accidental drift between the
/// record and the graded tree, not a determined forger. A real corpus digest for the
/// pre-registration should use BLAKE3.
fn digest_of(files: &BTreeMap<String, String>) -> String {
    let mut buf = String::new();
    for (path, hash) in files {
        let _ = writeln!(buf, "{path}:{hash}");
    }
    format!("fnv1a:{:016x}", fnv1a(buf.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn copy_tree(
    base: &Path,
    dir: &Path,
    into: &Path,
    files: &mut BTreeMap<String, String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;

        // Symlinks are not copied. A link pointing outside the workspace would otherwise
        // drag whatever it names into the graded submission.
        if meta.file_type().is_symlink() {
            continue;
        }

        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };
        let relative_str = relative.to_string_lossy().into_owned();

        if meta.is_dir() {
            fs::create_dir_all(into.join(relative))?;
            copy_tree(base, &path, into, files)?;
        } else {
            let bytes = fs::read(&path)?;
            files.insert(relative_str, format!("{:016x}", fnv1a(&bytes)));
            let dest = into.join(relative);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(dest, &bytes)?;
        }
    }
    Ok(())
}

fn copy_tree_digest_only(
    base: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };
        if meta.is_dir() {
            copy_tree_digest_only(base, &path, files)?;
        } else {
            let bytes = fs::read(&path)?;
            files.insert(
                relative.to_string_lossy().into_owned(),
                format!("{:016x}", fnv1a(&bytes)),
            );
        }
    }
    Ok(())
}
