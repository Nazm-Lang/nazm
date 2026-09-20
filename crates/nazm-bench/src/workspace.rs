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

/// Evidence that nothing can still write to the tree about to be frozen.
///
/// The freeze's whole value is that what gets graded cannot change afterwards, and that
/// rests entirely on no candidate process being alive to change it. Writing that down as
/// a precondition does not make callers meet it — this does, by making the precondition
/// an argument [`Workspace::freeze`] cannot be called without.
///
/// The only way to obtain one from a real attempt is
/// [`crate::sandbox::execution::StopOutcome::writers_stopped`], and a `StopOutcome`
/// exists only when the container was killed *and* observed to have exited. A stop that
/// fails returns an error, yields no token, and therefore cannot be followed by a freeze.
#[derive(Debug, Clone)]
pub struct WritersStopped {
    why: String,
    /// The attempt this is evidence about, when it is evidence about one.
    ///
    /// A confirmed stop names its attempt, and [`Workspace::freeze`] refuses a token
    /// belonging to a different one: stopping attempt A's candidate says nothing about
    /// whether attempt B's is still writing, and with concurrency that is a live
    /// mistake rather than a theoretical one. `None` only for `none_running`, whose
    /// whole claim is that no candidate exists to bind to.
    attempt: Option<String>,
}

impl WritersStopped {
    /// From a confirmed stop. Crate-internal, so the only public route to one is the
    /// honest declaration below.
    pub(crate) fn confirmed(attempt: impl Into<String>, why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            attempt: Some(attempt.into()),
        }
    }

    /// For a caller with no candidate process at all — a unit test, a fixture, a tree
    /// this process built itself.
    ///
    /// Deliberately not a default. It takes a reason so the assumption is stated where
    /// it is made and can be read back, rather than being the silent behaviour of every
    /// caller that never thought about it.
    #[must_use]
    pub fn none_running(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            attempt: None,
        }
    }

    /// Why the caller believes nothing can write. Recorded, not interpreted.
    #[must_use]
    pub fn why(&self) -> &str {
        &self.why
    }

    /// The attempt this vouches for, if it vouches for a particular one.
    #[must_use]
    pub fn attempt(&self) -> Option<&str> {
        self.attempt.as_deref()
    }
}

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
    #[error(
        "refusing to freeze attempt {workspace} on evidence about attempt {evidence}. \
         Stopping one attempt's candidate says nothing about another's, and freezing a \
         tree that is still being written produces a snapshot of no state it was ever in."
    )]
    WrongAttempt { workspace: String, evidence: String },
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
    /// After this returns, further writes to the workspace — by a lingering agent
    /// process, an orphaned child, anything — cannot change what gets graded.
    ///
    /// That holds only if nothing is writing *during* the copy, which is what
    /// [`WritersStopped`] is for: the copy is not atomic, and a tree changing underneath
    /// it produces a snapshot that matches no state the workspace was ever in.
    pub fn freeze(
        &self,
        into: &Path,
        writers_stopped: &WritersStopped,
    ) -> Result<Submission, WorkspaceError> {
        // Evidence about another attempt is not evidence about this one. Stopping A's
        // candidate leaves B's running, and freezing B on A's token would snapshot a
        // tree still being written — the exact failure the token exists to prevent,
        // reached by passing the wrong one.
        if let Some(vouched) = writers_stopped.attempt() {
            if vouched != self.attempt_id {
                return Err(WorkspaceError::WrongAttempt {
                    workspace: self.attempt_id.clone(),
                    evidence: vouched.to_owned(),
                });
            }
        }
        fs::create_dir_all(into).map_err(WorkspaceError::Freeze)?;
        let mut files = BTreeMap::new();
        walk_tree(self.root.path(), self.root.path(), Some(into), &mut files)
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
        walk_tree(&self.path, &self.path, None, &mut files)
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
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What the manifest records for one entry.
///
/// A content hash alone is not identity. The same bytes with and without the executable
/// bit behave differently, and only one of them runs.
///
/// **Scope: file content and permission bits, and nothing else.** The mask is
/// `0o7777` — the permission bits plus setuid, setgid and sticky — recorded here and
/// applied to the copy, so what affects identity is exactly what the snapshot preserves.
/// Modification times, ownership, extended attributes and ACLs are neither preserved nor
/// recorded, and no claim is made about them.
///
/// Recorded as `file:<mode>:<hash>` on unix and `file:<hash>` elsewhere, where there are
/// no unix permission bits to preserve.
#[cfg(unix)]
fn entry_for(meta: &fs::Metadata, bytes: &[u8]) -> String {
    use std::os::unix::fs::PermissionsExt as _;
    format!(
        "file:{:04o}:{:016x}",
        meta.permissions().mode() & MODE_MASK,
        fnv1a(bytes)
    )
}

#[cfg(not(unix))]
fn entry_for(_meta: &fs::Metadata, bytes: &[u8]) -> String {
    format!("file:{:016x}", fnv1a(bytes))
}

/// The permission bits the snapshot preserves and the manifest records.
#[cfg(unix)]
const MODE_MASK: u32 = 0o7777;

/// The manifest key for a path, or an error naming it.
///
/// Non-UTF-8 names are refused rather than normalised. `to_string_lossy` would map two
/// distinct names onto one key — silently, and in a `BTreeMap` the second would overwrite
/// the first, so a submission could hide a file behind another file's name and the digest
/// would never know. A benchmark submission with a name we cannot represent exactly is a
/// submission we cannot grade honestly, so it is rejected and says so.
fn manifest_key(relative: &Path) -> std::io::Result<String> {
    relative.to_str().map(str::to_owned).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "the submission contains a path that is not valid UTF-8 ({}). It is \
                 refused rather than renamed: two such names can differ and still \
                 collapse onto one manifest entry, which would let one file hide behind \
                 another and leave the digest none the wiser.",
                relative.display()
            ),
        )
    })
}

/// Did the copy end up with the mode it was recorded with?
///
/// Split out so it can be tested at all. A filesystem that silently refuses permission
/// bits is not something a test can conjure — on ordinary local filesystems
/// `set_permissions` always takes — so the comparison is exercised directly instead. It
/// matters because a snapshot whose permissions differ from the manifest describing it
/// verifies as drifted, and the run is voided for a reason nobody can see.
#[cfg(unix)]
fn mode_matches(path: &Path, wanted: u32, got: u32) -> std::io::Result<()> {
    if got == wanted {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "{} was copied with mode {got:04o}, not the {wanted:04o} it was recorded with",
            path.display()
        ),
    ))
}

/// Read a file, cross-checking that the thing opened is the thing that was stat'd.
///
/// **This is a consistency check, not a defence.** It compares device and inode across
/// the open, which catches an accidental swap between the walk and the read — a stale
/// entry, a file replaced by a build still running. It does not make the read safe
/// against a hostile writer, and three cases say why: a replacement link can resolve to
/// the same inode and pass; a replacement FIFO blocks inside `open` before the check is
/// ever reached; and the same inode can have entirely different contents by the time it
/// is read.
///
/// What the freeze actually rests on is the precondition enforced by [`WritersStopped`]:
/// nothing is running that could write here. Defending the read itself against a live
/// adversary needs `openat` with `O_NOFOLLOW` at every component, which the standard
/// library does not offer.
fn read_consistently(path: &Path, before: &fs::Metadata) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut file = fs::File::open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let after = file.metadata()?;
        if after.dev() != before.dev() || after.ino() != before.ino() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{} changed identity between being listed and being read. The \
                     snapshot would not be the tree that was walked.",
                    path.display()
                ),
            ));
        }
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Walk `dir`, recording every entry, and copy it when `into` is `Some`.
///
/// One walker for both the freeze and the re-digest. They were two, and two
/// implementations of one manifest format is how the format drifts: a difference between
/// them reads as a changed submission, which voids a result for no reason.
fn walk_tree(
    base: &Path,
    dir: &Path,
    into: Option<&Path>,
    files: &mut BTreeMap<String, String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;

        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };
        let key = manifest_key(relative)?;

        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "the submission contains a symlink ({} -> {}), which is not \
                     supported. Containing one is harder than it looks and a rule that \
                     is nearly right is worse than a refusal.",
                    relative.display(),
                    target.display()
                ),
            ));
        }

        if meta.is_dir() {
            if let Some(into) = into {
                fs::create_dir_all(into.join(relative))?;
            }
            walk_tree(base, &path, into, files)?;
            continue;
        }

        let bytes = read_consistently(&path, &meta)?;
        files.insert(key, entry_for(&meta, &bytes));
        if let Some(into) = into {
            let dest = into.join(relative);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, &bytes)?;
            // Carry the mode across, so the copy the grader runs is the file the
            // candidate produced. `fs::write` creates with the process umask, which
            // would silently strip the executable bit off every script.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let wanted = meta.permissions().mode() & MODE_MASK;
                fs::set_permissions(&dest, fs::Permissions::from_mode(wanted))?;
                // Read back: a filesystem may refuse bits silently, and a snapshot whose
                // permissions differ from the manifest describing it would verify as
                // drifted for a reason nobody could see.
                let got = fs::metadata(&dest)?.permissions().mode() & MODE_MASK;
                mode_matches(relative, wanted, got)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::manifest_key;
    use std::path::Path;

    #[test]
    #[cfg(unix)]
    fn a_copy_whose_mode_did_not_take_is_refused() {
        use super::mode_matches;

        mode_matches(Path::new("run.sh"), 0o755, 0o755).expect("a match is fine");

        let err = mode_matches(Path::new("run.sh"), 0o755, 0o644)
            .expect_err("a mode that did not take must be refused");
        let text = err.to_string();
        assert!(text.contains("0644") && text.contains("0755"), "{text}");
        assert!(text.contains("run.sh"), "{text}");
    }

    #[test]
    #[cfg(unix)]
    fn the_mask_covers_setuid_and_sticky_not_just_permissions() {
        use super::{mode_matches, MODE_MASK};

        assert_eq!(MODE_MASK, 0o7777);
        // A setuid bit that did not survive the copy is a difference the grader would
        // feel, so it has to be one the comparison sees.
        assert!(mode_matches(Path::new("x"), 0o4755, 0o755).is_err());
    }

    #[test]
    fn a_non_utf8_name_is_refused_with_its_path() {
        // Tested here rather than only end to end, because not every filesystem will
        // create such a name: APFS rejects the bytes outright, while ext4 accepts them.
        // A guard that is only exercised where the fixture happens to be constructible
        // is a guard that is untested exactly where it matters.
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt as _;

            let bad = Path::new(OsStr::from_bytes(b"bad\xff.txt"));
            let err = manifest_key(bad).expect_err("must be refused");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("not valid UTF-8"), "{err}");
        }
        assert_eq!(manifest_key(Path::new("fine.txt")).unwrap(), "fine.txt");
    }

    #[test]
    fn two_distinct_non_utf8_names_would_have_collided() {
        // The reason refusal is the policy rather than normalisation. Both of these
        // render as the same lossy string, so as manifest keys the second would
        // overwrite the first and one file would vanish behind another's name.
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt as _;

            let a = Path::new(OsStr::from_bytes(b"bad\xff.txt"));
            let b = Path::new(OsStr::from_bytes(b"bad\xfe.txt"));
            assert_ne!(a, b, "the names differ");
            assert_eq!(
                a.to_string_lossy(),
                b.to_string_lossy(),
                "and lossily they do not, which is the collision"
            );
            assert!(manifest_key(a).is_err() && manifest_key(b).is_err());
        }
    }
}
