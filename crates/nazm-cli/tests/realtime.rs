//! N63: real-time bounds — what is bounded, what is measured, what is unknown.
//!
//! `docs/architecture.md` §7.65. What a whole check or build shows: the `realtime` profile accepts
//! a program whose loops have the counted form and states each loop's bound, its call depth and its
//! site counts, with `wcet` unknown; it refuses an unbounded loop as `unknown` with the reason, and a
//! blocking channel operation and a clock by site; a board build writes `bounds.json` — a static
//! stack bound along the image's deepest path, or `null` with why; and `--stack-watermark` is the
//! board's alone. That a run stays inside its bound is the ignored test at the end, run in the
//! `nazm-qemu:n62` container — a measurement held against the bound, never the bound.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const BOARD: &str = "aarch64-unknown-none";

/// Three counted loops: 10 turns, 4 (10, 7, 4, 1), and 3 (0, 2, 4 against an immutable 5).
const BOUNDED: &str = "fn sum(n: Int) -> Int {\n\
                       \x20   let mut i = 0;\n\
                       \x20   let mut s = n;\n\
                       \x20   while i < 10 {\n\
                       \x20       let mut j = 10;\n\
                       \x20       while j >= 1 { s = s + j; j = j - 3; }\n\
                       \x20       i = i + 1;\n\
                       \x20   }\n\
                       \x20   s\n\
                       }\n\
                       fn main() -> Int ! {} {\n\
                       \x20   let limit = 5;\n\
                       \x20   let mut k = 0;\n\
                       \x20   while k <= limit { k = k + 2; }\n\
                       \x20   sum(k)\n\
                       }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn program(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    d
}

fn report(src: &str) -> (bool, serde_json::Value) {
    let d = program(src);
    let o = nazm(
        d.path(),
        &[
            "check",
            "--profile",
            "realtime",
            "--profile-report",
            "prog.nz",
        ],
    );
    let line = String::from_utf8_lossy(&o.stdout)
        .lines()
        .find(|l| l.contains("nazm.profile-report/1"))
        .unwrap_or_else(|| panic!("no report: {}", err(&o)))
        .to_owned();
    (o.status.success(), serde_json::from_str(&line).unwrap())
}

fn verdict(v: &serde_json::Value, id: &str) -> String {
    v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .map(|r| r["verdict"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| panic!("no rule {id}: {v}"))
}

/// The bound of a one-loop `main` with this counter, condition and step.
fn one_loop(init: &str, cond: &str, step: &str) -> serde_json::Value {
    let src = format!(
        "fn main() -> Int ! {{}} {{ let mut i = {init}; let mut t = 0; \
         while {cond} {{ t = t + 1; i = {step}; }} t }}\n"
    );
    let (_, v) = report(&src);
    v["bounds"]["loops"][0].clone()
}

#[test]
fn realtime_states_every_bound_it_establishes() {
    let (ok, v) = report(BOUNDED);
    assert!(ok, "{v}");
    assert_eq!(v["outcome"], "accepted");
    let rules: Vec<&str> = v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        rules,
        [
            "no-io",
            "no-spawn",
            "bounded-allocation",
            "no-ambient-time",
            "no-recursion",
            "no-blocking",
            "bounded-loops"
        ]
    );
    let b = &v["bounds"];
    let loops: Vec<u64> = b["loops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["bound"].as_u64().unwrap())
        .collect();
    assert_eq!(loops, [10, 4, 3]);
    assert_eq!(b["loops"][0]["function"], "sum");
    assert_eq!(b["loops"][2]["function"], "main");
    assert_eq!(b["call_depth"], 2);
    assert_eq!(b["allocation_sites"], 0);
    assert_eq!(b["task_sites"], 0);
    assert_eq!(b["queue_sites"], 0);
    // Not computed, and said so: never a number that looks like a bound.
    assert!(b["wcet"].is_null());
    assert!(b["stack_bytes"].is_null());
    assert_eq!(b["kinds"]["wcet"], "unknown: not computed");
    assert_eq!(b["kinds"]["sites"], "static count of sites, not executions");
}

#[test]
fn a_counted_loop_s_bound_is_its_exact_trip_count() {
    for (init, cond, step, want) in [
        ("0", "i < 10", "i + 1", 10),
        ("0", "i < 10", "i + 3", 4),
        ("0", "i <= 10", "i + 5", 3),
        ("10", "i < 10", "i + 1", 0),
        ("11", "i <= 10", "i + 1", 0),
        ("10", "i > 0", "i - 1", 10),
        ("10", "i >= 0", "i - 4", 3),
        ("-5", "i < 5", "i + 2", 5),
        // The widest span an `Int` has: computed in 128 bits, so it does not wrap.
        (
            "-9223372036854775807",
            "i < 9223372036854775807",
            "i + 9223372036854775807",
            2,
        ),
    ] {
        let l = one_loop(init, cond, step);
        assert_eq!(l["bound"], want, "{init}; {cond}; {step}: {l}");
        assert!(l["reason"].is_null(), "{l}");
    }
}

#[test]
fn a_loop_without_the_counted_form_is_unknown_and_says_why() {
    for (src, why) in [
        (
            "fn main() -> Int ! {} { let mut x = 100; while x > 1 { if x % 2 == 0 { x = x / 2; } else { x = 3 * x + 1; } } x }\n",
            "the counter is not written exactly once in the body",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; let mut c = 1; while i < 10 { i = i + c; } i }\n",
            "the counter is not stepped by a literal",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; while i < 10 { i = i - 1; } i }\n",
            "the counter does not step towards its limit",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; let mut t = 0; while i < 10 { i = i + 1; if t > 3 { continue; } t = t + 1; } t }\n",
            "a `continue` to the loop skips the counter's step",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; while i != 10 { i = i + 1; } i }\n",
            "its condition is an equality, not an ordering",
        ),
        (
            "fn start() -> Int ! {} { 0 }\nfn main() -> Int ! {} { let mut i = start(); while i < 10 { i = i + 1; } i }\n",
            "the counter's start is not a literal",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; let mut n = 10; while i < n { i = i + 1; } i }\n",
            "its condition does not compare the counter with a literal",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; while i < 10 { if i > 4 { i = i + 2; } else { i = i + 1; } } i }\n",
            "the counter is not written exactly once in the body",
        ),
        (
            "fn main() -> Int ! {} { let mut i = 0; if true { i = 3; } while i < 10 { i = i + 1; } i }\n",
            "the counter is written before the loop where the compiler does not follow",
        ),
    ] {
        let (ok, v) = report(src);
        assert!(!ok, "accepted: {src}");
        assert_eq!(verdict(&v, "bounded-loops"), "unknown", "{src}");
        let l = &v["bounds"]["loops"][0];
        assert!(l["bound"].is_null(), "{l}");
        assert_eq!(l["reason"], why, "{src}");
    }
}

#[test]
fn an_unknown_loop_is_refused_with_its_reason() {
    let d = program("fn main() -> Int ! {} { let mut i = 0; while i != 10 { i = i + 1; } i }\n");
    let o = nazm(d.path(), &["check", "--profile", "realtime", "prog.nz"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(e.contains("N0510"), "{e}");
    assert!(e.contains("rule `bounded-loops`"), "{e}");
    assert!(
        e.contains("its condition is an equality, not an ordering"),
        "{e}"
    );
    assert!(
        e.contains("unknown: the compiler could not establish"),
        "{e}"
    );
}

#[test]
fn realtime_refuses_waiting_and_the_clock_by_site() {
    let (ok, v) = report(
        "fn main() -> Int ! {} { let c = chan_new(1); if chan_send(c, 4) { 1 } else { 0 } }\n",
    );
    assert!(!ok);
    assert_eq!(verdict(&v, "no-blocking"), "fail");
    assert_eq!(v["bounds"]["queue_sites"], 1);
    let (ok, v) = report("fn main(t: TimeCap) -> Int ! {} { time_now_ms() }\n");
    assert!(!ok);
    assert_eq!(verdict(&v, "no-ambient-time"), "fail");
    assert_eq!(verdict(&v, "no-blocking"), "pass");
    // `embedded` does not have the rule: a send is not refused there.
    let d = program(
        "fn main() -> Int ! {} { let c = chan_new(1); if chan_send(c, 4) { 1 } else { 0 } }\n",
    );
    let o = nazm(d.path(), &["check", "--profile", "embedded", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn recursion_and_a_function_value_leave_the_call_depth_unknown() {
    let (_, v) = report(
        "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) } }\nfn main() -> Int ! {} { f(3) }\n",
    );
    assert!(v["bounds"]["call_depth"].is_null(), "{v}");
    assert_eq!(verdict(&v, "no-recursion"), "fail");
}

#[test]
fn a_board_build_states_its_stack_bound_with_the_path() {
    let d = program(BOUNDED);
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert!(o.status.success(), "{}", err(&o));
    let text = std::fs::read_to_string(d.path().join("out/bounds.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema"], "nazm.bounds/1");
    assert_eq!(v["target"], BOARD);
    let s = &v["stack"];
    assert_eq!(s["kind"], "static bound", "{v}");
    assert_eq!(s["reserved"], 65536);
    assert_eq!(s["within"], true);
    let path: Vec<&str> = s["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(path[0], "nz_board_main");
    // The bound is the frames along the path, summed: nothing else is added.
    let frames = v["frames"].as_object().unwrap();
    let sum: u64 = path.iter().map(|f| frames[*f].as_u64().unwrap()).sum();
    assert_eq!(s["bytes"], sum);
    assert!(frames.contains_key("nz.m.prog_2Enz.main"), "{v}");
    assert!(frames.contains_key("nz.m.prog_2Enz.sum"), "{v}");
    // The deepest path: no function has a deeper chain beneath it than the one chosen.
    assert!(
        sum >= frames["nz_board_main"].as_u64().unwrap() + frames["nz.uart_int"].as_u64().unwrap()
    );
    assert_eq!(v["heap_bytes"]["bytes"], 0);
    assert!(v["wcet"]["cycles"].is_null());

    // Two builds, the same bound: the frames are the code generator's, for this build.
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "again", "prog.nz"],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        std::fs::read_to_string(d.path().join("again/bounds.json")).unwrap(),
        text
    );
}

#[test]
fn a_call_cycle_leaves_the_stack_unbounded_and_says_where() {
    let d = program(
        "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }\n\
         fn main() -> Int { down(10) }\n",
    );
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert!(o.status.success(), "{}", err(&o));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/bounds.json")).unwrap())
            .unwrap();
    assert_eq!(v["stack"]["kind"], "unknown");
    assert!(v["stack"]["bytes"].is_null());
    assert_eq!(
        v["stack"]["reason"],
        "`nz.m.prog_2Enz.down` is in a call cycle"
    );
}

#[test]
fn the_watermark_is_the_board_s() {
    let d = program("fn main() -> Int { 0 }\n");
    let o = nazm(
        d.path(),
        &["build", "--stack-watermark", "prog.nz", "-o", "prog"],
    );
    assert_eq!(o.status.code(), Some(4));
    assert!(
        err(&o).contains("`--stack-watermark` measures a board's stack"),
        "{}",
        err(&o)
    );
    let o = nazm(
        d.path(),
        &[
            "build",
            "--target",
            BOARD,
            "--stack-watermark",
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let a64 = nazm_lir::runtime::board::AARCH64;
    let plain = nazm_lir::runtime::board::emit_entry(a64, "@m", 0, false);
    let painted = nazm_lir::runtime::board::emit_entry(a64, "@m", 0, true);
    assert!(!plain.contains("nz.stack_used"), "{plain}");
    assert!(painted.contains("ldr x2, =0x5a5a5a5a5a5a5a5a"), "{painted}");
    assert!(painted.contains("call i64 @nz.stack_used()"), "{painted}");
}

/// Programs of different shapes, at `-O0` and `-O2`, each booted on QEMU's `virt` machine with
/// the stack painted: what the run touched never exceeds the bound the build stated. Needs Docker
/// and `nazm-qemu:n62`: `cargo test -p nazm-cli --test realtime -- --ignored`.
#[test]
#[ignore = "starts a container"]
fn a_run_stays_inside_the_stated_stack_bound() {
    let chain = "fn a(x: Int) -> Int { x + 1 }\nfn b(x: Int) -> Int { a(x) * 2 }\n\
                 fn c(x: Int) -> Int { b(x) + b(x + 1) }\nfn d(x: Int) -> Int { c(x) - a(x) }\n\
                 fn main() -> Int ! {} { let mut i = 0; let mut s = 0; while i < 1000 { s = s + d(i); i = i + 1; } s }\n";
    let wide = "fn w(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int, i: Int) -> Int { a + b + c + d + e + f + g + h + i }\n\
                fn main() -> Int ! {} { let mut i = 0; let mut s = 0; while i < 50 { s = s + w(i, 1, 2, 3, 4, 5, 6, 7, 8); i = i + 1; } s }\n";
    for (name, src) in [("bounded", BOUNDED), ("chain", chain), ("wide", wide)] {
        for opt in ["0", "2"] {
            let d = program(src);
            let o = nazm(
                d.path(),
                &[
                    "build",
                    "--target",
                    BOARD,
                    "--opt-level",
                    opt,
                    "--stack-watermark",
                    "--objects",
                    "out",
                    "prog.nz",
                ],
            );
            assert!(o.status.success(), "{name} -O{opt}: {}", err(&o));
            let out = d.path().join("out");
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(out.join("bounds.json")).unwrap())
                    .unwrap();
            let bound = v["stack"]["bytes"]
                .as_u64()
                .unwrap_or_else(|| panic!("{name}: {v}"));
            let o = Command::new("docker")
                .args(["run", "--rm", "--network", "none", "-v"])
                .arg(format!("{}:/w", out.display()))
                .args([
                    "-w",
                    "/w",
                    "nazm-qemu:n62",
                    "sh",
                    "-c",
                    "ld.lld -T link.ld 00-m0.o 01-runtime.o 02-entry.o -o /tmp/x.elf && \
                     timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none \
                     -semihosting -kernel /tmp/x.elf",
                ])
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            assert_eq!(o.status.code(), Some(0), "{name} -O{opt}: {stdout}");
            let used: u64 = stdout
                .lines()
                .find_map(|l| l.strip_prefix("nazm-stack: used="))
                .unwrap_or_else(|| panic!("{name}: {stdout}"))
                .trim()
                .parse()
                .unwrap();
            eprintln!("{name} -O{opt}: used {used} of a bound of {bound}");
            assert!(
                used > 0 && used <= bound,
                "{name} -O{opt}: used {used} > bound {bound}"
            );
        }
    }
}
