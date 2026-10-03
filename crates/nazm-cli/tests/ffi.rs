//! N42: foreign functions — the C boundary.
//!
//! What only a whole build shows: that a Nazm program calls C functions compiled by clang, with
//! `Int` and `Bool` crossing by the C convention, under both backends, with identical results and
//! a balanced memory report; that every declaration outside the v1 subset is refused by its own
//! code before anything is built; that a foreign call needs a `ForeignCap` wherever it is made;
//! that `nazm run` refuses such a program before running any of it; that the linker is handed
//! exactly what `--link` names, and a missing file or a missing definition is reported as the
//! user's rather than as a compiler bug; and that the C symbol enters the object's key.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The C side of every test: `int64_t` and `bool`, by value, in registers and on the stack.
const C: &str = r#"
#include <stdalign.h>
#include <stdbool.h>
#include <stdint.h>
/* The layout the boundary table promises, checked by the C compiler on this target. */
_Static_assert(sizeof(int64_t) == 8 && alignof(int64_t) == 8, "Int is int64_t");
_Static_assert(sizeof(bool) == 1 && alignof(bool) == 1, "Bool is a one-byte bool");
int64_t c_add(int64_t a, int64_t b) { return a + b; }
int64_t c_neg(int64_t a) { return -a; }
bool c_is_even(int64_t n) { return (n % 2) == 0; }
bool c_not(bool b) { return !b; }
int64_t c_bool_to_int(bool b) { return b ? 1 : 0; }
int64_t c_sum9(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f,
               int64_t g, int64_t h, int64_t i) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i;
}
bool c_mixed(int64_t a, bool b, int64_t c, bool d) { return (a + c > 0) == (b && !d); }
int64_t c_one(void) { return 1; }
int64_t c_two(void) { return 2; }
static int64_t counter = 0;
int64_t c_tick(void) { return ++counter; }
"#;

struct Dir {
    dir: tempfile::TempDir,
}

impl Dir {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// The C fixture, compiled by clang to an object.
    fn object(&self) -> PathBuf {
        let c = self.path("fixture.c");
        std::fs::write(&c, C).unwrap();
        let o = self.path("fixture.o");
        let r = Command::new("clang")
            .args(["-c", "-O1"])
            .arg(&c)
            .arg("-o")
            .arg(&o)
            .output()
            .expect("clang");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        o
    }

    fn nazm(&self, args: &[&str]) -> Output {
        Command::new(NAZM)
            .current_dir(self.dir.path())
            .args(args)
            .output()
            .expect("nazm")
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Every `nazm.diagnostic/1` on stderr, by code.
fn codes(o: &Output) -> Vec<String> {
    stderr(o)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_exe(exe: &Path) -> Ran {
    let o = common::bounded_exe(exe)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .output()
        .expect("run");
    Ran {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// Build `prog.nz` against the fixture with `backend` and run it.
fn built(d: &Dir, backend: &str, extra: &[&str]) -> Ran {
    let obj = d.object();
    let exe = d.path(&format!("prog-{backend}"));
    let mut args = vec!["build", "--backend", backend, "--link"];
    let obj = obj.to_string_lossy().into_owned();
    args.push(&obj);
    args.extend_from_slice(extra);
    let exe_s = exe.to_string_lossy().into_owned();
    args.extend_from_slice(&["prog.nz", "-o", &exe_s]);
    let b = d.nazm(&args);
    assert!(b.status.success(), "{backend}: {}", stderr(&b));
    run_exe(&exe)
}

/// Both backends, with the memory report: they must agree, exactly.
fn both(src: &str) -> Ran {
    let d = Dir::new(&[("prog.nz", src)]);
    let l = built(&d, "llvm", &["--no-cache"]);
    let c = built(&d, "cranelift", &["--no-cache"]);
    assert_eq!(c, l, "Cranelift and LLVM disagree");
    l
}

const DECLS: &str = r#"
extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add";
extern "C" fn c_neg(a: Int) -> Int = "c_neg";
extern "C" fn c_is_even(n: Int) -> Bool = "c_is_even";
extern "C" fn c_not(b: Bool) -> Bool = "c_not";
extern "C" fn c_bool_to_int(b: Bool) -> Int = "c_bool_to_int";
extern "C" fn c_sum9(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int = "c_sum9";
extern "C" fn c_mixed(a: Int, b: Bool, c: Int, d: Bool) -> Bool = "c_mixed";
extern "C" fn c_tick() -> Int = "c_tick";
"#;

fn with_decls(body: &str) -> String {
    format!("{DECLS}{body}")
}

fn say(n: &str) -> String {
    format!("print(int_to_str({n})); print(\" \");")
}

#[test]
fn a_program_calls_c_with_ints_and_bools_and_both_backends_agree() {
    let body = format!(
        "fn b(x: Bool) -> Int {{ if x {{ 1 }} else {{ 0 }} }}\n\
         fn main(io: IoCap, f: ForeignCap) -> Int\n{{ {} {} {} {} {} {} {} {} {} {} {} 0 }}\n",
        say("c_add(40, 2)"),
        say("c_add(9223372036854775807, 0)"),
        say("c_neg(-9223372036854775807)"),
        say("b(c_is_even(10)) + 2 * b(c_is_even(7))"),
        say("b(c_not(true)) + 2 * b(c_not(false))"),
        say("c_bool_to_int(true) + 2 * c_bool_to_int(false)"),
        say("c_sum9(1, 2, 3, 4, 5, 6, 7, 8, 9)"),
        say("b(c_mixed(1, true, 1, false)) + 2 * b(c_mixed(-5, true, 1, true))"),
        say("c_add(c_add(1, 2), c_neg(c_add(3, 4)))"),
        say("c_tick() + c_tick() * 10"),
        say("if c_is_even(c_add(1, 1)) { 7 } else { 8 }"),
    );
    let r = both(&with_decls(&body));
    assert_eq!(r.code, Some(0), "{r:?}");
    // `main`'s result is printed last.
    let lines: Vec<&str> = r.stdout.split_whitespace().collect();
    assert_eq!(
        &lines[..],
        &[
            "42",
            "9223372036854775807",
            "9223372036854775807",
            "1",
            "2",
            "1",
            "285",
            "3",
            "-4",
            "21",
            "7",
            "0"
        ],
        "{}",
        r.stdout
    );
}

#[test]
fn a_foreign_call_inside_managed_code_leaks_nothing() {
    // Strings and sequences live across foreign calls, and a failure after one unwinds.
    let body = "fn main(io: IoCap, f: ForeignCap) -> Int\n{ let s = int_to_str(c_add(1, 2)); \
                let v = ints_new(); ints_push(v, c_neg(5)); ints_push(v, c_add(ints_get(v, 0), 10)); \
                print(s); print(int_to_str(ints_get(v, 1))); \
                let t = str_concat(s, int_to_str(c_tick())); print(t); \
                ints_get(v, c_add(5, 0)) }\n";
    let r = both(&with_decls(body));
    assert_eq!(r.stdout, "3531", "{r:?}");
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0405"), "{}", r.stderr);
    let report = r
        .stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap();
    // Sequences, strings, channels and — since N50 — closure objects.
    assert_eq!(
        report.matches("live=0").count(),
        4,
        "everything reclaimed: {report}"
    );
}

#[test]
fn a_foreign_function_declared_in_one_module_is_called_from_another() {
    let d = Dir::new(&[
        (
            "lib.nz",
            "pub extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
             pub fn twice(f: ForeignCap, n: Int) -> Int ! { foreign }\n{ c_add(n, n) }\n",
        ),
        (
            "prog.nz",
            "use \"lib.nz\";\nextern \"C\" fn add(a: Int, b: Int) -> Int = \"c_add\";\n\
             fn main(io: IoCap, f: ForeignCap) -> Int\n{ print(int_to_str(twice(f, 21) + add(0, 1))); 0 }\n",
        ),
    ]);
    for backend in ["llvm", "cranelift"] {
        let r = built(&d, backend, &[]);
        assert_eq!(r.stdout, "430\n", "{backend}: {r:?}");
    }
    // The cache holds the interface with its foreign export: a second build reuses it and
    // links the same program.
    let r = built(&d, "cranelift", &[]);
    assert_eq!(r.stdout, "430\n");
}

fn check_codes(src: &str) -> Vec<String> {
    let d = Dir::new(&[("prog.nz", src)]);
    let o = d.nazm(&["check", "--json", "prog.nz"]);
    assert!(!o.status.success(), "check accepted:\n{src}");
    codes(&o)
}

#[test]
fn every_declaration_outside_the_subset_is_refused_by_its_own_code() {
    let main = "fn main(f: ForeignCap) -> Int { 0 }\n";
    let cases: &[(&str, &str)] = &[
        ("extern \"Rust\" fn a(x: Int) -> Int = \"a\";", "N0380"),
        ("extern \"c\" fn a(x: Int) -> Int = \"a\";", "N0380"),
        ("extern \"C\" fn a(x: Ints) -> Int = \"a\";", "N0381"),
        // A `Str` result crosses since N85, copied; a sequence result never does.
        ("extern \"C\" fn a(x: Int) -> Strs = \"a\";", "N0381"),
        ("extern \"C\" fn a(x: Ints) -> Int = \"a\";", "N0381"),
        ("extern \"C\" fn a(x: ForeignCap) -> Int = \"a\";", "N0381"),
        (
            "struct P { x: Int }\nextern \"C\" fn a(p: P) -> Int = \"a\";",
            "N0381",
        ),
        ("extern \"C\" fn a(x: Int) -> Int = \"1a\";", "N0382"),
        ("extern \"C\" fn a(x: Int) -> Int = \"a.b\";", "N0382"),
        ("extern \"C\" fn a(x: Int) -> Int = \"\";", "N0382"),
        ("extern \"C\" fn a(x: Int) -> Int = \"malloc\";", "N0383"),
        ("extern \"C\" fn a(x: Int) -> Int = \"main\";", "N0383"),
        (
            "extern \"C\" fn a(x: Int) -> Int = \"f\";\nextern \"C\" fn b(x: Bool) -> Int = \"f\";",
            "N0383",
        ),
        ("extern \"C\" fn a[T](x: Int) -> Int = \"a\";", "N0384"),
        (
            "extern \"C\" fn a(x: Int) -> Int ! { io } = \"a\";",
            "N0384",
        ),
    ];
    for (decl, want) in cases {
        let got = check_codes(&format!("{decl}\n{main}"));
        assert!(
            got.iter().any(|c| c == want),
            "`{decl}`: wanted {want}, got {got:?}"
        );
    }
    // The same symbol declared twice the same way is one C function, and fine.
    let d = Dir::new(&[(
        "prog.nz",
        "extern \"C\" fn a(x: Int) -> Int = \"f\";\nextern \"C\" fn b(y: Int) -> Int = \"f\";\nfn main() -> Int { 0 }\n",
    )]);
    let o = d.nazm(&["check", "prog.nz"]);
    assert!(o.status.success(), "{}", stderr(&o));
}

#[test]
fn two_modules_that_disagree_about_a_symbol_are_refused_before_linking() {
    let d = Dir::new(&[
        (
            "lib.nz",
            "extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
             pub fn one(f: ForeignCap) -> Int ! { foreign }\n{ c_add(1, 0) }\n",
        ),
        (
            "prog.nz",
            "use \"lib.nz\";\nextern \"C\" fn wrong(a: Int) -> Bool = \"c_add\";\n\
             fn main(f: ForeignCap) -> Int\n{ if wrong(1) { one(f) } else { 0 } }\n",
        ),
    ]);
    let obj = d.object();
    let o = d.nazm(&[
        "build",
        "--json",
        "--link",
        &obj.to_string_lossy(),
        "prog.nz",
        "-o",
        "x",
    ]);
    assert!(!o.status.success());
    assert_eq!(codes(&o), ["N0383"], "{}", stderr(&o));
    assert!(!d.path("x").exists());
}

#[test]
fn a_foreign_call_needs_a_foreign_cap_everywhere() {
    // Without an effect set, too: N37's bridge does not reach C.
    let plain = format!(
        "{DECLS}fn g(n: Int) -> Int\n{{ c_add(n, 1) }}\nfn main(f: ForeignCap) -> Int {{ g(1) }}\n"
    );
    assert!(check_codes(&plain).contains(&"N0369".to_owned()));
    // With a declared set that leaves `foreign` out, the effect is missing too.
    let typed = format!(
        "{DECLS}fn g(f: ForeignCap, n: Int) -> Int ! {{}}\n{{ c_add(n, 1) }}\nfn main(f: ForeignCap) -> Int {{ g(f, 1) }}\n"
    );
    assert!(check_codes(&typed).contains(&"N0366".to_owned()));
    // Held and declared: accepted.
    let ok = format!(
        "{DECLS}fn g(f: ForeignCap, n: Int) -> Int ! {{ foreign }}\n{{ c_add(n, 1) }}\nfn main(f: ForeignCap) -> Int {{ g(f, 1) }}\n"
    );
    let d = Dir::new(&[("prog.nz", &ok)]);
    let o = d.nazm(&["check", "prog.nz"]);
    assert!(o.status.success(), "{}", stderr(&o));
}

#[test]
fn an_imported_foreign_function_needs_a_foreign_cap_too() {
    // The importer sees only `lib.nz`'s interface, and it must say the export is C's.
    let lib = "pub extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n";
    let d = Dir::new(&[
        ("lib.nz", lib),
        (
            "prog.nz",
            "use \"lib.nz\";\nfn g(n: Int) -> Int\n{ c_add(n, 1) }\nfn main(f: ForeignCap) -> Int { g(1) }\n",
        ),
    ]);
    for cache in [&["--no-cache"][..], &[][..], &[][..]] {
        let mut args = vec!["check", "--json"];
        args.extend_from_slice(cache);
        args.push("prog.nz");
        let o = d.nazm(&args);
        assert!(!o.status.success(), "{cache:?}");
        assert!(
            codes(&o).contains(&"N0369".to_owned()),
            "{cache:?}: {}",
            stderr(&o)
        );
    }
    // An edit to the importer re-checks it against `lib.nz`'s interface as the store holds
    // it, read back from disk: the persisted export must still say it is C's.
    std::fs::write(
        d.path("prog.nz"),
        "use \"lib.nz\";\nfn g(n: Int) -> Int\n{ c_add(n, 2) }\nfn main(f: ForeignCap) -> Int { g(1) }\n",
    )
    .unwrap();
    let o = d.nazm(&["check", "--json", "prog.nz"]);
    assert!(codes(&o).contains(&"N0369".to_owned()), "{}", stderr(&o));
    // The published interface names the symbol, so what `lib.nz` offers is legible without
    // its source.
    let o = d.nazm(&["interface", "lib.nz"]);
    let published = String::from_utf8_lossy(&o.stdout);
    assert!(
        published.contains(r#""effects":["foreign"],"foreign":"c_add""#),
        "{published}"
    );
    let d = Dir::new(&[
        ("lib.nz", lib),
        (
            "prog.nz",
            "use \"lib.nz\";\nfn main(io: IoCap, f: ForeignCap) -> Int\n{ print(int_to_str(c_add(20, 22))); 0 }\n",
        ),
    ]);
    for backend in ["llvm", "cranelift"] {
        assert_eq!(built(&d, backend, &[]).stdout, "420\n", "{backend}");
    }
}

#[test]
fn what_c_returns_carries_the_unknown_origin() {
    // A path computed from a C result is not one a restricted function may write to: C's
    // result may be anything, and may be derived from what it was given.
    let src = |effects: &str| {
        format!(
            "extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
             fn save(io: IoCap, f: ForeignCap) -> Int{effects}\n{{ write_file(int_to_str(c_add(1, 2)), \"x\") }}\n\
             fn main(io: IoCap, f: ForeignCap) -> Int {{ save(io, f) }}\n"
        )
    };
    let d = Dir::new(&[("prog.nz", &src(" ! { io, foreign }"))]);
    let o = d.nazm(&["check", "--json", "prog.nz"]);
    assert_eq!(codes(&o), ["N0372"], "{}", stderr(&o));
    assert!(
        stderr(&o).contains("is a foreign function"),
        "{}",
        stderr(&o)
    );
    // A computed path that does not come from C is fine; a function with no effect set is
    // not restricted (N38).
    let d = Dir::new(&[("prog.nz", &src(""))]);
    assert!(d.nazm(&["check", "prog.nz"]).status.success());
    let ok = "extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
              fn save(io: IoCap, f: ForeignCap) -> Int ! { io, foreign }\n{ c_add(1, 2); write_file(\"out\", \"x\") }\n\
              fn main(io: IoCap, f: ForeignCap) -> Int { save(io, f) }\n";
    let d = Dir::new(&[("prog.nz", ok)]);
    let o = d.nazm(&["check", "prog.nz"]);
    assert!(o.status.success(), "{}", stderr(&o));
}

#[test]
fn a_task_calls_c_on_its_own_thread() {
    // C is called on the OS thread the task runs on; the capability is handed to it like any
    // other value, and nothing is shared but the channel.
    let src = "extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
               fn worker(f: ForeignCap, c: Chan, n: Int) -> Int\n{ chan_send(c, c_add(n, 1)); 0 }\n\
               fn main(io: IoCap, t: SpawnCap, f: ForeignCap) -> Int\n{ let c = chan_new(4); let buf = ints_new(); \
               scope { spawn worker(f, c, 10); spawn worker(f, c, 20); } chan_recv(c, buf); chan_recv(c, buf); \
               print(int_to_str(ints_get(buf, 0) + ints_get(buf, 1))); 0 }\n";
    let r = both(src);
    assert_eq!(r.stdout, "320\n", "{r:?}");
}

#[test]
fn nazm_run_refuses_a_foreign_call_before_running_anything() {
    let src = with_decls(
        "fn main(io: IoCap, f: ForeignCap) -> Int\n{ print(\"started\"); print(int_to_str(c_add(1, 2))); 0 }\n",
    );
    let d = Dir::new(&[("prog.nz", &src)]);
    let o = d.nazm(&["run", "--json", "prog.nz"]);
    assert!(!o.status.success());
    assert_eq!(codes(&o), ["N0385"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "", "nothing ran");
    // A program that only declares one runs.
    let d = Dir::new(&[("prog.nz", &with_decls("fn main() -> Int { 3 }\n"))]);
    let o = d.nazm(&["run", "prog.nz"]);
    assert!(o.status.success(), "{}", stderr(&o));
}

#[test]
fn the_linker_is_handed_exactly_what_link_names() {
    let src = with_decls(
        "fn main(io: IoCap, f: ForeignCap) -> Int\n{ print(int_to_str(c_add(1, 2))); 0 }\n",
    );
    let d = Dir::new(&[("prog.nz", &src)]);
    // A file that is not there is refused before anything is compiled.
    let o = d.nazm(&["build", "--link", "missing.o", "prog.nz", "-o", "x"]);
    assert_eq!(o.status.code(), Some(4));
    assert!(
        stderr(&o).contains("`--link missing.o`: no such file"),
        "refused by nazm, not by the linker: {}",
        stderr(&o)
    );
    assert!(!d.path("x").exists());
    // No `--link` at all: the linker cannot resolve the symbol, and the message says that
    // the user's definition is what is missing — not that the compiler is broken.
    for backend in ["llvm", "cranelift"] {
        let o = d.nazm(&["build", "--backend", backend, "prog.nz", "-o", "x"]);
        assert_eq!(o.status.code(), Some(4), "{backend}");
        let e = stderr(&o);
        assert!(
            e.contains("`c_add`") && e.contains("--link"),
            "{backend}: {e}"
        );
        assert!(!e.contains("bug in"), "{backend}: {e}");
        assert!(!d.path("x").exists());
    }
}

#[test]
fn the_c_symbol_enters_the_object_key() {
    // Two C functions with one signature: a declaration that changes only its symbol must
    // not reuse the object built for the other.
    let src = |sym: &str| {
        format!(
            "extern \"C\" fn k() -> Int = \"{sym}\";\nfn main(io: IoCap, f: ForeignCap) -> Int\n{{ k() }}\n"
        )
    };
    let d = Dir::new(&[("prog.nz", &src("c_one"))]);
    for backend in ["cranelift", "llvm"] {
        std::fs::write(d.path("prog.nz"), src("c_one")).unwrap();
        assert_eq!(built(&d, backend, &[]).stdout, "1\n", "{backend}");
        std::fs::write(d.path("prog.nz"), src("c_two")).unwrap();
        assert_eq!(built(&d, backend, &[]).stdout, "2\n", "{backend}");
    }
}

#[test]
fn mir_names_the_foreign_symbol_and_has_no_body_for_it() {
    let d = Dir::new(&[(
        "prog.nz",
        &with_decls("fn main(f: ForeignCap) -> Int { c_add(1, 2) }\n"),
    )]);
    let o = d.nazm(&["mir", "prog.nz"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("\"foreign\": \"c_add\""), "{text}");
}

#[test]
fn a_reserved_c_symbol_is_every_one_the_runtime_calls() {
    let all = nazm_lir::Needs {
        tasks: true,
        channels: true,
        sequences: true,
        io: true,
        join: true,
        strings: true,
        shims: true,
        stack: true,
        closures: true,
        foreign: true,
    };
    let text = nazm_lir::emit_runtime(all);
    let mut found = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if !(t.starts_with("declare ") || t.starts_with("define ")) {
            continue;
        }
        let Some(at) = t.find('@') else { continue };
        let name: String = t[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if !name.contains('.') {
            found.push(name);
        }
    }
    assert!(found.iter().any(|n| n == "malloc"), "{found:?}");
    for n in &found {
        assert!(
            nazm_core::check::RESERVED_C_SYMBOLS.contains(&n.as_str()),
            "the runtime declares `{n}`, which a foreign declaration could redeclare"
        );
    }
    for i in nazm_lir::runtime::IMPORTS {
        if i.part == nazm_lir::runtime::Part::Libc {
            assert!(
                nazm_core::check::RESERVED_C_SYMBOLS.contains(&i.symbol),
                "`{}`",
                i.symbol
            );
        }
    }
}
