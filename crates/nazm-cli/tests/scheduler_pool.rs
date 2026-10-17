//! N83: the task pool — M:N tasks on bounded workers, opt-in by `NAZM_SCHEDULER=pool` until N106
//! made it the default (`scheduler_default.rs`); here the thread model is asked for by name.
//!
//! `docs/architecture.md` §7.84. What the programs show: every concurrency shape gives the same
//! output, ending and memory report on the pool as on one thread per task, on both backends;
//! ten thousand tasks run, all alive at once, on two workers; a task's failure and stack limit are
//! its own on a shared worker; a task polling with a select that does not wait lets a sibling on
//! the same worker run; a deadline fires for a parked task.

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

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Build `src` with `backend`, into `dir/name`.
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
    assert!(o.status.success(), "{name}/{backend}: {}", text(&o));
}

/// Run `dir/name` with `env`, the memory and schedule reports on.
fn run(
    dir: &Path,
    name: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> (String, String, Option<i32>) {
    let mut c = Command::new(dir.join(name));
    c.current_dir(dir)
        .args(args)
        .env_remove("NAZM_SCHEDULER")
        .env("NAZM_MEMORY_REPORT", "1")
        .env("NAZM_SCHED_REPORT", "1");
    for (k, v) in env {
        c.env(k, v);
    }
    // A deadline: a lost wake-up, a select that never yields or a deadline that never fires
    // blocks for ever, and must fail the test rather than hang it.
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

/// The report lines both models share: everything but the pool's own line.
fn shared(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|l| !l.starts_with("nazm-pool:"))
        .collect::<Vec<_>>()
        .join("\n")
}

const SHAPES: &[(&str, &str)] = &[
    (
        "fanin",
        "fn worker(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, n * n); 0 }\n\
         fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
             let c = chan_new(2); let got = ints_new();\n\
             scope { let mut i = 0; while i < 50 { spawn worker(io, c, i); i = i + 1; }\n\
                     let mut k = 0; while k < 50 { let r = chan_recv(c, got); k = k + 1; } }\n\
             let mut s = 0; let mut j = 0; while j < 50 { s = s + ints_get(got, j); j = j + 1; }\n\
             print(int_to_str(s)); 0 }\n",
    ),
    (
        "pipeline",
        "fn stage(io: IoCap, inp: Chan[Str], out: Chan[Str]) -> Int ! { io } {\n\
             let got = vec_new[Str]();\n\
             while chan_recv_of(inp, got) { chan_send_of(out, str_concat(vec_pop(got), \"!\")); }\n\
             chan_close_of(out); 0 }\n\
         fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
             let a = chan_new_of[Str](1); let b = chan_new_of[Str](1); let c = chan_new_of[Str](1);\n\
             let got = vec_new[Str]();\n\
             scope { spawn stage(io, a, b); spawn stage(io, b, c);\n\
                     let mut i = 0; while i < 20 { chan_send_of(a, int_to_str(i)); let r = chan_recv_of(c, got); i = i + 1; }\n\
                     chan_close_of(a); let e = chan_recv_of(c, got); }\n\
             print(vec_get(got, 19)); print(int_to_str(vec_len(got))); 0 }\n",
    ),
    (
        "selecting",
        "fn producer(io: IoCap, c: Chan[Int], n: Int) -> Int ! { io } { chan_send_of(c, n); 0 }\n\
         fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
             let x = chan_new_of[Int](1); let y = chan_new_of[Int](1);\n\
             let cs = vec_new[Chan[Int]](); vec_push(cs, x); vec_push(cs, y);\n\
             let out = vec_new[Int](); let mut sum = 0; let mut n = 0;\n\
             scope { spawn producer(io, x, 3); spawn producer(io, y, 4);\n\
                     while n < 2 { let i = chan_select_of(cs, out); sum = sum + vec_pop(out); n = n + 1; } }\n\
             print(int_to_str(sum)); 0 }\n",
    ),
    (
        "nested",
        "fn leaf(io: IoCap, c: Chan) -> Int ! { io } { chan_send(c, 1); 0 }\n\
         fn branch(io: IoCap, t: SpawnCap, c: Chan) -> Int ! { io, spawn } { scope { spawn leaf(io, c); spawn leaf(io, c); } 0 }\n\
         fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
             let c = chan_new(16); let got = ints_new();\n\
             scope { spawn branch(io, t, c); spawn branch(io, t, c); }\n\
             let mut k = 0; while k < 4 { let r = chan_recv(c, got); k = k + 1; }\n\
             print(int_to_str(ints_len(got))); 0 }\n",
    ),
];

#[test]
fn every_concurrency_shape_behaves_the_same_on_the_pool_on_both_backends() {
    let d = tempfile::tempdir().unwrap();
    for (name, src) in SHAPES {
        for backend in ["llvm", "cranelift"] {
            built(d.path(), name, src, backend);
            let threads = run(d.path(), name, &[], &[("NAZM_SCHEDULER", "threads")]);
            let pool = run(
                d.path(),
                name,
                &[],
                &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "2")],
            );
            assert_eq!(threads.2, Some(0), "{name}/{backend}: {}", threads.1);
            assert_eq!(threads.0, pool.0, "{name}/{backend}: output");
            assert_eq!(threads.2, pool.2, "{name}/{backend}: ending");
            assert_eq!(
                shared(&threads.1),
                shared(&pool.1),
                "{name}/{backend}: reports"
            );
            assert!(
                pool.1.contains("nazm-pool: workers=2"),
                "{name}/{backend}: {}",
                pool.1
            );
            assert!(
                !threads.1.contains("nazm-pool:"),
                "{name}/{backend}: no pool line on threads"
            );
        }
    }
}

const MANY: &str = "fn worker(io: IoCap, jobs: Chan, results: Chan) -> Int ! { io } {\n\
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

#[test]
fn ten_thousand_tasks_all_alive_at_once_run_on_two_workers() {
    let d = tempfile::tempdir().unwrap();
    built(d.path(), "many", MANY, "llvm");
    let n = "x".repeat(10_000);
    let (out, err, code) = run(
        d.path(),
        "many",
        &[&n],
        &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "2")],
    );
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, "500050000\n");
    // Every task was spawned before main sent the first job, and each waits for one: all
    // ten thousand were alive at once, on two worker threads.
    assert!(err.contains("nazm-pool: workers=2 peak=10000"), "{err}");
    assert!(err.contains("spawned=10000 joined=10000"), "{err}");
}

/// Performance leadership 6.2: a worker's tasks share its run stack, and a parked task's frames are
/// copied out and back. Two hundred tasks each park at the bottom of a recursion of their own depth,
/// with a live array in every frame, and are resumed fifty times in turn: every frame comes back as
/// it was on one worker, on three, under both backends.
#[test]
fn tasks_parked_deep_in_recursion_keep_their_frames_on_a_shared_run_stack() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn down(n: Int, k: Int, inp: Chan, out: Chan) -> Int {\n\
               let mut pad = [0; 16]; pad[n % 16] = n + k;\n\
               if n == 0 { let buf = ints_new(); let mut got = 0;\n\
               while chan_recv(inp, buf) { let v = ints_pop(buf); got += v; chan_send(out, v + k); }\n\
               got + pad[0] } else { let r = down(n - 1, k, inp, out); r + pad[n % 16] - n - k } }\n\
               fn worker(k: Int, depth: Int, inp: Chan, out: Chan, res: Chan) -> Int { chan_send(res, down(depth, k, inp, out)); 0 }\n\
               fn main(io: IoCap, tasks: SpawnCap) {\n\
               let n = 200; let rounds = 50; let mut ins: Vec[Chan] = []; let res = chan_new(1000); let out = chan_new(1);\n\
               let mut total = 0; let buf = ints_new();\n\
               scope { let mut i = 0; while i < n { let c = chan_new(1); ins.push(c); spawn worker(i, 10 + i % 37, c, out, res); i += 1; }\n\
               let mut r = 0; while r < rounds { let mut j = 0; while j < n { chan_send(ins[j], r); if chan_recv(out, buf) { total += ints_pop(buf); } j += 1; } r += 1; }\n\
               let mut j = 0; while j < n { chan_close(ins[j]); j += 1; } }\n\
               let mut sum = 0; let mut j = 0; while j < n { if chan_recv(res, buf) { sum += ints_pop(buf); } j += 1; }\n\
               println(total); println(sum) }\n";
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "frames", src, backend);
        for workers in ["1", "3"] {
            let (out, err, code) = run(
                d.path(),
                "frames",
                &[],
                &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", workers)],
            );
            assert_eq!(code, Some(0), "{backend} {workers}: {err}");
            assert!(
                out.starts_with("1240000\n264900\n"),
                "{backend} {workers}: {out}{err}"
            );
        }
    }
}

/// Performance leadership 6.3: tasks a task spawns start on its worker and idle workers steal the
/// unstarted ones; a worker spins briefly, then sleeps until signalled; a scope's join waits without
/// a lock when its tasks are done. Five hundred tasks each spawn two that park on a full channel,
/// beside five thousand parked on a gate: every one runs and is joined, on one worker and on four,
/// under both backends.
#[test]
fn tasks_a_task_spawns_are_stolen_by_idle_workers_and_all_run() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn leaf(c: Chan, v: Int) -> Int { chan_send(c, v); 0 }\n\
               fn branch(t: SpawnCap, c: Chan, v: Int) -> Int ! { spawn } { scope { spawn leaf(c, v); spawn leaf(c, v + 1); } 0 }\n\
               fn waiter(gate: Chan, started: Atomic) -> Int { atomic_add(started, 1); let buf = ints_new(); if chan_recv(gate, buf) { 1 } else { 0 } }\n\
               fn main(io: IoCap, tasks: SpawnCap, clock: TimeCap) {\n\
               let c = chan_new(64); let gate = chan_new(1); let started = atomic_new(0); let mut sum = 0;\n\
               scope { let mut i = 0; while i < 5000 { spawn waiter(gate, started); i += 1; }\n\
               let mut j = 0; while j < 500 { spawn branch(tasks, c, j); j += 1; }\n\
               let buf = ints_new(); let mut k = 0; while k < 1000 { if chan_recv(c, buf) { sum += ints_pop(buf); } k += 1; }\n\
               while atomic_load(started) < 5000 { time_sleep_ms(1); }\n\
               chan_close(gate); }\n\
               println(sum); println(atomic_load(started)) }\n";
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "steal", src, backend);
        for workers in ["1", "4"] {
            let (out, err, code) = run(
                d.path(),
                "steal",
                &[],
                &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", workers)],
            );
            assert_eq!(code, Some(0), "{backend} {workers}: {err}");
            assert!(
                out.starts_with("250000\n5000\n"),
                "{backend} {workers}: {out}{err}"
            );
            assert!(
                err.contains("spawned=6500 joined=6500"),
                "{backend} {workers}: {err}"
            );
        }
    }
}

/// Performance leadership 6.3: preemption. Two tasks loop until the last of four short ones, queued
/// behind them on the same two workers, has run. Without preemption the loops hold both workers and
/// the short tasks never start; with it the monitor interrupts a loop that has held its worker for a
/// time slice while others wait, and it yields where it was, so all four run and the loops end —
/// with no instruction added to either loop. On macOS, where generated code has its own section —
/// LLVM's (`Code::MachO`) and Cranelift's (its object's code section renamed); Nazm's own code
/// generator's is `nazm_backend.rs`'s; Linux preempts nothing yet.
#[cfg(target_os = "macos")]
#[test]
fn a_long_loop_yields_its_worker_to_tasks_waiting_behind_it() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn spin(stop: Atomic) -> Int { let mut x = 0; while atomic_load(stop) == 0 { x = (x + 1) % 1000; } x }\n\
               fn short(stop: Atomic, done: Atomic) -> Int { atomic_add(done, 1); if atomic_load(done) == 4 { atomic_store(stop, 1); } 0 }\n\
               fn main(io: IoCap, tasks: SpawnCap) {\n\
               let stop = atomic_new(0); let done = atomic_new(0);\n\
               scope { spawn spin(stop); spawn spin(stop); spawn short(stop, done); spawn short(stop, done); spawn short(stop, done); spawn short(stop, done); }\n\
               println(atomic_load(done)) }\n";
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "starve", src, backend);
        let (out, err, code) = run(
            d.path(),
            "starve",
            &[],
            &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "2")],
        );
        assert_eq!(code, Some(0), "{backend}: {err}");
        assert!(out.starts_with("4\n"), "{backend}: {out}{err}");
    }
}

#[test]
fn a_tasks_failure_and_stack_limit_are_its_own_on_a_shared_worker() {
    let d = tempfile::tempdir().unwrap();
    // One worker: both tasks share it. The first fails while the second is parked, and the
    // second still finishes; the scope reports the first's failure, as on threads.
    let src = "fn bad(io: IoCap, c: Chan, z: Int) -> Int ! { io } { let r = chan_recv(c, ints_new()); 10 / z }\n\
               fn good(io: IoCap, c: Chan, done: Chan) -> Int ! { io } { chan_send(c, 1); chan_send(done, 7); 0 }\n\
               fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                   let c = chan_new(1); let done = chan_new(1); let got = ints_new();\n\
                   scope { spawn bad(io, c, str_len(\"\")); spawn good(io, c, done); let r = chan_recv(done, got); }\n\
                   print(int_to_str(ints_get(got, 0))); 0 }\n";
    built(d.path(), "fail", src, "llvm");
    let threads = run(d.path(), "fail", &[], &[("NAZM_SCHEDULER", "threads")]);
    let pool = run(
        d.path(),
        "fail",
        &[],
        &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "1")],
    );
    assert_eq!(threads.2, Some(2), "{}", threads.1);
    assert_eq!((&threads.0, threads.2), (&pool.0, pool.2));
    assert!(
        pool.1.contains("division by zero") || pool.1.contains("N04"),
        "{}",
        pool.1
    );
    // Recursion past a task's stack is the stack check's refusal, not a crash; a larger stack
    // runs it as a thread does. `main` receives after the scope, so a task that fails before
    // sending ends the program through its scope rather than leaving `main` waiting.
    let deep = "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }\n\
                fn work(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, down(n)); 0 }\n\
                fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                    let c = chan_new(1); let got = ints_new();\n\
                    scope { spawn work(io, c, str_len(arg(0)) * 1000); }\n\
                    let r = chan_recv(c, got);\n\
                    print(int_to_str(ints_get(got, 0))); 0 }\n";
    built(d.path(), "deep", deep, "llvm");
    let n = "x".repeat(20);
    let small = run(d.path(), "deep", &[&n], &[("NAZM_SCHEDULER", "pool")]);
    assert_eq!(small.2, Some(2), "{}", small.1);
    assert!(small.1.contains("N0408"), "{}", small.1);
    let threads = run(d.path(), "deep", &[&n], &[("NAZM_SCHEDULER", "threads")]);
    let large = run(
        d.path(),
        "deep",
        &[&n],
        &[("NAZM_SCHEDULER", "pool"), ("NAZM_TASK_STACK", "16777216")],
    );
    assert_eq!(threads.2, Some(0), "{}", threads.1);
    assert_eq!((&threads.0, threads.2), (&large.0, large.2));
}

/// Q1-A1-01: a task whose own frame is larger than its stack — five 64 KiB arrays on the default
/// 256 KiB — fails with `N0408` at the function's name under both backends and both levels, rather
/// than faulting on the guard page as its prologue lays the frame out; so does a recursive function
/// whose 8 KiB frames two hundred deep do not fit a task's stack. A stack that holds them runs both.
#[test]
fn a_task_frame_larger_than_its_stack_fails_with_n0408_rather_than_faulting() {
    let d = tempfile::tempdir().unwrap();
    let big = "fn worker(c: Chan[Int]) -> Int ! {spawn} {\n\
                   let mut a = [0; 8192]; let mut b = [0; 8192]; let mut x = [0; 8192];\n\
                   let mut y = [0; 8192]; let mut z = [0; 8192];\n\
                   a[1] = 1; b[2] = 2; x[3] = 3; y[4] = 4; z[5] = 5;\n\
                   chan_send_of(c, a[1] + b[2] + x[3] + y[4] + z[5]); 0 }\n\
               fn main(out: OutCap, s: SpawnCap) -> Int ! {io, spawn} {\n\
                   let c = chan_new_of[Int](1);\n\
                   scope { spawn worker(c); }\n\
                   let got = vec_new[Int](); chan_recv_of(c, got);\n\
                   print(int_to_str(vec_get(got, 0))); 0 }\n";
    let deep = "fn down(n: Int) -> Int { let mut pad = [0; 1024]; pad[n % 1024] = n;\n\
                    if n == 0 { 0 } else { pad[n % 1024] - n + 1 + down(n - 1) } }\n\
                fn work(io: IoCap, c: Chan, n: Int) -> Int ! { io } { chan_send(c, down(n)); 0 }\n\
                fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
                    let c = chan_new(1); let got = ints_new();\n\
                    scope { spawn work(io, c, str_len(arg(0)) * 10); }\n\
                    let r = chan_recv(c, got);\n\
                    print(int_to_str(ints_get(got, 0))); 0 }\n";
    for backend in ["llvm", "cranelift"] {
        for level in ["0", "2"] {
            let name = format!("big-{backend}-{level}");
            std::fs::write(d.path().join("big.nz"), big).unwrap();
            let o = nazm(
                d.path(),
                &[
                    "build",
                    "--backend",
                    backend,
                    "--opt-level",
                    level,
                    "big.nz",
                    "-o",
                    &name,
                ],
            );
            assert!(o.status.success(), "{name}: {}", text(&o));
            let small = run(d.path(), &name, &[], &[]);
            assert_eq!(small.2, Some(2), "{name}: {}", small.1);
            assert!(small.1.contains("error[N0408]"), "{name}: {}", small.1);
            assert!(small.1.contains("big.nz:1:4"), "{name}: {}", small.1);
            let large = run(d.path(), &name, &[], &[("NAZM_TASK_STACK", "1048576")]);
            assert_eq!(
                (large.0.as_str(), large.2),
                ("150\n", Some(0)),
                "{name}: {}",
                large.1
            );
        }
        let name = format!("deep-{backend}");
        built(d.path(), &name, deep, backend);
        let n = "x".repeat(20);
        let small = run(d.path(), &name, &[&n], &[]);
        assert_eq!(small.2, Some(2), "{name}: {}", small.1);
        assert!(small.1.contains("error[N0408]"), "{name}: {}", small.1);
        let threads = run(d.path(), &name, &[&n], &[("NAZM_SCHEDULER", "threads")]);
        assert_eq!(
            (threads.0.as_str(), threads.2),
            ("2000\n", Some(0)),
            "{name}: {}",
            threads.1
        );
    }
}

#[test]
fn a_select_that_does_not_wait_lets_a_sibling_on_its_worker_run() {
    let d = tempfile::tempdir().unwrap();
    // The sender waits until the poller is about to poll (N108: without `go`, which task ran
    // first was not fixed, and a sender that ran first left nothing to wait for — the mutant that
    // takes the yield away survived one run in three). Whichever starts, the sender is parked when
    // the polling begins, and only a select that gives its worker back lets it run.
    let src = "fn sender(io: IoCap, go: Chan[Int], c: Chan[Int]) -> Int ! { io } {\n\
                   let g = vec_new[Int](); let r = chan_recv_of(go, g); chan_send_of(c, 42); 0 }\n\
               fn poller(io: IoCap, tm: TimeCap, go: Chan[Int], c: Chan[Int], done: Chan[Int]) -> Int ! { io } {\n\
                   let cs = vec_new[Chan[Int]](); vec_push(cs, c); let out = vec_new[Int]();\n\
                   chan_send_of(go, 1);\n\
                   let mut spins = 0;\n\
                   while chan_select_until(cs, out, 0) == 0 - 2 { spins = spins + 1; }\n\
                   chan_send_of(done, vec_pop(out)); 0 }\n\
               fn main(io: IoCap, t: SpawnCap, tm: TimeCap) -> Int ! { io, spawn } {\n\
                   let go = chan_new_of[Int](1); let c = chan_new_of[Int](1); let done = chan_new_of[Int](1); let got = vec_new[Int]();\n\
                   scope { spawn poller(io, tm, go, c, done); spawn sender(io, go, c); let r = chan_recv_of(done, got); }\n\
                   print(int_to_str(vec_get(got, 0))); 0 }\n";
    built(d.path(), "poll", src, "llvm");
    // Five runs, every one required: a worker that is never given back is seen in most runs, not
    // in all — the order the two tasks first run in is the scheduler's (N108 measured one run in
    // five finishing under that mutant), so one run is not evidence and five are.
    for _ in 0..5 {
        let (out, err, code) = run(
            d.path(),
            "poll",
            &[],
            &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "1")],
        );
        assert_eq!(code, Some(0), "{err}");
        assert_eq!(out, "420\n");
    }
}

/// Q1-M-11: a deadline too far to count in nanoseconds is "never" — a parked task waits without
/// using a CPU. Before the fix the wrapped deadline was already past: the task was woken, found
/// its own (second-resolution) deadline not yet reached, parked again, and its worker spun. The
/// answer was right; the core was not idle — which is what this measures. Main waits four seconds
/// on the main thread; three seconds in, the process has used next to no CPU time (`ps`). The Q1
/// campaign showed the earlier, output-only versions of this test could not see the spin.
/// The CPU time `ps` reports for a running process, in seconds: `00:00:01` (procps) or `0:01.49`
/// (BSD) — seconds, minutes, hours from the right.
fn cpu_seconds(pid: u32) -> (f64, String) {
    let cpu = Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&cpu.stdout).trim().to_owned();
    let seconds = text
        .rsplit(':')
        .zip([1.0, 60.0, 3600.0])
        .map(|(part, unit)| part.trim().parse::<f64>().unwrap_or(f64::MAX) * unit)
        .sum();
    (seconds, text)
}

/// Q1-M-12: a pool task waiting on an ordinary deadline — four seconds — is parked until the
/// pool's monotonic clock reaches it, not woken at once and parked again in a loop. Select and
/// sleep look again on the monotonic clock either way, so a pool that compared the deadline with
/// another clock would still answer on time and only spin: three seconds in, the process must have
/// used next to no CPU.
#[test]
fn a_pool_task_waiting_on_a_deadline_parks_without_spinning() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn waiter(io: IoCap, tm: TimeCap, c: Chan[Int], res: Chan[Int]) -> Int ! { io } {\n\
                   let cs = vec_new[Chan[Int]](); vec_push(cs, c); let out = vec_new[Int]();\n\
                   chan_send_of(res, chan_select_until(cs, out, 4000)); 0 }\n\
               fn main(io: IoCap, t: SpawnCap, tm: TimeCap) -> Int ! { io, spawn } {\n\
                   let c = chan_new_of[Int](1); let res = chan_new_of[Int](1); let got = vec_new[Int]();\n\
                   scope { spawn waiter(io, tm, c, res); let r = chan_recv_of(res, got); }\n\
                   print(int_to_str(vec_get(got, 0))); 0 }\n";
    built(d.path(), "near", src, "llvm");
    let child = Command::new(d.path().join("near"))
        .env("NAZM_SCHEDULER", "pool")
        .env("NAZM_WORKERS", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let (seconds, text) = cpu_seconds(child.id());
    let o = child.wait_with_output().unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout), "-20\n");
    assert!(
        seconds < 1.0,
        "a pool task waiting four seconds used {text} of CPU in three"
    );
}

#[test]
fn a_deadline_past_the_nanosecond_range_parks_without_spinning() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn waiter(io: IoCap, tm: TimeCap, c: Chan[Int], res: Chan[Int]) -> Int ! { io } {\n\
                   let cs = vec_new[Chan[Int]](); vec_push(cs, c); let out = vec_new[Int]();\n\
                   chan_send_of(res, chan_select_until(cs, out, 10000000000000)); 0 }\n\
               fn main(io: IoCap, t: SpawnCap, tm: TimeCap) -> Int ! { io, spawn } {\n\
                   let c = chan_new_of[Int](1); let res = chan_new_of[Int](1); let got = vec_new[Int]();\n\
                   let idle = chan_new_of[Int](1); let ics = vec_new[Chan[Int]](); vec_push(ics, idle);\n\
                   let none = vec_new[Int]();\n\
                   scope { spawn waiter(io, tm, c, res);\n\
                       let w = chan_select_until(ics, none, 4000);\n\
                       chan_send_of(c, 42); let r = chan_recv_of(res, got); }\n\
                   print(int_to_str(vec_get(got, 0))); 0 }\n";
    built(d.path(), "far", src, "llvm");
    let child = Command::new(d.path().join("far"))
        .env("NAZM_SCHEDULER", "pool")
        .env("NAZM_WORKERS", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let cpu = Command::new("ps")
        .args(["-o", "time=", "-p", &child.id().to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&cpu.stdout).trim().to_owned();
    // `00:00:01` (procps) or `0:01.49` (BSD): seconds, minutes, hours from the right.
    let seconds: f64 = text
        .rsplit(':')
        .zip([1.0, 60.0, 3600.0])
        .map(|(part, unit)| part.trim().parse::<f64>().unwrap_or(f64::MAX) * unit)
        .sum();
    let o = child.wait_with_output().unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout), "00\n");
    assert!(
        seconds < 1.0,
        "a parked task with a far deadline used {text} of CPU in three seconds"
    );
}

#[test]
fn a_parked_tasks_deadline_fires_on_the_pool() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn waiter(io: IoCap, tm: TimeCap, c: Chan[Int], res: Chan[Int]) -> Int ! { io } {\n\
                   let cs = vec_new[Chan[Int]](); vec_push(cs, c); let out = vec_new[Int]();\n\
                   chan_send_of(res, chan_select_until(cs, out, 30)); 0 }\n\
               fn main(io: IoCap, t: SpawnCap, tm: TimeCap) -> Int ! { io, spawn } {\n\
                   let c = chan_new_of[Int](1); let res = chan_new_of[Int](1); let got = vec_new[Int]();\n\
                   let before = time_now_ms();\n\
                   scope { spawn waiter(io, tm, c, res); let r = chan_recv_of(res, got); }\n\
                   let waited = time_now_ms() - before;\n\
                   print(int_to_str(vec_get(got, 0))); print(if waited >= 30 { \" waited\" } else { \" early\" }); 0 }\n";
    built(d.path(), "dead", src, "llvm");
    let (out, err, code) = run(
        d.path(),
        "dead",
        &[],
        &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "1")],
    );
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, "-2 waited0\n");
    assert!(err.contains("timeouts=1"), "{err}");
}

// --- Performance leadership 6.4: a direct hand-off on the `Int` channel --------------------------

/// Parked receivers on an empty channel and parked senders on a full one, then a close: every
/// receiver ends with `false`, every sender is refused, and the value queued before stays.
const CLOSING_PARKED: &str = "fn taker(io: IoCap, c: Chan, done: Chan) -> Int ! { io } {\n\
    let got = ints_new();\n\
    if chan_recv(c, got) { chan_send(done, 1); } else { chan_send(done, 10); }\n\
    0 }\n\
fn giver(io: IoCap, c: Chan, done: Chan, v: Int) -> Int ! { io } {\n\
    if chan_send(c, v) { chan_send(done, 100); } else { chan_send(done, 1000); }\n\
    0 }\n\
fn closer(io: IoCap, a: Chan, b: Chan) -> Int ! { io } { chan_close(a); chan_close(b); 0 }\n\
fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
    let empty = chan_new(1); let full = chan_new(1); let done = chan_new(64);\n\
    let sent = chan_send(full, 7);\n\
    scope {\n\
        let mut i = 0;\n\
        while i < 8 { spawn taker(io, empty, done); spawn giver(io, full, done, i); i = i + 1; }\n\
        spawn closer(io, empty, full);\n\
    }\n\
    let got = ints_new(); let mut k = 0; let mut s = 0;\n\
    while k < 16 { if chan_recv(done, got) { s = s + ints_pop(got); } k = k + 1; }\n\
    let left = chan_recv(full, got);\n\
    print(int_to_str(s)); print(\" \"); print(int_to_str(ints_pop(got))); 0 }\n";

/// Five senders park on a full channel of one, in the order they ran; one receiver then takes
/// every value. Each value it frees a slot for is the next parked sender's, first come first
/// served — on one worker the run order is fixed, so the order received is too.
const SENDERS_IN_ORDER: &str =
    "fn giver(io: IoCap, c: Chan, v: Int) -> Int ! { io } { let s = chan_send(c, v); 0 }\n\
fn taker(io: IoCap, c: Chan, out: Chan, n: Int) -> Int ! { io } {\n\
    let got = ints_new(); let mut k = 0; let mut s = 0;\n\
    while k < n { if chan_recv(c, got) { s = s * 10 + ints_pop(got); } k = k + 1; }\n\
    let r = chan_send(out, s); 0 }\n\
fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
    let c = chan_new(1); let out = chan_new(1);\n\
    scope {\n\
        let mut i = 1;\n\
        while i < 6 { spawn giver(io, c, i); i = i + 1; }\n\
        spawn taker(io, c, out, 5);\n\
    }\n\
    let got = ints_new(); let r = chan_recv(out, got);\n\
    print(int_to_str(ints_pop(got))); 0 }\n";

/// Three receivers park on an empty channel in the order they ran; one sender then sends 1, 2
/// and 3, each straight to the receiver parked longest, which reports whether it got its own.
const RECEIVERS_IN_ORDER: &str =
    "fn taker(io: IoCap, c: Chan, out: Chan, want: Int) -> Int ! { io } {\n\
    let got = ints_new();\n\
    if chan_recv(c, got) { let r = chan_send(out, if ints_pop(got) == want { 1 } else { 0 }); }\n\
    0 }\n\
fn giver(io: IoCap, c: Chan) -> Int ! { io } {\n\
    let mut i = 1; while i < 4 { let s = chan_send(c, i); i = i + 1; } 0 }\n\
fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
    let c = chan_new(1); let out = chan_new(8);\n\
    scope {\n\
        let mut i = 1;\n\
        while i < 4 { spawn taker(io, c, out, i); i = i + 1; }\n\
        spawn giver(io, c);\n\
    }\n\
    let got = ints_new(); let mut k = 0; let mut s = 0;\n\
    while k < 3 { if chan_recv(out, got) { s = s + ints_pop(got); } k = k + 1; }\n\
    print(int_to_str(s)); 0 }\n";

#[test]
fn a_close_ends_every_parked_receiver_and_refuses_every_parked_sender() {
    let d = tempfile::tempdir().unwrap();
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "closing", CLOSING_PARKED, backend);
        for env in [
            &[("NAZM_SCHEDULER", "threads")][..],
            &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "1")][..],
            &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "2")][..],
        ] {
            let (out, err, code) = run(d.path(), "closing", &[], env);
            assert_eq!(code, Some(0), "{backend} {env:?}: {err}");
            assert!(out.starts_with("8080 7"), "{backend} {env:?}: {out}");
            assert!(err.contains("live=0"), "{backend} {env:?}: {err}");
        }
    }
}

#[test]
fn parked_senders_and_receivers_are_served_first_come_first_served() {
    let d = tempfile::tempdir().unwrap();
    for backend in ["llvm", "cranelift"] {
        built(d.path(), "senders", SENDERS_IN_ORDER, backend);
        built(d.path(), "receivers", RECEIVERS_IN_ORDER, backend);
        let one = [("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "1")];
        let (out, err, code) = run(d.path(), "senders", &[], &one);
        assert_eq!(code, Some(0), "{backend}: {err}");
        assert!(out.starts_with("12345"), "{backend}: {out}");
        let (out, err, code) = run(d.path(), "receivers", &[], &one);
        assert_eq!(code, Some(0), "{backend}: {err}");
        assert!(out.starts_with('3'), "{backend}: {out}");
        // Under more workers or threads the order is the schedule's; every value still arrives.
        for env in [
            &[("NAZM_SCHEDULER", "threads")][..],
            &[("NAZM_SCHEDULER", "pool"), ("NAZM_WORKERS", "2")][..],
        ] {
            let (out, err, code) = run(d.path(), "senders", &[], env);
            assert_eq!(code, Some(0), "{backend} {env:?}: {err}");
            let mut digits: Vec<char> = out.trim_end().chars().take(5).collect();
            digits.sort_unstable();
            assert_eq!(
                digits,
                ['1', '2', '3', '4', '5'],
                "{backend} {env:?}: {out}"
            );
        }
    }
}
