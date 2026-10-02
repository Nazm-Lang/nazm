//! A local registry (N56, `docs/architecture.md` §7.58): an index per package and an immutable,
//! digest-checked copy of every published version, in a directory a manifest names.
//!
//! ```text
//! REGISTRY/index/NAME.toml                 nazm.registry-index/1: every version, its digest
//! REGISTRY/packages/NAME/VERSION/…         the published package: its manifest and sources
//! ```
//!
//! Nothing here reaches a network. A registry is read only through [`Index::read`] and a package
//! in it only through [`Index::fetch`], which refuses one whose bytes do not match the digest its
//! index records or that holds anything but files and directories.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use nazm_diag::{codes, Diagnostic};
use serde::Deserialize;

use crate::{digest, error, valid_name, Manifest, Version, MANIFEST};

/// The index's schema.
pub const INDEX_SCHEMA: &str = "nazm.registry-index/1";

/// What a dependency on a registry package accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VersionReq {
    /// `1.2.3` or `=1.2.3`: that version and no other.
    Exact(Version),
    /// `^1.2.3`: at least it, and the same left-most non-zero component — `^1.2.3` is
    /// `>=1.2.3, <2.0.0`, `^0.2.3` is `>=0.2.3, <0.3.0`, `^0.0.3` is exactly `0.0.3`.
    Caret(Version),
    /// `~1.2.3`: at least it, with the same major and minor: `>=1.2.3, <1.3.0`.
    Tilde(Version),
}

impl VersionReq {
    /// `text` as a requirement, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(v) = text.strip_prefix('^') {
            return Version::parse(v).map(Self::Caret);
        }
        if let Some(v) = text.strip_prefix('~') {
            return Version::parse(v).map(Self::Tilde);
        }
        Version::parse(text.strip_prefix('=').unwrap_or(text)).map(Self::Exact)
    }

    /// Whether `v` satisfies it.
    #[must_use]
    pub fn matches(self, v: Version) -> bool {
        match self {
            Self::Exact(base) => v == base,
            Self::Caret(base) => {
                v >= base
                    && if base.major > 0 {
                        v.major == base.major
                    } else if base.minor > 0 {
                        v.major == 0 && v.minor == base.minor
                    } else {
                        v == base
                    }
            }
            Self::Tilde(base) => v >= base && v.major == base.major && v.minor == base.minor,
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(v) => write!(f, "={v}"),
            Self::Caret(v) => write!(f, "^{v}"),
            Self::Tilde(v) => write!(f, "~{v}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    schema: String,
    name: String,
    #[serde(default)]
    version: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    version: String,
    digest: String,
    #[serde(default)]
    yanked: bool,
}

/// One published version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub version: Version,
    pub digest: String,
    /// Withdrawn by its publisher: never chosen anew, still used where a lockfile names it.
    pub yanked: bool,
}

/// A package's index: every version ever published, sorted, each once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub entries: Vec<Entry>,
    registry: PathBuf,
}

fn well_formed_digest(d: &str) -> bool {
    d.strip_prefix("blake3:").is_some_and(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

impl Index {
    /// The index of `name` in the registry at `registry`.
    ///
    /// # Errors
    ///
    /// `N0501` when the registry has no such package; `N0512` when its index is not a
    /// `nazm.registry-index/1` naming it, records a version twice, or a digest of another shape.
    pub fn read(registry: &Path, name: &str) -> Result<Self, Box<Diagnostic>> {
        if !valid_name(name) {
            return Err(Box::new(error(
                codes::MANIFEST_INVALID,
                format!("`{name}` is not a package name"),
            )));
        }
        let path = registry.join("index").join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&path).map_err(|_| {
            Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!(
                    "the registry at `{}` has no package `{name}`",
                    registry.display()
                ),
            ))
        })?;
        let bad = |what: String| {
            Box::new(error(
                codes::REGISTRY_INVALID,
                format!("`{}`: {what}", path.display()),
            ))
        };
        let raw: RawIndex = toml::from_str(&text).map_err(|e| bad(e.message().to_owned()))?;
        if raw.schema != INDEX_SCHEMA {
            return Err(bad(format!(
                "the schema is `{}`, not `{INDEX_SCHEMA}`",
                raw.schema
            )));
        }
        if raw.name != name {
            return Err(bad(format!("it indexes `{}`, not `{name}`", raw.name)));
        }
        let mut seen: BTreeMap<Version, Entry> = BTreeMap::new();
        for e in raw.version {
            let v = Version::parse(&e.version)
                .ok_or_else(|| bad(format!("`{}` is not a version", e.version)))?;
            if !well_formed_digest(&e.digest) {
                return Err(bad(format!(
                    "{v}'s digest `{}` is not `blake3:` and 64 hex digits",
                    e.digest
                )));
            }
            if seen.contains_key(&v) {
                return Err(bad(format!("{v} is recorded twice")));
            }
            seen.insert(
                v,
                Entry {
                    version: v,
                    digest: e.digest,
                    yanked: e.yanked,
                },
            );
        }
        Ok(Self {
            name: raw.name,
            entries: seen.into_values().collect(),
            registry: registry.to_path_buf(),
        })
    }

    /// The version to use: `preferred` when it satisfies every requirement and is published
    /// (yanked or not — a lockfile's choice stands); otherwise the highest non-yanked version
    /// that does. Never a version the requirements exclude.
    #[must_use]
    pub fn choose(&self, reqs: &[VersionReq], preferred: Option<Version>) -> Option<Version> {
        let ok = |v: Version| reqs.iter().all(|r| r.matches(v));
        if let Some(p) = preferred {
            if ok(p) && self.entries.iter().any(|e| e.version == p) {
                return Some(p);
            }
        }
        self.entries
            .iter()
            .rev()
            .find(|e| !e.yanked && ok(e.version))
            .map(|e| e.version)
    }

    /// Where a version's published copy is.
    #[must_use]
    pub fn dir(&self, v: Version) -> PathBuf {
        self.registry
            .join("packages")
            .join(&self.name)
            .join(v.to_string())
    }

    /// The published copy of `v`, checked: its manifest names this package and version, it holds
    /// only files and directories, and its bytes match the digest the index records.
    ///
    /// # Errors
    ///
    /// `N0501` when it is not there, `N0514` for a link or a special file, `N0500` for a manifest
    /// naming another package or version, `N0507` for bytes that do not match the digest.
    pub fn fetch(&self, v: Version) -> Result<(Manifest, PathBuf), Box<Diagnostic>> {
        let Some(entry) = self.entries.iter().find(|e| e.version == v) else {
            return Err(Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!("the registry has no `{}` {v}", self.name),
            )));
        };
        let dir = self.dir(v);
        if !dir.is_dir() {
            return Err(Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!(
                    "the registry indexes `{}` {v} and has no copy at `{}`",
                    self.name,
                    dir.display()
                ),
            )));
        }
        only_files(&dir)?;
        let manifest_path = dir.join(MANIFEST);
        let bytes = std::fs::read(&manifest_path).map_err(|_| {
            Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!("no manifest `{}`", manifest_path.display()),
            ))
        })?;
        let manifest = Manifest::parse(
            &String::from_utf8_lossy(&bytes),
            &manifest_path.display().to_string(),
        )?;
        if manifest.name != self.name || manifest.version != v {
            return Err(Box::new(error(
                codes::MANIFEST_INVALID,
                format!(
                    "the registry's copy of `{}` {v} says it is `{}` {}",
                    self.name, manifest.name, manifest.version
                ),
            )));
        }
        let have = digest(&bytes, &dir.join(&manifest.root));
        if have != entry.digest {
            return Err(Box::new(
                error(
                    codes::DIGEST_MISMATCH,
                    format!(
                        "the registry's copy of `{}` {v} does not match the digest its index \
                         records: it changed after it was published",
                        self.name
                    ),
                )
                .help("a published version is immutable; publish a new version instead"),
            ));
        }
        Ok((manifest, dir))
    }
}

/// Refuse a directory tree holding anything but regular files and directories — a symbolic link
/// could point anywhere, inside the registry or out of it.
fn only_files(dir: &Path) -> Result<(), Box<Diagnostic>> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| {
            Box::new(error(
                codes::PACKAGE_UNSAFE,
                format!("`{}` cannot be read: {e}", d.display()),
            ))
        })?;
        for e in entries.flatten() {
            let path = e.path();
            let kind = std::fs::symlink_metadata(&path).map(|m| m.file_type());
            match kind {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_file() => {}
                _ => {
                    return Err(Box::new(error(
                        codes::PACKAGE_UNSAFE,
                        format!(
                        "`{}` is not a file or a directory; a registry package holds only those",
                        path.display()
                    ),
                    )))
                }
            }
        }
    }
    Ok(())
}

/// Every `.nz` file under `root` and the manifest, relative, in path order — what a publish copies.
fn published_files(package: &Path, manifest: &Manifest) -> Vec<String> {
    let mut out = vec![MANIFEST.to_owned()];
    for (rel, _) in crate::sources(&package.join(&manifest.root)) {
        out.push(format!("{}/{rel}", manifest.root));
    }
    out
}

/// Publish the package in `dir` into the registry at `registry`: copy its manifest and sources,
/// and record the version and its digest in the index. Returns the version and the digest.
///
/// # Errors
///
/// `N0500` for an invalid manifest or a path dependency (a published package depends only on the
/// registry), `N0514` for a link in it, `N0512` when the version is already published — a
/// published version never changes — and the filesystem's, as `N0512`.
/// Every check a publish makes before it writes anything, shared by [`publish`] and
/// [`publish_dry_run`] so the two cannot disagree about what would be refused.
fn prepare(dir: &Path, registry: &Path) -> Result<(Manifest, String, Vec<Entry>), Box<Diagnostic>> {
    let manifest_path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest_path).map_err(|_| {
        Box::new(error(
            codes::PACKAGE_NOT_FOUND,
            format!("no manifest `{}`", manifest_path.display()),
        ))
    })?;
    let manifest = Manifest::parse(&text, &manifest_path.display().to_string())?;
    if let Some((name, _)) = manifest
        .dependencies
        .iter()
        .find(|(_, d)| matches!(d, crate::Dependency::Path { .. }))
    {
        return Err(Box::new(error(
            codes::MANIFEST_INVALID,
            format!(
                "`{}` depends on `{name}` by path; a published package depends only on registry \
                 packages",
                manifest.name
            ),
        )));
    }
    only_files(dir)?;
    let index_path = registry
        .join("index")
        .join(format!("{}.toml", manifest.name));
    let entries = if index_path.is_file() {
        Index::read(registry, &manifest.name)?.entries
    } else {
        Vec::new()
    };
    if entries.iter().any(|e| e.version == manifest.version) {
        return Err(Box::new(error(
            codes::REGISTRY_INVALID,
            format!(
                "`{}` {} is already published; a published version never changes",
                manifest.name, manifest.version
            ),
        )));
    }
    Ok((manifest, text, entries))
}

pub fn publish(dir: &Path, registry: &Path) -> Result<(Version, String), Box<Diagnostic>> {
    let (manifest, text, mut entries) = prepare(dir, registry)?;
    let io = |e: std::io::Error| {
        Box::new(error(
            codes::REGISTRY_INVALID,
            format!("the registry at `{}`: {e}", registry.display()),
        ))
    };
    let target = registry
        .join("packages")
        .join(&manifest.name)
        .join(manifest.version.to_string());
    for rel in published_files(dir, &manifest) {
        let to = target.join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::copy(dir.join(&rel), &to).map_err(io)?;
    }
    let d = digest(text.as_bytes(), &target.join(&manifest.root));
    entries.push(Entry {
        version: manifest.version,
        digest: d.clone(),
        yanked: false,
    });
    write_index(registry, &manifest.name, &mut entries).map_err(io)?;
    Ok((manifest.version, d))
}

/// What [`publish`] would do, writing nothing (N74): the same checks, the version, the digest the
/// index would record, and the files it would copy, relative to `dir`.
///
/// # Errors
///
/// Exactly [`publish`]'s refusals.
pub fn publish_dry_run(
    dir: &Path,
    registry: &Path,
) -> Result<(Version, String, Vec<String>), Box<Diagnostic>> {
    let (manifest, text, _) = prepare(dir, registry)?;
    let files = published_files(dir, &manifest);
    let d = digest(text.as_bytes(), &dir.join(&manifest.root));
    Ok((manifest.version, d, files))
}

/// Mark `name` `version` yanked, or not: it stays published and keeps its digest.
///
/// # Errors
///
/// [`Index::read`]'s, `N0501` for a version not published, and the filesystem's, as `N0512`.
pub fn yank(
    registry: &Path,
    name: &str,
    version: Version,
    yanked: bool,
) -> Result<(), Box<Diagnostic>> {
    let mut entries = Index::read(registry, name)?.entries;
    let Some(e) = entries.iter_mut().find(|e| e.version == version) else {
        return Err(Box::new(error(
            codes::PACKAGE_NOT_FOUND,
            format!("the registry has no `{name}` {version}"),
        )));
    };
    e.yanked = yanked;
    write_index(registry, name, &mut entries).map_err(|e| {
        Box::new(error(
            codes::REGISTRY_INVALID,
            format!("the registry at `{}`: {e}", registry.display()),
        ))
    })
}

/// An index's exact bytes, versions ascending, written by a rename so a reader never sees half.
fn write_index(registry: &Path, name: &str, entries: &mut [Entry]) -> std::io::Result<()> {
    entries.sort_by_key(|e| e.version);
    let mut out = String::new();
    out.push_str(&format!("schema = \"{INDEX_SCHEMA}\"\nname = \"{name}\"\n"));
    for e in entries.iter() {
        out.push_str(&format!(
            "\n[[version]]\nversion = \"{}\"\ndigest = \"{}\"\nyanked = {}\n",
            e.version, e.digest, e.yanked
        ));
    }
    let dir = registry.join("index");
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{name}.toml.tmp"));
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, dir.join(format!("{name}.toml")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_requirement_admits_exactly_what_its_operator_says() {
        let caret = VersionReq::parse("^1.2.3").unwrap();
        assert!(caret.matches(v("1.2.3")) && caret.matches(v("1.9.0")));
        assert!(!caret.matches(v("1.2.2")) && !caret.matches(v("2.0.0")));
        let zero = VersionReq::parse("^0.2.3").unwrap();
        assert!(zero.matches(v("0.2.9")) && !zero.matches(v("0.3.0")));
        let tiny = VersionReq::parse("^0.0.3").unwrap();
        assert!(tiny.matches(v("0.0.3")) && !tiny.matches(v("0.0.4")));
        let tilde = VersionReq::parse("~1.2.3").unwrap();
        assert!(tilde.matches(v("1.2.9")) && !tilde.matches(v("1.3.0")));
        for exact in ["1.2.3", "=1.2.3"] {
            let e = VersionReq::parse(exact).unwrap();
            assert!(e.matches(v("1.2.3")) && !e.matches(v("1.2.4")));
        }
        for bad in ["^", "~1.2", ">=1.0.0", "1.2.x", "^^1.0.0"] {
            assert!(VersionReq::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_highest_unyanked_match_wins_unless_a_preference_still_matches() {
        let index = Index {
            name: "a".to_owned(),
            entries: ["1.0.0", "1.1.0", "1.2.0", "2.0.0"]
                .iter()
                .map(|s| Entry {
                    version: v(s),
                    digest: String::new(),
                    yanked: *s == "1.2.0",
                })
                .collect(),
            registry: PathBuf::new(),
        };
        let caret = [VersionReq::parse("^1.0.0").unwrap()];
        assert_eq!(index.choose(&caret, None), Some(v("1.1.0")));
        assert_eq!(index.choose(&caret, Some(v("1.0.0"))), Some(v("1.0.0")));
        // A lockfile may keep a yanked version; nothing chooses one anew.
        assert_eq!(index.choose(&caret, Some(v("1.2.0"))), Some(v("1.2.0")));
        assert_eq!(index.choose(&caret, Some(v("2.0.0"))), Some(v("1.1.0")));
        let none = [VersionReq::parse("^3.0.0").unwrap()];
        assert_eq!(index.choose(&none, None), None);
    }
}
