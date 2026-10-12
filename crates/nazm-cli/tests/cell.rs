//! R1b: a mutable cell, `Cell[T]` (`docs/spec.md`, *A mutable cell*).
//!
//! One value that can be replaced, reached through a handle, so a callee's change is its caller's.
//! The tests ask every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`,
//! and Cranelift — for what the section's rules work out by hand, with every allocation reclaimed;
//! the ownership rule, which counts a cell as it counts a `Vec`; the refusals a mutable handle
//! has; the prelude names that keep every 1.0 program's meaning; provenance through a cell; and a
//! `Cell` type crossing a module's published interface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The program under the interpreter and the LIR oracle, and built and run by each backend,
/// each with the memory report asked for.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
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
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

/// The memory report's line, which every implementation but the LIR oracle writes when asked.
fn report(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"))
        .to_owned()
}

fn nothing_live(line: &str) -> bool {
    line.split_whitespace()
        .filter(|w| w.starts_with("live="))
        .all(|w| w == "live=0")
}

const CELLS: &str = "struct Context { next: Cell[Int], log: Cell[Str], }
struct Point { x: Int, y: Int, }

fn show(out: OutCap, s: Str) -> Int ! {io} {
    print(str_concat(s, \"\\n\"))
}

// A callee changes what its caller sees: the reason a cell exists.
fn fresh(c: Context, base: Str) -> Str {
    let n = cell_get(c.next);
    cell_set(c.next, n + 1);
    cell_set(c.log, str_concat(cell_get(c.log), base));
    str_concat(base, num_to_str(n))
}

fn main(out: OutCap) -> Int ! {io} {
    let c = Context(next: cell_new(0), log: cell_new(\"\"));
    show(out, fresh(c, \"t\"));
    show(out, fresh(c, \"u\"));
    show(out, cell_get(c.log));
    // Assignment shares: one cell, two names.
    let d = c.next;
    show(out, num_to_str(cell_set(d, 40)));
    show(out, num_to_str(cell_get(c.next) + 2));
    // Replacing a value releases the old one: a string, a record, a vector.
    let s = cell_new(str_concat(\"a\", \"b\"));
    cell_set(s, str_concat(\"c\", \"d\"));
    let p = cell_new(Point(x: 1, y: 2));
    let q = cell_get(p);
    cell_set(p, Point(x: q.y, y: q.x));
    let v = cell_new(vec_new[Int]());
    vec_push(cell_get(v), 5);
    cell_set(v, vec_new[Int]());
    vec_push(cell_get(v), 6);
    show(out, str_concat(cell_get(s), num_to_str(cell_get(p).x * 10 + vec_get(cell_get(v), 0))));
    // A cell inside a box is still a cell: the box cannot change, what the cell holds can.
    let b = box_new(cell_new(1));
    cell_set(box_get(b), 7);
    show(out, num_to_str(cell_get(box_get(b))));
    0
}
";

/// What the section's rules give, worked out by hand.
const EXPECTED: &str = "t0\nu1\ntu\n0\n42\ncd26\n7\n";

#[test]
fn a_cell_means_the_same_everywhere_and_everything_in_it_is_reclaimed() {
    for (label, r) in everywhere(CELLS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

/// `nazm check` on `src`: every error code it reports, in order.
fn codes(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    r.stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect()
}

#[test]
fn the_ownership_rule_counts_a_cell_as_it_counts_a_vec() {
    let none = Vec::<String>::new();
    for (src, want) in [
        // A cell of what holds no cycle: accepted.
        (
            "struct C { n: Cell[Int], s: Cell[Str], }\nfn main() -> Int { 0 }",
            none.clone(),
        ),
        // A cycle through a cell: it could be set to hold the value that holds it.
        (
            "struct Node { next: Option[Cell[Node]], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        (
            "struct A { b: Cell[B], }\nstruct B { a: Box[A], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        // Through a generic definition's parameter, which a `Cell` counts.
        (
            "struct W[T] { c: Cell[T], }\nstruct Node { w: W[Node], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        // A cell inside an immutable indirection is still counted.
        (
            "struct Node { b: Box[Cell[Node]], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
    ] {
        assert_eq!(codes(src), want, "{src}");
    }
}

#[test]
fn a_cell_is_neither_compared_nor_given_to_a_task_nor_written_bare() {
    assert_eq!(
        codes("fn main() -> Int { let a = cell_new(1); if a == a { 1 } else { 0 } }"),
        ["N0304"]
    );
    // A record holding one has no derived equality either.
    assert_eq!(
        codes(
            "struct R { c: Cell[Int], }\n\
             fn main() -> Int { let a = R(c: cell_new(1)); if a == a { 1 } else { 0 } }"
        ),
        ["N0304"]
    );
    assert_eq!(
        codes(
            "fn w(c: Cell[Int]) -> Int { 0 }\n\
             fn main() -> Int ! {spawn} { let c = cell_new(1); scope { spawn w(c); } 0 }"
        )
        .first()
        .map(String::as_str),
        Some("N0321")
    );
    assert_eq!(
        codes("fn f(c: Cell) -> Int { 0 }\nfn main() -> Int { 0 }"),
        ["N0352"]
    );
    // A closure may capture a cell of what can hold no function value; not one that can (`N0616`).
    assert_eq!(
        codes(
            "fn main() -> Int { let c = cell_new(1); \
             let f = fn() -> Int { cell_get(c) }; f() }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn one() -> Int { 1 }\n\
             fn main() -> Int { let c = cell_new(one); \
             let f = fn() -> Int { let g = cell_get(c); g() }; f() }"
        ),
        ["N0616"]
    );
}

/// `nazm run` on `src`: its status and output.
fn run(src: &str) -> Ran {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "p.nz"]),
        "nazm run",
    )
}

#[test]
fn the_cell_names_are_prelude_names_a_1_0_program_keeps() {
    // A 1.0 program's own `Cell` type and `cell_new` function mean what they meant.
    let r = run(
        "struct Cell { v: Int, }\nfn cell_new(x: Int) -> Cell { Cell(v: x + 1) }\n\
         fn main() -> Int { cell_new(41).v }\n",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.starts_with("42"), "{}", r.stdout);
    let r = run("fn cell_get(x: Int) -> Int { x * 3 }\nfn main() -> Int { cell_get(3) }\n");
    assert!(r.stdout.starts_with('9'), "{r:?}");
}

#[test]
fn what_a_cell_holds_carries_its_provenance() {
    // A file's contents in a cell — put there when it was made or set later — may not name a
    // written file (`N0372`); a literal through the same may.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let c = cell_new(read_file(\"cfg\")); \
             write_file(cell_get(c), \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let c = cell_new(\"out.txt\"); \
             cell_set(c, read_file(\"cfg\")); write_file(cell_get(c), \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let c = cell_new(\"out.txt\"); \
             write_file(cell_get(c), \"x\") }\n"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_cell_type_crosses_a_modules_interface() {
    // A module publishes a record holding a cell and a function that writes it; its importer
    // checks twice — the second time from the persisted interface — and runs.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("counter.nz"),
        "pub struct Counter { n: Cell[Int], }\n\
         pub fn counter() -> Counter { Counter(n: cell_new(0)) }\n\
         pub fn bump(c: Counter, by: Int) -> Int { cell_set(c.n, cell_get(c.n) + by) }\n\
         pub fn count(c: Counter) -> Int { cell_get(c.n) }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"counter.nz\";\n\
         fn main() -> Int { let c = counter(); bump(c, 20); bump(c, 22); count(c) }\n",
    )
    .unwrap();
    for _ in 0..2 {
        let c = run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["check", "main.nz"]),
            "nazm check",
        );
        assert_eq!(c.code, Some(0), "{c:?}");
    }
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "main.nz"]),
        "nazm run",
    );
    assert!(r.stdout.starts_with("42"), "{r:?}");
}
