//! N12.1: a block used as a value — a `match` arm, a `let` initialiser, an argument, a
//! field — compiled by `nazm build` exactly as `nazm run` evaluates it.
//!
//! # What is being held
//!
//! The language has one `Block`: statements, then an optional tail, and a completion that
//! is `Value`, `Unit` or `Diverges`. The interpreter always evaluated a block in any
//! position; the native backend refused one anywhere but a function body and an `if`
//! branch. Nothing about the language changes here. What these cases hold is that the
//! compiled program now agrees with the interpreted one about the value **and** about
//! every memory counter, in the positions the refusal used to cover:
//!
//! * a block's result is established before anything that backs it could be let go — a
//!   `Str` tail survives its local, a payload survives its pattern binding and the
//!   scrutinee, a `Vec` tail is the same handle and not a copy;
//! * a path through a block that transfers contributes no value: an arm that returns,
//!   breaks, continues or propagates with `?` is not a predecessor of the join;
//! * a transfer out of a block in an argument or a field initialiser stops everything after
//!   it and reclaims what was made before it.
//!
//! # Why the churn
//!
//! A block released one step too early is often still readable. Allocating and discarding
//! small strings afterwards hands a prematurely freed block back out, and a wrong answer or
//! a signal shows it where the counters alone would not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod memory_report;

use memory_report::{counts, Ran};

use std::process::Command;

use nazm_lir::MEMORY_VARIABLE;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Interpret it, compile and run it, and require the two to agree about everything.
/// `code` is the exit status both must have: `0`, or `2` for a runtime failure.
fn both_exiting(source: &str, code: i32) -> (Ran, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let interpreted = Command::new(NAZM)
        .args(["run", "m.nz"])
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&interpreted.stderr).into_owned();
    assert_eq!(
        interpreted.status.code(),
        Some(code),
        "interpreting: {stderr}"
    );
    let a = counts(
        &stderr,
        String::from_utf8_lossy(&interpreted.stdout)
            .trim()
            .to_owned(),
    );

    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "building failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let (status, native_out, native_err) = run_with_deadline(dir.path());
    assert_eq!(status, Some(code), "the program: {native_err}");
    let b = counts(&native_err, native_out.trim().to_owned());

    assert_eq!(a, b, "the two implementations disagree");
    (a, native_err)
}

/// Run the compiled `prog` under a deadline: `(status, stdout, stderr)`.
///
/// A compiled program that never finishes is a wrong answer, not a slow one — a lowering
/// that drops a loop's increment produces exactly that — so it fails the case here rather
/// than holding the suite until some outer limit kills it with no verdict. The output goes
/// to files, so a program that writes a lot cannot block on a full pipe while we wait.
fn run_with_deadline(dir: &std::path::Path) -> (Option<i32>, String, String) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(60);
    let out_path = dir.join("prog.out");
    let err_path = dir.join("prog.err");
    let mut child = Command::new(dir.join("prog"))
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir)
        .stdout(std::fs::File::create(&out_path).expect("stdout file"))
        .stderr(std::fs::File::create(&err_path).expect("stderr file"))
        .spawn()
        .expect("execute");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the compiled program did not finish within {LIMIT:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    (
        status.code(),
        std::fs::read_to_string(&out_path).unwrap_or_default(),
        std::fs::read_to_string(&err_path).unwrap_or_default(),
    )
}

fn both(source: &str) -> Ran {
    let (r, stderr) = both_exiting(source, 0);
    assert_eq!(stderr.lines().count(), 1, "unexpected stderr: {stderr}");
    r
}

/// Everything allocated was reclaimed.
fn clean(r: &Ran) {
    for (name, c) in [
        ("sequences", r.sequences),
        ("strings", r.strings),
        ("channels", r.channels),
    ] {
        assert_eq!(c.live, 0, "{name} left live: {r:?}");
        assert_eq!(c.allocated, c.reclaimed, "{name}: {r:?}");
    }
}

/// The codes `nazm check` refuses `source` with.
fn refused(source: &str) -> Vec<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");
    let out = Command::new(NAZM)
        .args(["check", "--no-cache", "m.nz", "--json"])
        .current_dir(dir.path())
        .output()
        .expect("check");
    assert!(!out.status.success(), "accepted: {source}");
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

/// Small strings made and thrown away, so that a block released too early is reused.
const CHURN: &str = "fn churn() -> Int {\n    let mut noise = vec_new[Str]();\n    let mut k = 0;\n    \
    while k < 64 {\n        vec_push(noise, str_concat(\"noise-\", int_to_str(k)));\n        k = k + 1;\n    }\n    \
    noise = vec_new[Str]();\n    vec_len(noise)\n}\n";

/// An owning record and an owning enum.
const TYPES: &str = "struct Item {\n    name: Str,\n    tags: Ints,\n}\n\
    enum Shape {\n    Named(label: Str),\n    Plain,\n}\n\
    fn item(n: Int) -> Item {\n    let tags = ints_new();\n    ints_push(tags, n);\n    \
    Item(name: str_concat(\"item-\", int_to_str(n)), tags: tags)\n}\n";

fn with(body: &str) -> String {
    format!("{TYPES}{CHURN}{body}")
}

// --- a block's result survives what backed it ---------------------------------------------

#[test]
fn a_str_tail_survives_its_local() {
    // §38. `t` is the block's local; the string it names is the block's value, and it is
    // still readable after the churn has reused anything released too soon. In a loop, so
    // the `let` inside the block re-executes over the same slot.
    let r = both(&with(
        "fn make(n: Int) -> Str { str_concat(\"text-\", int_to_str(n)) }\n\
         fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    while i < 5 {\n        \
         let s = {\n            let t = make(i * 111);\n            t\n        };\n        churn();\n        \
         total = total + str_len(s);\n        i = i + 1;\n    }\n    total\n}\n",
    ));
    // "text-0", then four of "text-NNN".
    assert_eq!(r.value, "38");
    clean(&r);
}

#[test]
fn a_record_tail_keeps_its_owning_fields() {
    let r = both(&with(
        "fn main() -> Int {\n    let it = {\n        let a = item(7);\n        let b = item(8);\n        a\n    };\n    \
         churn();\n    str_len(it.name) * 100 + ints_get(it.tags, 0)\n}\n",
    ));
    assert_eq!(r.value, "607");
    clean(&r);
}

#[test]
fn an_enum_tail_keeps_its_active_payload() {
    let r = both(&with(
        "fn main() -> Int {\n    let sh = {\n        let txt = str_concat(\"lab\", int_to_str(1234));\n        \
         let made = Shape.Named(label: txt);\n        made\n    };\n    churn();\n    \
         match sh { Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }\n}\n",
    ));
    assert_eq!(r.value, "7");
    clean(&r);
}

#[test]
fn a_vec_tail_is_the_same_handle_and_not_a_copy() {
    // §39. A push through the block's result is visible through the binding it came from,
    // and there is one vector — `churn` accounts for the other two.
    let r = both(&with(
        "fn main() -> Int {\n    let existing = vec_new[Str]();\n    vec_push(existing, int_to_str(5));\n    \
         let xs = {\n        let ys = existing;\n        ys\n    };\n    vec_push(xs, int_to_str(66));\n    churn();\n    \
         vec_len(existing) * 10 + str_len(vec_get(existing, 1))\n}\n",
    ));
    assert_eq!(r.value, "22");
    clean(&r);
    assert_eq!(r.sequences.allocated, 3, "{r:?}");
}

#[test]
fn a_pattern_binding_returned_through_an_arm_block_outlives_the_scrutinee() {
    // §17 and §40: `Option[Str]`, `Result[Item, Str]` and an enum carrying a `Vec`. Each
    // arm's block rebinds the payload and hands the local back; the scrutinee, the pattern
    // binding and the arm's local are all gone by the time the churn runs.
    let r = both(&with(
        "enum Bag {\n    Full(items: Vec[Str]),\n    Empty,\n}\n\
         fn text(o: Option[Str]) -> Str {\n    match o {\n        \
         Option.None() => {\n            let d = str_concat(\"no\", \"ne\");\n            d\n        },\n        \
         Option.Some(value: s) => {\n            let x = s;\n            x\n        },\n    }\n}\n\
         fn thing(r: Result[Item, Str]) -> Item {\n    match r {\n        \
         Result.Ok(value: i) => {\n            let kept = i;\n            kept\n        },\n        \
         Result.Err(error: e) => {\n            let fallback = item(str_len(e));\n            fallback\n        },\n    }\n}\n\
         fn bag(b: Bag) -> Vec[Str] {\n    match b {\n        \
         Bag.Full(items: xs) => {\n            let ys = xs;\n            ys\n        },\n        \
         Bag.Empty() => {\n            let fresh = vec_new[Str]();\n            vec_push(fresh, int_to_str(0));\n            fresh\n        },\n    }\n}\n\
         fn main() -> Int {\n    let a = text(Option[Str].Some(value: str_concat(\"some-\", int_to_str(42))));\n    \
         let b = text(Option[Str].None());\n    churn();\n    \
         let i = thing(Result[Item, Str].Ok(value: item(3)));\n    \
         let j = thing(Result[Item, Str].Err(error: int_to_str(12345)));\n    churn();\n    \
         let full = vec_new[Str]();\n    vec_push(full, int_to_str(777));\n    vec_push(full, int_to_str(8));\n    \
         let c = bag(Bag.Full(items: full));\n    let d = bag(Bag.Empty());\n    churn();\n    \
         str_len(a) * 1000000 + str_len(b) * 100000 + ints_get(i.tags, 0) * 10000 + ints_get(j.tags, 0) * 1000 \
         + vec_len(c) * 100 + str_len(vec_get(c, 0)) * 10 + vec_len(d)\n}\n",
    ));
    // "some-42" = 7, "none" = 4, item(3), item(len "12345" = 5), ["777", "8"], ["0"].
    assert_eq!(r.value, "7435231");
    clean(&r);
}

#[test]
fn a_generic_arm_block_moves_a_t_and_nothing_more() {
    // §26. `choose` checks parametrically and does nothing to its `T` but hand it on;
    // three instances, each reclaimed.
    let r = both(&with(
        "fn choose[T](x: Option[T], fallback: T) -> T {\n    match x {\n        \
         Option.None() => {\n            let f = fallback;\n            f\n        },\n        \
         Option.Some(value: v) => {\n            let w = v;\n            w\n        },\n    }\n}\n\
         fn main() -> Int {\n    let a = choose(Option[Str].None(), int_to_str(1234));\n    \
         let b = choose(Option[Str].Some(value: str_concat(\"x\", \"yz\")), int_to_str(5));\n    \
         let i = choose(Option[Item].Some(value: item(6)), item(9));\n    \
         let v = choose(Option[Vec[Str]].None(), vec_new[Str]());\n    churn();\n    \
         str_len(a) * 1000 + str_len(b) * 100 + ints_get(i.tags, 0) * 10 + vec_len(v)\n}\n",
    ));
    assert_eq!(r.value, "4360");
    clean(&r);
}

// --- a path that transfers contributes nothing --------------------------------------------

#[test]
fn a_question_mark_inside_an_arm_block_cleans_the_arm_and_joins_its_scope() {
    // §30, §31 and §42: `?` inside a `scope` inside an arm block. On `Err` the arm's
    // locals, the scope's local and the `match` are all released, and the task is joined
    // before the function returns — its send arrives.
    let r = both(&with(
        "fn worker(out: Chan, tag: Str) -> Int {\n    let mut i = 0;\n    let mut s = 0;\n    \
         while i < 200000 { s = s + i % 3; i = i + 1; }\n    chan_send(out, str_len(tag));\n    s\n}\n\
         fn g(ok: Bool) -> Result[Int, Str] {\n    if ok { return Result[Int, Str].Ok(value: 41); }\n    \
         Result[Int, Str].Err(error: int_to_str(404))\n}\n\
         enum State {\n    A,\n    B,\n}\n\
         fn f(s: State, ok: Bool, out: Chan, tasks: SpawnCap) -> Result[Int, Str] {\n    let x = match s {\n        \
         State.A() => {\n            let held = str_concat(\"held-\", int_to_str(1));\n            let mut n = 0;\n            \
         scope {\n                spawn worker(out, held);\n                let inner = int_to_str(99);\n                \
         n = g(ok)? + str_len(inner);\n            }\n            n + str_len(held)\n        },\n        \
         State.B() => {\n            0\n        },\n    };\n    Result[Int, Str].Ok(value: x)\n}\n\
         fn main(tasks: SpawnCap) -> Int {\n    let out = chan_new(8);\n    let a = f(State.A(), true, out, tasks);\n    \
         let b = f(State.A(), false, out, tasks);\n    let c = f(State.B(), false, out, tasks);\n    chan_close(out);\n    \
         let buf = ints_new();\n    let got = chan_recv(out, buf);\n    let more = chan_recv(out, buf);\n    \
         let rest = chan_recv(out, buf);\n    churn();\n    \
         let va = match a { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, };\n    \
         let vb = match b { Result.Ok(value: v) => v, Result.Err(error: e) => str_len(e), };\n    \
         let vc = match c { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, };\n    \
         va * 10000 + vb * 1000 + vc * 100 + ints_len(buf) * 10 + ints_get(buf, 0)\n}\n",
    ));
    // a = 41 + len "99" + len "held-1" = 49; b's error "404"; c = 0; both tasks sent 6.
    assert_eq!(r.value, "493026");
    clean(&r);
}

#[test]
fn a_three_arm_match_joins_only_the_arm_that_arrives() {
    // §45: one arm returns, one produces a value from a block, one propagates with `?`.
    // Only the second is a predecessor of the join.
    let r = both(&with(
        "enum E {\n    A,\n    B,\n    C,\n}\n\
         fn g(ok: Bool) -> Result[Int, Str] {\n    if ok { return Result[Int, Str].Ok(value: 5); }\n    \
         Result[Int, Str].Err(error: str_concat(\"x\", \"y\"))\n}\n\
         fn f(e: E, ok: Bool, tasks: SpawnCap) -> Result[Int, Str] {\n    let v = match e {\n        \
         E.A() => {\n            let gone = int_to_str(1);\n            return Result[Int, Str].Ok(value: 100);\n        },\n        \
         E.B() => {\n            let s = str_concat(\"ab\", \"c\");\n            str_len(s)\n        },\n        \
         E.C() => {\n            let held = int_to_str(22);\n            g(ok)? + str_len(held)\n        },\n    };\n    \
         Result[Int, Str].Ok(value: v)\n}\n\
         fn show(r: Result[Int, Str]) -> Int {\n    match r { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - str_len(e), }\n}\n\
         fn main(tasks: SpawnCap) -> Int {\n    let a = show(f(E.A(), true, tasks));\n    let b = show(f(E.B(), true, tasks));\n    \
         let c = show(f(E.C(), true, tasks));\n    let d = show(f(E.C(), false, tasks));\n    churn();\n    \
         a * 10000 + b * 1000 + c * 10 + d\n}\n",
    ));
    // 100, 3, 5 + 2 = 7, and "xy" propagated: -2.
    assert_eq!(r.value, "1003068");
    clean(&r);
}

#[test]
fn a_match_whose_arms_all_leave_produces_no_value() {
    // §19: no arm reaches the join, so there is no join; each arm's owning local is
    // released by the `return` that leaves.
    let r = both(&with(
        "fn f(o: Option[Str], tasks: SpawnCap) -> Int {\n    let n = match o {\n        \
         Option.None() => {\n            let s = int_to_str(11);\n            return str_len(s);\n        },\n        \
         Option.Some(value: v) => {\n            let w = v;\n            return str_len(w) * 10;\n        },\n    };\n    n\n}\n\
         fn main(tasks: SpawnCap) -> Int {\n    let a = f(Option[Str].None(), tasks);\n    \
         let b = f(Option[Str].Some(value: str_concat(\"ab\", \"c\")), tasks);\n    churn();\n    a + b\n}\n",
    ));
    assert_eq!(r.value, "32");
    clean(&r);
}

#[test]
fn break_and_continue_leave_an_arm_block_with_no_value() {
    // §44. The `None` arm continues, the `Some` arm breaks on 8; neither contributes to the
    // `match`'s value on those paths, and what they had made is released.
    let r = both(&with(
        "fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    while i < 10 {\n        i = i + 1;\n        \
         let o = if i % 2 == 0 { Option[Str].Some(value: int_to_str(i * 100)) } else { Option[Str].None() };\n        \
         let v = match o {\n            Option.None() => {\n                let wasted = str_concat(\"skip-\", int_to_str(i));\n                \
         continue;\n            },\n            Option.Some(value: s) => {\n                let kept = s;\n                \
         if i == 8 { break; }\n                kept\n            },\n        };\n        churn();\n        \
         total = total + str_len(v);\n    }\n    total\n}\n",
    ));
    // "200", "400", "600", then the break at 8.
    assert_eq!(r.value, "9");
    clean(&r);
}

#[test]
fn a_return_inside_a_bare_block_leaves_the_function() {
    // §43.
    let r = both(&with(
        "fn pick(n: Int) -> Str {\n    let s = {\n        let a = str_concat(\"a-\", int_to_str(n));\n        \
         if n > 5 { return a; }\n        let b = str_concat(a, \"-b\");\n        b\n    };\n    str_concat(s, \"!\")\n}\n\
         fn main() -> Int {\n    let x = pick(3);\n    let y = pick(9);\n    churn();\n    str_len(x) * 100 + str_len(y)\n}\n",
    ));
    assert_eq!(r.value, "603");
    clean(&r);
}

// --- composed with arguments and constructions --------------------------------------------

#[test]
fn a_block_argument_that_leaves_stops_the_call_and_reclaims_the_argument_before_it() {
    // §23: left to right; the block's `return` releases the first argument's string,
    // `later` never runs and `consume` is never called.
    let r = both(&with(
        "fn later(log: Ints) -> Str {\n    ints_push(log, 1);\n    str_concat(\"late\", \"r\")\n}\n\
         fn consume(a: Str, b: Str, c: Str) -> Int { str_len(a) + str_len(b) + str_len(c) }\n\
         fn go(fail: Bool, log: Ints) -> Int {\n    \
         consume(int_to_str(12345), {\n        let t = str_concat(\"mid-\", int_to_str(1));\n        \
         if fail { return 0 - 1; }\n        t\n    }, later(log))\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    let a = go(false, log);\n    let b = go(true, log);\n    \
         churn();\n    a * 1000 + (b + 1) * 100 + ints_len(log)\n}\n",
    ));
    assert_eq!(r.value, "15001");
    clean(&r);
}

#[test]
fn a_block_initialiser_that_leaves_releases_the_half_built_value() {
    // §24 and §25: a record whose second field's block returns, and a variant whose one
    // field's block returns. The first field's string and the block's own sequence are
    // released; no value is completed.
    let r = both(&with(
        "fn record(fail: Bool) -> Int {\n    \
         let it = Item(name: str_concat(\"nm-\", int_to_str(7)), tags: {\n        let ts = ints_new();\n        \
         ints_push(ts, 3);\n        if fail { return 0 - 1; }\n        ts\n    });\n    \
         str_len(it.name) * 10 + ints_get(it.tags, 0)\n}\n\
         fn variant(fail: Bool) -> Int {\n    \
         let sh = Shape.Named(label: {\n        let l = int_to_str(4444);\n        if fail { return 0 - 1; }\n        l\n    });\n    \
         match sh { Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }\n}\n\
         fn main() -> Int {\n    let a = record(false);\n    let b = record(true);\n    \
         let c = variant(false);\n    let d = variant(true);\n    churn();\n    \
         a * 1000 + (b + 1) * 100 + c * 10 + (d + 1)\n}\n",
    ));
    assert_eq!(r.value, "43040");
    clean(&r);
}

#[test]
fn a_block_producing_a_result_is_propagated_once() {
    // §32: `(if …)?`, `({ … })?` and `(match … { … => { … } })?`. Each operand runs once —
    // the log counts the calls — and an `Err` from one stops the rest.
    let r = both(&with(
        "fn mk(log: Ints, ok: Bool) -> Result[Str, Str] {\n    ints_push(log, 1);\n    \
         if ok { return Result[Str, Str].Ok(value: int_to_str(4321)); }\n    \
         Result[Str, Str].Err(error: int_to_str(9))\n}\n\
         fn f(log: Ints, c: Bool, ok: Bool, tasks: SpawnCap) -> Result[Int, Str] {\n    \
         let a = (if c { mk(log, ok) } else { mk(log, true) })?;\n    \
         let b = ({\n        let r = mk(log, ok);\n        r\n    })?;\n    \
         let m = (match Option[Int].Some(value: 1) {\n        Option.None() => {\n            mk(log, true)\n        },\n        \
         Option.Some(value: q) => {\n            let r2 = mk(log, ok);\n            r2\n        },\n    })?;\n    \
         Result[Int, Str].Ok(value: str_len(a) + str_len(b) + str_len(m))\n}\n\
         fn main(tasks: SpawnCap) -> Int {\n    let log = ints_new();\n    let x = f(log, true, true, tasks);\n    let y = f(log, true, false, tasks);\n    \
         churn();\n    let vx = match x { Result.Ok(value: n) => n, Result.Err(error: e) => 0 - 1, };\n    \
         let vy = match y { Result.Ok(value: n) => 0 - 1, Result.Err(error: e) => str_len(e), };\n    \
         vx * 1000 + vy * 100 + ints_len(log)\n}\n",
    ));
    assert_eq!(r.value, "12104");
    clean(&r);
}

// --- statement position, failure, and the `if` controls -----------------------------------

#[test]
fn a_statement_match_whose_arms_produce_nothing_assigns_in_place() {
    // An arm block with no tail is `Unit`, which a statement-position `match` accepts. The
    // assignment releases the old string and keeps the new one.
    let r = both(&with(
        "fn main() -> Int {\n    let mut name = str_concat(\"start\", \"\");\n    let mut i = 0;\n    while i < 6 {\n        \
         let o = if i % 3 == 0 { Option[Str].None() } else { Option[Str].Some(value: int_to_str(i * 1000)) };\n        \
         match o {\n            Option.None() => {\n                name = str_concat(name, \".\");\n            },\n            \
         Option.Some(value: s) => {\n                let t = s;\n                name = t;\n            },\n        };\n        \
         i = i + 1;\n    }\n    {\n        let tail = str_concat(name, \"\");\n        name = tail;\n    }\n    \
         churn();\n    str_len(name)\n}\n",
    ));
    assert_eq!(r.value, "4");
    clean(&r);
}

#[test]
fn a_failure_inside_an_arm_block_releases_the_arm_and_the_match() {
    // §41: the division fails with the arm's string and sequence live, and the scrutinee
    // held by the `match`. Each is released exactly once on the way out.
    let (r, stderr) = both_exiting(
        "fn main() -> Int {\n    let o = Option[Ints].Some(value: ints_new());\n    let v = match o {\n        \
         Option.None() => {\n            0\n        },\n        Option.Some(value: zs) => {\n            \
         let held = str_concat(\"held\", int_to_str(1));\n            let xs = ints_new();\n            \
         ints_push(xs, 5);\n            ints_get(xs, 0) / ints_len(zs)\n        },\n    };\n    v\n}\n",
        2,
    );
    assert!(stderr.contains("N0401"), "{stderr}");
    clean(&r);
    assert_eq!(r.sequences.allocated, 2, "{r:?}");
    assert_eq!(r.strings.allocated, 2, "{r:?}");
}

#[test]
fn if_blocks_keep_every_completion_pairing() {
    // §21: Value/Value, Value/Diverges, Diverges/Value and Diverges/Diverges, each with
    // statements before the tail. The block work must not have moved N10.2.
    let r = both(&with(
        "fn vv(c: Bool) -> Str {\n    if c {\n        let a = int_to_str(1);\n        a\n    } else {\n        \
         let b = int_to_str(22);\n        b\n    }\n}\n\
         fn vd(c: Bool) -> Str {\n    let s = if c {\n        let a = int_to_str(333);\n        a\n    } else {\n        \
         let b = int_to_str(0);\n        return b;\n    };\n    s\n}\n\
         fn dv(c: Bool) -> Str {\n    let s = if c {\n        let a = int_to_str(0);\n        return a;\n    } else {\n        \
         let b = int_to_str(4444);\n        b\n    };\n    s\n}\n\
         fn dd(c: Bool) -> Str {\n    if c {\n        return int_to_str(55555);\n    } else {\n        \
         return int_to_str(666666);\n    }\n}\n\
         fn main() -> Int {\n    let t = str_len(vv(true)) + str_len(vv(false)) * 10 + str_len(vd(true)) * 100 \
         + str_len(vd(false)) * 1000 + str_len(dv(false)) * 10000 + str_len(dv(true)) * 100000;\n    \
         churn();\n    t + str_len(dd(true)) * 1000000 + str_len(dd(false)) * 10000000\n}\n",
    ));
    assert_eq!(r.value, "65141321");
    clean(&r);
}

#[test]
fn completions_that_do_not_join_are_still_refused() {
    // §20 and the refusals: Value/Unit in an `if`, a Unit arm beside a Value arm used as a
    // value, arm blocks of two types, and a Unit block where a value is needed.
    assert!(refused(
        "fn f(c: Bool, tasks: SpawnCap) -> Int {\n    let mut z = 0;\n    if c { 1 } else { z = 2; }\n}\nfn main(tasks: SpawnCap) -> Int { f(true, tasks) }\n"
    )
    .contains(&"N0302".to_owned()));
    assert!(refused(
        "fn f(o: Option[Int], tasks: SpawnCap) -> Int {\n    let mut z = 0;\n    let x = match o {\n        \
         Option.None() => {\n            z = 1;\n        },\n        Option.Some(value: v) => {\n            v\n        },\n    };\n    x\n}\n\
         fn main(tasks: SpawnCap) -> Int { f(Option[Int].None(), tasks) }\n"
    )
    .contains(&"N0300".to_owned()));
    assert!(refused(
        "fn f(o: Option[Int], tasks: SpawnCap) -> Int {\n    match o {\n        Option.None() => {\n            1\n        },\n        \
         Option.Some(value: v) => {\n            let s = int_to_str(v);\n            s\n        },\n    }\n}\n\
         fn main(tasks: SpawnCap) -> Int { f(Option[Int].None(), tasks) }\n"
    )
    .contains(&"N0302".to_owned()));
    assert!(refused(
        "fn main() -> Int {\n    let mut z = 0;\n    let x = {\n        z = 1;\n    };\n    0\n}\n"
    )
    .contains(&"N0300".to_owned()));
}

// --- across a module boundary --------------------------------------------------------------

#[test]
fn an_importer_matches_a_dependency_s_enum_with_arm_blocks() {
    // §83: B exports an enum and an `Option`-producing API; A matches both with arm blocks.
    // Built natively as two units, so nothing here depends on seeing B's bodies.
    let dir = tempfile::tempdir().expect("tempdir");
    let write =
        |name: &str, text: &str| std::fs::write(dir.path().join(name), text).expect("write");
    write("nazm.root", "");
    write(
        "b.nz",
        "pub enum Token {\n    Word(text: Str),\n    Number(value: Int),\n    End,\n}\n\
         pub fn token(n: Int) -> Token {\n    if n == 0 { return Token.End(); }\n    \
         if n % 2 == 0 { return Token.Number(value: n); }\n    Token.Word(text: str_concat(\"w\", int_to_str(n)))\n}\n\
         pub fn find(n: Int) -> Option[Str] {\n    if n > 3 { return Option[Str].Some(value: int_to_str(n * 11)); }\n    Option[Str].None()\n}\n",
    );
    write(
        "a.nz",
        "use \"b.nz\";\n\
         fn score(n: Int) -> Int {\n    let t = token(n);\n    let base = match t {\n        \
         Token.Word(text: s) => {\n            let w = s;\n            str_len(w) * 100\n        },\n        \
         Token.Number(value: v) => {\n            let doubled = v * 2;\n            doubled\n        },\n        \
         Token.End() => {\n            0\n        },\n    };\n    let extra = match find(n) {\n        \
         Option.None() => {\n            0\n        },\n        Option.Some(value: f) => {\n            let g = f;\n            str_len(g)\n        },\n    };\n    \
         base + extra\n}\n\
         fn main() -> Int { score(0) + score(3) * 10 + score(4) * 1000 + score(15) * 100000 }\n",
    );
    let run = Command::new(NAZM)
        .args(["run", "a.nz"])
        .current_dir(dir.path())
        .output()
        .expect("run");
    let built = Command::new(NAZM)
        .args(["build", "a.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let native = Command::new(dir.path().join("prog"))
        .output()
        .expect("execute");
    // score(0) = 0; score(3) = len "w3" * 100 = 200; score(4) = 8 + len "44" = 10;
    // score(15) = len "w15" * 100 + len "165" = 303.
    let expected = (200 * 10 + 10 * 1000 + 303 * 100_000).to_string();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), expected);
    assert_eq!(String::from_utf8_lossy(&native.stdout).trim(), expected);
}
