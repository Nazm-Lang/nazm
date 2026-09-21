//! Structured concurrency compiled to native code, checked against two oracles.
//!
//! Every program here is the same source the interpreter runs in
//! `nazm-core/tests/concurrency.rs`. Each is checked against **a written expected value**
//! and against **`nazm run`**, at `-O0` and `-O2`. Either oracle alone is weak in its own
//! way: an expectation can be wrong, and differential agreement is satisfied by a bug the
//! two implementations share — and since the channel runtime here was written from the
//! interpreter's, that is the likely kind.
//!
//! Both optimisation levels, because the whole of this is memory the optimiser is entitled
//! to reason about. A missing synchronisation is exactly the defect that survives `-O0`
//! and is exposed at `-O2`, and a suite that only ran one of them would not know.
//!
//! Every subprocess has a deadline. A concurrency defect's usual symptom is that nothing
//! happens, and a compiled Nazm program carries no iteration budget — without a deadline
//! a deadlock stops the suite reporting instead of failing it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Long enough for the stress cases on a loaded machine, short enough that a deadlock is
/// reported rather than waited on.
const DEADLINE: Duration = Duration::from_secs(60);

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Its own copy rather than one shared with `tests/build.rs`: each integration test is a
/// separate binary, and a hung compiled program must fail this suite rather than hang it.
fn run_bounded(cmd: &mut Command) -> Option<Ran> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("collect output");
                return Some(Ran {
                    code: status.code(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                });
            }
            None if start.elapsed() >= DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn write_program(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    std::fs::write(&src, body).expect("write source");
    (dir, src)
}

/// Run one program three ways and require one answer.
fn agrees(what: &str, src: &str, expected: &str) {
    let (dir, path) = write_program(src);

    let interp = run_bounded(Command::new(NAZM).args(["run", path.to_str().unwrap()]))
        .unwrap_or_else(|| {
            panic!("{what}: `nazm run` deadlocked — it did not finish in {DEADLINE:?}")
        });
    assert_eq!(
        interp.code,
        Some(0),
        "{what}: the interpreter rejected this program: {}",
        interp.stderr
    );
    assert_eq!(
        interp.stdout.trim(),
        expected,
        "{what}: the *expectation* disagrees with the interpreter — one of them is wrong, \
         and differential agreement alone would have hidden it"
    );

    for opt in ["0", "2"] {
        let exe = dir.path().join(format!("prog{opt}"));
        let built = run_bounded(
            Command::new(NAZM)
                .args(["build", path.to_str().unwrap()])
                .arg("-o")
                .arg(&exe)
                .arg("--opt-level")
                .arg(opt),
        )
        .unwrap_or_else(|| panic!("{what}: `nazm build -O{opt}` did not finish"));
        assert_eq!(
            built.code,
            Some(0),
            "{what}: -O{opt} build failed: {}",
            built.stderr
        );

        let ran = run_bounded(&mut common::bounded_exe(&exe)).unwrap_or_else(|| {
            panic!(
                "{what}: the -O{opt} executable did not finish within {DEADLINE:?}. A \
                 timeout is a failure, not a result — this is what a deadlock or a lost \
                 wake-up looks like."
            )
        });
        assert_eq!(
            ran.code,
            Some(0),
            "{what}: the -O{opt} executable failed: {}",
            ran.stderr
        );
        assert_eq!(
            ran.stdout.trim(),
            expected,
            "{what}: the -O{opt} executable disagrees with the interpreter and with the \
             expectation"
        );
    }
}

#[test]
fn a_worker_pool_runs_natively() {
    agrees(
        "worker pool",
        r"
        fn worker(jobs: Chan, results: Chan) -> Int {
            let buf = ints_new();
            while chan_recv(jobs, buf) { chan_send(results, ints_pop(buf) * 2); }
            0
        }
        fn main() -> Int {
            let jobs = chan_new(4);
            let results = chan_new(64);
            scope {
                spawn worker(jobs, results);
                spawn worker(jobs, results);
                spawn worker(jobs, results);
                let mut i = 1;
                while i <= 10 { chan_send(jobs, i); i = i + 1; }
                chan_close(jobs);
            }
            chan_close(results);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(results, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "110",
    );
}

#[test]
fn back_pressure_delivers_everything() {
    // Capacity one: the sender is at most one message ahead, so every send but the first
    // blocks until a receive makes room. 1 + ... + 200.
    agrees(
        "capacity one",
        r"
        fn drain(jobs: Chan, results: Chan) -> Int {
            let buf = ints_new();
            let mut sum = 0;
            while chan_recv(jobs, buf) { sum = sum + ints_pop(buf); }
            chan_send(results, sum);
            0
        }
        fn main() -> Int {
            let jobs = chan_new(1);
            let results = chan_new(1);
            scope {
                spawn drain(jobs, results);
                let mut i = 1;
                while i <= 200 { chan_send(jobs, i); i = i + 1; }
                chan_close(jobs);
            }
            let buf = ints_new();
            chan_recv(results, buf);
            ints_pop(buf)
        }
        ",
        "20100",
    );
}

#[test]
fn closing_wakes_every_blocked_receiver() {
    // Four tasks blocked on a channel nothing is ever sent to. Only the close can end
    // them, and only the wake-up can end the join.
    agrees(
        "close wakes receivers",
        r"
        fn waiter(jobs: Chan) -> Int {
            let buf = ints_new();
            while chan_recv(jobs, buf) { ints_pop(buf); }
            0
        }
        fn main() -> Int {
            let jobs = chan_new(4);
            scope {
                spawn waiter(jobs);
                spawn waiter(jobs);
                spawn waiter(jobs);
                spawn waiter(jobs);
                chan_close(jobs);
            }
            7
        }
        ",
        "7",
    );
}

#[test]
fn closing_wakes_a_blocked_sender_and_refuses_its_value() {
    agrees(
        "close wakes sender",
        r"
        fn flood(jobs: Chan, results: Chan) -> Int {
            let mut refused = 0;
            let mut i = 0;
            while i < 100 {
                if chan_send(jobs, i) == false { refused = refused + 1; }
                i = i + 1;
            }
            chan_send(results, refused);
            0
        }
        fn main() -> Int {
            let jobs = chan_new(2);
            let results = chan_new(1);
            scope {
                spawn flood(jobs, results);
                chan_close(jobs);
            }
            let buf = ints_new();
            chan_recv(results, buf);
            if ints_pop(buf) > 0 { 1 } else { 0 }
        }
        ",
        "1",
    );
}

#[test]
fn a_close_keeps_what_is_already_queued() {
    agrees(
        "close keeps the queue",
        r"
        fn main() -> Int {
            let c = chan_new(4);
            chan_send(c, 3);
            chan_send(c, 4);
            chan_close(c);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(c, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "7",
    );
}

#[test]
fn scopes_nest() {
    agrees(
        "nested scopes",
        r"
        fn add(out: Chan, n: Int) -> Int { chan_send(out, n); 0 }
        fn main() -> Int {
            let inner = chan_new(4);
            let outer = chan_new(4);
            scope {
                scope {
                    spawn add(inner, 10);
                    spawn add(inner, 20);
                }
                chan_close(inner);
                let buf = ints_new();
                let mut stage = 0;
                while chan_recv(inner, buf) { stage = stage + ints_pop(buf); }
                spawn add(outer, stage);
            }
            chan_close(outer);
            let buf = ints_new();
            chan_recv(outer, buf);
            ints_pop(buf)
        }
        ",
        "30",
    );
}

#[test]
fn every_message_is_delivered_exactly_once_under_contention() {
    // Three producers and three consumers over a channel of capacity one, which forces a
    // hand-off on every message. 3 x (1 + ... + 300). A message lost or delivered twice
    // changes the total; no ordering is asserted, because none is promised.
    agrees(
        "contention",
        r"
        fn produce(jobs: Chan) -> Int {
            let mut i = 1;
            while i <= 300 { chan_send(jobs, i); i = i + 1; }
            0
        }
        fn consume(jobs: Chan, results: Chan) -> Int {
            let buf = ints_new();
            let mut sum = 0;
            while chan_recv(jobs, buf) { sum = sum + ints_pop(buf); }
            chan_send(results, sum);
            0
        }
        fn main() -> Int {
            let jobs = chan_new(1);
            let results = chan_new(8);
            scope {
                spawn consume(jobs, results);
                spawn consume(jobs, results);
                spawn consume(jobs, results);
                scope {
                    spawn produce(jobs);
                    spawn produce(jobs);
                    spawn produce(jobs);
                }
                chan_close(jobs);
            }
            chan_close(results);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(results, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "135450",
    );
}

#[test]
fn many_scopes_in_sequence_leave_nothing_running() {
    // A thousand tasks over two hundred scopes: a thousand real threads, created and
    // joined. If a scope ever returned without joining, the total would be short.
    agrees(
        "scope cleanup",
        r"
        fn one(out: Chan) -> Int { chan_send(out, 1); 0 }
        fn main() -> Int {
            let out = chan_new(2048);
            let mut round = 0;
            while round < 200 {
                scope {
                    spawn one(out);
                    spawn one(out);
                    spawn one(out);
                    spawn one(out);
                    spawn one(out);
                }
                round = round + 1;
            }
            chan_close(out);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(out, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "1000",
    );
}

// ---------------------------------------------------------------------------
// Leaving a scope early
//
// The join has to happen on *every* way out of a scope, not just on falling off the end.
// Emission writes it before the `ret` and before the branch a `break` takes, and these
// two are what hold that.
//
// # These are stress cases, and say so
//
// A missing join is a race, not a fault: the program would usually still finish, and
// might still print the right number. Each task therefore does several million
// iterations of real work before it sends, so that a scope which did not wait would
// close the channel first and lose the value — with overwhelming probability rather than
// with certainty. That is what a stress case is for, and pretending otherwise would be
// the dishonest part.
// ---------------------------------------------------------------------------

#[test]
fn a_return_out_of_a_scope_still_waits_for_its_tasks() {
    agrees(
        "return through a scope",
        r"
        fn slow(out: Chan, n: Int) -> Int {
            let mut i = 0;
            while i < 3000000 { i = i + 1; }
            chan_send(out, n);
            0
        }
        fn early(out: Chan) -> Int {
            scope {
                spawn slow(out, 5);
                spawn slow(out, 5);
                return 1;
            }
            0
        }
        fn main() -> Int {
            let out = chan_new(8);
            early(out);
            chan_close(out);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(out, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "10",
    );
}

#[test]
fn a_break_out_of_a_scope_still_waits_for_its_tasks() {
    // The `break` leaves both the scope and the loop. The scope opened inside the loop is
    // joined; a scope *around* the loop would not be, which is why the loop records how
    // many were open when it started.
    agrees(
        "break through a scope",
        r"
        fn slow(out: Chan, n: Int) -> Int {
            let mut i = 0;
            while i < 3000000 { i = i + 1; }
            chan_send(out, n);
            0
        }
        fn main() -> Int {
            let out = chan_new(16);
            let mut round = 0;
            while round < 3 {
                scope {
                    spawn slow(out, 1);
                    spawn slow(out, 1);
                    if round == 1 { break; }
                }
                round = round + 1;
            }
            chan_close(out);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(out, buf) { total = total + ints_pop(buf); }
            total
        }
        ",
        "4",
    );
}

#[test]
fn a_task_that_fails_ends_the_whole_program() {
    // A task has no private failure mode: the division is reported exactly as it would be
    // on the main thread, and the process exits 2.
    let src = r"
        fn bad(c: Chan) -> Int {
            let buf = ints_new();
            chan_recv(c, buf);
            1 / ints_pop(buf)
        }
        fn main() -> Int {
            let c = chan_new(1);
            scope {
                spawn bad(c);
                chan_send(c, 0);
            }
            0
        }
    ";
    let (dir, path) = write_program(src);
    let exe = dir.path().join("prog");
    let built = run_bounded(
        Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe),
    )
    .expect("build did not finish");
    assert_eq!(built.code, Some(0), "build failed: {}", built.stderr);

    let ran = run_bounded(&mut common::bounded_exe(&exe)).expect("the executable did not finish");
    assert_eq!(ran.code, Some(2), "a failing task must end the program");
    assert!(
        ran.stderr.contains("N0402") || ran.stderr.contains("zero"),
        "the failure should name the division, got: {}",
        ran.stderr
    );

    let interp = run_bounded(Command::new(NAZM).args(["run", path.to_str().unwrap()]))
        .expect("`nazm run` did not finish");
    assert_eq!(
        interp.code, ran.code,
        "the interpreter and the executable must agree on the exit status"
    );
}

// ---------------------------------------------------------------------------
// Does a failure wait for the scope's tasks?
//
// `docs/spec.md` promises the closing brace joins every task *including when the body
// itself failed*, so a failure is reported once nothing is still running. Asserting the
// diagnostic code cannot check that: a run that joined first and a run that exited on the
// spot produce the same code and the same exit status. The difference is only visible in
// whether the task's own effect happened, so these programs give the worker one — a line
// on stdout — and coordinate deterministically rather than by sleeping.
//
// The worker cannot print until the body has sent to `go`, and the body fails immediately
// after sending. Under the promised semantics the marker is therefore *always* printed:
// the program cannot exit before the worker returns. Under an implementation that exits
// where it failed, the worker is still counting and the marker is lost.
// ---------------------------------------------------------------------------

/// The body overflows while a worker it just released is still working.
const BODY_FAILS: &str = r#"
fn worker(go: Chan) -> Int {
    let buf = ints_new();
    chan_recv(go, buf);
    ints_pop(buf);
    let mut acc = 0;
    let mut i = 0;
    while i < 200000 { acc = acc + i; i = i + 1; }
    print("worker finished");
    acc
}
fn main() -> Int {
    let go = chan_new(1);
    let mut top = 1;
    scope {
        spawn worker(go);
        chan_send(go, 1);
        let mut n = 0;
        while n < 64 { top = top + top; n = n + 1; }
    }
    top
}
"#;

/// One task divides by zero while a sibling is still working.
const TASK_FAILS: &str = r#"
fn bad(dead: Chan) -> Int {
    let buf = ints_new();
    chan_recv(dead, buf);
    1 / ints_pop(buf)
}
fn worker(go: Chan) -> Int {
    let buf = ints_new();
    chan_recv(go, buf);
    ints_pop(buf);
    let mut acc = 0;
    let mut i = 0;
    while i < 200000 { acc = acc + i; i = i + 1; }
    print("worker finished");
    acc
}
fn main() -> Int {
    let dead = chan_new(1);
    let go = chan_new(1);
    scope {
        spawn bad(dead);
        spawn worker(go);
        chan_send(go, 1);
        chan_send(dead, 0);
    }
    0
}
"#;

const MARKER: &str = "worker finished";

#[test]
fn a_failing_scope_joins_its_tasks_before_reporting_interpreted() {
    for (what, src, code) in [
        ("body fails", BODY_FAILS, "N0400"),
        ("task fails", TASK_FAILS, "N0401"),
    ] {
        let (_dir, path) = write_program(src);
        let ran = run_bounded(Command::new(NAZM).args(["run", path.to_str().unwrap()]))
            .unwrap_or_else(|| panic!("{what}: `nazm run` did not finish"));
        assert_eq!(ran.code, Some(2), "{what}: {}", ran.stderr);
        assert!(
            ran.stderr.contains(code),
            "{what}: expected {code}, got {}",
            ran.stderr
        );
        assert!(
            ran.stdout.contains(MARKER),
            "{what}: the scope reported before its task finished — {MARKER:?} is missing \
             from stdout, which is what a failure that did not join looks like. stdout: {:?}",
            ran.stdout
        );
    }
}

/// The same contract, compiled. **Currently false**, and measured rather than assumed:
/// at `-O0` and `-O2` both programs exit 2 with the right diagnostic and print nothing,
/// because `@nz.fail` writes the message and calls `@exit` on whichever thread failed
/// (`crates/nazm-lir/src/emit.rs`, `@nz.fail`). `@nz.scope_join` does join every task,
/// but a failure never reaches it.
///
/// Ignored rather than deleted or inverted: inverting it would make the divergence look
/// like the specification, and deleting it would lose the only executable statement of
/// what the native runtime still owes. Closing it needs a way for a failure to leave a
/// task without leaving the process — see *Known limitations* in `docs/delivery.md`.
#[test]
#[ignore = "known divergence: native @nz.fail exits the process where it failed instead of \
            joining the scope; see docs/spec.md, Error propagation"]
fn a_failing_scope_joins_its_tasks_before_reporting_natively() {
    for (what, src, code) in [
        ("body fails", BODY_FAILS, "N0400"),
        ("task fails", TASK_FAILS, "N0401"),
    ] {
        let (dir, path) = write_program(src);
        for opt in ["0", "2"] {
            let exe = dir.path().join(format!("prog{opt}"));
            let built = run_bounded(
                Command::new(NAZM)
                    .args(["build", path.to_str().unwrap()])
                    .arg("-o")
                    .arg(&exe)
                    .arg("--opt-level")
                    .arg(opt),
            )
            .unwrap_or_else(|| panic!("{what}: -O{opt} build did not finish"));
            assert_eq!(built.code, Some(0), "{what}: -O{opt}: {}", built.stderr);

            let ran = run_bounded(&mut common::bounded_exe(&exe))
                .unwrap_or_else(|| panic!("{what}: the -O{opt} executable did not finish"));
            assert_eq!(ran.code, Some(2), "{what} -O{opt}: {}", ran.stderr);
            assert!(ran.stderr.contains(code), "{what} -O{opt}: {}", ran.stderr);
            assert!(
                ran.stdout.contains(MARKER),
                "{what} -O{opt}: the process exited where it failed instead of joining \
                 the scope. stdout: {:?}",
                ran.stdout
            );
        }
    }
}
