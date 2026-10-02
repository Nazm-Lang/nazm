//! N66: vectorisation under checked arithmetic.
//!
//! `docs/architecture.md` §7.68. What whole builds show: `ints_sum_from` gives the ordered checked sum —
//! the same value as a scalar loop for every length across the 64-element block boundaries and
//! every magnitude either side of the fast path's bound, and the same failure, at the same place,
//! wherever a running sum overflows, including where the total would fit — under the interpreter,
//! both backends and two optimisation levels; a native build replaces the counted summation loop
//! with it and the interpreter does not; `nazm explain-cost` says what became of every loop.

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

/// What `src` prints and how it ends, under the interpreter and under each native build.
fn everywhere(src: &str) -> Vec<(String, String, Option<i32>)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    let run = nazm(d.path(), &["run", "prog.nz"]);
    out.push((
        "run".to_owned(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        run.status.code(),
    ));
    for (backend, opt) in [
        ("llvm", "0"),
        ("llvm", "2"),
        ("cranelift", "0"),
        ("cranelift", "2"),
    ] {
        let exe = d.path().join(format!("p-{backend}-{opt}"));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert!(
            b.status.success(),
            "{backend} -O{opt}: {}",
            String::from_utf8_lossy(&b.stderr)
        );
        let o = Command::new(&exe).output().unwrap();
        out.push((
            format!("{backend} -O{opt}"),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.code(),
        ));
    }
    out
}

/// A failure's `error[...] ... at FILE:L:C` line from standard error, wherever it was printed.
fn failure(dir: &Path, args: &[&str]) -> String {
    let o = nazm(dir, args);
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .find(|l| l.contains("error["))
        .unwrap_or_default()
        .to_owned()
}

/// Every length from 0 to 200 — so every tail length and the 64- and 128-element boundaries —
/// and three magnitudes: well inside the fast path, exactly at its 2^56 bound, and past it.
const PROPERTY: &str = "fn scalar(start: Int, v: Ints) -> Int {\n\
    let mut s = start;\n\
    let mut i = 0;\n\
    while i < ints_len(v) { s = ints_get(v, i) + s; i = i + 1; }\n\
    s\n\
}\n\
fn main() -> Int {\n\
    let mut bad = 0;\n\
    let mut n = 0;\n\
    while n <= 200 {\n\
        let mut m = 0;\n\
        while m < 3 {\n\
            let v = ints_new();\n\
            let mut k = 0;\n\
            while k < n {\n\
                let big = if m == 0 { 1000 } else { if m == 1 { 72057594037927936 } else { 72057594037927937 } };\n\
                let x = if k % 2 == 0 { big - k } else { k - big };\n\
                ints_push(v, x);\n\
                k = k + 1;\n\
            }\n\
            let start = if n % 3 == 0 { 0 } else { 4611686018427387000 - n * 1000 };\n\
            if ints_sum_from(start, v) != scalar(start, v) { bad = bad + 1; }\n\
            m = m + 1;\n\
        }\n\
        n = n + 1;\n\
    }\n\
    bad\n\
}\n";

#[test]
fn the_sum_is_the_ordered_sum_for_every_length_and_magnitude() {
    for (tier, stdout, status) in everywhere(PROPERTY) {
        assert_eq!((stdout.as_str(), status), ("0\n", Some(0)), "{tier}");
    }
}

/// `count` ones, then `big`, then `after` — all summed from `start`.
fn overflow_program(count: i64, big: &str, after: &str) -> String {
    format!(
        "fn main() -> Int {{\n\
         \x20   let v = ints_new();\n\
         \x20   let mut i = 0;\n\
         \x20   while i < {count} {{ ints_push(v, 1); i = i + 1; }}\n\
         \x20   ints_push(v, {big});\n\
         \x20   ints_push(v, {after});\n\
         \x20   ints_sum_from(0, v)\n\
         }}\n"
    )
}

#[test]
fn a_running_sum_that_overflows_fails_where_the_loop_would_even_if_the_total_fits() {
    // `count` ones and then `MAX - count + 1`: the running sum passes `MAX` at that element, and
    // the `-300` after it would bring the total back inside `Int`. Positions either side of the
    // block boundaries.
    for count in [1, 62, 63, 64, 65, 127, 128, 200] {
        let big = format!("9223372036854775807 - {}", count - 1);
        let src = overflow_program(count, &big, "-300");
        for (tier, stdout, status) in everywhere(&src) {
            assert_eq!(status, Some(2), "{count} {tier}: {stdout}");
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), &src).unwrap();
        let interpreted = failure(d.path(), &["run", "prog.nz"]);
        assert!(
            interpreted.contains("error[N0400]: this addition overflows `Int`"),
            "{interpreted}"
        );
        let b = nazm(
            d.path(),
            &["build", "--opt-level", "2", "prog.nz", "-o", "p"],
        );
        assert!(b.status.success());
        let o = Command::new(d.path().join("p")).output().unwrap();
        let native = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(
            native.contains("error[N0400]: this addition overflows `Int` at prog.nz:7:5"),
            "{count}: {native}"
        );
    }
    // A whole block that would wrap if it were added unchecked: 64 elements of 2^59, past the
    // fast path's 2^56 bound, overflow at the sixteenth; and small elements onto a start too
    // close to `MAX` for the fast path's room, overflowing at the eleventh.
    for src in [
        "fn main() -> Int { let v = ints_new(); let mut i = 0; while i < 64 { ints_push(v, 576460752303423488); i = i + 1; } ints_sum_from(0, v) }\n",
        "fn main() -> Int { let v = ints_new(); let mut i = 0; while i < 64 { ints_push(v, 1); i = i + 1; } ints_sum_from(9223372036854775797, v) }\n",
    ] {
        for (tier, stdout, status) in everywhere(src) {
            assert_eq!(status, Some(2), "{tier}: {stdout}");
        }
    }
    // One less and nothing overflows: the same programs give the total.
    let src = overflow_program(64, "9223372036854775807 - 64", "-300");
    for (tier, stdout, status) in everywhere(&src) {
        assert_eq!(
            (stdout.as_str(), status),
            ("9223372036854775507\n", Some(0)),
            "{tier}"
        );
    }
}

const IDIOM: &str = "fn total(v: Ints) -> Int {\n\
    let mut s = 0;\n\
    let mut i = 0;\n\
    while i < ints_len(v) {\n\
        s = s + ints_get(v, i);\n\
        i = i + 1;\n\
    }\n\
    s\n\
}\n\
fn main() -> Int {\n\
    let v = ints_new();\n\
    let mut k = 0;\n\
    while k < 1000 { ints_push(v, k * 7 - 300); k = k + 1; }\n\
    ints_push(v, 9223372036854775807);\n\
    total(v)\n\
}\n";

#[test]
fn a_native_build_replaces_the_summation_loop_and_reports_its_failure_at_the_addition() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), IDIOM).unwrap();
    let interpreted = failure(d.path(), &["run", "prog.nz"]);
    assert!(interpreted.contains("error[N0400]"), "{interpreted}");
    // Where the interpreter's loop failed: the addition, line 5 column 7.
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("prog.nz:5:7"),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    for backend in ["llvm", "cranelift"] {
        let b = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "--opt-level",
                "2",
                "prog.nz",
                "-o",
                "p",
                "--emit-ir",
            ],
        );
        assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
        let o = Command::new(d.path().join("p")).output().unwrap();
        // The loop's own addition: the same place the interpreter names.
        let native = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(
            native.contains("error[N0400]: this addition overflows `Int` at prog.nz:5:7"),
            "{backend}: {native}"
        );
        if backend == "llvm" {
            // The LLVM unit calls the runtime's sum in place of the loop.
            let unit = std::fs::read_to_string(d.path().join("p.m0.ll")).unwrap();
            assert!(unit.contains("call i64 @nz.ints_sum("), "{unit}");
        }
    }
}

#[test]
fn the_interpreter_keeps_the_loop() {
    // Its iteration budget still counts every turn of the loop a native build replaced.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), IDIOM).unwrap();
    let e = failure(d.path(), &["run", "--max-iterations", "1500", "prog.nz"]);
    assert!(e.contains("N0402"), "{e}");
}

#[test]
fn explain_cost_says_what_became_of_every_loop() {
    let d = tempfile::tempdir().unwrap();
    let near = "fn from_one(v: Ints) -> Int { let mut s = 0; let mut i = 1; while i < ints_len(v) { s = s + ints_get(v, i); i = i + 1; } s }\n\
                fn by_two(v: Ints) -> Int { let mut s = 0; let mut i = 0; while i < ints_len(v) { s = s + ints_get(v, i); i = i + 2; } s }\n";
    std::fs::write(
        d.path().join("prog.nz"),
        PROPERTY.to_owned() + &IDIOM.replace("fn main", "fn unused_main") + near,
    )
    .unwrap();
    let o = nazm(d.path(), &["explain-cost", "--json", "prog.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let loops = v["loops"].as_array().unwrap();
    let by_function = |f: &str| -> Vec<&serde_json::Value> {
        loops.iter().filter(|l| l["function"] == f).collect()
    };
    let total = by_function("total");
    assert_eq!(total.len(), 1);
    assert_eq!(total[0]["vectorised"], "summation");
    assert!(total[0]["reason"].is_null());
    // Its operands swapped: the same work, not the idiom, and said so.
    let scalar = by_function("scalar");
    assert!(scalar[0]["vectorised"].is_null());
    assert_eq!(
        scalar[0]["reason"],
        "its body does not add an element to an accumulator"
    );
    for l in by_function("main") {
        assert!(l["vectorised"].is_null());
        assert!(l["reason"].as_str().is_some_and(|r| !r.is_empty()), "{l}");
    }
    // One counted loop away from the idiom is not it.
    assert_eq!(
        by_function("from_one")[0]["reason"],
        "its counter does not start at 0 in the loop's block"
    );
    assert_eq!(
        by_function("by_two")[0]["reason"],
        "its counter does not step by 1"
    );
    let text = nazm(d.path(), &["explain-cost", "prog.nz"]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("vectorised (summation)"), "{text}");
    assert!(
        text.contains("not vectorised: its body does not add an element to an accumulator"),
        "{text}"
    );
}
