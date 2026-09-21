//! Structured concurrency, exercised through the public `run` entry point.
//!
//! # Every test has a deadline
//!
//! A concurrency bug's usual symptom is that nothing happens. Under a plain `#[test]`
//! that is a suite which hangs rather than a suite which fails: no name, no output, and
//! on CI a job killed at the job timeout with nothing to read. So every program here runs
//! on its own thread behind [`within`], and a program that does not finish in time fails
//! by name and says what it was waiting for.
//!
//! # What these tests assert
//!
//! Permitted outcomes and invariants, never a schedule. `docs/spec.md` promises no
//! fairness and no ordering between tasks, so a test that pinned one interleaving would
//! be asserting something the language does not offer — and would pass or fail with the
//! machine it ran on. Every assertion here is of the form "this total", "this many", "it
//! finished at all".

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::mpsc;
use std::time::Duration;

use nazm_core::{run, Outcome, Value};
use nazm_diag::codes;

/// How long any one program here is allowed to take.
///
/// Generous against a loaded machine and short against a human's patience: these
/// programs do microseconds of work, so anything approaching this is a hang, not
/// slowness.
const DEADLINE: Duration = Duration::from_secs(20);

/// Run `f` on its own thread and fail if it does not finish within [`DEADLINE`].
///
/// The thread is abandoned on a timeout rather than joined — joining is exactly the thing
/// that is not happening. It dies with the process; the test has already failed by then.
fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(what.to_string())
        .spawn(move || drop(tx.send(f())))
        .expect("could not start the test thread");
    match rx.recv_timeout(DEADLINE) {
        Ok(v) => v,
        Err(_) => panic!("{what} did not finish within {DEADLINE:?} — it deadlocked"),
    }
}

/// The value a program produces, under a deadline.
fn value(what: &'static str, src: &'static str) -> Value {
    within(what, move || match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    })
}

/// The single diagnostic code a program fails with, under a deadline.
fn sole_error(what: &'static str, src: &'static str) -> nazm_diag::Code {
    within(what, move || match run(src) {
        Outcome::CheckFailed(ds) => {
            assert_eq!(ds.len(), 1, "expected one error, got {ds:#?}");
            ds[0].code.clone()
        }
        Outcome::RuntimeFailed(d) => d.code,
        Outcome::Value(v) => panic!("expected an error, got {v}"),
    })
}

// ---------------------------------------------------------------------------
// A task runs while its scope's body does
//
// The first property, and the one the first implementation got wrong: it collected
// spawned tasks and started them at the closing brace. Every test below passes under
// that implementation except this one, which deadlocks — the body fills a bounded
// channel before any reader exists. It is the shape of every worker pool.
// ---------------------------------------------------------------------------

#[test]
fn a_worker_pool_finishes_work_the_body_is_still_producing() {
    let src = r"
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
    ";
    // Which worker takes which job is not decided; the sum is. 2 x (1 + ... + 10).
    assert_eq!(value("worker pool", src), Value::Int(110));
}

#[test]
fn a_channel_of_capacity_one_still_delivers_everything() {
    // The strongest back-pressure the language offers: the sender can be no more than one
    // message ahead of the receiver, so every send but the first blocks until a receive
    // makes room. Finishing at all is the assertion; the total is what says nothing was
    // dropped or delivered twice.
    let src = r"
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
    ";
    assert_eq!(value("capacity one", src), Value::Int(20100));
}

// ---------------------------------------------------------------------------
// The join
// ---------------------------------------------------------------------------

#[test]
fn a_scope_does_not_finish_until_every_task_has() {
    // Each task sends one message *after* doing its work. Reading exactly as many as were
    // spawned, from a channel closed after the scope, can only happen if the closing
    // brace waited: a close that ran early would end the drain short.
    let src = r"
        fn contribute(out: Chan, n: Int) -> Int {
            let mut i = 0;
            let mut acc = 0;
            while i < 1000 { acc = acc + n; i = i + 1; }
            chan_send(out, acc);
            0
        }
        fn main() -> Int {
            let out = chan_new(8);
            scope {
                spawn contribute(out, 1);
                spawn contribute(out, 2);
                spawn contribute(out, 3);
                spawn contribute(out, 4);
            }
            chan_close(out);
            let buf = ints_new();
            let mut seen = 0;
            let mut total = 0;
            while chan_recv(out, buf) { total = total + ints_pop(buf); seen = seen + 1; }
            if seen == 4 { total } else { 0 - seen }
        }
    ";
    assert_eq!(value("join", src), Value::Int(10_000));
}

#[test]
fn a_scope_with_no_tasks_is_an_ordinary_block() {
    let src = r"
        fn main() -> Int {
            let mut n = 1;
            scope { n = n + 41; }
            n
        }
    ";
    assert_eq!(value("empty scope", src), Value::Int(42));
}

#[test]
fn scopes_nest_and_the_inner_one_joins_first() {
    // The inner scope's tasks have finished by the time the outer body's next statement
    // runs, so `stage` is complete before the outer task is even started with it.
    let src = r"
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
    ";
    assert_eq!(value("nested scopes", src), Value::Int(30));
}

// ---------------------------------------------------------------------------
// Closing, which is the cancellation mechanism
// ---------------------------------------------------------------------------

#[test]
fn closing_a_channel_wakes_every_blocked_receiver() {
    // Four tasks blocked on a channel that never receives anything. Nothing but the close
    // can end them, and nothing but the wake-up can end the join. Without the deadline
    // this test would hang instead of failing.
    let src = r"
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
    ";
    assert_eq!(value("close wakes receivers", src), Value::Int(7));
}

#[test]
fn closing_a_full_channel_wakes_a_blocked_sender_and_reports_the_send_failed() {
    // The sender is blocked on a full channel; the close is what releases it, and the
    // value it was trying to send was not sent. A `chan_send` that reported success after
    // a close would silently lose the message.
    let src = r"
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
            // How many of the hundred were refused depends on how far the sender got
            // before the close landed — anywhere from 98 to 100. That it was released at
            // all, and that at least one send reported failure, is the promise.
            if ints_pop(buf) > 0 { 1 } else { 0 }
        }
    ";
    assert_eq!(value("close wakes sender", src), Value::Int(1));
}

#[test]
fn a_close_does_not_discard_what_is_already_queued() {
    let src = r"
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
    ";
    assert_eq!(value("close keeps the queue", src), Value::Int(7));
}

#[test]
fn closing_twice_is_allowed() {
    // Cancellation is something several tasks may reach at once. A close that failed the
    // second time would make the race a program error.
    let src = r"
        fn stop(c: Chan) -> Int { chan_close(c); 0 }
        fn main() -> Int {
            let c = chan_new(1);
            scope {
                spawn stop(c);
                spawn stop(c);
                chan_close(c);
            }
            let buf = ints_new();
            if chan_recv(c, buf) { 0 } else { 5 }
        }
    ";
    assert_eq!(value("idempotent close", src), Value::Int(5));
}

// ---------------------------------------------------------------------------
// Failure
// ---------------------------------------------------------------------------

#[test]
fn a_task_that_fails_fails_the_whole_program() {
    // `docs/spec.md`: a task has no private failure mode. The division is reported
    // exactly as it would be on the main thread.
    let src = r"
        fn bad(c: Chan) -> Int {
            let buf = ints_new();
            chan_recv(c, buf);
            let zero = ints_pop(buf);
            1 / zero
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
    assert_eq!(sole_error("failing task", src), codes::DIVISION_BY_ZERO);
}

#[test]
fn a_failing_task_does_not_stop_the_others_being_joined() {
    // One task fails immediately, the rest are still working. The scope reports the
    // failure — and only after every other task has finished, which is what stops a
    // failure from leaking a running task past its scope.
    let src = r"
        fn bad(c: Chan) -> Int {
            let buf = ints_new();
            chan_recv(c, buf);
            1 / ints_pop(buf)
        }
        fn slow(c: Chan) -> Int {
            let buf = ints_new();
            while chan_recv(c, buf) { ints_pop(buf); }
            0
        }
        fn main() -> Int {
            let dead = chan_new(1);
            let work = chan_new(1);
            scope {
                spawn bad(dead);
                spawn slow(work);
                spawn slow(work);
                chan_send(dead, 0);
                let mut i = 0;
                while i < 50 { chan_send(work, i); i = i + 1; }
                chan_close(work);
            }
            0
        }
    ";
    assert_eq!(sole_error("failure joins", src), codes::DIVISION_BY_ZERO);
}

#[test]
fn a_scope_whose_body_fails_reports_the_bodys_own_failure() {
    // The body overflows while a task is running, and the overflow is what comes out.
    //
    // This does **not** establish that the scope joined first: a run that joined and a run
    // that exited where it failed produce the same code and the same exit status, so the
    // code alone cannot tell them apart. That needs an observable effect from the task,
    // which needs a subprocess to observe stdout — it lives in
    // `nazm-cli/tests/concurrent.rs`, where the interpreter and both optimisation levels
    // are run the same way.
    let src = r"
        fn drain(c: Chan) -> Int {
            let buf = ints_new();
            while chan_recv(c, buf) { ints_pop(buf); }
            0
        }
        fn main() -> Int {
            let work = chan_new(1);
            let mut top = 1;
            scope {
                spawn drain(work);
                let mut i = 0;
                while i < 20 { chan_send(work, i); i = i + 1; }
                chan_close(work);
                let mut n = 0;
                while n < 64 { top = top + top; n = n + 1; }
            }
            top
        }
    ";
    assert_eq!(sole_error("body fails", src), codes::INTEGER_OVERFLOW);
}

#[test]
fn when_both_the_body_and_a_task_fail_the_bodys_failure_is_the_one_reported() {
    // Two different codes so the answer is legible: the task divides by zero (N0401),
    // the body overflows (N0400). `docs/spec.md` says a task that failed because the body
    // stopped feeding it is a consequence rather than a cause, so the body's failure wins.
    // The order does not depend on which happened first — the scope joins, then reports
    // the body's.
    let src = r"
        fn bad(c: Chan) -> Int {
            let buf = ints_new();
            chan_recv(c, buf);
            1 / ints_pop(buf)
        }
        fn main() -> Int {
            let c = chan_new(1);
            let mut top = 1;
            scope {
                spawn bad(c);
                chan_send(c, 0);
                let mut n = 0;
                while n < 64 { top = top + top; n = n + 1; }
            }
            top
        }
    ";
    assert_eq!(sole_error("both fail", src), codes::INTEGER_OVERFLOW);
}

// ---------------------------------------------------------------------------
// What the checker refuses, so that the interpreter never has to decide
// ---------------------------------------------------------------------------

#[test]
fn spawn_outside_a_scope_is_refused() {
    let src = r"
        fn t() -> Int { 0 }
        fn main() -> Int { spawn t(); 0 }
    ";
    assert_eq!(
        sole_error("spawn outside scope", src),
        codes::SPAWN_OUTSIDE_SCOPE
    );
}

#[test]
fn a_sequence_may_not_cross_into_a_task() {
    // The capture rule. A sequence is a handle to unsynchronised shared storage; letting
    // one through would be a data race the language had invited.
    let src = r"
        fn t(xs: Ints) -> Int { ints_push(xs, 1); 0 }
        fn main() -> Int {
            let xs = ints_new();
            scope { spawn t(xs); }
            ints_len(xs)
        }
    ";
    assert_eq!(
        sole_error("sequence capture", src),
        codes::ILLEGAL_TASK_CAPTURE
    );
}

// ---------------------------------------------------------------------------
// Stress: the invariant has to survive contention, not just a single pass
// ---------------------------------------------------------------------------

#[test]
fn every_message_is_delivered_exactly_once_under_contention() {
    // Three producers and three consumers on a channel of capacity one, which forces a
    // hand-off on every single message. 3 x (1 + ... + 300) = 135450. A message delivered
    // twice or lost changes that total; no ordering is asserted, because none is
    // promised.
    let src = r"
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
                // The consumers belong to the *outer* scope: they have to outlive the
                // inner one, because the inner one is what waits for the producers and
                // the producers cannot finish until somebody is taking messages.
                spawn consume(jobs, results);
                spawn consume(jobs, results);
                spawn consume(jobs, results);
                scope {
                    spawn produce(jobs);
                    spawn produce(jobs);
                    spawn produce(jobs);
                }
                // Every producer has finished, so closing now cannot cut one short —
                // the inner scope is what makes that true.
                chan_close(jobs);
            }
            chan_close(results);
            let buf = ints_new();
            let mut total = 0;
            while chan_recv(results, buf) { total = total + ints_pop(buf); }
            total
        }
    ";
    assert_eq!(value("contention", src), Value::Int(135_450));
}

#[test]
fn many_scopes_in_sequence_leave_nothing_running() {
    // A thousand tasks over two hundred scopes. If a scope ever returned without joining,
    // the threads would pile up and the total would be short.
    let src = r"
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
    ";
    assert_eq!(value("scope cleanup", src), Value::Int(1000));
}
