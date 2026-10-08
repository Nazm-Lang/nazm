//! `nazm doc FILE|DIR [-o OUT]` (N74, `docs/architecture.md` §7.76): a program's public API, from its
//! semantic definitions.
//!
//! What a definition is comes from the checker — its module's durable key, its parameters' and
//! result's types, its declared and required effects, the capabilities it takes, its foreign symbol
//! or C export — and where it is from the syntax tree's spans. The only text read is the signature
//! as written and the comment lines directly above a definition, shown as its description: the
//! language has no doc comments, and this is the tool's convention, not a rule of the language.
//!
//! One `nazm.api-doc/2` document (N103: re-exports are items too). Each definition's anchor is its identity, so a link into
//! `api.md` holds across rebuilds and moves only when the definition does. Nothing depends on the
//! time or on an absolute path: a source is named relative to the package, or to the root file's
//! directory.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};

use crate::{exit, load};

/// The comment lines directly above byte `at` of `text`, without their `//`.
fn description(text: &str, at: usize) -> String {
    let before = text.get(..at).unwrap_or("");
    let mut lines: Vec<&str> = before.lines().collect();
    // The definition's own line, up to where it starts.
    if !before.ends_with('\n') {
        lines.pop();
    }
    let mut out = Vec::new();
    while let Some(l) = lines.pop() {
        let t = l.trim();
        let Some(c) = t.strip_prefix("//") else { break };
        out.push(c.strip_prefix(' ').unwrap_or(c).to_owned());
    }
    out.reverse();
    out.join("\n")
}

fn line_of(text: &str, at: usize) -> usize {
    text.get(..at).map_or(1, |t| t.matches('\n').count() + 1)
}

fn slug(identity: &str) -> String {
    identity
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// The modules to start from: a package's every module, or the file itself.
/// A package's modules, or the one file named, with the directory links are relative to; or the
/// refusal with its status — a package that does not resolve is 1, a directory that cannot be
/// read 3 (Q1-C1).
fn roots(path: &Path) -> Result<(Vec<PathBuf>, PathBuf), (u8, String)> {
    if path.is_dir() {
        let graph = nazm_package::resolve(path).map_err(|d| {
            (
                exit::CHECK_FAILED,
                d.first().map_or_else(
                    || "the package does not resolve".to_owned(),
                    |x| x.message.clone(),
                ),
            )
        })?;
        let root = graph
            .root_package()
            .ok_or((exit::CHECK_FAILED, "no root package".to_owned()))?;
        let mut out = Vec::new();
        let mut stack = vec![root.source_root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir)
                .map_err(|e| (exit::NO_SUCH_FILE, e.to_string()))?
                .flatten()
            {
                let p = e.path();
                // Not through a symlinked directory: the walk must end (Q1-A2-03's rule).
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(p);
                } else if p.is_dir() {
                    continue;
                } else if p.extension().is_some_and(|x| x == "nz") {
                    out.push(p);
                }
            }
        }
        out.sort();
        Ok((out, root.dir.clone()))
    } else {
        // `m.nz`'s parent is the empty path, which names no directory to be relative to.
        let base = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Ok((vec![path.to_path_buf()], base))
    }
}

fn relative(name: &str, base: &Path) -> String {
    let p = Path::new(name);
    let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    canon.strip_prefix(&base).map_or_else(
        |_| name.to_owned(),
        |r| r.to_string_lossy().replace('\\', "/"),
    )
}

/// Every public definition of one compilation's own modules, by module key.
fn document(
    path: &Path,
    base: &Path,
    into: &mut BTreeMap<String, BTreeMap<String, Value>>,
) -> Result<(), String> {
    let source_root = crate::root::decide(path, None).map_err(|e| e.to_string())?;
    // The loader's own diagnostics, not a bare "cannot load": the reason is the user's to see
    // (Q1-D-08).
    let loaded = load::load(path, source_root.path()).map_err(|(_, diagnostics)| {
        let why: Vec<String> = diagnostics.iter().map(|d| d.message.clone()).collect();
        format!("cannot load {}: {}", path.display(), why.join("; "))
    })?;
    let (map, graph) = (&loaded.map, &loaded.graph);
    let found = nazm_core::on_module_map(map, graph, |program, res| {
        let names = nazm_core::cir::Names::new(res, graph);
        let types = res.types();
        let module_key = |file: nazm_span::FileId| {
            graph
                .module_of(file)
                .and_then(|m| graph.key(m))
                .map(|k| k.as_str().to_owned())
        };
        // A module compiled into the toolchain is not this program's API.
        let own = |file: nazm_span::FileId| !map.name(file).starts_with('@');
        let mut out: Vec<(String, String, Value)> = Vec::new();
        for (i, def) in res.functions().iter().enumerate() {
            let file = def.name_span.file;
            let exported = def.visibility == nazm_core::sema::interface::Visibility::Exported;
            if def.closure.is_some() || !exported || !own(file) {
                continue;
            }
            let Some(module) = module_key(file) else {
                continue;
            };
            let Some(f) = program.functions.get(def.decl as usize) else {
                continue;
            };
            let text = map.text(file);
            let start =
                f.pub_span
                    .map_or(f.span.start, |p| p.start.min(f.span.start)) as usize;
            let end = f.body.as_ref().map_or(f.span.end, |b| b.span.start) as usize;
            let signature = text
                .get(start..end)
                .unwrap_or("")
                .trim_end()
                .trim_end_matches(';')
                .to_owned();
            let id = names.function(nazm_core::sema::FnId::from_index(i));
            let capabilities: Vec<String> = def
                .params
                .iter()
                .filter(|t| t.is_capability())
                .map(|t| types.show(*t))
                .collect();
            out.push((
                module,
                id.clone(),
                json!({
                    "id": id,
                    "anchor": slug(&id),
                    "kind": "fn",
                    "name": def.name,
                    "signature": signature,
                    "description": description(text, start),
                    "params": def.params.iter().map(|t| types.show(*t)).collect::<Vec<_>>(),
                    "ret": types.show(def.ret),
                    "generic": !def.type_params.is_empty(),
                    "effects": {
                        "declared": crate::inspect::effects(def.declared_effects),
                        "required": crate::inspect::effects(def.inferred_effects),
                    },
                    "capabilities": capabilities,
                    "foreign": def.foreign,
                    "export": def.export,
                    "source": { "path": relative(map.name(file), base), "line": line_of(text, def.name_span.start as usize) },
                }),
            ));
        }
        let mut types_of = |kind: &str,
                            name: &str,
                            name_span: nazm_span::Span,
                            pub_span: Option<nazm_span::Span>,
                            span: nazm_span::Span,
                            fields: Value| {
            let file = name_span.file;
            if pub_span.is_none() || !own(file) {
                return;
            }
            let Some(module) = module_key(file) else {
                return;
            };
            let text = map.text(file);
            let start = pub_span.map_or(span.start, |p| p.start.min(span.start)) as usize;
            let id = format!("{module}::{kind} {name}");
            out.push((
                module,
                id.clone(),
                json!({
                    "id": id,
                    "anchor": slug(&id),
                    "kind": kind,
                    "name": name,
                    "signature": text.get(start..span.end as usize).unwrap_or(""),
                    "description": description(text, start),
                    kind_fields(kind): fields,
                    "source": { "path": relative(map.name(file), base), "line": line_of(text, name_span.start as usize) },
                }),
            ));
        };
        for r in &program.records {
            let fields: Vec<Value> = r.fields.iter().map(|f| json!({ "name": f.name, "type": map.text(f.ty.span.file).get(f.ty.span.start as usize..f.ty.span.end as usize) })).collect();
            types_of(
                "struct",
                &r.name,
                r.name_span,
                r.pub_span,
                r.span,
                json!(fields),
            );
        }
        for e in &program.enums {
            let variants: Vec<Value> = e.variants.iter().map(|v| json!({ "name": v.name, "fields": v.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>() })).collect();
            types_of(
                "enum",
                &e.name,
                e.name_span,
                e.pub_span,
                e.span,
                json!(variants),
            );
        }
        // What each module re-exports (N103): part of its surface, documented as the name it
        // offers and the definition it is — the defining module's identity, never a copy.
        for import in &program.imports {
            let Some(re) = &import.reexport else { continue };
            let file = import.span.file;
            if !own(file) {
                continue;
            }
            let Some(module) = module_key(file) else {
                continue;
            };
            let text = map.text(file);
            let signature = text
                .get(import.span.start as usize..import.span.end as usize)
                .unwrap_or("")
                .trim_end_matches(';')
                .to_owned();
            let line = line_of(text, import.span.start as usize);
            match &re.names {
                None => {
                    let id = format!("{module}::reexport {}", import.path);
                    out.push((
                        module.clone(),
                        id.clone(),
                        json!({
                            "id": id,
                            "anchor": slug(&id),
                            "kind": "reexport",
                            "name": import.path,
                            "signature": signature,
                            "description": "",
                            "of": "every export",
                            "source": { "path": relative(map.name(file), base), "line": line },
                        }),
                    ));
                }
                Some(list) => {
                    for item in list {
                        let offered = item.alias.as_ref().map_or(&item.name, |(a, _)| a);
                        let of = match res.callee(item.span) {
                            Some(nazm_core::sema::Callee::Function(id)) => Some(names.function(id)),
                            _ => res
                                .type_name(item.span)
                                .map(|n| match n {
                                    nazm_core::sema::TypeNode::Record(r) => {
                                        names.ty(nazm_core::sema::Type::Record(r))
                                    }
                                    nazm_core::sema::TypeNode::Enum(e) => {
                                        names.ty(nazm_core::sema::Type::Enum(e))
                                    }
                                })
                                .or_else(|| {
                                    res.traits()
                                        .mentions()
                                        .find(|(at, _)| *at == item.span)
                                        .and_then(|(_, t)| res.traits().get(t))
                                        .map(|t| {
                                            format!("{}::trait {}", names.module(t.module), t.name)
                                        })
                                }),
                        };
                        let id = format!("{module}::reexport {offered}");
                        out.push((
                            module.clone(),
                            id.clone(),
                            json!({
                                "id": id,
                                "anchor": slug(&id),
                                "kind": "reexport",
                                "name": offered,
                                "signature": signature,
                                "description": "",
                                "of": of,
                                "source": { "path": relative(map.name(file), base), "line": line },
                            }),
                        ));
                    }
                }
            }
        }
        Ok(out)
    });
    let items = found.map_err(|d| {
        let n = d.len();
        crate::emit(&d, map, false);
        format!(
            "{n} error{} — a program that does not check has no API to document",
            if n == 1 { "" } else { "s" }
        )
    })?;
    for (module, id, v) in items {
        into.entry(module).or_default().insert(id, v);
    }
    Ok(())
}

fn kind_fields(kind: &str) -> &'static str {
    if kind == "enum" {
        "variants"
    } else {
        "fields"
    }
}

/// The value at a JSON pointer, or null: the document is this module's own, and a missing field
/// is shown as absent rather than trusted to be there.
fn at<'a>(v: &'a Value, pointer: &str) -> &'a Value {
    v.pointer(pointer).unwrap_or(&Value::Null)
}

fn markdown(doc: &Value) -> String {
    let mut md = String::new();
    let title = at(doc, "/package/name").as_str().unwrap_or("API");
    let _ = writeln!(md, "# {title}\n");
    if let Some(p) = at(doc, "/package/profiles")
        .as_array()
        .filter(|p| !p.is_empty())
    {
        let names: Vec<&str> = p.iter().filter_map(Value::as_str).collect();
        let _ = writeln!(md, "Held to the profiles: {}.\n", names.join(", "));
    }
    for m in doc["modules"].as_array().into_iter().flatten() {
        let _ = writeln!(md, "## Module `{}`\n", m["module"].as_str().unwrap_or(""));
        for it in m["items"].as_array().into_iter().flatten() {
            let _ = writeln!(md, "<a id=\"{}\"></a>", it["anchor"].as_str().unwrap_or(""));
            let _ = writeln!(
                md,
                "### `{} {}`\n",
                it["kind"].as_str().unwrap_or(""),
                it["name"].as_str().unwrap_or("")
            );
            let _ = writeln!(
                md,
                "```nazm\n{}\n```\n",
                it["signature"].as_str().unwrap_or("")
            );
            if let Some(d) = it["description"].as_str().filter(|d| !d.is_empty()) {
                let _ = writeln!(md, "{d}\n");
            }
            if it["kind"] == "fn" {
                let show = |v: &Value| match v.as_array() {
                    Some(a) if a.is_empty() => "none".to_owned(),
                    Some(a) => a
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "),
                    None => "not declared".to_owned(),
                };
                let _ = writeln!(
                    md,
                    "- effects: declared {}; required {}",
                    show(at(it, "/effects/declared")),
                    show(at(it, "/effects/required"))
                );
                let caps = show(&it["capabilities"]);
                let _ = writeln!(md, "- capabilities: {caps}");
                if let Some(s) = it["foreign"].as_str() {
                    let _ = writeln!(md, "- foreign: calls C's `{s}`");
                }
                if let Some(s) = it["export"].as_str() {
                    let _ = writeln!(md, "- export: C calls it as `{s}`");
                }
            }
            if it["kind"] == "reexport" {
                let of = it["of"].as_str().unwrap_or("nothing resolved");
                let _ = writeln!(md, "- re-exports: `{of}`");
            }
            let path = at(it, "/source/path").as_str().unwrap_or("");
            let line = at(it, "/source/line").as_u64().unwrap_or(1);
            let _ = writeln!(md, "- source: [{path}:{line}]({path}#L{line})\n");
        }
    }
    md
}

pub(crate) fn run(path: &Path, out: Option<&Path>) -> ExitCode {
    let (modules, base) = match roots(path) {
        Ok(r) => r,
        Err((code, e)) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(code);
        }
    };
    let mut by_module: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for m in &modules {
        if let Err(e) = document(m, &base, &mut by_module) {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::CHECK_FAILED);
        }
    }
    let package = crate::package::graph_for(path)
        .and_then(Result::ok)
        .map_or(Value::Null, |g| {
            let root = g.root_package();
            json!({
                "name": root.map(|r| r.name.clone()),
                "version": root.map(|r| r.version.to_string()),
                "profiles": g.packages.iter().filter_map(|p| p.profile.clone()).collect::<Vec<_>>(),
            })
        });
    let modules: Vec<Value> = by_module
        .into_iter()
        .map(|(module, items)| json!({ "module": module, "items": items.into_values().collect::<Vec<_>>() }))
        .collect();
    let doc = json!({ "schema": "nazm.api-doc/2", "package": package, "modules": modules });
    let Some(dir) = out else {
        println!("{doc}");
        return ExitCode::from(exit::OK);
    };
    let wrote = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join("api.json"), format!("{doc:#}\n")))
        .and_then(|()| std::fs::write(dir.join("api.md"), markdown(&doc)));
    if let Err(e) = wrote {
        eprintln!("nazm: cannot write {}: {e}", dir.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    eprintln!("nazm: wrote {0}/api.json and {0}/api.md", dir.display());
    ExitCode::from(exit::OK)
}
