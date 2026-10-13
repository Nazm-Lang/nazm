//! Following `use` from a root file to the whole program.
//!
//! # A map of files, not a buffer
//!
//! Every reachable file is read once and given a [`FileId`]; the result is a
//! [`SourceMap`]. Each file is then parsed **on its own text**, so a span is a position in
//! a file a reader can open.
//!
//! Until 2026-09-21 the files were concatenated into a single anonymous buffer and the
//! originating file was recovered by binary-searching the offset. That worked, and it is
//! why this module used to carry a running `base` offset to place an import diagnostic
//! correctly. The arithmetic is gone: a diagnostic raised while reading a file already
//! names that file.
//!
//! # What the loader decides, and what it does not
//!
//! It decides *which files*, *in what order*, and **which of them named which** — the
//! `use` edges, resolved from paths to ids. It does not decide what any of it means: a
//! module's own definitions, what it exports and what it may see are the checker's, and
//! after this function returns nothing consults the filesystem again.
//!
//! Resolving a path is why this lives in the driver rather than in the front end. Reading
//! a directory is an effect; deciding what `parse` refers to is not, and the two are kept
//! on opposite sides of this boundary.
//!
//! # Determinism
//!
//! The root comes first, then its imports depth-first in source order, and each file is
//! read at most once keyed by its **canonical path** — so a file reached by two different
//! spellings is one file, and the map's contents depend on the import graph rather than on
//! the filesystem's iteration order. A cycle terminates because the second visit finds the
//! file already loaded; it is not an error, and `docs/spec.md` says why.
//!
//! # Where the bytes come from
//!
//! Moved from `nazm-cli` in N16 so the command line and the language service share one
//! loader rather than each having one. The only thing that differs between them is where
//! a file's **bytes** come from, so that is the only thing that is a parameter: a
//! [`Sources`]. [`Disk`] reads the filesystem, which is what every command does; the
//! language service passes one that answers from an open editor buffer first. Which files
//! a compilation has, how a path becomes an identity, cycles, the prelude and every
//! diagnostic are decided here, once, whichever it is.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use nazm_core::sema::{ModuleGraph, ModuleKey};
use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, SourceMap, Span};

/// The durable identity of the module at `path`, relative to `root`.
///
/// Lexical, deliberately: `ModuleKey` is a position in a project, and asking the
/// filesystem here would make the answer depend on where the project was checked out.
/// `None` when the module sits outside the root, which is not an error — see
/// `docs/spec.md`, *Durable identity*.
///
/// Both sides are made absolute first, so the answer does not depend on the shell's
/// working directory: `nazm check lib/util.nz` and `nazm check ../p/lib/util.nz` are the
/// same module of the same project. Neither absolute path survives into the key — only
/// what is left after one is removed from the other.
fn module_key(root: &Path, path: &Path) -> Option<ModuleKey> {
    let absolute = crate::root::absolute(path);
    let relative = absolute.strip_prefix(root).unwrap_or(&absolute);
    // One spelling on every platform. `Path` components are already `.`-free and
    // separator-normalised; `ModuleKey::relative` folds what remains and refuses an
    // escape.
    let mut text = String::new();
    for part in relative.components() {
        use std::path::Component;
        let piece = match part {
            Component::Normal(s) => s.to_str()?,
            Component::CurDir => continue,
            Component::ParentDir => "..",
            // An absolute path, a drive letter or a UNC prefix means this module is not
            // under the root at all, whatever the rest of it says.
            Component::RootDir | Component::Prefix(_) => return None,
        };
        if !text.is_empty() {
            text.push('/');
        }
        text.push_str(piece);
    }
    ModuleKey::relative(&text)
}

/// Where a compilation's source bytes come from.
///
/// Two questions and nothing else. Identity — which two paths are one module — is not one
/// of them: the loader decides it from the path, the same way whatever answers these.
pub trait Sources {
    /// The exact text of the file at `path`.
    ///
    /// # Errors
    ///
    /// Whatever made it unreadable; the loader reports it as a missing module.
    fn read(&self, path: &Path) -> io::Result<String>;

    /// Whether an import naming `path` names a file that exists.
    fn exists(&self, path: &Path) -> bool;
}

/// The filesystem, as every command reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Disk;

impl Sources for Disk {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// The package `name` as the package holding `importer` declares it.
fn dependency<'g>(
    packages: Option<&'g nazm_package::Graph>,
    importer: &Path,
    name: &str,
) -> Result<&'g nazm_package::Package, Box<Diagnostic>> {
    let Some(g) = packages else {
        return Err(Box::new(
            Diagnostic::error(
                codes::UNKNOWN_DEPENDENCY,
                Span::synthetic(),
                format!("`{name}:` names a package, and this compilation is not a package's"),
            )
            .help("a package is a directory holding `nazm.toml`; build it by naming its directory"),
        ));
    };
    let from = g.package_of(importer).and_then(|i| g.packages.get(i));
    let found = from
        .and_then(|p| p.dependencies.get(name))
        .and_then(|&i| g.packages.get(i));
    found.ok_or_else(|| {
        let declared: Vec<String> = from
            .map(|p| p.dependencies.keys().map(|d| format!("`{d}`")).collect())
            .unwrap_or_default();
        Box::new(
            Diagnostic::error(
                codes::UNKNOWN_DEPENDENCY,
                Span::synthetic(),
                format!(
                    "`{name}` is not a dependency of `{}`",
                    from.map_or("this package", |p| p.name.as_str())
                ),
            )
            .help(if declared.is_empty() {
                "it declares no dependencies; add one under `[dependencies]` in `nazm.toml`"
                    .to_owned()
            } else {
                format!("it declares {}", declared.join(", "))
            }),
        )
    })
}

/// One thing the loader has still to read: a file, or a standard module and its source.
#[derive(Debug, Clone)]
enum Item {
    File(PathBuf),
    Std(&'static str, &'static str),
}

impl Item {
    /// The identity a second import of it is recognised by.
    fn canonical(self) -> PathBuf {
        match self {
            Self::File(c) => c.canonicalize().unwrap_or(c),
            Self::Std(name, _) => PathBuf::from(nazm_core::sema::stdlib::file_name(name)),
        }
    }
}

/// The source of a compilation, and how its modules relate.
///
/// Two values that are always built together and always travel together: the text, for
/// diagnostics and slicing, and the graph, for everything semantic.
#[derive(Debug)]
pub struct Loaded {
    pub map: SourceMap,
    pub graph: ModuleGraph,
}

/// Read `root` and everything it imports.
///
/// # Errors
///
/// A file that cannot be read, or an import that cannot be found. Parse errors are not
/// reported here: each file is parsed again by the caller, which reports them all at once
/// against the positions they will keep.
/// On failure what was loaded **so far** comes back with the diagnostics, so a
/// "cannot find" points at the `use` line that wrote it rather than at an empty file.
pub fn load(root: &Path, source_root: &Path) -> Result<Loaded, (Loaded, Vec<Diagnostic>)> {
    load_from(root, source_root, &Disk)
}

/// [`load`], reading every file through `sources`.
///
/// # Errors
///
/// As [`load`].
pub fn load_from(
    root: &Path,
    source_root: &Path,
    sources: &dyn Sources,
) -> Result<Loaded, (Loaded, Vec<Diagnostic>)> {
    // A package build (N46): the source root is the one a manifest beside it declares.
    let packages = match package_context(source_root) {
        None => None,
        Some(Ok(g)) => Some(g),
        Some(Err(d)) => {
            let loaded = Loaded {
                map: SourceMap::from_parts(Vec::new()),
                graph: ModuleGraph::default(),
            };
            return Err((loaded, d));
        }
    };
    load_packaged(root, source_root, sources, packages.as_ref())
}

/// The package whose declared source root `source_root` is, resolved with its dependencies —
/// or `None` for a compilation that is not a package's.
#[must_use]
pub fn package_context(source_root: &Path) -> Option<Result<nazm_package::Graph, Vec<Diagnostic>>> {
    let dir = source_root.parent()?;
    if !nazm_package::is_package(dir) {
        return None;
    }
    match nazm_package::resolve(dir) {
        Err(d) => Some(Err(d)),
        Ok(g) => {
            let ours = source_root.canonicalize().ok();
            let root = g.root_package().map(|p| p.source_root.clone());
            (ours.is_some() && ours == root).then_some(Ok(g))
        }
    }
}

/// [`load_from`], with the package graph the compilation belongs to, if any.
///
/// # Errors
///
/// As [`load`], and an import that names a package the importing package does not declare
/// (`N0508`) or a relative import that leaves its package's source root.
pub fn load_packaged(
    root: &Path,
    source_root: &Path,
    sources: &dyn Sources,
    packages: Option<&nazm_package::Graph>,
) -> Result<Loaded, (Loaded, Vec<Diagnostic>)> {
    // Every module's durable identity is its path relative to this, so a checkout moved
    // from one directory to another keeps its identities — which is the property
    // `docs/spec.md` asks for and the reason the absolute path never reaches one. Whether
    // the root was declared or derived is `crate::root`'s decision, made before any file
    // was read.
    let root_path = source_root;

    let mut parts: Vec<(String, String)> = Vec::new();
    // Per module, in load order: its durable identity, or `None`.
    let mut keys: Vec<Option<ModuleKey>> = Vec::new();
    // Canonical path to the position the file took in load order. Canonical, so a file
    // reached by two spellings is one module with one identity.
    let mut index: HashMap<PathBuf, usize> = HashMap::new();
    let mut visited: HashMap<PathBuf, ()> = HashMap::new();
    // Parallel to `parts`: for each file, the imports it named, as `(canonical path,
    // where it was written)`. Resolved to ids once every file has one.
    let mut named: Vec<Vec<Named>> = Vec::new();
    let mut errors: Vec<Diagnostic> = Vec::new();

    // An explicit stack rather than recursion: an import chain has no bound, and the
    // compiler's own rule is that a recursive walk needs a bound or a controlled stack.
    let mut stack: Vec<Item> = vec![Item::File(root.to_path_buf())];

    while let Some(item) = stack.pop() {
        // A standard module (N45) is the toolchain's: its bytes are the compiler's copy, its
        // key is `@std/name`, and its "path" is that file name, which no canonical path on
        // disk can equal — a canonical path is absolute.
        let (path, canonical, logical, standard) = match item {
            Item::File(path) => {
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                // A dependency package's module is keyed by its package and its path in that
                // package's source root; the root package's, as any project's.
                let logical = match packages.and_then(|g| {
                    let i = g.package_of(&path)?;
                    (i != g.root).then(|| g.packages.get(i)).flatten()
                }) {
                    Some(p) => canonical
                        .strip_prefix(&p.source_root)
                        .ok()
                        .and_then(|rel| rel.to_str())
                        .and_then(|rel| {
                            ModuleKey::in_package(&p.key_prefix(), &rel.replace('\\', "/"))
                        }),
                    None => module_key(root_path, &path),
                };
                (path, canonical, logical, None)
            }
            Item::Std(name, source) => {
                let file = PathBuf::from(nazm_core::sema::stdlib::file_name(name));
                (
                    file.clone(),
                    file,
                    Some(nazm_core::sema::stdlib::key(name)),
                    Some(source),
                )
            }
        };
        if let Some(&already) = index.get(&canonical) {
            // One file, reached a second time. Ordinarily that is a diamond and the two
            // spellings normalise to one key. When they do not, a symbolic link has made
            // one file answer to two logical names — which the compiler written in Nazm
            // would see as *two modules*, because it normalises text rather than asking
            // the filesystem. The two implementations would compute different durable
            // identities for the same source, so neither gets one.
            if keys.get(already).is_some_and(|k| *k != logical) {
                if let Some(k) = keys.get_mut(already) {
                    *k = None;
                }
            }
            continue;
        }
        if visited.insert(canonical.clone(), ()).is_some() {
            continue;
        }

        let read = match standard {
            Some(source) => Ok(source.to_owned()),
            None => sources.read(&path),
        };
        let text = match read {
            // A project file's derives, expanded (R1c, L2): the generated functions are part of
            // the module from here on, its imports included.
            Ok(t) if standard.is_none() => nazm_core::derive::expand(&t),
            Ok(t) => t,
            Err(e) => {
                // No text to point into: this file could not be read. It gets an empty entry
                // of its own, so the report names it — not whichever file has id 0, which for
                // an unreadable root was the prelude, loaded last and so first (v1, Q1-D-09).
                // It is in the graph with no identity and no imports; the load fails anyway.
                let id = FileId::from_index(parts.len());
                index.insert(canonical, parts.len());
                keys.push(None);
                named.push(Vec::new());
                parts.push((path.display().to_string(), String::new()));
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        Span::new(id, 0, 0),
                        format!("cannot read `{}`", path.display()),
                    )
                    .help(format!("{e}")),
                );
                continue;
            }
        };

        // The id this file is about to get. Known before the map exists, because it is
        // the file's position in load order — which is what a `FileId` is.
        let id = FileId::from_index(parts.len());

        // Parsed for its imports only. Parse errors are ignored here and reported by the
        // caller, which parses every file again with the same ids, so a syntax error is
        // reported once at the position it will still have.
        let (program, _) = nazm_core::__syntax::parse_file(id, &text);
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let mut children = Vec::new();
        for import in &program.imports {
            // A path beginning with `@std/` is the standard library's and is never looked for
            // on disk.
            if nazm_core::sema::stdlib::reserved(&import.path) {
                if let Some((name, source)) = nazm_core::sema::stdlib::module(&import.path) {
                    children.push((
                        Item::Std(name, source),
                        import.path_span,
                        alias_of(import),
                        reexport_of(import),
                    ));
                } else {
                    let known: Vec<String> = nazm_core::sema::stdlib::MODULES
                        .iter()
                        .map(|(n, _)| format!("`{}{n}`", nazm_core::sema::stdlib::PREFIX))
                        .collect();
                    errors.push(
                        Diagnostic::error(
                            codes::MODULE_NOT_FOUND,
                            import.path_span,
                            format!("there is no standard module `{}`", import.path),
                        )
                        .help(format!(
                            "a path beginning with `@std/` names a module the toolchain \
                             provides; the standard modules are {}",
                            known.join(", ")
                        )),
                    );
                }
                continue;
            }
            // A standard module imports only other standard modules: it has no directory.
            if standard.is_some() {
                errors.push(Diagnostic::error(
                    codes::MODULE_NOT_FOUND,
                    import.path_span,
                    format!("a standard module cannot import `{}`", import.path),
                ));
                continue;
            }
            // `NAME:path` (N46): a module of a package the importing package depends on.
            let (target, package) = match import.path.split_once(':') {
                Some((name, rest)) => match dependency(packages, &canonical, name) {
                    Ok(dep) => (dep.source_root.join(rest), Some(dep.source_root.clone())),
                    Err(mut d) => {
                        // Aimed at the import that named the package.
                        d.primary.span = import.path_span;
                        errors.push(*d);
                        continue;
                    }
                },
                None => (
                    dir.join(&import.path),
                    packages
                        .and_then(|g| g.package_of(&canonical).and_then(|i| g.packages.get(i)))
                        .map(|p| p.source_root.clone()),
                ),
            };
            // Inside a package, an import stays inside the source root it resolves against.
            if let Some(root) = &package {
                let inside = target.canonicalize().map_or(true, |t| t.starts_with(root));
                if !inside {
                    errors.push(
                        Diagnostic::error(
                            codes::MODULE_NOT_FOUND,
                            import.path_span,
                            format!("`{}` leaves its package's source root", import.path),
                        )
                        .help("a package reaches another package's modules as `NAME:path`"),
                    );
                    continue;
                }
            }
            if sources.exists(&target) {
                children.push((
                    Item::File(target),
                    import.path_span,
                    alias_of(import),
                    reexport_of(import),
                ));
            } else {
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        // Already names this file: no base to add.
                        import.path_span,
                        format!("cannot find `{}`", import.path),
                    )
                    .help(format!(
                        "looked for `{}`, relative to the importing file",
                        target.display()
                    )),
                );
            }
        }

        // Pushed in reverse so the stack pops them in source order: the map's order
        // follows the program's text, which is what makes it reproducible.
        for (child, _, _, _) in children.iter().rev() {
            stack.push(child.clone());
        }

        index.insert(canonical, parts.len());
        keys.push(logical);
        named.push(
            children
                .into_iter()
                .map(|(c, at, alias, re)| (c.canonical(), at, alias, re))
                .collect(),
        );
        parts.push((path.display().to_string(), text));
    }

    // The core prelude, last: it is part of every compilation, and putting it after every
    // project module means no project file's id — and no span in one — depends on it.
    // `docs/spec.md`, *The core prelude*.
    let core = parts.len();
    parts.push((
        nazm_core::sema::prelude::FILE_NAME.to_owned(),
        nazm_core::sema::prelude::SOURCE.to_owned(),
    ));

    // Ids exist now, so the edges can be. A named file that is not in `index` failed to
    // load and already has a diagnostic; dropping the edge here would otherwise turn one
    // error into two.
    let mut graph = ModuleGraph::default();
    for i in 0..core {
        graph.add(FileId::from_index(i), keys.get(i).cloned().flatten());
    }
    for (i, imports) in named.iter().enumerate() {
        let Some(from) = graph.module_of(FileId::from_index(i)) else {
            continue;
        };
        for (target, at, alias, re) in imports {
            let Some(&j) = index.get(target) else {
                continue;
            };
            if let Some(to) = graph.module_of(FileId::from_index(j)) {
                graph.import_as(from, to, *at, alias.clone());
                if let Some(spec) = re {
                    graph.reexport(from, to, spec.clone());
                }
            }
        }
    }

    graph.attach_prelude(FileId::from_index(core));

    let loaded = Loaded {
        map: SourceMap::from_parts(parts),
        graph,
    };
    if errors.is_empty() {
        Ok(loaded)
    } else {
        Err((loaded, errors))
    }
}

/// One import a file wrote, resolved to a path: the canonical path, where the quoted path is
/// written, and the name it gives the module, if any (N49).
type Named = (
    PathBuf,
    Span,
    Option<(String, Span)>,
    Option<nazm_core::sema::ReexportSpec>,
);

/// The name an import gives its module, `as NAME` (N49), as the graph records it.
fn alias_of(import: &nazm_core::__syntax::Import) -> Option<(String, Span)> {
    import.alias.as_ref().map(|q| (q.name.clone(), q.span))
}

/// What a `pub use` re-exports (N103), as the graph records it.
fn reexport_of(import: &nazm_core::__syntax::Import) -> Option<nazm_core::sema::ReexportSpec> {
    import
        .reexport
        .as_ref()
        .map(|r| nazm_core::sema::ReexportSpec {
            at: r.pub_span,
            names: r.names.as_ref().map(|list| {
                list.iter()
                    .map(|n| nazm_core::sema::ReexportItem {
                        name: n.name.clone(),
                        span: n.span,
                        alias: n.alias.clone(),
                    })
                    .collect()
            }),
        })
}
