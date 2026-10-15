//! Where this compiler keeps what it can recompute.
//!
//! One root, `.nazm/` beside the source root, with one subdirectory per kind of derived
//! artefact:
//!
//! ```text
//! .nazm/check/    semantic check results     nazm.check/2
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
//!
//! # The user cache (R1.5, D12)
//!
//! The runtime's and the standard library's objects are the same for every project, so they are
//! kept once per user rather than once per `.nazm/` ([`user_objects`], `docs/architecture.md`
//! §7.113). Same format, same keys, same rules: derived data, and deleting it changes nothing.

use std::ffi::OsString;
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

/// The environment variable that names the user cache on its own (R1.5, D12): empty turns it off,
/// and its units go back to the project store.
pub(crate) const USER_CACHE_DIR: &str = "NAZM_USER_CACHE_DIR";

/// The user cache's root, or `None` when there is none.
pub(crate) fn user_root() -> Option<PathBuf> {
    user_root_from(&|name| std::env::var_os(name), std::env::consts::OS)
}

/// [`user_root`], from these variables on this operating system: `NAZM_USER_CACHE_DIR`, then
/// `NAZM_CACHE_DIR` (one variable still moves everything), then the platform's own cache directory.
fn user_root_from(var: &dyn Fn(&str) -> Option<OsString>, os: &str) -> Option<PathBuf> {
    if let Some(dir) = var(USER_CACHE_DIR) {
        return (!dir.is_empty()).then(|| PathBuf::from(dir));
    }
    if let Some(dir) = var(CACHE_DIR).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    // Windows (W1): the account's local application data, as Windows' own caches are.
    if os == "windows" {
        return var("LOCALAPPDATA")
            .filter(|d| !d.is_empty())
            .map(|d| PathBuf::from(d).join("nazm"));
    }
    let home = var("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    if os == "macos" {
        return home.map(|h| h.join("Library").join("Caches").join("nazm"));
    }
    if let Some(xdg) = var("XDG_CACHE_HOME").map(PathBuf::from) {
        if xdg.is_absolute() {
            return Some(xdg.join("nazm"));
        }
    }
    home.map(|h| h.join(".cache").join("nazm"))
}

/// Where the runtime's and the standard library's objects live, every project's alike.
pub(crate) fn user_objects() -> Option<ObjectStore> {
    user_root().map(|root| ObjectStore::under(&root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn the_user_cache_is_the_platform_s_unless_a_variable_names_another() {
        let home = [("HOME", "/h")];
        assert_eq!(
            user_root_from(&with(&home), "macos"),
            Some(PathBuf::from("/h/Library/Caches/nazm"))
        );
        assert_eq!(
            user_root_from(&with(&home), "linux"),
            Some(PathBuf::from("/h/.cache/nazm"))
        );
        // Absolute on the host the test runs on: a drive's on Windows.
        let x = if cfg!(windows) { r"C:\x" } else { "/x" };
        let xdg = [("HOME", "/h"), ("XDG_CACHE_HOME", x)];
        assert_eq!(
            user_root_from(&with(&xdg), "linux"),
            Some(PathBuf::from(x).join("nazm"))
        );
        let windows = [("HOME", "/h"), ("LOCALAPPDATA", r"C:\a")];
        assert_eq!(
            user_root_from(&with(&windows), "windows"),
            Some(PathBuf::from(r"C:\a").join("nazm"))
        );
        assert_eq!(user_root_from(&with(&home), "windows"), None);
        // A relative XDG_CACHE_HOME is invalid by its specification, and ignored.
        let relative = [("HOME", "/h"), ("XDG_CACHE_HOME", "x")];
        assert_eq!(
            user_root_from(&with(&relative), "linux"),
            Some(PathBuf::from("/h/.cache/nazm"))
        );
        let moved = [("HOME", "/h"), ("NAZM_CACHE_DIR", "/c")];
        assert_eq!(
            user_root_from(&with(&moved), "macos"),
            Some(PathBuf::from("/c"))
        );
        let named = [
            ("HOME", "/h"),
            ("NAZM_CACHE_DIR", "/c"),
            ("NAZM_USER_CACHE_DIR", "/u"),
        ];
        assert_eq!(
            user_root_from(&with(&named), "macos"),
            Some(PathBuf::from("/u"))
        );
        let off = [("HOME", "/h"), ("NAZM_USER_CACHE_DIR", "")];
        assert_eq!(user_root_from(&with(&off), "macos"), None);
        assert_eq!(user_root_from(&with(&[]), "linux"), None);
    }
}
