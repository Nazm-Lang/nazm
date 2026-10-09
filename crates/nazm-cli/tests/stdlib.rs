//! N45: the standard library — `use "@std/…"`.
//!
//! `docs/architecture.md` §7.47 and `docs/spec.md` *The standard library*. What only whole
//! programs show: that each standard module is found by name and only by name; that its functions
//! behave as documented at their edges under the interpreter, both optimisation levels of the LLVM
//! backend and the Cranelift backend, and reclaim everything; that the ones reaching the outside
//! world need the authority their signatures show and carry the origins of what they read; that
//! ordinary failure is a value; that the spec's example runs; and that every public function is
//! documented where the spec lists it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

#[derive(Debug)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn ran(o: &std::process::Output) -> Ran {
    Ran {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// `files` written to a directory, `prog.nz` run by the interpreter and three native builds.
fn four(files: &[(&str, &str)], args: &[&str]) -> Vec<(String, Ran)> {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let path = dir.path().join("prog.nz");
    let mut out = vec![(
        "interpreter".to_owned(),
        ran(&Command::new(NAZM)
            .current_dir(dir.path())
            .arg("run")
            .arg(&path)
            .arg("--")
            .args(args)
            .output()
            .unwrap()),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.path().join(label.replace([' ', '-'], ""));
        let b = Command::new(NAZM)
            .current_dir(dir.path())
            .args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
            ])
            .arg(&path)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            b.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&b.stderr)
        );
        let mut c = common::bounded_exe(&exe);
        c.current_dir(dir.path())
            .args(args)
            .env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), ran(&c.output().unwrap())));
    }
    out
}

fn agrees(files: &[(&str, &str)], expected: &str) {
    for (label, r) in four(files, &[]) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, expected, "{label}");
        if label != "interpreter" {
            let report = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: {}", r.stderr));
            assert_eq!(report.matches("live=0").count(), 4, "{label}: {report}");
        }
    }
}

fn codes(files: &[(&str, &str)]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let o = Command::new(NAZM)
        .current_dir(dir.path())
        .args(["check", "--json", "prog.nz"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect()
}

const MODULES: &[&str] = &[
    "chan", "deque", "env", "error", "file", "fmt", "fs", "hash", "hashmap", "hashset", "io",
    "ioerror", "json", "map", "net", "num", "option", "path", "process", "result", "seq", "show",
    "sort", "test", "text", "time",
];

#[test]
fn every_standard_module_is_found_by_name_and_checks_cleanly() {
    for m in MODULES {
        let src = format!("use \"@std/{m}\";\nfn main() -> Int {{ 0 }}\n");
        assert!(codes(&[("prog.nz", &src)]).is_empty(), "@std/{m}");
    }
    // Only by name: an unknown one or a file name is refused, and a path beginning with `@std/`
    // is never looked for on disk — a project file of that very name is there, and is not read.
    for bad in ["@std/nope", "@std/text.nz"] {
        let src = format!("use \"{bad}\";\nfn main() -> Int {{ 0 }}\n");
        let beside = "pub fn f() -> Int { 1 }\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("@std")).unwrap();
        std::fs::write(dir.path().join(bad), beside).unwrap();
        std::fs::write(dir.path().join("prog.nz"), &src).unwrap();
        let o = Command::new(NAZM)
            .current_dir(dir.path())
            .args(["check", "--json", "prog.nz"])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            !o.status.success() && err.contains("\"N0205\""),
            "{bad}: {err}"
        );
    }
    // Any other `@` path is an ordinary relative path, as it always was.
    let src = "use \"@elsewhere/lib.nz\";\nfn main() -> Int { f() }\n";
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("@elsewhere")).unwrap();
    std::fs::write(
        dir.path().join("@elsewhere/lib.nz"),
        "pub fn f() -> Int { 1 }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("prog.nz"), src).unwrap();
    let o = Command::new(NAZM)
        .current_dir(dir.path())
        .args(["check", "prog.nz"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_standard_module_is_reused_from_the_cache_like_any_module() {
    // Its key, `@std/text`, reads back from a stored entry as itself.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("nazm.root"), "").unwrap();
    std::fs::write(
        dir.path().join("prog.nz"),
        "use \"@std/text\";\nuse \"@std/seq\";\nfn main() -> Int { str_len(text_trim(\" a \")) }\n",
    )
    .unwrap();
    let report = |o: &std::process::Output| -> (u64, u64) {
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let line = text
            .lines()
            .find(|l| l.contains("nazm.check-report/1"))
            .unwrap()
            .to_owned();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        (
            v["checked"].as_u64().unwrap(),
            v["reused"].as_u64().unwrap(),
        )
    };
    let check = || {
        Command::new(NAZM)
            .current_dir(dir.path())
            .args(["check", "prog.nz", "--cache-report"])
            .output()
            .unwrap()
    };
    let (checked, _) = report(&check());
    assert!(checked >= 3);
    let (checked, reused) = report(&check());
    assert_eq!(
        (checked, reused),
        (0, 4),
        "the program, two standard modules and the prelude"
    );
}

#[test]
fn a_standard_module_is_one_module_however_many_import_it() {
    let lib = "use \"@std/text\";\npub fn shout(s: Str) -> Str ! {} { str_concat(text_trim(s), \"!\") }\n";
    let prog = "use \"lib.nz\";\nuse \"@std/text\";\nfn main(io: IoCap) -> Int { print(shout(\"  hi \")); print(text_repeat(\"-\", 2)); 0 }\n";
    agrees(&[("lib.nz", lib), ("prog.nz", prog)], "hi!--0\n");
    // Its names are the importer's: defining one of them beside the import is a collision.
    let clash = "use \"@std/text\";\nfn text_trim(s: Str) -> Str { s }\nfn main() -> Int { 0 }\n";
    assert!(codes(&[("prog.nz", clash)]).contains(&"N0208".to_owned()));
}

/// Gate 2 §7: a map of strings and of integers through a thousand inserts and growth, removal past
/// the tombstones it leaves, a set of a program's own records keyed by an impl composing the
/// library's, the hash vectors (FNV-1a then SplitMix64's finaliser, computed independently), and a
/// deque wrapping both ways — alike interpreted, under LLVM and under Cranelift, all reclaimed.
const COLLECTIONS: &str = r#"use "@std/hash";
use "@std/hashmap";
use "@std/hashset";
use "@std/deque";

struct Point { x: Int, y: Int, }

impl Hash for Point {
    fn hash(self: Point) -> UInt64 ! {} { hash_combine(self.x.hash(), self.y.hash()) }
    fn same(self: Point, other: Point) -> Bool ! {} { self == other }
}

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, "\n")) }

fn get_or(m: HashMap[Str, Int], k: Str) -> Int {
    match hashmap_get(m, k) {
        Option.Some(value: v) => v,
        Option.None() => 0 - 1,
    }
}

fn main(out: OutCap) -> Int ! {io} {
    let m = hashmap_new[Str, Int]();
    hashmap_set(m, "one", 1);
    hashmap_set(m, "two", 2);
    if hashmap_set(m, "one", 11) { show(out, "replaced"); } else { show(out, "new"); };
    show(out, int_to_str(get_or(m, "one") + get_or(m, "two") + get_or(m, "three")));
    let big = hashmap_new[Int, Int]();
    let mut i = 0;
    while i < 1000 {
        hashmap_set(big, i, i * i);
        i = i + 1;
    }
    let mut j = 0;
    while j < 1000 {
        if j % 3 == 0 { hashmap_remove(big, j); };
        j = j + 1;
    }
    let mut total = 0;
    let vs = hashmap_values(big);
    let mut k = 0;
    while k < vec_len(vs) { total = total + vec_get(vs, k); k = k + 1; }
    show(out, int_to_str(hashmap_len(big)));
    show(out, int_to_str(total));
    hashmap_set(big, 3, 7);
    show(out, int_to_str(hashmap_len(big)));
    let pts = hashset_new[Point]();
    hashset_add(pts, Point(x: 1, y: 2));
    if hashset_add(pts, Point(x: 1, y: 2)) { show(out, "added twice"); } else { show(out, "already there"); };
    hashset_add(pts, Point(x: 2, y: 1));
    show(out, int_to_str(hashset_len(pts)));
    if hashset_has(pts, Point(x: 2, y: 1)) && !hashset_has(pts, Point(x: 3, y: 3)) { show(out, "set ok"); } else { show(out, "set wrong"); };
    show(out, num_to_str(hash_bytes("")));
    show(out, num_to_str(hash_bytes("nazm")));
    show(out, num_to_str(UInt8(7).hash()));
    let d = deque_new[Int]();
    let mut n = 0;
    while n < 10 {
        deque_push_back(d, n);
        deque_push_front(d, 0 - n);
        n = n + 1;
    }
    let mut line = "";
    let mut p = 0;
    while p < deque_len(d) {
        match deque_get(d, p) {
            Option.Some(value: v) => { line = str_concat(line, str_concat(int_to_str(v), " ")); },
            Option.None() => {},
        };
        p = p + 1;
    }
    show(out, line);
    match deque_pop_front(d) { Option.Some(value: v) => show(out, int_to_str(v)), Option.None() => show(out, "empty"), };
    match deque_pop_back(d) { Option.Some(value: v) => show(out, int_to_str(v)), Option.None() => show(out, "empty"), };
    show(out, int_to_str(deque_len(d)));
    0
}
"#;

#[test]
fn hash_maps_sets_and_deques_behave_alike_everywhere() {
    agrees(
        &[("prog.nz", COLLECTIONS)],
        "replaced\n12\n666\n221555889\n667\nalready there\n2\nset ok\n\
         17665956581633026203\n10414939609503865103\n12548494866695815710\n\
         -9 -8 -7 -6 -5 -4 -3 -2 -1 0 0 1 2 3 4 5 6 7 8 9 \n-9\n9\n18\n0\n",
    );
}

/// Gate 2 §13: an `IoError` becomes an `Error` that propagates with `?`, gathers what each caller
/// was doing, and reads outermost first; a C `errno` maps to its kind.
const ERRORS: &str = r#"use "@std/ioerror";
use "@std/error";
use "@std/file";
fn load(io: IoCap, path: Str) -> Result[Int, Error] ! { io } {
    let b = error_io_result(file_read_all(io, path))?;
    Result[Int, Error].Ok(value: bytes_len(b))
}
fn config(io: IoCap) -> Result[Int, Error] ! { io } {
    match load(io, "missing.cfg") {
        Result.Ok(value: n) => Result[Int, Error].Ok(value: n),
        Result.Err(error: e) => Result[Int, Error].Err(error: error_context(e, "loading the configuration")),
    }
}
fn main(io: IoCap) -> Int {
    match config(io) {
        Result.Ok(value: n) => print("loaded\n"),
        Result.Err(error: e) => print(str_concat(error_show(e), "\n")),
    };
    print(str_concat(error_show(error_context(error_context(error_new("bad digit"), "line 3"), "reading the table")), "\n"));
    print(str_concat(io_kind_name(io_error_from_errno(2).kind), "\n"));
    0
}
"#;

#[test]
fn an_error_gathers_context_and_reads_outermost_first_everywhere() {
    agrees(
        &[("prog.nz", ERRORS)],
        "loading the configuration: No such file or directory (NotFound)\n\
         reading the table: line 3: bad digit\nNotFound\n0\n",
    );
}

#[test]
fn a_seed_changes_a_maps_order_and_nothing_else() {
    let src = "use \"@std/hash\";\nuse \"@std/hashmap\";\n\
        fn keys(m: HashMap[Int, Int]) -> Str {\n\
            let ks = hashmap_keys(m);\n\
            let mut s = \"\";\n\
            let mut i = 0;\n\
            while i < vec_len(ks) { s = str_concat(s, int_to_str(vec_get(ks, i))); i = i + 1; }\n\
            s\n\
        }\n\
        fn fill(m: HashMap[Int, Int]) -> Int { let mut i = 0; while i < 6 { hashmap_set(m, i, i); i = i + 1; } 0 }\n\
        fn main(out: OutCap) -> Int {\n\
            let a = hashmap_new[Int, Int]();\n\
            let b = hashmap_with_seed[Int, Int](UInt64(99));\n\
            fill(a); fill(b);\n\
            let c = hashmap_with_seed[Int, Int](UInt64(0));\n\
            fill(c);\n\
            print(if keys(a) == keys(b) { \"same order\\n\" } else { \"other order\\n\" });\n\
            print(if hashmap_len(a) == hashmap_len(b) { \"same length\\n\" } else { \"other length\\n\" });\n\
            print(if keys(a) == keys(c) { \"reproducible\\n\" } else { \"not reproducible\\n\" });\n\
            0\n\
        }\n";
    agrees(
        &[("prog.nz", src)],
        "other order\nsame length\nreproducible\n0\n",
    );
}

#[test]
fn text_behaves_as_documented_at_its_edges() {
    let prog = r#"use "@std/text";
use "@std/option";
use "@std/result";
fn b(x: Bool) -> Str { if x { "T" } else { "F" } }
fn o(x: Option[Int]) -> Str { match x { Option.Some(value: v) => int_to_str(v), Option.None() => "none" } }
fn p(s: Str) -> Str { match text_parse_int(s) { Result.Ok(value: v) => int_to_str(v), Result.Err(error: e) => str_concat("err:", e) } }
fn line(io: IoCap, s: Str) -> Int { print(s); print("|") }
fn main(io: IoCap) -> Int {
    line(io, b(text_starts_with("abc", ""))); line(io, b(text_starts_with("abc", "ab"))); line(io, b(text_starts_with("ab", "abc")));
    line(io, b(text_ends_with("abc", "bc"))); line(io, b(text_ends_with("abc", "abcd"))); line(io, b(text_ends_with("", "")));
    line(io, o(text_find("aaa", "aa"))); line(io, o(text_find("abc", ""))); line(io, o(text_find("", "a"))); line(io, o(text_find("abcabc", "ca")));
    line(io, b(text_contains("hello", "ll"))); line(io, b(text_contains("hello", "lo!")));
    line(io, text_repeat("ab", 3)); line(io, text_repeat("x", 0)); line(io, text_repeat("x", 0 - 3));
    let s = text_split("a,,b,", ","); line(io, int_to_str(strs_len(s))); line(io, str_join(s, "/"));
    line(io, int_to_str(strs_len(text_split("", ",")))); line(io, str_join(text_split("a,b", ""), "/"));
    line(io, str_join(text_split("a--b--c", "--"), "+"));
    let ws = str_concat(str_concat(str_from_byte(9), " x y "), str_concat(str_from_byte(10), str_from_byte(13)));
    line(io, text_trim(ws)); line(io, text_trim("   ")); line(io, text_trim("")); line(io, b(text_is_space(11))); line(io, b(text_is_space(14)));
    line(io, p("0")); line(io, p("-0")); line(io, p("+7")); line(io, p("007")); line(io, p("")); line(io, p("-")); line(io, p("12a"));
    line(io, p("9223372036854775807")); line(io, p("-9223372036854775808")); line(io, p("9223372036854775808")); line(io, p("-9223372036854775809")); line(io, p("99999999999999999999"));
    0
}
"#;
    let expected = "T|T|F|T|F|T|0|0|none|2|T|F|ababab|||4|a//b/|1|a,b|a+b+c|x y|||T|F|0|0|7|7|err:no digits|err:no digits|err:not a digit|9223372036854775807|-9223372036854775808|err:out of range|err:out of range|err:out of range|0\n";
    agrees(&[("prog.nz", prog)], expected);
}

#[test]
fn option_result_and_seq_helpers_work_for_every_type() {
    let prog = r#"use "@std/option";
use "@std/result";
use "@std/seq";
struct P { x: Int, name: Str }
fn i(n: Int) -> Str { int_to_str(n) }
fn b(x: Bool) -> Str { if x { "T" } else { "F" } }
fn show(io: IoCap, s: Str) -> Int { print(s); print("|") }
fn main(io: IoCap) -> Int {
    let some = Option[Str].Some(value: "v"); let none = Option[Str].None();
    show(io, b(option_is_some(some))); show(io, b(option_is_none(none))); show(io, option_unwrap_or(none, "fallback")); show(io, option_unwrap_or(some, "fallback"));
    let ok = Result[P, Str].Ok(value: P(x: 3, name: "p")); let bad = Result[P, Str].Err(error: "e");
    show(io, b(result_is_ok(ok))); show(io, b(result_is_err(bad))); show(io, result_unwrap_or(bad, P(x: 9, name: "d")).name);
    show(io, match result_ok(ok) { Option.Some(value: v) => i(v.x), Option.None() => "none" });
    show(io, match result_err(bad) { Option.Some(value: e) => e, Option.None() => "none" });
    show(io, match result_err(ok) { Option.Some(value: e) => e, Option.None() => "none" });
    let xs = ints_new(); ints_push(xs, 4); ints_push(xs, 0 - 2); ints_push(xs, 9); ints_push(xs, 4);
    show(io, i(ints_sum(xs))); show(io, i(option_unwrap_or(ints_max(xs), 0))); show(io, i(option_unwrap_or(ints_min(xs), 0)));
    show(io, i(option_unwrap_or(ints_index_of(xs, 4), 0 - 1))); show(io, i(option_unwrap_or(ints_index_of(xs, 5), 0 - 1))); show(io, b(ints_contains(xs, 9)));
    let empty = ints_new(); show(io, b(option_is_none(ints_max(empty)))); show(io, b(option_is_none(ints_min(empty)))); show(io, i(ints_sum(empty)));
    let ss = strs_new(); strs_push(ss, "a"); strs_push(ss, "b"); show(io, i(option_unwrap_or(strs_index_of(ss, "b"), 0 - 1))); show(io, b(strs_contains(ss, "c")));
    let ps = vec_new[P](); vec_push(ps, P(x: 1, name: "a")); vec_push(ps, P(x: 2, name: "b"));
    show(io, i(option_unwrap_or(vec_index_of(ps, P(x: 2, name: "b")), 0 - 1))); show(io, i(option_unwrap_or(vec_index_of(ps, P(x: 2, name: "c")), 0 - 1)));
    0
}
"#;
    agrees(
        &[("prog.nz", prog)],
        "T|T|fallback|v|T|T|d|3|e|none|15|9|-2|0|-1|T|T|T|0|1|F|1|-1|0\n",
    );
}

#[test]
fn io_and_process_need_the_authority_they_show() {
    // Reading a file that is there and one that is not; a newline after each line; the
    // arguments without the program's name.
    let prog = r#"use "@std/io";
use "@std/process";
use "@std/result";
fn main(io: IoCap) -> Int {
    io_println(io, result_unwrap_or(io_read(io, "data.txt"), "missing"));
    match io_read(io, "absent.txt") { Result.Ok(value: v) => io_println(io, v), Result.Err(error: e) => io_eprintln(io, e) };
    let args = process_args(io);
    io_println(io, str_join(args, "+"));
    strs_len(args)
}
"#;
    for (label, r) in four(
        &[("prog.nz", prog), ("data.txt", "contents")],
        &["x", "y z"],
    ) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, "contents\nx+y z\n2\n", "{label}");
        assert!(
            r.stderr.contains("no such file: absent.txt\n"),
            "{label}: {}",
            r.stderr
        );
    }
    // A function that calls one must declare `io`, and hold the capability to hand over.
    let undeclared = "use \"@std/io\";\nfn f(io: IoCap) -> Int ! {} { io_println(io, \"x\") }\nfn main(io: IoCap) -> Int { f(io) }\n";
    assert!(codes(&[("prog.nz", undeclared)]).contains(&"N0366".to_owned()));
    let unheld = "use \"@std/process\";\nfn main() -> Int { strs_len(process_args(0)) }\n";
    assert!(!codes(&[("prog.nz", unheld)]).is_empty());
}

#[test]
fn what_a_standard_function_reads_keeps_its_origin() {
    // A path read from a file, through `io_read`, is still a file's contents.
    let prog = "use \"@std/io\";\nuse \"@std/result\";\n\
                fn save(io: IoCap) -> Int ! { io } { let p = result_unwrap_or(io_read(io, \"cfg\"), \"out\"); write_file(p, \"x\") }\n\
                fn main(io: IoCap) -> Int { save(io) }\n";
    assert_eq!(codes(&[("prog.nz", prog)]), ["N0372"]);
    // A pure helper passes its arguments' origins through, like any function.
    let through = "use \"@std/text\";\n\
                   fn save(io: IoCap) -> Int ! { io } { let p = text_trim(read_file(\"cfg\")); write_file(p, \"x\") }\n\
                   fn main(io: IoCap) -> Int { save(io) }\n";
    assert_eq!(codes(&[("prog.nz", through)]), ["N0372"]);
    let computed = "use \"@std/text\";\n\
                    fn save(io: IoCap) -> Int ! { io } { let p = text_trim(\"  out.txt \"); write_file(p, \"x\") }\n\
                    fn main(io: IoCap) -> Int { save(io) }\n";
    assert!(codes(&[("prog.nz", computed)]).is_empty());
}

#[test]
fn chan_take_receives_one_value_at_a_time_across_tasks() {
    let prog = "use \"@std/chan\";\nuse \"@std/option\";\n\
                fn produce(c: Chan, n: Int) -> Int { let mut i = 1; while i <= n { chan_send(c, i); i = i + 1; } chan_close(c); 0 }\n\
                fn main(t: SpawnCap) -> Int { let c = chan_new(2); let mut total = 0; let mut more = true;\n\
                  scope { spawn produce(c, 100); while more { match chan_take(c) { Option.Some(value: v) => { total = total + v; }, Option.None() => { more = false; } } } }\n\
                  total }\n";
    agrees(&[("prog.nz", prog)], "5050\n");
}

#[test]
fn the_spec_s_example_runs_everywhere() {
    let spec = std::fs::read_to_string(repo().join("docs/spec.md")).unwrap();
    let at = spec
        .find("<!-- id: std -->")
        .expect("the standard library section");
    let rest = &spec[at..];
    let open = rest.find("```nazm\n").unwrap() + "```nazm\n".len();
    let block = &rest[open..open + rest[open..].find("```").unwrap()];
    agrees(&[("prog.nz", block)], "4\n42\n");
}

#[test]
fn every_public_standard_function_is_documented_and_listed() {
    let spec = std::fs::read_to_string(repo().join("docs/spec.md")).unwrap();
    let at = spec.find("<!-- id: std -->").unwrap();
    let section = &spec[at..at + spec[at..].find("\n### ").unwrap()];
    let mut seen = 0;
    for m in MODULES {
        let text = std::fs::read_to_string(repo().join(format!("library/std/{m}.nz"))).unwrap();
        assert!(
            section.contains(&format!("`@std/{m}`")),
            "@std/{m} is not in the spec's table"
        );
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let Some(rest) = l.strip_prefix("pub fn ") else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            assert!(
                i > 0 && lines[i - 1].starts_with("///"),
                "`{name}` in @std/{m} has no doc comment"
            );
            assert!(
                section.contains(&format!("`{name}")),
                "`{name}` is not in the spec's table"
            );
            assert!(l.contains(" ! {"), "`{name}` declares no effect set");
            seen += 1;
        }
    }
    assert!(seen >= 30, "{seen}");
}

/// Measurements for `docs/performance.md`. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by the benchmark gate"]
fn what_the_standard_library_costs() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("split", "use \"@std/text\";\nfn main() -> Int { let s = text_repeat(\"word,\", 20000); let mut n = 0; let mut r = 0; while r < 50 { n = n + strs_len(text_split(s, \",\")); r = r + 1; } n }\n"),
        ("find", "use \"@std/text\";\nuse \"@std/option\";\nfn main() -> Int { let s = str_concat(text_repeat(\"a\", 100000), \"b\"); let mut n = 0; let mut r = 0; while r < 20 { n = n + option_unwrap_or(text_find(s, \"ab\"), 0); r = r + 1; } n }\n"),
        ("parse", "use \"@std/text\";\nuse \"@std/result\";\nfn main() -> Int { let mut n = 0; let mut i = 0; while i < 1000000 { n = n + result_unwrap_or(text_parse_int(int_to_str(i)), 0) % 7; i = i + 1; } n }\n"),
        ("seq", "use \"@std/seq\";\nuse \"@std/option\";\nfn main() -> Int { let xs = ints_new(); let mut i = 0; while i < 1000000 { ints_push(xs, i % 1000); i = i + 1; } let mut r = 0; let mut n = 0; while r < 20 { n = n + ints_sum(xs) % 7 + option_unwrap_or(ints_max(xs), 0); r = r + 1; } n }\n"),
    ];
    for (name, src) in cases {
        let path = dir.path().join(format!("{name}.nz"));
        std::fs::write(&path, src).unwrap();
        for backend in ["llvm", "cranelift"] {
            let exe = dir.path().join(format!("{name}-{backend}"));
            let b = Command::new(NAZM)
                .args([
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "--opt-level",
                    "2",
                ])
                .arg(&path)
                .arg("-o")
                .arg(&exe)
                .output()
                .unwrap();
            assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
            let mut best = f64::MAX;
            for _ in 0..5 {
                let s = std::time::Instant::now();
                assert!(Command::new(&exe).output().unwrap().status.success());
                best = best.min(s.elapsed().as_secs_f64() * 1000.0);
            }
            println!("{name} {backend}: {best:.1} ms");
        }
    }
}
