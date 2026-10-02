//! Packages on the command line (N46, `docs/architecture.md` §7.48).
//!
//! `nazm check|build|run DIR` builds the package in `DIR` from its `main`; `--locked` holds any
//! of them to the package's lockfile before anything is compiled; `nazm lock DIR` writes that
//! lockfile. Resolution, digests and the lockfile's bytes are `nazm-package`'s.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use crate::exit;

/// A package diagnostic is about a manifest, a directory or the lockfile — never a position
/// in a module — so it is written without a source excerpt.
fn report(diagnostics: &[Diagnostic]) -> ExitCode {
    for d in diagnostics {
        eprintln!("error[{}]: {}", d.code, d.message);
        if let Some(h) = &d.help {
            eprintln!("  = help: {h}");
        }
    }
    let n = diagnostics.len();
    eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
    ExitCode::from(exit::CHECK_FAILED)
}

/// The package a command's file argument names or lies in, resolved.
pub(crate) fn graph_for(path: &Path) -> Option<Result<nazm_package::Graph, Vec<Diagnostic>>> {
    if path.is_dir() {
        return Some(nazm_package::resolve(path));
    }
    let root = nazm_service::root::decide(path, None).ok()?;
    if root.how() != nazm_service::root::How::Package {
        return None;
    }
    nazm_service::load::package_context(root.path())
}

/// The file a command should start from: the `main` of the package `path` names, when it is a
/// package's directory; `None` to use `path` as it is.
///
/// # Errors
///
/// The exit code, once every diagnostic has been reported: a directory that is not a package,
/// a resolution that fails, a package with no `main`, or — with `locked` — a lockfile that is
/// missing or does not match.
pub(crate) fn entry(path: &Path, locked: bool) -> Result<Option<PathBuf>, ExitCode> {
    if path.is_dir() && !nazm_package::is_package(path) {
        eprintln!(
            "nazm: `{}` is a directory with no `{}`, so it is not a package",
            path.display(),
            nazm_package::MANIFEST
        );
        return Err(ExitCode::from(exit::NO_SUCH_FILE));
    }
    let Some(resolved) = graph_for(path) else {
        if locked {
            eprintln!(
                "nazm: `--locked` holds a package build to its lockfile, and `{}` is not in a \
                 package",
                path.display()
            );
            return Err(ExitCode::from(exit::NO_SUCH_FILE));
        }
        return Ok(None);
    };
    let graph = resolved.map_err(|d| report(&d))?;
    if locked {
        nazm_package::verify_locked(&graph).map_err(|d| report(&d))?;
    }
    if !path.is_dir() {
        return Ok(None);
    }
    let Some(root) = graph.root_package() else {
        return Ok(None);
    };
    match &root.main {
        Some(main) => Ok(Some(main.clone())),
        None => Err(report(&[Diagnostic::error(
            codes::NO_ENTRY,
            Span::synthetic(),
            format!("the package `{}` has no `main` to build or run", root.name),
        )
        .help(
            "`main = \"main.nz\"` under `[package]` names the entry module, relative to the root",
        )])),
    }
}

/// `nazm publish DIR --registry REG` (N56).
pub(crate) fn publish(dir: &Path, registry: &Path) -> ExitCode {
    match nazm_package::publish(dir, registry) {
        Ok((version, digest)) => {
            eprintln!(
                "nazm: published {version} into {} ({digest})",
                registry.display()
            );
            ExitCode::SUCCESS
        }
        Err(d) => report(&[*d]),
    }
}

/// `nazm yank NAME@VERSION --registry REG [--undo]` (N56).
pub(crate) fn yank(which: &str, registry: &Path, undo: bool) -> ExitCode {
    let parsed = which
        .split_once('@')
        .and_then(|(n, v)| nazm_package::Version::parse(v).map(|v| (n, v)));
    let Some((name, version)) = parsed else {
        eprintln!("nazm: `{which}` is not NAME@VERSION");
        return ExitCode::from(exit::CHECK_FAILED);
    };
    match nazm_package::yank(registry, name, version, !undo) {
        Ok(()) => {
            eprintln!(
                "nazm: {} {name} {version}",
                if undo { "restored" } else { "yanked" }
            );
            ExitCode::SUCCESS
        }
        Err(d) => report(&[*d]),
    }
}

/// `nazm lock DIR`.
pub(crate) fn lock(dir: &Path) -> ExitCode {
    let graph = match nazm_package::resolve(dir) {
        Ok(g) => g,
        Err(d) => return report(&d),
    };
    match nazm_package::write_lock(&graph) {
        Ok(true) => {
            eprintln!(
                "nazm: wrote {} ({} package{})",
                dir.join(nazm_package::LOCKFILE).display(),
                graph.packages.len(),
                if graph.packages.len() == 1 { "" } else { "s" }
            );
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!(
                "nazm: {} is up to date",
                dir.join(nazm_package::LOCKFILE).display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nazm: could not write the lockfile: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}
