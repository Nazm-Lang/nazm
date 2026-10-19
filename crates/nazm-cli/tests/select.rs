//! N54: select, deadlines, cooperative cancellation and the scheduler's counters.
//!
//! `docs/internal/architecture.md` §7.56 and the spec section *Select, deadlines and cancellation*. What
//! only a whole run shows: that a select takes the lowest ready channel, a closed one included,
//! under the interpreter and both native backends alike — the same stdout, status, memory
//! report and scheduler counts — that a waiting select is woken by another task's send and by
//! a close, which is how a task is cancelled; that a deadline is a lower bound; and that a
//! clock needs a `TimeCap` and `critical` refuses one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use common::{run_bounded, Ran};
use differential::{codes, memory_line, nazm, NAZM};

use std::process::Command;

fn sched_line(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-sched:"))
        .unwrap_or_else(|| panic!("no scheduler report: {}", r.stderr))
        .to_owned()
}

/// The program `files[0]` under the interpreter and built and run by each backend at each
/// optimisation level, every one with the memory report on.
fn everywhere(files: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let main = files[0].0;
    let mut interpreted = Command::new(NAZM);
    interpreted
        .current_dir(d.path())
        .args(["run", "--interpret", main])
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .env(nazm_lir::runtime::SCHED_VARIABLE, "1");
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(&mut interpreted, "nazm run"),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                main,
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env(nazm_lir::MEMORY_VARIABLE, "1")
            .env(nazm_lir::runtime::SCHED_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected`, exits with `code`, writes the same memory report
/// — closure objects counted with everything else — and reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) -> String {
    let runs = everywhere(files);
    let first = memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        assert_eq!(
            sched_line(r),
            sched_line(&runs[0].1),
            "{label} and the interpreter disagree about what the scheduler did"
        );
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
    first
}

fn agrees(src: &str, expected: &str) -> String {
    agrees_on(&[("prog.nz", src)], expected, 0)
}

// --- choosing ------------------------------------------------------------------------------

#[test]
fn a_select_takes_the_lowest_ready_channel_everywhere() {
    let line = agrees(
        r#"
fn main(out: OutCap) -> Int {
    let a = chan_new_of[Int](2);
    let b = chan_new_of[Int](2);
    chan_send_of(a, 1);
    chan_send_of(b, 2);
    chan_send_of(b, 3);
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, a);
    vec_push(cs, b);
    let got = vec_new[Int]();
    let mut i = 0;
    while i < 3 {
        print(int_to_str(chan_select_of(cs, got)));
        i = i + 1;
    }
    print(":");
    print(int_to_str(vec_get(got, 0) * 100 + vec_get(got, 1) * 10 + vec_get(got, 2)));
    chan_close_of(b);
    // `b` closed and drained, `a` empty and open: `b` is the one ready, and gives nothing.
    print(":");
    print(int_to_str(chan_select_of(cs, got)));
    print(int_to_str(vec_len(got)));
    chan_close_of(a);
    print(int_to_str(chan_select_of(cs, got)));
    let none = vec_new[Chan[Int]]();
    print(":");
    print(int_to_str(chan_select_of(none, got)));
    0
}
"#,
        "011:123:130:-10",
    );
    assert!(line.contains("channels allocated=2"), "{line}");
}

#[test]
fn a_value_select_did_not_choose_stays_queued_and_is_released_once() {
    agrees(
        r#"
struct Note { text: Str, }

fn main(out: OutCap) -> Int {
    let a = chan_new_of[Note](1);
    let b = chan_new_of[Note](1);
    chan_send_of(a, Note(text: str_concat("fi", "rst")));
    chan_send_of(b, Note(text: str_concat("sec", "ond")));
    let cs = vec_new[Chan[Note]]();
    vec_push(cs, a);
    vec_push(cs, b);
    let got = vec_new[Note]();
    chan_select_of(cs, got);
    print(vec_pop(got).text);
    0
}
"#,
        "first0",
    );
}

// --- waiting and cancelling -------------------------------------------------------------------

#[test]
fn a_waiting_select_is_woken_by_another_task_s_send() {
    // Q1-M-17: the sender waits for `go`, which `main` sends just before it selects, and then
    // sleeps, so the select has found nothing and is parked when the value comes — only the wake
    // of a parked selector can end it. Without the handshake the send could land first, and the
    // test passed whether or not a parked select was ever woken.
    agrees(
        r#"
fn later(t: TimeCap, go: Chan[Int], c: Chan[Str]) -> Bool ! {} {
    let g = vec_new[Int]();
    chan_recv_of(go, g);
    time_sleep_ms(50);
    chan_send_of(c, str_concat("wo", "ke"))
}

fn main(io: IoCap, sp: SpawnCap, t: TimeCap) -> Int ! { io, spawn } {
    let go = chan_new_of[Int](1);
    let c = chan_new_of[Str](1);
    let idle = chan_new_of[Str](1);
    let cs = vec_new[Chan[Str]]();
    vec_push(cs, idle);
    vec_push(cs, c);
    let got = vec_new[Str]();
    let mut i = 0;
    scope {
        spawn later(t, go, c);
        chan_send_of(go, 1);
        i = chan_select_of(cs, got);
    }
    print(int_to_str(i));
    print(vec_pop(got));
    0
}
"#,
        "1woke0",
    );
}

#[test]
fn closing_a_channel_cancels_a_task_waiting_in_select() {
    // The worker doubles jobs until `stop` is closed. It is waiting in a select when the close
    // comes — every job it was sent has been answered, and `main` sleeps before closing so the
    // worker has looked again and parked (Q1-M-17) — so only the close can end it, and the
    // scope's join returning at all is the evidence.
    agrees(
        r#"
fn worker(stop: Chan[Int], jobs: Chan[Int], out: Chan[Int]) -> Int ! {} {
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, stop);
    vec_push(cs, jobs);
    let got = vec_new[Int]();
    let mut running = true;
    let mut done = 0;
    while running {
        let i = chan_select_of(cs, got);
        if i == 1 && vec_len(got) > 0 {
            chan_send_of(out, vec_pop(got) * 2);
            done = done + 1;
        } else {
            running = false;
        }
    }
    done
}

fn main(io: IoCap, sp: SpawnCap, t: TimeCap) -> Int ! { io, spawn } {
    let stop = chan_new_of[Int](1);
    let jobs = chan_new_of[Int](4);
    let out = chan_new_of[Int](4);
    let r = vec_new[Int]();
    let mut sum = 0;
    scope {
        spawn worker(stop, jobs, out);
        chan_send_of(jobs, 5);
        chan_send_of(jobs, 6);
        if chan_recv_of(out, r) { sum = sum + vec_pop(r); }
        if chan_recv_of(out, r) { sum = sum + vec_pop(r); }
        time_sleep_ms(50);
        chan_close_of(stop);
    }
    print(int_to_str(sum));
    0
}
"#,
        "220",
    );
}

#[test]
fn a_deadline_is_a_lower_bound_and_a_zero_one_only_looks() {
    agrees(
        r#"
fn main(io: IoCap, t: TimeCap) -> Int ! { io } {
    let quiet = chan_new_of[Int](1);
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, quiet);
    let got = vec_new[Int]();
    let t0 = time_now_ms();
    print(int_to_str(chan_select_until(cs, got, 40)));
    if time_now_ms() - t0 >= 40 { print(" waited") } else { print(" early") };
    print(" ");
    print(int_to_str(chan_select_until(cs, got, 0)));
    chan_send_of(quiet, 7);
    print(" ");
    print(int_to_str(chan_select_until(cs, got, 0)));
    print(int_to_str(vec_pop(got)));
    0
}
"#,
        "-2 waited -2 070",
    );
}

#[test]
fn the_scheduler_s_counters_agree_everywhere() {
    let runs = everywhere(&[(
        "prog.nz",
        r#"
fn one(c: Chan[Int]) -> Bool ! {} { chan_send_of(c, 1) }

fn main(sp: SpawnCap, t: TimeCap) -> Int ! { spawn } {
    let c = chan_new_of[Int](4);
    scope {
        spawn one(c);
        spawn one(c);
        spawn one(c);
    }
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, c);
    let got = vec_new[Int]();
    chan_select_of(cs, got);
    chan_select_until(cs, got, 1);
    let empty = chan_new_of[Int](1);
    let es = vec_new[Chan[Int]]();
    vec_push(es, empty);
    chan_select_until(es, got, 1)
}
"#,
    )]);
    for (label, r) in &runs {
        assert_eq!(
            sched_line(r),
            "nazm-sched: spawned=3 joined=3 selects=3 timeouts=1",
            "{label}"
        );
    }
}

// --- authority and refusals ---------------------------------------------------------------------

#[test]
fn a_clock_needs_a_time_cap_held_where_it_is_read() {
    assert_eq!(codes("fn main() -> Int { time_now_ms() }\n"), ["N0369"]);
    assert_eq!(
        codes(
            "fn main() -> Int { let cs = vec_new[Chan[Int]](); \
             chan_select_until(cs, vec_new[Int](), 1) }\n"
        ),
        ["N0369"]
    );
    // Held as a parameter, in a function with a declared contract or none; a select without
    // a deadline needs nothing.
    assert_eq!(
        codes("fn now(t: TimeCap) -> Int ! {} { time_now_ms() }\nfn main(t: TimeCap) -> Int { now(t) }\n"),
        Vec::<String>::new()
    );
    assert_eq!(
        codes(
            "fn main() -> Int { let cs = vec_new[Chan[Int]](); \
             chan_select_of(cs, vec_new[Int]()) }\n"
        ),
        Vec::<String>::new()
    );
    // The vector a select fills holds the channels' element, nothing else.
    assert!(!codes(
        "fn main() -> Int { let cs = vec_new[Chan[Int]](); \
         chan_select_of(cs, vec_new[Str]()) }\n"
    )
    .is_empty());
}

#[test]
fn critical_refuses_a_clock() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main(t: TimeCap) -> Int ! {} { time_now_ms() }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "critical", "prog.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(
        r.stderr.contains("N0510") && r.stderr.contains("no-ambient-time"),
        "{r:?}"
    );
    let ok = nazm(d.path(), &["check", "--profile", "general", "prog.nz"]);
    assert_eq!(ok.code, Some(0), "{ok:?}");
}

#[test]
fn a_select_s_operations_are_resource_sites() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main() -> Int {\n    let cs = vec_new[Chan[Int]]();\n    \
         chan_select_of(cs, vec_new[Int]())\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["explain-cost", "prog.nz", "--json"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    let ops: Vec<&str> = v["functions"][0]["sites"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == "block")
        .filter_map(|s| s["op"].as_str())
        .collect();
    assert_eq!(ops, ["chan_select_of"], "{v}");
}

/// A C shim the native programs below run with: 150 ms after its first call, the wall clock
/// (`CLOCK_REALTIME`) it reports steps back an hour, as an operator's or NTP's correction might;
/// the monotonic clock is untouched. Interposed with `DYLD_INSERT_LIBRARIES` or `LD_PRELOAD`.
#[cfg(unix)]
const CLOCK_STEP: &str = r#"
#define _GNU_SOURCE
#include <stdatomic.h>
#include <time.h>
static _Atomic long long first_ms = -1;
static long long mono_ms(void) {
    struct timespec m;
#ifdef __APPLE__
    clock_gettime(CLOCK_MONOTONIC_RAW, &m);
#else
    clock_gettime(CLOCK_MONOTONIC, &m);
#endif
    return (long long)m.tv_sec * 1000 + m.tv_nsec / 1000000;
}
static void step(clockid_t c, struct timespec *t) {
    if (c != CLOCK_REALTIME) return;
    long long now = mono_ms(), none = -1;
    atomic_compare_exchange_strong(&first_ms, &none, now);
    if (now - atomic_load(&first_ms) >= 150) t->tv_sec -= 3600;
}
#ifdef __APPLE__
static int shim(clockid_t c, struct timespec *t) { int r = clock_gettime(c, t); step(c, t); return r; }
__attribute__((used)) static struct { const void *n; const void *o; } interposers[]
    __attribute__((section("__DATA,__interpose"))) = { { (const void *)shim, (const void *)clock_gettime } };
#else
#include <dlfcn.h>
int clock_gettime(clockid_t c, struct timespec *t) {
    static int (*real)(clockid_t, struct timespec *);
    if (!real) real = (int (*)(clockid_t, struct timespec *))dlsym(RTLD_NEXT, "clock_gettime");
    int r = real(c, t);
    step(c, t);
    return r;
}
#endif
"#;

/// Q1-M-12: a deadline is on the monotonic clock, so a wall clock stepped back an hour while a
/// select or a sleep waits moves nothing — under both backends, on a thread and as a pool task.
/// With the deadline on the wall clock, the wait found its deadline an hour away and did not end.
#[cfg(unix)]
#[test]
fn a_wall_clock_stepped_back_moves_no_deadline() {
    let d = tempfile::tempdir().unwrap();
    let shim = d.path().join(if cfg!(target_os = "macos") {
        "step.dylib"
    } else {
        "step.so"
    });
    std::fs::write(d.path().join("step.c"), CLOCK_STEP).unwrap();
    let mut cc = Command::new("clang");
    cc.current_dir(d.path());
    if cfg!(target_os = "macos") {
        cc.args(["-dynamiclib", "step.c", "-o"]);
    } else {
        cc.args(["-shared", "-fPIC", "step.c", "-ldl", "-o"]);
    }
    let o = cc.arg(&shim).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::write(
        d.path().join("prog.nz"),
        r#"
fn wait(io: IoCap, t: TimeCap, done: Chan[Int]) -> Int ! { io } {
    let quiet = chan_new_of[Int](1);
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, quiet);
    let got = vec_new[Int]();
    let t0 = time_now_ms();
    let r = chan_select_until(cs, got, 1000);
    time_sleep_ms(300);
    chan_send_of(done, time_now_ms() - t0);
    r
}

fn main(io: IoCap, t: TimeCap, sp: SpawnCap) -> Int ! { io, spawn } {
    let done = chan_new_of[Int](2);
    let mine = wait(io, t, done);
    scope { spawn wait(io, t, done); }
    let got = vec_new[Int]();
    chan_recv_of(done, got);
    chan_recv_of(done, got);
    let b = vec_pop(got);
    let a = vec_pop(got);
    if a >= 1300 && b >= 1300 && a < 4000 && b < 4000 { print("on time") } else { print("off") };
    print(" ");
    print(int_to_str(mine));
    0
}
"#,
    )
    .unwrap();
    let inject = if cfg!(target_os = "macos") {
        "DYLD_INSERT_LIBRARIES"
    } else {
        "LD_PRELOAD"
    };
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(crate::common::exe_name(backend));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        for scheduler in ["pool", "threads"] {
            // Not through `bounded_exe`: its `/bin/sh` is a protected binary on macOS, whose
            // loader drops `DYLD_*` before the program runs.
            let mut c = Command::new(&exe);
            c.env(inject, &shim).env("NAZM_SCHEDULER", scheduler);
            let r = common::run_within(&mut c, std::time::Duration::from_secs(20)).unwrap_or_else(
                || panic!("{backend}, {scheduler}: a wait did not end — its deadline moved"),
            );
            assert_eq!(
                (r.code, r.stdout.as_str()),
                (Some(0), "on time -20\n"),
                "{backend}, {scheduler}: {r:?}"
            );
        }
    }
}

// --- a select over channels of different types (performance leadership 6.4) ------------------

/// `src` under the interpreter, both native backends at each level and — on macOS, where it
/// runs before 1.0 — Nazm's own code generator: each prints `expected`, ends 0, writes the
/// interpreter's memory report and scheduler counts, and reclaims everything.
fn select_agrees(src: &str, expected: &str) {
    let mut runs = everywhere(&[("prog.nz", src)]);
    if cfg!(target_os = "macos") {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), src).unwrap();
        let exe = d.path().join(crate::common::exe_name("nzcg-built"));
        let mut build = Command::new(NAZM);
        build
            .current_dir(d.path())
            .env(nazm_codegen_nazm::VARIABLE, crate::differential::nzcg())
            .args([
                "build",
                "--no-cache",
                "--backend",
                "nazm",
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]);
        let b = run_bounded(&mut build, "nazm build --backend nazm");
        assert_eq!(b.code, Some(0), "nazm backend: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env(nazm_lir::MEMORY_VARIABLE, "1")
            .env(nazm_lir::runtime::SCHED_VARIABLE, "1");
        runs.push((
            "nazm backend".to_owned(),
            run_bounded(&mut c, "nazm backend"),
        ));
    }
    let first = memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        assert_eq!(
            sched_line(r),
            sched_line(&runs[0].1),
            "{label}: scheduler counts"
        );
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
}

#[test]
fn a_select_over_channels_of_different_types_performs_the_lowest_ready_case_everywhere() {
    select_agrees(
        r#"
fn main(out: OutCap) -> Int {
    let a = chan_new_of[Int](2);
    let b = chan_new_of[Str](2);
    let d = chan_new_of[Str](1);
    chan_send_of(b, str_concat("x", "y"));
    let ints = vec_new[Int]();
    let strs = vec_new[Str]();
    let s = sel_new();
    let ia = sel_recv(s, a, ints);
    let ib = sel_recv(s, b, strs);
    let ic = sel_send(s, d, str_concat("se", "nt"));
    print(int_to_str(ia * 100 + ib * 10 + ic));
    print(":");
    // `b` holds a value; `a` is empty.
    print(int_to_str(sel_wait(s)));
    print(vec_get(strs, 0));
    // Nothing to receive; `d` has room, so the send case is performed, once.
    print(int_to_str(sel_wait(s)));
    let t = s;
    chan_send_of(a, 7);
    print(int_to_str(sel_wait(t)));
    print(int_to_str(vec_get(ints, 0)));
    // `b` closed and drained: its index, encoded; the spent send case is not looked at.
    chan_close_of(b);
    print(int_to_str(sel_wait(s)));
    let got = vec_new[Str]();
    if chan_recv_of(d, got) { print(vec_get(got, 0)); }
    // A select whose only case is a spent send has nothing to wait on.
    let e = sel_new();
    let f = chan_new_of[Int](1);
    let k = sel_send(e, f, 5);
    print(int_to_str(sel_wait(e)));
    print(int_to_str(sel_wait(e)));
    print(int_to_str(sel_wait(sel_new())));
    0
}
"#,
        "12:1xy207-4sent0-1-10",
    );
}

#[test]
fn a_send_case_refused_by_a_close_keeps_its_value_and_a_select_released_unsent_releases_it() {
    select_agrees(
        r#"
fn main(out: OutCap) -> Int {
    let c = chan_new_of[Str](1);
    chan_send_of(c, str_concat("fu", "ll"));
    let s = sel_new();
    let i = sel_send(s, c, str_concat("wait", "ing"));
    chan_close_of(c);
    // Closed: refused, and the case is ready — refused — at every wait.
    print(int_to_str(sel_wait(s)));
    print(int_to_str(sel_wait(s)));
    // A select dropped with a value it never sent, of a channel it holds the last reference to.
    let u = sel_new();
    let v = chan_new_of[Str](1);
    chan_send_of(v, str_concat("a", "b"));
    let w = sel_send(u, v, str_concat("c", "d"));
    let ws = vec_new[Str]();
    let r = sel_recv(u, chan_new_of[Str](1), ws);
    print(int_to_str(i + w + r));
    0
}
"#,
        "-3-310",
    );
}

#[test]
fn a_waiting_select_over_different_types_is_woken_by_a_send_a_receive_and_a_close() {
    select_agrees(
        r#"
fn producer(c: Chan[Str], n: Int) -> Int ! {} {
    let mut i = 0;
    while i < n { chan_send_of(c, int_to_str(i)); i = i + 1; }
    chan_close_of(c)
}
fn drainer(c: Chan[Int], n: Int, done: Chan[Int]) -> Int ! {} {
    let got = vec_new[Int]();
    let mut sum = 0;
    let mut k = 0;
    while k < n { if chan_recv_of(c, got) { sum = sum + vec_pop(got); } k = k + 1; }
    chan_send_of(done, sum);
    0
}
fn main(out: OutCap, t: SpawnCap) -> Int {
    let words = chan_new_of[Str](1);
    let nums = chan_new_of[Int](1);
    let done = chan_new_of[Int](1);
    let texts = vec_new[Str]();
    let mut seen = 0;
    let mut sent = 0;
    scope {
        spawn producer(words, 20);
        spawn drainer(nums, 5, done);
        while seen < 20 || sent < 5 {
            // A select per round, built afresh: a receive of the next word while words are
            // due, a send of the next number while numbers are.
            let s = sel_new();
            let mut r = 0 - 10;
            let mut k = 0 - 10;
            if seen < 20 { r = sel_recv(s, words, texts); }
            if sent < 5 { k = sel_send(s, nums, sent + 1); }
            let i = sel_wait(s);
            if i == r { seen = seen + 1; } else if i == k { sent = sent + 1; }
        }
        let got = vec_new[Int]();
        if chan_recv_of(done, got) { print(int_to_str(vec_pop(got))); }
    }
    print(":");
    print(int_to_str(seen));
    print(":");
    print(vec_get(texts, 19));
    0
}
"#,
        "15:20:190",
    );
}

#[test]
fn a_select_deadline_needs_a_time_cap_and_is_a_lower_bound() {
    select_agrees(
        r#"
fn main(out: OutCap, tm: TimeCap) -> Int {
    let c = chan_new_of[Int](1);
    let got = vec_new[Int]();
    let s = sel_new();
    let i = sel_recv(s, c, got);
    let before = time_now_ms();
    print(int_to_str(sel_wait_until(s, 30)));
    print(if time_now_ms() - before >= 30 { " waited " } else { " early " });
    print(int_to_str(sel_wait_until(s, 0)));
    0
}
"#,
        "-2 waited -20",
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main(out: OutCap) -> Int {\n    let s = sel_new();\n    sel_wait_until(s, 5)\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "prog.nz"]);
    assert_ne!(r.code, Some(0), "{}", r.stderr);
    assert!(r.stderr.contains("N0369"), "{}", r.stderr);
}

#[test]
fn a_select_never_crosses_into_a_task_and_its_name_is_the_languages() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("task.nz"),
        "fn w(s: Select) -> Int ! {} { sel_wait(s) }\n\
         fn main(t: SpawnCap) -> Int {\n    let s = sel_new();\n    scope { spawn w(s); }\n    0\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "task.nz"]);
    assert!(r.stderr.contains("N0321"), "{}", r.stderr);
    std::fs::write(
        d.path().join("name.nz"),
        "struct Select { x: Int }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "name.nz"]);
    assert!(r.stderr.contains("N0331"), "{}", r.stderr);
    // A program's own function of a built-in's name hides it (a prelude name).
    std::fs::write(
        d.path().join("own.nz"),
        "fn sel_new() -> Int { 4 }\nfn main(out: OutCap) -> Int { print(int_to_str(sel_new())); 0 }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "own.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert!(r.stdout.starts_with('4'), "{}", r.stdout);
}
