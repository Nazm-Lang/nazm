//! Gate 2: `Bytes` and `Text`.
//!
//! `docs/general-purpose.md` §5 and §6, and the spec's *Bytes* and *Text*. What only a whole run
//! shows: that a `Bytes` is one buffer every view of which sees every write, read and written in
//! either byte order at every width; that a `Text` is UTF-8 by every way of making one, the
//! validation table agreeing byte for byte; that scalar values iterate the same way; that every
//! failure stops with its code and sentence; that buffers are reclaimed — in the interpreter and
//! in both native backends at both optimisation levels. Every expected line was worked out from
//! the specification and RFC 3629.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program under the interpreter, and built and run by each backend at each level.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["run", "prog.nz"]),
            "nazm run",
        ),
    )];
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
        let mut c = common::bounded_exe(&exe);
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints exactly `expected` — then `main`'s result, `0` — and exits 0.
fn agrees(body: &str, expected: &str) {
    agrees_with("", body, expected);
}

/// [`agrees`], with `items` declared beside `main`.
fn agrees_with(items: &str, body: &str, expected: &str) {
    let expected = format!("{expected}0\n");
    let src = format!(
        "fn show(out: OutCap, s: Str) -> Int ! {{io}} {{ print(str_concat(s, \"\\n\")) }}\n\
         {items}\n\
         fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n0\n}}\n"
    );
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(0), "{label}: {}\n{src}", r.stderr);
        assert_eq!(r.stdout, expected, "{label}\n{src}");
    }
}

/// The codes `nazm check` refuses a program with.
fn refused(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["check", "--json", "prog.nz"]);
    assert_eq!(r.code, Some(1), "expected a refusal: {r:?}\n{src}");
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|d| d["severity"] == "error")
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_bytes_is_one_buffer_that_every_view_shares() {
    agrees(
        r#"
let b = bytes_new(8);
show(out, num_to_str(bytes_get(b, 7)));
bytes_set(b, 0, 72);
let v = bytes_slice(b, 4, 8);
let w = bytes_slice(v, 1, 3);
bytes_set(w, 0, 9);
show(out, num_to_str(bytes_get(b, 5)));
show(out, int_to_str(bytes_len(v) + bytes_len(w)));
let c = bytes_from_str("hello");
show(out, int_to_str(bytes_copy(c, bytes_from_str("J"))));
show(out, bytes_to_str(c));
let s = bytes_to_str(c);
bytes_set(c, 0, 77);
show(out, s);
show(out, bytes_to_str(c));
bytes_copy(bytes_slice(c, 1, 5), bytes_slice(c, 0, 4));
show(out, bytes_to_str(c));
show(out, int_to_str(bytes_len(bytes_new(0))));
"#,
        "0\n9\n6\n1\nJello\nJello\nMello\nMMell\n0\n",
    );
}

#[test]
fn every_width_reads_and_writes_in_either_byte_order_everywhere() {
    agrees(
        r#"
let b = bytes_new(16);
bytes_write_be(b, 0, UInt32(0xDEADBEEF));
show(out, num_to_str(bytes_get(b, 0)));
show(out, num_to_str(bytes_read_le[UInt32](b, 0)));
show(out, num_to_str(bytes_read_be[UInt32](b, 0)));
show(out, num_to_str(bytes_read_be[UInt16](b, 1)));
show(out, num_to_str(bytes_read_le[Int8](b, 0)));
bytes_write_le(b, 0, Int16(-2));
show(out, num_to_str(bytes_read_le[Int16](b, 0)));
show(out, num_to_str(bytes_read_be[UInt16](b, 0)));
bytes_write_le(b, 2, -9223372036854775807 - 1);
show(out, int_to_str(bytes_read_le[Int](b, 2)));
show(out, num_to_str(bytes_get(b, 9)));
bytes_write_be(b, 8, UInt64(18446744073709551615));
show(out, num_to_str(bytes_read_le[UInt64](b, 8)));
bytes_write_le(b, 0, -1.5);
show(out, num_to_str(bytes_read_le[Float64](b, 0)));
show(out, num_to_str(bytes_get(b, 7)));
bytes_write_be(b, 4, Float32(0.1));
show(out, num_to_str(bytes_read_be[Float32](b, 4)));
show(out, num_to_str(bytes_read_be[UInt32](b, 4)));
bytes_write_le(b, 12, Int32(-123456789));
show(out, num_to_str(bytes_read_le[Int32](b, 12)));
"#,
        "222\n4022250974\n3735928559\n44478\n-34\n-2\n65279\n-9223372036854775808\n128\n\
         18446744073709551615\n-1.5\n191\n0.1\n1036831949\n-123456789\n",
    );
}

#[test]
fn utf8_validation_agrees_on_every_edge_everywhere() {
    // RFC 3629's table: each line is a byte sequence and whether it is UTF-8 — the shortest
    // forms at each length's edges, overlong forms, surrogates, past U+10FFFF, truncations,
    // stray continuation bytes and the bytes that never appear.
    let cases: &[(&[u8], bool)] = &[
        (&[], true),
        (&[0x7F], true),
        (&[0xC2, 0x80], true),
        (&[0xDF, 0xBF], true),
        (&[0xE0, 0xA0, 0x80], true),
        (&[0xEF, 0xBF, 0xBF], true),
        (&[0xED, 0x9F, 0xBF], true),
        (&[0xEE, 0x80, 0x80], true),
        (&[0xF0, 0x90, 0x80, 0x80], true),
        (&[0xF4, 0x8F, 0xBF, 0xBF], true),
        (&[0x80], false),
        (&[0xBF], false),
        (&[0xC0, 0x80], false),
        (&[0xC1, 0xBF], false),
        (&[0xE0, 0x9F, 0xBF], false),
        (&[0xF0, 0x8F, 0xBF, 0xBF], false),
        (&[0xED, 0xA0, 0x80], false),
        (&[0xED, 0xBF, 0xBF], false),
        (&[0xF4, 0x90, 0x80, 0x80], false),
        (&[0xF5, 0x80, 0x80, 0x80], false),
        (&[0xFE], false),
        (&[0xFF], false),
        (&[0xE2, 0x82], false),
        (&[0xF0, 0x9F, 0x98], false),
        (&[0xC2, 0x41], false),
        (&[0x41, 0xC2], false),
        (&[0xE2, 0x28, 0xA1], false),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (bytes, valid) in cases {
        let mut s = String::from("\"\"");
        for b in *bytes {
            s = format!("str_concat({s}, str_from_byte({b}))");
        }
        body.push_str(&format!(
            "if utf8_valid({s}) {{ show(out, \"yes\"); }} else {{ show(out, \"no\"); }};\n\
             match try_convert[Text]({s}) {{ Option.Some(value: t) => show(out, \"some\"), Option.None() => show(out, \"none\"), }};\n"
        ));
        assert_eq!(std::str::from_utf8(bytes).is_ok(), *valid, "{bytes:?}");
        expected.push_str(if *valid { "yes\nsome\n" } else { "no\nnone\n" });
    }
    agrees(&body, &expected);
}

#[test]
fn text_iterates_by_scalar_value_and_cuts_only_at_boundaries() {
    agrees_with(
        r#"
fn scalars(t: Text) -> Str {
    let mut i = 0;
    let mut acc = "";
    while i < text_len(t) {
        acc = str_concat(acc, str_concat(int_to_str(text_scalar_at(t, i)), " "));
        i = text_next(t, i);
    }
    acc
}

fn boundaries(t: Text) -> Str {
    let mut i = 0;
    let mut acc = "";
    while i <= text_len(t) {
        acc = str_concat(acc, if text_is_boundary(t, i) { "|" } else { "." });
        i = i + 1;
    }
    acc
}
"#,
        r#"
show(out, scalars("héllo, 世界! 😀"));
show(out, boundaries("aé世😀"));
let t = Text("añb");
show(out, int_to_str(text_scalar_count(t)));
show(out, int_to_str(text_len(t)));
show(out, text_as_str(text_slice(t, 1, 3)));
show(out, text_as_str(text_concat(t, scalar_text(128512))));
show(out, text_as_str(scalar_text(0)));
show(out, int_to_str(text_scalar_at(scalar_text(1114111), 0)));
if scalar_valid(55295) && scalar_valid(57344) && !scalar_valid(55296) && !scalar_valid(57343) && !scalar_valid(1114112) && !scalar_valid(0 - 1) { show(out, "scalars"); } else { show(out, "wrong"); };
if t == Text("añb") && t != "ana" && "añb" == t { show(out, "equal by bytes"); } else { show(out, "wrong"); };
if text_is_boundary(t, 0 - 1) || text_is_boundary(t, 9) { show(out, "wrong"); } else { show(out, "outside is no boundary"); };
"#,
        "104 233 108 108 111 44 32 19990 30028 33 32 128512 \n||.|..|...|\n3\n4\nñ\n\
         añb😀\n\0\n1114111\nscalars\nequal by bytes\noutside is no boundary\n",
    );
}

/// Every implementation stops with exit status 2, `code` and `sentence`.
fn traps_saying(body: &str, code: &str, sentence: &str) {
    let src = format!("fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n}}\n");
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}\n{src}");
        assert!(r.stderr.contains(code), "{label}: {}\n{src}", r.stderr);
        assert!(r.stderr.contains(sentence), "{label}: {}\n{src}", r.stderr);
    }
}

#[test]
fn every_bytes_and_text_failure_traps_with_its_code_everywhere() {
    let outside = "this index is outside the `Bytes`";
    traps_saying("let b = bytes_new(4); bytes_get(b, 4); 0", "N0405", outside);
    traps_saying(
        "let b = bytes_new(4); let i = 0 - 1; bytes_set(b, i, 1); 0",
        "N0405",
        outside,
    );
    traps_saying(
        "let b = bytes_new(4); bytes_slice(b, 2, 5); 0",
        "N0405",
        "this slice is not inside the `Bytes`",
    );
    traps_saying(
        "let b = bytes_new(4); bytes_read_le[UInt32](b, 1); 0",
        "N0405",
        "this read is outside the `Bytes`",
    );
    traps_saying(
        "let b = bytes_new(4); bytes_read_le[Int](b, 9223372036854775807); 0",
        "N0405",
        "this read is outside the `Bytes`",
    );
    traps_saying(
        "let b = bytes_new(4); bytes_write_be(b, 3, UInt16(1)); 0",
        "N0405",
        "this write is outside the `Bytes`",
    );
    traps_saying(
        "let b = bytes_new(2); bytes_copy(b, bytes_new(3)); 0",
        "N0405",
        "the source is longer than the destination",
    );
    traps_saying(
        "let n = 0 - 1; bytes_new(n); 0",
        "N0405",
        "a buffer's length is never negative",
    );
    traps_saying("Text(str_from_byte(255)); 0", "N0413", "this is not UTF-8");
    traps_saying(
        "text_slice(Text(\"añ\"), 0, 2); 0",
        "N0413",
        "this is not UTF-8",
    );
    traps_saying(
        "text_slice(Text(\"añ\"), 0, 9); 0",
        "N0405",
        "this slice is not inside the `Text`",
    );
    traps_saying(
        "text_scalar_at(Text(\"añ\"), 2); 0",
        "N0413",
        "this is not UTF-8",
    );
    traps_saying(
        "text_next(Text(\"añ\"), 3); 0",
        "N0405",
        "this index is outside the `Text`",
    );
    traps_saying(
        "scalar_text(55296); 0",
        "N0413",
        "this is not a Unicode scalar value",
    );
    traps_saying(
        "scalar_text(1114112); 0",
        "N0413",
        "this is not a Unicode scalar value",
    );
}

#[test]
fn bytes_and_text_misuse_is_refused_before_anything_runs() {
    let main = |body: &str| format!("fn main() -> Int {{ {body} }}\n");
    // A `Bytes` is a mutable handle: no `==`, no inline copy, no crossing into a task.
    assert_eq!(
        refused(&main("let a = bytes_new(1); if a == a { 1 } else { 0 }")),
        ["N0304"]
    );
    assert_eq!(refused(&main("let a = [bytes_new(1); 2]; 0")), ["N0621"]);
    assert_eq!(
        refused(
            "fn w(b: Bytes) -> Int ! {spawn} { 0 }\n\
             fn main(s: SpawnCap) -> Int ! {spawn} { scope { spawn w(bytes_new(1)); } 0 }\n"
        ),
        ["N0321"]
    );
    // A `Text` is made from a `Str`, and is not one.
    assert_eq!(refused(&main("Text(5); 0")), ["N0300"]);
    assert_eq!(refused(&main("try_convert[Text](1); 0")), ["N0300"]);
    assert_eq!(refused(&main("let t = Text(\"a\"); str_len(t)")), ["N0300"]);
    assert_eq!(refused(&main("let s = \"a\"; text_len(s)")), ["N0300"]);
    // A byte is a `UInt8`, and both names are the language's.
    assert_eq!(
        refused(&main("bytes_set(bytes_new(1), 0, 256); 0")),
        ["N0003"]
    );
    assert_eq!(
        refused("struct Text { a: Int, }\nfn main() -> Int { 0 }\n"),
        ["N0331"]
    );
    assert_eq!(
        refused("fn bytes_new(n: Int) -> Int { n }\nfn main() -> Int { 0 }\n"),
        ["N0203"]
    );
}

#[test]
fn every_buffer_is_reclaimed_by_its_last_view_everywhere() {
    let src = r#"
fn view(b: Bytes) -> Bytes { bytes_slice(b, 1, 3) }
fn main() -> Int {
    let mut i = 0;
    let mut keep = bytes_new(1);
    while i < 5 {
        let b = bytes_new(8);
        let v = view(b);
        bytes_set(v, 0, 7);
        keep = v;
        let t = Text("añb");
        let u = text_slice(text_concat(t, scalar_text(66)), 1, 3);
        i = i + 1;
    }
    let a = bytes_new(8);
    bytes_set(a, 1, 9);
    let b = bytes_new(8);
    bytes_set(b, 1, 9);
    let c = bytes_new(8);
    bytes_set(c, 1, 9);
    let e = bytes_new(8);
    bytes_set(e, 1, 9);
    Int(bytes_get(keep, 0))
}
"#;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    // Four buffers of the buffer's size, made after the loop, each marked at the byte the view
    // reads: an allocator that hands out a freed buffer again — glibc's per-thread cache does,
    // last freed first, and does not perturb what it caches — gives one of them a buffer freed
    // under a living view, and the view reads its 9. G2-C1: the scribble below did not reach a
    // cached chunk, so on Linux the mutant dropping the view's count went uncaught.
    let report = "strings allocated=20 reclaimed=20 live=0";
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .env("NAZM_MEMORY_REPORT", "1")
            .args(["run", "prog.nz"]),
        "nazm run",
    );
    assert!(r.stderr.contains(report), "interpreter: {}", r.stderr);
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env("NAZM_MEMORY_REPORT", "1");
        // Freed memory scribbled, by glibc and by macOS's allocator: a view whose buffer was freed
        // under it reads the scribble, not the 7 it wrote. Without this a use after free read the
        // old byte and passed — on Linux the Q1 campaign found the Gate 2 mutant that drops the
        // view's count deferred.
        c.env("MALLOC_PERTURB_", "165").env("MallocScribble", "1");
        let r = run_bounded(&mut c, backend);
        assert_eq!(r.stdout, "7\n", "{backend}: {r:?}");
        assert!(r.stderr.contains(report), "{backend}: {}", r.stderr);
    }
}

#[test]
fn bytes_and_text_cross_a_public_signature_cold_and_cached() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub fn greet(t: Text) -> Text ! {} { text_concat(\"hi \", t) }\n\
         pub fn fill(b: Bytes, v: UInt8) -> Bytes ! {} { bytes_set(b, 0, v); b }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"lib.nz\";\n\
         fn main(out: OutCap) -> Int ! {io} {\n\
             print(str_concat(text_as_str(greet(\"añ\")), \"\\n\"));\n\
             Int(bytes_get(fill(bytes_new(2), 9), 0))\n\
         }\n",
    )
    .unwrap();
    // Twice: the second check reads `lib`'s persisted interface, where the types are named.
    for pass in ["cold", "cached"] {
        let r = nazm(d.path(), &["check", "main.nz"]);
        assert_eq!(r.code, Some(0), "{pass}: {}", r.stderr);
    }
    let r = nazm(d.path(), &["run", "main.nz"]);
    assert_eq!(r.stdout, "hi añ\n9\n", "{}", r.stderr);
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "main.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let r = run_bounded(&mut common::bounded_exe(&exe), backend);
        assert_eq!(r.stdout, "hi añ\n9\n", "{backend}: {r:?}");
    }
}
