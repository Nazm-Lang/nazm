//! `nazm explain-cost` and `nazm explain-flow` (N52, `docs/architecture.md` §7.54).
//!
//! Both print facts the compiler already settled, for a reader or — with `--json` — for a tool:
//! where each function allocates, retains, blocks, talks to a channel, calls C or starts a task,
//! read off MIR (`nazm.cost/1`); and every sink and declassification with the origins that reach
//! it, as provenance solved them (`nazm.flow/1`). A count is per call of its function, and is
//! `at-most-once` or `unknown` — never a number the compiler does not know. Paths are durable
//! module names, so the output does not depend on where the checkout is.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};

use crate::{exit, load};

fn loaded(path: &Path) -> Result<load::Loaded, ExitCode> {
    let source_root = crate::root::decide(path, None).map_err(|e| {
        eprintln!("nazm: {e}");
        ExitCode::from(exit::NO_SUCH_FILE)
    })?;
    load::load(path, source_root.path()).map_err(|(partial, diagnostics)| {
        crate::emit(&diagnostics, &partial.map, false);
        ExitCode::from(exit::NO_SUCH_FILE)
    })
}

fn failed(diagnostics: &[nazm_diag::Diagnostic], map: &nazm_span::SourceMap) -> ExitCode {
    let n = diagnostics.len();
    crate::emit(diagnostics, map, false);
    eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
    ExitCode::from(exit::CHECK_FAILED)
}

/// Where a span is, by durable module name, line and column.
fn place(names: &nazm_lir::SourceNames<'_>, at: nazm_span::Span) -> Value {
    if at == nazm_span::Span::synthetic() {
        return Value::Null;
    }
    let lc = names.line_col(at);
    json!({ "module": names.name(at.file), "line": lc.line, "column": lc.column })
}

/// A field of a JSON object, or `null`: what indexing a `Value` means, without the lint.
fn field<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&Value::Null)
}

fn shown(p: &Value) -> String {
    match (
        field(p, "module").as_str(),
        field(p, "line").as_u64(),
        field(p, "column").as_u64(),
    ) {
        (Some(m), Some(l), Some(c)) => format!("{m}:{l}:{c}"),
        _ => "(compiler-made)".to_owned(),
    }
}

pub(crate) fn cost(path: &Path, as_json: bool) -> ExitCode {
    let loaded = match loaded(path) {
        Ok(l) => l,
        Err(code) => return code,
    };
    let map = &loaded.map;
    let graph = &loaded.graph;
    let names = nazm_lir::SourceNames::of(map, graph);
    let built = nazm_core::on_lowered(map, graph, nazm_core::Entry::Absent, |core, res| {
        let program = crate::mir::lower(core, res, graph)?;
        let mut functions = Vec::new();
        let mut totals: std::collections::BTreeMap<&str, (u64, u64)> =
            std::collections::BTreeMap::new();
        for fc in nazm_mir::cost::of(&program) {
            let Some(f) = program.function(fc.func) else {
                continue;
            };
            let sites: Vec<Value> = fc
                .sites
                .iter()
                .map(|s| {
                    let t = totals.entry(s.kind.name()).or_default();
                    match s.count {
                        nazm_mir::cost::Count::AtMostOnce => t.0 += 1,
                        nazm_mir::cost::Count::Unknown => t.1 += 1,
                    }
                    json!({
                        "kind": s.kind.name(),
                        "op": s.op,
                        "at": place(&names, s.at),
                        "count": s.count.name(),
                        "on_failure": s.on_failure,
                    })
                })
                .collect();
            functions.push(json!({
                "identity": nazm_mir::text::identity(&f.identity),
                "sites": sites,
            }));
        }
        functions.sort_by_key(|v| field(v, "identity").as_str().map(str::to_owned));
        let totals: serde_json::Map<String, Value> = totals
            .into_iter()
            .map(|(k, (once, unknown))| {
                (
                    k.to_owned(),
                    json!({ "at-most-once": once, "unknown": unknown }),
                )
            })
            .collect();
        Ok((functions, totals))
    });
    let (functions, totals) = match built {
        Ok(x) => x,
        Err(d) => return failed(&d, map),
    };
    if as_json {
        println!(
            "{}",
            json!({ "schema": "nazm.cost/1", "functions": functions, "totals": totals })
        );
        return ExitCode::SUCCESS;
    }
    for f in &functions {
        println!("{}", field(f, "identity").as_str().unwrap_or("?"));
        let sites = field(f, "sites").as_array().cloned().unwrap_or_default();
        if sites.is_empty() {
            println!("  (no resource site)");
        }
        for s in sites {
            println!(
                "  {}  {} `{}`, {} per call{}",
                shown(field(&s, "at")),
                field(&s, "kind").as_str().unwrap_or("?"),
                field(&s, "op").as_str().unwrap_or("?"),
                field(&s, "count").as_str().unwrap_or("?"),
                if field(&s, "on_failure") == true {
                    ", on a failure path"
                } else {
                    ""
                },
            );
        }
    }
    ExitCode::SUCCESS
}

pub(crate) fn flow(path: &Path, as_json: bool) -> ExitCode {
    let loaded = match loaded(path) {
        Ok(l) => l,
        Err(code) => return code,
    };
    let map = &loaded.map;
    let graph = &loaded.graph;
    let names = nazm_lir::SourceNames::of(map, graph);
    let checked = nazm_core::on_checked_map(map, graph, |_, res| {
        let mut sinks: Vec<Value> = res
            .sinks()
            .iter()
            .map(|s| {
                json!({
                    "class": s.class.name(),
                    "function": res.function(s.function).map(|d| d.name.clone()),
                    "at": place(&names, s.at),
                    "origins": s.origins.names(),
                })
            })
            .collect();
        sinks.sort_by_key(|v| v.to_string());
        Ok(sinks)
    });
    let sinks = match checked {
        Ok(s) => s,
        Err(d) => return failed(&d, map),
    };
    if as_json {
        println!("{}", json!({ "schema": "nazm.flow/1", "sinks": sinks }));
        return ExitCode::SUCCESS;
    }
    if sinks.is_empty() {
        println!("(no sink)");
    }
    for s in &sinks {
        let origins: Vec<String> = field(s, "origins")
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| o.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let what = if field(s, "class") == "vouch" {
            "vouch, clearing"
        } else {
            "receives"
        };
        println!(
            "{}  {} in `{}`: {} {}",
            shown(field(s, "at")),
            field(s, "class").as_str().unwrap_or("?"),
            field(s, "function").as_str().unwrap_or("?"),
            what,
            if origins.is_empty() {
                "nothing restricted (no origin)".to_owned()
            } else {
                origins.join(", ")
            },
        );
    }
    ExitCode::SUCCESS
}
