//! `nazm diagnostics` — the root's current diagnostics as a compact index,
//! `nazm.diagnostic-index/1`, or one of them in full, `nazm.diagnostic-detail/1`. N30.
//!
//! The root is analysed from disk exactly as `nazm check` would load it — as a module, as the
//! language server analyses it — the answer is derived and everything is dropped. `nazm
//! check --json` and its `nazm.diagnostic/1` stream are untouched by this command.
//! `docs/architecture.md` §7.32 says what an index and a detail hold.

use std::path::Path;
use std::process::ExitCode;

use nazm_service::{DetailResult, IndexResult, LanguageService};

/// Print the index, or with `detail` — an id and the index state it is of — that one
/// diagnostic. Exit 0 with an answer, 1 with a reason there is none, 2 when the root cannot
/// be read.
pub(crate) fn run(
    root: &Path,
    detail: Option<&(String, String)>,
    json: bool,
    source_root: Option<&Path>,
) -> ExitCode {
    let root = root.to_path_buf();
    let source_root = source_root.map(Path::to_path_buf);
    let detail = detail.cloned();
    let answered = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut service = LanguageService::with_source_root(source_root);
                service
                    .open_from_disk(&root)
                    .map_err(|e| format!("{}: {e}", root.display()))?;
                Ok::<_, String>(match detail {
                    None => {
                        let r = service.diagnostic_index(&root, 0);
                        let ok = matches!(r, IndexResult::Available(_));
                        (serde_json::to_value(&r), r.to_json(), ok)
                    }
                    Some((id, state)) => {
                        let r = service.diagnostic_detail(&root, 0, &id, &state);
                        let ok = matches!(r, DetailResult::Available(_));
                        (serde_json::to_value(&r), r.to_json(), ok)
                    }
                })
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the analysis stopped unexpectedly".to_owned())?
    });
    let (value, compact, ok) = match answered {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm diagnostics: {e}");
            return ExitCode::from(crate::exit::TOOLCHAIN_FAILED);
        }
    };
    let text = if json {
        compact
    } else {
        value
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or(compact)
    };
    println!("{text}");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
