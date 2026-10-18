//! N106: the pool is the default runtime — M:N tasks on bounded workers unless asked otherwise.
//!
//! `docs/architecture.md` §7.107. What the programs show: with no `NAZM_SCHEDULER` a native
//! program's tasks run on the pool, on both backends, ten thousand of them on one worker per
//! processor;
//! `threads` asks for a thread per task and gives the same output, ending and report; a value
//! that names neither starts no task, with `N0404`, rather than being read as either; a program
//! that calls C runs a thread per task unless the pool is asked for, and `nazm explain-cost` says
//! which and why; and one worker runs a program's tasks in the same order every time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .env_remove("NAZM_SCHEDULER")
        .args(args)
        .output()
        .unwrap()
}

fn built(dir: &Path, name: &str, src: &str, backend: &str) {
    std::fs::write(dir.join(format!("{name}.nz")), src).unwrap();
    let o = nazm(
        dir,
        &[
            "build",
            "--backend",
            backend,
            &format!("{name}.nz"),
            "-o",
            name,
        ],
    );
    assert!(
        o.status.success(),
        "{name}/{backend}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Run `dir/name` with exactly `env` beside the reports, under a deadline.
fn run(
    dir: &Path,
    name: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> (String, String, Option<i32>) {
    let mut c = Command::new(dir.join(crate::common::exe_name(name)));
    c.current_dir(dir)
        .args(args)
        .env_remove("NAZM_SCHEDULER")
        .env_remove("NAZM_WORKERS")
        .env_remove("NAZM_TASK_STACK")
        .env("NAZM_MEMORY_REPORT", "1")
        .env("NAZM_SCHED_REPORT", "1");
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(60) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code(),
    )
}

/// The pool's workers when `NAZM_WORKERS` is unset: one per processor online, at most 64
/// (performance leadership, 2026-10-15; four before, whatever the machine).
fn default_workers() -> String {
    let n = std::thread::available_parallelism().map_or(4, |n| n.get().min(64));
    format!("nazm-pool: workers={n} ")
}

/// The report lines both models share: everything but the pool's own line.
fn shared(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|l| !l.starts_with("nazm-pool:"))
        .collect::<Vec<_>>()
        .join("\n")
}

const FANIN: &str =
    "fn worker(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, n * n); 0 }\n\
    fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
        let c = chan_new(2); let got = ints_new();\n\
        scope { let mut i = 0; while i < 50 { spawn worker(io, c, i); i = i + 1; }\n\
                let mut k = 0; while k < 50 { let r = chan_recv(c, got); k = k + 1; } }\n\
        let mut s = 0; let mut j = 0; while j < 50 { s = s + ints_get(got, j); j = j + 1; }\n\
        print(int_to_str(s)); 0 }\n";

#[test]
fn with_no_variable_a_programs_tasks_run_on_the_pool_on_both_backends() {
    let d = tempfile::tempdir().unwrap();
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "fanin", FANIN, backend);
        let default = run(d.path(), "fanin", &[], &[]);
        let threads = run(d.path(), "fanin", &[], &[("NAZM_SCHEDULER", "threads")]);
        let pool = run(d.path(), "fanin", &[], &[("NAZM_SCHEDULER", "pool")]);
        // The sum of the first fifty squares, worked out: 49 · 50 · 99 / 6 = 40425, then `main`'s 0.
        assert_eq!(default.0, "404250\n", "{backend}: {}", default.1);
        assert_eq!(default.2, Some(0), "{backend}: {}", default.1);
        assert!(
            default.1.contains(&default_workers()),
            "{backend}: the default is not the pool: {}",
            default.1
        );
        assert!(
            !threads.1.contains("nazm-pool:"),
            "{backend}: {}",
            threads.1
        );
        for other in [&threads, &pool] {
            assert_eq!((&other.0, other.2), (&default.0, default.2), "{backend}");
            assert_eq!(shared(&other.1), shared(&default.1), "{backend}: reports");
        }
    }
}

#[test]
fn ten_thousand_tasks_run_on_one_worker_per_processor_by_default() {
    let src = "fn worker(io: IoCap, jobs: Chan, results: Chan) -> Int ! { io } {\n\
        let got = ints_new();\n\
        if chan_recv(jobs, got) { chan_send(results, ints_get(got, 0) + 1); }\n\
        0 }\n\
    fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
        let n = str_len(arg(0));\n\
        let jobs = chan_new(1); let results = chan_new(1); let sum = ints_new(); ints_push(sum, 0);\n\
        scope {\n\
            let mut i = 0; while i < n { spawn worker(io, jobs, results); i = i + 1; }\n\
            let mut k = 0; let got = ints_new();\n\
            while k < n { chan_send(jobs, k); let r = chan_recv(results, got); k = k + 1; }\n\
            let mut j = 0; let mut s = 0; while j < n { s = s + ints_get(got, j); j = j + 1; }\n\
            ints_set(sum, 0, s);\n\
        }\n\
        print(int_to_str(ints_get(sum, 0))); 0 }\n";
    let d = tempfile::tempdir().unwrap();
    built(d.path(), "many", src, "llvm");
    let n = "x".repeat(10_000);
    let (out, err, code) = run(d.path(), "many", &[&n], &[]);
    assert_eq!(code, Some(0), "{err}");
    // 1 + 2 + … + 10,000 = 50005000, then `main`'s 0.
    assert_eq!(out, "500050000\n");
    // Ten thousand logical tasks alive at once on one OS thread per processor, where the thread
    // model would have made ten thousand.
    assert!(
        err.contains(&format!("{}peak=10000", default_workers())),
        "{err}"
    );
    assert!(err.contains("spawned=10000 joined=10000"), "{err}");
}

#[test]
fn a_scheduler_the_environment_does_not_name_starts_no_task() {
    let d = tempfile::tempdir().unwrap();
    built(d.path(), "fanin", FANIN, "llvm");
    for bad in ["bogus", "Pool", "thread", ""] {
        let (out, err, code) = run(d.path(), "fanin", &[], &[("NAZM_SCHEDULER", bad)]);
        assert_eq!(code, Some(2), "`{bad}`: {err}");
        assert!(err.contains("error[N0404]"), "`{bad}`: {err}");
        assert!(out.is_empty(), "`{bad}` printed {out}");
    }
}

#[test]
fn a_program_that_calls_c_runs_a_thread_per_task_unless_the_pool_is_asked_for() {
    let src = "extern \"C\" fn labs(a: Int) -> Int = \"labs\";\n\
        fn worker(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, n); 0 }\n\
        fn main(io: IoCap, t: SpawnCap, f: ForeignCap) -> Int {\n\
            let c = chan_new(4); let got = ints_new();\n\
            scope { let mut i = 0; while i < 4 { spawn worker(io, c, i); i = i + 1; }\n\
                    let mut k = 0; while k < 4 { let r = chan_recv(c, got); k = k + 1; } }\n\
            print(int_to_str(labs(0 - 6) + ints_len(got))); 0 }\n";
    let d = tempfile::tempdir().unwrap();
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "withc", src, backend);
        let default = run(d.path(), "withc", &[], &[]);
        // |-6| + 4 tasks' values = 10, then `main`'s 0.
        assert_eq!(
            (default.0.as_str(), default.2),
            ("100\n", Some(0)),
            "{}",
            default.1
        );
        assert!(
            !default.1.contains("nazm-pool:"),
            "{backend}: {}",
            default.1
        );
        let pool = run(d.path(), "withc", &[], &[("NAZM_SCHEDULER", "pool")]);
        assert_eq!((&pool.0, pool.2), (&default.0, default.2));
        assert!(pool.1.contains(&default_workers()), "{backend}: {}", pool.1);
    }
    // And the cost report says which scheduler, and why.
    let cost = |file: &str| -> serde_json::Value {
        let o = nazm(d.path(), &["explain-cost", "--json", file]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    };
    std::fs::write(d.path().join("fanin.nz"), FANIN).unwrap();
    std::fs::write(d.path().join("plain.nz"), "fn main() -> Int { 1 }\n").unwrap();
    let c = cost("withc.nz");
    assert_eq!(c["schema"], "nazm.cost/2");
    assert_eq!(c["scheduler"]["default"], "threads");
    assert!(c["scheduler"]["why"].as_str().unwrap().contains("calls C"));
    assert_eq!(cost("fanin.nz")["scheduler"]["default"], "pool");
    assert!(cost("plain.nz")["scheduler"].is_null());
}

#[test]
fn one_worker_runs_a_programs_tasks_in_the_same_order_every_time() {
    // Each task appends its number to a shared channel the moment it runs; on one worker,
    // first in, first out, they run in the order they were spawned, every time — where several
    // workers may interleave them differently from run to run.
    let src = "fn worker(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, n); 0 }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
            let c = chan_new(64); let got = ints_new();\n\
            scope { let mut i = 0; while i < 32 { spawn worker(io, c, i); i = i + 1; } }\n\
            let mut k = 0; while k < 32 { let r = chan_recv(c, got); k = k + 1; }\n\
            let mut j = 0; while j < 32 { print(int_to_str(ints_get(got, j))); print(\" \"); j = j + 1; }\n\
            0 }\n";
    let d = tempfile::tempdir().unwrap();
    built(d.path(), "order", src, "llvm");
    let want: String = (0..32).map(|i| format!("{i} ")).collect::<String>() + "0\n";
    for _ in 0..5 {
        let (out, err, code) = run(d.path(), "order", &[], &[("NAZM_WORKERS", "1")]);
        assert_eq!(code, Some(0), "{err}");
        assert!(err.contains("nazm-pool: workers=1"), "{err}");
        assert_eq!(out, want);
    }
}

/// What the default costs against threads (N106, `docs/performance.md`): spawn and join, a
/// channel round trip, a blocked receive woken, and resident memory per live task — each program
/// built once at `-O2`, five runs, median. Run with `--ignored --nocapture`; it measures, it does
/// not judge.
#[test]
#[ignore = "a measurement, printed for performance.md"]
fn what_the_default_scheduler_costs() {
    let d = tempfile::tempdir().unwrap();
    let programs = [
        (
            "spawns",
            "fn nothing(io: IoCap) -> Int ! { io } { 0 }\n\
             fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                 let mut i = 0; while i < 50000 { scope { spawn nothing(io); } i = i + 1; } 0 }\n",
            50_000.0,
        ),
        (
            "pingpong",
            "fn echo(io: IoCap, a: Chan, b: Chan) -> Int ! { io } {\n\
                 let got = ints_new(); while chan_recv(a, got) { chan_send(b, ints_pop(got)); } 0 }\n\
             fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                 let a = chan_new(1); let b = chan_new(1); let got = ints_new();\n\
                 scope { spawn echo(io, a, b);\n\
                     let mut i = 0; while i < 50000 { chan_send(a, i); let r = chan_recv(b, got); ints_pop(got); i = i + 1; }\n\
                     chan_close(a); }\n\
                 0 }\n",
            50_000.0,
        ),
        (
            "wake",
            "fn waiter(io: IoCap, a: Chan, done: Chan) -> Int ! { io } {\n\
                 let got = ints_new(); let r = chan_recv(a, got); chan_send(done, 1); 0 }\n\
             fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                 let mut i = 0; let got = ints_new();\n\
                 while i < 20000 { let a = chan_new(1); let done = chan_new(1);\n\
                     scope { spawn waiter(io, a, done); chan_send(a, i); let r = chan_recv(done, got); ints_pop(got); }\n\
                     i = i + 1; }\n\
                 0 }\n",
            20_000.0,
        ),
    ];
    let time = |exe: &Path, env: &[(&str, &str)], args: &[&str]| -> (f64, f64) {
        let mut runs: Vec<(f64, f64)> = (0..5)
            .map(|_| {
                let mut c = Command::new("/usr/bin/time");
                c.arg(if cfg!(target_os = "macos") {
                    "-l"
                } else {
                    "-v"
                })
                .arg(exe)
                .args(args)
                .env_remove("NAZM_SCHEDULER");
                for (k, v) in env {
                    c.env(k, v);
                }
                let t = std::time::Instant::now();
                let o = c.output().unwrap();
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
                let err = String::from_utf8_lossy(&o.stderr);
                // macOS reports bytes, GNU time kilobytes.
                let rss = err
                    .lines()
                    .find_map(|l| {
                        let l = l.trim();
                        if let Some(n) = l.strip_suffix("maximum resident set size") {
                            n.trim().parse::<f64>().ok()
                        } else {
                            l.strip_prefix("Maximum resident set size (kbytes):")
                                .and_then(|n| n.trim().parse::<f64>().ok())
                                .map(|k| k * 1024.0)
                        }
                    })
                    .unwrap_or(f64::NAN);
                (ms, rss)
            })
            .collect();
        runs.sort_by(|a, b| a.0.total_cmp(&b.0));
        runs[2]
    };
    let build = |name: &str, src: &str| {
        std::fs::write(d.path().join(format!("{name}.nz")), src).unwrap();
        let o = nazm(
            d.path(),
            &[
                "build",
                "--opt-level",
                "2",
                &format!("{name}.nz"),
                "-o",
                name,
            ],
        );
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        d.path().join(name)
    };
    let models: [(&str, &[(&str, &str)]); 2] = [
        ("pool (default)", &[]),
        ("threads", &[("NAZM_SCHEDULER", "threads")]),
    ];
    for (name, src, n) in programs {
        let exe = build(name, src);
        for (model, env) in models {
            let (ms, _) = time(&exe, env, &[]);
            println!(
                "{name} {model}: {:.2} µs each ({ms:.0} ms for {n})",
                ms * 1000.0 / n
            );
        }
    }
    // Resident memory with N tasks alive at once, each parked on a receive: the pool's peak is
    // logical tasks; its workers are the OS threads.
    let many = build(
        "many",
        "fn worker(io: IoCap, jobs: Chan, results: Chan) -> Int ! { io } {\n\
             let got = ints_new(); if chan_recv(jobs, got) { chan_send(results, ints_get(got, 0)); } 0 }\n\
         fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
             let n = str_len(arg(0)); let jobs = chan_new(1); let results = chan_new(1);\n\
             scope { let mut i = 0; while i < n { spawn worker(io, jobs, results); i = i + 1; }\n\
                 let mut k = 0; let got = ints_new(); while k < n { chan_send(jobs, k); let r = chan_recv(results, got); k = k + 1; } }\n\
             0 }\n",
    );
    let (_, base) = time(&many, &[], &["x"]);
    for tasks in [1_000_u32, 10_000] {
        let arg = "x".repeat(tasks as usize);
        for (model, env) in models {
            if model == "threads" && tasks > 1_000 {
                println!("many {model}: {tasks} tasks not run (one OS thread each)");
                continue;
            }
            let (ms, rss) = time(&many, env, &[&arg]);
            println!(
                "many {model}: {tasks} tasks alive, {:.1} MB resident, {:.1} KB per task over one, {ms:.0} ms",
                rss / 1e6,
                (rss - base) / 1e3 / f64::from(tasks)
            );
        }
    }
}
