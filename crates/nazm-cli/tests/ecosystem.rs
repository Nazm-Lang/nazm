//! N74: the ecosystem a fresh user meets — templates, API docs, C headers, a publish dry run and an
//! editor client.
//!
//! `docs/architecture.md` §7.76. What the commands show: every template initialises into a project
//! its own README's commands check, test, build and inspect; a library package is checked as its
//! modules; `nazm doc` documents exactly the public definitions with their semantic facts and links
//! back to the source, the same bytes twice; `nazm bindgen` translates only what crosses the C
//! boundary, refuses the rest by name, and what it writes links against real C; a dry-run publish
//! writes nothing and records what the publish then does; the editor client holds no language logic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let o = nazm(dir, args);
    assert!(
        o.status.success(),
        "nazm {args:?} in {}: {}{}",
        dir.display(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn init(root: &Path, name: &str, template: &str) -> std::path::PathBuf {
    ok(root, &["init", name, "--template", template]);
    root.join(name)
}

#[test]
fn every_template_is_a_project_its_own_commands_hold() {
    let d = tempfile::tempdir().unwrap();
    for t in ["cli", "library", "server", "embedded", "wasm", "contract"] {
        let p = init(d.path(), &format!("my-{t}"), t);
        let manifest = std::fs::read_to_string(p.join("nazm.toml")).unwrap();
        assert!(
            manifest.contains(&format!("name = \"my-{t}\"")),
            "{t}: {manifest}"
        );
        assert!(std::fs::read_to_string(p.join("README.md"))
            .unwrap()
            .starts_with(&format!("# my-{t}")));
        assert_eq!(ok(&p, &["check", "."]).trim(), "ok", "{t}");
        assert!(ok(&p, &["test", "tests"]).contains("ok"), "{t}");
        ok(&p, &["lock", "."]);
        // Whatever it is, its API is documented.
        ok(&p, &["doc", ".", "-o", "api"]);
    }
    let at = |t: &str| d.path().join(format!("my-{t}"));
    assert_eq!(ok(&at("cli"), &["run", "."]), "hello, world\n0\n");
    ok(&at("cli"), &["build", ".", "-o", "app"]);
    assert_eq!(
        String::from_utf8_lossy(&Command::new(at("cli").join("app")).output().unwrap().stdout),
        "hello, world\n0\n"
    );
    assert!(ok(&at("cli"), &["inspect", "."]).contains("\"nazm.inspect/2\""));
    assert_eq!(
        ok(&at("server"), &["run", ".", "--", "requests.txt"]),
        "4 requests served\n30\n"
    );
    let usage = nazm(&at("server"), &["run", "."]);
    assert_eq!(
        usage.status.code(),
        Some(2),
        "a missing argument is a usage error"
    );
    for (t, triple) in [
        ("embedded", "aarch64-unknown-none"),
        ("wasm", "wasm32-unknown-unknown"),
    ] {
        ok(
            &at(t),
            &["build", "--target", triple, ".", "--objects", "build"],
        );
        assert!(at(t).join("build/link.txt").is_file(), "{t}");
    }
    let run = ok(
        &at("contract"),
        &["contract", "src/main.nz", "--txs", "txs.txt"],
    );
    let v: serde_json::Value = serde_json::from_str(&run).unwrap();
    assert_eq!(v["schema"], "nazm.contract-run/1");
    // A library has nothing to run, and says so.
    let r = nazm(&at("library"), &["run", "."]);
    assert!(String::from_utf8_lossy(&r.stderr).contains("N0509"));
}

#[test]
fn init_refuses_rather_than_overwrite_or_guess() {
    let d = tempfile::tempdir().unwrap();
    init(d.path(), "taken", "cli");
    let o = nazm(d.path(), &["init", "taken"]);
    assert!(
        !o.status.success()
            && String::from_utf8_lossy(&o.stderr).contains("exists and is not empty")
    );
    let o = nazm(d.path(), &["init", "Not_A_Name"]);
    assert!(
        !o.status.success() && String::from_utf8_lossy(&o.stderr).contains("is not a package name")
    );
    let o = nazm(d.path(), &["init", "x", "--template", "nope"]);
    assert!(
        !o.status.success()
            && String::from_utf8_lossy(&o.stderr)
                .contains("cli, library, server, embedded, wasm, contract")
    );
    assert!(!d.path().join("x").exists());
}

#[test]
fn every_template_file_is_one_init_writes() {
    let d = tempfile::tempdir().unwrap();
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    for t in std::fs::read_dir(&templates).unwrap().flatten() {
        let name = t.file_name().to_string_lossy().into_owned();
        let p = init(d.path(), &format!("t-{name}"), &name);
        let mut stack = vec![t.path()];
        while let Some(dir) = stack.pop() {
            for f in std::fs::read_dir(&dir).unwrap().flatten() {
                if f.path().is_dir() {
                    stack.push(f.path());
                } else {
                    let rel = f.path().strip_prefix(t.path()).unwrap().to_path_buf();
                    assert!(
                        p.join(&rel).is_file(),
                        "{name}: {} is not written by `nazm init`",
                        rel.display()
                    );
                }
            }
        }
    }
}

#[test]
fn a_library_is_checked_as_its_modules_each_error_once() {
    let d = tempfile::tempdir().unwrap();
    let p = init(d.path(), "lib", "library");
    std::fs::write(
        p.join("src/extra.nz"),
        "pub fn bad() -> Int ! {} { true }\n",
    )
    .unwrap();
    std::fs::write(
        p.join("src/uses.nz"),
        "use \"extra.nz\";\npub fn b2() -> Int ! {} { bad() }\n",
    )
    .unwrap();
    let o = nazm(&p, &["check", "."]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert_eq!(e.matches("error[N0300]").count(), 1, "{e}");
    assert!(e.contains("nazm: 1 error"), "{e}");
}

#[test]
fn the_api_document_is_the_public_definitions_with_their_facts_and_sources() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("m.nz"),
        "// A point on the grid.\npub struct Point { x: Int, y: Int }\n\n\
         pub enum Dir { North, To(p: Point) }\n\n\
         // C's adder.\npub extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
         pub extern \"C\" fn nazm_triple(x: Int) -> Int ! {} = \"nazm_triple\" { x * 3 }\n\
         pub fn shout(io: IoCap, s: Str) -> Int ! { io } { print(s); 0 }\n\
         fn hidden() -> Int { 1 }\n\
         fn main(io: IoCap) -> Int ! { io } { shout(io, \"x\") }\n",
    )
    .unwrap();
    let first = ok(d.path(), &["doc", "m.nz"]);
    assert_eq!(
        ok(d.path(), &["doc", "m.nz"]),
        first,
        "the same bytes twice"
    );
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    let items = v["modules"][0]["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Dir", "c_add", "nazm_triple", "shout", "Point"]);
    assert!(!first.contains("hidden") && !first.contains("\"main\""));
    let by = |n: &str| items.iter().find(|i| i["name"] == n).unwrap();
    assert_eq!(by("shout")["capabilities"], serde_json::json!(["IoCap"]));
    assert_eq!(
        by("shout")["effects"]["required"],
        serde_json::json!(["io"])
    );
    assert_eq!(by("c_add")["foreign"], "c_add");
    assert_eq!(by("c_add")["description"], "C's adder.");
    assert_eq!(by("nazm_triple")["export"], "nazm_triple");
    assert_eq!(by("Point")["description"], "A point on the grid.");
    assert_eq!(
        by("Point")["source"],
        serde_json::json!({ "path": "m.nz", "line": 2 })
    );
    assert_eq!(by("Dir")["variants"][1]["fields"], serde_json::json!(["p"]));
    // A budget, so the document stays a thing an agent can read: under 700 bytes a definition.
    assert!(
        first.len() < 700 * items.len(),
        "{} bytes for {} definitions",
        first.len(),
        items.len()
    );
    ok(d.path(), &["doc", "m.nz", "-o", "api"]);
    let md = std::fs::read_to_string(d.path().join("api/api.md")).unwrap();
    assert!(md.contains(&format!(
        "<a id=\"{}\"></a>",
        by("shout")["anchor"].as_str().unwrap()
    )));
    assert!(md.contains("- source: [m.nz:9](m.nz#L9)"), "{md}");
    // A program that does not check has no API to document.
    std::fs::write(
        d.path().join("bad.nz"),
        "pub fn f() -> Int { true }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    assert!(!nazm(d.path(), &["doc", "bad.nz"]).status.success());
}

const HEADER: &str = "#ifndef M_H\n#define M_H\n#include <stdint.h>\n#include <stdbool.h>\n\
#define LIMIT 10\n/* Adds. */\nint64_t m_add(int64_t a, int64_t b);\nbool m_is_even(int64_t n);\n\
int64_t m_len(const char *s);\nint m_small(int x);\nvoid m_log(int64_t level);\n\
const char *m_name(void);\ndouble m_half(double x);\nint64_t m_sum(int64_t n, ...);\n\
void m_on(void (*cb)(int64_t));\nstruct point { int64_t x; int64_t y; };\nint64_t m_buf(char *out);\nint64_t malloc(int64_t n);\nint64_t m_widen(int x);\n#endif\n";

#[test]
fn bindgen_translates_what_crosses_and_refuses_the_rest_by_name() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("m.h"), HEADER).unwrap();
    let o = nazm(d.path(), &["bindgen", "m.h", "-o", "m.nz"]);
    assert_eq!(o.status.code(), Some(1), "something was refused");
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let translated: Vec<&str> = v["translated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["nazm"].as_str().unwrap())
        .collect();
    assert_eq!(
        translated,
        [
            // A complete struct of `int64_t` fields is a C struct since N85, declared first.
            "pub extern \"C\" struct point { x: Int, y: Int }",
            "pub extern \"C\" fn m_add(a: Int, b: Int) -> Int = \"m_add\";",
            "pub extern \"C\" fn m_is_even(n: Int) -> Bool = \"m_is_even\";",
            "pub extern \"C\" fn m_len(s: Str) -> Int = \"m_len\";",
            // And a `const char *` result a copy, which a header never promises is not null.
            "pub extern \"C\" fn m_name() -> Option[Str] = \"m_name\";",
        ]
    );
    let refused = |c: &str| {
        v["refused"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["c"].as_str().unwrap().contains(c))
            .map(|r| r["reason"].as_str().unwrap().to_owned())
            .unwrap_or_else(|| panic!("{c} not refused: {v}"))
    };
    assert!(refused("m_small").contains("`int` is not `int64_t`"));
    assert!(refused("m_log").contains("returns nothing"));
    assert!(refused("m_half").contains("no floating-point"));
    assert!(refused("m_sum").contains("variadic"));
    assert!(refused("m_on").contains("function pointer"));
    assert!(refused("m_buf").contains("`char *` is a pointer"));
    assert!(refused("LIMIT").contains("macro"));
    assert!(refused("malloc").contains("N0383"), "{}", refused("malloc"));
    // A parameter is held as strictly as a result: C's `int` is never declared as `Int`.
    assert!(
        refused("m_widen").contains("parameter 1: `int` is not `int64_t`"),
        "{}",
        refused("m_widen")
    );
    // The include guard and the includes are a header's shape, not refused.
    assert!(v["refused"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| !r["c"].as_str().unwrap().contains("M_H")));
    // Nothing written but what checks.
    let module = std::fs::read_to_string(d.path().join("m.nz")).unwrap();
    assert!(!module.contains("m_small") && module.contains("m_add"));
}

#[test]
fn what_bindgen_writes_links_against_real_c() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("m.h"), "#include <stdint.h>\n#include <stdbool.h>\nint64_t m_add(int64_t a, int64_t b);\nbool m_is_even(int64_t n);\n").unwrap();
    std::fs::write(
        d.path().join("m.c"),
        "#include <stdint.h>\n#include <stdbool.h>\nint64_t m_add(int64_t a, int64_t b) { return a + b; }\nbool m_is_even(int64_t n) { return n % 2 == 0; }\n",
    )
    .unwrap();
    assert_eq!(
        nazm(d.path(), &["bindgen", "m.h", "-o", "m.nz"])
            .status
            .code(),
        Some(0)
    );
    let cc = Command::new("clang")
        .current_dir(d.path())
        .args(["-c", "m.c", "-o", "m.o"])
        .status()
        .unwrap();
    assert!(cc.success());
    std::fs::write(
        d.path().join("main.nz"),
        "use \"m.nz\";\nfn main(f: ForeignCap) -> Int ! { foreign } { if m_is_even(4) { m_add(40, 2) } else { 0 } }\n",
    )
    .unwrap();
    ok(
        d.path(),
        &["build", "main.nz", "-o", "app", "--link", "m.o"],
    );
    let out = Command::new(d.path().join("app")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_dry_run_publish_writes_nothing_and_records_what_the_publish_does() {
    let d = tempfile::tempdir().unwrap();
    init(d.path(), "lib", "library");
    std::fs::create_dir(d.path().join("reg")).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&ok(
        d.path(),
        &["publish", "--registry", "reg", "lib", "--dry-run"],
    ))
    .unwrap();
    assert_eq!(plan["schema"], "nazm.publish-plan/1");
    assert_eq!(
        plan["files"],
        serde_json::json!(["nazm.toml", "src/lib.nz"])
    );
    assert_eq!(
        std::fs::read_dir(d.path().join("reg")).unwrap().count(),
        0,
        "nothing written"
    );
    let o = nazm(d.path(), &["publish", "--registry", "reg", "lib"]);
    assert!(o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(plan["digest"].as_str().unwrap()),
        "the digest the plan named"
    );
    // Refused for the same reason the publish would be.
    let again = nazm(
        d.path(),
        &["publish", "--registry", "reg", "lib", "--dry-run"],
    );
    assert!(
        !again.status.success()
            && String::from_utf8_lossy(&again.stderr).contains("already published")
    );
}

#[test]
fn the_editor_client_starts_the_compilers_server_and_knows_no_language() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/vscode");
    let package: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
    assert_eq!(
        package["contributes"]["languages"][0]["extensions"],
        serde_json::json!([".nz"])
    );
    assert_eq!(
        package["contributes"]["configuration"]["properties"]["nazm.path"]["default"],
        "nazm"
    );
    let client = std::fs::read_to_string(root.join("extension.js")).unwrap();
    assert!(client.contains("args: [\"lsp\"]"));
    // A client, not a second implementation: no more than starting the server takes.
    assert!(
        client.lines().count() < 40,
        "{} lines",
        client.lines().count()
    );
    for f in [
        "launch.example.json",
        "tasks.example.json",
        "language-configuration.json",
    ] {
        let _: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(f)).unwrap()).unwrap();
    }
    assert!(std::fs::read_to_string(root.join("tasks.example.json"))
        .unwrap()
        .contains("nazm build --debug"));
}

#[test]
fn the_capability_inventory_names_every_authority_target_and_command() {
    let d = tempfile::tempdir().unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&ok(d.path(), &["capabilities", "--json"])).unwrap();
    let has = |k: &str, x: &str| v[k].as_array().unwrap().iter().any(|e| e == x);
    for cap in [
        "IoCap",
        "SpawnCap",
        "ForeignCap",
        "VouchCap",
        "TimeCap",
        "MmioCap",
    ] {
        assert!(has("authority", cap), "{cap}");
    }
    for t in ["aarch64-unknown-none", "wasm32-unknown-unknown"] {
        assert!(has("targets", t), "{t}");
    }
    for c in ["init", "doc", "bindgen", "publish", "contract", "repl"] {
        assert!(has("commands", c), "{c}");
    }
}

/// The human `types:` line lists every built-in type a program can write, capability types
/// included, each once. Until 1.0 it printed the checker's vocabulary alone and stopped at
/// `IoCap, SpawnCap`; the JSON keeps `types` and `authority` apart, as its schema fixes.
#[test]
fn the_human_types_line_names_every_capability_type_once() {
    let d = tempfile::tempdir().unwrap();
    let text = ok(d.path(), &["capabilities"]);
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("types: "))
        .unwrap_or_else(|| panic!("a types line: {text}"));
    let types: Vec<&str> = line.split(", ").collect();
    for t in [
        "Int",
        "Bool",
        "Str",
        "Chan",
        "IoCap",
        "SpawnCap",
        "ForeignCap",
        "VouchCap",
        "TimeCap",
        "MmioCap",
        "OutCap",
    ] {
        assert_eq!(
            types.iter().filter(|x| **x == t).count(),
            1,
            "{t} in {line}"
        );
    }
    let v: serde_json::Value =
        serde_json::from_str(&ok(d.path(), &["capabilities", "--json"])).unwrap();
    assert!(
        !v["types"].as_array().unwrap().iter().any(|e| e == "OutCap"),
        "the JSON `types` stays the checker's vocabulary"
    );
}

/// Every command says what it does, in its own help, and names the schema it prints at the
/// version it prints. Until 1.0 `explain-cost`'s description sat on `profile`, so one had two
/// and the other none, and three commands named a schema a major version behind.
#[test]
fn every_command_has_its_own_help_naming_the_schema_it_prints() {
    let d = tempfile::tempdir().unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&ok(d.path(), &["capabilities", "--json"])).unwrap();
    for c in v["commands"].as_array().unwrap() {
        let c = c.as_str().unwrap();
        let help = ok(d.path(), &["help", c]);
        let first = help.lines().next().unwrap_or_default();
        assert!(
            !first.starts_with("Usage:") && !first.trim().is_empty(),
            "`nazm {c}` has no description: {help}"
        );
    }
    for (c, schema, not) in [
        ("explain-cost", "nazm.cost/2", "nazm.profile/"),
        ("profile", "nazm.profile/2", "nazm.cost/"),
        ("explain-flow", "nazm.flow/2", "nazm.flow/1"),
        ("accel", "nazm.accel/2", "nazm.accel/1"),
        ("core-ir", "nazm.core-ir/2", "nazm.core-ir/1"),
    ] {
        let help = ok(d.path(), &["help", c]);
        assert!(help.contains(schema), "`nazm {c}` names {schema}: {help}");
        assert!(
            !help.contains(not),
            "`nazm {c}` does not name {not}: {help}"
        );
    }
}

/// `nazm --version` and `nazm --help` name the release they are: toolchain 1.0.0, language 1.0
/// (`docs/stability.md`). The crate version is the toolchain version; the help's sentence is
/// written by hand and is held to it here.
#[test]
fn the_help_and_version_name_the_release() {
    let d = tempfile::tempdir().unwrap();
    let version = ok(d.path(), &["--version"]);
    assert_eq!(version.trim(), format!("nazm {}", env!("CARGO_PKG_VERSION")));
    let help = ok(d.path(), &["--help"]);
    let language = env!("CARGO_PKG_VERSION")
        .rsplit_once('.')
        .map(|(major_minor, _)| major_minor)
        .unwrap();
    assert!(
        help.contains(&format!(
            "Nazm language {language}, toolchain {}",
            env!("CARGO_PKG_VERSION")
        )),
        "{help}"
    );
    assert!(!help.contains("experimental release"), "{help}");
}
