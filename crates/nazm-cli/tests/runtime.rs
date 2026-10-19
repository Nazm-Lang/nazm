//! N43: the native runtime, tested directly.
//!
//! `docs/architecture.md` §7.45. The runtime's text (`nazm_lir::emit_runtime`, every part) is
//! compiled by clang with a C harness and nothing else — no Nazm program, no entry — and the
//! harness calls its entry points one by one: string backings, sequences, sequences of strings
//! and of records, channels across threads, the thread-local failure state, and integer
//! formatting. Each check counts through the runtime's own accounting, so a leak, a double
//! release or an underflow is a wrong number rather than a crash that happens to be lucky; and
//! each allocation-failure path is taken on purpose and must leave everything as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::Path;
use std::process::Command;

const HARNESS: &str = r#"
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef __APPLE__
#define NZ(s) __asm__("_nz." s)
#else
#define NZ(s) __asm__("nz." s)
#endif

typedef struct { int64_t len, cap; void *data; int64_t rc; } Seq;
typedef struct { const char *ptr; int64_t len; void *owner; } Str;

extern void *str_alloc(int64_t) NZ("str_alloc");
extern void str_retain(void *) NZ("str_retain");
extern void str_release(void *) NZ("str_release");
extern void *seq_new(void) NZ("seq_new");
extern void *arr_new(void) NZ("arr_new");
extern void *arr_grow(void *, int64_t) NZ("arr_grow");
extern void arr_retain(void *) NZ("arr_retain");
extern bool arr_release(void *) NZ("arr_release");
extern void seq_release(void *) NZ("seq_release");
extern void strs_release(void *) NZ("strs_release");
extern void vec_release(void *, void (*)(void *, void *), int64_t, void *) NZ("vec_release");
extern void *chan_new(int64_t) NZ("chan_new");
extern bool chan_send(void *, int64_t) NZ("chan_send");
extern bool chan_recv(void *, void *, const char *, int64_t) NZ("chan_recv");
extern void chan_close(void *) NZ("chan_close");
extern void chan_retain(void *) NZ("chan_retain");
extern void chan_release(void *) NZ("chan_release");
extern void fail(const char *, int64_t) NZ("fail");
extern int32_t failed_now(void) NZ("failed_now");
extern int64_t format_int(char *, int64_t) NZ("format_int");
extern _Thread_local const char *errmsg NZ("errmsg");
extern int64_t str_allocated NZ("str_allocated");
extern int64_t str_reclaimed NZ("str_reclaimed");
extern int64_t seq_allocated NZ("seq_allocated");
extern int64_t seq_reclaimed NZ("seq_reclaimed");
extern int64_t chan_allocated NZ("chan_allocated");
extern int64_t chan_reclaimed NZ("chan_reclaimed");

static int failures = 0;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d %s\n", __func__, __LINE__, #c); failures++; } } while (0)

static void push_int(Seq *s, int64_t v) {
    int64_t *d = arr_grow(s, 8);
    CHECK(d != NULL);
    d[s->len++] = v;
}

static Str make_str(const char *text) {
    int64_t n = (int64_t)strlen(text);
    char *b = str_alloc(n);
    memcpy(b + 16, text, (size_t)n);
    Str s = { b + 16, n, b };
    return s;
}

static void strings(void) {
    int64_t a0 = str_allocated, r0 = str_reclaimed;
    for (int i = 0; i < 1000000; i++) {
        void *b = str_alloc(8);
        str_retain(b);
        str_release(b);
        CHECK(*(int64_t *)b == 1);
        str_release(b);
    }
    CHECK(str_allocated - a0 == 1000000);
    CHECK(str_reclaimed - r0 == 1000000);
    // Null is an immortal owner: retaining or releasing it touches nothing.
    str_retain(NULL);
    str_release(NULL);
    // A length whose header would overflow is refused, and nothing is counted.
    CHECK(str_alloc(-8) == NULL);
    CHECK(str_allocated - a0 == 1000000);
    // The header: the count at 0, the length at 8, the bytes after 16.
    void *b = str_alloc(5);
    CHECK(*(int64_t *)b == 1 && ((int64_t *)b)[1] == 5);
    str_release(b);
}

static void sequences(void) {
    int64_t a0 = seq_allocated, r0 = seq_reclaimed;
    Seq *s = seq_new();
    CHECK(s->len == 0 && s->cap == 0 && s->data == NULL && s->rc == 1);
    for (int64_t i = 0; i < 100000; i++) push_int(s, i * 3);
    int64_t *d = s->data;
    int64_t sum = 0;
    for (int64_t i = 0; i < s->len; i++) sum += d[i];
    CHECK(sum == 3 * (99999LL * 100000LL / 2));
    CHECK(s->cap >= s->len && s->cap <= 2 * s->len);
    // A growth that would overflow fails and leaves the sequence exactly as it was.
    int64_t len = s->len, cap = s->cap;
    void *data = s->data;
    s->len = s->cap;
    CHECK(arr_grow(s, INT64_MAX / 2) == NULL);
    CHECK(s->cap == cap && s->data == data);
    s->len = len;
    // And one whose byte count wraps to something small — 4 × (2^62 + 1) is 4 modulo 2^64 —
    // is refused too, rather than allocated at four bytes for a caller about to write more.
    Seq *t = seq_new();
    CHECK(arr_grow(t, (INT64_C(1) << 62) + 1) == NULL);
    CHECK(t->cap == 0 && t->data == NULL);
    seq_release(t);
    // Shared: the first release keeps it, the second reclaims it.
    arr_retain(s);
    seq_release(s);
    CHECK(seq_reclaimed - r0 == 1);
    seq_release(s);
    CHECK(seq_allocated - a0 == 2 && seq_reclaimed - r0 == 2);
    seq_release(NULL);
    // The uncounted internal list is not a program sequence.
    Seq *l = arr_new();
    push_int(l, 7);
    CHECK(arr_release(l));
    CHECK(seq_allocated - a0 == 2);
}

static void strings_in_sequences(void) {
    int64_t sa = str_allocated, sr = str_reclaimed, qa = seq_allocated, qr = seq_reclaimed;
    Seq *s = seq_new();
    for (int i = 0; i < 1000; i++) {
        Str *slot = arr_grow(s, sizeof(Str));
        CHECK(slot != NULL);
        slot[s->len++] = make_str("element");
    }
    // One element held elsewhere too: releasing the sequence must leave it alive.
    Str kept = ((Str *)s->data)[500];
    str_retain(kept.owner);
    strs_release(s);
    CHECK(str_allocated - sa == 1000);
    CHECK(str_reclaimed - sr == 999);
    CHECK(memcmp(kept.ptr, "element", 7) == 0);
    str_release(kept.owner);
    CHECK(str_reclaimed - sr == 1000);
    CHECK(seq_allocated - qa == 1 && seq_reclaimed - qr == 1);
}

typedef struct { Str name; int64_t n; Str tag; } Rec;
static int64_t released_records = 0;
static void release_rec(void *p, void *queue) {
    (void)queue;
    Rec *r = p;
    str_release(r->name.owner);
    str_release(r->tag.owner);
    released_records++;
}

static void records_in_sequences(void) {
    int64_t sa = str_allocated, sr = str_reclaimed;
    Seq *s = seq_new();
    for (int i = 0; i < 300; i++) {
        Rec *slot = arr_grow(s, sizeof(Rec));
        Rec r = { make_str("name"), i, make_str("tag") };
        slot[s->len++] = r;
    }
    arr_retain(s);
    vec_release(s, release_rec, sizeof(Rec), NULL);
    CHECK(released_records == 0);
    vec_release(s, release_rec, sizeof(Rec), NULL);
    CHECK(released_records == 300);
    CHECK(str_allocated - sa == 600 && str_reclaimed - sr == 600);
}

/* R1a: an element's release releasing a sequence of its own passes the queue it was given, so
 * the inner sequence is queued and emptied by the outermost release, not by recursion. */
static void release_inner(void *p, void *queue) {
    vec_release(*(void **)p, release_rec, sizeof(Rec), queue);
}

static void sequences_of_sequences(void) {
    int64_t before = released_records;
    Seq *outer = seq_new();
    for (int i = 0; i < 50; i++) {
        Seq *inner = seq_new();
        for (int j = 0; j < 4; j++) {
            Rec *slot = arr_grow(inner, sizeof(Rec));
            Rec r = { make_str("n"), j, make_str("t") };
            slot[inner->len++] = r;
        }
        void **slot = arr_grow(outer, sizeof(void *));
        slot[outer->len++] = inner;
    }
    vec_release(outer, release_inner, sizeof(void *), NULL);
    CHECK(released_records - before == 200);
}

static const char MSG[] = "out of memory";

static void *producer(void *c) {
    for (int64_t i = 1; i <= 20000; i++) CHECK(chan_send(c, i));
    return NULL;
}

static void channels(void) {
    int64_t ca = chan_allocated, cr = chan_reclaimed, qa = seq_allocated, qr = seq_reclaimed;
    void *c = chan_new(2);
    CHECK(c != NULL);
    CHECK(chan_send(c, 10) && chan_send(c, 20));
    Seq *got = seq_new();
    CHECK(chan_recv(c, got, MSG, sizeof MSG - 1));
    CHECK(chan_recv(c, got, MSG, sizeof MSG - 1));
    CHECK(got->len == 2 && ((int64_t *)got->data)[0] == 10 && ((int64_t *)got->data)[1] == 20);
    // Values queued before a close are still received; after them, false.
    CHECK(chan_send(c, 30));
    chan_close(c);
    CHECK(!chan_send(c, 40));
    CHECK(chan_recv(c, got, MSG, sizeof MSG - 1));
    CHECK(!chan_recv(c, got, MSG, sizeof MSG - 1));
    CHECK(got->len == 3 && !failed_now());
    chan_retain(c);
    chan_release(c);
    CHECK(chan_reclaimed == cr);
    chan_release(c);
    CHECK(chan_allocated - ca == 1 && chan_reclaimed - cr == 1);
    seq_release(got);

    // Two threads, a buffer of four: every value arrives, once, in order.
    void *d = chan_new(4);
    pthread_t t;
    pthread_create(&t, NULL, producer, d);
    Seq *all = seq_new();
    for (int i = 0; i < 20000; i++) CHECK(chan_recv(d, all, MSG, sizeof MSG - 1));
    pthread_join(t, NULL);
    int64_t *v = all->data;
    bool ordered = true;
    for (int64_t i = 0; i < all->len; i++) ordered = ordered && v[i] == i + 1;
    CHECK(all->len == 20000 && ordered);
    chan_close(d);
    chan_release(d);
    seq_release(all);
    CHECK(chan_reclaimed - cr == 2);
    CHECK(seq_allocated - qa == 2 && seq_reclaimed - qr == 2);
}

static void *shared_worker(void *b) {
    for (int i = 0; i < 200000; i++) {
        str_retain(b);
        void *own = str_alloc(4);
        str_release(own);
        str_release(b);
    }
    return NULL;
}

static void shared_strings(void) {
    int64_t a0 = str_allocated, r0 = str_reclaimed;
    void *b = str_alloc(16);
    pthread_t t[4];
    for (int i = 0; i < 4; i++) pthread_create(&t[i], NULL, shared_worker, b);
    for (int i = 0; i < 4; i++) pthread_join(t[i], NULL);
    // Four threads retained and released one backing 800,000 times: its count is back to one,
    // and every counter is exact.
    CHECK(*(int64_t *)b == 1);
    str_release(b);
    CHECK(str_allocated - a0 == 800001 && str_reclaimed - r0 == 800001);
}

static void *fresh_thread(void *out) {
    *(int32_t *)out = failed_now();
    fail("other", 5);
    return NULL;
}

static void failure_state(void) {
    CHECK(failed_now() == 0);
    fail("first", 5);
    fail("second", 6);
    CHECK(failed_now() == 1);
    CHECK(memcmp(errmsg, "first", 5) == 0);
    // Thread-local: a new thread starts clean, and its failure is not this one's.
    int32_t seen = -1;
    pthread_t t;
    pthread_create(&t, NULL, fresh_thread, &seen);
    pthread_join(t, NULL);
    CHECK(seen == 0);
    CHECK(memcmp(errmsg, "first", 5) == 0);
}

static void formatting(void) {
    char buf[21];
    int64_t n = format_int(buf, INT64_MIN);
    CHECK(n == 20 && memcmp(buf, "-9223372036854775808", 20) == 0);
    n = format_int(buf, INT64_MAX);
    CHECK(n == 19 && memcmp(buf, "9223372036854775807", 19) == 0);
    CHECK(format_int(buf, 0) == 1 && buf[0] == '0');
}

int main(void) {
    strings();
    sequences();
    strings_in_sequences();
    records_in_sequences();
    sequences_of_sequences();
    channels();
    shared_strings();
    formatting();
    failure_state();
    if (failures == 0) printf("ok\n");
    return failures == 0 ? 0 : 1;
}
"#;

fn all() -> nazm_lir::Needs {
    nazm_lir::Needs {
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
        numbers: true,
        files: true,
        net: true,
        wide: true,
        // TLS's engine is a unit of its own linked with the platform's library (`tls.rs` tests it).
        tls: false,
        single: false,
    }
}

fn build(dir: &Path) -> std::path::PathBuf {
    std::fs::write(dir.join("runtime.ll"), nazm_lir::emit_runtime(all())).unwrap();
    // The task pool's switch for this host (N83), which the runtime's pool calls.
    use nazm_lir::runtime::platform::{emit_os, emit_stack};
    use nazm_lir::runtime::switch::{emit_errno, emit_switch, SwitchTarget};
    let host = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => SwitchTarget::Aarch64Darwin,
        ("aarch64", _) => SwitchTarget::Aarch64Linux,
        ("x86_64", "macos") => SwitchTarget::X86_64Darwin,
        ("x86_64", "windows") => SwitchTarget::X86_64Windows,
        _ => SwitchTarget::X86_64Linux,
    };
    std::fs::write(dir.join("switch.ll"), emit_switch(host)).unwrap();
    // And where it keeps `errno` (N85), which the runtime's capture asks.
    std::fs::write(dir.join("errno.ll"), emit_errno(host)).unwrap();
    // And what its C library says about files (Gate 2).
    std::fs::write(dir.join("os.ll"), emit_os(host)).unwrap();
    // And its task stacks, stack budget and clocks (Gate 3).
    std::fs::write(dir.join("stack.ll"), emit_stack(host)).unwrap();
    std::fs::write(dir.join("harness.c"), HARNESS).unwrap();
    let exe = dir.join(crate::common::exe_name("harness"));
    let mut clang = Command::new("clang");
    clang.args(["-O1", "-Wno-override-module", "-pthread"]);
    if cfg!(windows) {
        // The `os` unit's sockets and random bytes are Winsock's and CNG's; `nazm build` links
        // them the same way (`-lws2_32 -lbcrypt`).
        clang.args(["-lws2_32", "-lbcrypt"]);
    }
    let o = clang
        .arg(dir.join("runtime.ll"))
        .arg(dir.join("switch.ll"))
        .arg(dir.join("errno.ll"))
        .arg(dir.join("os.ll"))
        .arg(dir.join("stack.ll"))
        .arg(dir.join("harness.c"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("clang");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    exe
}

#[test]
fn the_runtime_s_services_keep_their_contract_when_called_directly() {
    let dir = tempfile::tempdir().unwrap();
    let exe = build(dir.path());
    // A deadline: a runtime whose channel loses a wakeup blocks for ever rather than failing.
    let mut child = common::bounded_exe(&exe)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run");
    let start = std::time::Instant::now();
    while child.try_wait().expect("wait").is_none() {
        if start.elapsed() > std::time::Duration::from_secs(120) {
            let _ = child.kill();
            panic!("the harness did not finish: a deadlock or a lost wakeup in the runtime");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let o = child.wait_with_output().expect("output");
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(out, "ok\n");
}

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

#[derive(Debug)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Build `src` with `backend` and run it with `args`, with or without the memory report.
fn run(src: &str, backend: &str, args: &[&str], report: bool) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let exe = dir.path().join(crate::common::exe_name("prog"));
    let b = Command::new(NAZM)
        .args(["build", "--no-cache", "--backend", backend])
        .arg(&path)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let mut c = common::bounded_exe(&exe);
    c.args(args);
    if report {
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
    } else {
        c.env_remove(nazm_lir::MEMORY_VARIABLE);
    }
    let o = c.output().unwrap();
    Ran {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

#[test]
fn the_three_ways_a_program_ends_are_distinguished() {
    for backend in ["llvm", "cranelift"] {
        // Normal return: the report (when asked for), then the result on stdout, status 0.
        let r = run(
            "fn main(io: IoCap) -> Int { let s = str_concat(\"a\", \"b\"); print(s); 7 }\n",
            backend,
            &[],
            true,
        );
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "ab7\n"),
            "{backend}: {r:?}"
        );
        assert!(r.stderr.starts_with("nazm-memory: "), "{backend}: {r:?}");
        assert!(
            r.stderr.contains("strings allocated=1 reclaimed=1 live=0"),
            "{backend}: {r:?}"
        );
        // Without the variable, nothing on stderr.
        let r = run("fn main() -> Int { 7 }\n", backend, &[], false);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (Some(0), "7\n", ""),
            "{backend}"
        );

        // Failure: the report first (the counts are final), then the first failure, status 2,
        // and no result.
        let r = run(
            "fn main() -> Int { let s = str_concat(\"a\", \"b\"); 9223372036854775807 + str_len(s) }\n",
            backend,
            &[],
            true,
        );
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(2), ""),
            "{backend}: {r:?}"
        );
        let lines: Vec<&str> = r.stderr.lines().collect();
        assert!(
            lines[0].starts_with("nazm-memory: ") && lines[0].contains("live=0"),
            "{backend}: {r:?}"
        );
        assert!(lines[1].contains("N0400"), "{backend}: {r:?}");

        // Explicit exit: that status at once — no report, no result, nothing reclaimed.
        let r = run(
            "fn main(io: IoCap) -> Int { let s = str_concat(\"a\", \"b\"); print(s); exit_with(5); 0 }\n",
            backend,
            &[],
            true,
        );
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (Some(5), "ab", ""),
            "{backend}: {r:?}"
        );
    }
}

#[test]
fn an_err_is_data_and_never_a_failure() {
    let src = "fn parse(n: Int) -> Result[Int, Str] { if n > 0 { Result[Int, Str].Ok(value: n) } else { Result[Int, Str].Err(error: \"no\") } }\n\
               fn main() -> Int { match parse(0) { Result.Ok(value: v) => v, Result.Err(error: e) => str_len(e) + 40 } }\n";
    for backend in ["llvm", "cranelift"] {
        let r = run(src, backend, &[], true);
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "42\n"),
            "{backend}: {r:?}"
        );
        assert!(
            r.stderr.contains("live=0") && !r.stderr.contains("N04"),
            "{backend}: {r:?}"
        );
    }
}

#[test]
fn the_arguments_are_stored_once_at_startup_without_the_program_name() {
    let src = "fn main(io: IoCap) -> Int { let mut i = 0; while i < args_count() { print(arg(i)); print(\",\"); i = i + 1; } args_count() }\n";
    for backend in ["llvm", "cranelift"] {
        let r = run(src, backend, &["one", "two words", ""], true);
        assert_eq!(r.code, Some(0), "{backend}: {r:?}");
        assert_eq!(r.stdout, "one,two words,,3\n", "{backend}: {r:?}");
        // `arg` strings are the process's own storage: nothing to allocate, nothing to free.
        assert!(
            r.stderr.contains("strings allocated=0 reclaimed=0 live=0"),
            "{backend}: {r:?}"
        );
        let r = run(src, backend, &[], false);
        assert_eq!(r.stdout, "0\n", "{backend}: {r:?}");
    }
}

#[test]
fn root_capabilities_come_from_the_entry_alone() {
    // `main` may take each kind, in any order, and is handed one of each.
    let src = "fn show(io: IoCap) -> Int ! { io } { print(\"ok\") }\n\
               fn main(t: SpawnCap, f: ForeignCap, io: IoCap) -> Int { show(io); 0 }\n";
    for backend in ["llvm", "cranelift"] {
        let r = run(src, backend, &[], false);
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "ok0\n"),
            "{backend}: {r:?}"
        );
    }
    // Nothing else mints one: a capability cannot be constructed, and no runtime entry
    // returns anything a program could take for one.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forge.nz");
    std::fs::write(&path, "fn main() -> Int { let c = IoCap(); 0 }\n").unwrap();
    let o = Command::new(NAZM)
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("\"N0370\""),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    for f in nazm_lir::runtime::INVENTORY {
        assert!(
            f.effect.is_empty() || ["io", "spawn"].contains(&f.effect),
            "{}",
            f.symbol
        );
    }
}

#[test]
fn every_targets_os_unit_is_llvm_its_toolchain_compiles() {
    // Gate 2: the files and network facts differ by target (`platform::emit_os`); each target's
    // unit must at least be well-formed LLVM for that target, or a cross build fails at link time.
    // Gate 3: and so must its stack and clock unit, and Windows' `init` unit.
    use nazm_lir::runtime::platform::{emit_init, emit_os, emit_stack};
    use nazm_lir::runtime::switch::SwitchTarget;
    let dir = tempfile::tempdir().unwrap();
    for (t, triple) in [
        (SwitchTarget::Aarch64Darwin, "arm64-apple-macos"),
        (SwitchTarget::X86_64Darwin, "x86_64-apple-macos"),
        (SwitchTarget::Aarch64Linux, "aarch64-linux-gnu"),
        (SwitchTarget::X86_64Linux, "x86_64-linux-gnu"),
        (SwitchTarget::X86_64Windows, "x86_64-pc-windows-gnu"),
    ] {
        let units = [
            ("os", Some(emit_os(t))),
            ("stack", Some(emit_stack(t))),
            ("init", emit_init(t)),
        ];
        for (unit, text) in units {
            let Some(text) = text else { continue };
            let ll = dir.path().join(format!("{triple}-{unit}.ll"));
            std::fs::write(&ll, text).unwrap();
            let o = Command::new("clang")
                .args(["-c", "-Wno-override-module", &format!("--target={triple}")])
                .arg(&ll)
                .arg("-o")
                .arg(dir.path().join(format!("{triple}-{unit}.o")))
                .output()
                .expect("clang");
            assert!(
                o.status.success(),
                "{triple} {unit}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
    }
}
