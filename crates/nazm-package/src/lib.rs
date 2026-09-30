//! Packages (N46): the manifest, deterministic resolution, integrity digests and the lockfile.
//!
//! `docs/architecture.md` §7.48 is the contract. A package is a directory holding
//! [`MANIFEST`]; [`resolve`] reads the root package's manifest and every dependency's, in name
//! order, and returns a [`Graph`] — packages sorted by name, each with its identity, its source
//! root, its dependencies and its [digest](digest). [`Graph::lock_text`] is the lockfile's exact
//! bytes and [`Graph::verify`] holds a build to one. Nothing here reads a network, an
//! environment variable, a timestamp or a directory's listing order.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use nazm_diag::{codes, Code, Diagnostic};
use nazm_span::Span;
use serde::Deserialize;

/// The manifest's file name.
pub const MANIFEST: &str = "nazm.toml";
/// The lockfile's file name, beside the root manifest.
pub const LOCKFILE: &str = "nazm.lock";
/// The manifest's schema.
pub const MANIFEST_SCHEMA: &str = "nazm.package/1";
/// The lockfile's schema.
pub const LOCK_SCHEMA: &str = "nazm.lock/1";
/// The digest's domain-separation tag: a change to what is hashed is a new tag.
const DIGEST_TAG: &str = "nazm.package-digest/1";
/// How deep a chain of dependencies may go before resolution refuses it.
const MAX_DEPTH: usize = 256;

/// An exact version: `MAJOR.MINOR.PATCH`, decimal, no pre-release or build metadata in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `text` as a version, or `None` if it is not exactly three decimal numbers.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut next = || -> Option<u64> {
            let p = parts.next()?;
            let ok = !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p == "0" || !p.starts_with('0'));
            if ok {
                p.parse().ok()
            } else {
                None
            }
        };
        let v = Self {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Whether `name` is a package name: `[a-z][a-z0-9-]*`, at most 64 bytes, not ending in `-`.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.first().is_some_and(u8::is_ascii_lowercase)
        && name.len() <= 64
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && !name.ends_with('-')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: String,
    package: RawPackage,
    #[serde(default)]
    dependencies: BTreeMap<String, RawDependency>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackage {
    name: String,
    version: String,
    root: Option<String>,
    main: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependency {
    path: String,
    version: String,
}

/// A manifest, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: Version,
    /// The source root, relative to the manifest's directory.
    pub root: String,
    /// The entry module, relative to the source root.
    pub main: Option<String>,
    /// Each dependency's name, with its path relative to this manifest and its exact version.
    pub dependencies: BTreeMap<String, (String, Version)>,
}

fn error(code: Code, message: String) -> Diagnostic {
    Diagnostic::error(code, Span::synthetic(), message)
}

/// Whether `rel` is a relative path that stays where it starts: no root, no drive, no `..`.
fn contained(rel: &str) -> bool {
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

impl Manifest {
    /// Parse a manifest's text; `file` names it in a diagnostic.
    ///
    /// # Errors
    ///
    /// `N0500`: not TOML, an unknown key, another schema, a name or version of the wrong shape,
    /// or a root or entry that leaves the package.
    pub fn parse(text: &str, file: &str) -> Result<Self, Box<Diagnostic>> {
        let raw: RawManifest = toml::from_str(text).map_err(|e| {
            Box::new(error(
                codes::MANIFEST_INVALID,
                format!("`{file}`: {}", e.message()),
            ))
        })?;
        let bad =
            |what: String| Box::new(error(codes::MANIFEST_INVALID, format!("`{file}`: {what}")));
        if raw.schema != MANIFEST_SCHEMA {
            return Err(bad(format!(
                "the schema is `{}`; this compiler reads `{MANIFEST_SCHEMA}`",
                raw.schema
            )));
        }
        if !valid_name(&raw.package.name) {
            return Err(bad(format!(
                "`{}` is not a package name: lowercase ASCII letters, digits and `-`, starting \
                 with a letter, at most 64 bytes",
                raw.package.name
            )));
        }
        let version = Version::parse(&raw.package.version).ok_or_else(|| {
            bad(format!(
                "`{}` is not a version: MAJOR.MINOR.PATCH",
                raw.package.version
            ))
        })?;
        let root = raw.package.root.unwrap_or_else(|| "src".to_owned());
        if !contained(&root) {
            return Err(bad(format!("the root `{root}` leaves the package")));
        }
        if let Some(m) = &raw.package.main {
            if !contained(m) || !m.ends_with(".nz") {
                return Err(bad(format!(
                    "the entry `{m}` is not a `.nz` file inside the root"
                )));
            }
        }
        let mut dependencies = BTreeMap::new();
        for (name, d) in raw.dependencies {
            if !valid_name(&name) {
                return Err(bad(format!("`{name}` is not a package name")));
            }
            let v = Version::parse(&d.version).ok_or_else(|| {
                bad(format!(
                    "the dependency `{name}`'s version `{}` is not MAJOR.MINOR.PATCH",
                    d.version
                ))
            })?;
            if name == raw.package.name {
                return Err(bad(format!("`{name}` depends on itself")));
            }
            dependencies.insert(name, (d.path, v));
        }
        Ok(Self {
            name: raw.package.name,
            version,
            root,
            main: raw.package.main,
            dependencies,
        })
    }
}

/// One resolved package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub version: Version,
    /// Its directory, canonical.
    pub dir: PathBuf,
    /// Its source root, canonical.
    pub source_root: PathBuf,
    /// Its entry module, if it has one.
    pub main: Option<PathBuf>,
    /// Where it is, relative to the root package's directory, `/`-separated: `path+.` for the
    /// root itself.
    pub source: String,
    /// Each dependency's name and its index in [`Graph::packages`], in name order.
    pub dependencies: BTreeMap<String, usize>,
    /// BLAKE3 over its manifest and its sources: `blake3:` and 64 hex digits.
    pub digest: String,
}

impl Package {
    /// The prefix of every one of its modules' durable keys: `NAME@VERSION`.
    #[must_use]
    pub fn key_prefix(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// A resolved build: every package, sorted by name, and which is the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    pub packages: Vec<Package>,
    pub root: usize,
}

/// Whether `dir` is a package: it holds a manifest.
#[must_use]
pub fn is_package(dir: &Path) -> bool {
    dir.join(MANIFEST).is_file()
}

struct Resolver {
    /// In discovery order; sorted at the end.
    found: Vec<(Manifest, PathBuf)>,
    by_dir: BTreeMap<PathBuf, usize>,
    by_name: BTreeMap<String, usize>,
    /// Canonical directories on the current path, for cycles.
    path: Vec<PathBuf>,
    edges: Vec<BTreeMap<String, usize>>,
}

impl Resolver {
    fn visit(
        &mut self,
        dir: &Path,
        wanted: Option<(&str, Version, &str)>,
    ) -> Result<usize, Box<Diagnostic>> {
        if self.path.len() > MAX_DEPTH {
            return Err(Box::new(error(
                codes::PACKAGE_CYCLE,
                format!("the chain of dependencies is deeper than {MAX_DEPTH}"),
            )));
        }
        let manifest_path = dir.join(MANIFEST);
        let canonical = dir.canonicalize().map_err(|_| {
            let (name, by) = wanted.map_or(("the root package", ""), |(n, _, b)| (n, b));
            Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!(
                    "`{name}`{}: no directory `{}`",
                    if by.is_empty() {
                        String::new()
                    } else {
                        format!(", declared by `{by}`")
                    },
                    dir.display()
                ),
            ))
        })?;
        if let Some(at) = self.path.iter().position(|p| *p == canonical) {
            let mut names: Vec<String> = self
                .path
                .get(at..)
                .unwrap_or(&[])
                .iter()
                .filter_map(|p| self.by_dir.get(p).and_then(|&i| self.found.get(i)))
                .map(|f| f.0.name.clone())
                .collect();
            if let Some(first) = names.first().cloned() {
                names.push(first);
            }
            return Err(Box::new(error(
                codes::PACKAGE_CYCLE,
                format!("a dependency cycle: {}", names.join(" → ")),
            )));
        }
        if let Some(&i) = self.by_dir.get(&canonical) {
            // A diamond: the same package, reached again. Its declared version still has to be
            // the one it states.
            if let (Some((name, v, by)), Some((m, _))) = (wanted, self.found.get(i)) {
                if m.version != v {
                    return Err(Box::new(version_mismatch(name, v, m.version, by)));
                }
            }
            return Ok(i);
        }
        let text = std::fs::read_to_string(&manifest_path).map_err(|_| {
            Box::new(error(
                codes::PACKAGE_NOT_FOUND,
                format!("no manifest `{}`", manifest_path.display()),
            ))
        })?;
        let manifest = Manifest::parse(&text, &manifest_path.display().to_string())?;
        if let Some((name, v, by)) = wanted {
            if manifest.name != name {
                return Err(Box::new(error(
                    codes::PACKAGE_CONFLICT,
                    format!(
                        "`{by}` declares the dependency `{name}`, and the package at `{}` is \
                         named `{}`",
                        dir.display(),
                        manifest.name
                    ),
                )));
            }
            if manifest.version != v {
                return Err(Box::new(version_mismatch(name, v, manifest.version, by)));
            }
        }
        if let Some(&other) = self.by_name.get(&manifest.name) {
            return Err(Box::new(error(
                codes::PACKAGE_CONFLICT,
                format!(
                    "two packages named `{}`: `{}` and `{}`; one build has one package of each name",
                    manifest.name,
                    self.found
                        .get(other)
                        .map(|f| f.1.display().to_string())
                        .unwrap_or_default(),
                    canonical.display()
                ),
            )));
        }
        let index = self.found.len();
        self.by_dir.insert(canonical.clone(), index);
        self.by_name.insert(manifest.name.clone(), index);
        self.found.push((manifest.clone(), canonical.clone()));
        self.edges.push(BTreeMap::new());
        self.path.push(canonical.clone());
        // In name order: a BTreeMap iterates sorted.
        for (name, (path, v)) in &manifest.dependencies {
            let child = self.visit(&canonical.join(path), Some((name, *v, &manifest.name)))?;
            if let Some(e) = self.edges.get_mut(index) {
                e.insert(name.clone(), child);
            }
        }
        self.path.pop();
        Ok(index)
    }
}

fn version_mismatch(name: &str, want: Version, have: Version, by: &str) -> Diagnostic {
    error(
        codes::VERSION_MISMATCH,
        format!("`{by}` declares `{name}` at {want}, and its manifest says {have}"),
    )
    .help("a dependency's declared version must equal the version its own manifest states")
}

/// `to` relative to `from`, `/`-separated; both canonical.
fn relative(from: &Path, to: &Path) -> String {
    let a: Vec<Component<'_>> = from.components().collect();
    let b: Vec<Component<'_>> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); a.len().saturating_sub(common)];
    for c in b.get(common..).unwrap_or(&[]) {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Every `.nz` file under `root`, as (path relative to `root`, `/`-separated; absolute path), in
/// path order.
fn sources(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = e.path();
            if path.is_dir() {
                stack.push((path, rel));
            } else if name.ends_with(".nz") {
                out.push((rel, path));
            }
        }
    }
    out.sort();
    out
}

/// A package's digest: BLAKE3 over its manifest's bytes and every `.nz` file under its source
/// root, each as its relative path, its length and its bytes, in path order.
#[must_use]
pub fn digest(manifest: &[u8], source_root: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(DIGEST_TAG.as_bytes());
    h.update(&[0]);
    h.update(&(manifest.len() as u64).to_le_bytes());
    h.update(manifest);
    for (rel, path) in sources(source_root) {
        let bytes = std::fs::read(&path).unwrap_or_default();
        h.update(rel.as_bytes());
        h.update(&[0]);
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    format!("blake3:{}", h.finalize().to_hex())
}

/// Resolve the package in `dir` and everything it depends on.
///
/// # Errors
///
/// Every refusal of §7.48, each by its own code: an invalid manifest (`N0500`), a missing
/// dependency (`N0501`), a version mismatch (`N0502`), two packages of one name or a declared
/// name its manifest does not have (`N0503`), a cycle (`N0504`).
pub fn resolve(dir: &Path) -> Result<Graph, Vec<Diagnostic>> {
    let mut r = Resolver {
        found: Vec::new(),
        by_dir: BTreeMap::new(),
        by_name: BTreeMap::new(),
        path: Vec::new(),
        edges: Vec::new(),
    };
    let root = r.visit(dir, None).map_err(|d| vec![*d])?;
    let root_dir = r.found.get(root).map(|f| f.1.clone()).unwrap_or_default();
    // Sorted by name: the order a lockfile lists them in, and the only order anything sees.
    let mut order: Vec<usize> = (0..r.found.len()).collect();
    order.sort_by_key(|&i| r.found.get(i).map(|f| f.0.name.clone()));
    let mut position = vec![0; r.found.len()];
    for (new, &old) in order.iter().enumerate() {
        if let Some(p) = position.get_mut(old) {
            *p = new;
        }
    }
    let mut packages = Vec::with_capacity(order.len());
    for &old in &order {
        let Some((m, dir)) = r.found.get(old) else {
            continue;
        };
        let source_root = dir.join(&m.root);
        let Ok(source_root) = source_root.canonicalize() else {
            return Err(vec![error(
                codes::MANIFEST_INVALID,
                format!("`{}`: no source root `{}`", m.name, source_root.display()),
            )]);
        };
        let manifest_bytes = std::fs::read(dir.join(MANIFEST)).unwrap_or_default();
        packages.push(Package {
            name: m.name.clone(),
            version: m.version,
            dir: dir.clone(),
            main: m.main.as_ref().map(|p| source_root.join(p)),
            digest: digest(&manifest_bytes, &source_root),
            source_root,
            source: format!("path+{}", relative(&root_dir, dir)),
            dependencies: r
                .edges
                .get(old)
                .map(|e| {
                    e.iter()
                        .map(|(n, &i)| (n.clone(), position.get(i).copied().unwrap_or(0)))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(Graph {
        packages,
        root: position.get(root).copied().unwrap_or(0),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLock {
    schema: String,
    #[serde(default)]
    package: Vec<RawLocked>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RawLocked {
    name: String,
    version: String,
    source: String,
    digest: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

impl Graph {
    /// The root package: always there, since resolution starts from it.
    #[must_use]
    pub fn root_package(&self) -> Option<&Package> {
        self.packages.get(self.root)
    }

    /// The package whose source root holds `path`, if any: the deepest, when roots nest.
    #[must_use]
    pub fn package_of(&self, path: &Path) -> Option<usize> {
        let canonical = path.canonicalize().ok()?;
        self.packages
            .iter()
            .enumerate()
            .filter(|(_, p)| canonical.starts_with(&p.source_root))
            .max_by_key(|(_, p)| p.source_root.components().count())
            .map(|(i, _)| i)
    }

    /// The lockfile's exact bytes: deterministic, so resolving the same packages twice writes
    /// the same file.
    #[must_use]
    pub fn lock_text(&self) -> String {
        let mut out = String::new();
        out.push_str("# Written by `nazm lock`. Do not edit: `nazm lock` rewrites it.\n");
        out.push_str(&format!("schema = \"{LOCK_SCHEMA}\"\n"));
        for p in &self.packages {
            out.push_str("\n[[package]]\n");
            out.push_str(&format!("name = \"{}\"\n", p.name));
            out.push_str(&format!("version = \"{}\"\n", p.version));
            out.push_str(&format!(
                "source = \"{}\"\n",
                p.source.replace('\\', "/").replace('"', "\\\"")
            ));
            out.push_str(&format!("digest = \"{}\"\n", p.digest));
            let deps: Vec<String> = p.dependencies.keys().map(|d| format!("\"{d}\"")).collect();
            out.push_str(&format!("dependencies = [{}]\n", deps.join(", ")));
        }
        out
    }

    /// Hold this resolution to a lockfile's text.
    ///
    /// # Errors
    ///
    /// `N0506` when the packages, their versions, sources or dependencies differ from what the
    /// lockfile records (or it is not a `nazm.lock/1`), `N0507` when a package is the one
    /// recorded but its bytes no longer match its digest.
    pub fn verify(&self, lock: &str) -> Result<(), Vec<Diagnostic>> {
        let raw: RawLock = toml::from_str(lock).map_err(|e| {
            vec![error(
                codes::LOCK_MISMATCH,
                format!("`{LOCKFILE}` cannot be read: {}", e.message()),
            )]
        })?;
        if raw.schema != LOCK_SCHEMA {
            return Err(vec![error(
                codes::LOCK_MISMATCH,
                format!("`{LOCKFILE}` is `{}`, not `{LOCK_SCHEMA}`", raw.schema),
            )]);
        }
        let mut errors = Vec::new();
        let mut recorded: BTreeMap<&str, &RawLocked> = BTreeMap::new();
        for l in &raw.package {
            recorded.insert(&l.name, l);
        }
        for p in &self.packages {
            let Some(l) = recorded.remove(p.name.as_str()) else {
                errors.push(error(
                    codes::LOCK_MISMATCH,
                    format!("`{}` {} is not in `{LOCKFILE}`", p.name, p.version),
                ));
                continue;
            };
            let deps: Vec<String> = p.dependencies.keys().cloned().collect();
            if l.version != p.version.to_string() || l.source != p.source || l.dependencies != deps
            {
                errors.push(error(
                    codes::LOCK_MISMATCH,
                    format!(
                        "`{}` resolves to {} at `{}`, and `{LOCKFILE}` records {} at `{}`",
                        p.name, p.version, p.source, l.version, l.source
                    ),
                ));
            } else if l.digest != p.digest {
                errors.push(
                    error(
                        codes::DIGEST_MISMATCH,
                        format!(
                            "`{}` {} does not match its digest in `{LOCKFILE}`: its manifest or \
                             sources changed after it was locked",
                            p.name, p.version
                        ),
                    )
                    .help("`nazm lock` records the packages as they are now"),
                );
            }
        }
        for name in recorded.keys() {
            errors.push(error(
                codes::LOCK_MISMATCH,
                format!("`{LOCKFILE}` records `{name}`, which this build does not use"),
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Write the lockfile for `graph` beside its root manifest, if its bytes would change.
/// Returns whether it was written.
///
/// # Errors
///
/// The filesystem's.
pub fn write_lock(graph: &Graph) -> std::io::Result<bool> {
    let Some(root) = graph.root_package() else {
        return Ok(false);
    };
    let path = root.dir.join(LOCKFILE);
    let text = graph.lock_text();
    if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
        return Ok(false);
    }
    std::fs::write(&path, text)?;
    Ok(true)
}

/// The lockfile beside the root manifest, held against `graph`.
///
/// # Errors
///
/// `N0505` when there is none, and [`Graph::verify`]'s.
pub fn verify_locked(graph: &Graph) -> Result<(), Vec<Diagnostic>> {
    let path = graph
        .root_package()
        .map(|r| r.dir.join(LOCKFILE))
        .unwrap_or_else(|| PathBuf::from(LOCKFILE));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Err(vec![error(
            codes::LOCK_MISSING,
            format!("`--locked`, and there is no `{}`", path.display()),
        )
        .help("`nazm lock` writes it")]);
    };
    graph.verify(&text)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_exactly_three_decimal_numbers() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(Version::parse("0.0.0").unwrap().to_string(), "0.0.0");
        for bad in [
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "",
            "1..3",
            "-1.2.3",
            "1.2.3-beta",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_name_is_lowercase_and_bounded() {
        assert!(valid_name("geometry") && valid_name("a1-b"));
        for bad in ["", "Geometry", "1a", "a_b", "a-", "a.b", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_manifest_accepts_nothing_it_does_not_know() {
        let ok = "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n";
        assert_eq!(Manifest::parse(ok, "m").unwrap().root, "src");
        for bad in [
            "schema = \"nazm.package/2\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\nedition = 1\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0\"\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\nroot = \"../x\"\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\nmain = \"/etc/x.nz\"\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nb = { path = \"../b\" }\n",
            "schema = \"nazm.package/1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\na = { path = \"../a\", version = \"1.0.0\" }\n",
            "not toml",
        ] {
            let e = Manifest::parse(bad, "m").unwrap_err();
            assert_eq!(e.code, codes::MANIFEST_INVALID, "{bad}");
        }
    }

    #[test]
    fn a_relative_source_is_spelled_the_same_everywhere() {
        assert_eq!(relative(Path::new("/a/b/app"), Path::new("/a/b/app")), ".");
        assert_eq!(
            relative(Path::new("/a/b/app"), Path::new("/a/b/lib")),
            "../lib"
        );
        assert_eq!(
            relative(Path::new("/a/b/app"), Path::new("/a/b/app/vendor/x")),
            "vendor/x"
        );
    }
}
