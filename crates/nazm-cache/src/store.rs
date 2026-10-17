//! Where check results are kept, and the rules that keep a damaged one from being
//! believed.
//!
//! # Local, and nothing more
//!
//! One directory of files. No server, no daemon, no registry, no network and no index —
//! an index would be a second thing that can disagree with the entries, and the entries
//! are addressed by their own key, so there is nothing for it to do.
//!
//! # Why entries are addressed by their key
//!
//! A [`CheckKey`](crate::CheckKey) covers every input capable of affecting the result, so
//! two entries with one key describe one answer. That makes an entry **immutable**: it is
//! written once and never edited, which is what lets two compilers run at the same time
//! with no lock. Both compute, both write, one rename lands second, and the bytes are the
//! same either way.
//!
//! # Publication is atomic or it did not happen
//!
//! A file is written under a temporary name, flushed, and then `rename`d into place.
//! `rename` within one directory is atomic on every platform this runs on: a reader sees
//! the previous entry or the new one, never half of one. A compiler killed mid-write
//! leaves a `.part` file, which is not an entry — nothing looks for one, and it is
//! removed on the next failure of the process that made it. Nothing sweeps, and nothing
//! outside this directory is ever touched.
//!
//! # A miss is always allowed
//!
//! Every failure in this file is a miss. A missing directory, an unreadable file, a full
//! disk, a corrupted entry, a wrong entry: the module is checked from its source, which
//! is the authority. A cache that could turn a damaged file into a compilation error
//! would be a cache whose absence changed a program's meaning.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::artifact::{CheckArtifact, Invalid, Reusable};
use crate::key::CheckKey;

/// The derived-data directory a project keeps beside itself.
///
/// It holds one subdirectory per kind of derived artefact — [`CHECK`] and
/// [`objects::DIRECTORY`](crate::objects::DIRECTORY) — because a semantic result and a
/// native object answer different questions and an entry of one must never be reachable
/// by a lookup of the other.
pub const DIRECTORY: &str = ".nazm";

/// The subdirectory holding semantic check results.
pub const CHECK: &str = "check";

/// What a lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// A valid entry for exactly this key.
    Hit(Box<Reusable>),
    /// Nothing is filed under it.
    Miss,
    /// Something is, and it may not be used. Reported rather than silently treated as
    /// absent, so that a store going wrong is visible to whoever asks.
    Rejected(Invalid),
}

/// A local directory of check results.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The store at `dir`. Nothing is created until something is written.
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where a project's store belongs: `.nazm/check` under its source root.
    ///
    /// # Why beside the project rather than under the user's home
    ///
    /// It is derived data about **this tree**, so it belongs with the tree: deleting the
    /// checkout deletes it, a second checkout does not silently inherit it, no home
    /// directory is written to by a compiler, and a container or a CI job that mounts the
    /// source has the cache without being told where it went. A shared per-user store
    /// would be *sound* — the key covers the project, so two projects cannot collide by
    /// accident — and it would trade all of that for hits between unrelated trees.
    ///
    /// The location is not part of any identity. The same entry computed anywhere is the
    /// same bytes under the same name; a directory holds entries, it does not name them.
    #[must_use]
    pub fn for_project(source_root: &Path) -> Self {
        Self::under(&source_root.join(DIRECTORY))
    }

    /// The store under an explicitly chosen derived-data root.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(CHECK))
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// Where the entry for `key` lives.
    ///
    /// One level of fan-out on the first byte, so a project with thousands of modules
    /// does not put thousands of files in one directory.
    fn path(&self, key: &CheckKey) -> PathBuf {
        let name = key.to_string();
        let prefix = name.get(..2).unwrap_or("00").to_owned();
        self.dir.join(prefix).join(format!("{name}.json"))
    }

    /// Look for a usable result.
    #[must_use]
    pub fn get(&self, key: &CheckKey) -> Found {
        let Ok(text) = std::fs::read_to_string(self.path(key)) else {
            return Found::Miss;
        };
        match CheckArtifact::read(&text, key) {
            Ok(r) => Found::Hit(Box::new(r)),
            Err(e) => Found::Rejected(e),
        }
    }

    /// As [`Self::get`], for a checker that will only reuse an entry offering `offered`, whose
    /// canonical bytes and fingerprint it has: [`CheckArtifact::read_expecting`].
    #[must_use]
    pub fn get_expecting(
        &self,
        key: &CheckKey,
        offered: &nazm_iface::PersistentUnitInterface,
        offered_json: &str,
        fingerprint: nazm_iface::InterfaceHash,
    ) -> Found {
        let Ok(text) = std::fs::read_to_string(self.path(key)) else {
            return Found::Miss;
        };
        match CheckArtifact::read_expecting(&text, key, offered, offered_json, fingerprint) {
            Ok(r) => Found::Hit(Box::new(r)),
            Err(e) => Found::Rejected(e),
        }
    }

    /// Publish a result, atomically.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refused. A caller treats it as "not stored" and carries
    /// on: failing to write a cache entry is not a reason to fail a compilation.
    pub fn put(&self, artifact: &CheckArtifact) -> Result<(), std::io::Error> {
        let key = CheckKey::parse(&artifact.key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "an artefact must carry the key it is filed under",
            )
        })?;
        let final_path = self.path(&key);
        let Some(parent) = final_path.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a store needs a directory",
            ));
        };
        std::fs::create_dir_all(parent)?;

        let bytes = artifact
            .to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Unique to this process and this call, so two compilers writing the same entry
        // at the same time cannot write to one temporary file. `.part` and not `.json`,
        // so a reader looking for an entry cannot find one that is still being written.
        let temp = parent.join(format!(
            "{}.{}.{}.part",
            artifact.key,
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));

        let write = (|| -> Result<(), std::io::Error> {
            let mut f = std::fs::File::create(&temp)?;
            f.write_all(bytes.as_bytes())?;
            // Flushed before the rename, so what lands is a complete entry rather than a
            // name pointing at bytes the kernel has not written yet.
            f.sync_all()?;
            drop(f);
            std::fs::rename(&temp, &final_path)
        })();

        if write.is_err() {
            // Only this call's own temporary file, and only on the path where it may
            // still be there. Nothing else in the directory is this function's business.
            let _ = std::fs::remove_file(&temp);
        }
        write
    }
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
