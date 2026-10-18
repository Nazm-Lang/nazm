//! N81: the backend contract v2, stage one — bytes, ownership and a printed LIR.
//!
//! `docs/architecture.md` §7.82. What the commands show: `nazm lir` prints the target's data layout
//! and every unit's symbols, layouts and ownership, the same bytes every time; offsets follow the
//! canonical field order at natural alignment, an enum's payload follows its tag word; a pointer is
//! four bytes on wasm32 and eight elsewhere, and nothing else moves; ownership is closed under
//! containment; an unknown target and a program that does not check are refused; and the tables
//! are the ones the Cranelift backend lays values out by, so a program using every shape runs the
//! same on both backends.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROGRAM: &str = "struct Q { flag: Bool, name: Str, n: Int }\n\
struct H { a: Ints, b: Strs }\n\
struct Plain { x: Int, y: Bool }\n\
enum S { Num(n: Int), Pair(q: Q, k: Int), Empty }\n\
enum T { One(x: Int), Two }\n\
fn val(s: S) -> Int { match s { S.Num(n: n) => n, S.Pair(q: q, k: k) => (if q.flag { str_len(q.name) } else { 0 }) + q.n + k, S.Empty() => 0, } }\n\
fn tee(t: T) -> Int { match t { T.One(x: x) => x, T.Two() => 7, } }\n\
fn main(out: OutCap) -> Int {\n\
    let h = H(a: ints_new(), b: strs_new());\n\
    ints_push(h.a, 5);\n\
    strs_push(h.b, \"hello\");\n\
    let p = Plain(x: 3, y: false);\n\
    print(int_to_str(val(S.Pair(q: Q(flag: true, name: \"abc\", n: 10), k: 4))));\n\
    print(int_to_str(tee(T.One(x: 2)) + tee(T.Two()) + p.x + ints_get(h.a, 0) + str_len(strs_get(h.b, 0))));\n\
    0\n\
}\n";

fn program() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), PROGRAM).unwrap();
    d
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn lir(dir: &Path, target: &str) -> Value {
    let o = nazm(dir, &["lir", "--target", target, "p.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

fn find<'a>(doc: &'a Value, kind: &str, name: &str) -> &'a Value {
    doc["units"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|u| u[kind].as_array().unwrap().iter())
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no {kind} {name}"))
}

fn offsets(s: &Value) -> Vec<(String, u64)> {
    s["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["type"].as_str().unwrap().to_owned(),
                f["offset"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_tables_are_the_same_bytes_every_time() {
    let d = program();
    let a = nazm(
        d.path(),
        &["lir", "--target", "x86_64-unknown-linux-gnu", "p.nz"],
    );
    let b = nazm(
        d.path(),
        &["lir", "--target", "x86_64-unknown-linux-gnu", "p.nz"],
    );
    assert!(a.status.success());
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(String::from_utf8_lossy(&a.stdout).lines().count(), 1);
    let doc: Value = serde_json::from_slice(&a.stdout).unwrap();
    assert_eq!(doc["schema"], "nazm.lir/2");
    assert_eq!(doc["target"], "x86_64-unknown-linux-gnu");
    // N105: every unit carries the instructions both backends translate, with their digest.
    for u in doc["units"].as_array().unwrap() {
        let text = u["lir"]["text"].as_str().unwrap();
        assert!(text.contains("\nfunction @"), "{text}");
        let digest = u["lir"]["digest"].as_str().unwrap();
        assert_eq!(digest.len(), 64, "{digest}");
    }
    let ops = nazm(
        d.path(),
        &[
            "lir",
            "--ops",
            "--target",
            "x86_64-unknown-linux-gnu",
            "p.nz",
        ],
    );
    let texts: String = doc["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| format!("{}\n", u["lir"]["text"].as_str().unwrap()))
        .collect();
    assert_eq!(String::from_utf8_lossy(&ops.stdout), texts);
}

/// R9: `nazm lir --ops --debug` lowers as `nazm build --debug` does, so the positions a code
/// generator makes line tables from — each function's `at` and every `loc` — are printed;
/// without `--debug` there are none.
#[test]
fn debug_ops_print_positions_and_plain_ops_do_not() {
    let d = program();
    let plain = nazm(d.path(), &["lir", "--ops", "p.nz"]);
    let debug = nazm(d.path(), &["lir", "--ops", "--debug", "p.nz"]);
    assert!(plain.status.success() && debug.status.success());
    let plain = String::from_utf8_lossy(&plain.stdout);
    let debug = String::from_utf8_lossy(&debug.stdout);
    let positions = |t: &str, k: &str| t.lines().filter(|l| l.trim_start().starts_with(k)).count();
    assert_eq!(positions(&plain, "loc "), 0, "{plain}");
    assert_eq!(positions(&plain, "at "), 0, "{plain}");
    assert!(positions(&debug, "loc ") > 0, "{debug}");
    assert!(positions(&debug, "at ") > 0, "{debug}");
    // The files the positions name, by name.
    assert!(
        debug
            .lines()
            .any(|l| l.starts_with("; file ") && l.ends_with("p.nz")),
        "{debug}"
    );
    assert!(!plain.contains("; file "), "{plain}");
}

#[test]
fn fields_follow_the_canonical_order_at_natural_alignment() {
    let d = program();
    let doc = lir(d.path(), "aarch64-apple-darwin");
    // `flag`, `n`, `name`: canonical by name, each aligned, the whole a multiple of 8.
    let q = find(&doc, "records", "Q");
    assert_eq!(
        offsets(q),
        [("Bool".into(), 0), ("Int".into(), 8), ("Str".into(), 16)]
    );
    assert_eq!(
        (q["size"].as_u64(), q["align"].as_u64()),
        (Some(40), Some(8))
    );
    let plain = find(&doc, "records", "Plain");
    assert_eq!(offsets(plain), [("Int".into(), 0), ("Bool".into(), 8)]);
    assert_eq!(plain["size"], 16);
    // An enum: the tag word at 0, the payload area at 8, as large as its largest variant.
    let s = find(&doc, "enums", "S");
    assert_eq!(
        (s["tag"].as_u64(), s["payload"].as_u64()),
        (Some(0), Some(8))
    );
    let pair = s["variants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Pair")
        .unwrap();
    assert_eq!(offsets(pair), [("Int".into(), 0), ("Q".into(), 8)]);
    assert_eq!(pair["size"], 48);
    assert_eq!(s["size"], 56);
    let data = &doc["data_layout"];
    assert_eq!(data["pointer"], 8);
    assert_eq!(
        (
            data["str"]["pointer"].as_u64(),
            data["str"]["length"].as_u64(),
            data["str"]["owner"].as_u64()
        ),
        (Some(0), Some(8), Some(16))
    );
}

#[test]
fn a_pointer_is_four_bytes_on_wasm32_and_nothing_else_moves() {
    let d = program();
    let host = lir(d.path(), "aarch64-unknown-linux-gnu");
    let wasm = lir(d.path(), "wasm32-unknown-unknown");
    assert_eq!(wasm["data_layout"]["pointer"], 4);
    // Two handles: one word each on a 64-bit target, four bytes each on wasm32.
    let (h64, h32) = (find(&host, "records", "H"), find(&wasm, "records", "H"));
    assert_eq!(offsets(h64), [("Ints".into(), 0), ("Strs".into(), 8)]);
    assert_eq!(h64["size"], 16);
    assert_eq!(offsets(h32), [("Ints".into(), 0), ("Strs".into(), 4)]);
    assert_eq!(h32["size"], 4 + 4);
    // A record of an `Int` and a `Bool` does not depend on the pointer.
    assert_eq!(
        find(&host, "records", "Plain"),
        find(&wasm, "records", "Plain")
    );
    // A `Str` is still 24 bytes, its length still 8: the pointer and owner narrow, padded.
    assert_eq!(wasm["data_layout"]["str"]["size"], 24);
}

#[test]
fn ownership_is_closed_under_containment() {
    let d = program();
    let doc = lir(d.path(), "aarch64-apple-darwin");
    assert_eq!(
        find(&doc, "records", "Q")["owns"],
        true,
        "a `Str` field owns"
    );
    assert_eq!(find(&doc, "records", "H")["owns"], true, "a handle owns");
    assert_eq!(find(&doc, "records", "Plain")["owns"], false);
    assert_eq!(
        find(&doc, "enums", "S")["owns"],
        true,
        "through `Pair`'s `Q`"
    );
    assert_eq!(find(&doc, "enums", "T")["owns"], false);
}

#[test]
fn an_unknown_target_and_a_program_that_does_not_check_are_refused() {
    let d = program();
    let o = nazm(
        d.path(),
        &["lir", "--target", "riscv64-unknown-linux-gnu", "p.nz"],
    );
    assert_eq!(o.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&o.stderr)
        .contains("is not a target this compiler generates code for"));
    std::fs::write(d.path().join("bad.nz"), "fn main() -> Int { \"x\" }\n").unwrap();
    let o = nazm(d.path(), &["lir", "bad.nz"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
}

#[test]
fn every_shape_in_the_tables_runs_the_same_on_both_backends() {
    // The Cranelift backend lays every value out by these tables (N81); LLVM lays its own
    // aggregates out. The program builds every shape printed above and must agree with the
    // interpreter on both.
    let d = program();
    let run = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "17220\n");
    for backend in ["llvm", "cranelift"] {
        let o = nazm(
            d.path(),
            &["build", "--backend", backend, "p.nz", "-o", backend],
        );
        assert!(
            o.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let native = Command::new(d.path().join(crate::common::exe_name(backend)))
            .output()
            .unwrap();
        assert_eq!(native.stdout, run.stdout, "{backend}");
    }
}

/// R3 (6.3.6): a generic channel's helper is named by its element's key, as a vector's is, not by
/// the checker's channel id — so `Chan[Int]`'s is the same symbol whatever other channel types a
/// program makes first, and a pipeline reading Core IR can derive it.
#[test]
fn a_generic_channels_helper_is_named_by_its_element() {
    let only = "fn main(out: OutCap) -> Int {\n\
    let cs = vec_new[Chan[Int]]();\n\
    vec_push(cs, chan_new_of[Int](1));\n\
    print(int_to_str(vec_len(cs)));\n\
    0\n\
}\n";
    let after_others = "fn main(out: OutCap) -> Int {\n\
    let s = chan_new_of[Str](1);\n\
    let b = chan_new_of[Bool](1);\n\
    chan_close_of(s);\n\
    chan_close_of(b);\n\
    let cs = vec_new[Chan[Int]]();\n\
    vec_push(cs, chan_new_of[Int](1));\n\
    print(int_to_str(vec_len(cs)));\n\
    0\n\
}\n";
    for source in [only, after_others] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("p.nz"), source).unwrap();
        let o = nazm(d.path(), &["lir", "--ops", "p.nz"]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("@nz.h.vel.chan.Int"), "{text}");
        let by_id = text
            .split(".chan.")
            .skip(1)
            .any(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        assert!(!by_id, "{text}");
    }
}
