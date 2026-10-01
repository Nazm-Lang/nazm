//! `nazm inspect` — one program's facts in one deterministic document, `nazm.inspect/1` (N48).
//!
//! `docs/architecture.md` §7.50. For every definition of a checked program: its durable identity,
//! signature, declared and required effects, the capabilities it takes, its provenance summary,
//! its Core IR digests, and for each concrete function MIR has for it the executable digest, size
//! and the validator's verdict; and for the build, the toolchain's identities — compiler, semantic
//! epoch, interface schema, runtime ABI, Cranelift, target — and the package, lock and profiles.
//! No absolute path, time, address or iteration order enters it: the same program, toolchain and
//! build give the same bytes.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Map, Value};

use crate::{exit, load};

fn effects(set: Option<nazm_core::sema::EffectSet>) -> Value {
    set.map_or(Value::Null, |s| json!(s.names()))
}

pub(crate) fn run(path: &Path, only: Option<&str>) -> ExitCode {
    let source_root = match crate::root::decide(path, None) {
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
    let map = &loaded.map;
    let graph = &loaded.graph;
    let built = nazm_core::on_lowered(map, graph, nazm_core::Entry::Absent, |core, res| {
        let mir = crate::mir::lower(core, res, graph);
        let names = nazm_core::cir::Names::new(res, graph);
        let digests = nazm_core::cir::digests(core, res, graph);
        let types = res.types();
        let mut defs = Vec::new();
        for (f, d) in core.functions.iter().zip(&digests) {
            let identity = names.function(f.def);
            if only.is_some_and(|o| !identity.ends_with(&format!("::fn {o}"))) {
                continue;
            }
            let Some(def) = res.function(f.def) else {
                continue;
            };
            let params: Vec<String> = def.params.iter().map(|t| types.show(*t)).collect();
            let capabilities: Vec<String> = def
                .params
                .iter()
                .filter(|t| t.is_capability())
                .map(|t| types.show(*t))
                .collect();
            let provenance = def.provenance.as_ref().map_or(Value::Null, |p| {
                json!({
                    "result": p.origins.names(),
                    "passes_on": p.params,
                    "reaches_sink": p.reaches,
                })
            });
            let native: Vec<Value> = match &mir {
                Ok(program) => program
                    .functions
                    .iter()
                    .filter(|m| m.def == f.def)
                    .map(|m| {
                        let statements: usize = m.blocks.iter().map(|b| b.stmts.len()).sum();
                        json!({
                            "identity": nazm_mir::text::identity(&m.identity),
                            "digest": nazm_mir::digest::executable(program, m),
                            "blocks": m.blocks.len(),
                            "statements": statements,
                            "locals": m.locals.len(),
                            "foreign": m.foreign,
                        })
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            let mut o = Map::new();
            o.insert("identity".into(), json!(identity));
            o.insert("params".into(), json!(params));
            o.insert("ret".into(), json!(types.show(def.ret)));
            o.insert("generic".into(), json!(!def.type_params.is_empty()));
            o.insert(
                "effects".into(),
                json!({
                    "declared": effects(def.declared_effects),
                    "required": effects(def.inferred_effects),
                }),
            );
            o.insert("capabilities".into(), json!(capabilities));
            o.insert("provenance".into(), provenance);
            o.insert("foreign".into(), json!(def.foreign));
            o.insert(
                "core_ir".into(),
                json!({"semantic": d.semantic_hex(), "body": d.body_hex()}),
            );
            o.insert("native".into(), json!(native));
            defs.push(Value::Object(o));
        }
        defs.sort_by_key(|v| v["identity"].as_str().map(str::to_owned));
        let mir_state = match &mir {
            Ok(p) => json!({"validated": true, "functions": p.functions.len()}),
            Err(d) => json!({"validated": false, "errors": d.len()}),
        };
        Ok((defs, mir_state))
    });
    let (defs, mir_state) = match built {
        Ok(x) => x,
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    let package = crate::package::graph_for(path)
        .and_then(Result::ok)
        .map_or(Value::Null, |g| {
            let lock = match nazm_package::verify_locked(&g) {
                Ok(()) => "verified",
                Err(d) if d.iter().any(|x| x.code == nazm_diag::codes::LOCK_MISSING) => "absent",
                Err(_) => "mismatch",
            };
            let root = g.root_package();
            json!({
                "name": root.map(|r| r.name.clone()),
                "version": root.map(|r| r.version.to_string()),
                "digest": root.map(|r| r.digest.clone()),
                "packages": g.packages.len(),
                "lock": lock,
                "profiles": g.packages.iter().filter_map(|p| p.profile.clone()).collect::<Vec<_>>(),
            })
        });
    let root_key = graph
        .module_of(nazm_span::FileId::ROOT)
        .and_then(|m| graph.key(m))
        .map_or(Value::Null, |k| json!(k.as_str()));
    let target = nazm_lir::backend::Target::host().map_or(Value::Null, |t| json!(t.triple));
    let doc = json!({
        "schema": "nazm.inspect/1",
        "root": root_key,
        "toolchain": {
            "compiler": env!("CARGO_PKG_VERSION"),
            "semantic_epoch": nazm_cache::SEMANTIC_EPOCH,
            "interface": nazm_iface::SCHEMA,
            "runtime_abi": nazm_lir::runtime::RUNTIME_ABI_REVISION,
            "cranelift": nazm_codegen_clif::CRANELIFT_VERSION,
            "target": target,
        },
        "package": package,
        "mir": mir_state,
        "definitions": defs,
    });
    println!("{doc}");
    ExitCode::from(exit::OK)
}
