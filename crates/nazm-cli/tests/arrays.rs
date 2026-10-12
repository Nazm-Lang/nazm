//! Gate 2: fixed-size arrays and constants.
//!
//! `docs/general-purpose.md` §4 and §15, and the spec's *Arrays* and *Constants*. What only a
//! whole run shows: that an array is a value — built, copied, compared, indexed and written the
//! same way in the interpreter and in both native backends at both optimisation levels — that
//! every index is checked against the length its type states, and that every misuse is refused
//! before anything runs. Every expected line was worked out from the specification.

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
    let expected = format!("{expected}0\n");
    let src = format!(
        "fn show(out: OutCap, s: Str) -> Int ! {{io}} {{ print(str_concat(s, \"\\n\")) }}\n\
         fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n0\n}}\n"
    );
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(0), "{label}: {}\n{src}", r.stderr);
        assert_eq!(r.stdout, expected, "{label}\n{src}");
    }
}

/// Every implementation stops with exit status 2 and `code`.
fn traps(body: &str, code: &str) {
    let src = format!("fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n}}\n");
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}\n{src}");
        assert!(r.stderr.contains(code), "{label}: {}\n{src}", r.stderr);
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
fn an_array_is_a_value_built_copied_and_written_in_place() {
    agrees(
        r#"
let mut a = [1, 2, 3, 4];
a[2] = 30;
let b = a;
a[0] = 100;
show(out, int_to_str(a[0] + a[1] + a[2] + a[3]));
show(out, int_to_str(b[0]));
let mut grid = [[0; 3]; 2];
grid[1][2] = 7;
grid[0][0] = grid[1][2] * 2;
show(out, int_to_str(grid[0][0] + grid[1][2] + grid[1][1]));
let zeros = [UInt8(0); 64];
show(out, num_to_str(zeros[63]));
let fives = [5; 300];
show(out, int_to_str(fives[0] + fives[299]));
let fs = [0.5, 1.5, 2.5];
show(out, num_to_str(fs[0] + fs[1] + fs[2]));
if [1, 2, 3] == [1, 2, 3] && [1, 2, 3] != [1, 2, 4] { show(out, "equal by elements"); } else { show(out, "wrong"); };
if [0.0 / 0.0] == [0.0 / 0.0] { show(out, "nan equal"); } else { show(out, "a nan array is not equal to itself"); };
"#,
        "136\n1\n21\n0\n10\n4.5\nequal by elements\na nan array is not equal to itself\n",
    );
}

#[test]
fn arrays_live_in_records_and_records_in_arrays() {
    let src = r#"
const SIZE: Int = 4;
const STEP: Float64 = 0.25;

struct Point { x: Int, y: Float64, }
struct Frame { data: [UInt8; SIZE], tag: Bool, }
enum Cell { Empty, Full(value: [Int; 2]), }

fn total(f: Frame) -> Int {
    let mut t = 0;
    let mut i = 0;
    while i < SIZE {
        t = t + Int(f.data[i]);
        i = i + 1;
    }
    t
}

fn make(n: Int) -> [Point; 3] {
    let mut ps = [Point(x: 0, y: 0.0); 3];
    let mut i = 0;
    while i < 3 {
        ps[i].x = n + i;
        ps[i].y = Float64(i) * STEP;
        i = i + 1;
    }
    ps
}

fn main(out: OutCap) -> Int ! {io} {
    let mut f = Frame(data: [UInt8(1), 2, 3, 4], tag: true);
    f.data[3] = 40;
    print(str_concat(int_to_str(total(f)), "\n"));
    let ps = make(10);
    print(str_concat(int_to_str(ps[2].x), "\n"));
    print(str_concat(num_to_str(ps[2].y), "\n"));
    let c = Cell.Full(value: [5, 6]);
    match c {
        Cell.Empty() => print("empty\n"),
        Cell.Full(value: v) => print(str_concat(int_to_str(v[0] * v[1]), "\n")),
    };
    let g = Frame(data: [UInt8(1), 2, 3, 40], tag: true);
    if f == g { print("frames equal\n"); } else { print("frames differ\n"); };
    0
}
"#;
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, "46\n12\n0.5\n30\nframes equal\n0\n", "{label}");
    }
}

#[test]
fn an_array_crosses_into_a_task_as_a_copy() {
    let src = r#"
fn worker(c: Chan[Int], xs: [Int; 3]) -> Int ! {spawn} {
    chan_send_of(c, xs[0] + xs[1] + xs[2]);
    0
}

fn main(out: OutCap, s: SpawnCap) -> Int ! {io, spawn} {
    let mut xs = [1, 2, 3];
    let c = chan_new_of[Int](1);
    scope { spawn worker(c, xs); }
    xs[0] = 100;
    let got = vec_new[Int]();
    chan_recv_of(c, got);
    print(str_concat(int_to_str(vec_get(got, 0) + xs[0]), "\n"));
    0
}
"#;
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, "106\n0\n", "{label}");
    }
}

#[test]
fn every_index_is_checked_against_the_length_everywhere() {
    traps("let a = [1, 2, 3]; let i = 3; let x = a[i]; 0", "N0405");
    traps("let a = [1, 2, 3]; let i = -1; let x = a[i]; 0", "N0405");
    traps("let mut a = [1, 2, 3]; let i = 5; a[i] = 1; 0", "N0405");
    traps(
        "let mut g = [[0; 2]; 2]; let i = 2; g[0][i] = 1; 0",
        "N0405",
    );
}

#[test]
fn constants_are_computed_where_they_are_declared() {
    let src = r#"
const A: Int = 6 * 7;
const B: Int = A - 2;
const MASK: UInt32 = 0xFF << 8;
const HALF: Float32 = 0.5;
const ON: Bool = A > B && !false;
const GREETING: Str = "hello";
const BIG: UInt64 = 18446744073709551615;
const NEG: Int8 = -128;
const WIDE: Int = Int(UInt16(65535));

fn main(out: OutCap) -> Int ! {io} {
    print(str_concat(int_to_str(A + B), "\n"));
    print(str_concat(num_to_str(MASK), "\n"));
    print(str_concat(num_to_str(HALF + HALF), "\n"));
    if ON { print("on\n"); } else { print("off\n"); };
    print(str_concat(GREETING, "\n"));
    print(str_concat(num_to_str(BIG), "\n"));
    print(str_concat(num_to_str(NEG), "\n"));
    print(str_concat(int_to_str(WIDE), "\n"));
    0
}
"#;
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(
            r.stdout, "82\n65280\n1\non\nhello\n18446744073709551615\n-128\n65535\n0\n",
            "{label}"
        );
    }
}

#[test]
fn array_and_constant_misuse_is_refused_before_anything_runs() {
    let main = |body: &str| format!("fn main() -> Int {{ {body} }}\n");
    // Values only — no handle, since R1a (L8) strings included — and at most 64 KiB.
    assert_eq!(
        refused(&main("let a = [vec_new[Int](), vec_new[Int]()]; 0")),
        ["N0621"]
    );
    assert_eq!(
        refused(
            "struct S { v: Vec[Int], }\nfn f(a: [S; 2]) -> Int { 0 }\nfn main() -> Int { 0 }\n"
        ),
        ["N0621"]
    );
    assert_eq!(refused(&main("let a = [0; 9000]; 0")), ["N0618"]);
    assert_eq!(refused(&main("let a = [UInt8(0); 70000]; 0")), ["N0618"]);
    // The bound is exact: 65,536 bytes is an array, one more is not.
    assert_eq!(refused(&main("let a = [UInt8(0); 65537]; 0")), ["N0618"]);
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        main("let a = [UInt8(0); 65536]; 0"),
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "prog.nz"]);
    assert_eq!(r.code, Some(0), "65,536 bytes is an array: {r:?}");
    // One element type.
    assert_eq!(refused(&main("let a = [1, true]; 0")), ["N0300"]);
    // A constant index outside the array, and an index into something that is not one.
    assert_eq!(refused(&main("let a = [1, 2]; a[2]")), ["N0624"]);
    assert_eq!(refused(&main("let v = vec_new[Int](); v[0]")), ["N0624"]);
    assert_eq!(refused(&main("let a = [1, 2]; a[true]")), ["N0300"]);
    // A length is a literal or an `Int` constant.
    assert_eq!(refused(&main("let n = 3; let a = [0; n]; 0")), ["N0622"]);
    // Constants: computed, of a value type, private, and never self-referential.
    assert_eq!(
        refused("const X: Int = 9223372036854775807 + 1;\nfn main() -> Int { X }\n"),
        ["N0623"]
    );
    assert_eq!(
        refused("const X: Int = Y;\nconst Y: Int = X;\nfn main() -> Int { X }\n"),
        ["N0622"]
    );
    assert_eq!(
        refused("const V: Vec[Int] = 0;\nfn main() -> Int { 0 }\n"),
        ["N0622"]
    );
    assert_eq!(
        refused("fn f() -> Int { 1 }\nconst X: Int = f();\nfn main() -> Int { X }\n"),
        ["N0622"]
    );
    // A static is its module's storage (a constant is exported since R1a, L8).
    assert_eq!(
        refused("pub static X: Int = 1;\nfn main() -> Int { X }\n"),
        ["N0101"]
    );
    assert_eq!(
        refused("const X: UInt8 = 256;\nfn main() -> Int { 0 }\n"),
        ["N0003"]
    );
    // `const` is a word only at the start of an item: a binding may still be called `const`.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main() -> Int { let const = 2; const }\n",
    )
    .unwrap();
    assert_eq!(nazm(d.path(), &["check", "prog.nz"]).code, Some(0));
}

#[test]
fn an_array_in_a_public_signature_crosses_modules() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub fn sum3(xs: [Int; 3]) -> Int { xs[0] + xs[1] + xs[2] }\npub fn make() -> [UInt8; 2] { [UInt8(7), 8] }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"lib.nz\";\nfn main() -> Int { sum3([1, 2, 3]) + Int(make()[1]) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "main.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(r.stdout.trim(), "14");
    // And again, against the interface the first check persisted.
    let r = nazm(d.path(), &["run", "main.nz"]);
    assert_eq!(r.stdout.trim(), "14", "{}", r.stderr);
    let exe = d.path().join("prog");
    let b = nazm(d.path(), &["build", "main.nz", "-o", exe.to_str().unwrap()]);
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let o = std::process::Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "14");
}
