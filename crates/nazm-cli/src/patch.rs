//! `nazm patch rename` and `nazm patch fix` — a semantic patch plan, `nazm.patch/1`. N29.
//!
//! The root is analysed from disk exactly as `nazm check` would load it, the plan is derived
//! from that analysis, and both are dropped. This command writes nothing: a patch is a
//! proposal bound to exact source state, and applying one is not something it does.
//! `docs/architecture.md` §7.31 says what a patch holds.

use std::path::Path;
use std::process::ExitCode;

use nazm_service::{LanguageService, PatchResult};

/// Which plan to ask for.
pub(crate) enum Ask {
    Rename(String),
    Fix(usize),
}

/// Print the plan for byte `byte` of `file` in the compilation rooted at `root`: compact
/// canonical JSON with `json`, the same object indented without it. Exit 0 with a patch, 1
/// with a reason there is none, 2 when the root cannot be read.
pub(crate) fn run(
    root: &Path,
    file: &Path,
    byte: u32,
    ask: &Ask,
    json: bool,
    source_root: Option<&Path>,
) -> ExitCode {
    let (root, file) = (root.to_path_buf(), file.to_path_buf());
    let source_root = source_root.map(Path::to_path_buf);
    let answered = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut service = LanguageService::with_source_root(source_root);
                service
                    .open_from_disk(&root)
                    .map_err(|e| format!("{}: {e}", root.display()))?;
                Ok::<_, String>(match ask {
                    Ask::Rename(name) => service.patch_rename(&root, 0, &file, byte, name),
                    Ask::Fix(index) => service.patch_fix(&root, 0, &file, byte, *index),
                })
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the analysis stopped unexpectedly".to_owned())?
    });
    let result = match answered {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm patch: {e}");
            return ExitCode::from(2);
        }
    };
    let text = if json {
        result.to_json()
    } else {
        serde_json::to_value(&result)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| result.to_json())
    };
    println!("{text}");
    match result {
        PatchResult::Available(_) => ExitCode::SUCCESS,
        PatchResult::Unavailable(_) => ExitCode::FAILURE,
    }
}
