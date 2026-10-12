//! R1a: immutable indirection, `Box[T]` and `List[T]` (`docs/spec.md`, *Immutable indirection*).
//!
//! A definition may refer to itself through one, so a syntax tree is a type. The tests ask every
//! implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift — for
//! what the section's rules work out by hand, with every allocation reclaimed; a million-deep
//! value released without the stack it would take to recurse; the ownership rule's refusals and
//! acceptances; the prelude names that keep every 1.0 program's meaning; provenance through both;
//! and a `Box` type crossing a module's published interface.

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

const TREES: &str = "enum Expr { Num(v: Int), Add(l: Box[Expr], r: Box[Expr]), Neg(e: Box[Expr]), }
struct Node { name: Str, kids: List[Node], }

fn show(out: OutCap, s: Str) -> Int ! {io} {
    print(str_concat(s, \"\\n\"))
}

fn eval(e: Expr) -> Int {
    match e {
        Expr.Num(v: v) => v,
        Expr.Add(l: l, r: r) => eval(box_get(l)) + eval(box_get(r)),
        Expr.Neg(e: x) => 0 - eval(box_get(x)),
    }
}

fn count(n: Node) -> Int {
    let mut c = 1;
    for k in n.kids { c = c + count(k); }
    c
}

fn main(out: OutCap) -> Int ! {io} {
    // A tree of boxes: 1 + -(5).
    let e = Expr.Add(l: box_new(Expr.Num(v: 1)), r: box_new(Expr.Neg(e: box_new(Expr.Num(v: 5)))));
    show(out, num_to_str(eval(e)));
    // A box is shared: two references, one value.
    let b = box_new(\"shared\");
    let c = b;
    show(out, str_concat(box_get(b), box_get(c)));
    // A list is a snapshot: what is pushed onto the vector afterwards does not reach it.
    let leaves = vec_new[Node]();
    vec_push(leaves, Node(name: \"a\", kids: list_empty[Node]()));
    vec_push(leaves, Node(name: \"b\", kids: list_empty[Node]()));
    let root = Node(name: \"r\", kids: list_from_vec(leaves));
    vec_push(leaves, Node(name: \"late\", kids: list_empty[Node]()));
    show(out, num_to_str(count(root)));
    show(out, num_to_str(list_len(root.kids)));
    show(out, list_get(root.kids, 1).name);
    // Nested: a list of lists.
    let row = vec_new[Int]();
    vec_push(row, 7);
    let rows = vec_new[List[Int]]();
    vec_push(rows, list_from_vec(row));
    vec_push(rows, list_empty[Int]());
    let grid = list_from_vec(rows);
    show(out, num_to_str(list_get(list_get(grid, 0), 0) + list_len(list_get(grid, 1))));
    0
}
";

/// What the section's rules give, worked out by hand.
const EXPECTED: &str = "-4\nsharedshared\n3\n2\nb\n7\n";

#[test]
fn trees_of_boxes_and_lists_mean_the_same_everywhere_and_are_reclaimed() {
    for (label, r) in everywhere(TREES) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

const DEEP: &str = "enum Chain { End, Link(v: Int, next: Box[Chain]), }
fn length(c: Chain) -> Int {
    let mut n = 0;
    let mut at = c;
    let mut going = true;
    while going {
        match at {
            Chain.End() => { going = false; },
            Chain.Link(v: v, next: next) => { n = n + 1; at = box_get(next); },
        }
    }
    n
}
fn main(out: OutCap) -> Int ! {io} {
    let mut c = Chain.End();
    let mut i = 0;
    while i < 1000000 {
        c = Chain.Link(v: i, next: box_new(c));
        i = i + 1;
    }
    print(str_concat(num_to_str(length(c)), \"\\n\"));
    0
}
";

#[test]
fn a_million_deep_value_is_released_without_a_million_frames() {
    // Release is a worklist, not recursion (`docs/spec.md`, *Release is not recursion*): before
    // R1a's queue, the interpreter overflowed its stack and a compiled program died releasing
    // this, its memory report never written.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("deep.nz"), DEEP).unwrap();
    let mut runs = vec![(
        "interpreter".to_owned(),
        run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["run", "deep.nz"])
                .env(nazm_lir::MEMORY_VARIABLE, "1"),
            "nazm run",
        ),
    )];
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "deep.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        runs.push((
            backend.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                backend,
            ),
        ));
    }
    for (label, r) in runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with("1000000\n"), "{label}: {}", r.stdout);
        let line = report(&r);
        assert!(
            line.contains("sequences allocated=1000000 reclaimed=1000000 live=0"),
            "{label}: {line}"
        );
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
fn the_ownership_rule_accepts_immutable_cycles_and_refuses_counted_ones() {
    let none = Vec::<String>::new();
    for (src, want) in [
        // Through an immutable indirection: accepted, alone or through a generic and `Option`.
        (
            "struct Node { next: Option[Box[Node]], }\nfn main() -> Int { 0 }",
            none.clone(),
        ),
        (
            "struct T[X] { v: X, kids: List[T[X]], }\nfn main() -> Int { 0 }",
            none.clone(),
        ),
        (
            "enum E { Leaf, Pair(a: Box[E], b: List[E]), }\nfn main() -> Int { 0 }",
            none.clone(),
        ),
        // Through a generic definition's parameter: the parameter's own use decides — a `List`
        // of it is immutable, so a cycle through it is accepted.
        (
            "struct W[T] { items: List[T], }\nstruct Node { w: W[Node], }\nfn main() -> Int { 0 }",
            none.clone(),
        ),
        // Through a mutable handle anywhere on the cycle: still an ownership cycle.
        (
            "struct W[T] { items: Vec[T], }\nstruct Node { w: W[Node], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        // A counted edge inside an immutable one, and the other way round, through a parameter.
        (
            "struct H[T] { b: Box[Vec[T]], }\nstruct Node { h: H[Node], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        (
            "struct H[T] { b: Vec[Box[T]], }\nstruct Node { h: H[Node], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        (
            "struct Node { kids: Vec[Node], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        (
            "struct Node { kids: Box[Vec[Node]], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        (
            "struct A { kids: Vec[B], }\nstruct B { up: Box[A], }\nfn main() -> Int { 0 }",
            vec!["N0359".to_owned()],
        ),
        // Inline only: still no layout.
        (
            "struct A { b: B, }\nstruct B { a: A, }\nfn main() -> Int { 0 }",
            vec!["N0336".to_owned()],
        ),
    ] {
        assert_eq!(codes(src), want, "{src}");
    }
}

#[test]
fn a_box_or_list_is_neither_ordered_nor_given_to_a_task_nor_written_bare() {
    // Equal by what it holds since L6, but not ordered.
    assert_eq!(
        codes("fn main() -> Int { let a = box_new(1); if a == a { 1 } else { 0 } }"),
        Vec::<String>::new()
    );
    assert_eq!(
        codes("fn main() -> Int { let a = box_new(1); if a < a { 1 } else { 0 } }"),
        ["N0304"]
    );
    assert_eq!(
        codes(
            "fn w(l: List[Int]) -> Int { 0 }\n\
             fn main() -> Int ! {spawn} { let l = list_empty[Int](); scope { spawn w(l); } 0 }"
        )
        .first()
        .map(String::as_str),
        Some("N0321")
    );
    assert_eq!(
        codes("fn f(b: Box) -> Int { 0 }\nfn main() -> Int { 0 }"),
        ["N0352"]
    );
    // A closure may capture a `List` that holds function values: nothing can be added to it.
    // A `Vec` that may hold one is still refused (`N0616`).
    assert_eq!(
        codes(
            "fn main() -> Int { let fs = list_empty[fn() -> Int](); \
             let f = fn() -> Int { list_len(fs) }; f() }"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main() -> Int { let fs = vec_new[fn() -> Int](); \
             let f = fn() -> Int { vec_len(fs) }; f() }"
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
fn the_new_names_are_prelude_names_a_1_0_program_keeps() {
    // A 1.0 program's own `Box` type and `box_new` function mean what they meant.
    let r = run(
        "struct Box { v: Int, }\nfn box_new(x: Int) -> Box { Box(v: x + 1) }\n\
         fn main() -> Int { box_new(41).v }\n",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.starts_with("42"), "{}", r.stdout);
    let r = run("struct List { n: Int, }\nfn main() -> Int { List(n: 9).n }\n");
    assert!(r.stdout.starts_with('9'), "{r:?}");
    // The built-ins of 1.0 are still never shadowed.
    assert_eq!(
        codes("fn vec_new(x: Int) -> Int { x }\nfn main() -> Int { 0 }"),
        ["N0203"]
    );
}

#[test]
fn what_an_indirection_holds_carries_its_provenance() {
    // A file's contents in a box, or in a list made from a vector, may not name a written file
    // (`N0372`); a literal through the same may.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let b = box_new(read_file(\"cfg\")); \
             write_file(box_get(b), \"x\") }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let b = box_new(\"out.txt\"); \
             write_file(box_get(b), \"x\") }\n"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let v = vec_new[Str](); \
             vec_push(v, read_file(\"cfg\")); let l = list_from_vec(v); \
             for p in l { write_file(p, \"x\"); } 0 }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let v = vec_new[Str](); \
             vec_push(v, \"out.txt\"); let l = list_from_vec(v); \
             write_file(list_get(l, 0), \"x\") }\n"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_box_type_crosses_a_modules_interface() {
    // A module publishes a recursive type and a function over it; its importer builds, checks
    // twice — the second time from the persisted interface — and runs.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("tree.nz"),
        "pub enum T { Leaf(v: Int), Node(l: Box[T], r: Box[T]), }\n\
         pub fn sum(t: T) -> Int { match t { T.Leaf(v: v) => v, \
         T.Node(l: l, r: r) => sum(box_get(l)) + sum(box_get(r)), } }\n\
         pub fn pair(a: Int, b: Int) -> T { T.Node(l: box_new(T.Leaf(v: a)), r: box_new(T.Leaf(v: b))) }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"tree.nz\";\nfn main() -> Int { sum(pair(20, 22)) }\n",
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
