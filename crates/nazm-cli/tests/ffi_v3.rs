//! N85: FFI v3 — handles, C-layout records, C strings returned, `errno`, callbacks and shared
//! libraries.
//!
//! `docs/architecture.md` §7.86 and the spec section *Foreign functions v3*. What only a whole build
//! shows: that an opaque handle, a C struct laid out in declaration order, a string C keeps and
//! Nazm copies, `errno` and an export passed as a C function pointer each cross under both
//! backends with identical results and a balanced memory report; that a null where a declaration
//! promised a value fails the call with `N0409`; that each refusal has its code; that a handle and
//! a C struct keep what they are across a module boundary and in the check cache; and that `nazm
//! build --lib --shared` writes a shared library a C program links, whose only symbols are the
//! exports.

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

/// The C side: a counter behind an opaque pointer, structs read through `const` pointers, a
/// string C keeps, `errno`, and callbacks called inside the call and from a thread C starts.
const C: &str = r#"
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <errno.h>
#include <pthread.h>
#ifdef __APPLE__
#include <malloc/malloc.h>
#endif
typedef struct counter { int64_t n; } counter;
counter *counter_new(int64_t start) { counter *c = malloc(sizeof *c); c->n = start; return c; }
counter *counter_maybe(int64_t start) { return start < 0 ? NULL : counter_new(start); }
int64_t counter_bump(counter *c, int64_t by) { c->n += by; return c->n; }
int64_t counter_free(counter *c) { free(c); return 0; }
const char *greeting(int64_t which) { return which ? "hello, C" : NULL; }
typedef struct point { int64_t x; bool flag; int64_t y; } point;
int64_t point_sum(const point *p) { return p->x * 1000 + p->y + (p->flag ? 100 : 0); }
typedef struct seg { point a; counter *owner; point b; int64_t tag; } seg;
int64_t seg_score(const seg *s) {
    return s->a.x * 1000000 + s->b.y * 100 + s->tag + (s->owner ? s->owner->n * 10 : 0);
}
int64_t point_offsets(const point *p) {
    return (int64_t)((const char *)&p->flag - (const char *)p) * 100
         + (int64_t)((const char *)&p->y - (const char *)p);
}
int64_t fail_with(int64_t e) { errno = (int)e; return -1; }
typedef int64_t (*cb)(int64_t);
int64_t apply_twice(cb f, int64_t x) { return f(f(x)); }
struct job { cb f; int64_t x; int64_t r; };
static void *runner(void *a) { struct job *j = a; j->r = j->f(j->x); return 0; }
int64_t apply_on_thread(cb f, int64_t x) {
    pthread_t t; struct job j = { f, x, 0 };
    pthread_create(&t, 0, runner, &j); pthread_join(t, 0); return j.r;
}
int64_t blocks_in_use(void) {
#ifdef __APPLE__
    malloc_statistics_t s; malloc_zone_statistics(NULL, &s); return (int64_t)s.blocks_in_use;
#else
    return 0;
#endif
}
"#;

const DECLS: &str = "extern \"C\" struct Counter;\n\
                     extern \"C\" struct Point { x: Int, flag: Bool, y: Int }\n\
                     extern \"C\" struct Seg { a: Point, owner: Counter, b: Point, tag: Int }\n\
                     extern \"C\" fn counter_new(start: Int) -> Counter = \"counter_new\";\n\
                     extern \"C\" fn counter_maybe(start: Int) -> Option[Counter] = \"counter_maybe\";\n\
                     extern \"C\" fn counter_bump(c: Counter, by: Int) -> Int = \"counter_bump\";\n\
                     extern \"C\" fn counter_free(c: Counter) -> Int = \"counter_free\";\n\
                     extern \"C\" fn greeting(which: Int) -> Option[Str] = \"greeting\";\n\
                     extern \"C\" fn point_sum(p: Point) -> Int = \"point_sum\";\n\
                     extern \"C\" fn point_offsets(p: Point) -> Int = \"point_offsets\";\n\
                     extern \"C\" fn seg_score(s: Seg) -> Int = \"seg_score\";\n\
                     extern \"C\" fn fail_with(e: Int) -> Int = \"fail_with\";\n\
                     extern \"C\" fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int = \"apply_twice\";\n\
                     extern \"C\" fn apply_on_thread(f: fn(Int) -> Int, x: Int) -> Int = \"apply_on_thread\";\n\
                     extern \"C\" fn blocks_in_use() -> Int = \"blocks_in_use\";\n\
                     pub extern \"C\" fn triple(x: Int) -> Int ! {} = \"nz_triple\" { x * 3 }\n";

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

    fn clang(&self, args: &[&str]) {
        let r = Command::new("clang")
            .current_dir(self.dir.path())
            .args(args)
            .output()
            .expect("clang");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
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

fn codes(o: &Output) -> Vec<String> {
    let mut c: Vec<String> = stderr(o)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect();
    c.sort();
    c.dedup();
    c
}

fn check_codes(src: &str) -> Vec<String> {
    let d = Dir::new(&[("prog.nz", src)]);
    codes(&d.nazm(&["check", "--json", "prog.nz"]))
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(exe: &Path) -> Ran {
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

/// `prog.nz` built against the C fixture by both backends and run: they must agree exactly.
fn both(src: &str) -> Ran {
    let d = Dir::new(&[("prog.nz", src), ("fixture.c", C)]);
    d.clang(&["-c", "-O1", "fixture.c", "-o", "fixture.o"]);
    let mut out = Vec::new();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path(&format!("prog-{backend}"));
        let b = d.nazm(&[
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--link",
            "fixture.o",
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ]);
        assert!(b.status.success(), "{backend}: {}", stderr(&b));
        out.push(run(&exe));
    }
    assert_eq!(out[0], out[1], "LLVM and Cranelift disagree");
    out.remove(0)
}

fn balanced(r: &Ran) {
    let line = r
        .stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"));
    assert!(
        line.contains("strings") && line.contains("live=0 channels"),
        "{line}"
    );
    assert!(
        !line.contains("live=1") && !line.contains("live=2"),
        "{line}"
    );
}

// --- handles, structs, strings, errno, callbacks ------------------------------------------------

#[test]
fn a_handle_a_c_struct_a_c_string_errno_and_a_callback_cross_under_both_backends() {
    let r = both(&format!(
        "{DECLS}\
         struct Holder {{ c: Counter, label: Str }}\n\
         fn keep(c: Counter) -> Holder ! {{}} {{ Holder(c: c, label: \"kept\") }}\n\
         fn main(io: IoCap, f: ForeignCap) -> Int ! {{ io, foreign }} {{\n    \
         let c = counter_new(5);\n    \
         print(int_to_str(counter_bump(c, 2))); print(\",\");\n    \
         let h = keep(c);\n    \
         print(int_to_str(counter_bump(h.c, 10))); print(\",\"); print(h.label); print(\"|\");\n    \
         let v = vec_new[Counter](); vec_push(v, c);\n    \
         print(int_to_str(counter_bump(vec_get(v, 0), 1))); print(\"|\");\n    \
         let none = match counter_maybe(-1) {{ Option.None() => \"none\", Option.Some(value: x) => \"some\" }};\n    \
         print(none); print(\"|\");\n    \
         let some = match counter_maybe(9) {{ Option.None() => 0, Option.Some(value: x) => counter_bump(x, 1) }};\n    \
         print(int_to_str(some)); print(\"|\");\n    \
         match greeting(1) {{ Option.None() => print(\"null\"), Option.Some(value: s) => print(str_concat(s, \"!\")) }};\n    \
         print(\"|\");\n    \
         match greeting(0) {{ Option.None() => print(\"null\"), Option.Some(value: s) => print(s) }};\n    \
         print(\"|\");\n    \
         print(int_to_str(point_sum(Point(x: 7, flag: true, y: 3)))); print(\",\");\n    \
         print(int_to_str(point_offsets(Point(y: 4, x: 2, flag: false)))); print(\"|\");\n    \
         let p = Point(y: 4, x: 2, flag: false);\n    \
         print(int_to_str(seg_score(Seg(a: p, owner: c, b: Point(x: 0, flag: true, y: 5), tag: 6)))); print(\"|\");\n    \
         let r = fail_with(34);\n    \
         print(int_to_str(r)); print(\",\"); print(int_to_str(c_errno())); print(\"|\");\n    \
         print(int_to_str(apply_twice(triple, 2))); print(\",\");\n    \
         print(int_to_str(apply_on_thread(triple, 5))); print(\"|\");\n    \
         print(int_to_str(counter_free(c)));\n    \
         0\n}}\n"
    ));
    assert_eq!(r.code, Some(0), "{r:?}");
    // The counter is one C object through a binding, a record field and a `Vec`; a null is
    // `None`; a C string is copied and owned; `Point`'s fields are where C's `struct point` has
    // them — `flag` at 8 and `y` at 16 — whatever order the construction names them in, and
    // nested inside `Seg` beside a handle; `errno` is what `fail_with` left; an export is
    // called by C inside the call and from C's own thread. `main`'s result is printed last.
    assert_eq!(
        r.stdout,
        "7,17,kept|18|none|10|hello, C!|null|7103,816|2000686|-1,34|18,15|00\n"
    );
    balanced(&r);
}

#[test]
fn a_null_where_the_declaration_promised_a_value_fails_the_call_with_n0409() {
    for (decl, call) in [
        (
            "extern \"C\" fn counter_maybe(start: Int) -> Counter = \"counter_maybe\";\n",
            "counter_bump(counter_maybe(-1), 1)",
        ),
        (
            "extern \"C\" fn greeting(which: Int) -> Str = \"greeting\";\n",
            "str_len(greeting(0))",
        ),
    ] {
        let r = both(&format!(
            "extern \"C\" struct Counter;\n\
             extern \"C\" fn counter_bump(c: Counter, by: Int) -> Int = \"counter_bump\";\n{decl}\
             fn main(io: IoCap, f: ForeignCap) -> Int ! {{ io, foreign }} {{\n    \
             print(\"before \");\n    let n = {call};\n    print(\"after\");\n    n\n}}\n"
        ));
        assert_eq!(r.code, Some(2), "{r:?}");
        assert_eq!(r.stdout, "before ");
        assert!(
            r.stderr.contains("N0409") && r.stderr.contains("returned null"),
            "{r:?}"
        );
    }
}

#[test]
fn a_struct_passed_to_c_is_a_copy_freed_after_every_call() {
    // The borrowed copy of a C struct is the caller's malloc, freed when C returns: ten
    // thousand calls leave the heap where they found it (macOS's allocator statistics; on
    // other hosts the fixture reports nothing and only the result is checked).
    let r = both(&format!(
        "{DECLS}fn main(io: IoCap, f: ForeignCap) -> Int ! {{ io, foreign }} {{\n    \
         let before = blocks_in_use();\n    \
         let mut i = 0;\n    let mut t = 0;\n    \
         while i < 10000 {{ t = t + point_sum(Point(x: i, flag: false, y: 1)); i = i + 1; }}\n    \
         let after = blocks_in_use();\n    \
         print(int_to_str(t)); print(\" \");\n    \
         print(if after - before < 16 {{ \"freed\" }} else {{ int_to_str(after - before) }});\n    \
         0\n}}\n"
    ));
    assert_eq!(r.code, Some(0), "{r:?}");
    // `main`'s result follows, as a native program prints it.
    assert_eq!(r.stdout, "49995010000 freed0\n");
}

// --- refusals -------------------------------------------------------------------------------

#[test]
fn every_misuse_of_a_handle_a_c_struct_or_a_callback_is_refused_by_its_code() {
    let h = "extern \"C\" struct Db;\n";
    let main = "fn main() -> Int { 0 }\n";
    let cases: &[(String, &[&str])] = &[
        // A handle is built and read only by C (`N0611`), has no equality (`N0304`) and never
        // crosses into a task, by capture or by channel (`N0321`, `N0390`).
        (format!("{h}fn f() -> Db ! {{}} {{ Db(x: 1) }}\n{main}"), &["N0611"]),
        (format!("{h}fn f(d: Db) -> Int ! {{}} {{ d.x }}\n{main}"), &["N0611"]),
        (format!("{h}fn f(d: Db) -> Bool ! {{}} {{ d == d }}\n{main}"), &["N0304"]),
        (
            format!(
                "{h}fn w(d: Db) -> Int ! {{}} {{ 0 }}\n\
                 fn g(d: Db, s: SpawnCap) -> Int ! {{ spawn }} {{ scope {{ spawn w(d); }} 0 }}\n{main}"
            ),
            &["N0321"],
        ),
        (format!("{h}fn f(c: Chan[Db]) -> Int ! {{}} {{ 0 }}\n{main}"), &["N0390"]),
        // What crosses, by position (`N0381`).
        (
            format!("{h}extern \"C\" fn g(d: Option[Db]) -> Int = \"g\";\n{main}"),
            &["N0381"],
        ),
        (
            format!("extern \"C\" struct S {{ s: Str }}\n{main}"),
            &["N0381"],
        ),
        (
            format!("struct P {{ x: Int }}\nextern \"C\" struct S {{ p: P }}\n{main}"),
            &["N0381"],
        ),
        (
            format!("extern \"C\" struct P {{ x: Int }}\nextern \"C\" fn g(x: Int) -> P = \"g\";\n{main}"),
            &["N0381"],
        ),
        (
            format!("extern \"C\" fn g(f: fn(Str) -> Int) -> Int = \"g\";\n{main}"),
            &["N0381"],
        ),
        // The declaration's own shape.
        (
            format!("extern \"C\" struct P[T] {{ x: Int }}\n{main}"),
            &["N0384"],
        ),
        (
            format!("extern \"Rust\" struct P {{ x: Int }}\n{main}"),
            &["N0380"],
        ),
        // A callback is an export's name (`N0612`): not another function, not a closure.
        (
            "extern \"C\" fn ap(f: fn(Int) -> Int, x: Int) -> Int = \"ap\";\n\
             fn dbl(x: Int) -> Int ! {} { x * 2 }\n\
             fn main(f: ForeignCap) -> Int ! { foreign } { ap(dbl, 1) }\n"
                .to_owned(),
            &["N0612"],
        ),
        (
            "extern \"C\" fn ap(f: fn(Int) -> Int, x: Int) -> Int = \"ap\";\n\
             fn main(f: ForeignCap) -> Int ! { foreign } { ap(fn(x: Int) -> Int { x }, 1) }\n"
                .to_owned(),
            &["N0612"],
        ),
        // `c_errno` is part of calling C: it needs a `ForeignCap`.
        (
            format!("fn h() -> Int ! {{}} {{ c_errno() }}\n{main}"),
            &["N0366", "N0369"],
        ),
    ];
    for (src, want) in cases {
        assert_eq!(check_codes(src), *want, "{src}");
    }
}

#[test]
fn a_program_reading_errno_runs_only_natively_and_only_where_there_is_one() {
    let src = "fn main(f: ForeignCap) -> Int ! { foreign } { c_errno() }\n";
    let d = Dir::new(&[("prog.nz", src)]);
    // `nazm run` cannot call C, and `errno` is C's (`N0385`).
    let o = d.nazm(&["run", "prog.nz"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("N0385"), "{}", stderr(&o));
    // A freestanding target has no C library keeping one (`N0613`); an ordinary foreign call
    // builds there, its `errno` capture doing nothing.
    for target in ["aarch64-unknown-none", "wasm32-unknown-unknown"] {
        let o = d.nazm(&["build", "--target", target, "--objects", "o", "prog.nz"]);
        assert!(!o.status.success(), "{target}");
        assert!(stderr(&o).contains("N0613"), "{target}: {}", stderr(&o));
    }
    let plain = Dir::new(&[(
        "prog.nz",
        "extern \"C\" fn tick(x: Int) -> Int = \"tick\";\n\
         fn main(f: ForeignCap) -> Int ! { foreign } { tick(1) }\n",
    )]);
    for target in ["aarch64-unknown-none", "wasm32-unknown-unknown"] {
        let o = plain.nazm(&["build", "--target", target, "--objects", "o", "prog.nz"]);
        assert!(o.status.success(), "{target}: {}", stderr(&o));
    }
}

// --- modules and the cache ------------------------------------------------------------------

#[test]
fn a_handle_and_a_c_struct_keep_what_they_are_across_a_module_boundary_and_in_the_cache() {
    let lib = "pub extern \"C\" struct Counter;\n\
               pub extern \"C\" struct Point { x: Int, flag: Bool, y: Int }\n\
               pub extern \"C\" fn counter_new(start: Int) -> Counter = \"counter_new\";\n\
               pub extern \"C\" fn counter_bump(c: Counter, by: Int) -> Int = \"counter_bump\";\n\
               pub extern \"C\" fn point_sum(p: Point) -> Int = \"point_sum\";\n";
    let prog = "use \"lib.nz\";\n\
                fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } {\n    \
                let c = counter_new(1);\n    \
                print(int_to_str(counter_bump(c, 1) + point_sum(Point(y: 2, flag: true, x: 3))));\n    \
                0\n}\n";
    let d = Dir::new(&[
        ("nazm.root", ""),
        ("lib.nz", lib),
        ("prog.nz", prog),
        ("fixture.c", C),
    ]);
    d.clang(&["-c", "-O1", "fixture.c", "-o", "fixture.o"]);
    for backend in ["llvm", "cranelift"] {
        let exe = d.path(&format!("p-{backend}"));
        for _ in 0..2 {
            // The second build reads the module's interface from the cache: the handle and the
            // struct's declaration order come back from it.
            let b = d.nazm(&[
                "build",
                "--backend",
                backend,
                "--link",
                "fixture.o",
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]);
            assert!(b.status.success(), "{backend}: {}", stderr(&b));
            assert_eq!(run(&exe).stdout, "31040\n", "{backend}");
        }
    }
    // An importer cannot build what the interface says is a handle.
    std::fs::write(
        d.path("prog.nz"),
        "use \"lib.nz\";\nfn f() -> Counter ! {} { Counter(x: 1) }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    assert_eq!(codes(&d.nazm(&["check", "--json", "prog.nz"])), ["N0611"]);
}

#[test]
fn a_c_struct_s_field_order_is_its_interface_and_an_ordinary_record_s_is_not() {
    // The interface's canonical bytes, which its fingerprint is the hash of.
    let fingerprint = |src: &str| -> String {
        let d = Dir::new(&[("m.nz", src)]);
        let o = d.nazm(&["interface", "m.nz"]);
        assert!(o.status.success(), "{}", stderr(&o));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let c_ab = fingerprint("pub extern \"C\" struct P { a: Int, b: Bool }\n");
    let c_ba = fingerprint("pub extern \"C\" struct P { b: Bool, a: Int }\n");
    assert_ne!(
        c_ab, c_ba,
        "moving a C struct's fields moves where C reads them"
    );
    let r_ab = fingerprint("pub struct P { a: Int, b: Bool }\n");
    let r_ba = fingerprint("pub struct P { b: Bool, a: Int }\n");
    assert_eq!(r_ab, r_ba, "an ordinary record's order is not semantic");
    assert_ne!(
        c_ab, r_ab,
        "a C struct is not an ordinary record of the same fields"
    );
    let handle = fingerprint("pub extern \"C\" struct P;\n");
    assert_ne!(handle, c_ab);
}

// --- shared libraries -----------------------------------------------------------------------

const LIB: &str = "fn square(x: Int) -> Int ! {} { x * x }\n\n\
                   pub extern \"C\" fn sum_squares(a: Int, b: Int) -> Int ! {} = \"nazm_sum_squares\" {\n    \
                   square(a) + square(b)\n}\n\n\
                   pub extern \"C\" fn is_even(n: Int) -> Bool ! {} = \"nazm_is_even\" { n % 2 == 0 }\n\n\
                   pub extern \"C\" fn wrap(n: Int) -> Int ! {} = \"nazm_wrap\" { n * 9223372036854775807 }\n";

const HOST: &str = r#"
#include <stdio.h>
#include "libnz.h"
int main(int argc, char **argv) {
    printf("%lld %d %d\n", (long long)nazm_sum_squares(3, 4), nazm_is_even(10), nazm_is_even(7));
    fflush(stdout);
    if (argc > 1) { printf("%lld\n", (long long)nazm_wrap(2)); printf("not reached\n"); }
    return 0;
}
"#;

// Linked and run on macOS here; Linux's is built by the same link with `-shared` and is exercised
// in the Linux container by hand (`docs/performance.md`, N85). Elsewhere it is ignored, not a pass.
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "links a macOS shared library")]
fn a_shared_library_links_into_a_c_program_and_shows_only_its_exports() {
    for backend in ["llvm", "cranelift"] {
        let d = Dir::new(&[("lib.nz", LIB), ("main.c", HOST)]);
        let b = d.nazm(&[
            "build",
            "--lib",
            "--shared",
            "--no-cache",
            "--backend",
            backend,
            "lib.nz",
            "-o",
            "libnz.dylib",
        ]);
        assert!(b.status.success(), "{backend}: {}", stderr(&b));
        // The same header a static library has.
        assert!(std::fs::read_to_string(d.path("libnz.h"))
            .unwrap()
            .contains("int64_t nazm_sum_squares(int64_t a0, int64_t a1);"));
        // Its interface is its exports and nothing of the compiler's or the runtime's.
        let nm = Command::new("nm")
            .args(["-gU", "libnz.dylib"])
            .current_dir(d.dir.path())
            .output()
            .unwrap();
        let mut symbols: Vec<String> = String::from_utf8_lossy(&nm.stdout)
            .lines()
            .filter_map(|l| l.split_whitespace().last().map(str::to_owned))
            .collect();
        symbols.sort();
        assert_eq!(
            symbols,
            ["_nazm_is_even", "_nazm_sum_squares", "_nazm_wrap"],
            "{backend}"
        );
        d.clang(&[
            "main.c",
            "-L.",
            "-lnz",
            "-Wl,-rpath,@executable_path",
            "-o",
            "host",
        ]);
        let ok = Command::new(d.path("host")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&ok.stdout), "25 1 0\n", "{backend}");
        // A failure inside an export ends the process; nothing unwinds into C.
        let f = Command::new(d.path("host")).arg("x").output().unwrap();
        assert_eq!(f.status.code(), Some(2), "{backend}");
        assert!(stderr(&f).contains("N0400"), "{}", stderr(&f));
        assert!(!String::from_utf8_lossy(&f.stdout).contains("not reached"));
    }
    // `--shared` is a kind of library: without `--lib` it is refused by the command line.
    let d = Dir::new(&[("lib.nz", LIB)]);
    assert!(!d
        .nazm(&["build", "--shared", "lib.nz", "-o", "x"])
        .status
        .success());
}

// --- bindings ---------------------------------------------------------------------------------

#[test]
fn bindgen_reads_handles_structs_and_returned_strings_and_names_what_it_refuses() {
    let header = "#include <stdint.h>\n#include <stdbool.h>\n\
                  typedef struct db db;\nstruct counter;\n\
                  struct point { int64_t x; bool flag; int64_t y; };\n\
                  struct seg { struct point a; struct counter *owner; int64_t tag; };\n\
                  struct grid { int64_t cells[4]; };\nunion u { int64_t a; };\n\
                  db *db_open(const char *path);\nint64_t db_close(db *d);\n\
                  const char *db_error(db *d);\nint64_t point_sum(const struct point *p);\n\
                  int64_t seg_score(const struct seg *s);\nstruct point point_make(int64_t x);\n\
                  int64_t apply(int64_t (*f)(int64_t), int64_t x);\nint64_t fill(int64_t *out);\n";
    let d = Dir::new(&[("api.h", header)]);
    let o = d.nazm(&["bindgen", "api.h", "-o", "api.nz"]);
    assert_eq!(o.status.code(), Some(1), "something was refused");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let translated: Vec<&str> = v["translated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["nazm"].as_str().unwrap())
        .collect();
    assert_eq!(
        translated,
        [
            "pub extern \"C\" struct db;",
            "pub extern \"C\" struct counter;",
            "pub extern \"C\" struct point { x: Int, flag: Bool, y: Int }",
            "pub extern \"C\" struct seg { a: point, owner: counter, tag: Int }",
            "pub extern \"C\" fn db_open(path: Str) -> Option[db] = \"db_open\";",
            "pub extern \"C\" fn db_close(d: db) -> Int = \"db_close\";",
            "pub extern \"C\" fn db_error(d: db) -> Option[Str] = \"db_error\";",
            "pub extern \"C\" fn point_sum(p: point) -> Int = \"point_sum\";",
            "pub extern \"C\" fn seg_score(s: seg) -> Int = \"seg_score\";",
        ]
    );
    let refused = |c: &str| {
        v["refused"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["c"].as_str().unwrap().contains(c))
            .map(|r| r["reason"].as_str().unwrap().to_owned())
            .unwrap_or_else(|| panic!("{c} not refused: {v}"))
    };
    assert!(refused("struct grid").contains("array"));
    assert!(refused("union u").contains("union"));
    assert!(refused("point_make").contains("by value"));
    assert!(refused("apply").contains("function pointer"));
    assert!(refused("fill").contains("buffer C writes into"));
    // What it wrote checks.
    let module = std::fs::read_to_string(d.path("api.nz")).unwrap();
    std::fs::write(
        d.path("prog.nz"),
        format!("{module}fn main() -> Int {{ 0 }}\n"),
    )
    .unwrap();
    assert!(d.nazm(&["check", "prog.nz"]).status.success());
}
