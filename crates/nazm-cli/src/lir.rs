//! `nazm lir` — print what LIR decided for a checked program, `nazm.lir/1` (N81).
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

use crate::{exit, load};

/// The schema this prints.
pub(crate) const SCHEMA: &str = "nazm.lir/1";

fn ty_name(u: &Unit, t: Ty) -> String {
    match t {
        Ty::Int => "Int".to_owned(),
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
            .map_or_else(|| "Vec".to_owned(), |v| format!("Vec[{}]", v.source)),
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

/// `nazm.lir/1` for `native` on `target`, or the first table that does not hold.
fn printed(native: &nazm_lir::Native, target: &Target) -> Result<Value, String> {
    let data = DataLayout::of(target);
    let str_fields = data.str_fields();
    let mut units = Vec::new();
    for (artefact, u, instance) in nazm_lir::backend::units(native) {
        let lay = ByteLayouts::of(u, data);
        lay.verify()
            .map_err(|e| format!("unit `{artefact}`: {e}"))?;
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

pub(crate) fn run(path: &Path, target: Option<&str>, declared: Option<&Path>) -> ExitCode {
    let target = match target.map_or_else(Target::host, Target::parse) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
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
    let map = &loaded.map;
    let names = nazm_lir::SourceNames::of(map, &loaded.graph);
    let done = nazm_core::on_lowered(
        map,
        &loaded.graph,
        nazm_core::Entry::Required,
        |core, res| {
            let program = crate::mir::lower(core, res, &loaded.graph)?;
            let native = nazm_lir::lower(program, &names).map_err(|e| e.diagnostics)?;
            printed(&native, &target).map_err(|e| {
                vec![nazm_diag::Diagnostic::error(
                    nazm_diag::codes::COMPILER_DEFECT,
                    nazm_span::Span::synthetic(),
                    format!("the layout tables do not hold: {e}"),
                )]
            })
        },
    );
    match done {
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
