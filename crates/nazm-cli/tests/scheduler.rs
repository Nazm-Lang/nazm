//! N44: structured concurrency and the scheduler contract, under load and at its edges.
//!
//! `docs/architecture.md` §7.46. `tests/concurrent.rs` holds the language's concurrency rules
//! program by program; this suite holds what the scheduler contract adds: thousands of tasks and
//! deep nesting reclaiming everything, which failure a scope reports when several do, the
//! authority and provenance a task receives, what may not cross into one, and `exit_with` from
//! inside one — each under the interpreter, both optimisation levels of the LLVM backend and the
//! Cranelift backend, with a deadline, because a scheduling defect's usual symptom is that nothing
//! happens.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use crate::common;

use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");
const DEADLINE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn bounded(cmd: &mut Command, what: &str) -> Ran {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let out = child.wait_with_output().expect("output");
            return Ran {
                code: status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            };
        }
        if start.elapsed() >= DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not finish within {DEADLINE:?}: a deadlock or a lost wakeup");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The program run by the interpreter and by three native builds, with the memory report.
fn four(src: &str) -> Vec<(String, Ran)> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let mut out = vec![(
        "interpreter".to_owned(),
        bounded(
            Command::new(NAZM).arg("run").arg("--interpret").arg(&path),
            "nazm run",
        ),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.path().join(label.replace([' ', '-'], ""));
        let b = bounded(
            Command::new(NAZM)
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
                .arg(&exe),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        // Many live tasks are this file's subject: see `TASKS_ADDRESS_SPACE_KB`.
        let mut c = common::bounded_exe_within(&exe, common::TASKS_ADDRESS_SPACE_KB);
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected` and exits 0, and every native build reclaims all it
/// allocated.
fn agrees(src: &str, expected: &str) {
    for (label, r) in four(src) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        if label != "interpreter" {
            reclaimed(&label, &r.stderr);
        }
    }
}

fn reclaimed(label: &str, stderr: &str) {
    let report = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("{label}: no memory report in {stderr:?}"));
    assert_eq!(report.matches("live=0").count(), 4, "{label}: {report}");
}

fn check_codes(src: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let o = Command::new(NAZM)
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn five_thousand_short_tasks_finish_and_reclaim_everything() {
    // Two hundred scopes in sequence, twenty-five tasks each; every task takes a string it did
    // not allocate and one it did, and reports on a channel the parent drains after the join.
    // Twenty-five at a time, not more: v1 starts one OS thread per task (§7.46), and a bounded
    // host — the contained gate's 512-process ceiling, shared with every test running beside
    // this one — may refuse a thread, which is the contract but not this test's subject.
    let src = "fn work(c: Chan, s: Str, n: Int) -> Int { let t = str_concat(s, int_to_str(n)); chan_send(c, str_len(t)); 0 }\n\
               fn main(t: SpawnCap) -> Int {\n\
                 let c = chan_new(25); let buf = ints_new(); let mut round = 0; let mut total = 0;\n\
                 while round < 200 {\n\
                   let s = str_concat(\"task\", \"-\");\n\
                   scope { let mut i = 0; while i < 25 { spawn work(c, s, i); i = i + 1; } }\n\
                   let mut k = 0; while k < 25 { chan_recv(c, buf); k = k + 1; }\n\
                   round = round + 1;\n\
                 }\n\
                 let mut j = 0; while j < ints_len(buf) { total = total + ints_get(buf, j); j = j + 1; }\n\
                 total }\n";
    // Each string is "task-" (5) and the decimal of 0..24: 10 of one digit and 15 of two.
    let per_round = 25 * 5 + 10 + 15 * 2;
    agrees(src, &(200 * per_round).to_string());
}

#[test]
fn thirty_nested_scopes_join_innermost_first() {
    // Each level starts a task that sends its depth; the parent drains after every scope has
    // closed. Innermost-first joining is observable as a record of which closed first: each
    // level sends its depth on a second channel right after its inner scope closes.
    let depth = 30;
    let mut body = String::new();
    for d in 0..depth {
        let _ = write!(body, "scope {{ spawn send(c, {d}); ");
    }
    for d in (0..depth).rev() {
        let _ = write!(body, "}} chan_send(order, {d}); ");
    }
    let src = format!(
        "fn send(c: Chan, v: Int) -> Int {{ chan_send(c, v); 0 }}\n\
         fn main(t: SpawnCap) -> Int {{\n\
           let c = chan_new({depth}); let order = chan_new({depth}); let buf = ints_new(); let seen = ints_new();\n\
           {body}\n\
           let mut k = 0; let mut sum = 0; while k < {depth} {{ chan_recv(c, buf); chan_recv(order, seen); sum = sum + ints_get(buf, k); k = k + 1; }}\n\
           let mut ok = 1; let mut i = 0; while i < {depth} {{ if ints_get(seen, i) != {depth} - 1 - i {{ ok = 0; }} i = i + 1; }}\n\
           sum * ok }}\n"
    );
    agrees(&src, &(depth * (depth - 1) / 2).to_string());
}

#[test]
fn a_scope_reports_its_first_failure_in_start_order_not_in_time() {
    // `first` is started first and fails only after `second` has failed and let it go: the
    // report must be `first`'s overflow, not `second`'s division by zero.
    let src = "fn first(go: Chan, seen: Ints) -> Int { chan_recv(go, seen); 9223372036854775807 + ints_len(seen) }\n\
               fn second(go: Chan, z: Int) -> Int { chan_send(go, 1); 10 / z }\n\
               fn main(t: SpawnCap) -> Int {\n\
                 let go = chan_new(1);\n\
                 scope { spawn first(go, ints_new()); spawn second(go, 0); }\n\
                 0 }\n";
    // A sequence may not be handed to a task, so `first` gets its buffer another way.
    let src = src.replace(
        "fn first(go: Chan, seen: Ints) -> Int { chan_recv(go, seen); 9223372036854775807 + ints_len(seen) }",
        "fn first(go: Chan, n: Int) -> Int { let seen = ints_new(); chan_recv(go, seen); 9223372036854775807 + ints_len(seen) + n }",
    );
    let src = src.replace("spawn first(go, ints_new());", "spawn first(go, 0);");
    for (label, r) in four(&src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(r.stderr.contains("N0400"), "{label}: {}", r.stderr);
        assert!(!r.stderr.contains("N0401"), "{label}: {}", r.stderr);
        if label != "interpreter" {
            reclaimed(&label, &r.stderr);
        }
    }
}

#[test]
fn a_failure_deep_in_nested_scopes_joins_every_level_and_reclaims_everything() {
    let src = "fn ok(c: Chan, s: Str) -> Int { chan_send(c, str_len(s)); 0 }\n\
               fn bad(n: Int) -> Int { 9223372036854775807 + n }\n\
               fn main(t: SpawnCap) -> Int {\n\
                 let c = chan_new(8); let s = str_concat(\"a\", \"b\");\n\
                 scope { spawn ok(c, s); scope { spawn ok(c, s); scope { spawn ok(c, s); spawn bad(1); spawn ok(c, s); } spawn ok(c, s); } }\n\
                 0 }\n";
    for (label, r) in four(src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(r.stderr.contains("N0400"), "{label}: {}", r.stderr);
        if label != "interpreter" {
            reclaimed(&label, &r.stderr);
        }
    }
}

#[test]
fn records_and_enums_handed_to_tasks_are_released_by_them() {
    // What may cross into a task includes records and enums holding strings; each task holds
    // its own references and gives them back, so everything is reclaimed however many do.
    let src = "struct Job { name: Str, n: Int }\n\
               enum Tag { Plain, Named(label: Str) }\n\
               fn run(c: Chan, j: Job, t: Tag) -> Int { let extra = match t { Tag.Plain() => 0, Tag.Named(label: l) => str_len(l) }; chan_send(c, str_len(j.name) + j.n + extra); 0 }\n\
               fn main(t: SpawnCap) -> Int {\n\
                 let c = chan_new(64); let buf = ints_new(); let mut r = 0;\n\
                 while r < 20 { scope { let mut i = 0; while i < 32 { let j = Job(name: str_concat(\"job\", int_to_str(i)), n: 1); if i % 2 == 0 { spawn run(c, j, Tag.Plain()); } else { spawn run(c, j, Tag.Named(label: str_concat(\"x\", \"y\"))); } i = i + 1; } } let mut k = 0; while k < 32 { chan_recv(c, buf); k = k + 1; } r = r + 1; }\n\
                 let mut total = 0; let mut j = 0; while j < ints_len(buf) { total = total + ints_get(buf, j); j = j + 1; } total }\n";
    // Per round: names are "job" + 0..31 (10 of 4 bytes, 22 of 5), each n is 1, and the 16 odd
    // jobs carry a two-byte label.
    let per_round = 10 * 4 + 22 * 5 + 32 + 16 * 2;
    agrees(src, &(20 * per_round).to_string());
}

#[test]
fn a_receiver_waiting_on_an_empty_channel_is_woken_by_a_send() {
    // The task computes for a while before it sends, so the parent is certainly waiting when
    // the value arrives: a send that woke nobody would leave it waiting for ever.
    let src = "fn late(c: Chan, n: Int) -> Int { let mut i = 0; let mut x = 0; while i < n { x = (x + i) % 97; i = i + 1; } chan_send(c, x + 1000); 0 }\n\
               fn main(t: SpawnCap) -> Int { let c = chan_new(1); let buf = ints_new(); scope { spawn late(c, 3000000); chan_recv(c, buf); } ints_get(buf, 0) }\n";
    let expected = {
        let mut x = 0_i64;
        for i in 0..3_000_000_i64 {
            x = (x + i) % 97;
        }
        (x + 1000).to_string()
    };
    agrees(src, &expected);
}

#[test]
fn a_task_has_exactly_the_authority_it_is_handed() {
    // Passed: a declared child prints with the `IoCap` it was given.
    let passed = "fn child(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }\n\
                  fn run(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { scope { spawn child(io, 7); } 0 }\n\
                  fn main(io: IoCap, t: SpawnCap) -> Int { run(io, t) }\n";
    agrees(passed, "70");
    // Withheld: a declared child cannot write a file without one, whatever its spawner holds.
    // (A file, not printing: since D8 every function of the program holds `main`'s output.)
    let withheld = "fn child(n: Int) -> Int ! { io } { write_file(\"f\", int_to_str(n)) }\n\
                    fn run(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { scope { spawn child(7); } 0 }\n\
                    fn main(io: IoCap, t: SpawnCap) -> Int { run(io, t) }\n";
    assert!(check_codes(withheld).contains(&"N0369".to_owned()));
    // An undeclared child has no authority of its own to borrow (N104): it is refused where it
    // writes, whatever its spawner holds.
    let bridged = "fn child() -> Int { write_file(\"f\", \"x\") }\n\
                   fn run(t: SpawnCap) -> Int ! { io, spawn } { scope { spawn child(); } 0 }\n\
                   fn main(io: IoCap, t: SpawnCap) -> Int { run(t) }\n";
    assert!(check_codes(bridged).contains(&"N0369".to_owned()));
    // And starting a task at all needs a `SpawnCap`, declared set or not.
    let unspawnable = "fn child() -> Int ! {} { 0 }\n\
                       fn run() -> Int ! { spawn } { scope { spawn child(); } 0 }\n\
                       fn main(t: SpawnCap) -> Int { run() }\n";
    assert!(check_codes(unspawnable).contains(&"N0369".to_owned()));
}

#[test]
fn a_task_s_arguments_carry_their_provenance_into_it() {
    let src = "fn save(io: IoCap, path: Str) -> Int ! { io } { write_file(path, \"x\") }\n\
               fn run(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { let p = read_file(\"config\"); scope { spawn save(io, p); } 0 }\n\
               fn main(io: IoCap, t: SpawnCap) -> Int { run(io, t) }\n";
    assert_eq!(check_codes(src), ["N0372"]);
    // The same task handed a path the program computed is fine.
    let ok = src.replace(
        "let p = read_file(\\\"config\\\");",
        "let p = str_concat(\"out\", \".txt\");",
    );
    let ok = ok.replace(
        "let p = read_file(\"config\");",
        "let p = str_concat(\"out\", \".txt\");",
    );
    assert!(check_codes(&ok).is_empty(), "{ok}");
}

#[test]
fn nothing_mutable_crosses_into_a_task() {
    for (decl, arg) in [
        ("", "ints_new()"),
        ("", "strs_new()"),
        ("struct Holder { v: Ints }\n", "Holder(v: ints_new())"),
        (
            "struct Outer { h: Inner }\nstruct Inner { v: Strs }\n",
            "Outer(h: Inner(v: strs_new()))",
        ),
        ("enum Maybe { None, Some(v: Ints) }\n", "Maybe.None()"),
    ] {
        let ty = if arg.starts_with("Holder") {
            "Holder"
        } else if arg.starts_with("Outer") {
            "Outer"
        } else if arg.starts_with("Maybe") {
            "Maybe"
        } else if arg.starts_with("ints") {
            "Ints"
        } else {
            "Strs"
        };
        let src = format!(
            "{decl}fn child(x: {ty}) -> Int {{ 0 }}\nfn main(t: SpawnCap) -> Int {{ scope {{ spawn child({arg}); }} 0 }}\n"
        );
        assert!(check_codes(&src).contains(&"N0321".to_owned()), "{src}");
    }
}

#[test]
fn exit_with_in_a_task_ends_the_process_with_its_status() {
    let src = "fn quit(io: IoCap) -> Int { exit_with(3); 0 }\n\
               fn wait(c: Chan) -> Int { let b = ints_new(); chan_recv(c, b); 0 }\n\
               fn main(io: IoCap, t: SpawnCap) -> Int { let c = chan_new(1); scope { spawn wait(c); spawn quit(io); } 5 }\n";
    for (label, r) in four(src) {
        // Its sibling is still blocked when the process ends: nothing is joined, nothing is
        // reported, and `main`'s result is never printed.
        assert_eq!((r.code, r.stdout.as_str()), (Some(3), ""), "{label}: {r:?}");
        assert!(!r.stderr.contains("nazm-memory"), "{label}: {r:?}");
    }
}

/// Measurements for `docs/performance.md`: task start-and-join, channel round trips, and
/// scaling over independent CPU-bound tasks. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by the benchmark gate"]
fn what_tasks_and_channels_cost() {
    let dir = tempfile::tempdir().unwrap();
    let build = |name: &str, src: &str, backend: &str| {
        let path = dir.path().join(format!("{name}.nz"));
        std::fs::write(&path, src).unwrap();
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
        exe
    };
    let time = |exe: &Path, args: &[&str]| {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let s = Instant::now();
            let o = Command::new(exe).args(args).output().unwrap();
            assert!(o.status.success());
            best = best.min(s.elapsed().as_secs_f64() * 1000.0);
        }
        best
    };
    let spawn = "fn nothing(n: Int) -> Int { n }\n\
                 fn main(t: SpawnCap) -> Int { let mut r = 0; while r < 200 { scope { let mut i = 0; while i < 50 { spawn nothing(i); i = i + 1; } } r = r + 1; } 0 }\n";
    let ping = "fn echo(a: Chan, b: Chan, n: Int) -> Int { let buf = ints_new(); let mut i = 0; while i < n { chan_recv(a, buf); chan_send(b, ints_get(buf, i)); i = i + 1; } 0 }\n\
                fn main(t: SpawnCap) -> Int { let a = chan_new(1); let b = chan_new(1); let buf = ints_new(); let n = 100000;\n\
                  scope { spawn echo(a, b, n); let mut i = 0; while i < n { chan_send(a, i); chan_recv(b, buf); i = i + 1; } } ints_len(buf) }\n";
    let stream = "fn produce(c: Chan, n: Int) -> Int { let mut i = 0; while i < n { chan_send(c, i); i = i + 1; } 0 }\n\
                  fn main(t: SpawnCap) -> Int { let c = chan_new(1024); let buf = ints_new(); let n = 1000000;\n\
                    scope { spawn produce(c, n); let mut i = 0; while i < n { chan_recv(c, buf); i = i + 1; } } ints_len(buf) }\n";
    let work = |tasks: usize| {
        let per = 400_000_000 / tasks;
        format!(
            "fn burn(c: Chan, n: Int) -> Int {{ let mut i = 0; let mut x = 0; while i < n {{ x = (x + i) % 1000003; i = i + 1; }} chan_send(c, x); 0 }}\n\
             fn main(t: SpawnCap) -> Int {{ let c = chan_new({tasks}); scope {{ let mut k = 0; while k < {tasks} {{ spawn burn(c, {per}); k = k + 1; }} }} 0 }}\n"
        )
    };
    for backend in ["llvm", "cranelift"] {
        let s = build("spawn", spawn, backend);
        let ms = time(&s, &[]);
        println!(
            "{backend}: 10,000 tasks started and joined in scopes of 50: {ms:.1} ms, {:.1} µs each",
            ms * 100.0 / 1000.0
        );
        let p = build("ping", ping, backend);
        let ms = time(&p, &[]);
        println!(
            "{backend}: 100,000 channel round trips between two tasks: {ms:.1} ms, {:.2} µs each",
            ms * 10.0 / 1000.0
        );
        let st = build("stream", stream, backend);
        let ms = time(&st, &[]);
        println!("{backend}: 1,000,000 values streamed through a 1,024-slot channel: {ms:.1} ms, {:.0} ns each", ms);
        for tasks in [1, 2, 4] {
            let w = build(&format!("work{tasks}"), &work(tasks), backend);
            let ms = time(&w, &[]);
            println!("{backend}: 400 M iterations over {tasks} task(s): {ms:.0} ms");
        }
    }
}
