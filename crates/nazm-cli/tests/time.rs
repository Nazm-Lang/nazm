//! Gate 2: time.
//!
//! `docs/general-purpose.md` §11 and the spec's *Time*. What only a whole run shows: that the
//! monotonic clock in nanoseconds advances by at least what a sleep waited, that the wall clock is
//! after 2023, that `@std/time`'s `Duration`, `Instant` and `WallTime` compose, and that a sleep on
//! the task pool parks its task — on **one** worker, a task that computes finishes before one that
//! sleeps — in the interpreter and in both native backends at both optimisation levels; and that
//! every clock and the sleep need a `TimeCap` and are what `critical` refuses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod differential;

use differential::{agrees, codes, nazm};

#[test]
fn the_clocks_and_a_sleep_agree_everywhere() {
    let src = "use \"@std/time\";\n\
        fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, \"\\n\")) }\n\
        fn main(t: TimeCap, out: OutCap) -> Int {\n\
            let start = time_now(t);\n\
            time_sleep(t, time_duration_ms(25));\n\
            let d = time_elapsed(t, start);\n\
            line(out, if time_duration_to_ms(d) >= 25 && time_duration_to_ms(d) < 5000 { \"slept at least 25 ms\" } else { int_to_str(d.nanos) });\n\
            let deadline = time_after(t, time_duration_ms(10));\n\
            line(out, if time_reached(t, deadline) { \"too soon\" } else { \"not yet\" });\n\
            time_sleep_ms(15);\n\
            line(out, if time_reached(t, deadline) { \"reached\" } else { \"not reached\" });\n\
            line(out, if time_wall(t).unix_nanos > 1700000000000000000 { \"after 2023\" } else { \"before\" });\n\
            line(out, int_to_str(time_duration_to_ms(time_duration_add(time_duration_ms(2), time_duration_ns(3000000)))));\n\
            line(out, int_to_str(time_sleep_ms(0 - 5)));\n\
            0\n\
        }\n";
    agrees(
        src,
        "slept at least 25 ms\nnot yet\nreached\nafter 2023\n5\n0\n0",
    );
}

const POOL: &str = "fn napper(t: TimeCap, done: Chan) -> Int { time_sleep_ms(300); chan_send(done, 1); 0 }\n\
fn busy(done: Chan) -> Int { let mut i = 0; let mut s = 0; while i < 100000 { s = s + i % 3; i = i + 1; } chan_send(done, 2); 0 }\n\
fn main(t: TimeCap, sp: SpawnCap, out: OutCap) -> Int {\n\
    let done = chan_new(4);\n\
    let got = ints_new();\n\
    scope {\n\
        spawn napper(t, done);\n\
        spawn busy(done);\n\
        chan_recv(done, got);\n\
        chan_recv(done, got);\n\
    }\n\
    print(str_concat(int_to_str(ints_get(got, 0)), int_to_str(ints_get(got, 1))));\n\
    0\n\
}\n";

#[test]
fn a_sleeping_task_does_not_hold_its_worker() {
    // The sleeper is started first; on one worker the computing task can only finish first if
    // the sleep parked.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), POOL).unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
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
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path()).env("NAZM_WORKERS", "1");
        let r = common::run_bounded(&mut c, backend);
        assert_eq!(r.code, Some(0), "{backend}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), "210", "{backend}");
    }
}

#[test]
fn every_clock_and_the_sleep_need_a_time_cap() {
    for call in ["time_now_ns()", "time_wall_ns()", "time_sleep_ms(1)"] {
        let src = format!("fn main() -> Int {{ {call} }}\n");
        assert!(
            codes(&src).contains(&"N0369".to_owned()),
            "{call}: {:?}",
            codes(&src)
        );
        let held = format!("fn main(t: TimeCap) -> Int {{ {call} }}\n");
        assert!(codes(&held).is_empty(), "{call}: {:?}", codes(&held));
    }
}

#[test]
fn critical_refuses_every_clock_and_the_sleep() {
    for call in ["time_now_ns()", "time_wall_ns()", "time_sleep_ms(1)"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("prog.nz"),
            format!("fn main(t: TimeCap) -> Int ! {{}} {{ {call} }}\n"),
        )
        .unwrap();
        let r = nazm(d.path(), &["check", "--profile", "critical", "prog.nz"]);
        assert_eq!(r.code, Some(1), "{call}");
        assert!(
            r.stderr.contains("rule `no-ambient-time`"),
            "{call}: {}",
            r.stderr
        );
    }
}
