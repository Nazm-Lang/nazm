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
use std::process::Command;
use std::time::Duration;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Long enough for the stress cases on a loaded machine, short enough that a deadlock is
/// reported rather than waited on.
const DEADLINE: Duration = Duration::from_secs(60);

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// A hung compiled program must fail this suite rather than hang it: `common::run_within`, which
/// drains both streams as the child runs, with this suite's own "did not terminate" wording.
fn run_bounded(cmd: &mut Command) -> Option<Ran> {
    // `common`'s draining run (Q1): this copy read the pipes only after the child exited.
    common::run_within(cmd, DEADLINE).map(|r| Ran {
        code: r.code,
        stdout: r.stdout,
        stderr: r.stderr,
    })
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn early(out: Chan, tasks: SpawnCap) -> Int {
            scope {
                spawn slow(out, 5);
                spawn slow(out, 5);
                return 1;
            }
            0
        }
        fn main(tasks: SpawnCap) -> Int {
            let out = chan_new(8);
            early(out, tasks);
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
        fn main(tasks: SpawnCap) -> Int {
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
        fn main(tasks: SpawnCap) -> Int {
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
// A failure waits for the scope's tasks
//
// `docs/spec.md` promises the closing brace joins every task *including when the body
// itself failed*, so a failure is reported once nothing is still running. The diagnostic
// code cannot check that: a run that joined first and a run that exited on the spot
// produce the same code and the same exit status. The difference is only visible in
// whether the task's own effect happened.
//
// # Why the ordering is deterministic
//
// An earlier version of these programs gave the worker a long counting loop and relied on
// it still being inside it when the failure happened. That is not ordering, it is a race
// that usually goes one way — a slower machine, a busier one, or a different optimisation
// level could finish the loop first and the test would pass on a runtime that had not
// joined anything.
//
// These use the channels instead. The worker announces itself on `started` and then
// blocks on `go`; the body cannot reach its failure until it has *received* that
// announcement, so the worker is provably running, and it cannot finish until the body
// sends `go`, so it is provably unfinished. Both facts are enforced by `chan_recv`
// blocking, not by how fast anything runs.
// ---------------------------------------------------------------------------

/// The worker half every program below shares.
const WORKER: &str = r#"
fn worker(started: Chan, go: Chan, out: OutCap) -> Int {
    let buf = ints_new();
    chan_send(started, 1);
    chan_recv(go, buf);
    ints_pop(buf);
    print("worker finished");
    0
}
"#;

/// The body overflows while a worker it has just released is still running.
const BODY_FAILS: &str = r#"
fn main(io: IoCap, tasks: SpawnCap) -> Int {
    let started = chan_new(1);
    let go = chan_new(1);
    let buf = ints_new();
    let mut top = 1;
    scope {
        spawn worker(started, go, io);
        chan_recv(started, buf);
        ints_pop(buf);
        chan_send(go, 1);
        let mut n = 0;
        while n < 64 { top = top + top; n = n + 1; }
    }
    top
}
"#;

/// A sibling task divides by zero while the worker is still running.
const TASK_FAILS: &str = r#"
fn bad(dead: Chan) -> Int {
    let buf = ints_new();
    chan_recv(dead, buf);
    1 / ints_pop(buf)
}
fn main(io: IoCap, tasks: SpawnCap) -> Int {
    let started = chan_new(1);
    let go = chan_new(1);
    let dead = chan_new(1);
    let buf = ints_new();
    scope {
        spawn worker(started, go, io);
        spawn bad(dead);
        chan_recv(started, buf);
        ints_pop(buf);
        chan_send(go, 1);
        chan_send(dead, 0);
    }
    0
}
"#;

/// The failure is two calls below the scope's body, so it has to travel out through both
/// before the join can happen.
const HELPER_FAILS: &str = r#"
fn divide(a: Int, b: Int) -> Int { a / b }
fn deeper(a: Int, b: Int) -> Int { divide(a, b) + 1 }
fn main(io: IoCap, tasks: SpawnCap) -> Int {
    let started = chan_new(1);
    let go = chan_new(1);
    let buf = ints_new();
    let mut r = 0;
    scope {
        spawn worker(started, go, io);
        chan_recv(started, buf);
        ints_pop(buf);
        chan_send(go, 1);
        r = deeper(1, 0);
    }
    r
}
"#;

const MARKER: &str = "worker finished";

/// One program, run three ways, all three required to agree.
fn failure_joins(what: &str, src: &str, code: &str, markers: &[&str]) {
    let (dir, path) = write_program(src);

    let mut runs: Vec<(String, Ran)> = Vec::new();
    runs.push((
        "interpreted".to_owned(),
        run_bounded(Command::new(NAZM).args(["run", path.to_str().unwrap()]))
            .unwrap_or_else(|| panic!("{what}: `nazm run` did not finish")),
    ));
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
        runs.push((
            format!("-O{opt}"),
            run_bounded(&mut common::bounded_exe(&exe))
                .unwrap_or_else(|| panic!("{what}: the -O{opt} executable did not finish")),
        ));
    }

    for (how, ran) in &runs {
        assert_eq!(ran.code, Some(2), "{what} ({how}): {}", ran.stderr);
        assert!(
            ran.stderr.contains(code),
            "{what} ({how}): expected {code}, got {}",
            ran.stderr
        );
        for marker in markers {
            assert!(
                ran.stdout.contains(marker),
                "{what} ({how}): {marker:?} is missing from stdout, which is what a \
                 failure that did not join its scope looks like — the task was ended \
                 where it stood instead of being waited for. stdout: {:?}",
                ran.stdout
            );
        }
    }
}

#[test]
fn a_failing_body_joins_its_tasks_before_reporting() {
    failure_joins(
        "body fails",
        &format!("{WORKER}{BODY_FAILS}"),
        "N0400",
        &[MARKER],
    );
}

#[test]
fn a_failing_task_lets_its_siblings_finish_before_reporting() {
    failure_joins(
        "task fails",
        &format!("{WORKER}{TASK_FAILS}"),
        "N0401",
        &[MARKER],
    );
}

#[test]
fn a_failure_below_the_body_still_joins_the_scope() {
    failure_joins(
        "helper fails",
        &format!("{WORKER}{HELPER_FAILS}"),
        "N0401",
        &[MARKER],
    );
}

#[test]
fn nested_scopes_each_join_before_the_failure_is_reported() {
    // Two scopes, a worker in each, and the failure in the inner body. Both workers must
    // finish: the inner scope's join is reached on the way out, and the outer's after it.
    let src = r#"
fn worker(started: Chan, go: Chan, which: Int, out: OutCap) -> Int {
    let buf = ints_new();
    chan_send(started, 1);
    chan_recv(go, buf);
    ints_pop(buf);
    if which == 1 { print("inner finished"); } else { print("outer finished"); }
    0
}
fn main(io: IoCap, tasks: SpawnCap) -> Int {
    let os = chan_new(1);
    let og = chan_new(1);
    let is = chan_new(1);
    let ig = chan_new(1);
    let buf = ints_new();
    let mut top = 1;
    scope {
        spawn worker(os, og, 2, io);
        chan_recv(os, buf);
        ints_pop(buf);
        chan_send(og, 1);
        scope {
            spawn worker(is, ig, 1, io);
            chan_recv(is, buf);
            ints_pop(buf);
            chan_send(ig, 1);
            let mut n = 0;
            while n < 64 { top = top + top; n = n + 1; }
        }
    }
    top
}
"#;
    failure_joins(
        "nested scopes",
        src,
        "N0400",
        &["inner finished", "outer finished"],
    );
}

#[test]
fn when_both_the_body_and_a_task_fail_the_bodys_failure_is_reported() {
    // The task's failure strictly *precedes* the body's: the body does not begin
    // overflowing until it has received `done`, which the task sends immediately before
    // dividing by zero. So this cannot pass by reporting whichever happened first —
    // `docs/spec.md` says the body's is the cause and the task's the consequence.
    let src = r#"
fn bad(dead: Chan, done: Chan) -> Int {
    let buf = ints_new();
    chan_recv(dead, buf);
    let z = ints_pop(buf);
    chan_send(done, 1);
    1 / z
}
fn main(tasks: SpawnCap) -> Int {
    let dead = chan_new(1);
    let done = chan_new(1);
    let buf = ints_new();
    let mut top = 1;
    scope {
        spawn bad(dead, done);
        chan_send(dead, 0);
        chan_recv(done, buf);
        ints_pop(buf);
        let mut n = 0;
        while n < 64 { top = top + top; n = n + 1; }
    }
    top
}
"#;
    failure_joins("both fail", src, "N0400", &[]);
}

#[test]
fn a_scope_that_joins_cleanly_still_produces_its_value() {
    // The other half: nothing above may be achieved by reporting a failure that is not
    // there. An uninitialised slot in a task's argument block would do exactly that.
    agrees(
        "clean scope",
        r"
fn w(c: Chan) -> Int { chan_send(c, 5); 0 }
fn main(tasks: SpawnCap) -> Int {
    let c = chan_new(2);
    let buf = ints_new();
    let mut t = 0;
    scope {
        spawn w(c);
        spawn w(c);
        chan_recv(c, buf);
        t = t + ints_pop(buf);
        chan_recv(c, buf);
        t = t + ints_pop(buf);
    }
    t
}
",
        "10",
    );
}
