//! N103: re-exports — `pub use "PATH";` and `pub use "PATH" { a, b as c };`.
//!
//! `docs/architecture.md` §7.104. A re-export offers another module's definition under this
//! module's name, and is the same definition: every answer about it — what a call means, what a
//! reference finds, what a persisted interface names, what a rename changes — is the defining
//! module's. Each test states its expected answer rather than comparing two paths that could
//! share a bug.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use nazm_core::sema::key::ModuleKey;
use nazm_core::sema::module::ModuleGraph;
use nazm_core::sema::resolve::{FnId, Resolution};
use nazm_iface::{PersistentUnitInterface, Rehydrated};
use nazm_span::{FileId, SourceMap};
use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const SHAPES: &str = "pub trait Area { fn area(self: Self) -> Int; }\n\
pub struct Sq { s: Int }\n\
impl Area for Sq { fn area(self: Sq) -> Int { self.s * self.s } }\n\
pub fn double(n: Int) -> Int { n * 2 }\n\
fn hidden() -> Int { 9 }\n";
const FACADE: &str = "pub use \"shapes.nz\" { Sq as Square, double as twice, Area };\n";
const ALL: &str = "pub use \"shapes.nz\";\n";
const MAIN: &str = "use \"facade.nz\";\nfn main() -> Int { Square(s: 3).area() + twice(4) }\n";
const MAIN2: &str = "use \"all.nz\";\nfn tw[T: Area](x: T) -> Int { x.area() }\n\
fn main() -> Int { Sq(s: 2).area() + double(1) + tw(Sq(s: 1)) }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    d
}

fn base() -> tempfile::TempDir {
    tree(&[
        ("shapes.nz", SHAPES),
        ("facade.nz", FACADE),
        ("all.nz", ALL),
        ("main.nz", MAIN),
        ("main2.nz", MAIN2),
    ])
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `(code, start, end)` of every diagnostic `nazm check --json` raises.
fn codes(dir: &Path, file: &str) -> Vec<(String, u64, u64)> {
    let o = nazm(dir, &["check", file, "--json"]);
    let mut out: Vec<(String, u64, u64)> = String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|d| {
            Some((
                d["code"].as_str()?.to_owned(),
                d["primary"]["span"]["start"].as_u64()?,
                d["primary"]["span"]["end"].as_u64()?,
            ))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_reexport_offers_the_same_definition_under_its_names() {
    let d = base();
    // Interpreted and native, by both backends: `Square` is `Sq`, `twice` is `double`, and
    // `Area`'s impl reaches the importer through the re-export.
    for (file, want) in [("main.nz", "17\n"), ("main2.nz", "7\n")] {
        let r = nazm(d.path(), &["run", file]);
        assert_eq!(stdout(&r), want, "{}", String::from_utf8_lossy(&r.stderr));
        for backend in ["llvm", "cranelift"] {
            let b = nazm(d.path(), &["build", "--backend", backend, file, "-o", "p"]);
            assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
            let o = Command::new(d.path().join("p")).output().unwrap();
            assert_eq!(stdout(&o), want, "{file} under {backend}");
        }
    }
    // What is not offered is not reached: a re-export brings only what it names.
    std::fs::write(
        d.path().join("only.nz"),
        "use \"facade.nz\";\nfn main() -> Int { double(1) }\n",
    )
    .unwrap();
    assert!(codes(d.path(), "only.nz")
        .iter()
        .any(|(c, _, _)| c == "N0201"));
}

#[test]
fn what_a_reexport_may_not_do_is_refused_by_name() {
    let d = base();
    // A name the module does not export, one it does not have, and one this module offers.
    std::fs::write(
        d.path().join("bad.nz"),
        "pub use \"shapes.nz\" { hidden, nope, double };\npub fn double(n: Int) -> Int { n }\n\
         fn main() -> Int { 0 }\n",
    )
    .unwrap();
    let got = codes(d.path(), "bad.nz");
    assert_eq!(
        got,
        [
            ("N0208".to_owned(), 8, 19),
            ("N0211".to_owned(), 22, 28),
            ("N0211".to_owned(), 30, 34),
            ("N0212".to_owned(), 36, 42),
        ]
    );
    // A trait is offered under its own name only.
    std::fs::write(
        d.path().join("tr.nz"),
        "pub use \"shapes.nz\" { Area as Shape };\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    assert_eq!(codes(d.path(), "tr.nz"), [("N0211".to_owned(), 22, 26)]);
    // A cycle of re-exports.
    std::fs::write(
        d.path().join("cyca.nz"),
        "pub use \"cycb.nz\";\npub fn a() -> Int { 1 }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("cycb.nz"),
        "pub use \"cyca.nz\";\npub fn b() -> Int { 2 }\n",
    )
    .unwrap();
    let got = codes(d.path(), "cyca.nz");
    assert_eq!(got.len(), 2, "{got:?}");
    // Each at its own `pub`.
    assert!(
        got.iter().all(|(c, s, e)| c == "N0213" && e - s == 3),
        "{got:?}"
    );
    // And `as` on the module, which a re-export does not take.
    std::fs::write(
        d.path().join("named.nz"),
        "pub use \"shapes.nz\" as s;\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    assert!(!nazm(d.path(), &["check", "named.nz"]).status.success());
}

#[test]
fn every_reference_is_to_the_defining_module() {
    let d = base();
    let o = nazm(d.path(), &["references", "shapes.nz::fn double", "main.nz"]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let uses: Vec<(String, u64)> = v["uses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["module"].as_str().unwrap().to_owned(),
                u["start"].as_u64().unwrap(),
            )
        })
        .collect();
    // `twice(4)` in `main.nz`, written under the alias, and the name in the re-export list.
    assert!(
        uses.contains(&("main.nz".to_owned(), MAIN.find("twice").unwrap() as u64)),
        "{v}"
    );
    assert!(
        uses.contains(&(
            "facade.nz".to_owned(),
            FACADE.find("double").unwrap() as u64
        )),
        "{v}"
    );
    assert_eq!(v["declaration"]["module"], "shapes.nz");
    // The resolved units say the same, by durable identity: no second definition anywhere.
    let r = stdout(&nazm(d.path(), &["resolve", "main.nz"]));
    assert!(!r.contains("facade.nz::fn twice"), "{r}");
    assert!(!r.contains("facade.nz::record Square"), "{r}");
    assert!(r.contains("shapes.nz::record Sq"), "{r}");
}

#[test]
fn the_persisted_interface_names_the_defining_identity_and_suffices_alone() {
    let d = base();
    let o = nazm(d.path(), &["interface", "facade.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let facade_json = stdout(&o).trim().to_owned();
    let v: Value = serde_json::from_str(&facade_json).unwrap();
    assert_eq!(v["schema"], "nazm.interface/12");
    let twice = v["exports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "twice")
        .unwrap();
    assert_eq!(twice["def"], "shapes.nz::fn double", "{twice}");
    assert_eq!(twice["reexport"], true);
    assert_eq!(
        v["reexported_types"],
        serde_json::json!([["Square", "shapes.nz::record Sq"]])
    );
    // A `/10` reader would have refused this; this one reads it back.
    PersistentUnitInterface::from_json(&facade_json).expect("read back");
    // Every source but the importer's gone: the importer checks from the facade's interface
    // and the prelude's alone.
    for f in ["shapes.nz", "facade.nz", "all.nz"] {
        std::fs::remove_file(d.path().join(f)).unwrap();
    }
    let core = prelude_interface();
    let errors = check_against(
        "use \"facade.nz\";\nfn main() -> Int { Square(s: 3).s + twice(4) }\n",
        &facade_json,
        &core,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    // The negative half: a name the interface does not offer is still undefined.
    let errors = check_against(
        "use \"facade.nz\";\nfn main() -> Int { double(4) }\n",
        &facade_json,
        &core,
    );
    assert!(
        errors.iter().any(|e| e.code.as_str() == "N0201"),
        "{errors:#?}"
    );
}

fn prelude_interface() -> String {
    struct Capture(Option<String>);
    impl nazm_core::Reuse for Capture {
        fn plan(
            &mut self,
            graph: &ModuleGraph,
            interfaces: &[nazm_core::sema::interface::UnitInterface],
            types: &nazm_core::sema::udt::UserTypes,
        ) {
            let core = graph.prelude().expect("a prelude");
            let published = interfaces
                .get(core.index())
                .and_then(|i| PersistentUnitInterface::published(i, graph, types).ok())
                .expect("the prelude publishes an interface");
            self.0 = Some(published.to_json().expect("json"));
        }
        fn must_check(&self, _: nazm_core::sema::module::ModuleId) -> bool {
            true
        }
        fn checked_clean(
            &mut self,
            _: nazm_core::sema::module::ModuleId,
            _: &nazm_core::sema::ModuleFacts,
            _: &nazm_core::sema::resolve::Resolution,
        ) {
        }
    }
    let (map, graph) = nazm_core::one_file("fn main() -> Int { 0 }\n");
    let (program, errors) = nazm_core::__syntax::parse_map(&map);
    assert!(errors.is_empty(), "{errors:#?}");
    let mut capture = Capture(None);
    let (_, errors) = nazm_core::check_reusing(&program, &graph, &mut capture, &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    capture.0.expect("the prelude's interface")
}

fn check_against(a_source: &str, b_json: &str, core_json: &str) -> Vec<nazm_diag::Diagnostic> {
    let map = SourceMap::from_parts(vec![("a.nz".to_owned(), a_source.to_owned())]);
    let (program, parse_errors) = nazm_core::__syntax::parse_map(&map);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");
    let b_iface = PersistentUnitInterface::from_json(b_json).expect("B's interface");
    let core_iface = PersistentUnitInterface::from_json(core_json).expect("the prelude's");
    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(FileId::from_index(1), b_iface.module_key());
    graph.import(a, b, nazm_span::Span::synthetic());
    let core = graph.attach_prelude(FileId::from_index(2));
    let units = [nazm_core::Unit::of(&program, &graph, a)];
    let mut res = Resolution::default();
    let mut errors = Vec::new();
    let mut seen = Rehydrated::default();
    let c = core_iface.rehydrate_into(core, FnId::from_index(8192), res.types_mut(), &mut seen);
    let bb = b_iface.rehydrate_into(b, FnId::from_index(4096), res.types_mut(), &mut seen);
    nazm_core::declare_types(&units, &mut res, &mut errors);
    nazm_core::declare_unit(&units[0], &[&bb, &c], &mut res, &mut errors);
    nazm_core::check_unit(&units[0], &[&bb, &c], &mut res, &mut errors);
    errors
}

/// `(accepted, checked, reused)` from a cached check.
fn check(dir: &Path, file: &str) -> (bool, u64, u64) {
    let o = nazm(dir, &["check", file, "--cache-report", "--json"]);
    let report: Value = stdout(&o)
        .lines()
        .find(|l| l.contains("nazm.check-report/1"))
        .map(|l| serde_json::from_str(l).unwrap())
        .unwrap();
    (
        o.status.success(),
        report["checked"].as_u64().unwrap(),
        report["reused"].as_u64().unwrap(),
    )
}

#[test]
fn reuse_follows_what_a_reexport_offers_and_nothing_else() {
    let d = base();
    // Cold: every module checked — the three project modules and the prelude.
    assert_eq!(check(d.path(), "main.nz"), (true, 4, 0));
    // Unchanged: nothing checked.
    assert_eq!(check(d.path(), "main.nz"), (true, 0, 4));
    // A private body of the defining module: it alone is checked; its interface did not move,
    // so the facade and its importer are reused.
    std::fs::write(
        d.path().join("shapes.nz"),
        SHAPES.replace("{ 9 }", "{ 10 }"),
    )
    .unwrap();
    assert_eq!(check(d.path(), "main.nz"), (true, 1, 3));
    // A re-exported signature: the facade's interface moves with it, so both are checked, and
    // the importer is refused against the new contract.
    std::fs::write(
        d.path().join("shapes.nz"),
        SHAPES
            .replace(
                "pub fn double(n: Int) -> Int",
                "pub fn double(n: Str) -> Int",
            )
            .replace("{ n * 2 }", "{ str_len(n) }"),
    )
    .unwrap();
    let (ok, checked, _) = check(d.path(), "main.nz");
    assert!(!ok);
    assert_eq!(checked, 3);
    std::fs::write(d.path().join("shapes.nz"), SHAPES).unwrap();
    assert!(check(d.path(), "main.nz").0);
    // The alias renamed: the importer that wrote the old name is refused.
    std::fs::write(
        d.path().join("facade.nz"),
        FACADE.replace("as twice", "as thrice"),
    )
    .unwrap();
    assert!(codes(d.path(), "main.nz")
        .iter()
        .any(|(c, _, _)| c == "N0201"));
    // And removed: the type it offered is gone too.
    std::fs::write(
        d.path().join("facade.nz"),
        "pub use \"shapes.nz\" { double as twice };\n",
    )
    .unwrap();
    assert!(codes(d.path(), "main.nz")
        .iter()
        .any(|(c, _, _)| c == "N0305"));
    // Back as it was: accepted, and the warm check reuses what it can.
    std::fs::write(d.path().join("facade.nz"), FACADE).unwrap();
    assert!(check(d.path(), "main.nz").0);
    assert_eq!(check(d.path(), "main.nz"), (true, 0, 4));
}

#[test]
fn a_rename_follows_the_definition_and_keeps_every_alias() {
    let d = base();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    let o = nazm(
        d.path(),
        &[
            "patch",
            "rename",
            "--root",
            "main2.nz",
            "--file",
            "shapes.nz",
            "--byte",
            &SHAPES.find("double").unwrap().to_string(),
            "--new-name",
            "dbl",
            "--json",
        ],
    );
    let p: Value = serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)));
    assert_eq!(p["status"], "available", "{p}");
    std::fs::write(
        d.path().join("plan.json"),
        serde_json::to_string(&p).unwrap(),
    )
    .unwrap();
    let a = nazm(d.path(), &["patch", "apply", "plan.json"]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    let read = |f: &str| std::fs::read_to_string(d.path().join(f)).unwrap();
    assert!(read("shapes.nz").contains("pub fn dbl("));
    // The list names the definition, and is renamed; the alias is the facade's, and is not.
    assert_eq!(read("facade.nz"), FACADE.replace("double as", "dbl as"));
    assert_eq!(read("main.nz"), MAIN);
    // Written by its own name through a re-export of everything: renamed.
    assert_eq!(read("main2.nz"), MAIN2.replace("double(1)", "dbl(1)"));
    assert_eq!(stdout(&nazm(d.path(), &["run", "main.nz"])), "17\n");
    assert_eq!(stdout(&nazm(d.path(), &["run", "main2.nz"])), "7\n");
}

#[test]
fn the_api_document_lists_each_reexport_by_its_definition() {
    let d = base();
    let o = nazm(d.path(), &["doc", "main.nz"]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["schema"], "nazm.api-doc/2");
    let mut got: Vec<(String, String)> = v["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["items"].as_array().unwrap().iter())
        .filter(|i| i["kind"] == "reexport")
        .map(|i| {
            (
                i["name"].as_str().unwrap().to_owned(),
                i["of"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("Area".to_owned(), "shapes.nz::trait Area".to_owned()),
            ("Square".to_owned(), "shapes.nz::record Sq".to_owned()),
            ("twice".to_owned(), "shapes.nz::fn double".to_owned()),
        ]
    );
}

/// `(compiled, reused)` native objects from a build report.
fn build(dir: &Path, root: &str) -> (u64, u64) {
    let o = nazm(dir, &["build", root, "-o", "prog", "--build-report"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = stdout(&o)
        .lines()
        .find(|l| l.contains("nazm.build-report/1"))
        .map(|l| serde_json::from_str(l).unwrap())
        .unwrap();
    (
        v["native"]["compiled"].as_u64().unwrap(),
        v["native"]["reused"].as_u64().unwrap(),
    )
}

#[test]
fn a_warm_build_reuses_what_a_reexport_did_not_change() {
    let d = base();
    let (cold, _) = build(d.path(), "main.nz");
    assert!(cold >= 3, "{cold}");
    // Unchanged: every object reused.
    assert_eq!(build(d.path(), "main.nz"), (0, cold));
    // The defining module's private body: its object alone is rebuilt, and the program still
    // gives its answer.
    std::fs::write(
        d.path().join("shapes.nz"),
        SHAPES.replace("{ 9 }", "{ 10 }"),
    )
    .unwrap();
    let (compiled, reused) = build(d.path(), "main.nz");
    assert_eq!(compiled, 1, "{compiled} compiled, {reused} reused");
    let o = Command::new(d.path().join("prog")).output().unwrap();
    assert_eq!(stdout(&o), "17\n");
    // The re-exported definition's own body: rebuilt, and the importer sees the new answer
    // through the alias.
    std::fs::write(
        d.path().join("shapes.nz"),
        SHAPES.replace("{ n * 2 }", "{ n * 3 }"),
    )
    .unwrap();
    build(d.path(), "main.nz");
    let o = Command::new(d.path().join("prog")).output().unwrap();
    assert_eq!(stdout(&o), "21\n");
}

fn manifest(name: &str, version: &str, main: bool, deps: &[(&str, &str, &str)]) -> String {
    let mut s = format!(
        "schema = \"nazm.package/1\"\n[package]\nname = \"{name}\"\nversion = \"{version}\"\n"
    );
    if main {
        s.push_str("main = \"main.nz\"\n");
    }
    if !deps.is_empty() {
        s.push_str("[dependencies]\n");
        for (n, path, v) in deps {
            s.push_str(&format!(
                "{n} = {{ path = \"{path}\", version = \"{v}\" }}\n"
            ));
        }
    }
    s
}

#[test]
fn a_package_reexports_another_packages_definition_and_reuse_follows_it() {
    let d = tempfile::tempdir().unwrap();
    let w = |name: &str, text: &str| {
        let p = d.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    w("shapes/nazm.toml", &manifest("shapes", "0.3.1", false, &[]));
    w("shapes/src/poly.nz", "pub fn sides(n: Int) -> Int { n }\n");
    w(
        "geo/nazm.toml",
        &manifest("geo", "1.2.0", false, &[("shapes", "../shapes", "0.3.1")]),
    );
    // Across the package boundary: `geo` offers `shapes`' function as its own `count`.
    w(
        "geo/src/lib.nz",
        "pub use \"shapes:poly.nz\" { sides as count };\n",
    );
    w(
        "app/nazm.toml",
        &manifest("app", "0.1.0", true, &[("geo", "../geo", "1.2.0")]),
    );
    w(
        "app/src/main.nz",
        "use \"geo:lib.nz\";\nfn main() -> Int { count(7) }\n",
    );
    let r = nazm(d.path(), &["run", "app"]);
    assert_eq!(stdout(&r), "7\n", "{}", String::from_utf8_lossy(&r.stderr));
    // The identity is the defining package's, not the re-exporting one's.
    let i = nazm(&d.path().join("geo/src"), &["interface", "lib.nz"]);
    let text = stdout(&i);
    assert!(
        text.contains("\"name\":\"count\""),
        "{text}{}",
        String::from_utf8_lossy(&i.stderr)
    );
    assert!(text.contains("poly.nz::fn sides"), "{text}");
    // The defining package's source changes: the program's answer follows.
    w(
        "shapes/src/poly.nz",
        "pub fn sides(n: Int) -> Int { n + 1 }\n",
    );
    assert_eq!(stdout(&nazm(d.path(), &["run", "app"])), "8\n");
    // Its version moves past what `geo` requires: refused, never quietly the old one.
    w("shapes/nazm.toml", &manifest("shapes", "0.4.0", false, &[]));
    assert!(!nazm(d.path(), &["run", "app"]).status.success());
}
