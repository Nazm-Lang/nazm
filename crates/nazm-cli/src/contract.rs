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

/// `v[key]`, or `null`: a JSON field read without the index operator.
fn at(v: &Json, key: &str) -> Json {
    v.get(key).cloned().unwrap_or(Json::Null)
}

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

/// The storage layout of `file`'s contract, by field name, for an upgrade check.
fn layout_of(file: &Path) -> Result<Vec<(String, String)>, ExitCode> {
    let source_root = crate::root::decide(file, None).map_err(|e| {
        eprintln!("nazm: {e}");
        ExitCode::from(exit::NO_SUCH_FILE)
    })?;
    let loaded = crate::load::load(file, source_root.path()).map_err(|(partial, d)| {
        crate::emit(&d, &partial.map, false);
        ExitCode::from(exit::NO_SUCH_FILE)
    })?;
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            let c = nazm_service::contract::of(core, res, root).map_err(|why| {
                vec![Diagnostic::error(
                    codes::NOT_A_CONTRACT,
                    Span::synthetic(),
                    format!("this module is not a contract: {why}"),
                )]
            })?;
            Ok(res
                .records()
                .get(c.state)
                .map(|d| {
                    d.fields
                        .iter()
                        .map(|f| (f.name.clone(), res.types().show(f.ty)))
                        .collect()
                })
                .unwrap_or_default())
        },
    )
    .map_err(|d| {
        crate::emit(&d, &loaded.map, false);
        ExitCode::from(exit::CHECK_FAILED)
    })
}

/// `nazm contract NEW --upgrade-check OLD`: whether NEW's storage can take over OLD's live
/// storage. Slots are by field name, so order is free and an added field moves nothing; a removed
/// field strands its value and a changed type misreads it.
pub(crate) fn upgrade_check(new: &Path, old: &Path) -> ExitCode {
    let (before, after) = match (layout_of(old), layout_of(new)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let mut changes = Vec::new();
    for (name, ty) in &before {
        match after.iter().find(|(n, _)| n == name) {
            None => changes.push(json!({ "field": name, "verdict": "incompatible", "why": format!("removed: the value stored for `{name}` would be stranded") })),
            Some((_, t)) if t != ty => changes.push(json!({ "field": name, "verdict": "incompatible", "why": format!("its type changed from `{ty}` to `{t}`: the stored word would be misread") })),
            Some(_) => {}
        }
    }
    for (name, _) in after
        .iter()
        .filter(|(n, _)| !before.iter().any(|(b, _)| b == n))
    {
        changes.push(json!({ "field": name, "verdict": "compatible", "why": "added: its slot is its own, and starts at zero" }));
    }
    let incompatible = changes.iter().any(|c| at(c, "verdict") == "incompatible");
    println!(
        "{}",
        json!({
            "schema": "nazm.evm-upgrade/1",
            "outcome": if incompatible { "incompatible" } else { "compatible" },
            "changes": changes,
        })
    );
    for c in changes
        .iter()
        .filter(|c| at(c, "verdict") == "incompatible")
    {
        eprintln!(
            "error[{}]: `{}` cannot be upgraded in place: {}",
            codes::EVM_UNSUPPORTED.as_str(),
            at(c, "field").as_str().unwrap_or("?"),
            at(c, "why").as_str().unwrap_or("?")
        );
    }
    if incompatible {
        ExitCode::from(exit::CHECK_FAILED)
    } else {
        ExitCode::SUCCESS
    }
}

const EVM_RUN: &str = include_str!("evm_run.py");

pub(crate) fn run(
    file: &Path,
    interface: bool,
    txs: Option<&Path>,
    evm: Option<&Path>,
    wasm: Option<&Path>,
    accounts: Option<&Path>,
) -> ExitCode {
    // The profile first, by the same path every command enforces one: a contract that is not
    // deterministic is not run at all.
    // An account-oriented target is held to `accounts` (N72): `web3`, and signed writes.
    let profile = if accounts.is_some() {
        "accounts"
    } else {
        "web3"
    };
    if let Err(code) = crate::profile::enforce(file, Some(profile), false, false) {
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
    let mut shape: Option<crate::contract_wasm::Shape> = None;
    let shape_out = &mut shape;
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
            if let Some(dir) = accounts {
                let fields_meta: Vec<Json> = c
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(k, name)| {
                        let is_bool = state_def
                            .and_then(|d| d.fields.get(k))
                            .is_some_and(|f| f.ty == nazm_core::sema::Type::Bool);
                        json!({ "name": name, "offset": 1 + 8 * k, "size": 8, "type": if is_bool { "bool as u64" } else { "i64 little-endian" } })
                    })
                    .collect();
                let instructions: Vec<Json> = c
                    .entrypoints
                    .iter()
                    .map(|e| {
                        let h = nazm_service::keccak::keccak256(format!("nazm.instruction.{}", e.name).as_bytes());
                        let writable = e.writes.as_ref().is_none_or(|w| !w.is_empty());
                        let args: Vec<Json> = e
                            .args
                            .iter()
                            .enumerate()
                            .map(|(k, a)| json!({ "name": a, "offset": 8 + 8 * k, "type": "i64 little-endian" }))
                            .collect();
                        json!({
                            "name": e.name,
                            "discriminator": nazm_service::keccak::hex(h.get(..8).unwrap_or(&[])),
                            "args": args,
                            "accounts": [
                                { "name": "state", "writable": writable, "signer": false, "owner": "this program" },
                                { "name": "caller", "writable": false, "signer": e.caller_checked },
                            ],
                        })
                    })
                    .collect();
                let doc = json!({
                    "schema": "nazm.sbpf-accounts/1",
                    "profile": "accounts",
                    "state_account": {
                        "owner": "this program",
                        "data": { "version": 1, "size": 1 + 8 * c.fields.len(), "layout": "byte 0 the layout version, then each field's 8 bytes by declaration index", "fields": fields_meta },
                    },
                    "instructions": instructions,
                    "cross_program_calls": "none: an entrypoint calls only Nazm functions",
                    "execution": "BLOCKED: no sBPF toolchain, validator or emulator on this host; this metadata is the target-neutral part",
                });
                if let Err(e) = std::fs::create_dir_all(dir)
                    .and_then(|()| std::fs::write(dir.join("accounts.json"), doc.to_string()))
                {
                    eprintln!("nazm: cannot write {}: {e}", dir.display());
                    return Err(Vec::new());
                }
                return Ok((doc, None));
            }
            if wasm.is_some() {
                let fields = state_def
                    .map(|d| {
                        d.fields
                            .iter()
                            .map(|f| (f.name.clone(), f.ty == nazm_core::sema::Type::Bool))
                            .collect()
                    })
                    .unwrap_or_default();
                *shape_out = Some(crate::contract_wasm::Shape {
                    fields,
                    entries: c
                        .entrypoints
                        .iter()
                        .map(|e| (e.name.clone(), e.args.len()))
                        .collect(),
                    conserved: c.conserved,
                });
                return Ok((facts, None));
            }
            if let Some(dir) = evm {
                let compiled = nazm_service::evm::compile(core, res, root, &c).map_err(|r| {
                    vec![Diagnostic::error(
                        codes::EVM_UNSUPPORTED,
                        r.at,
                        format!("`{}` cannot be compiled to the EVM: {}", r.function, r.why),
                    )
                    .primary_label("not emitted")]
                })?;
                let hex = nazm_service::keccak::hex;
                let abi: Vec<Json> = compiled
                    .abi
                    .iter()
                    .map(|a| json!({ "name": a.name, "signature": a.signature, "selector": hex(&a.selector), "args": a.args, "gas_bound": a.gas_bound }))
                    .collect();
                let storage: Vec<Json> = compiled
                    .storage
                    .iter()
                    .map(|s| json!({ "name": s.field, "type": s.ty, "slot": hex(&s.slot) }))
                    .collect();
                let abi_doc = json!({ "schema": "nazm.evm-abi/1", "entrypoints": abi });
                let storage_doc =
                    json!({ "schema": "nazm.evm-storage/1", "version": 1, "fields": storage });
                let written = std::fs::create_dir_all(dir)
                    .and_then(|()| std::fs::write(dir.join("deploy.hex"), hex(&compiled.deploy)))
                    .and_then(|()| std::fs::write(dir.join("runtime.hex"), hex(&compiled.runtime)))
                    .and_then(|()| std::fs::write(dir.join("abi.json"), abi_doc.to_string()))
                    .and_then(|()| {
                        std::fs::write(dir.join("storage.json"), storage_doc.to_string())
                    })
                    .and_then(|()| std::fs::write(dir.join("evm_run.py"), EVM_RUN));
                if let Err(e) = written {
                    eprintln!("nazm: cannot write {}: {e}", dir.display());
                    return Err(Vec::new());
                }
                return Ok((
                    json!({
                        "schema": "nazm.evm/1",
                        "deploy_bytes": compiled.deploy.len(),
                        "runtime_bytes": compiled.runtime.len(),
                        "abi": abi_doc,
                        "storage": storage_doc,
                        "gas": "gas_bound is a static upper bound on an entrypoint's execution gas, intrinsic and calldata costs apart; null where a loop leaves it open",
                    }),
                    None,
                ));
            }
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
                            put(
                                &mut record,
                                "code",
                                json!(codes::CONSERVATION_BROKEN.as_str()),
                            );
                            put(
                                &mut record,
                                "conserved",
                                json!({ "before": before, "after": after }),
                            );
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
    if let (Some(dir), Some(shape), Ok((facts, _))) = (wasm, shape.as_ref(), outcome.as_ref()) {
        return wasm_build(file, dir, shape, facts);
    }
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

/// `nazm contract FILE --wasm DIR` (N71): the ordinary WebAssembly build of the contract with its
/// adapter appended, its metadata, and the reference host.
fn wasm_build(
    file: &Path,
    dir: &Path,
    shape: &crate::contract_wasm::Shape,
    facts: &Json,
) -> ExitCode {
    let output = dir.join("contract");
    let built = crate::build::run(
        file,
        Some(&output),
        false,
        false,
        2,
        true,
        false,
        false,
        crate::build::Choice {
            backend: nazm_lir::backend::BackendId::Llvm,
            target: Some("wasm32-unknown-unknown"),
            link: &[],
            library: false,
            objects: Some(dir),
            stack_watermark: false,
            layout: "aos",
            adapter: Some(shape),
            timings: false,
            debug: false,
            runtime: None,
        },
    );
    if built != ExitCode::SUCCESS {
        return built;
    }
    let enc = |name: &str| {
        name.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    char::from(b).to_string()
                } else {
                    format!("_{b:02X}")
                }
            })
            .collect::<String>()
    };
    let fields: Vec<Json> = shape
        .fields
        .iter()
        .enumerate()
        .map(|(k, (name, is_bool))| json!({ "index": k, "name": name, "type": if *is_bool { "Bool" } else { "Int" } }))
        .collect();
    let entries: Vec<Json> = at(facts, "entrypoints")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            let name = at(&e, "name").as_str().unwrap_or_default().to_owned();
            json!({ "name": name, "export": format!("nazm_call_{}", enc(&name)), "args": at(&e, "args"), "reads": at(&e, "reads"), "writes": at(&e, "writes") })
        })
        .collect();
    let meta = json!({
        "schema": "nazm.wasm-contract/1",
        "version": 1,
        "state": { "fields": fields, "serialisation": "one i64 per field, by declaration index; a Bool as 0 or 1" },
        "entrypoints": entries,
        "init": "nazm_init",
        "imports": ["nazm_host.state_get", "nazm_host.state_set", "nazm_host.revert", "nazm_host.report"],
        "status": { "0": "committed", "1": "reverted by Err(code)", "2": "rejected: the conserved quantity changed", "3": "failed at run time" },
    });
    let written = std::fs::write(dir.join("contract.json"), meta.to_string())
        .and_then(|()| std::fs::write(dir.join("contract_host.mjs"), crate::contract_wasm::HOST));
    if let Err(e) = written {
        eprintln!("nazm: cannot write {}: {e}", dir.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    println!("{meta}");
    ExitCode::SUCCESS
}
