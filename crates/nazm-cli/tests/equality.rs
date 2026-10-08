//! N13: derived equality compiled by `nazm build` exactly as `nazm run` evaluates it.
//!
//! # What is being held
//!
//! `crates/nazm-core/tests/equality.rs` holds the law — which types have `==` and what it
//! answers. What these cases hold is the native backend's side of it:
//!
//! * the compiled program gives the **stated** answer, and the interpreter's, at `-O0` and
//!   at `-O2`;
//! * comparing borrows: an owning operand stays valid until the answer exists, a temporary
//!   is released exactly once afterwards, and a bound operand is still usable — so every
//!   memory counter agrees between the two implementations and nothing stays live;
//! * an enum's helper reads only the active variant's slot, and a record's reads fields in
//!   layout order, so declaration order changes no byte of the program.
//!
//! # Why the churn
//!
//! A string released one step too early is often still readable. Allocating and discarding
//! small strings afterwards hands a prematurely freed block back out, and a wrong answer or
//! a signal shows it where the counters alone would not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod memory_report;

use memory_report::{counts, Ran};

use std::process::Command;

use nazm_lir::MEMORY_VARIABLE;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Run the compiled `prog` under a deadline: `(status, stdout, stderr)`.
///
/// A compiled program that never finishes is a wrong answer, not a slow one — an equality
/// helper whose loop over fields lost its exit produces exactly that — so it fails the case
/// here rather than holding the suite until some outer limit kills it with no verdict. The
/// output goes to files, so a program that writes a lot cannot block on a full pipe.
fn run_with_deadline(dir: &std::path::Path) -> (Option<i32>, String, String) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(60);
    let out_path = dir.join("prog.out");
    let err_path = dir.join("prog.err");
    let mut child = Command::new(dir.join("prog"))
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir)
        .stdout(std::fs::File::create(&out_path).expect("stdout file"))
        .stderr(std::fs::File::create(&err_path).expect("stderr file"))
        .spawn()
        .expect("execute");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the compiled program did not finish within {LIMIT:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    (
        status.code(),
        std::fs::read_to_string(&out_path).unwrap_or_default(),
        std::fs::read_to_string(&err_path).unwrap_or_default(),
    )
}

/// Interpret it, compile and run it at `-O0` and at `-O2`, require all three to agree about
/// everything, and require what they agree on to be `expected`, with nothing left live.
fn agree(source: &str, expected: &str) -> Ran {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let interpreted = Command::new(NAZM)
        .args(["run", "m.nz"])
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&interpreted.stderr).into_owned();
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "interpreting: {stderr}\n{source}"
    );
    let a = counts(
        &stderr,
        String::from_utf8_lossy(&interpreted.stdout)
            .trim()
            .to_owned(),
    );
    assert_eq!(a.value, expected, "the interpreter's answer\n{source}");

    for level in ["0", "2"] {
        let built = Command::new(NAZM)
            .args([
                "build",
                "m.nz",
                "-o",
                "prog",
                "--no-cache",
                "--opt-level",
                level,
            ])
            .current_dir(dir.path())
            .output()
            .expect("build");
        assert!(
            built.status.success(),
            "building at -O{level} failed: {}\n{source}",
            String::from_utf8_lossy(&built.stderr)
        );
        let (status, native_out, native_err) = run_with_deadline(dir.path());
        assert_eq!(
            status,
            Some(0),
            "the program at -O{level}: {native_err}\n{source}"
        );
        let b = counts(&native_err, native_out.trim().to_owned());
        assert_eq!(
            a, b,
            "the two implementations disagree at -O{level}\n{source}"
        );
    }
    for (name, c) in [
        ("sequences", a.sequences),
        ("strings", a.strings),
        ("channels", a.channels),
    ] {
        assert_eq!(c.live, 0, "{name} left live: {a:?}\n{source}");
        assert_eq!(c.allocated, c.reclaimed, "{name}: {a:?}\n{source}");
    }
    a
}

/// The IR `nazm build --emit-ir` writes for `source`.
fn ir_of(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");
    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog", "--emit-ir", "--no-cache"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "building failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    // One file per unit beside the output, named as the artefacts are: every one of them,
    // in name order, so two builds of one program give one text.
    let mut units: Vec<std::path::PathBuf> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ll"))
        .collect();
    units.sort();
    assert!(!units.is_empty(), "no IR was emitted");
    units
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("the IR"))
        .collect()
}

/// The text of one `define` in `ir`, from its line to its closing brace.
fn function<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let start = ir
        .lines()
        .find(|l| l.starts_with("define ") && l.contains(&format!("@{symbol}(")))
        .and_then(|l| ir.find(l))
        .unwrap_or_else(|| panic!("no definition of {symbol} in:\n{ir}"));
    let end = ir[start..].find("\n}").map_or(ir.len(), |e| start + e + 2);
    &ir[start..end]
}

/// The symbol of the only equality helper whose name contains `ty`.
fn helper(ir: &str, ty: &str) -> String {
    let found: Vec<&str> = ir
        .lines()
        .filter(|l| l.starts_with("define internal i1 @nz.h.eq."))
        .filter_map(|l| l.split('@').nth(1))
        .filter_map(|l| l.split('(').next())
        .filter(|s| s.contains(ty))
        .collect();
    assert_eq!(found.len(), 1, "{found:?}\n{ir}");
    found[0].to_owned()
}

/// Small strings made and thrown away, so that a block released too early is reused.
const CHURN: &str = "fn churn() -> Int {\n    let mut noise = vec_new[Str]();\n    let mut k = 0;\n    \
    while k < 64 {\n        vec_push(noise, str_concat(\"noise-\", int_to_str(k)));\n        k = k + 1;\n    }\n    \
    noise = vec_new[Str]();\n    vec_len(noise)\n}\n";

/// Owning types: a record, an enum whose variants each own something else, and a nest.
const TYPES: &str = "struct Item {\n    name: Str,\n    n: Int,\n}\n\
    enum Shape {\n    Plain,\n    Named(label: Str),\n    Pair(left: Str, right: Str),\n}\n\
    struct Outer {\n    item: Item,\n    shape: Shape,\n    note: Option[Str],\n}\n\
    fn text(n: Int) -> Str { str_concat(\"t-\", int_to_str(n)) }\n\
    fn item(n: Int) -> Item { Item(name: text(n), n: n) }\n\
    fn shape(k: Int) -> Shape {\n    if k % 3 == 0 { return Shape.Plain(); }\n    \
    if k % 3 == 1 { return Shape.Named(label: text(k)); }\n    Shape.Pair(left: text(k), right: text(k + 1))\n}\n\
    fn outer(n: Int) -> Outer { Outer(item: item(n), shape: shape(n), note: Option[Str].Some(value: text(n))) }\n\
    fn some_text(n: Int) -> Option[Str] { Option[Str].Some(value: text(n)) }\n";

fn with(body: &str) -> String {
    format!("{TYPES}{CHURN}{body}")
}

// --- the answers, compiled ---------------------------------------------------------------------

#[test]
fn record_equality_compiles_to_the_interpreter_s_answer() {
    let src = "struct Point { x: Int, y: Int, }\n\
               struct Named { name: Str, count: Int, flag: Bool, }\n\
               fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
               fn main() -> Int {\n    let p = Point(x: 1, y: 2);\n    \
               bit(p == Point(x: 1, y: 2), 1) + bit(p == Point(x: 9, y: 2), 2) + bit(p == Point(x: 1, y: 9), 4)\n    \
               + bit(p != Point(x: 1, y: 9), 8) + bit(p != p, 16)\n    \
               + bit(Named(name: str_concat(\"a\", \"b\"), count: 2, flag: true) == Named(flag: true, count: 2, name: \"ab\"), 32)\n    \
               + bit(Named(name: \"ab\", count: 2, flag: true) == Named(name: \"ab\", count: 2, flag: false), 64)\n}\n";
    agree(src, "41");
}

#[test]
fn enum_equality_compiles_to_the_interpreter_s_answer() {
    let src = "enum MaybeInt { None, Some(value: Int), }\n\
               enum Twins { Left(n: Int), Right(n: Int), }\n\
               enum P { Empty, Pair(left: Int, right: Str), }\n\
               enum Solo { Only, }\n\
               fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
               fn main() -> Int {\n    \
               bit(MaybeInt.None() == MaybeInt.None(), 1) + bit(MaybeInt.None() == MaybeInt.Some(value: 0), 2)\n    \
               + bit(MaybeInt.Some(value: 1) == MaybeInt.Some(value: 1), 4) + bit(MaybeInt.Some(value: 1) == MaybeInt.Some(value: 2), 8)\n    \
               + bit(Twins.Left(n: 1) == Twins.Right(n: 1), 16) + bit(Twins.Right(n: 1) != Twins.Right(n: 2), 32)\n    \
               + bit(P.Pair(left: 1, right: \"a\") == P.Pair(right: \"a\", left: 1), 64) + bit(P.Pair(left: 1, right: \"a\") == P.Pair(left: 1, right: \"b\"), 128)\n    \
               + bit(P.Empty() == P.Empty(), 256) + bit(Solo.Only() == Solo.Only(), 512)\n}\n";
    agree(src, "869");
}

#[test]
fn option_result_and_generic_instances_compile_to_the_interpreter_s_answer() {
    let src = "struct Box[T] { value: T, }\n\
               struct Point { x: Int, y: Int, }\n\
               fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
               fn main() -> Int {\n    \
               bit(Option[Int].None() == Option[Int].None(), 1) + bit(Option[Int].Some(value: 1) == Option[Int].Some(value: 1), 2)\n    \
               + bit(Option[Int].Some(value: 1) == Option[Int].Some(value: 2), 4)\n    \
               + bit(Option[Str].Some(value: str_concat(\"a\", \"b\")) == Option[Str].Some(value: \"ab\"), 8)\n    \
               + bit(Result[Int, Str].Ok(value: 1) == Result[Int, Str].Ok(value: 1), 16)\n    \
               + bit(Result[Int, Str].Ok(value: 1) == Result[Int, Str].Err(error: \"1\"), 32)\n    \
               + bit(Box[Int](value: 1) == Box[Int](value: 1), 64) + bit(Box[Str](value: \"a\") == Box[Str](value: \"b\"), 128)\n    \
               + bit(Box[Point](value: Point(x: 1, y: 2)) == Box[Point](value: Point(x: 1, y: 2)), 256)\n    \
               + bit(Box[Box[Int]](value: Box[Int](value: 4)) != Box[Box[Int]](value: Box[Int](value: 5)), 512)\n}\n";
    agree(src, "859");
}

#[test]
fn nested_composites_compile_to_the_interpreter_s_answer() {
    let src = "struct Name { text: Str, }\n\
               struct Person { name: Name, age: Int, }\n\
               enum Shape { Dot, Circle(centre: Person, r: Int), }\n\
               struct Drawing { shape: Shape, }\n\
               enum Layer { Hidden, Shown(top: Drawing), }\n\
               struct Box[T] { value: T, }\n\
               fn person(t: Str, age: Int) -> Person { Person(name: Name(text: t), age: age) }\n\
               fn layer(t: Str) -> Layer { Layer.Shown(top: Drawing(shape: Shape.Circle(centre: person(t, 1), r: 2))) }\n\
               fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
               fn main() -> Int {\n    \
               bit(person(\"a\", 1) == person(\"a\", 1), 1) + bit(person(\"a\", 1) == person(\"b\", 1), 2)\n    \
               + bit(layer(\"a\") == layer(\"a\"), 4) + bit(layer(\"a\") == layer(\"q\"), 8) + bit(layer(\"a\") == Layer.Hidden(), 16)\n    \
               + bit(Option[Person].Some(value: person(\"a\", 1)) == Option[Person].Some(value: person(\"a\", 1)), 32)\n    \
               + bit(Result[Shape, Person].Err(error: person(\"a\", 1)) == Result[Shape, Person].Ok(value: Shape.Dot()), 64)\n    \
               + bit(Box[Shape](value: Shape.Dot()) == Box[Shape](value: Shape.Dot()), 128)\n}\n";
    agree(src, "165");
}

// --- ownership ----------------------------------------------------------------------------------

#[test]
fn owning_temporaries_live_through_the_comparison_and_are_released_once() {
    // Every operand here is a temporary that owns heap strings: built, compared, and
    // released after the answer. Premature release reads freed bytes; a missed release is
    // a live string at the end; a double release is a count that disagrees.
    let r = agree(
        &with(
            "fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
             fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    while i < 6 {\n        \
             total = total + bit(item(i) == item(i), 1) + bit(item(i) != item(i + 1), 10);\n        \
             total = total + bit(shape(i) == shape(i), 100) + bit(shape(i) == shape(i + 1), 1000);\n        \
             total = total + bit(outer(i) == outer(i), 10000);\n        \
             total = total + bit(Option[Str].Some(value: text(i)) == Option[Str].Some(value: text(i)), 100000);\n        \
             total = total + bit(Result[Str, Str].Ok(value: text(i)) == Result[Str, Str].Err(error: text(i)), 1000000);\n        \
             total = total + bit(some_text(i) == some_text(i + 1), 10000000);\n        \
             churn();\n        i = i + 1;\n    }\n    total\n}\n",
        ),
        // Per turn: 1 + 10 + 100 + 0 (neighbouring shapes are different variants) + 10000
        // + 100000 + 0 (`Ok` against `Err`) + 0 (two returned temporaries holding different
        // strings of one length).
        "660666",
    );
    assert!(r.strings.allocated > 100, "{r:?}");
}

#[test]
fn a_compared_binding_is_still_its_own_afterwards() {
    // Equality observes and does not move: both bindings are read after the comparison
    // and after the churn, and each still holds its own string.
    agree(
        &with(
            "fn main() -> Int {\n    let a = outer(4);\n    let b = outer(4);\n    let c = outer(5);\n    \
             let same = a == b;\n    let differ = a != c;\n    churn();\n    let mut n = 0;\n    \
             if same { n = n + 1; }\n    if differ { n = n + 10; }\n    \
             n + 100 * (str_len(a.item.name) + str_len(b.item.name) + str_len(c.item.name))\n}\n",
        ),
        "911",
    );
}

#[test]
fn an_operand_that_leaves_releases_what_the_other_side_built() {
    // The right side returns part way through building its record, after the left side
    // built one owning a string: that string is released on the way out, and nothing is
    // compared.
    agree(
        &with(
            "fn right(k: Int) -> Int {\n    if item(k) == Item(name: text(k), n: { return 7; }) { 1 } else { 2 }\n}\n\
             fn left(k: Int) -> Int {\n    if Item(name: text(k), n: { return 8; }) == item(k) { 1 } else { 2 }\n}\n\
             fn main() -> Int { let a = right(1); churn(); let b = left(2); a * 10 + b }\n",
        ),
        "78",
    );
}

#[test]
fn equality_inside_a_value_block_and_after_propagation_compiles() {
    agree(
        &with(
            "fn found(ok: Bool) -> Result[Option[Str], Int] {\n    \
             if ok { Result[Option[Str], Int].Ok(value: Option[Str].Some(value: text(3))) } else { Result[Option[Str], Int].Err(error: 3) }\n}\n\
             fn compare(ok: Bool) -> Result[Int, Int] {\n    let got = found(ok)?;\n    \
             if got == Option[Str].Some(value: text(3)) { Result[Int, Int].Ok(value: 1) } else { Result[Int, Int].Ok(value: 0) }\n}\n\
             fn main() -> Int {\n    let s = shape(1);\n    let same = match s {\n        \
             Shape.Named(label: x) => {\n            let want = text(1);\n            x == want\n        },\n        \
             Shape.Plain() => { false },\n        Shape.Pair(left: _, right: _) => { false },\n    };\n    \
             churn();\n    let one = if same { 1 } else { 0 };\n    \
             let ten = match compare(true) { Result.Ok(value: v) => 10 * v, Result.Err(error: _) => 0, };\n    \
             let hundred = match compare(false) { Result.Ok(value: _) => 0, Result.Err(error: c) => 100 * c, };\n    \
             one + ten + hundred\n}\n",
        ),
        "311",
    );
}

// --- the emitted helpers ---------------------------------------------------------------------------

#[test]
fn an_enum_helper_reads_only_the_active_variant_s_slot() {
    // Different tags leave before any payload is read, and each arm of the `switch` reads one
    // variant's payload at the same offsets from both operands (N105: the helper is LIR's, its
    // fields bytes at `abi`'s offsets).
    let ir = ir_of(
        "enum Shape { Plain, Named(label: Str), Pair(left: Str, right: Int), }\n\
         fn main() -> Int { if Shape.Named(label: \"a\") == Shape.Plain() { 1 } else { 0 } }\n",
    );
    let body = function(&ir, &helper(&ir, "Shape"));
    let entry: &str = body.split("\nb").next().unwrap_or("");
    assert!(
        !entry.contains("getelementptr"),
        "a payload read before the tags agree:\n{body}"
    );
    assert!(entry.contains("icmp eq i64"), "{body}");
    // Canonical order: `Named` is tag 0, `Pair` 1, `Plain` 2 — and `Plain`, which carries
    // nothing, has no arm.
    assert!(body.contains("switch i64"), "{body}");
    assert!(
        body.contains("i64 0, label") && body.contains("i64 1, label"),
        "{body}"
    );
    assert!(
        !body.contains("i64 2, label"),
        "a zero-payload variant has an arm:\n{body}"
    );
    let offsets = |arm: &str, p: &str| -> Vec<String> {
        let mut o: Vec<String> = arm
            .lines()
            .filter(|l| l.contains(&format!("getelementptr i8, ptr {p}, i64 ")))
            .filter_map(|l| l.rsplit(' ').next().map(str::to_owned))
            .collect();
        o.sort();
        o
    };
    let mut arms = 0;
    for arm in body.split("\nb").skip(1) {
        let (a, b) = (offsets(arm, "%p0"), offsets(arm, "%p1"));
        if a.is_empty() && b.is_empty() {
            continue;
        }
        arms += 1;
        assert_eq!(a, b, "two different payloads compared:\n{arm}");
    }
    assert!(arms >= 2, "{body}");
    // `!=` calls the same helper and negates it: there is no second one.
    let ir = ir_of(
        "enum E { A(n: Int), B, }\nfn main() -> Int { if E.A(n: 1) != E.A(n: 1) { 1 } else { 0 } }\n",
    );
    assert_eq!(
        ir.matches("define internal i1 @nz.h.eq.").count(),
        1,
        "{ir}"
    );
    assert!(ir.contains("xor i1"), "{ir}");
}

#[test]
fn declaration_order_changes_no_byte_of_the_program() {
    // Fields, variants and payload fields written in another order — each edit keeps
    // every later offset where it was, so a runtime position baked into the program cannot
    // be what differs.
    let a = "struct P { a: Int, b: Str, }\nenum E { A(p: Int, q: Str), B, }\n\
             fn main() -> Int { let x = P(a: 1, b: \"s\"); let y = E.A(p: 1, q: \"t\"); \
             if x == P(b: \"s\", a: 1) { if y != E.B() { 1 } else { 2 } } else { 3 } }\n";
    let b = "struct P { b: Str, a: Int, }\nenum E { B, A(q: Str, p: Int), }\n\
             fn main() -> Int { let x = P(a: 1, b: \"s\"); let y = E.A(p: 1, q: \"t\"); \
             if x == P(b: \"s\", a: 1) { if y != E.B() { 1 } else { 2 } } else { 3 } }\n";
    assert_eq!(a.len(), b.len());
    assert_eq!(ir_of(a), ir_of(b));
    agree(a, "1");
    agree(b, "1");
}

#[test]
fn a_helper_is_named_by_the_type_s_durable_identity_and_its_arguments() {
    // `Box[Int]` and `Box[Str]` are two types and get two helpers; one comparison written
    // twice gets one. No helper is emitted for a type nobody compares.
    let ir = ir_of(
        "struct Box[T] { value: T, }\nstruct Unused { n: Int, }\n\
         fn main() -> Int {\n    let mut n = 0;\n    \
         if Box[Int](value: 1) == Box[Int](value: 1) { n = n + 1; }\n    \
         if Box[Int](value: 2) == Box[Int](value: 2) { n = n + 1; }\n    \
         if Box[Str](value: \"a\") == Box[Str](value: \"a\") { n = n + 1; }\n    n\n}\n",
    );
    let helpers: Vec<&str> = ir
        .lines()
        .filter(|l| l.starts_with("define internal i1 @nz.h.eq."))
        .collect();
    assert_eq!(helpers.len(), 2, "{helpers:#?}");
    assert_ne!(helpers[0], helpers[1]);
    assert!(helpers.iter().all(|h| h.contains("Box")), "{helpers:#?}");
}
