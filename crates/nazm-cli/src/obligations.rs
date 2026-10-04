//! `nazm obligations FILE` — the obligation census, `nazm.obligations/1` (N87, §7.88).
//!
//! Every contract clause, every call of a function stating a precondition, every operation that
//! can overflow or divide by zero, every index and allocation, every call through a function
//! value and every call into C, each with its status: proved (and why), checked as the program
//! runs, or unknown. Compiler evidence for a reviewer, read off the Core IR every execution path
//! starts from; not certification.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};

use crate::{exit, load};

pub(crate) fn run(path: &Path, as_json: bool, declared: Option<&Path>) -> ExitCode {
    let source_root = match crate::root::decide(path, declared) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let loaded = match load::load(path, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let found = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| Ok(nazm_core::obligations::census(core, res)),
    );
    let obligations = match found {
        Ok(o) => o,
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, &loaded.map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    let count =
        |s: nazm_core::obligations::Status| obligations.iter().filter(|o| o.status == s).count();
    let rows: Vec<Value> = obligations
        .iter()
        .map(|o| {
            let lc = loaded.map.line_col(o.at);
            let mut row = json!({
                "function": o.function,
                "kind": o.kind.name(),
                "status": o.status.name(),
                "file": loaded.map.name(o.at.file),
                "line": lc.line,
                "column": lc.column,
            });
            if let (Some(w), Some(m)) = (&o.witness, row.as_object_mut()) {
                m.insert("witness".to_owned(), json!(w));
            }
            row
        })
        .collect();
    use nazm_core::obligations::Status;
    let (proved, checked, unknown) = (
        count(Status::Proved),
        count(Status::Checked),
        count(Status::Unknown),
    );
    let field = |r: &Value, k: &str| r.get(k).cloned().unwrap_or(Value::Null);
    let text = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{}:{}:{} {} {} {}{}",
                text(r, "file"),
                field(r, "line"),
                field(r, "column"),
                text(r, "function"),
                text(r, "kind"),
                text(r, "status"),
                r.get("witness")
                    .and_then(Value::as_str)
                    .map_or_else(String::new, |w| format!(" ({w})"))
            )
        })
        .collect();
    let doc = json!({
        "schema": "nazm.obligations/1",
        "evidence": "compiler evidence, not certification",
        "proved": proved,
        "checked": checked,
        "unknown": unknown,
        "obligations": rows,
    });
    if as_json {
        println!("{doc}");
    } else {
        for l in &lines {
            println!("{l}");
        }
        println!(
            "{proved} proved, {checked} checked, {unknown} unknown — compiler evidence, not certification"
        );
    }
    ExitCode::from(exit::OK)
}
