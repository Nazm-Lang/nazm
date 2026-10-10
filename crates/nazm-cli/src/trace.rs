//! `nazm trace FILE --requirements FILE` — requirements traced to what verifies them,
//! `nazm.trace/1` (Gate 3, `docs/systems-domains.md` Part S).
//!
//! A requirement is a stable identifier in a project file, `requirements.toml`: its text, the
//! definitions it constrains — functions of the program's own module, by name — and the `@test`
//! functions that verify it. Each definition is resolved to its durable identity (`DefKey`) and its
//! semantic digest, the Core IR one `nazm inspect` reports; its obligations are the census's, keyed
//! by that definition rather than by a name another module could share; each test is run, as `nazm
//! test` runs it. A link that does not hold — a definition renamed away, a test missing, a test
//! that fails — is reported by name and fails the command; nothing is dropped. Evidence for a
//! reviewer, not certification.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::{json, Value};

use crate::exit;

/// One requirement as `requirements.toml` writes it.
#[derive(Debug)]
struct Requirement {
    id: String,
    text: String,
    definitions: Vec<String>,
    tests: Vec<String>,
}

fn strings(t: &toml::Table, key: &str) -> Result<Vec<String>, String> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("`{key}` holds something that is not a string"))
            })
            .collect(),
        Some(_) => Err(format!("`{key}` is not a list of strings")),
    }
}

/// The requirements file: `schema = "nazm.requirements/1"` and one `[[requirement]]` each.
fn read_requirements(text: &str) -> Result<Vec<Requirement>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    if doc.get("schema").and_then(toml::Value::as_str) != Some("nazm.requirements/1") {
        return Err("`schema` must be \"nazm.requirements/1\"".to_owned());
    }
    let Some(toml::Value::Array(list)) = doc.get("requirement") else {
        return Err("no `[[requirement]]`".to_owned());
    };
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for r in list {
        let t = r
            .as_table()
            .ok_or("a `[[requirement]]` that is not a table")?;
        let id = t
            .get("id")
            .and_then(toml::Value::as_str)
            .ok_or("a requirement without an `id`")?
            .to_owned();
        if !seen.insert(id.clone()) {
            return Err(format!("the requirement `{id}` is written twice"));
        }
        out.push(Requirement {
            text: t
                .get("text")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            definitions: strings(t, "definitions").map_err(|e| format!("`{id}`: {e}"))?,
            tests: strings(t, "tests").map_err(|e| format!("`{id}`: {e}"))?,
            id,
        });
    }
    Ok(out)
}

/// One definition as the trace states it.
struct Traced {
    key: String,
    digest: String,
    obligations: Vec<Value>,
    proved: usize,
    checked: usize,
    unknown: usize,
}

/// Every `@test` of `file`, run as `nazm test --json` runs them: each one's outcome, by function.
fn test_outcomes(file: &Path) -> Result<BTreeMap<String, bool>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this nazm: {e}"))?;
    let out = Command::new(exe)
        .args(["test", "--json", "--interpret-only"])
        .arg(file)
        .output()
        .map_err(|e| format!("cannot run `nazm test`: {e}"))?;
    let mut outcomes = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("kind").and_then(Value::as_str) != Some("test") {
            continue;
        }
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
        let function = name.rsplit("::").next().unwrap_or(name).to_owned();
        let passed = v.get("passed").and_then(Value::as_bool).unwrap_or(false);
        outcomes.insert(function, passed);
    }
    Ok(outcomes)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(path: &Path, requirements: &Path, declared: Option<&Path>) -> ExitCode {
    let reqs = match std::fs::read_to_string(requirements)
        .map_err(|e| format!("cannot read `{}`: {e}", requirements.display()))
        .and_then(|t| read_requirements(&t))
    {
        Ok(r) => r,
        Err(why) => {
            eprintln!("nazm: {}: {why}", requirements.display());
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    let loaded = match crate::opened::opened(path, declared, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    // Each function of the program's own module: its durable identity, its digest, its
    // obligations.
    let found = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            let names = nazm_core::cir::Names::new(res, &loaded.graph);
            let digests = nazm_core::cir::digest::digests(core, res, &loaded.graph);
            let census = nazm_core::obligations::census(core, res);
            let mut out: BTreeMap<String, Traced> = BTreeMap::new();
            for (f, d) in core.functions.iter().zip(&digests) {
                if Some(f.module) != root || f.foreign.is_some() {
                    continue;
                }
                let mine: Vec<&nazm_core::obligations::Obligation> =
                    census.iter().filter(|o| o.def == f.def).collect();
                let count = |s: nazm_core::obligations::Status| {
                    mine.iter().filter(|o| o.status == s).count()
                };
                use nazm_core::obligations::Status;
                out.insert(
                    f.name.clone(),
                    Traced {
                        key: names.function(f.def),
                        digest: d.semantic_hex(),
                        obligations: mine
                            .iter()
                            .map(|o| {
                                let lc = loaded.map.line_col(o.at);
                                json!({ "kind": o.kind.name(), "status": o.status.name(), "line": lc.line, "column": lc.column })
                            })
                            .collect(),
                        proved: count(Status::Proved),
                        checked: count(Status::Checked),
                        unknown: count(Status::Unknown),
                    },
                );
            }
            Ok(out)
        },
    );
    let definitions = match found {
        Ok(d) => d,
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, &loaded.map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    let outcomes = match test_outcomes(path) {
        Ok(o) => o,
        Err(why) => {
            eprintln!("nazm: {why}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    let mut broken = Vec::new();
    let mut rows = Vec::new();
    for r in &reqs {
        let defs: Vec<Value> = r
            .definitions
            .iter()
            .map(|name| match definitions.get(name) {
                Some(t) => json!({
                    "name": name,
                    "key": t.key,
                    "digest": t.digest,
                    "obligations": { "proved": t.proved, "checked": t.checked, "unknown": t.unknown, "each": t.obligations },
                }),
                None => {
                    broken.push(format!(
                        "{}: the definition `{name}` is not in {}",
                        r.id,
                        path.display()
                    ));
                    json!({ "name": name, "key": Value::Null, "broken": "no such definition" })
                }
            })
            .collect();
        let tests: Vec<Value> = r
            .tests
            .iter()
            .map(|name| match outcomes.get(name) {
                Some(true) => json!({ "name": name, "outcome": "passed" }),
                Some(false) => {
                    broken.push(format!("{}: the test `{name}` fails", r.id));
                    json!({ "name": name, "outcome": "failed" })
                }
                None => {
                    broken.push(format!(
                        "{}: the test `{name}` is not a `@test` of {}",
                        r.id,
                        path.display()
                    ));
                    json!({ "name": name, "outcome": "missing" })
                }
            })
            .collect();
        if r.definitions.is_empty() || r.tests.is_empty() {
            broken.push(format!(
                "{}: a requirement needs a definition it constrains and a test that verifies it",
                r.id
            ));
        }
        rows.push(json!({ "id": r.id, "text": r.text, "definitions": defs, "tests": tests }));
    }
    let doc = json!({
        "schema": "nazm.trace/1",
        "file": path.display().to_string(),
        "requirements": rows,
        "status": if broken.is_empty() { "traced" } else { "broken" },
        "broken": broken,
        "evidence": "compiler evidence: each definition's durable identity, digest and obligations, and each test's outcome; not certification",
    });
    println!("{doc}");
    if broken.is_empty() {
        ExitCode::SUCCESS
    } else {
        for b in &broken {
            eprintln!("nazm: {b}");
        }
        ExitCode::from(exit::CHECK_FAILED)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::read_requirements;

    #[test]
    fn a_requirements_file_is_held_to_its_schema() {
        let ok = read_requirements(
            "schema = \"nazm.requirements/1\"\n[[requirement]]\nid = \"R1\"\ntext = \"t\"\n\
             definitions = [\"f\"]\ntests = [\"g\"]\n",
        )
        .unwrap();
        assert_eq!((ok[0].id.as_str(), ok[0].tests.len()), ("R1", 1));
        assert!(read_requirements("[[requirement]]\nid = \"R1\"\n").is_err());
        assert!(read_requirements(
            "schema = \"nazm.requirements/1\"\n[[requirement]]\nid = \"R\"\n[[requirement]]\nid = \"R\"\n"
        )
        .unwrap_err()
        .contains("twice"));
        assert!(read_requirements(
            "schema = \"nazm.requirements/1\"\n[[requirement]]\nid = \"R\"\ntests = [1]\n"
        )
        .is_err());
    }
}
