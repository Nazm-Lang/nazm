//! R1a: ordering and hashing are derived (`docs/spec.md`, *Ordering and hashing are derived*),
//! and a `Box` or `List` compares by what it holds.
//!
//! Every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift
//! — is asked for what the section's rules work out by hand, with every allocation reclaimed:
//! orders, maps keyed by tuples, records and enums, and derived hashes against the library's own
//! functions; then that reordering a declaration changes no order and no hash; then what is
//! refused, by which code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

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

fn nothing_live(r: &Ran) -> bool {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .is_some_and(|l| {
            l.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0")
        })
}

fn run_everywhere(src: &str, expected: &str) {
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(expected), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

const ORDERS: &str = "struct P { y: Int, x: Int, }
struct Q { tag: Str, at: P, small: Int8, flag: Bool, }
enum L { Zed, Alpha(n: Int), Mid(s: Str), }
enum Expr { Num(v: Int), Neg(e: Box[Expr]), }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }
fn b(c: Bool) -> Str { if c { \"1\" } else { \"0\" } }

fn main(out: OutCap) -> Int ! {io} {
    // Strings by bytes, a prefix first; `false` before `true`.
    show(out, str_concat(b(\"apple\" < \"banana\"), str_concat(b(\"ab\" < \"a\"),
        str_concat(b(\"\" < \"a\"), b(false < true)))));
    // A record by its fields in the order of their names: `x` before `y`.
    show(out, str_concat(b(P(y: 9, x: 1) < P(y: 0, x: 2)), b(P(y: 1, x: 5) < P(y: 2, x: 5))));
    // Nested, with a narrow signed integer and a `Bool`: `at`, `flag`, `small`, `tag`.
    let q1 = Q(tag: \"z\", at: P(y: 1, x: 1), small: Int8(-3), flag: true);
    let q2 = Q(tag: \"a\", at: P(y: 1, x: 1), small: Int8(2), flag: true);
    show(out, str_concat(b(q1 < q2), str_concat(b(q1 >= q1), b(q2 > q1))));
    // An enum by its variants' names — `Alpha`, `Mid`, `Zed` — then the payload.
    show(out, str_concat(b(L.Alpha(n: 5) < L.Zed()), str_concat(b(L.Mid(s: \"a\") < L.Alpha(n: 0)),
        str_concat(b(L.Alpha(n: 1) < L.Alpha(n: 2)), b(L.Mid(s: \"b\") <= L.Mid(s: \"a\"))))));
    // A tuple by position.
    show(out, str_concat(b((1, \"b\") < (2, \"a\")), str_concat(b((1, \"b\") <= (1, \"b\")),
        b((2, P(y: 0, x: 0)) > (2, P(y: 0, x: 1))))));
    // A `Box` and a `List` compare by what they hold, through a recursive type.
    let e1 = Expr.Neg(e: box_new(Expr.Neg(e: box_new(Expr.Num(v: 3)))));
    let e2 = Expr.Neg(e: box_new(Expr.Neg(e: box_new(Expr.Num(v: 3)))));
    let e3 = Expr.Neg(e: box_new(Expr.Num(v: 3)));
    let xs = vec_new[Str]();
    vec_push(xs, \"a\");
    let l1 = list_from_vec(xs);
    let l2 = list_from_vec(xs);
    show(out, str_concat(b(e1 == e2), str_concat(b(e1 != e3), str_concat(b(l1 == l2),
        b(l1 == list_empty[Str]())))));
    0
}
";

/// By hand: `q1 < q2` is decided by `small` (`at` and `flag` equal, `small` before `tag`), and
/// `Mid(s: "b") <= Mid(s: "a")` by the payload.
const ORDERS_EXPECTED: &str = "1011\n11\n111\n1010\n110\n1110\n";

#[test]
fn derived_orders_mean_the_same_everywhere_and_are_reclaimed() {
    run_everywhere(ORDERS, ORDERS_EXPECTED);
}

/// Derived hashes and maps keyed by them. `Point`'s fields are declared `y` then `x`; its hash
/// folds `x` first, by name.
fn hashes(point: &str, key: &str) -> String {
    format!(
        "use \"@std/hash\";
use \"@std/hashmap\";
{point}
{key}

fn show(out: OutCap, s: Str) -> Int ! {{io}} {{ print(str_concat(s, \"\\n\")) }}
fn b(c: Bool) -> Str {{ if c {{ \"1\" }} else {{ \"0\" }} }}

fn main(out: OutCap) -> Int ! {{io}} {{
    let m = hashmap_new[(Str, Point), Int]();
    hashmap_set(m, (\"a\", Point(y: 2, x: 1)), 7);
    hashmap_set(m, (\"b\", Point(y: 2, x: 1)), 8);
    hashmap_set(m, (\"a\", Point(y: 2, x: 1)), 9);
    let k = hashmap_new[Key, Int]();
    hashmap_set(k, Key.One(s: \"z\", n: Int8(-1), f: true), 1);
    hashmap_set(k, Key.Two(), 2);
    hashmap_set(k, Key.One(s: \"z\", n: Int8(-1), f: true), 3);
    let got = match hashmap_get(m, (\"a\", Point(y: 2, x: 1))) {{
        Option.Some(value: v) => v,
        Option.None() => 0,
    }};
    show(out, num_to_str(hashmap_len(m) * 100 + got * 10 + hashmap_len(k)));
    // Each by the definition, in the library's own functions.
    let p = Hash.hash(Point(y: 5, x: 3));
    let p_def = hash_combine(hash_combine(hash_u64(UInt64(0)), Hash.hash(3)), Hash.hash(5));
    let e = Hash.hash(Key.One(s: \"z\", n: Int8(-1), f: true));
    let e_def = hash_combine(hash_combine(hash_combine(hash_bytes(\"One\"), Hash.hash(true)),
        Hash.hash(Int8(-1))), Hash.hash(\"z\"));
    let t = Hash.hash((7, \"q\"));
    let t_def = hash_combine(hash_combine(hash_u64(UInt64(0)), Hash.hash(7)), Hash.hash(\"q\"));
    let z = Hash.hash(Key.Two());
    show(out, str_concat(b(p == p_def), str_concat(b(e == e_def), str_concat(b(t == t_def),
        b(z == hash_bytes(\"Two\"))))));
    show(out, str_concat(b(Hash.same((1, \"a\"), (1, \"a\"))), b(Hash.same(Point(y: 1, x: 2), Point(y: 2, x: 1)))));
    // The hash itself, which reordering a declaration must not change.
    show(out, num_to_str(Int(p >> 40)));
    show(out, num_to_str(Int(e >> 40)));
    0
}}
"
    )
}

#[test]
fn derived_hashes_follow_the_definition_everywhere() {
    let src = hashes(
        "struct Point { y: Int, x: Int, }",
        "enum Key { Two, One(s: Str, n: Int8, f: Bool), }",
    );
    let (_, first) = everywhere(&src).into_iter().next().unwrap();
    let lines: Vec<&str> = first.stdout.lines().collect();
    // Two `(Str, Point)` keys, the first's value replaced by 9; two `Key`s.
    assert_eq!(&lines[..3], ["292", "1111", "10"], "{}", first.stdout);
    run_everywhere(
        &src,
        &first.stdout.lines().take(5).collect::<Vec<_>>().join("\n"),
    );
}

#[test]
fn reordering_a_declaration_changes_no_order_and_no_hash() {
    let a = hashes(
        "struct Point { y: Int, x: Int, }",
        "enum Key { Two, One(s: Str, n: Int8, f: Bool), }",
    );
    let b = hashes(
        "struct Point { x: Int, y: Int, }",
        "enum Key { One(f: Bool, n: Int8, s: Str), Two, }",
    );
    let out = |src: &str| {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["run", "p.nz"]),
            "nazm run",
        )
        .stdout
    };
    let (x, y) = (out(&a), out(&b));
    assert!(x.lines().count() >= 5, "{x}");
    assert_eq!(x, y);
    let swap = |src: &str| {
        src.replace(
            "struct P { y: Int, x: Int, }",
            "struct P { x: Int, y: Int, }",
        )
        .replace(
            "enum L { Zed, Alpha(n: Int), Mid(s: Str), }",
            "enum L { Mid(s: Str), Alpha(n: Int), Zed, }",
        )
    };
    assert_ne!(swap(ORDERS), ORDERS);
    assert_eq!(out(&swap(ORDERS)), out(ORDERS));
}

/// `nazm check` on `src`: every error code, in order, and the first message.
fn refused(src: &str) -> (Vec<String>, String) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    let codes = r
        .stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect();
    let first = r
        .stderr
        .lines()
        .find(|l| l.starts_with("error["))
        .unwrap_or("")
        .to_owned();
    (codes, first)
}

#[test]
fn what_has_no_order_or_derived_hash_and_by_which_code() {
    for (src, code, says) in [
        // A component with no order, named.
        (
            "struct R { v: Vec[Int], }\nfn main() -> Int { let a = R(v: vec_new[Int]()); if a < a { 1 } else { 0 } }",
            "N0304",
            "its `v` is a `Ints`, which has no order", // `Vec[Int]` (R1, 5b.18)
        ),
        // A float orders on its own, but makes no structure ordered.
        (
            "struct F { x: Float64, }\nfn main() -> Int { if F(x: 1.0) < F(x: 2.0) { 1 } else { 0 } }",
            "N0304",
            "which has no order",
        ),
        // Through an indirection, nothing orders.
        (
            "fn main() -> Int { if box_new(1) < box_new(2) { 1 } else { 0 } }",
            "N0304",
            "",
        ),
        // An unconstrained parameter has no order.
        (
            "fn f[T](a: T, b: T) -> Bool { a < b }\nfn main() -> Int { 0 }",
            "N0304",
            "",
        ),
        // A key whose component cannot be hashed.
        (
            "use \"@std/hash\";\nuse \"@std/hashmap\";\nstruct R { w: (Int, Vec[Int]), }\n\
             fn main() -> Int { let m = hashmap_new[R, Int](); 0 }",
            "N0364",
            "does not derive one: its `w.1` is a `Ints`",
        ),
        // A component with a written impl: its `same` may mean more than `==`.
        (
            "use \"@std/hash\";\nuse \"@std/hashmap\";\nstruct K { n: Int, }\n\
             impl Hash for K { fn hash(self: K) -> UInt64 ! {} { hash_u64(UInt64(0)) } \
             fn same(self: K, other: K) -> Bool ! {} { true } }\nstruct W { k: K, }\n\
             fn main() -> Int { let m = hashmap_new[W, Int](); 0 }",
            "N0364",
            "its `k` is a `K`",
        ),
        // Inside a generic body only a concrete type derives.
        (
            "use \"@std/hash\";\nstruct P[T] { v: T, }\n\
             fn f[T: Hash](x: P[T]) -> UInt64 { Hash.hash(x) }\nfn main() -> Int { 0 }",
            "N0605",
            "",
        ),
        // A ninth tuple element is refused.
        (
            "fn main() -> Int { let t = (1, 2, 3, 4, 5, 6, 7, 8, 9); 0 }",
            "N0101",
            "at most eight",
        ),
    ] {
        let (codes, first) = refused(src);
        assert_eq!(codes.first().map(String::as_str), Some(code), "{src}: {codes:?}");
        assert_eq!(codes.len(), 1, "{src}: one mistake, one error: {codes:?}");
        assert!(first.contains(says), "{src}: {first}");
    }
}

#[test]
fn a_type_with_a_written_impl_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "use \"@std/hash\";\nuse \"@std/hashmap\";\nstruct K { n: Int, }\n\
         impl Hash for K { fn hash(self: K) -> UInt64 ! {} { hash_u64(UInt64(0)) } \
         fn same(self: K, other: K) -> Bool ! {} { true } }\n\
         fn main() -> Int { let m = hashmap_new[K, Int](); hashmap_set(m, K(n: 1), 1); \
         hashmap_set(m, K(n: 2), 2); hashmap_len(m) }\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "p.nz"]),
        "nazm run",
    );
    // Every `K` is the same key by its own `same`: one entry, not two.
    assert!(r.stdout.starts_with("1\n"), "{r:?}");
}
