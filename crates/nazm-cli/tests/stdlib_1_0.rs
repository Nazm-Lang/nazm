//! N84: the standard library 1.0.
//!
//! `docs/architecture.md` §7.85. What the programs show, on the interpreter, LLVM at `-O0` and
//! `-O2` and Cranelift, with everything reclaimed: every new function behaves as documented at its
//! edges; the stable sort keeps equal elements in order and does what its function does; a JSON
//! round trip, and every malformed input an `Err` naming where; the four representative programs
//! give their expected output; and the public surface is exactly `library/std/API-1.0`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// `(tier, exit code, stdout, stderr)` for `prog.nz` in `dir` on every tier.
fn tiers(dir: &Path, args: &[&str]) -> Vec<(String, Option<i32>, String, String)> {
    let path = dir.join("prog.nz");
    let o = Command::new(NAZM)
        .current_dir(dir)
        .arg("run")
        .arg(&path)
        .arg("--")
        .args(args)
        .output()
        .unwrap();
    let mut out = vec![(
        "interpreter".to_owned(),
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let b = Command::new(NAZM)
            .current_dir(dir)
            .args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
            ])
            .arg(&path)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            b.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&b.stderr)
        );
        let mut c = common::bounded_exe(&exe);
        c.current_dir(dir)
            .args(args)
            .env(nazm_lir::MEMORY_VARIABLE, "1");
        let r = c.output().unwrap();
        out.push((
            label.to_owned(),
            r.status.code(),
            String::from_utf8_lossy(&r.stdout).into_owned(),
            String::from_utf8_lossy(&r.stderr).into_owned(),
        ));
    }
    out
}

/// `src` as `prog.nz` gives `expected` on every tier, exits 0, and reclaims everything natively.
fn agrees(src: &str, expected: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    for (label, code, out, err) in tiers(d.path(), &[]) {
        assert_eq!(code, Some(0), "{label}: {err}");
        assert_eq!(out, expected, "{label}");
        if label != "interpreter" {
            let report = err
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: {err}"));
            assert_eq!(report.matches("live=0").count(), 4, "{label}: {report}");
        }
    }
}

#[test]
fn text_1_0_behaves_as_documented_at_its_edges() {
    agrees(
        "use \"@std/text\";\n\
         fn main(io: IoCap) -> Int ! { io } {\n\
             print(text_to_upper(\"ab-Cz\")); print(\"|\"); print(text_to_lower(\"AB-cZ\")); print(\"|\");\n\
             print(text_replace(\"a,b,,c\", \",\", \";\")); print(\"|\"); print(text_replace(\"abc\", \"\", \"x\")); print(\"|\");\n\
             print(int_to_str(text_count(\"aaaa\", \"aa\"))); print(int_to_str(text_count(\"a\", \"\"))); print(\"|\");\n\
             print(text_pad_left(\"7\", 3, \"0\")); print(text_pad_left(\"1234\", 3, \"0\")); print(text_pad_right(\"ab\", 4, \".\")); print(text_pad_right(\"ab\", 4, \"\")); print(\"|\");\n\
             let ls = text_lines(\"x\\r\\ny\\n\\nz\\n\"); print(int_to_str(strs_len(ls))); print(strs_get(ls, 1)); print(int_to_str(strs_len(text_lines(\"\")))); print(\"|\");\n\
             print(int_to_str(text_compare(\"ab\", \"abc\") + 10 * text_compare(\"b\", \"a\") + 100 * text_compare(\"q\", \"q\"))); print(\"|\");\n\
             print(text_trim_start(\" \\t a \")); print(\"|\"); print(text_trim_end(\" a \\n\")); print(\"|\");\n\
             print(if text_is_digit(48) && !text_is_digit(58) && text_is_alpha(90) && !text_is_alpha(91) { \"T\" } else { \"F\" });\n\
             0 }\n",
        "AB-CZ|ab-cz|a;b;;c|abc|20|0071234ab..ab|4y0|9|a | a|T0\n",
    );
}

#[test]
fn the_sort_is_stable_does_what_its_function_does_and_the_seq_helpers_clamp() {
    agrees(
        "use \"@std/seq\";\nuse \"@std/sort\";\n\
         struct P { k: Int, tag: Str }\n\
         fn main(io: IoCap) -> Int ! { io } {\n\
             let v = vec_new[P]();\n\
             vec_push(v, P(k: 2, tag: \"a\")); vec_push(v, P(k: 1, tag: \"b\")); vec_push(v, P(k: 2, tag: \"c\")); vec_push(v, P(k: 0, tag: \"d\")); vec_push(v, P(k: 1, tag: \"e\"));\n\
             let s = vec_sort_by(v, fn(x: P, y: P) -> Bool { x.k < y.k });\n\
             let mut i = 0; while i < vec_len(s) { print(vec_get(s, i).tag); i = i + 1; } print(\"|\");\n\
             print(vec_get(v, 0).tag); print(\"|\");\n\
             let e = vec_sort_by(vec_new[Int](), fn(a: Int, b: Int) -> Bool { a < b }); print(int_to_str(vec_len(e))); print(\"|\");\n\
             let xs = ints_new(); ints_push(xs, 5); ints_push(xs, 0 - 2); ints_push(xs, 9); ints_push(xs, 5);\n\
             let ys = ints_sort(xs); i = 0; while i < ints_len(ys) { print(int_to_str(ints_get(ys, i))); print(\",\"); i = i + 1; } print(\"|\");\n\
             let ss = strs_new(); strs_push(ss, \"b\"); strs_push(ss, \"ab\"); strs_push(ss, \"a\"); strs_push(ss, \"\");\n\
             let zs = strs_sort(ss); i = 0; while i < strs_len(zs) { print(\"[\"); print(strs_get(zs, i)); print(\"]\"); i = i + 1; } print(\"|\");\n\
             print(int_to_str(vec_len(vec_slice(s, 0 - 5, 99)))); print(int_to_str(vec_len(vec_slice(s, 4, 2)))); print(\"|\");\n\
             let r = vec_reverse(vec_slice(s, 1, 4)); i = 0; while i < vec_len(r) { print(vec_get(r, i).tag); i = i + 1; } print(\"|\");\n\
             let asked = vec_any(s, fn(x: P) -> Bool ! { io } { print(\"?\"); x.k == 1 });\n\
             print(if asked { \"T\" } else { \"F\" });\n\
             print(if vec_all(vec_concat(s, s), fn(x: P) -> Bool { x.k >= 0 }) { \"T\" } else { \"F\" });\n\
             print(if vec_all(vec_new[P](), fn(x: P) -> Bool { false }) { \"T\" } else { \"F\" });\n\
             0 }\n",
        "dbeac|a|0|-2,5,5,9,|[][a][ab][b]|50|aeb|??TTT0\n",
    );
}

#[test]
fn fmt_and_num_behave_as_documented_at_their_edges() {
    agrees(
        "use \"@std/fmt\";\n\
         use \"@std/num\";\n\
         fn o(x: Option[Int]) -> Str { match x { Option.Some(value: v) => int_to_str(v), Option.None() => \"none\" } }\n\
         fn main(io: IoCap) -> Int ! { io } {\n\
             print(fmt_int_zero(0 - 42, 5)); print(\"|\"); print(fmt_int_zero(123456, 3)); print(\"|\"); print(fmt_int_width(7, 3)); print(\"|\");\n\
             print(fmt_hex(255)); print(\"|\"); print(fmt_hex(0 - 1)); print(\"|\"); print(fmt_hex(0)); print(\"|\"); print(fmt_bool(true)); print(\"|\");\n\
             let xs = ints_new(); ints_push(xs, 1); ints_push(xs, 2); print(fmt_ints(xs, \"+\")); print(\"|\");\n\
             let ss = strs_new(); print(fmt_list(ss, \",\", \"[\", \"]\")); print(\"|\");\n\
             print(int_to_str(num_gcd(0 - 12, 18))); print(int_to_str(num_gcd(0, 0))); print(\"|\");\n\
             print(int_to_str(num_clamp(15, 0, 10))); print(int_to_str(num_clamp(5, 10, 0))); print(\"|\");\n\
             print(o(num_pow(2, 62))); print(\"|\"); print(o(num_pow(2, 63))); print(\"|\"); print(o(num_pow(0 - 3, 3))); print(\"|\"); print(o(num_pow(2, 0 - 1))); print(\"|\");\n\
             print(int_to_str(num_abs(0 - 9) + num_sign(0 - 4) + num_min(3, 1) + num_max(3, 1)));\n\
             0 }\n",
        "-00042|123456|  7|ff|ffffffffffffffff|0|true|1+2|[]|60|1010|4611686018427387904|none|-27|none|120\n",
    );
}

#[test]
fn map_path_fs_time_test_and_chan_behave_as_documented() {
    agrees(
        "use \"@std/map\";\nuse \"@std/path\";\nuse \"@std/fs\";\nuse \"@std/time\";\nuse \"@std/test\";\nuse \"@std/chan\";\n\
         fn producer(io: IoCap, c: Chan[Int]) -> Int ! { io } {\n\
             let xs = vec_new[Int](); vec_push(xs, 1); vec_push(xs, 2); vec_push(xs, 3);\n\
             let n = chan_send_all_ints(c, xs); chan_close_of(c); n }\n\
         fn check() -> Result[Int, Str] ! {} {\n\
             let a = test_int(\"sum\", 1 + 1, 2)?;\n\
             let b = test_str(\"name\", \"x\", \"y\")?;\n\
             Result[Int, Str].Ok(value: a) }\n\
         fn lines2() -> Strs { let s = strs_new(); strs_push(s, \"one\"); strs_push(s, \"two\"); s }\n\
         fn main(io: IoCap, tm: TimeCap, sp: SpawnCap) -> Int ! { io, spawn } {\n\
             let m = map_new[Int]();\n\
             let r1 = map_set(m, \"b\", 2); let r2 = map_set(m, \"a\", 1); let r3 = map_set(m, \"c\", 3); let r4 = map_set(m, \"b\", 20);\n\
             print(str_join(map_keys(m), \",\")); print(if r4 && !r1 { \"R\" } else { \"?\" }); print(\"|\");\n\
             print(match map_get(m, \"b\") { Option.Some(value: v) => int_to_str(v), Option.None() => \"none\" }); print(\"|\");\n\
             print(match map_remove(m, \"a\") { Option.Some(value: v) => int_to_str(v), Option.None() => \"none\" });\n\
             print(match map_remove(m, \"zz\") { Option.Some(value: v) => int_to_str(v), Option.None() => \"none\" }); print(int_to_str(map_len(m))); print(\"|\");\n\
             print(path_join(\"a/b\", \"c.txt\")); print(\" \"); print(path_join(\"a/\", \"/abs\")); print(\" \"); print(path_base(\"/x/y/z.tar.gz\")); print(\" \"); print(path_dir(\"/x\")); print(\" \"); print(path_dir(\"z\")); print(\" \");\n\
             print(path_ext(\"/x/y/z.tar.gz\")); print(\" \"); print(path_stem(\"z.tar.gz\")); print(\" \"); print(path_ext(\".profile\")); print(\" \");\n\
             print(path_normalize(\"/a/./b/../../../c//d\")); print(\" \"); print(path_normalize(\"x/../../y\")); print(\" \"); print(path_normalize(\"\")); print(\"|\");\n\
             let w = fs_write_lines(io, \"t.txt\", lines2());\n\
             print(match fs_read_lines(io, \"t.txt\") { Result.Ok(value: ls) => int_to_str(strs_len(ls)), Result.Err(error: e) => e }); print(\" \");\n\
             print(match fs_read(io, \"nope.txt\") { Result.Ok(value: s) => s, Result.Err(error: e) => e }); print(\"|\");\n\
             let d = time_deadline(tm, 0); print(if time_passed(tm, d) && time_since(tm, time_ms(tm)) >= 0 { \"passed\" } else { \"not\" }); print(\"|\");\n\
             print(match check() { Result.Ok(value: v) => \"ok\", Result.Err(error: e) => e }); print(\"|\");\n\
             let c = chan_new_of[Int](1);\n\
             let mut got = vec_new[Int]();\n\
             scope { spawn producer(io, c); got = chan_drain_ints(c); }\n\
             print(int_to_str(vec_len(got)));\n\
             0 }\n",
        "a,b,cR|20|1none2|a/b/c.txt /abs z.tar.gz / . gz z.tar  /c/d ../y .|2 no such file: nope.txt|passed|name: expected \"y\", found \"x\"|30\n",
    );
}

#[test]
fn json_round_trips_and_names_where_malformed_text_went_wrong() {
    agrees(
        "use \"@std/json\";\n\
         fn show(r: Result[JsonDoc, Str]) -> Str { match r { Result.Ok(value: d) => json_encode(d), Result.Err(error: e) => str_concat(\"ERR \", e) } }\n\
         fn main(io: IoCap) -> Int ! { io } {\n\
             print(show(json_parse(\" {\\\"a\\\": [1, -2, true, null, \\\"x\\\\ny\\\\u0041\\\\u00e9\\\"], \\\"b\\\": {}, \\\"c\\\": [] } \"))); print(\"|\");\n\
             print(show(json_parse(\"[1, 2\"))); print(\"|\");\n\
             print(show(json_parse(\"1.5\"))); print(\"|\");\n\
             print(show(json_parse(\"{\\\"a\\\" 1}\"))); print(\"|\");\n\
             print(show(json_parse(\"-9223372036854775808\"))); print(\"|\");\n\
             print(show(json_parse(\"9223372036854775808\"))); print(\"|\");\n\
             print(show(json_parse(\"[1] x\"))); print(\"|\");\n\
             print(show(json_parse(\"01\"))); print(\"|\");\n\
             print(show(json_parse(\"\"))); print(\"|\");\n\
             let b = json_new(); let ks = strs_new(); strs_push(ks, \"k\"); strs_push(ks, \"q\\\"t\"); let vs = ints_new(); ints_push(vs, json_add_int(b, 7)); ints_push(vs, json_add_bool(b, false));\n\
             let built = json_with_root(b, json_add_object(b, ks, vs));\n\
             print(json_encode(built)); print(\"|\");\n\
             print(show(json_parse(json_encode(built)))); print(\"|\");\n\
             let r = json_root(built);\n\
             print(json_kind(built, r)); print(json_kind(built, 999));\n\
             print(match json_get(built, r, \"k\") { Option.Some(value: n) => match json_int(built, n) { Option.Some(value: v) => int_to_str(v), Option.None() => \"?\" }, Option.None() => \"none\" });\n\
             0 }\n",
        "{\"a\":[1,-2,true,null,\"x\\nyA\u{e9}\"],\"b\":{},\"c\":[]}|ERR expected , or ] at byte 5|ERR a number with a fraction or exponent is not supported at byte 1|ERR expected : at byte 5|-9223372036854775808|ERR a number outside Int's range at byte 19|ERR text after the value at byte 4|ERR a number with a leading zero at byte 2|ERR unexpected end at byte 0|{\"k\":7,\"q\\\"t\":false}|{\"k\":7,\"q\\\"t\":false}|object70\n",
    );
}

/// A representative program from `examples/std/`, with its data, run on every tier.
fn representative(name: &str, args: &[&str], expected: &str) {
    let d = tempfile::tempdir().unwrap();
    let dir = repo().join("examples/std");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        let to = if p.file_name().unwrap() == format!("{name}.nz").as_str() {
            d.path().join("prog.nz")
        } else {
            d.path().join(p.file_name().unwrap())
        };
        std::fs::copy(&p, to).unwrap();
    }
    for (label, code, out, err) in tiers(d.path(), args) {
        assert_eq!(code, Some(0), "{name} {label}: {err}");
        assert_eq!(out, expected, "{name} {label}");
    }
}

#[test]
fn a_command_line_tool_counts_words_with_the_library_alone() {
    representative("wordfreq", &["words.txt"], "  4 the\n  2 a\n  2 cat\n0\n");
}

#[test]
fn a_data_tool_sums_a_table_with_the_library_alone() {
    representative(
        "tablesum",
        &[],
        "{apple=80, fig=20, pear=30} skipped 1\n0\n",
    );
}

#[test]
fn a_concurrent_service_runs_on_the_library_alone() {
    representative("service", &[], "1 2 3 4\n0\n");
}

#[test]
fn a_package_tool_edits_a_json_manifest_with_the_library_alone() {
    representative(
        "pkgtool",
        &["extra"],
        "src/main.nz nz\nsrc/util/fmt.nz nz\nsrc/tool \n{\"name\":\"demo\",\"deps\":[\"core\",\"extra\"],\"modules\":[\"src/main.nz\",\"src/util/fmt.nz\",\"src/tool\"]}\n0\n",
    );
}

#[test]
fn every_public_standard_item_is_in_the_1_0_manifest() {
    // The surface, regenerated from the modules exactly as `library/std/API-1.0` was written.
    let std_dir = repo().join("library/std");
    let mut modules: Vec<String> = std::fs::read_dir(&std_dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".nz").map(str::to_owned)
        })
        .collect();
    modules.sort();
    let mut lines = Vec::new();
    for m in &modules {
        let text = std::fs::read_to_string(std_dir.join(format!("{m}.nz"))).unwrap();
        for l in text.lines() {
            if ["pub fn ", "pub struct ", "pub enum ", "pub trait "]
                .iter()
                .any(|p| l.starts_with(p))
            {
                let sig = l.strip_suffix(" {").unwrap_or(l);
                lines.push(format!("@std/{m} {sig}"));
            }
        }
    }
    let manifest = std::fs::read_to_string(std_dir.join("API-1.0")).unwrap();
    let listed: Vec<&str> = manifest
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect();
    assert_eq!(
        listed,
        lines.iter().map(String::as_str).collect::<Vec<_>>(),
        "the public surface moved: 1.x may add a line to library/std/API-1.0, never remove or change one"
    );
    assert!(listed.len() >= 120, "{}", listed.len());
}
