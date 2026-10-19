//! The seven representative N33 tasks and their naive baselines, in one place: the N33
//! byte benchmark (`benchmark.rs`) and the N34 token benchmark
//! (`crates/nazm-tokens/tests/benchmark.rs`) both read them from here, so the two can never
//! measure different baselines. Each baseline is what an agent without the planner reads for
//! the same task: the whole files the facts are in, the whole documents the sections are in,
//! and — where a task needs tests — the catalogue entries naming the file, as a search for its
//! path would return them. Every task's mechanical checks run here, against the authorities
//! themselves (N27's packet, N32's section, the catalogue parsed independently).

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nazm_repo::{Budget, Intent, TaskRequest};
use nazm_service::context::ContextResult;
use nazm_service::LanguageService;
use serde_json::Value;

/// One task: its baseline's texts, its context and what checking it cost.
pub(crate) struct Scenario {
    pub(crate) name: &'static str,
    /// The naive baseline, one text per file or catalogue entry.
    pub(crate) naive: Vec<String>,
    /// Where each baseline text comes from, one per text: a file's path, or a catalogue
    /// entry's name. N35 prints them above the texts; nothing here counts them.
    pub(crate) labels: Vec<String>,
    pub(crate) context: Value,
    /// The context exactly as `nazm repo --task --json` prints it, without the newline.
    pub(crate) json: String,
    pub(crate) ms: f64,
    pub(crate) checks: usize,
}

impl Scenario {
    pub(crate) fn naive_bytes(&self) -> usize {
        self.naive.iter().map(String::len).sum()
    }
}

fn scenario(
    name: &'static str,
    naive: Vec<(String, String)>,
    context: Value,
    size: usize,
    ms: f64,
    checks: usize,
) -> Scenario {
    let json = nazm_repo::task(
        &root(),
        &TaskRequest {
            intent: Some(match context["intent"].as_str().unwrap() {
                "understand" => Intent::Understand,
                "edit" => Intent::Edit,
                "diagnose" => Intent::Diagnose,
                _ => Intent::Test,
            }),
            seeds: vec![context["seeds"][0].as_str().unwrap().to_owned()],
            state: None,
            budget: Budget::default(),
        },
    )
    .to_json();
    assert_eq!(json.len(), size, "{name}: the same request, the same bytes");
    let (labels, naive) = naive.into_iter().unzip();
    Scenario {
        name,
        naive,
        labels,
        context,
        json,
        ms,
        checks,
    }
}

pub(crate) fn root() -> PathBuf {
    nazm_docs::default_root()
}

pub(crate) fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// One whole file, with its path.
pub(crate) fn file(path: &str) -> (String, String) {
    (path.to_owned(), read(path))
}

/// The whole files, as the agent without the planner reads them.
pub(crate) fn texts(paths: &BTreeSet<String>) -> Vec<(String, String)> {
    paths.iter().map(|p| file(p)).collect()
}

/// A catalogue entry, with the name a search for it would show.
fn entry(name: &str, block: &str) -> (String, String) {
    (
        format!("xtask/mutations/mutations.toml [[mutation]] {name}"),
        block.to_owned(),
    )
}

/// A task, timed.
pub(crate) fn task(intent: Intent, seed: &str) -> (Value, usize, f64) {
    let request = TaskRequest {
        intent: Some(intent),
        seeds: vec![seed.to_owned()],
        state: None,
        budget: Budget::default(),
    };
    let start = Instant::now();
    let r = nazm_repo::task(&root(), &request);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let json = r.to_json();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(
        v["status"] == "complete" || v["status"] == "partial",
        "{seed}: {json}"
    );
    (v, json.len(), ms)
}

pub(crate) fn item<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("{id} is not in the context"))
}

/// A definition's N27 packet in `root`'s compilation, straight from the service.
pub(crate) fn packet(root_file: &str, identity: &str) -> nazm_service::ContextPacket {
    let dir = root().join(Path::new(root_file).parent().unwrap());
    let file = root().join(root_file);
    let identity = identity.to_owned();
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(s, move || {
                let mut service = LanguageService::with_source_root(Some(dir.clone()));
                service.open_from_disk(&file).unwrap();
                let defs = service.durable_definitions(&file, 0).unwrap();
                let d = defs
                    .definitions
                    .iter()
                    .find(|d| d.identity == identity)
                    .unwrap();
                match service.context(&file, 0, &dir.join(&d.file), d.name_start) {
                    ContextResult::Available(p) => *p,
                    ContextResult::Unavailable(u) => panic!("{u:?}"),
                }
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

pub(crate) fn section(id: &str) -> nazm_docs::DocsSection {
    match nazm_docs::section(&root(), id, None) {
        nazm_docs::SectionResult::Available(s) => *s,
        other => panic!("{id}: {}", other.to_json()),
    }
}

/// Every catalogue entry, as the text a search for its file would find, with its name,
/// file, find text and killers — parsed here, not by the planner.
pub(crate) fn catalogue() -> Vec<(String, String, String, String, Vec<String>)> {
    let text = read("xtask/mutations/mutations.toml");
    let table: toml::Table = text.parse().unwrap();
    let blocks: Vec<&str> = text.split("[[mutation]]").skip(1).collect();
    let mut out = Vec::new();
    for (m, block) in table["mutation"].as_array().unwrap().iter().zip(blocks) {
        let killers = m
            .get("killers")
            .and_then(toml::Value::as_array)
            .map(|k| {
                k.iter()
                    .map(|k| {
                        let pkg = k["package"].as_str().unwrap();
                        let target = if k.get("lib").and_then(toml::Value::as_bool) == Some(true) {
                            format!("lib:{}", pkg.replace('-', "_"))
                        } else if let Some(t) = k.get("test").and_then(toml::Value::as_str) {
                            format!("test:{t}")
                        } else {
                            format!("bin:{}", k["bin"].as_str().unwrap())
                        };
                        format!("test:{pkg}:{target}::{}", k["name"].as_str().unwrap())
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push((
            m["name"].as_str().unwrap().to_owned(),
            m["file"].as_str().unwrap().to_owned(),
            m["find"].as_str().unwrap().to_owned(),
            block.to_owned(),
            killers,
        ));
    }
    out
}

/// Every task, each checked.
pub(crate) fn all() -> Vec<Scenario> {
    let mut rows: Vec<Scenario> = Vec::new();
    let catalogue = catalogue();

    // A — understand one definition: the checker's entry point.
    let seed = "def:compiler:analyse.nz::fn check_program";
    let (v, size, ms) = task(Intent::Understand, seed);
    let p = packet("compiler/emit.nz", "analyse.nz::fn check_program");
    let mut checks = 0;
    let s = item(&v, seed);
    assert_eq!(
        s["content"]["signature"],
        p.target.signature.clone().unwrap().as_str()
    );
    assert_eq!(s["content"]["source"], p.target.source.text.as_str());
    checks += 2;
    let deps: BTreeSet<String> = p
        .dependencies
        .iter()
        .filter_map(|d| d.identity.definition.clone())
        .filter(|k| k != "analyse.nz::fn check_program")
        .collect();
    assert!(deps.len() <= nazm_repo::FAN_OUT + p.related_types.len());
    for k in &deps {
        item(&v, &format!("def:compiler:{k}"));
        checks += 1;
    }
    for t in &p.related_types {
        let t = item(
            &v,
            &format!("def:compiler:{}", t.identity.definition.clone().unwrap()),
        );
        assert_eq!(t["reason"], "signature_type");
        assert!(t["content"]["fields"].is_array());
        checks += 1;
    }
    assert_eq!(
        item(&v, "doc:bootstrap:2")["content"]["body"],
        section("bootstrap:2").body.as_str()
    );
    checks += 1;
    let mut files: BTreeSet<String> = deps
        .iter()
        .map(|k| format!("compiler/{}", k.split_once("::").unwrap().0))
        .collect();
    files.insert("compiler/analyse.nz".into());
    files.insert("docs/internal/bootstrap.md".into());
    rows.push(scenario(
        "A understand check_program",
        texts(&files),
        v,
        size,
        ms,
        checks,
    ));

    // B — edit one definition: what uses it in every compilation, and its catalogue tests.
    let seed = "def:compiler:module.nz::fn attach_core";
    let (v, size, ms) = task(Intent::Edit, seed);
    let mut checks = 0;
    let mut files = BTreeSet::from([
        "compiler/module.nz".to_owned(),
        "docs/internal/bootstrap.md".to_owned(),
    ]);
    for r in ["compiler/emit.nz", "compiler/check.nz"] {
        let p = packet(r, "module.nz::fn attach_core");
        assert_eq!(
            item(&v, seed)["content"]["source"],
            p.target.source.text.as_str()
        );
        checks += 1;
        for c in p.callers.items.unwrap() {
            let k = c.identity.definition.unwrap();
            assert_eq!(
                item(&v, &format!("def:compiler:{k}"))["reason"],
                "direct_caller"
            );
            files.insert(format!("compiler/{}", k.split_once("::").unwrap().0));
            checks += 1;
        }
        for d in p
            .dependencies
            .iter()
            .filter_map(|d| d.identity.definition.clone())
        {
            files.insert(format!("compiler/{}", d.split_once("::").unwrap().0));
        }
    }
    let text = read("compiler/module.nz");
    let decl = &packet("compiler/emit.nz", "module.nz::fn attach_core")
        .target
        .source;
    let mut entries = Vec::new();
    for (name, path, find, block, killers) in &catalogue {
        if path != "compiler/module.nz" {
            continue;
        }
        entries.push(entry(name, block));
        let at = text.find(find.as_str()).unwrap() as u32;
        if at >= decl.start && at + find.len() as u32 <= decl.end {
            for k in killers {
                assert_eq!(item(&v, k)["reason"], "killer_test", "{name}");
                checks += 1;
            }
        }
    }
    assert_eq!(
        item(&v, "target:nazm-cli:test:selfhost")["reason"],
        "profile_target"
    );
    checks += 1;
    let mut naive = texts(&files);
    naive.extend(entries);
    rows.push(scenario("B edit attach_core", naive, v, size, ms, checks));

    // C — diagnose one diagnostic, with its owner.
    let seed =
        "diagnostic:examples-bad:return_type_mismatch.nz:N0300@return_type_mismatch.nz:59-69#0";
    let (v, size, ms) = task(Intent::Diagnose, seed);
    let s = item(&v, seed);
    assert_eq!(s["content"]["code"], "N0300");
    assert_eq!(
        s["content"]["message"],
        "this returns `Str`, but the function returns `Int`"
    );
    let owner = item(&v, "def:examples-bad:return_type_mismatch.nz::fn describe");
    let p = packet(
        "examples/bad/return_type_mismatch.nz",
        "return_type_mismatch.nz::fn describe",
    );
    assert_eq!(owner["content"]["source"], p.target.source.text.as_str());
    let naive = vec![file("examples/bad/return_type_mismatch.nz")];
    rows.push(scenario("C diagnose N0300", naive, v, size, ms, 3));

    // D — understand one specification concept, and one generated guide rule.
    let seed = "doc:spec:returns";
    let (v, size, ms) = task(Intent::Understand, seed);
    assert_eq!(
        item(&v, seed)["content"]["body"],
        section("spec:returns").body.as_str()
    );
    assert_eq!(
        item(&v, "doc:spec:return")["content"]["body"],
        section("spec:return").body.as_str()
    );
    assert_eq!(item(&v, "doc:spec:return")["reason"], "linked_section");
    let naive = vec![file("docs/user/spec.md")];
    rows.push(scenario("D understand spec:returns", naive, v, size, ms, 3));

    let seed = "doc:guide:the-syntax/record";
    let (v, size, ms) = task(Intent::Understand, seed);
    let g = item(&v, "doc:grammar:record");
    assert_eq!(g["reason"], "generated_from");
    assert_eq!(
        g["content"]["body"],
        section("grammar:record").body.as_str()
    );
    assert_eq!(g["authority"], "grammar");
    let naive = vec![file("docs/user/guide.md"), file("docs/user/grammar.ebnf")];
    rows.push(scenario("D understand guide record", naive, v, size, ms, 3));

    // E — the tests relevant to a changed area: a package, and one definition.
    let seed = "package:nazm-docs";
    let (v, size, ms) = task(Intent::Test, seed);
    let mut checks = 0;
    let mut entries = Vec::new();
    for (name, path, _, block, killers) in &catalogue {
        if path.starts_with("crates/nazm-docs/") {
            entries.push(entry(name, block));
            for k in killers {
                assert_eq!(item(&v, k)["reason"], "killer_test");
                checks += 1;
            }
        }
    }
    for t in [
        "target:nazm-docs:test:corpus",
        "target:nazm-docs:lib:nazm_docs",
    ] {
        item(&v, t);
        checks += 1;
    }
    let mut naive = vec![
        file("crates/nazm-docs/Cargo.toml"),
        file("crates/nazm-docs/tests/corpus.rs"),
    ];
    naive.extend(entries);
    rows.push(scenario("E tests of nazm-docs", naive, v, size, ms, checks));

    let seed = "def:compiler:module.nz::fn core_module";
    let (v, size, ms) = task(Intent::Test, seed);
    let decl = &packet("compiler/emit.nz", "module.nz::fn core_module")
        .target
        .source;
    let mut checks = 0;
    let mut entries = Vec::new();
    for (name, path, find, block, killers) in &catalogue {
        if path != "compiler/module.nz" {
            continue;
        }
        entries.push(entry(name, block));
        let at = text.find(find.as_str()).unwrap() as u32;
        let inside = at >= decl.start && at + find.len() as u32 <= decl.end;
        for k in killers {
            let present = v["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["id"] == k.as_str());
            if inside {
                assert!(present, "{k}");
                checks += 1;
            }
        }
    }
    assert!(checks > 0);
    let mut naive = entries;
    naive.push(file("compiler/module.nz"));
    rows.push(scenario(
        "E tests of core_module",
        naive,
        v,
        size,
        ms,
        checks,
    ));

    rows
}
