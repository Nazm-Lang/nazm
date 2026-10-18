//! Gate 3: statics (`docs/spec.md`, *Statics*; `docs/systems-domains.md` Part B).
//!
//! What only a whole run shows: that a static is one storage for the whole run — a static
//! `Atomic` one cell every function, task, generic instance and module reaches, in the
//! interpreter and in both native backends at both optimisation levels — that a numeric or
//! `Bool` static reads as its value, and that every misuse is refused before anything runs.

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

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program, with other modules beside `prog.nz`, under the interpreter, and built and run
/// by each backend at each level.
fn everywhere_with(src: &str, others: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    for (name, text) in others {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["run", "--interpret", "prog.nz"]),
            "nazm run",
        ),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
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
fn agrees(src: &str, expected: &str) {
    agrees_with(src, &[], expected);
}

fn agrees_with(src: &str, others: &[(&str, &str)], expected: &str) {
    let expected = format!("{expected}0\n");
    for (label, r) in everywhere_with(src, others) {
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

const SHOW: &str =
    "fn show(out: OutCap, n: Int) -> Int ! {io} { print(str_concat(num_to_str(n), \"\\n\")) }\n";

#[test]
fn a_number_or_bool_static_reads_as_its_value() {
    let src = format!(
        "{SHOW}static LIMIT: Int = 3 * 1000;
static SMALL: UInt8 = 200;
static SCALE: Float64 = 2.5;
static ON: Bool = !false;
const TWICE: Int = LIMIT * 2;

fn main(out: OutCap) -> Int ! {{io}} {{
    show(out, LIMIT);
    show(out, Int(SMALL) + 1);
    show(out, Int(SCALE * 2.0));
    show(out, if ON {{ 1 }} else {{ 0 }});
    show(out, TWICE);
    0
}}
"
    );
    agrees(&src, "3000\n201\n5\n1\n6000\n");
}

#[test]
fn a_static_atomic_is_one_cell_for_every_function_task_and_instance() {
    let src = format!(
        "{SHOW}static START: Int = 7;
static TICKS: Atomic = atomic_new(START);

fn tick(n: Int) -> Int {{
    let mut i = 0;
    while i < n {{
        atomic_add(TICKS, 1);
        i = i + 1;
    }}
    0
}}

fn through[T](x: T) -> Int {{ atomic_add(TICKS, 100) }}

fn main(out: OutCap, tasks: SpawnCap) -> Int ! {{io, spawn}} {{
    show(out, atomic_load(TICKS));
    scope {{
        spawn tick(1500);
        spawn tick(1500);
        spawn tick(1500);
    }}
    show(out, atomic_load(TICKS));
    show(out, through(true));
    show(out, through(\"s\"));
    let copy = TICKS;
    atomic_store(copy, 1);
    show(out, atomic_load(TICKS));
    0
}}
"
    );
    agrees(&src, "7\n4507\n4507\n4607\n1\n");
}

#[test]
fn a_static_is_its_modules_and_its_functions_reach_it_from_anywhere() {
    let counter = "static COUNT: Atomic = atomic_new(40);\n\
                   pub fn bump() -> Int ! {} { atomic_add(COUNT, 1) }\n\
                   pub fn count() -> Int ! {} { atomic_load(COUNT) }\n";
    let src = format!(
        "{SHOW}use \"counter.nz\";

fn main(out: OutCap) -> Int ! {{io}} {{
    bump();
    bump();
    show(out, count());
    0
}}
"
    );
    agrees_with(&src, &[("counter.nz", counter)], "42\n");
}

/// Two modules each declaring `COUNT` and `LIMIT` have four statics, not two: each function
/// reaches its own module's, in the interpreter and both backends.
#[test]
fn two_modules_same_named_statics_are_each_their_own() {
    let counter = "static COUNT: Atomic = atomic_new(40);\n\
                   static LIMIT: Int = 70;\n\
                   pub fn bump() -> Int ! {} { atomic_add(COUNT, 1) }\n\
                   pub fn count() -> Int ! {} { atomic_load(COUNT) }\n\
                   pub fn limit() -> Int ! {} { LIMIT }\n";
    let src = format!(
        "{SHOW}use \"counter.nz\";

static COUNT: Atomic = atomic_new(1);
static LIMIT: Int = 5;

fn main(out: OutCap) -> Int ! {{io}} {{
    atomic_add(COUNT, 1);
    bump();
    bump();
    show(out, atomic_load(COUNT));
    show(out, count());
    show(out, LIMIT + limit());
    0
}}
"
    );
    agrees_with(&src, &[("counter.nz", counter)], "2\n42\n75\n");
}

#[test]
fn every_misuse_of_a_static_is_refused() {
    const MAIN: &str = "fn main() -> Int { 0 }\n";
    // Read-only: there are no mutable global variables.
    assert_eq!(
        refused("static A: Int = 1;\nfn main() -> Int { A = 2; 0 }\n"),
        ["N0625"]
    );
    assert_eq!(
        refused("static A: Atomic = atomic_new(1);\nfn main() -> Int { A = atomic_new(2); 0 }\n"),
        ["N0625"]
    );
    // Its module's own.
    assert_eq!(
        refused(&format!("pub static A: Int = 1;\n{MAIN}")),
        ["N0101"]
    );
    // A number, a `Bool` or an `Atomic`, started by a constant or `atomic_new` of one.
    assert_eq!(
        refused(&format!("static A: Str = \"x\";\n{MAIN}")),
        ["N0622"]
    );
    assert_eq!(
        refused(&format!(
            "struct P {{ x: Int, }}\nstatic A: P = P(x: 1);\n{MAIN}"
        )),
        ["N0622"]
    );
    assert_eq!(
        refused(&format!("static A: Atomic = 5;\n{MAIN}")),
        ["N0622"]
    );
    assert_eq!(
        refused(&format!(
            "fn f() -> Int {{ 1 }}\nstatic A: Int = f();\n{MAIN}"
        )),
        ["N0622"]
    );
    // Storage is not a length, and an atomic's cell is no constant's value.
    assert_eq!(
        refused(&format!(
            "static N: Int = 2;\nstruct Q {{ xs: [Int; N], }}\n{MAIN}"
        )),
        ["N0622"]
    );
    assert_eq!(
        refused(&format!(
            "static A: Atomic = atomic_new(1);\nconst B: Int = A;\n{MAIN}"
        )),
        ["N0622"]
    );
    // A value that would trap is refused where it is declared, as a constant's is.
    assert_eq!(
        refused(&format!("static A: Int = 9223372036854775807 + 1;\n{MAIN}")),
        ["N0623"]
    );
}

#[test]
fn static_stays_a_name_outside_an_item_start() {
    // A word only before a name and a `:` at an item's start, as `const` is.
    agrees(
        "fn main() -> Int { let static = 2; static * 21 - 42 }\n",
        "",
    );
}
