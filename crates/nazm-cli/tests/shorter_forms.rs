//! R1.5 (`docs/spec.md`, *Shorter forms*): each spelling means what the longer one did.
//!
//! D1 — a result type written after `:`, in a function, a trait method, a function type and a
//! function expression; `->` still read beside it.
//! D2 — an expression body, `= e`, and a private function's result type taken from it.
//! D3 — a short closure, `|a, b| a < b`, its types taken from the function type expected.
//! D4 — fields by position, `Point(1, 2)`, and a variant without its enum, `Circle(2)`, `Dot`.
//! D5 — map literals, `{"a": 1}`, and `Map[K, V]`.
//! D6 — `[1, 2, 3]` is a list where no fixed array is expected.
//! D7 — `use std.json`, and `List[T]` the growable list, R1a's sequence `Chain[T]`.
//! D8 — printing in your own program's functions without passing `out`.
//! D9 — the prelude's standard modules, imported where a program's module names them.
//! D10 — `m.top(n)` and `m.sort_by(less)` on maps.
//! 4.5 — `@std/option` and `@std/result` among D9's prelude modules: `r.unwrap_or(1)` with no `use`.
//! 4.6 — a line break separates `match` arms, as it ends a statement.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn a_result_type_after_a_colon_means_the_arrow_everywhere() {
    differential::agrees(
        "trait Shape { fn area(self: Self): Int; fn name(self: Self): Str { \"shape\" } }
struct Sq { side: Int, }
impl Shape for Sq { fn area(self: Sq): Int { self.side * self.side } }

fn fib(n: Int): Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn apply(f: fn(Int): Int, x: Int): Int { f(x) }

fn old(n: Int) -> Int { n + 1 }

fn parse(s: Str): Result[Int, Str] {
    if s == \"\" { Err(\"empty\") } else { Ok(str_len(s)) }
}

fn main(out: OutCap): Int ! {io} {
    let twice = fn(n: Int): Int { n * 2 }
    let k = match parse(\"abc\") { Ok(n) => n, Err(e) => 0 }
    print(f\"{fib(15)} {apply(twice, 21)} {Sq(side: 3).area()} {Sq(side: 1).name()} {old(1)} {k}\\n\")
    0
}
",
        "610 42 9 shape 2 3\n0",
    );
}

#[test]
fn fmt_keeps_the_result_marker_as_written_until_r2() {
    // The tree's own sources stay in the forms the Nazm-written compiler reads until R2, and
    // the formatter holds them as written; it neither adds nor removes a `:` or a `->`.
    let d = tempfile::tempdir().unwrap();
    let src =
        "fn a(n: Int): Int { n }\nfn b(n: Int) -> Int { n }\nfn main() { println(a(1) + b(2)) }\n";
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "--check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
}

#[test]
fn a_colon_without_a_type_is_refused_where_the_type_was_expected() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn f(n: Int): { n }\nfn main() { }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("type"), "{r:?}");
}

#[test]
fn an_expression_body_and_its_inferred_result_type_mean_the_block_everywhere() {
    differential::agrees(
        // `main` first, and `quad` before what it calls: each inferred type is settled before
        // any caller is checked, wherever the caller is written.
        "fn main(out: OutCap): Int ! {io} {
    let f = quad
    print(f\"{double(21)} {add(2, 3)} {greet(\"nazm\")} {f(5)} {even(4)} {first(7, 8)} {pick(Some(9))}\\n\")
    0
}

fn quad(n: Int) = add(double(n), double(n))
fn double(n: Int): Int = n * 2
fn add(a: Int, b: Int) = a + b
fn greet(name: Str) = f\"hello {name}\"
fn even(n: Int) = n % 2 == 0
fn first[T](a: T, b: T) = a
fn pick(o: Option[Int]) = match o { Some(v) => v, None => 0 }
",
        "42 5 hello nazm 20 true 7 9\n0",
    );
}

#[test]
fn a_result_type_that_must_be_written_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    for (src, why) in [
        ("pub fn p(n: Int) = n\nfn main() { }\n", "`pub`"),
        (
            "fn r(n: Int) = if n < 1 { 0 } else { r(n - 1) }\nfn main() { }\n",
            "reaches itself",
        ),
        (
            "fn a(n: Int) = b(n)\nfn b(n: Int) = a(n)\nfn main() { }\n",
            "reaches itself",
        ),
        (
            "trait T { fn m(self: Self): Int; }\nimpl T for Int { fn m(self: Int) = self }\nfn main() { }\n",
            "method",
        ),
    ] {
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        let r = nazm(d.path(), &["check", "p.nz"]);
        assert_ne!(r.code, Some(0), "{src}: {r:?}");
        assert!(r.stderr.contains("N0634"), "{src}: {r:?}");
        assert!(r.stderr.contains(why), "{src}: {r:?}");
    }
}

#[test]
fn a_short_closure_takes_its_types_from_what_is_expected_everywhere() {
    differential::agrees(
        "fn apply(f: fn(Int, Int) -> Bool, a: Int, b: Int) -> Bool { f(a, b) }
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }

fn main(out: OutCap): Int ! {io} {
    let inc = |n: Int| n + 1
    let k = 10
    let add_k = |n: Int| { n + k }
    let zero = || 0
    let shout = |s: Str| str_concat(s, \"!\")
    let typed: fn(Int) -> Int = |x| x - 1
    print(f\"{apply(|a, b| a < b, 1, 2)} {apply(|a, b| a > b, 1, 2)} {twice(|x| x * 3, 2)} \")
    print(f\"{inc(41)} {add_k(5)} {zero()} {typed(1)} {shout(\"hi\")}\\n\")
    0
}
",
        "true false 18 42 15 0 0 hi!\n0",
    );
}

#[test]
fn a_short_closure_parameter_with_nothing_to_take_its_type_from_is_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() { let f = |a, b| a < b }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert_eq!(r.stderr.matches("N0635").count(), 2, "{r:?}");
}

#[test]
fn fields_by_position_and_variants_without_their_enum_mean_the_long_forms_everywhere() {
    differential::agrees(
        "enum Shape { Circle(r: Int), Rect(w: Int, h: Int), Dot, }
struct Point { x: Int, y: Int, }
struct Twice { n: Int, }

// A function of a capitalised name is still called by it, a record of that name or not.
fn Twice(n: Int) -> Int { n * 2 }

fn area(s: Shape) -> Int {
    match s { Circle(r) => 3 * r * r, Rect(w, h) => w * h, Dot => 0, }
}

fn describe(s: Shape) -> Str {
    match s {
        Shape.Circle(r) if r > 5 => \"big\",
        Circle(_) => \"circle\",
        Shape.Rect(w: w, h: _) => f\"rect {w}\",
        Dot => \"dot\",
    }
}

fn main(out: OutCap): Int ! {io} {
    let p = Point(1, 2)
    // A built-in type's `Name(x)` is still a conversion, never a construction.
    let b = Int(UInt8(3))
    let shapes: Vec[Shape] = [Circle(2), Shape.Rect(3, 4), Dot, Circle(b)]
    let mut total = p.x * 100 + p.y * 10 + Twice(1)
    for s in shapes {
        total += area(s)
        print(f\"{describe(s)} \")
    }
    print(f\"{total}\\n\")
    0
}
",
        "circle rect 3 dot circle 173\n0",
    );
}

#[test]
fn a_shared_variant_name_and_a_mixed_or_short_field_list_are_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    for (src, code, why) in [
        (
            "enum A { X(n: Int), }\nenum B { X(n: Int), }\nfn main() { let a = X(1) }\n",
            "N0636",
            "both",
        ),
        (
            "enum E { V(x: Int, y: Int), }\nfn main() { let a = E.V(1, y: 2) }\n",
            "N0334",
            "both by position and by name",
        ),
        (
            "struct P { x: Int, y: Int, }\nfn main() { let p = P(1) }\n",
            "N0334",
            "given by position",
        ),
    ] {
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        let r = nazm(d.path(), &["check", "p.nz"]);
        assert_ne!(r.code, Some(0), "{src}: {r:?}");
        assert_eq!(
            r.stderr.matches("error[").count(),
            1,
            "one error: {src}: {r:?}"
        );
        assert!(
            r.stderr.contains(code) && r.stderr.contains(why),
            "{src}: {r:?}"
        );
    }
}

#[test]
fn a_secret_bound_by_position_is_still_a_secret() {
    // A file's contents, carried in a payload and bound by position, still reach the path of
    // the write that uses it (`N0372`), as they do bound by name.
    let d = tempfile::tempdir().unwrap();
    for pattern in ["Wrap(p)", "Holder.Wrap(v: p)"] {
        let src = format!(
            "enum Holder {{ Wrap(v: Str), }}\n\
             fn main(io: IoCap) -> Int ! {{ io }} {{\n\
             let h = Holder.Wrap(v: read_file(\"cfg\"));\n\
             match h {{ {pattern} => {{ write_file(p, \"x\"); }}, }}\n\
             0\n}}\n"
        );
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        let r = nazm(d.path(), &["check", "p.nz"]);
        assert!(r.stderr.contains("N0372"), "{pattern}: {r:?}");
    }
}

#[test]
fn a_map_literal_builds_the_map_everywhere() {
    differential::agrees(
        "use \"@std/hash\";
use \"@std/hashmap\";

fn total(m: Map[Str, Int]) -> Int { m[\"a\"] + m[\"b\"] }

fn main(out: OutCap): Int ! {io} {
    let k = \"c\"
    let m = {\"a\": 1, \"b\": 2, k: 3}
    let ages: Map[Str, Int] = {\"x\": 40}
    m[\"d\"] = 4
    let squares = {1: 1, 2: 4, 3: 9}
    print(f\"{total(m)} {hashmap_len(m)} {ages[\"x\"]} {squares[3]} {hashmap_len(squares)}\\n\")
    0
}
",
        "3 4 40 9 3\n0",
    );
}

#[test]
fn an_empty_map_is_braces_where_a_map_is_expected_everywhere() {
    // R1.5, D5: `{}` is the empty map where a `Map[K, V]` is expected — a typed `let`, an
    // argument, a field, a result, a `match` arm — and the empty block everywhere else, where a
    // value wanted of it is refused with help that writes the type.
    differential::agrees(
        "struct Tally { counts: Map[Str, Int] }

fn total(m: Map[Str, Int]): Int {
    let mut n = 0
    for (k, v) in m.top(10) { n += v }
    n
}

fn fresh(): Map[Str, Int] = {}

fn main() {
    let mut m: Map[Str, Int] = {}
    m[\"a\"] += 2
    m[\"b\"] = 3
    let t = Tally(counts: {})
    let mut c = t.counts
    c[\"x\"] = 1
    let picked: Map[Str, Int] = match m.len() {
        2 => {},
        _ => fresh(),
    }
    if true {}
    println(f\"{total(m)} {total({})} {t.counts.len()} {fresh().len()} {picked.len()}\")
}
",
        "5 0 1 0 0\n0",
    );
    let codes = differential::codes("fn main() {\n    let m = {}\n    println(m)\n}\n");
    assert!(codes.iter().any(|c| c == "N0300"), "{codes:?}");
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() {\n    let m = {}\n    println(m)\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert!(
        r.stderr.contains("let m: Map[Str, Int] = {}"),
        "the help writes the type: {r:?}"
    );
}

#[test]
fn map_without_the_hash_map_names_the_import() {
    // Since D9 the prelude gives a program's module the hash map when it names `Map`; a
    // dependency's module imports only what it writes, so there `Map` still names the import.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn f(m: Map[Str, Int]) -> Int { 0 }\nfn main() { }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    for (rel, text) in [
        (
            "lib/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\n",
        ),
        ("lib/src/f.nz", "pub fn f(m: Map[Str, Int]) -> Int { 0 }\n"),
        (
            "app/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"1.0.0\"\n\
             main = \"main.nz\"\n[dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
        ),
        ("app/src/main.nz", "use \"lib:f.nz\";\nfn main() { }\n"),
    ] {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let r = differential::nazm(d.path(), &["check", "app"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("@std/hashmap"), "{r:?}");
}

#[test]
fn a_standard_module_by_its_dotted_path_is_the_quoted_import_everywhere() {
    differential::agrees(
        "use std.hash
use std.hashmap
use std.text as t

fn main(out: OutCap): Int ! {io} {
    let m = {\"a\": 1}
    print(f\"{m[\"a\"] + str_len(t::text_to_upper(\"abc\"))}\\n\")
    0
}
",
        "4\n0",
    );
}

#[test]
fn the_programs_own_functions_print_without_being_passed_out_everywhere() {
    differential::agrees(
        "fn greet(name: Str) {
    println(f\"hello {name}\")
}

fn twice(name: Str) {
    greet(name)
    greet(name)
}

fn main(out: OutCap): Int ! {io} {
    twice(\"nazm\")
    0
}
",
        "hello nazm\nhello nazm\n0",
    );
}

#[test]
fn a_packages_function_still_needs_out_and_files_still_need_io() {
    let d = tempfile::tempdir().unwrap();
    let write = |rel: &str, text: &str| {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    write(
        "lib/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\n",
    );
    // `say` holds nothing and is refused; `shout` holds an `IoCap`, which gives an `OutCap`.
    write(
        "lib/src/say.nz",
        "pub fn say(s: Str) { println(s) }\npub fn shout(io: IoCap, s: Str) { println(s) }\n",
    );
    write(
        "app/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"1.0.0\"\nmain = \"main.nz\"\n\
         [dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
    );
    write(
        "app/src/main.nz",
        "use \"lib:say.nz\";\nfn main() { say(\"hi\") }\n",
    );
    // The dependency's function prints with nothing passed: refused there, in its own module.
    let r = nazm(d.path(), &["check", "app"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(
        r.stderr.contains("N0369") && r.stderr.contains("`say`"),
        "{r:?}"
    );
    assert!(
        !r.stderr.contains("`shout`"),
        "an `IoCap` gives an `OutCap`: {r:?}"
    );
    // In the program itself, a file is still written only with an `IoCap` passed.
    std::fs::write(
        d.path().join("p.nz"),
        "fn save(p: Str) { write_file(p, \"x\") }\nfn main() { }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert!(
        r.stderr.contains("N0369") && r.stderr.contains("IoCap"),
        "{r:?}"
    );
}

#[test]
fn list_is_the_growable_list_everywhere() {
    // `List[T]` is `Vec[T]` (D7): pushed to, indexed, passed where a `Vec[Int]` or an `Ints` is
    // expected, a field's type and a generic parameter's.
    differential::agrees(
        "struct Bag { items: List[Int], }

fn total(v: Vec[Int]): Int {
    let mut s = 0
    for x in v { s += x }
    s
}

fn first[T](xs: List[T]): T { xs[0] }

fn names(): List[Str] {
    let n: List[Str] = vec_new[Str]()
    n.push(\"a\")
    n
}

fn main(out: OutCap): Int ! {io} {
    let v: List[Int] = vec_new[Int]()
    v.push(1)
    v.push(2)
    let w: Ints = v
    let b = Bag(items: v)
    ints_push(b.items, 3)
    print(f\"{total(v)} {ints_len(w)} {first(names())} {b.items[2]} {first(v)}\\n\")
    0
}
",
        "6 3 a 3 1\n0",
    );
}

#[test]
fn a_chain_is_what_r1a_called_a_list_everywhere() {
    // `Chain[T]` and `chain_*` (D7) are R1a's immutable sequence: a copy of the vector as it was,
    // a recursive type through it, `for`, and equality by what it holds.
    differential::agrees(
        "struct Node { name: Str, kids: Chain[Node], }

fn count(n: Node): Int {
    let mut c = 1
    for k in n.kids { c += count(k) }
    c
}

fn main(out: OutCap): Int ! {io} {
    let leaves = vec_new[Node]()
    leaves.push(Node(name: \"a\", kids: chain_empty[Node]()))
    leaves.push(Node(name: \"b\", kids: chain_empty[Node]()))
    let root = Node(name: \"r\", kids: chain_from_vec(leaves))
    leaves.push(Node(name: \"late\", kids: chain_empty[Node]()))
    let xs: List[Int] = [1, 2]
    let c = chain_from_vec(xs)
    let same = c == chain_from_vec(xs)
    print(f\"{count(root)} {chain_len(root.kids)} {chain_get(root.kids, 1).name} {chain_len(c)} {same}\\n\")
    0
}
",
        "3 2 b 2 true\n0",
    );
}

#[test]
fn an_old_list_built_in_is_refused_naming_its_chain_one() {
    let d = tempfile::tempdir().unwrap();
    for (src, new) in [
        ("fn main() { let l = list_empty[Int]() }\n", "chain_empty"),
        (
            "fn main() { let l = chain_empty[Int](); let n = list_len(l) }\n",
            "chain_len",
        ),
        (
            "fn main() { let v = vec_new[Int](); list_from_vec(v) }\n",
            "chain_from_vec",
        ),
    ] {
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        let r = nazm(d.path(), &["check", "p.nz"]);
        assert_ne!(r.code, Some(0), "{src}: {r:?}");
        assert_eq!(r.stderr.matches("error[").count(), 1, "{src}: {r:?}");
        assert!(
            r.stderr.contains("N0637") && r.stderr.contains(new),
            "{src}: {r:?}"
        );
    }
    // A module's own function of the name keeps it, as a prelude name's would.
    std::fs::write(
        d.path().join("p.nz"),
        "fn list_len(n: Int) -> Int { n }\nfn main() -> Int { list_len(0) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
}

#[test]
fn a_list_literal_is_a_shared_list_and_a_typed_one_an_array_everywhere() {
    // D6: `[1, 2]` where no `[T; n]` is expected is a `Vec[Int]`, one list every name for it
    // shares; written where its type says `[Int; 2]` it is an array, copied on assignment.
    differential::agrees(
        "fn total(xs: Vec[Int]): Int {
    let mut t = 0
    for x in xs { t += x }
    t
}

fn main(out: OutCap): Int ! {io} {
    let a = [1, 2]
    let b = a
    b.push(3)
    let c: [Int; 2] = [1, 2]
    let mut d = c
    d[0] = 10
    let words = [\"x\", \"y\"]
    print(f\"{a.len()} {b.len()} {a[2]} {total(a)} {c[0]} {d[0]} {words.len()}\\n\")
    0
}
",
        "3 3 3 6 1 10 2\n0",
    );
}

#[test]
fn an_empty_list_with_no_type_to_take_is_refused_and_a_typed_one_is_not() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), "fn main() { let v = [] }\n").unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0618"), "{r:?}");
    assert!(
        r.stderr.contains("List[Int]"),
        "the help writes the type: {r:?}"
    );
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() {\n    let v: List[Int] = []\n    println(v.len())\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
}

#[test]
fn the_prelude_imports_the_standard_modules_a_program_names_everywhere() {
    // D9 and D10: no `use` — `split` is `@std/text`'s, `hashmap_new` and `top` `@std/hashmap`'s,
    // `compare` `@std/ordmap`'s, `sort_by` `@std/sort`'s, and a map literal's `hashmap_single`.
    differential::agrees(
        "fn count(text: Str): Map[Str, Int] {
    let counts = hashmap_new[Str, Int]()
    for w in text.split(\" \") {
        if w.len() > 0 { counts[w] += 1 }
    }
    counts
}

fn main() {
    let counts = count(\"b a c a b a d e e e\")
    for (w, n) in counts.top(3) { println(f\"{w} {n}\") }
    println(f\"{counts.top(0).len()} {counts.top(99).len()} {\"x\".compare(\"y\")}\")
    let mut keys = \"\"
    for (k, _) in counts.sort_by(|a, b| a.0 < b.0) { keys = keys + k }
    let m = {\"one\": 1}
    println(f\"{keys} {m[\"one\"]}\")
}
",
        "a 3\ne 3\nb 2\n0 5 -1\nabcde 1\n0",
    );
}

#[test]
fn a_module_s_own_name_hides_the_prelude_s_and_a_written_use_does_not() {
    // D9: the module's `text_split` is the one called, with no collision; written, the same
    // import collides as it always did (`N0208`).
    differential::agrees(
        "fn text_split(s: Str, sep: Str): Int = 7

fn main() {
    println(text_split(\"a b\", \" \"))
    println(\"a b\".split(\" \"))
}
",
        "7\n7\n0",
    );
    let codes = differential::check_codes(&[(
        "p.nz",
        "use \"@std/text\"\n\nfn text_split(s: Str, sep: Str): Int = 7\n\nfn main() {\n    println(text_split(\"a b\", \" \"))\n}\n",
    )]);
    assert_eq!(codes, ["N0208"]);
}

#[test]
fn a_module_that_names_none_of_them_imports_none() {
    // D9: a restriction profile holds a program to everything it imports, so attaching the
    // prelude's modules to every module would refuse every embedded program. `v.len()` is a
    // built-in's, and names none; `s.split` names `@std/text`, which `embedded` refuses.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main(): Int {\n    let v = vec_new[Int]()\n    vec_push(v, 1)\n    v.len()\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "embedded", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    std::fs::write(
        d.path().join("p.nz"),
        "fn main(): Int {\n    \"a b\".split(\" \").len()\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "embedded", "p.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("@std/text"), "{r:?}");
}

#[test]
fn a_package_s_module_imports_only_what_it_writes() {
    // D9 is the program's: a dependency's module that calls `text_split` with no `use` is
    // refused there, so what a package reaches stays written in it.
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in [
        (
            "lib/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\n",
        ),
        (
            "lib/src/words.nz",
            "pub fn words(s: Str): Int { text_split(s, \" \").len() }\n",
        ),
        (
            "app/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"1.0.0\"\n\
             main = \"main.nz\"\n[dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
        ),
        (
            "app/src/main.nz",
            "use \"lib:words.nz\";\nfn main(): Int { words(\"a b\") + \"c d\".split(\" \").len() }\n",
        ),
    ] {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let r = differential::nazm(d.path(), &["check", "app"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(
        r.stderr.contains("N0201") && r.stderr.contains("words.nz"),
        "the package's call is refused in its own module: {r:?}"
    );
    assert!(
        !r.stderr.contains("main.nz"),
        "and the program's `split` is not: {r:?}"
    );
}

#[test]
fn option_and_result_methods_need_no_use_everywhere() {
    // 4.5: `@std/option` and `@std/result` are prelude modules, attached where a method of
    // theirs is called; `unwrap_or` names both, and the receiver's type picks the function.
    differential::agrees(
        "fn parse(s: Str): Result[Int, Str] {
    if s == \"x\" { Err(\"bad\") } else { Ok(5) }
}

fn main() {
    let r = parse(\"x\")
    let o: Option[Int] = None
    println(f\"{r.unwrap_or(1)} {o.unwrap_or(0)} {parse(\"y\").unwrap_or(1)}\")
    println(f\"{r.is_err()} {r.is_ok()} {o.is_none()} {parse(\"y\").ok().is_some()}\")
    println(option_unwrap_or(r.err(), \"none\"))
}
",
        "1 0 5\ntrue false true true\nbad\n0",
    );
}

#[test]
fn a_module_s_own_option_function_hides_the_prelude_s_and_a_profile_sees_what_is_named() {
    // 4.5: a module's own `option_unwrap_or` is what `o.unwrap_or` calls, and a method called
    // only in an interpolation's hole still names its module; the two modules are pure, so
    // `--profile embedded` accepts a program that names them.
    differential::agrees(
        "fn option_unwrap_or(o: Option[Int], fallback: Int): Int = 42

fn main() {
    let o: Option[Int] = None
    let r: Result[Int, Int] = Err(2)
    println(f\"{o.unwrap_or(0)} {r.unwrap_or(3)}\")
}
",
        "42 3\n0",
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main(): Int {\n    let o: Option[Int] = None\n    o.unwrap_or(3)\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "embedded", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    // `unwrap_or` names both modules; `is_none` only `@std/option`.
    let r = nazm(d.path(), &["inspect", "p.nz"]);
    assert!(
        r.stdout.contains("@std/result::fn result_unwrap_or"),
        "{r:?}"
    );
    std::fs::write(
        d.path().join("p.nz"),
        "fn main(): Int {\n    let o: Option[Int] = None\n    if o.is_none() { 3 } else { 4 }\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["inspect", "p.nz"]);
    assert!(r.stdout.contains("@std/option::fn option_is_none"), "{r:?}");
    assert!(
        !r.stdout.contains("@std/result"),
        "only what is named: {r:?}"
    );
}

#[test]
fn match_arms_on_their_own_lines_need_no_comma_everywhere() {
    // 4.6: a line break separates two arms; `,` is still read, the two mix, an arm's expression
    // still spans lines (a line beginning with `*` continues it), and a line beginning with `(`
    // begins a pattern, not a call.
    differential::agrees(
        "enum Shape {
    Circle(r: Int),
    Rect(w: Int, h: Int),
    Dot,
}

fn area(s: Shape): Int {
    match s {
        Circle(r) => 3 * r * r
        Rect(w, h) => w
            * h
        Dot => 0
    }
}

fn name(n: Int): Str {
    match n {
        0 => \"zero\",
        1 => \"one\"
        n if n < 0 => {
            \"negative\"
        }
        (_) => \"many\"
    }
}

fn main() {
    println(f\"{area(Circle(2))} {area(Rect(3, 4))} {area(Dot)}\")
    println(f\"{name(0)} {name(1)} {name(-5)} {name(9)}\")
}
",
        "12 12 0\nzero one negative many\n0",
    );
}

#[test]
fn two_arms_on_one_line_still_need_their_comma_and_fmt_keeps_what_was_written() {
    // 4.6: only a line break stands for the `,`; and `nazm fmt` neither adds nor removes one.
    let codes = differential::codes("fn main(): Int {\n    match 1 { 0 => 1 _ => 2 }\n}\n");
    assert_eq!(codes, ["N0100"]);
    let d = tempfile::tempdir().unwrap();
    let src = "fn f(n: Int): Int {\n    match n {\n        0 => 1,\n        1 => 2\n        _ => 3\n    }\n}\n";
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "--check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(std::fs::read_to_string(d.path().join("p.nz")).unwrap(), src);
}

#[test]
fn a_for_over_a_split_reads_its_pieces_without_a_list_everywhere() {
    // R1.5, 3.3: `for w in s.split(sep)` is lowered through `text_split_end` and
    // `text_split_next`, building no list; the pieces, the empty ones, `break` and `continue`
    // are the list's.
    let src = "fn show(s: Str, sep: Str) {
    let mut out = \"\"
    let mut n = 0
    for w in s.split(sep) {
        out = out + \"[\" + w + \"]\"
        n += 1
        if w == \"stop\" { break }
        if w == \"skip\" { continue }
    }
    println(f\"{n} {out}\")
}

fn main() {
    show(\"a b  c\", \" \")
    show(\"\", \" \")
    show(\" \", \" \")
    show(\"abc\", \"\")
    show(\"a--b--\", \"--\")
    show(\"a-b--c-\", \"--\")
    show(\"x skip y stop z\", \" \")
    for w in text_split(\"p,q\", \",\") { print(w) }
    println(\"\")
}
";
    differential::agrees(
        src,
        "4 [a][b][][c]\n1 []\n2 [][]\n1 [abc]\n3 [a][b][]\n2 [a-b][c-]\n4 [x][skip][y][stop]\npq\n0",
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = nazm(d.path(), &["core-ir", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    let calls = |name: &str| {
        r.stdout
            .matches(&format!("\"callee\": \"@std/text::fn {name}\""))
            .count()
    };
    assert_eq!(
        calls("text_split_end"),
        2,
        "each loop reads its pieces one at a time"
    );
    // `text_split` is called as often as in a program with no loop over one: only inside
    // `@std/text` itself, never by the program's loops.
    let in_loops = calls("text_split");
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() {\n    println(text_split(\"a b\", \" \").len())\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["core-ir", "p.nz"]);
    let once = r
        .stdout
        .matches("\"callee\": \"@std/text::fn text_split\"")
        .count();
    assert_eq!(in_loops + 1, once, "no list is built");
}

#[test]
fn a_for_over_file_lines_reads_a_chunk_at_a_time_everywhere() {
    // R1.5, 3.5: `for line in file_lines(io, path)` is lowered through `@std/file`'s reader,
    // which reads 64 KiB at a time and builds no list; its lines are `text_lines(read_file(path))`'s
    // — a carriage return cut at a chunk's edge, a line longer than a chunk, empty lines, a last
    // line without a newline — `break` and `continue` are the list's, and every reader is closed.
    let mut input = "x".repeat(65535) + "\r\n";
    for i in 0..3000 {
        input += &format!("line {i}{}", if i % 3 == 0 { "\r\n" } else { "\n" });
        if i % 7 == 0 {
            input += "\n";
        }
    }
    input += "skip\nstop\n";
    input += &"y".repeat(200_000);
    input += "\nend";
    let lines: Vec<&str> = input
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let total: usize = lines.iter().map(|l| l.len()).sum();
    let src = "use \"@std/file\"

fn main(io: IoCap) {
    let mut n = 0
    let mut total = 0
    for line in file_lines(io, \"in.txt\") {
        if line == \"skip\" { continue }
        n += 1
        total += line.len()
    }
    let all = text_lines(read_file(\"in.txt\"))
    let mut sum = 0
    for l in all { sum += l.len() }
    println(f\"{n} {total} {all.len()} {sum}\")
    let mut seen = 0
    for line in file_lines(io, \"in.txt\") {
        if line == \"stop\" { break }
        seen += 1
    }
    println(f\"{seen} {file_lines(io, \"in.txt\").len()}\")
}
";
    let expected = format!(
        "{} {} {} {}\n{} {}\n0",
        lines.len() - 1,
        total - 4,
        lines.len(),
        total,
        lines.len() - 3,
        lines.len()
    );
    differential::agrees_on(&[("prog.nz", src), ("in.txt", &input)], &expected, 0);
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = nazm(d.path(), &["core-ir", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    let calls = |name: &str| {
        r.stdout
            .matches(&format!("\"callee\": \"@std/file::fn {name}\""))
            .count()
    };
    assert_eq!(calls("file_lines_close"), 2, "each loop closes its reader");
    // `file_lines` itself is called once, for the list: the loops build none.
    assert_eq!(calls("file_lines"), 1, "only the list is built");
}
