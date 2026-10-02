//! `nazm contract FILE` (N69, `docs/architecture.md` §7.71): a contract's machine-readable facts
//! (`--interface`), or a deterministic run of transactions against it (`--txs FILE`) on the
//! interpreter, which owns the persistent state between them.
//!
//! Every run first holds the module to the `web3` profile. A transaction is one line:
//! `CALLER ENTRY ARG…`, every value an `Int`; it commits only if its entrypoint returns `Ok`, and —
//! where the contract declares `conserved` — only if that quantity is unchanged. Anything else
//! reverts, leaving the state as it was.

use std::path::Path;
use std::process::ExitCode;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use serde_json::{json, Value as Json};

use crate::exit;

fn put(record: &mut Json, key: &str, value: Json) {
    if let Some(o) = record.as_object_mut() {
        o.insert(key.to_owned(), value);
    }
}

fn shown(v: &nazm_core::Value) -> String {
    v.to_string()
}

fn fields_of(v: &nazm_core::Value, n: usize) -> Vec<String> {
    match v {
        nazm_core::Value::Record(r) => (0..n)
            .map(|i| {
                r.field(u32::try_from(i).unwrap_or(u32::MAX))
                    .map_or_else(String::new, shown)
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn run(file: &Path, interface: bool, txs: Option<&Path>) -> ExitCode {
    // The profile first, by the same path every command enforces one: a contract that is not
    // deterministic is not run at all.
    if let Err(code) = crate::profile::enforce(file, Some("web3"), false, false) {
        return code;
    }
    let source_root = match crate::root::decide(file, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let loaded = match crate::load::load(file, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let transactions: Vec<(usize, String)> = match txs {
        None => Vec::new(),
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => t
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .map(|(i, l)| (i + 1, l.to_owned()))
                .collect(),
            Err(e) => {
                eprintln!("nazm: cannot read `{}`: {e}", p.display());
                return ExitCode::from(exit::NO_SUCH_FILE);
            }
        },
    };
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    let outcome = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            let c = nazm_service::contract::of(core, res, root).map_err(|why| {
            vec![Diagnostic::error(
                codes::NOT_A_CONTRACT,
                Span::synthetic(),
                format!("this module is not a contract: {why}"),
            )
            .help("a contract is a `State` record, `fn init() -> State`, and `pub fn name(s: State, caller: Int, …: Int) -> Result[State, Int] ! {}` entrypoints")]
        })?;
            let state_def = res.records().get(c.state);
            let state_fields: Vec<Json> = state_def
                .map(|d| {
                    d.fields
                        .iter()
                        .map(|f| json!({ "name": f.name, "type": res.types().show(f.ty) }))
                        .collect()
                })
                .unwrap_or_default();
            let set = |s: &Option<std::collections::BTreeSet<String>>| match s {
                Some(s) => json!(s),
                None => json!("all"),
            };
            let entrypoints: Vec<Json> = c
            .entrypoints
            .iter()
            .map(|e| json!({ "name": e.name, "args": e.args, "reads": set(&e.reads), "writes": set(&e.writes) }))
            .collect();
            let facts = json!({
                "schema": "nazm.contract/1",
                "profile": "web3",
                "state": { "fields": state_fields },
                "entrypoints": entrypoints,
                "conserved": c.conserved,
                "external_calls": "none: an entrypoint calls only Nazm functions, so nothing can re-enter it",
            });
            if interface || txs.is_none() {
                return Ok((facts, None));
            }
            // The simulator.
            let id = |name: &str| {
                core.functions
                    .iter()
                    .find(|f| f.name == name && Some(f.module) == root)
                    .map(|f| (f.def, f.params))
            };
            let n = c.fields.len();
            let mut interpreter = nazm_core::Interpreter::new(core.clone());
            let limits = nazm_core::eval::Limits::default();
            interpreter.reset_limits(limits);
            let Some((init, _)) = id("init") else {
                return Err(Vec::new());
            };
            let mut state = interpreter
                .call(init, Vec::new())
                .map_err(|e| vec![*e.diagnostic])?;
            let conserved = c
                .conserved
                .then(|| id("conserved").map(|(f, _)| f))
                .flatten();
            let amount = |interp: &mut nazm_core::Interpreter, s: &nazm_core::Value| {
                conserved.and_then(|f| {
                    interp.reset_limits(limits);
                    interp.call(f, vec![s.clone()]).ok().map(|v| shown(&v))
                })
            };
            let mut runs = Vec::new();
            let mut broken = Vec::new();
            for (line, text) in &transactions {
                let words: Vec<&str> = text.split_whitespace().collect();
                let parsed: Option<(i64, &str, Vec<i64>)> = (|| {
                    let caller = words.first()?.parse().ok()?;
                    let entry = *words.get(1)?;
                    let args: Option<Vec<i64>> =
                        words.get(2..)?.iter().map(|w| w.parse().ok()).collect();
                    Some((caller, entry, args?))
                })();
                let Some((caller, entry, args)) = parsed else {
                    runs.push(json!({ "line": line, "outcome": "malformed", "text": text }));
                    continue;
                };
                let Some(e) = c.entrypoints.iter().find(|e| e.name == entry) else {
                    runs.push(
                        json!({ "line": line, "entry": entry, "outcome": "no-such-entrypoint" }),
                    );
                    continue;
                };
                if e.args.len() != args.len() {
                    runs.push(json!({ "line": line, "entry": entry, "outcome": "wrong-arity", "expected": e.args.len() }));
                    continue;
                }
                let Some((f, _)) = id(entry) else { continue };
                let before = amount(&mut interpreter, &state);
                interpreter.reset_limits(limits);
                let calls = interpreter.calls();
                let mut call = vec![state.clone(), nazm_core::Value::Int(caller)];
                call.extend(args.iter().map(|a| nazm_core::Value::Int(*a)));
                let result = interpreter.call(f, call);
                let turns = limits.max_iterations - interpreter.iterations_left();
                let mut record = json!({
                    "line": line,
                    "caller": caller,
                    "entry": entry,
                    "meter": { "calls": interpreter.calls() - calls, "loop_turns": turns },
                });
                match result {
                    Ok(nazm_core::Value::Enum(r)) if r.variant() == 0 => {
                        let next = r.field(0).cloned().unwrap_or(nazm_core::Value::Int(0));
                        let after = amount(&mut interpreter, &next);
                        let (old, new) = (fields_of(&state, n), fields_of(&next, n));
                        let changed: Vec<&String> = c
                            .fields
                            .iter()
                            .zip(old.iter().zip(new.iter()))
                            .filter(|(_, (a, b))| a != b)
                            .map(|(name, _)| name)
                            .collect();
                        if before == after {
                            put(&mut record, "outcome", json!("ok"));
                            put(&mut record, "changed", json!(changed));
                            state = next;
                        } else {
                            put(&mut record, "outcome", json!("rejected"));
                            put(&mut record, "code", json!(codes::CONSERVATION_BROKEN.as_str()));
                            put(&mut record, "conserved", json!({ "before": before, "after": after }));
                            broken.push((*line, entry.to_owned(), before, after));
                        }
                    }
                    Ok(nazm_core::Value::Enum(r)) => {
                        put(&mut record, "outcome", json!("revert"));
                        put(&mut record, "code", json!(r.field(0).map(shown)));
                    }
                    Ok(_) => put(&mut record, "outcome", json!("revert")),
                    Err(e) => {
                        put(&mut record, "outcome", json!("fail"));
                        put(&mut record, "code", json!(e.diagnostic.code.as_str()));
                    }
                }
                runs.push(record);
            }
            let final_state: serde_json::Map<String, Json> = c
                .fields
                .iter()
                .cloned()
                .zip(fields_of(&state, n).into_iter().map(Json::from))
                .collect();
            Ok((
                json!({
                    "schema": "nazm.contract-run/1",
                    "transactions": runs,
                    "state": final_state,
                    "meter": "per transaction, the calls the interpreter made below the entrypoint and the loop turns it took: counts, not gas",
                }),
                Some(broken),
            ))
        },
    );
    match outcome {
        Ok((doc, broken)) => {
            println!("{doc}");
            let broken = broken.unwrap_or_default();
            for (line, entry, before, after) in &broken {
                eprintln!(
                    "error[{}]: transaction {line} (`{entry}`) changed the conserved quantity from {} to {}; it was rejected",
                    codes::CONSERVATION_BROKEN.as_str(),
                    before.as_deref().unwrap_or("?"),
                    after.as_deref().unwrap_or("?")
                );
            }
            if broken.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(exit::CHECK_FAILED)
            }
        }
        Err(d) => {
            crate::emit(&d, &loaded.map, false);
            ExitCode::from(exit::CHECK_FAILED)
        }
    }
}
