//! Gate 3: atomics (`docs/spec.md`, *Atomics*; `docs/systems-domains.md` Part B).
//!
//! What only a whole run shows: that an `Atomic` is one cell every copy and every task reaches,
//! that its operations mean the same in the interpreter and in both native backends at both
//! optimisation levels — the same values, the same overflow stop — and that its misuses are
//! refused before anything runs. Every expected line was worked out from the specification.

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

/// The program under the interpreter, and built and run by each backend at each level.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
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
    let expected = format!("{expected}0\n");
    for (label, r) in everywhere(src) {
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
fn every_operation_answers_as_specified() {
    let src = format!(
        "{SHOW}fn main(out: OutCap) -> Int ! {{io}} {{
    let a = atomic_new(5);
    show(out, atomic_load(a));
    show(out, atomic_store(a, 6));
    show(out, atomic_load(a));
    show(out, atomic_add(a, 10));
    show(out, atomic_add(a, -20));
    show(out, atomic_load(a));
    show(out, atomic_swap(a, 40));
    show(out, if atomic_compare_swap(a, 41, 0) {{ 1 }} else {{ 0 }});
    show(out, atomic_load(a));
    show(out, if atomic_compare_swap(a, 40, 41) {{ 1 }} else {{ 0 }});
    show(out, atomic_load(a));
    0
}}
"
    );
    agrees(&src, "5\n0\n6\n6\n16\n-4\n-4\n0\n40\n1\n41\n");
}

#[test]
fn an_atomic_is_no_string_in_the_memory_report_on_any_tier() {
    // `docs/spec.md` (*Strings*): the interpreter and a compiled program allocate the same
    // strings, and `atomic_new` allocates none. Until runtime ABI 38 a compiled program counted
    // each atomic's cell as a string (R4's finding, on `http_listen`'s three).
    // Nothing printed but `main`'s result (formatting a number would make a string).
    let line = crate::differential::agrees(
        "fn main() -> Int {
    let a = atomic_new(0);
    let b = atomic_new(1);
    atomic_add(a, 2);
    if atomic_load(a) + atomic_load(b) == 3 { 0 } else { 1 }
}
",
        "0",
    );
    assert!(
        line.contains(" strings allocated=0 reclaimed=0 live=0 "),
        "{line}"
    );
}

#[test]
fn every_copy_and_every_task_reaches_one_cell() {
    let src = format!(
        "{SHOW}struct Counter {{ hits: Atomic, name: Int, }}

fn bump(c: Counter, n: Int) -> Int {{
    let mut i = 0;
    while i < n {{
        atomic_add(c.hits, 1);
        i = i + 1;
    }}
    0
}}

fn main(out: OutCap, tasks: SpawnCap) -> Int ! {{io, spawn}} {{
    let c = Counter(hits: atomic_new(0), name: 7);
    let copy = c;
    scope {{
        spawn bump(c, 2000);
        spawn bump(copy, 2000);
        spawn bump(c, 2000);
        spawn bump(copy, 2000);
    }}
    show(out, atomic_load(c.hits));
    show(out, atomic_load(copy.hits));
    0
}}
"
    );
    agrees(&src, "8000\n8000\n");
}

#[test]
fn a_sum_outside_int_stops_the_program_as_plus_does() {
    let src = format!(
        "{SHOW}fn main(out: OutCap) -> Int ! {{io}} {{
    let a = atomic_new(9223372036854775806);
    show(out, atomic_add(a, 1));
    atomic_add(a, 1);
    show(out, 0);
    0
}}
"
    );
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "9223372036854775806\n", "{label}");
        assert!(r.stderr.contains("N0400"), "{label}: {}", r.stderr);
        assert!(
            r.stderr.contains("this addition overflows `Int`"),
            "{label}: {}",
            r.stderr
        );
    }
}

#[test]
fn an_atomic_has_no_equality_is_not_plain_data_and_its_names_are_the_languages() {
    assert_eq!(
        refused("fn main() -> Int { let a = atomic_new(1); if a == a { 1 } else { 0 } }\n"),
        ["N0304"]
    );
    assert_eq!(
        refused("struct Pair { cells: [Atomic; 2], }\nfn main() -> Int { 0 }\n"),
        ["N0621"]
    );
    assert_eq!(
        refused("struct Atomic { x: Int, }\nfn main() -> Int { 0 }\n"),
        ["N0331"]
    );
    assert_eq!(
        refused("fn atomic_load(x: Int) -> Int { x }\nfn main() -> Int { 0 }\n"),
        ["N0203"]
    );
}
