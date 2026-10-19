//! Where a compilation's durable identities are measured from.
//!
//! # The problem N3 left open, on purpose
//!
//! A `ModuleKey` is a module's path relative to the **source root**, and N3 derived that
//! root from the root module's directory. So `lib/common.nz` is `lib/common.nz` when the
//! compilation began at `app.nz` and `common.nz` when it began at `lib/common.nz` itself.
//! Both are correct within their compilation, and `docs/user/spec.md` said so — but nothing
//! that reuses work across compilations can rest on it, because the same module would
//! answer to two names.
//!
//! # The smallest mechanism that fixes it
//!
//! A marker file, `nazm.root`, whose **presence** declares a directory to be the source
//! root. It is required to be empty. That is the whole of it, and the emptiness is
//! enforced rather than suggested: the moment it is allowed to carry a field it becomes a
//! manifest, and a manifest is a package system's front door — versions, dependencies, a
//! registry, a resolver. None of those is needed to answer *where does this project
//! begin*, and building them to answer it would be the mistake `docs/user/roadmap.md` has been
//! naming since N3.
//!
//! `--source-root DIR` names one explicitly, for a tree that cannot be written to and for
//! tests that need to be sure. When neither is present the root is derived exactly as it
//! was, so nothing that worked before changes.
//!
//! # Why this is not a package system
//!
//! It has no content, so it cannot express a dependency, a version, a feature or a
//! target. It does not name the modules in the project — the import graph still does. It
//! does not have to exist. What it owns is one fact that nothing else could own, because
//! the alternative is to infer it from whichever file was named on the command line.
//!
//! # Absolute here, relative everywhere else
//!
//! The root and the modules are compared as absolute paths so that the answer cannot
//! depend on the shell's working directory, and neither the absolute root nor the working
//! directory reaches a key: only the relative remainder does. Paths in diagnostics are
//! untouched — they are still the spelling the user typed.

use std::path::{Component, Path, PathBuf};

/// The file whose presence declares a source root. Required to be empty.
pub const MARKER: &str = "nazm.root";

/// A compilation's source root, and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    path: PathBuf,
    how: How,
}

/// How the source root was arrived at.
///
/// Reported rather than hidden: only a declared root gives a module the same key
/// whichever file the compilation started from, so a reader of a cache report should not
/// have to guess which of the two they got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// `--source-root` named it.
    Given,
    /// A `nazm.root` marker was found at or above the root module.
    Marker,
    /// A package manifest (N46) at or above the root module declared the source root it
    /// lies in.
    Package,
    /// Neither: the root module's own directory, as before N4. Durable identity still
    /// works, and is relative to a root that depends on which file was named — so two
    /// compilations from two entry points do not share entries. Sound, and a smaller
    /// cache.
    Derived,
}

impl SourceRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn how(&self) -> How {
        self.how
    }
}

impl How {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Given => "given",
            Self::Marker => "marker",
            Self::Package => "package",
            Self::Derived => "derived",
        }
    }
}

/// Make `path` absolute without asking the filesystem what it points at.
///
/// Lexical on purpose. `canonicalize` resolves symbolic links, and the two compiler
/// implementations disagree about those — `docs/user/spec.md` settles that by declining a
/// durable identity where they could, which only works if this function does not quietly
/// pick one of the two answers. The working directory is joined because a relative path
/// means nothing without it, and it is removed again by the time a key exists.
pub fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The source root the package manifest in `dir` declares, if there is one and it holds
/// `module` — in the module's own spelling, like every other answer here.
///
/// # Errors
///
/// A manifest that is there and is not a valid one: a build does not quietly become a
/// non-package build because its manifest has a typo.
fn package_root(dir: &Path, module: &Path) -> Result<Option<PathBuf>, String> {
    let path = dir.join(nazm_package::MANIFEST);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let manifest = nazm_package::Manifest::parse(&text, &path.display().to_string())
        .map_err(|d| format!("{}: {}", d.code, d.message))?;
    let root = dir.join(&manifest.root);
    Ok(module.starts_with(&root).then_some(root))
}

/// Decide the source root for a compilation whose root module is `module`.
///
/// # Errors
///
/// A `--source-root` that is not a directory or does not contain the root module, and a
/// marker file that is not empty. Each is a mistake about the project's shape, said once
/// rather than turned into a compilation that quietly computes different identities.
pub fn decide(module: &Path, given: Option<&Path>) -> Result<SourceRoot, String> {
    let module = absolute(module);
    let here = module.parent().unwrap_or(Path::new("/")).to_path_buf();

    if let Some(given) = given {
        let path = absolute(given);
        if !path.is_dir() {
            return Err(format!("`{}` is not a directory", given.display()));
        }
        // Returned in the **root module's** spelling rather than in the one that was
        // typed. Every module of this compilation is reached by joining a relative import
        // onto that spelling, so a root spelled differently — `/var/…` where the shell
        // says `/private/var/…`, a mount point, a home directory reached through a link —
        // would be a prefix of nothing and every module would silently lose its key.
        //
        // `canonicalize` appears here and nowhere else, and only to decide **which
        // ancestor** the given directory is. It never decides a module's identity: that
        // stays lexical, because the compiler written in Nazm has no `realpath` and
        // `docs/user/spec.md` settles the difference by declining rather than by picking one.
        let want = path.canonicalize().unwrap_or_else(|_| path.clone());
        let found = module
            .ancestors()
            .find(|a| a.canonicalize().is_ok_and(|c| c == want));
        return found
            .map(|p| SourceRoot {
                path: p.to_path_buf(),
                how: How::Given,
            })
            .ok_or_else(|| {
                format!(
                    "`{}` is not inside the source root `{}`; every module of a compilation \
                 is identified by its path relative to the root, and the root module \
                 cannot be outside it",
                    module.display(),
                    given.display()
                )
            });
    }

    // Upwards from the root module's directory, first marker wins. A nested project
    // inside another one is therefore its own project, which is what a reader standing in
    // the inner directory would expect.
    let mut at: Option<&Path> = Some(&here);
    while let Some(dir) = at {
        // A package's manifest (N46) declares its source root; the first one upward whose root
        // holds the module is this compilation's. Checked before the marker in the same
        // directory, and it too is the first found.
        if let Some(root) = package_root(dir, &module)? {
            return Ok(SourceRoot {
                path: root,
                how: How::Package,
            });
        }
        let marker = dir.join(MARKER);
        if marker.is_file() {
            let text = std::fs::read_to_string(&marker)
                .map_err(|e| format!("could not read `{}`: {e}", marker.display()))?;
            if !text.trim().is_empty() {
                return Err(format!(
                    "`{}` must be empty. It declares where this project begins and owns \
                     nothing else; a marker that carried settings would be a manifest, \
                     and a package's manifest is `nazm.toml`",
                    marker.display()
                ));
            }
            return Ok(SourceRoot {
                path: dir.to_path_buf(),
                how: How::Marker,
            });
        }
        at = dir.parent();
    }

    Ok(SourceRoot {
        path: here,
        how: How::Derived,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(&path, text).expect("write");
    }

    #[test]
    fn with_no_marker_the_root_is_the_root_modules_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "lib/util.nz", "");
        let root = decide(&dir.path().join("lib/util.nz"), None).expect("a root");
        assert_eq!(root.how(), How::Derived);
        assert_eq!(root.path(), absolute(&dir.path().join("lib")));
    }

    #[test]
    fn a_marker_makes_the_root_the_same_for_every_entry_point() {
        // The N3 limitation, closed. Two modules at different depths of one project agree
        // about where the project begins, which is what makes their keys comparable.
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), MARKER, "");
        write(dir.path(), "app.nz", "");
        write(dir.path(), "lib/deep/util.nz", "");

        let from_app = decide(&dir.path().join("app.nz"), None).expect("a root");
        let from_util = decide(&dir.path().join("lib/deep/util.nz"), None).expect("a root");
        assert_eq!(from_app.path(), from_util.path());
        assert_eq!(from_app.how(), How::Marker);
        assert_eq!(from_util.how(), How::Marker);
    }

    #[test]
    fn the_nearest_marker_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), MARKER, "");
        write(dir.path(), "inner/nazm.root", "");
        write(dir.path(), "inner/m.nz", "");
        let root = decide(&dir.path().join("inner/m.nz"), None).expect("a root");
        assert_eq!(root.path(), absolute(&dir.path().join("inner")));
    }

    #[test]
    fn a_marker_with_content_is_refused_rather_than_read() {
        // It declares a root. The first field it is allowed to carry is the first field
        // of a manifest, and this language has nowhere to put one.
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), MARKER, "name = \"thing\"\n");
        write(dir.path(), "m.nz", "");
        let e = decide(&dir.path().join("m.nz"), None).expect_err("refused");
        assert!(e.contains("must be empty"), "{e}");
    }

    #[test]
    fn a_given_root_must_contain_the_root_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "a/m.nz", "");
        std::fs::create_dir_all(dir.path().join("b")).expect("b");
        let e =
            decide(&dir.path().join("a/m.nz"), Some(&dir.path().join("b"))).expect_err("refused");
        assert!(e.contains("not inside the source root"), "{e}");
    }

    #[test]
    fn a_given_root_beats_a_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "inner/nazm.root", "");
        write(dir.path(), "inner/m.nz", "");
        let root = decide(&dir.path().join("inner/m.nz"), Some(dir.path())).expect("a root");
        assert_eq!(root.how(), How::Given);
        assert_eq!(root.path(), absolute(dir.path()));
    }

    #[test]
    fn absolutising_does_not_depend_on_the_working_directory_once_it_is_absolute() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("x/../y/./z.nz");
        assert_eq!(absolute(&p), dir.path().join("y/z.nz"));
    }
}
