//! `nazm patch rename` and `nazm patch fix` — a semantic patch plan, `nazm.patch/1`. N29.
//!
//! The root is analysed from disk exactly as `nazm check` would load it, the plan is derived
//! from that analysis, and both are dropped. This command writes nothing: a patch is a
//! proposal bound to exact source state, and applying one is not something it does.
//! `docs/architecture.md` §7.31 says what a patch holds. `nazm patch apply` (N89, §7.90) is the
//! one thing that writes one: all or nothing, only to files whose bytes are the plan's.

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
            return ExitCode::from(crate::exit::TOOLCHAIN_FAILED);
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

/// Apply the plan at `plan` to the files under `base` (N89, §7.90): every file's bytes must hash
/// to its `digest` and hold each edit's `expected` bytes, or nothing is written and the first
/// differing file is named. Exit 0 when written, 1 when refused, 2 when the plan is unreadable.
pub(crate) fn apply(plan: &Path, base: &Path) -> ExitCode {
    let text = match std::fs::read_to_string(plan) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm patch apply: cannot read {}: {e}", plan.display());
            return ExitCode::from(crate::exit::NO_SUCH_FILE);
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("nazm patch apply: {} is not JSON: {e}", plan.display());
            return ExitCode::from(crate::exit::CHECK_FAILED);
        }
    };
    if v.get("schema").and_then(serde_json::Value::as_str) != Some("nazm.patch/1")
        || v.get("status").and_then(serde_json::Value::as_str) != Some("available")
    {
        eprintln!("nazm patch apply: not an available `nazm.patch/1` plan");
        return ExitCode::from(crate::exit::CHECK_FAILED);
    }
    let files = v
        .get("files")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Everything is checked and computed before anything is written.
    let mut writes: Vec<(std::path::PathBuf, String)> = Vec::with_capacity(files.len());
    for f in &files {
        let rel = f
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if rel.is_empty()
            || Path::new(rel)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            eprintln!("nazm patch apply: refused: `{rel}` is not a path inside the base");
            return ExitCode::FAILURE;
        }
        let path = base.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("nazm patch apply: refused: `{rel}` cannot be read");
            return ExitCode::FAILURE;
        };
        let want = f
            .get("digest")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if nazm_service::digest(&bytes) != want {
            eprintln!(
                "nazm patch apply: refused: `{rel}` is not the file the plan was made from; nothing was written"
            );
            return ExitCode::FAILURE;
        }
        let Ok(mut out) = String::from_utf8(bytes) else {
            eprintln!("nazm patch apply: refused: `{rel}` is not UTF-8");
            return ExitCode::FAILURE;
        };
        let mut edits: Vec<(usize, usize, String, String)> = Vec::new();
        for e in f
            .get("edits")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let num = |k: &str| {
                e.get(k)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
            };
            let txt = |k: &str| {
                e.get(k)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            };
            let (Some(start), Some(end), Some(expected), Some(replacement)) = (
                num("start"),
                num("end"),
                txt("expected"),
                txt("replacement"),
            ) else {
                eprintln!("nazm patch apply: refused: an edit of `{rel}` is malformed");
                return ExitCode::FAILURE;
            };
            if out.get(start..end) != Some(expected.as_str()) {
                eprintln!(
                    "nazm patch apply: refused: `{rel}` does not hold an edit's expected bytes"
                );
                return ExitCode::FAILURE;
            }
            edits.push((start, end, expected, replacement));
        }
        // Back to front, so each edit's offsets are still the planned ones.
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (start, end, _, replacement) in edits {
            out.replace_range(start..end, &replacement);
        }
        writes.push((path, out));
    }
    for (path, text) in &writes {
        let tmp = path.with_extension("nz.nazm-apply");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path)) {
            let _ = std::fs::remove_file(&tmp);
            eprintln!("nazm patch apply: writing {} failed: {e}", path.display());
            return ExitCode::from(crate::exit::TOOLCHAIN_FAILED);
        }
    }
    println!(
        "nazm patch apply: {} file{} written",
        writes.len(),
        if writes.len() == 1 { "" } else { "s" }
    );
    ExitCode::SUCCESS
}
