//! N14: a generic function that requires equality of `T`, compiled by `nazm build`
//! exactly as `nazm run` evaluates it.
//!
//! What is held here is that a requirement is a fact for the checker and nothing else:
//!
//! * a bounded body instantiated at `Int`, `Str`, a record, an enum, `Option`, `Result`
//!   and `Box` gives the stated answer at `-O0` and `-O2`, and the interpreter's;
//! * comparing through it observes its parameters: owning arguments stay valid, every
//!   temporary is released once, and nothing stays live;
//! * the native program carries no trace of the requirement — the instance of
//!   `same[T: Equality]` at `Int` is the same instructions as a hand-written
//!   `same_int(a: Int, b: Int)`, and a composite goes through N13's equality helper.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use nazm_lir::MEMORY_VARIABLE;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Class {
    allocated: u64,
    reclaimed: u64,
    live: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    value: String,
    sequences: Class,
    strings: Class,
    channels: Class,
}

fn counts(stderr: &str, value: String) -> Ran {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report in: {stderr}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    let class = |name: &str| {
        let at = words
            .iter()
            .position(|w| *w == name)
            .unwrap_or_else(|| panic!("no `{name}` in: {line}"));
        let field = |offset: usize, prefix: &str| {
            words
                .get(at + offset)
                .and_then(|w| w.strip_prefix(prefix))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("no `{prefix}` after `{name}` in: {line}"))
        };
        Class {
            allocated: field(1, "allocated="),
            reclaimed: field(2, "reclaimed="),
            live: field(3, "live="),
        }
    };
    Ran {
        value,
        sequences: class("sequences"),
        strings: class("strings"),
        channels: class("channels"),
    }
}

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

const CHURN: &str = "fn churn() -> Int {\n    let mut noise = vec_new[Str]();\n    let mut k = 0;\n    \
    while k < 64 {\n        vec_push(noise, str_concat(\"noise-\", int_to_str(k)));\n        k = k + 1;\n    }\n    \
    noise = vec_new[Str]();\n    vec_len(noise)\n}\n";

const DECLS: &str = "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
    fn wrap[T: Equality](a: T, b: T) -> Bool { same(a, b) }\n\
    fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
    fn text(n: Int) -> Str { str_concat(\"t-\", int_to_str(n)) }\n\
    struct Box[T] { value: T, }\n\
    struct Item { name: Str, n: Int, }\n\
    enum Shape { Plain, Named(label: Str), Pair(left: Str, right: Str), }\n\
    fn item(n: Int) -> Item { Item(name: text(n), n: n) }\n\
    fn shape(k: Int) -> Shape { if k % 3 == 0 { return Shape.Plain(); } if k % 3 == 1 { return Shape.Named(label: text(k)); } Shape.Pair(left: text(k), right: text(k + 1)) }\n\
    fn box_same[T: Equality](a: Box[T], b: Box[T]) -> Bool { a == b }\n";

fn with(body: &str) -> String {
    format!("{DECLS}{CHURN}{body}")
}

#[test]
fn a_bounded_body_compiles_to_the_answer_at_every_instance() {
    agree(
        &with(
            "struct Point { x: Int, y: Int, }\n\
             fn main() -> Int {\n    \
             bit(same(1, 1), 1) + bit(same[Int](1, 2), 2) + bit(same(text(1), \"t-1\"), 4)\n    \
             + bit(same(Point(x: 1, y: 2), Point(x: 1, y: 2)), 8) + bit(same(Shape.Plain(), Shape.Named(label: text(1))), 16)\n    \
             + bit(same(Option[Int].Some(value: 3), Option[Int].Some(value: 3)), 32)\n    \
             + bit(same(Result[Int, Str].Err(error: text(2)), Result[Int, Str].Err(error: text(2))), 64)\n    \
             + bit(same(Box[Int](value: 4), Box[Int](value: 5)), 128)\n    \
             + bit(box_same(Box[Option[Str]](value: Option[Str].Some(value: text(9))), Box[Option[Str]](value: Option[Str].Some(value: text(9)))), 256)\n    \
             + bit(wrap(Result[Box[Int], Str].Ok(value: Box[Int](value: 1)), Result[Box[Int], Str].Ok(value: Box[Int](value: 1))), 512)\n}\n",
        ),
        "877",
    );
}

#[test]
fn a_bounded_comparison_observes_owning_arguments_and_releases_its_temporaries_once() {
    let r = agree(
        &with(
            "fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    while i < 6 {\n        \
             total = total + bit(same(text(i), text(i)), 1) + bit(same(item(i), item(i + 1)), 10);\n        \
             total = total + bit(same(shape(i), shape(i)), 100) + bit(wrap(Option[Str].Some(value: text(i)), Option[Str].Some(value: text(i))), 1000);\n        \
             total = total + bit(same(Result[Str, Str].Ok(value: text(i)), Result[Str, Str].Ok(value: text(i))), 10000);\n        \
             churn();\n        i = i + 1;\n    }\n    \
             let a = item(4);\n    let b = item(4);\n    let s = same(a, b);\n    churn();\n    \
             total + bit(s, 100000) + 1000000 * (str_len(a.name) + str_len(b.name))\n}\n",
        ),
        "6166606",
    );
    assert!(r.strings.allocated > 50, "{r:?}");
}

#[test]
fn a_requirement_leaves_no_trace_in_the_program() {
    // The instance at `Int` is the concrete function, instruction for instruction; and no
    // symbol, global or argument in the program speaks of a capability.
    let generic = ir_of(
        "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
         fn main() -> Int { if same(1, 2) { 1 } else { 0 } }\n",
    );
    let concrete = ir_of(
        "fn same_int(a: Int, b: Int) -> Bool { a == b }\n\
         fn main() -> Int { if same_int(1, 2) { 1 } else { 0 } }\n",
    );
    let body = |ir: &str| -> Vec<String> {
        let start = ir
            .lines()
            .position(|l| l.starts_with("define ") && l.contains("same"))
            .unwrap_or_else(|| panic!("no instance in:\n{ir}"));
        ir.lines()
            .skip(start + 1)
            .take_while(|l| *l != "}")
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        body(&generic),
        body(&concrete),
        "\n{generic}\n---\n{concrete}"
    );
    for word in ["Equality", "equality", "dict", "vtable"] {
        assert!(!generic.contains(word), "`{word}` in:\n{generic}");
    }
    // A composite instance calls N13's helper, one per concrete type.
    let boxed = ir_of(
        "struct Box[T] { value: T, }\n\
         fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
         fn main() -> Int { if same(Box[Int](value: 1), Box[Int](value: 1)) { if same(Box[Str](value: \"a\"), Box[Str](value: \"b\")) { 2 } else { 1 } } else { 0 } }\n",
    );
    assert_eq!(
        boxed.matches("define internal i1 @nz.h.eq.").count(),
        2,
        "{boxed}"
    );
}
