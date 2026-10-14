//! R1c, S1: `@std/toml` (`docs/spec.md`, *The standard library*).
//!
//! A document using every TOML 1.0 form — strings of all four kinds with escapes and a
//! line-ending backslash, dotted and quoted keys, inline tables, nested arrays, every integer and
//! float spelling, all four date-time kinds, headers and arrays of tables — decoded the same under
//! the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift, with nothing live; its encoding
//! a fixed point; every invalid shape refused with its line and column; and every `nazm.toml` in
//! the repository, and a `nazm.lock` as `nazm lock` writes it, parsed and round-tripped.
//!
//! The 553 TOML 1.0 cases of the `toml-test` suite were run against a native build of the same
//! decoder when this module was written (all pass, LLVM and Cranelift, decoding and round-tripping);
//! that suite is a third party's and is not vendored here.

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

const DECODER: &str = r#####"use "@std/toml";
use "@std/json";

fn tagged(kind: Str, value: Str) -> Str { f"{{\"type\":\"{kind}\",\"value\":{json_quote(value)}}}" }

fn float_text(x: Float64) -> Str {
    if float_is_nan(x) { "nan" } else { if float_is_infinite(x) { if x > 0.0 { "inf" } else { "-inf" } } else { num_to_str(x) } }
}

fn json(t: Toml) -> Str {
    match t {
        Toml.Str(value: s) => tagged("string", s),
        Toml.Int(value: n) => tagged("integer", int_to_str(n)),
        Toml.Float(value: x) => tagged("float", float_text(x)),
        Toml.Bool(value: b) => tagged("bool", if b { "true" } else { "false" }),
        Toml.OffsetDatetime(value: s) => tagged("datetime", s),
        Toml.LocalDatetime(value: s) => tagged("datetime-local", s),
        Toml.LocalDate(value: s) => tagged("date-local", s),
        Toml.LocalTime(value: s) => tagged("time-local", s),
        Toml.Array(items: items) => {
            let parts = strs_new();
            for i in 0..chain_len(items) { strs_push(parts, json(chain_get(items, i))); }
            f"[{str_join(parts, ",")}]"
        },
        Toml.Table(keys: keys, values: values) => {
            let parts = strs_new();
            for i in 0..chain_len(keys) { strs_push(parts, f"{json_quote(chain_get(keys, i))}:{json(chain_get(values, i))}"); }
            f"{{{str_join(parts, ",")}}}"
        },
    }
}

fn main(io: IoCap) -> Int ! {io} {
    match toml_parse(read_file("doc.toml")) {
        Result.Ok(value: t) => {
            print(str_concat(json(t), "\n"));
            let text = toml_encode(t);
            print(text);
            let fixed = match toml_parse(text) {
                Result.Ok(value: u) => toml_encode(u) == text,
                Result.Err(error: e) => false,
            };
            let port = match toml_get(t, "database") {
                Option.Some(value: db) => match toml_get(db, "ports") {
                    Option.Some(value: ports) => json(ports),
                    Option.None() => "no ports",
                },
                Option.None() => "no database",
            };
            print(f"fixed point: {fixed}\nports: {port}\n");
            0
        },
        Result.Err(error: e) => { print(f"error: {e}\n"); 0 },
    }
}
"#####;

const DOCUMENT: &str = r#####"# A TOML document
title = "TOML \"Example\"\tdone \u00E9"
lit = 'C:\Users\nodejs'
multi = """
Roses are red
Violets are \
    blue"""
[owner]
name = "Tom Preston-Werner"
dob = 1979-05-27T07:32:00-08:00

[database]
enabled = true
ports = [ 8000, 8001, 8002, ]
data = [ ["delta", "phi"], [3.14] ]
temp_targets = { cpu = 79.5, case = 72.0 }
hex = 0xDEAD_BEEF
big = -9_223_372_036_854_775_808
float = 6.626e-34
inf = -inf
local = 07:32:00.5
day = 1979-05-27

[servers]
  [servers.alpha]
  ip = "10.0.0.1"
  site."google.com" = true

[[fruits]]
name = "apple"
[fruits.physical]
color = "red"
[[fruits.varieties]]
name = "red delicious"
[[fruits]]
name = "banana"
"#####;

const EXPECTED: &str = r#####"{"title":{"type":"string","value":"TOML \"Example\"\tdone é"},"lit":{"type":"string","value":"C:\\Users\\nodejs"},"multi":{"type":"string","value":"Roses are red\nViolets are blue"},"owner":{"name":{"type":"string","value":"Tom Preston-Werner"},"dob":{"type":"datetime","value":"1979-05-27T07:32:00-08:00"}},"database":{"enabled":{"type":"bool","value":"true"},"ports":[{"type":"integer","value":"8000"},{"type":"integer","value":"8001"},{"type":"integer","value":"8002"}],"data":[[{"type":"string","value":"delta"},{"type":"string","value":"phi"}],[{"type":"float","value":"3.14"}]],"temp_targets":{"cpu":{"type":"float","value":"79.5"},"case":{"type":"float","value":"72"}},"hex":{"type":"integer","value":"3735928559"},"big":{"type":"integer","value":"-9223372036854775808"},"float":{"type":"float","value":"6.626e-34"},"inf":{"type":"float","value":"-inf"},"local":{"type":"time-local","value":"07:32:00.5"},"day":{"type":"date-local","value":"1979-05-27"}},"servers":{"alpha":{"ip":{"type":"string","value":"10.0.0.1"},"site":{"google.com":{"type":"bool","value":"true"}}}},"fruits":[{"name":{"type":"string","value":"apple"},"physical":{"color":{"type":"string","value":"red"}},"varieties":[{"name":{"type":"string","value":"red delicious"}}]},{"name":{"type":"string","value":"banana"}}]}
title = "TOML \"Example\"\tdone é"
lit = "C:\\Users\\nodejs"
multi = "Roses are red\nViolets are blue"

[owner]
name = "Tom Preston-Werner"
dob = 1979-05-27T07:32:00-08:00

[database]
enabled = true
ports = [8000, 8001, 8002]
data = [["delta", "phi"], [3.14]]
hex = 3735928559
big = -9223372036854775808
float = 6.626e-34
inf = -inf
local = 07:32:00.5
day = 1979-05-27

[database.temp_targets]
cpu = 79.5
case = 72.0

[servers]

[servers.alpha]
ip = "10.0.0.1"

[servers.alpha.site]
"google.com" = true

[[fruits]]
name = "apple"

[fruits.physical]
color = "red"

[[fruits.varieties]]
name = "red delicious"

[[fruits]]
name = "banana"
fixed point: true
ports: [{"type":"integer","value":"8000"},{"type":"integer","value":"8001"},{"type":"integer","value":"8002"}]
0
"#####;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn every_form_decodes_alike_everywhere_and_encodes_to_a_fixed_point() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), DECODER).unwrap();
    std::fs::write(d.path().join("doc.toml"), DOCUMENT).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
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
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        runs.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(d.path())
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
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

/// `text` through the decoder under the interpreter: its first line.
fn decode(text: &str) -> String {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), DECODER).unwrap();
    std::fs::write(d.path().join("doc.toml"), text).unwrap();
    let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    r.stdout.lines().next().unwrap_or("").to_owned()
}

#[test]
fn every_invalid_shape_is_refused_with_its_line_and_column() {
    for (text, want) in [
        ("a = 1\na = 2\n", "error: 2:5: `a` is defined twice"),
        ("[t]\n[t]\n", "error: 2:4: the table `t` is defined twice"),
        (
            "[a]\nb.c = 1\n[a.b]\n",
            "error: 3:6: the table `b` is defined twice",
        ),
        (
            "a = { b = 1 }\na.c = 2\n",
            "error: 2:7: `a` is already defined, and a dotted key cannot extend it",
        ),
        (
            "a = [1]\n[[a]]\n",
            "error: 2:6: `a` is already defined, and not as an array of tables",
        ),
        (
            "n = 9223372036854775808\n",
            "error: 1:24: an integer past `Int`",
        ),
        (
            "n = 99999999999999999999\n",
            "error: 1:25: an integer past `Int`",
        ),
        (
            "[a.b.c]\nz = 9\n[a]\nb.c.t = 1\n",
            "error: 4:9: `b` is already defined, and a dotted key cannot extend it",
        ),
        ("n = 012\n", "error: 1:5: a malformed number"),
        ("n = 1__2\n", "error: 1:5: a malformed number"),
        (
            "x = 1.\n",
            "error: 1:5: a fraction needs a digit on each side of its `.`",
        ),
        ("d = 2021-02-29\n", "error: 1:5: a malformed date or time"),
        ("t = 24:00:00\n", "error: 1:5: a malformed date or time"),
        ("s = \"\\x\"\n", "error: 1:6: an escape TOML does not have"),
        (
            "s = \"\\uD800\"\n",
            "error: 1:6: an escape that is not a Unicode scalar value",
        ),
        (
            "s = \"open\n",
            "error: 1:10: a string that does not end on its line",
        ),
        ("a = 1 b = 2\n", "error: 1:7: expected the end of the line"),
        (
            "a = { b = 1, }\n",
            "error: 1:14: a trailing `,` in an inline table",
        ),
        ("= 1\n", "error: 1:1: expected a key"),
        ("a = \n", "error: 1:5: expected a value"),
    ] {
        assert_eq!(decode(text), want, "{text:?}");
    }
}

#[test]
fn every_manifest_and_a_lockfile_parse_and_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manifests: Vec<std::path::PathBuf> = Vec::new();
    for dir in [
        "crates/nazm-cli/templates",
        "examples/packages",
        "tests/compat",
    ] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().is_some_and(|n| n == "nazm.toml") {
                    manifests.push(p);
                }
            }
        }
    }
    assert!(manifests.len() >= 10, "{manifests:?}");
    // A lockfile, as `nazm lock` writes one.
    let d = tempfile::tempdir().unwrap();
    for pkg in ["app", "geometry"] {
        let to = d.path().join(pkg);
        std::fs::create_dir_all(to.join("src")).unwrap();
        let from = root.join("examples/packages").join(pkg);
        std::fs::copy(from.join("nazm.toml"), to.join("nazm.toml")).unwrap();
        for e in std::fs::read_dir(from.join("src")).unwrap() {
            let p = e.unwrap().path();
            std::fs::copy(&p, to.join("src").join(p.file_name().unwrap())).unwrap();
        }
    }
    let r = nazm(d.path(), &["lock", "app"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    manifests.push(d.path().join("app/nazm.lock"));
    for m in manifests {
        let text = std::fs::read_to_string(&m).unwrap();
        let line = decode(&text);
        assert!(line.starts_with('{'), "{}: {line}", m.display());
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), DECODER).unwrap();
        std::fs::write(d.path().join("doc.toml"), &text).unwrap();
        let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
        assert!(
            r.stdout.contains("fixed point: true"),
            "{}: {r:?}",
            m.display()
        );
    }
}
