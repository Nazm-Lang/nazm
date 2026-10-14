//! R1c, L2c: `@derive(toml)` (`docs/spec.md`, *Derived serialisation*, **TOML**).
//!
//! A deployment of nested records — an array of tables, an internally tagged enum, an externally
//! tagged one beside JSON, an absent `Option`, `rename_all`, a `default` — written as TOML, read
//! back to the same text, read from hand-written TOML, refused with its path where malformed, the
//! same under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; the
//! repository's own package manifest read through a derived record; and what TOML cannot carry.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROGRAM: &str = r#####"@derive(json, toml)
pub enum Mode { Fast, Safe, Custom(level: Int), }

@derive(toml)
@codec(tag = "type")
pub enum Store { Memory, Disk(path: Str, size_mb: Int), }

@derive(toml)
pub struct Server { host: Str, port: UInt16, }

@derive(json, toml)
@codec(rename_all = "kebab-case")
pub struct Config {
    app_name: Str,
    version: Float64,
    debug: Bool,
    owner: Option[Str],
    tags: Vec[Str],
    mode: Mode,
    retry_after: Mode,
}

@derive(toml)
pub struct Deploy {
    config: Config,
    servers: Vec[Server],
    store: Store,
    @codec(default)
    replicas: Int,
}

fn show(out: OutCap, s: Str) -> Int ! { io } { print(str_concat(s, "\n")) }

fn main(out: OutCap) -> Int ! { io } {
    let tags = vec_new[Str]();
    vec_push(tags, "web");
    let servers = vec_new[Server]();
    vec_push(servers, Server(host: "a.example", port: UInt16(8080)));
    vec_push(servers, Server(host: "b.example", port: UInt16(8081)));
    let c = Config(app_name: "demo", version: 1.5, debug: false, owner: Option[Str].None(), tags: tags, mode: Mode.Fast(), retry_after: Mode.Custom(level: 3));
    let d = Deploy(config: c, servers: servers, store: Store.Disk(path: "/var/data", size_mb: 512), replicas: 2);
    let text = deploy_to_toml(d);
    print(text);
    show(out, "---");
    match deploy_from_toml(text) {
        Result.Ok(value: e) => show(out, if deploy_to_toml(e) == text { "round trip: same text" } else { "round trip: DIFFERENT" }),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    show(out, config_to_json(c));
    match deploy_from_toml("[config]\napp-name = \"x\"\nversion = 2\ndebug = true\ntags = []\nmode = \"Safe\"\nretry-after = { Custom = { level = 9 } }\nowner = \"me\"\n\n[[servers]]\nhost = \"h\"\nport = 1\n\n[store]\ntype = \"Memory\"\n") {
        Result.Ok(value: e) => show(out, deploy_to_toml(e)),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    match deploy_from_toml("[config]\napp-name = \"x\"\nversion = 1.0\ndebug = true\ntags = [1]\nmode = \"Safe\"\nretry-after = \"Fast\"\n[store]\ntype = \"Memory\"\nservers = []\n") {
        Result.Ok(value: e) => show(out, "WRONG"),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    match deploy_from_toml("servers = [{ host = \"h\", port = 70000 }]\n[config]\napp-name = \"x\"\nversion = 1.0\ndebug = true\ntags = []\nmode = \"Safe\"\nretry-after = \"Fast\"\n[store]\ntype = \"Memory\"\n") {
        Result.Ok(value: e) => show(out, "WRONG"),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    0
}
"#####;

const EXPECTED: &str = r#####"replicas = 2

[config]
app-name = "demo"
version = 1.5
debug = false
tags = ["web"]
mode = "Fast"

[config.retry-after]

[config.retry-after.Custom]
level = 3

[[servers]]
host = "a.example"
port = 8080

[[servers]]
host = "b.example"
port = 8081

[store]
type = "Disk"
path = "/var/data"
size_mb = 512
---
round trip: same text
{"app-name":"demo","version":1.5,"debug":false,"owner":null,"tags":["web"],"mode":"Fast","retry-after":{"Custom":{"level":3}}}
replicas = 0

[config]
app-name = "x"
version = 2.0
debug = true
owner = "me"
tags = []
mode = "Safe"

[config.retry-after]

[config.retry-after.Custom]
level = 9

[[servers]]
host = "h"
port = 1

[store]
type = "Memory"

error: $.config.tags[0]: expected a string
error: $.servers[0].port: 70000 is outside `UInt16`
0
"#####;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn a_derived_toml_codec_agrees_everywhere() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("t.nz"), PROGRAM).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "t.nz"][..]),
        ("lir oracle", &["lir", "--run", "t.nz"][..]),
    ] {
        runs.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "t.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        runs.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, r) in runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, EXPECTED, "{label}");
        if label != "lir oracle" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {r:?}"));
            assert!(
                line.split_whitespace()
                    .filter(|w| w.starts_with("live="))
                    .all(|w| w == "live=0"),
                "{label}: {line}"
            );
        }
    }
}

#[test]
fn a_package_manifest_reads_through_a_derived_record() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("examples/packages/app/nazm.toml")).unwrap();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.toml"), manifest).unwrap();
    std::fs::write(
        d.path().join("m.nz"),
        "@derive(toml)\nstruct Package { name: Str, version: Str, main: Option[Str], }\n\n\
         @derive(toml)\nstruct Manifest { schema: Str, package: Package, }\n\n\
         fn main(io: IoCap) -> Int ! {io} {\n    \
         match manifest_from_toml(read_file(\"nazm.toml\")) {\n        \
         Result.Ok(value: m) => { print(f\"{m.schema} {m.package.name} {m.package.version}\\n\"); 0 },\n        \
         Result.Err(error: e) => { print(f\"{e}\\n\"); 1 },\n    }\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "--interpret", "m.nz"]);
    assert_eq!(r.stdout, "nazm.package/1 app 0.1.0\n0\n", "{r:?}");
}

#[test]
fn what_toml_cannot_carry_is_refused() {
    let d = tempfile::tempdir().unwrap();
    for (src, want) in [
        (
            "@derive(toml)\nstruct A { xs: Vec[Option[Int]], }\n",
            "error[N0619]: `@derive(toml)` cannot carry the field `xs`: TOML has no `null`",
        ),
        (
            "@derive(toml)\nstruct B { o: Option[Option[Int]], }\n",
            "error[N0619]: `@derive(toml)` cannot carry the field `o`: TOML has no `null`",
        ),
        (
            "@derive(toml)\nstruct C { n: UInt64, }\n",
            "error[N0619]: `@derive(toml)` cannot carry the field `n`: `UInt64` has values past `Int`",
        ),
        (
            "struct Plain { n: Int, }\n@derive(json, toml)\nstruct D { p: Plain, }\n",
            "error[N0619]: `@derive(json)` cannot carry the field `p`: `Plain` does not derive `json`",
        ),
    ] {
        std::fs::write(d.path().join("r.nz"), format!("{src}fn main() -> Int {{ 0 }}\n")).unwrap();
        let r = nazm(d.path(), &["check", "r.nz"]);
        assert_ne!(r.code, Some(0), "{src}");
        let errors: Vec<&str> = r.stderr.lines().filter(|l| l.starts_with("error")).collect();
        assert!(!errors.is_empty() && errors[0].starts_with(want), "{src}: {errors:?}");
    }
}
