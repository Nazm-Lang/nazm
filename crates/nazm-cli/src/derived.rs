//! Where this compiler keeps what it can recompute.
//!
//! One root, `.nazm/` beside the source root, with one subdirectory per kind of derived
//! artefact:
//!
//! ```text
//! .nazm/check/    semantic check results     nazm.check/1
//! .nazm/native/   compiled objects           nazm.object/1
//! ```
//!
//! `NAZM_CACHE_DIR` moves **the root**, not one store: a read-only checkout, a container
//! with a scratch volume and a test that needs two projects to share put both kinds
//! somewhere else together. One variable rather than one per store, because a second
//! mechanism is a second thing to get wrong and there is nothing it could express that
//! this cannot.
//!
//! Everything under it is derived data. Deleting the whole directory changes no program's
//! meaning, and `crates/nazm-cli/tests/` proves it rather than asserting it.

use std::path::{Path, PathBuf};

use nazm_cache::{ObjectStore, Store};

/// The environment variable that moves the derived-data root somewhere else.
///
/// It changes where entries are kept and nothing about what they are: a location is not
/// part of any identity, and the same entry computed anywhere is the same bytes under the
/// same name.
pub(crate) const CACHE_DIR: &str = "NAZM_CACHE_DIR";

/// This compilation's derived-data root.
pub(crate) fn root(source_root: &Path) -> PathBuf {
    match std::env::var_os(CACHE_DIR) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => source_root.join(nazm_cache::store::DIRECTORY),
    }
}

/// Where this project's check results live.
pub(crate) fn checks(source_root: &Path) -> Store {
    Store::under(&root(source_root))
}

/// Where this project's objects live.
pub(crate) fn objects(source_root: &Path) -> ObjectStore {
    ObjectStore::under(&root(source_root))
}
