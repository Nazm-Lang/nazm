//! R1c, L2: `@derive(json)` (`docs/spec.md`, *Derived serialisation*).
//!
//! A record of every field kind the derive carries — integers in and out of range, a float, a
//! string, `Option`, `Vec`, `List` of itself, a derived enum with and without payloads — encoded,
//! decoded back to the same text, refused with its path where malformed, and described by a JSON
//! Schema with `$defs`, the same under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
//! Cranelift with nothing live; a derived type of another module, plain and qualified; and every
//! refusal at the attribute or the field.
//!
//! R1c, L2b: `@codec(…)` — `rename_all`, `deny_unknown`, an enum's `tag` and `untagged`, a field's
//! `rename`, `default` and `omit_none`, a variant's `rename` — encoded, decoded and described alike
//! everywhere, every misuse refused alone, and a field's attribute formatted on its own line.
//!
//! The schema was also checked, when the derive was written, by an independent JSON Schema
//! (2020-12) validator: it is one, the encoding satisfies it, and a value out of a field's range,
//! a member of the wrong type and a missing member each fail it.

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

const PROGRAM: &str = r#####"@derive(json)
pub enum Shape {
    Empty,
    Circle(r: Float64),
    Rect(w: Int, h: Int),
}

@derive(json)
pub struct Point {
    x: Int,
    small: UInt8,
    label: Option[Str],
    tags: Vec[Str],
    shape: Shape,
    kids: List[Point],
    ok: Bool,
}

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, "\n")) }

fn main(out: OutCap) -> Int ! {io} {
    let tags = vec_new[Str]();
    vec_push(tags, "a");
    vec_push(tags, "b\"q");
    let leaf = Point(x: 2, small: UInt8(7), label: Option[Str].Some(value: "leaf"), tags: vec_new[Str](), shape: Shape.Empty(), kids: list_empty[Point](), ok: false);
    let kids = vec_new[Point]();
    vec_push(kids, leaf);
    let p = Point(x: -1, small: UInt8(255), label: Option[Str].None(), tags: tags, shape: Shape.Rect(w: 3, h: 4), kids: list_from_vec(kids), ok: true);
    let text = point_to_json(p);
    show(out, text);
    match point_from_json(text) {
        Result.Ok(value: q) => show(out, if point_to_json(q) == text { "decoded: same text" } else { "decoded: DIFFERENT" }),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    match point_from_json("{\"x\": 1, \"small\": 300, \"tags\": [], \"shape\": \"Empty\", \"kids\": [], \"ok\": true}") {
        Result.Ok(value: q) => show(out, "WRONG: accepted"),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    match point_from_json("{\"x\": 1, \"small\": 3, \"tags\": [1], \"shape\": {\"Circle\": {\"r\": 2}}, \"kids\": [], \"ok\": true}") {
        Result.Ok(value: q) => show(out, "WRONG: accepted"),
        Result.Err(error: e) => show(out, f"error: {e}"),
    };
    show(out, shape_to_json(Shape.Circle(r: 1.5)));
    show(out, point_json_schema());
    0
}
"#####;

const CODEC: &str = r#####"@derive(json)
@codec(rename_all = "camelCase", deny_unknown)
pub struct User {
    @codec(rename = "id")
    user_id: Int,
    display_name: Str,
    @codec(omit_none)
    email: Option[Str],
    @codec(default)
    tags: Vec[Str],
}

@derive(json)
@codec(tag = "kind")
pub enum Event { Started, Moved(x: Int, y: Int), }

@derive(json)
@codec(untagged)
pub enum Value { Nothing, Num(n: Int), Text(s: Str, @codec(default) lang: Str), }

@derive(json)
@codec(rename_all = "SCREAMING_SNAKE_CASE")
pub enum Level { LowPower, @codec(rename = "hi") High, }

fn show(out: OutCap, s: Str) -> Int ! { io } { print(str_concat(s, "\n")) }

fn user(out: OutCap, text: Str) -> Int ! { io } {
    match user_from_json(text) {
        Result.Ok(value: u) => show(out, f"ok: {user_to_json(u)}"),
        Result.Err(error: e) => show(out, f"err: {e}"),
    }
}

fn main(out: OutCap) -> Int ! { io } {
    let u = User(user_id: 7, display_name: "Ann", email: Option[Str].None(), tags: vec_new[Str]());
    show(out, user_to_json(u));
    user(out, "{\"id\": 1, \"displayName\": \"B\"}");
    user(out, "{\"id\": 1, \"displayName\": \"B\", \"email\": \"b@x\", \"tags\": [\"t\"]}");
    user(out, "{\"id\": 1, \"displayName\": \"B\", \"extra\": 1}");
    user(out, "{\"id\": 1}");
    show(out, event_to_json(Event.Started()));
    show(out, event_to_json(Event.Moved(x: 1, y: 2)));
    match event_from_json("{\"kind\": \"Moved\", \"x\": 3, \"y\": 4}") { Result.Ok(value: e) => show(out, event_to_json(e)), Result.Err(error: e) => show(out, e), };
    match event_from_json("{\"kind\": \"Jumped\"}") { Result.Ok(value: e) => show(out, "WRONG"), Result.Err(error: e) => show(out, e), };
    show(out, value_to_json(Value.Num(n: 5)));
    show(out, value_to_json(Value.Text(s: "hi", lang: "en")));
    show(out, value_to_json(Value.Nothing()));
    match value_from_json("{\"s\": \"yo\"}") { Result.Ok(value: v) => show(out, value_to_json(v)), Result.Err(error: e) => show(out, e), };
    match value_from_json("{\"n\": 9}") { Result.Ok(value: v) => show(out, value_to_json(v)), Result.Err(error: e) => show(out, e), };
    match value_from_json("{\"q\": true}") { Result.Ok(value: v) => show(out, "WRONG"), Result.Err(error: e) => show(out, e), };
    show(out, level_to_json(Level.LowPower()));
    show(out, level_to_json(Level.High()));
    show(out, user_json_schema());
    show(out, event_json_schema());
    show(out, value_json_schema());
    0
}
"#####;

const CODEC_EXPECTED: &str = r#####"{"id":7,"displayName":"Ann","tags":[]}
ok: {"id":1,"displayName":"B","tags":[]}
ok: {"id":1,"displayName":"B","email":"b@x","tags":["t"]}
err: $: unknown member `extra`
err: $: no member `displayName`
{"kind":"Started"}
{"kind":"Moved","x":1,"y":2}
{"kind":"Moved","x":3,"y":4}
$: `Jumped` is not a variant of `Event`
{"n":5}
{"s":"hi","lang":"en"}
"Nothing"
{"s":"yo","lang":""}
{"n":9}
$: matches no variant of `Value`
"LOW_POWER"
"hi"
{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/User","$defs":{"User":{"type":"object","properties":{"id":{"type":"integer"},"displayName":{"type":"string"},"email":{"anyOf":[{"type":"string"},{"type":"null"}]},"tags":{"type":"array","items":{"type":"string"}}},"required":["id","displayName"],"additionalProperties":false}}}
{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/Event","$defs":{"Event":{"oneOf":[{"type":"object","properties":{"kind":{"const":"Started"}},"required":["kind"]},{"type":"object","properties":{"kind":{"const":"Moved"},"x":{"type":"integer"},"y":{"type":"integer"}},"required":["kind","x","y"]}]}}}
{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/Value","$defs":{"Value":{"anyOf":[{"const":"Nothing"},{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]},{"type":"object","properties":{"s":{"type":"string"},"lang":{"type":"string"}},"required":["s"]}]}}}
0
"#####;

const EXPECTED: &str = r#####"{"x":-1,"small":255,"label":null,"tags":["a","b\"q"],"shape":{"Rect":{"w":3,"h":4}},"kids":[{"x":2,"small":7,"label":"leaf","tags":[],"shape":"Empty","kids":[],"ok":false}],"ok":true}
decoded: same text
error: $.small: 300 is outside `UInt8`
error: $.tags[0]: expected a string
{"Circle":{"r":1.5}}
{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/Point","$defs":{"Point":{"type":"object","properties":{"x":{"type":"integer"},"small":{"type":"integer","minimum":0,"maximum":255},"label":{"anyOf":[{"type":"string"},{"type":"null"}]},"tags":{"type":"array","items":{"type":"string"}},"shape":{"$ref":"#/$defs/Shape"},"kids":{"type":"array","items":{"$ref":"#/$defs/Point"}},"ok":{"type":"boolean"}},"required":["x","small","tags","shape","kids","ok"]},"Shape":{"oneOf":[{"const":"Empty"},{"type":"object","properties":{"Circle":{"type":"object","properties":{"r":{"type":"number"}},"required":["r"]}},"required":["Circle"],"additionalProperties":false},{"type":"object","properties":{"Rect":{"type":"object","properties":{"w":{"type":"integer"},"h":{"type":"integer"}},"required":["w","h"]}},"required":["Rect"],"additionalProperties":false}]}}}
0
"#####;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn a_derived_codec_and_schema_agree_everywhere() {
    everywhere(PROGRAM, EXPECTED);
}

#[test]
fn a_shaped_encoding_agrees_everywhere() {
    everywhere(CODEC, CODEC_EXPECTED);
}

/// `program` under every implementation prints `expected`, leaving nothing live.
fn everywhere(program: &str, expected: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("d.nz"), program).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "d.nz"][..]),
        ("lir oracle", &["lir", "--run", "d.nz"][..]),
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
                "d.nz",
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
        assert_eq!(r.stdout, expected, "{label}");
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
fn a_derived_type_of_another_module_is_carried_plain_and_qualified() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("geo.nz"),
        "@derive(json)\npub struct Pt { x: Int, y: Int, }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"geo.nz\";\nuse \"geo.nz\" as g;\n\n@derive(json)\nstruct Line { a: Pt, b: g::Pt, }\n\n\
         fn main(out: OutCap) -> Int ! {io} {\n    \
         print(str_concat(line_to_json(Line(a: Pt(x: 1, y: 2), b: Pt(x: 3, y: 4))), \"\\n\"));\n    \
         print(str_concat(line_json_schema(), \"\\n\"));\n    0\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "--interpret", "main.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(
        r.stdout,
        "{\"a\":{\"x\":1,\"y\":2},\"b\":{\"x\":3,\"y\":4}}\n\
         {\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"$ref\":\"#/$defs/Line\",\
         \"$defs\":{\"Line\":{\"type\":\"object\",\"properties\":{\"a\":{\"$ref\":\"#/$defs/Pt\"},\
         \"b\":{\"$ref\":\"#/$defs/Pt\"}},\"required\":[\"a\",\"b\"]},\"Pt\":{\"type\":\"object\",\
         \"properties\":{\"x\":{\"type\":\"integer\"},\"y\":{\"type\":\"integer\"}},\"required\":[\"x\",\"y\"]}}}\n0\n"
    );
}

#[test]
fn what_a_derive_cannot_carry_is_refused_where_it_is_written() {
    let d = tempfile::tempdir().unwrap();
    for (src, want) in [
        (
            "@derive(json)\nstruct A { n: UInt64, }\n",
            "error[N0619]: `@derive(json)` cannot carry the field `n`: `UInt64` has values past `Int`",
        ),
        (
            "@derive(json)\nstruct B[T] { v: T, }\n",
            "error[N0619]: `B` is generic, and `@derive(json)` is for a type that is not",
        ),
        (
            "@derive(yaml)\nstruct C { n: Int, }\n",
            "error[N0619]: `@derive` takes the formats it derives",
        ),
        (
            "@derive(json)\nfn f() -> Int { 0 }\n",
            "error[N0619]: `@derive` marks a `struct` or an `enum`, not a function",
        ),
        (
            "@derive(json)\nstruct F { b: Bytes, }\n",
            "error[N0619]: `@derive(json)` cannot carry the field `b`: `Bytes` has no JSON form",
        ),
        (
            "struct Plain { n: Int, }\n@derive(json)\nstruct G { p: Option[Plain], }\n",
            "error[N0619]: `@derive(json)` cannot carry the field `p`: `Plain` does not derive `json`",
        ),
        (
            "@derive(json)\nstruct H { n: Int, }\nfn h_to_json(v: H) -> Str { \"\" }\n",
            "error[N0203]: `h_to_json` is defined more than once in this module",
        ),
    ] {
        std::fs::write(d.path().join("r.nz"), format!("{src}fn main() -> Int {{ 0 }}\n")).unwrap();
        let r = nazm(d.path(), &["check", "r.nz"]);
        assert_ne!(r.code, Some(0), "{src}");
        let errors: Vec<&str> = r.stderr.lines().filter(|l| l.starts_with("error")).collect();
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
        assert!(errors[0].starts_with(want), "{src}: {errors:?}");
    }
}

#[test]
fn every_misuse_of_codec_is_refused_alone() {
    let d = tempfile::tempdir().unwrap();
    for (src, want) in [
        (
            "@codec(deny_unknown)\nstruct A { n: Int, }\n",
            "error[N0619]: `@codec` shapes a derived encoding, and `A` derives none",
        ),
        (
            "@derive(json)\n@codec(tag = \"k\")\nstruct B { n: Int, }\n",
            "error[N0619]: `tag` and `untagged` shape an enum",
        ),
        (
            "@derive(json)\n@codec(tag = \"k\", untagged)\nenum C { X, }\n",
            "error[N0619]: an enum is tagged or untagged, not both",
        ),
        (
            "@derive(json)\n@codec(rename_all = \"Title Case\")\nstruct D { n: Int, }\n",
            "error[N0619]: `@codec` does not take this here",
        ),
        (
            "@derive(json)\nstruct E { @codec(omit_none) n: Int, }\n",
            "error[N0619]: `omit_none` leaves out a `None`, and `n` is not an `Option`",
        ),
        (
            "@derive(json)\nstruct F { @codec(default) p: Box[Int], }\n",
            "error[N0619]: `default` gives a missing `p` its type's empty value, and this type has none",
        ),
        (
            "@derive(json)\nstruct G { @codec(rename = \"a\") x: Int, a: Int, }\n",
            "error[N0619]: `a` names two members of one object",
        ),
        (
            "@derive(json)\n@codec(tag = \"x\")\nenum H { V(x: Int), }\n",
            "error[N0619]: `x` names two members of one object",
        ),
        (
            "@derive(json)\nstruct I { @cfg(os = \"linux\") n: Int, }\n",
            "error[N0619]: `@cfg` is not written before a field",
        ),
        (
            "@derive(json)\nenum J { @codec(default) X, }\n",
            "error[N0619]: `@codec` does not take this here",
        ),
        (
            "@derive(json)\nstruct K { @codec(rename = \"a\", rename = \"b\") n: Int, }\n",
            "error[N0619]: `rename` is written twice",
        ),
        (
            "@derive(json)\n@codec(rename_all = \"camelCase\")\nenum L { FooBar, @codec(rename = \"fooBar\") Other, }\n",
            "error[N0619]: two variants of `L` are both named `fooBar`",
        ),
        (
            "@codec(untagged)\nfn g() -> Int { 0 }\n",
            "error[N0619]: `@codec` shapes a derived encoding: it marks a type, a field or a variant, not a function",
        ),
    ] {
        std::fs::write(d.path().join("r.nz"), format!("{src}fn main() -> Int {{ 0 }}\n")).unwrap();
        let r = nazm(d.path(), &["check", "r.nz"]);
        assert_ne!(r.code, Some(0), "{src}");
        let errors: Vec<&str> = r.stderr.lines().filter(|l| l.starts_with("error")).collect();
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
        assert!(errors[0].starts_with(want), "{src}: {errors:?}");
    }
}

#[test]
fn a_fields_attribute_is_formatted_on_its_own_line_and_inline_where_its_list_is() {
    let d = tempfile::tempdir().unwrap();
    let written = "@derive(json)\nstruct S {\n    @codec(rename = \"a\") x: Int,\n    \
                   @codec(omit_none)   y: Option[Int],\n}\n\n@derive(json)\n\
                   enum E { A, @codec(rename = \"b\")   B, C(n: Int, @codec(default) m: Int), }\n";
    let want = "@derive(json)\nstruct S {\n    @codec(rename = \"a\")\n    x: Int,\n    \
                @codec(omit_none)\n    y: Option[Int],\n}\n\n@derive(json)\n\
                enum E { A, @codec(rename = \"b\") B, C(n: Int, @codec(default) m: Int), }\n";
    std::fs::write(d.path().join("f.nz"), written).unwrap();
    let r = nazm(d.path(), &["fmt", "f.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(d.path().join("f.nz")).unwrap(),
        want
    );
    let r = nazm(d.path(), &["fmt", "--check", "f.nz"]);
    assert_eq!(
        r.code,
        Some(0),
        "formatting it again changes nothing: {r:?}"
    );
}
