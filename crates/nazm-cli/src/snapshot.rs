//! `nazm snapshot` and `nazm delta` — a root compilation's semantic snapshot,
//! `nazm.snapshot/3`, and what changed since one, `nazm.delta/3`. N28.
//!
//! The root is analysed from disk exactly as `nazm check` would load it, the snapshot or
//! delta is derived from that analysis, and both are dropped: nothing is kept between runs.
//! A baseline is a file the caller names and keeps; this command reads it, and writes
//! nothing. `docs/internal/architecture.md` §7.30 says what the digests are taken over.

use std::path::Path;
use std::process::ExitCode;

use nazm_service::{DeltaResult, LanguageService, NoDelta, SnapshotResult};
use serde::Serialize;

/// Print the snapshot of the compilation rooted at `root`. Exit 0 with a snapshot, 1 with
/// a reason there is none, 2 when the root cannot be read.
pub(crate) fn snapshot(root: &Path, json: bool, source_root: Option<&Path>) -> ExitCode {
    let answered = analysed(root, source_root, |service, root| {
        service.semantic_snapshot(root, 0)
    });
    match answered {
        Ok(r) => {
            print(&r, json, &r.to_json());
            match r {
                SnapshotResult::Available(_) => ExitCode::SUCCESS,
                SnapshotResult::Unavailable(_) => ExitCode::FAILURE,
            }
        }
        Err(e) => {
            eprintln!("nazm snapshot: {e}");
            ExitCode::from(crate::exit::TOOLCHAIN_FAILED)
        }
    }
}

/// Print what changed between the snapshot in `baseline` and the compilation rooted at
/// `root`. Exit 0 with a delta, 1 with a reason there is none — a baseline that is not JSON
/// is a malformed one — and 2 when the root or the baseline cannot be read.
pub(crate) fn delta(
    root: &Path,
    baseline: &Path,
    json: bool,
    source_root: Option<&Path>,
) -> ExitCode {
    let text = match std::fs::read_to_string(baseline) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm delta: {}: {e}", baseline.display());
            return ExitCode::from(crate::exit::NO_SUCH_FILE);
        }
    };
    let answered = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => analysed(root, source_root, move |service, root| {
            service.semantic_delta(root, 0, &value)
        }),
        Err(_) => Ok(NoDelta::BaselineMalformed.result()),
    };
    match answered {
        Ok(r) => {
            print(&r, json, &r.to_json());
            match r {
                DeltaResult::Available(_) => ExitCode::SUCCESS,
                DeltaResult::Unavailable(_) => ExitCode::FAILURE,
            }
        }
        Err(e) => {
            eprintln!("nazm delta: {e}");
            ExitCode::from(crate::exit::TOOLCHAIN_FAILED)
        }
    }
}

/// Open `root` from disk in a fresh service, on the parser's stack, and ask it `f`.
fn analysed<T: Send>(
    root: &Path,
    source_root: Option<&Path>,
    f: impl FnOnce(&LanguageService, &Path) -> T + Send,
) -> Result<T, String> {
    let root = root.to_path_buf();
    let source_root = source_root.map(Path::to_path_buf);
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut service = LanguageService::with_source_root(source_root);
                service
                    .open_from_disk(&root)
                    .map_err(|e| format!("{}: {e}", root.display()))?;
                Ok::<_, String>(f(&service, &root))
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the analysis stopped unexpectedly".to_owned())?
    })
}

/// The canonical compact JSON with `json`, the same object indented without it.
fn print(value: &impl Serialize, json: bool, compact: &str) {
    let text = if json {
        compact.to_owned()
    } else {
        serde_json::to_value(value)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| compact.to_owned())
    };
    println!("{text}");
}
