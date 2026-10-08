//! `nazm inspect` — one program's facts in one deterministic document, `nazm.inspect/2` (N48; `/2` since N104, which dropped `inherits`).
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

/// Every target `--target` accepts, in the order a reader should meet them; `nazm capabilities`
/// lists the same (N74).
pub(crate) const TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    // The boards, with no operating system (N62, §7.64; N86, §7.87): always objects and
    // `link.ld`.
    "aarch64-unknown-none",
    "riscv64gc-unknown-none-elf",
    // WebAssembly (N64, §7.66): objects, `wasm-ld`'s line and a reference host.
    "wasm32-unknown-unknown",
];

/// The board table, as data: where each image loads, its UART, how its machine stops, how it is
/// booted, and whether it has ever run on hardware.
fn boards() -> Vec<Value> {
    use nazm_lir::runtime::board::{Stop, BOARDS};
    BOARDS
        .iter()
        .map(|b| {
            let stop = match b.stop {
                Stop::Semihosting => json!({ "kind": "arm-semihosting" }),
                Stop::SifiveTest { address } => {
                    json!({ "kind": "sifive-test", "address": format!("{address:#x}") })
                }
            };
            json!({
                "triple": b.triple,
                "name": b.name,
                "load_address": format!("{:#x}", b.load_address),
                "uart": format!("{:#x}", b.uart),
                "stop": stop,
                "emulator": b.emulator,
                "hardware": b.hardware,
            })
        })
        .collect()
}

pub(crate) fn effects(set: Option<nazm_core::sema::EffectSet>) -> Value {
    set.map_or(Value::Null, |s| json!(s.names()))
}

pub(crate) fn run(path: &Path, only: Option<&str>) -> ExitCode {
    let source_root = match crate::root::decide(path, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(crate::exit::root_status(&e));
        }
    };
    let loaded = match load::load(path, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(crate::exit::load_status(&diagnostics));
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
    let host = nazm_lir::backend::Target::host().ok();
    let target = host.as_ref().map_or(Value::Null, |t| json!(t.triple));
    // Every target `--target` accepts, and whether this host links it or builds it to objects
    // (N57, §7.59).
    let targets: Vec<Value> = TARGETS
        .iter()
        .filter_map(|t| nazm_lir::backend::Target::parse(t).ok())
        .map(|t| {
            let links = host.as_ref().is_some_and(|h| t.links_on(h));
            json!({ "triple": t.triple, "output": if links { "executable" } else { "objects" } })
        })
        .collect();
    let doc = json!({
        "schema": "nazm.inspect/2",
        "root": root_key,
        "toolchain": {
            "compiler": env!("CARGO_PKG_VERSION"),
            "semantic_epoch": nazm_cache::SEMANTIC_EPOCH,
            "interface": nazm_iface::SCHEMA,
            "runtime_abi": nazm_lir::runtime::RUNTIME_ABI_REVISION,
            "cranelift": nazm_codegen_clif::CRANELIFT_VERSION,
            "target": target,
            "targets": targets,
            // The support matrix (N95, §7.96): every target, run-verified, compile-only or
            // unsupported under each backend, with its evidence and CPU identity.
            "support": nazm_lir::backend::SUPPORT.iter().map(|c| serde_json::json!({
                "triple": c.triple,
                "llvm": { "status": c.llvm.name(), "evidence": c.llvm.evidence(), "cpu": c.llvm_cpu },
                "cranelift": { "status": c.cranelift.name(), "evidence": c.cranelift.evidence(), "cpu": c.cranelift_cpu },
                "debugger": c.debugger,
            })).collect::<Vec<_>>(),
            "non_goals": nazm_lir::backend::NON_GOALS.iter()
                .map(|(what, why)| serde_json::json!({ "what": what, "why": why }))
                .collect::<Vec<_>>(),
            // Each board's description (N86, §7.87): what is the board's and not the language's.
            "boards": boards(),
            // What a native build still runs besides this compiler (N59, §7.61): the C
            // compiler driver, for the runtime's LLVM text under both backends and for the link.
            "needs": {
                "runtime": "clang compiles the runtime's LLVM text, under both backends",
                "link": "the C compiler driver links every executable",
            },
        },
        "package": package,
        "mir": mir_state,
        "definitions": defs,
    });
    println!("{doc}");
    ExitCode::from(exit::OK)
}
