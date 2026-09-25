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
    let mut named: Vec<Vec<(PathBuf, Span)>> = Vec::new();
    let mut errors: Vec<Diagnostic> = Vec::new();

    // An explicit stack rather than recursion: an import chain has no bound, and the
    // compiler's own rule is that a recursive walk needs a bound or a controlled stack.
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let logical = module_key(root_path, &path);
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

        let text = match sources.read(&path) {
            Ok(t) => t,
            Err(e) => {
                // No file to point into: this one could not be read. Said plainly rather
                // than aimed at an arbitrary byte of whatever had been loaded so far,
                // which is what the merged-buffer version did.
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        Span::synthetic(),
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
            let target = dir.join(&import.path);
            if sources.exists(&target) {
                children.push((target, import.path_span));
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
        for (child, _) in children.iter().rev() {
            stack.push(child.clone());
        }

        index.insert(canonical, parts.len());
        keys.push(logical);
        named.push(
            children
                .into_iter()
                .map(|(c, at)| (c.canonicalize().unwrap_or(c), at))
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
        for (target, at) in imports {
            let Some(&j) = index.get(target) else {
                continue;
            };
            if let Some(to) = graph.module_of(FileId::from_index(j)) {
                graph.import(from, to, *at);
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
