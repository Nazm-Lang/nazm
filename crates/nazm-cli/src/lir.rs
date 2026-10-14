//! `nazm lir` — print what LIR decided for a checked program, `nazm.lir/2` (N81, N105).
//!
//! `docs/architecture.md` §7.82. One deterministic line of JSON: the target's data layout, and
//! for every unit its functions' symbols and linkage, what it declares from elsewhere, every
//! record's and enum's byte layout on that target, and which types own anything. The tables are
//! the ones both backends read, validated as every native build validates them. What LIR does not
//! own yet — the operations each backend translates — is not here, and §7.82 says so.

use std::path::Path;
use std::process::ExitCode;

use nazm_lir::abi::{ByteLayouts, DataLayout, Struct};
use nazm_lir::backend::Target;
use nazm_lir::{Ty, Unit};
use serde_json::{json, Value};

use crate::exit;

/// The schema this prints.
pub(crate) const SCHEMA: &str = "nazm.lir/2";

fn ty_name(u: &Unit, t: Ty) -> String {
    match t {
        Ty::Int => "Int".to_owned(),
        Ty::Num(k) => k.name().to_owned(),
        Ty::Array(a) => u.arrays.iter().find(|x| x.id == a).map_or_else(
            || "array".to_owned(),
            |x| format!("[{}; {}]", ty_name(u, x.elem), x.len),
        ),
        Ty::Bool => "Bool".to_owned(),
        Ty::Str => "Str".to_owned(),
        Ty::Ints => "Ints".to_owned(),
        Ty::Strs => "Strs".to_owned(),
        Ty::Chan => "Chan".to_owned(),
        Ty::Record(id) => u
            .records
            .iter()
            .find(|r| r.id == id)
            .map_or_else(|| format!("record {}", id.0), |r| r.source.clone()),
        Ty::Enum(id) => u
            .enums
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| format!("enum {}", id.0), |e| e.source.clone()),
        Ty::Vec(id) => u
            .vecs
            .iter()
            .find(|v| v.id == id)
            // `source` is the vector type as the checker shows it: `Vec[Point]`, `Ints`.
            .map_or_else(|| "Vec".to_owned(), |v| v.source.clone()),
        Ty::Fn(_) => "fn".to_owned(),
        Ty::ChanOf(_) => "Chan[_]".to_owned(),
    }
}

fn structure(u: &Unit, s: &Struct, name: &str, owns: Option<bool>) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("name".into(), json!(name));
    m.insert("size".into(), json!(s.lay.size));
    m.insert("align".into(), json!(s.lay.align));
    if let Some(o) = owns {
        m.insert("owns".into(), json!(o));
    }
    m.insert(
        "fields".into(),
        s.offsets
            .iter()
            .zip(&s.fields)
            .map(|(off, t)| json!({ "type": ty_name(u, *t), "offset": off }))
            .collect(),
    );
    Value::Object(m)
}

/// `nazm.lir/2` for `native` on `target`, or the first table that does not hold, or a unit whose
/// LIR does not validate.
fn printed(
    native: &nazm_lir::Native,
    names: &nazm_lir::SourceNames<'_>,
    target: &Target,
) -> Result<Value, Vec<nazm_diag::Diagnostic>> {
    let defect = |e: String| {
        vec![nazm_diag::Diagnostic::error(
            nazm_diag::codes::COMPILER_DEFECT,
            nazm_span::Span::synthetic(),
            e,
        )]
    };
    let data = DataLayout::of(target);
    let str_fields = data.str_fields();
    let mut units = Vec::new();
    for (artefact, u, instance) in nazm_lir::backend::units(native) {
        let lay = ByteLayouts::of(u, data);
        lay.verify().map_err(|e| {
            defect(format!(
                "the layout tables do not hold: unit `{artefact}`: {e}"
            ))
        })?;
        // The unit's instructions (N105, `nazm.lir/2`): what both backends translate, validated.
        let m = nazm_lir::op::lower_unit(native, u, names, data, false, false)?;
        nazm_lir::op::validate::module(&m)
            .map_err(|e| defect(format!("LIR does not validate: {}", e.join("; "))))?;
        let functions: Vec<Value> = u
            .functions
            .iter()
            .filter_map(|f| native.sigs.get(f.index()))
            .map(|s| {
                json!({
                    "symbol": s.symbol.as_str(),
                    "linkage": format!("{:?}", s.linkage).to_lowercase(),
                    "params": s.params.iter().map(|t| ty_name(u, *t)).collect::<Vec<_>>(),
                    "ret": ty_name(u, s.ret),
                })
            })
            .collect();
        let externals: Vec<Value> = u
            .externals
            .iter()
            .map(|e| json!({ "symbol": e.symbol.as_str(), "foreign": e.foreign }))
            .collect();
        let records: Vec<Value> = u
            .records
            .iter()
            .filter_map(|r| {
                let s = lay.records.get(&r.id.0)?;
                Some(structure(u, s, &r.source, Some(lay.owns(Ty::Record(r.id)))))
            })
            .collect();
        let enums: Vec<Value> = u
            .enums
            .iter()
            .filter_map(|e| {
                let s = lay.enums.get(&e.id.0)?;
                Some(json!({
                    "name": e.source,
                    "size": s.lay.size,
                    "align": s.lay.align,
                    "tag": 0,
                    "payload": nazm_lir::abi::PAYLOAD,
                    "owns": lay.owns(Ty::Enum(e.id)),
                    "variants": e
                        .variants
                        .iter()
                        .zip(&s.variants)
                        .map(|(v, st)| structure(u, st, &v.source, None))
                        .collect::<Vec<_>>(),
                }))
            })
            .collect();
        units.push(json!({
            "artefact": artefact,
            "source": u.source,
            "instance": instance,
            "functions": functions,
            "externals": externals,
            "records": records,
            "enums": enums,
            "lir": { "digest": nazm_lir::op::digest(&m), "text": nazm_lir::op::print::module(&m) },
        }));
    }
    Ok(json!({
        "schema": SCHEMA,
        "target": target.triple,
        "data_layout": {
            "pointer": data.pointer,
            "int": 8,
            "bool": 1,
            "str": { "size": data.str().size, "align": data.str().align,
                     "pointer": str_fields[0], "length": str_fields[1], "owner": str_fields[2] },
        },
        "entry": native.entry.as_str(),
        "units": units,
    }))
}

/// Every unit's instruction-level LIR as text, each validated (N105).
fn ops(
    native: &nazm_lir::Native,
    names: &nazm_lir::SourceNames<'_>,
    target: &Target,
    soa: bool,
    debug: bool,
) -> Result<String, Vec<nazm_diag::Diagnostic>> {
    let mut out = String::new();
    for (_, u, _) in nazm_lir::backend::units(native) {
        let m = nazm_lir::op::lower_unit(native, u, names, DataLayout::of(target), debug, soa)?;
        if let Err(e) = nazm_lir::op::validate::module(&m) {
            return Err(vec![nazm_diag::Diagnostic::error(
                nazm_diag::codes::COMPILER_DEFECT,
                nazm_span::Span::synthetic(),
                format!("LIR does not validate: {}", e.join("; ")),
            )]);
        }
        out.push_str(&nazm_lir::op::print::module(&m));
        if debug {
            // What each position's file index names, as the backends' debug information names
            // it (R9: a code generator's line table needs the names, which the module holds
            // only as indices).
            let mut files = std::collections::BTreeSet::new();
            for f in &m.functions {
                if let Some((file, _)) = f.at {
                    files.insert(file);
                }
                for b in &f.blocks {
                    for i in &b.insts {
                        if let nazm_lir::op::ir::Inst::Loc(p) = i {
                            files.insert(p.file);
                        }
                    }
                }
            }
            for i in files {
                out.push_str(&format!("; file {i} {}\n", names.debug_file(i)));
            }
        }
        out.push('\n');
    }
    Ok(out)
}

pub(crate) fn run(
    path: &Path,
    target: Option<&str>,
    declared: Option<&Path>,
    as_ops: bool,
    soa: bool,
    debug: bool,
) -> ExitCode {
    let target = match target.map_or_else(Target::host, Target::parse) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::target_refused(target.is_some()));
        }
    };
    let loaded = match crate::opened::opened(path, declared, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let map = &loaded.map;
    let names = nazm_lir::SourceNames::of(map, &loaded.graph);
    let done = nazm_core::on_lowered(
        map,
        &loaded.graph,
        nazm_core::Entry::Required,
        |core, res| {
            let program = crate::mir::lower(core, res, &loaded.graph)?;
            let native = nazm_lir::lower(program, &names).map_err(|e| e.diagnostics)?;
            if as_ops {
                return ops(&native, &names, &target, soa, debug).map(Value::String);
            }
            printed(&native, &names, &target)
        },
    );
    match done {
        Ok(Value::String(text)) => {
            print!("{text}");
            ExitCode::from(exit::OK)
        }
        Ok(v) => {
            println!("{v}");
            ExitCode::from(exit::OK)
        }
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            ExitCode::from(exit::CHECK_FAILED)
        }
    }
}

/// `nazm lir --run`: the program on LIR's interpreter (N105), ended as the platform entry ends a
/// native build — its output, then `main`'s result and status 0, or the failure and status 2.
/// Outside the interpreter's subset (tasks, channels, the clock, C) it says so and exits 4.
pub(crate) fn oracle(path: &Path, declared: Option<&Path>, args: &[String]) -> ExitCode {
    use std::io::Write as _;
    let loaded = match crate::opened::opened(path, declared, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let map = &loaded.map;
    let names = nazm_lir::SourceNames::of(map, &loaded.graph);
    let target = match Target::host() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    let done = nazm_core::on_lowered(
        map,
        &loaded.graph,
        nazm_core::Entry::Required,
        |core, res| {
            let program = crate::mir::lower(core, res, &loaded.graph)?;
            let native = nazm_lir::lower(program, &names).map_err(|e| e.diagnostics)?;
            let mut modules = Vec::new();
            for (_, u, _) in nazm_lir::backend::units(&native) {
                let m = nazm_lir::op::lower_unit(
                    &native,
                    u,
                    &names,
                    DataLayout::of(&target),
                    false,
                    false,
                )?;
                if let Err(e) = nazm_lir::op::validate::module(&m) {
                    return Err(vec![nazm_diag::Diagnostic::error(
                        nazm_diag::codes::COMPILER_DEFECT,
                        nazm_span::Span::synthetic(),
                        format!("LIR does not validate: {}", e.join("; ")),
                    )]);
                }
                modules.push(m);
            }
            Ok((
                modules,
                native.entry.as_str().to_owned(),
                native.entry_roots,
            ))
        },
    );
    let (modules, entry, roots) = match done {
        Ok(x) => x,
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    // On a thread of its own with room for the oracle's depth, as the interpreter has.
    let args = args.to_vec();
    let run = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || nazm_lir::op::interp::run(&modules, &entry, roots, &args, 2_000_000_000))
        .map_err(|e| e.to_string())
        .and_then(|h| h.join().map_err(|_| "the oracle panicked".to_owned()));
    match run {
        Ok(Ok(o)) => {
            let _ = std::io::stdout().write_all(&o.stdout);
            let _ = std::io::stderr().write_all(&o.stderr);
            ExitCode::from(u8::try_from(o.code).unwrap_or(1))
        }
        Ok(Err(nazm_lir::op::interp::Stop::Refused(why))) => {
            // A program the oracle does not cover: the request's, not the machine's (Q1-C1).
            eprintln!("nazm: the LIR interpreter does not run this: {why}");
            ExitCode::from(exit::CHECK_FAILED)
        }
        Ok(Err(nazm_lir::op::interp::Stop::Budget)) => {
            // The program ran past its budget: a run-time stop, as the interpreter's is.
            eprintln!("nazm: the LIR interpreter's budget is spent");
            ExitCode::from(exit::RUNTIME_FAILED)
        }
        Ok(Err(nazm_lir::op::interp::Stop::Defect(why))) | Err(why) => {
            eprintln!("nazm: error[N0900]: the LIR interpreter found a defect: {why}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}
