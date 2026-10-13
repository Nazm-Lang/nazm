//! N12: `Result`, `Option` and `?` — reclamation, ownership order, and cleanup on the way
//! out, in both implementations.
//!
//! # What is being held
//!
//! `Result` and `Option` are ordinary generic enums, so everything N10 and N11 proved about
//! enums should hold of them with no new rule: that is the first half of this file, and it
//! is deliberately unremarkable. The second half is `?`, which is a `match` with a `return`
//! in one arm and must be exactly as sound as one — the success payload taken before the
//! temporary `Result` is let go, the error payload owned by the returned `Result` before
//! anything it came out of is released, and every scope, local and half-built value between
//! the `?` and the function's exit cleaned on the way out.
//!
//! # Why the churn
//!
//! A payload released one step too early is often still readable: nothing has reused its
//! block yet. So the order-sensitive cases allocate and discard enough small strings after
//! the propagation that a block freed too soon would be handed back out, and a wrong
//! answer or a signal would show it. Counters alone would not.
//!
//! Every case runs interpreted and compiled, and the two must agree on the value **and**
//! on every counter — `docs/spec.md`'s memory constitution is about the language.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::memory_report;

use memory_report::{counts, Class, Ran};

use std::process::Command;

use nazm_lir::MEMORY_VARIABLE;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Interpret it, compile and run it, and require the two to agree about everything —
/// including that neither wrote anything but the memory report to stderr, because a
/// propagated error is a return and not a runtime failure.
fn both(source: &str) -> Ran {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let interpreted = Command::new(NAZM)
        .args(["run", "m.nz"])
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&interpreted.stderr).into_owned();
    assert!(
        interpreted.status.success(),
        "interpreting failed: {stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "unexpected stderr: {stderr}");
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
    let native = Command::new(dir.path().join("prog"))
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("execute");
    let stderr = String::from_utf8_lossy(&native.stderr).into_owned();
    assert!(native.status.success(), "the program failed: {stderr}");
    assert_eq!(stderr.lines().count(), 1, "unexpected stderr: {stderr}");
    let b = counts(
        &stderr,
        String::from_utf8_lossy(&native.stdout).trim().to_owned(),
    );

    assert_eq!(a, b, "the two implementations disagree");
    a
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

/// Small strings made and thrown away, so that a block released too early is reused.
const CHURN: &str = "fn churn() -> Int {\n    let mut noise = vec_new[Str]();\n    let mut k = 0;\n    \
    while k < 64 {\n        vec_push(noise, str_concat(\"noise-\", int_to_str(k)));\n        k = k + 1;\n    }\n    \
    noise = vec_new[Str]();\n    vec_len(noise)\n}\n";

/// The shared vocabulary: an owning record, an owning enum, and a diagnostic.
const TYPES: &str = "struct Item {\n    name: Str,\n    tags: Ints,\n}\n\
    enum Shape {\n    Named(label: Str),\n    Plain,\n}\n\
    struct Diag {\n    code: Str,\n    start: Int,\n}\n\
    fn item(n: Int) -> Item {\n    let tags = ints_new();\n    ints_push(tags, n);\n    \
    Item(name: str_concat(\"item-\", int_to_str(n)), tags: tags)\n}\n\
    fn diags(n: Int) -> Vec[Diag] {\n    let ds = vec_new[Diag]();\n    let mut i = 0;\n    \
    while i < n {\n        vec_push(ds, Diag(code: str_concat(\"N0\", int_to_str(300 + i)), start: i));\n        i = i + 1;\n    }\n    ds\n}\n";

fn with(body: &str) -> String {
    format!("{TYPES}{CHURN}{body}")
}

// --- Result and Option are ordinary enums -------------------------------------------------

#[test]
fn every_result_shape_is_constructed_copied_returned_and_reclaimed() {
    // §80's matrix: five success shapes and five error shapes, each made in a function,
    // returned, copied into a second binding, and taken apart.
    let r = both(&with(
        "fn ok_int() -> Result[Int, Str] { Result[Int, Str].Ok(value: 1) }\n\
         fn ok_str() -> Result[Str, Str] { Result[Str, Str].Ok(value: int_to_str(22)) }\n\
         fn ok_rec() -> Result[Item, Str] { Result[Item, Str].Ok(value: item(3)) }\n\
         fn ok_enum() -> Result[Shape, Str] { Result[Shape, Str].Ok(value: Shape.Named(label: int_to_str(444))) }\n\
         fn ok_vec() -> Result[Vec[Item], Str] {\n    let xs = vec_new[Item]();\n    vec_push(xs, item(5));\n    \
         Result[Vec[Item], Str].Ok(value: xs)\n}\n\
         fn err_str() -> Result[Int, Str] { Result[Int, Str].Err(error: int_to_str(66)) }\n\
         fn err_rec() -> Result[Int, Item] { Result[Int, Item].Err(error: item(7)) }\n\
         fn err_enum() -> Result[Int, Shape] { Result[Int, Shape].Err(error: Shape.Named(label: int_to_str(8888))) }\n\
         fn err_vec() -> Result[Int, Vec[Diag]] { Result[Int, Vec[Diag]].Err(error: diags(9)) }\n\
         fn err_chan() -> Result[Int, Chan] { Result[Int, Chan].Err(error: chan_new(2)) }\n\
         fn main() -> Int {\n    let a = ok_int();\n    let a2 = a;\n    \
         let s = ok_str();\n    let s2 = s;\n    let r = ok_rec();\n    let r2 = r;\n    \
         let e = ok_enum();\n    let e2 = e;\n    let v = ok_vec();\n    let v2 = v;\n    \
         let t = 0\n        + match a2 { Result.Ok(value: n) => n, Result.Err(error: x) => 0, }\n        \
         + match s2 { Result.Ok(value: x) => str_len(x), Result.Err(error: x) => 0, }\n        \
         + match r2 { Result.Ok(value: i) => ints_get(i.tags, 0), Result.Err(error: x) => 0, }\n        \
         + match e2 { Result.Ok(value: sh) => match sh { Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }, Result.Err(error: x) => 0, }\n        \
         + match v2 { Result.Ok(value: xs) => vec_len(xs) * 5, Result.Err(error: x) => 0, };\n    \
         let u = 0\n        + match err_str() { Result.Ok(value: n) => 0, Result.Err(error: x) => str_len(x), }\n        \
         + match err_rec() { Result.Ok(value: n) => 0, Result.Err(error: i) => ints_get(i.tags, 0), }\n        \
         + match err_enum() { Result.Ok(value: n) => 0, Result.Err(error: sh) => match sh { Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }, }\n        \
         + match err_vec() { Result.Ok(value: n) => 0, Result.Err(error: ds) => vec_len(ds), }\n        \
         + match err_chan() { Result.Ok(value: n) => 0, Result.Err(error: c) => chan_close(c) + 1, };\n    \
         t * 100 + u\n}\n",
    ));
    // t = 1 + 2 + 3 + 3 + 5 = 14 ; u = 2 + 7 + 4 + 9 + 1 = 23
    assert_eq!(r.value, "1423");
    clean(&r);
    assert!(r.strings.allocated >= 10, "{r:?}");
    assert_eq!(r.channels.allocated, 1, "{r:?}");
}

#[test]
fn every_option_shape_is_constructed_copied_returned_and_reclaimed() {
    let r = both(&with(
        "fn none() -> Option[Str] { Option[Str].None() }\n\
         fn some_int() -> Option[Int] { Option[Int].Some(value: 4) }\n\
         fn some_str() -> Option[Str] { Option[Str].Some(value: int_to_str(55)) }\n\
         fn some_rec() -> Option[Item] { Option[Item].Some(value: item(6)) }\n\
         fn some_vec() -> Option[Vec[Str]] {\n    let xs = vec_new[Str]();\n    vec_push(xs, int_to_str(1));\n    \
         vec_push(xs, int_to_str(2));\n    Option[Vec[Str]].Some(value: xs)\n}\n\
         fn main() -> Int {\n    let n = none();\n    let n2 = n;\n    let s = some_str();\n    let s2 = s;\n    \
         let v = some_vec();\n    let v2 = v;\n    \
         match n2 { Option.None() => 1000, Option.Some(value: x) => 0, }\n        \
         + match some_int() { Option.None() => 0, Option.Some(value: x) => x * 100, }\n        \
         + match s2 { Option.None() => 0, Option.Some(value: x) => str_len(x) * 10, }\n        \
         + match some_rec() { Option.None() => 0, Option.Some(value: i) => ints_get(i.tags, 0), }\n        \
         + match v2 { Option.None() => 0, Option.Some(value: xs) => vec_len(xs) * 10000, }\n}\n",
    ));
    assert_eq!(r.value, "21426");
    clean(&r);
}

// --- `?`: which reference survives -----------------------------------------------------------

#[test]
fn a_success_payload_outlives_the_result_it_came_out_of() {
    // Each `?` takes its payload out of a temporary `Result` that dies on that line. The
    // churn afterwards reuses whatever a premature release freed.
    let r = both(&with(
        "fn text(n: Int) -> Result[Str, Str] { Result[Str, Str].Ok(value: str_concat(\"value-\", int_to_str(n))) }\n\
         fn thing(n: Int) -> Result[Item, Str] { Result[Item, Str].Ok(value: item(n)) }\n\
         fn many(n: Int) -> Result[Vec[Str], Str] {\n    let xs = vec_new[Str]();\n    vec_push(xs, int_to_str(n));\n    \
         Result[Vec[Str], Str].Ok(value: xs)\n}\n\
         fn f() -> Result[Int, Str] {\n    let s = text(12345)?;\n    let i = thing(7)?;\n    let xs = many(99)?;\n    \
         churn();\n    Result[Int, Str].Ok(value: str_len(s) * 10000 + str_len(i.name) * 100 + ints_get(i.tags, 0) \
         + str_len(vec_get(xs, 0)) * 1000000)\n}\n\
         fn main() -> Int {\n    match f() { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, }\n}\n",
    ));
    // "value-12345" = 11, "item-7" = 6, tag 7, "99" = 2
    assert_eq!(r.value, "2110607");
    clean(&r);
}

#[test]
fn an_error_payload_outlives_every_frame_it_propagates_out_of() {
    // A `Str`, a record and a `Vec[Diag]`, each propagated through two functions, with
    // churn between every return.
    let r = both(&with(
        "fn leaf_s() -> Result[Int, Str] { Result[Int, Str].Err(error: str_concat(\"broken-\", int_to_str(42))) }\n\
         fn leaf_r() -> Result[Int, Item] { Result[Int, Item].Err(error: item(31)) }\n\
         fn leaf_v() -> Result[Int, Vec[Diag]] { Result[Int, Vec[Diag]].Err(error: diags(5)) }\n\
         fn mid_s() -> Result[Int, Str] {\n    let n = leaf_s()?;\n    churn();\n    Result[Int, Str].Ok(value: n)\n}\n\
         fn mid_r() -> Result[Int, Item] {\n    let n = leaf_r()?;\n    Result[Int, Item].Ok(value: n)\n}\n\
         fn mid_v() -> Result[Str, Vec[Diag]] {\n    let n = leaf_v()?;\n    Result[Str, Vec[Diag]].Ok(value: int_to_str(n))\n}\n\
         fn top_s() -> Result[Str, Str] {\n    let n = mid_s()?;\n    Result[Str, Str].Ok(value: int_to_str(n))\n}\n\
         fn top_r() -> Result[Int, Item] {\n    let n = mid_r()?;\n    Result[Int, Item].Ok(value: n)\n}\n\
         fn top_v() -> Result[Int, Vec[Diag]] {\n    let s = mid_v()?;\n    Result[Int, Vec[Diag]].Ok(value: str_len(s))\n}\n\
         fn main() -> Int {\n    let a = top_s();\n    churn();\n    let b = top_r();\n    churn();\n    let c = top_v();\n    churn();\n    \
         match a { Result.Ok(value: s) => 0, Result.Err(error: e) => str_len(e) * 10000, }\n        \
         + match b { Result.Ok(value: n) => 0, Result.Err(error: i) => str_len(i.name) * 100 + ints_get(i.tags, 0), }\n        \
         + match c { Result.Ok(value: n) => 0, Result.Err(error: ds) => vec_len(ds) * 1000000 + str_len(vec_get(ds, 4).code) * 10000000, }\n}\n",
    ));
    // "broken-42" = 9 ; "item-31" = 7, tag 31 ; 5 diagnostics, "N0304" = 5
    assert_eq!(r.value, "55090731");
    clean(&r);
}

#[test]
fn a_vec_of_diagnostics_is_one_handle_through_three_returns() {
    // The error is a handle. Pushing through it after three propagations reaches the vector
    // the leaf built — there is one, not a copy per frame — and it is reclaimed once.
    let r = both(&with(
        "fn leaf(ds: Vec[Diag]) -> Result[Int, Vec[Diag]] { Result[Int, Vec[Diag]].Err(error: ds) }\n\
         fn one(ds: Vec[Diag]) -> Result[Int, Vec[Diag]] {\n    let n = leaf(ds)?;\n    Result[Int, Vec[Diag]].Ok(value: n)\n}\n\
         fn two(ds: Vec[Diag]) -> Result[Int, Vec[Diag]] {\n    let n = one(ds)?;\n    Result[Int, Vec[Diag]].Ok(value: n)\n}\n\
         fn three(ds: Vec[Diag]) -> Result[Int, Vec[Diag]] {\n    let n = two(ds)?;\n    Result[Int, Vec[Diag]].Ok(value: n)\n}\n\
         fn grow(ds: Vec[Diag]) -> Int {\n    vec_push(ds, Diag(code: \"N0999\", start: 9));\n    vec_len(ds)\n}\n\
         fn main() -> Int {\n    let mine = diags(2);\n    let got = match three(mine) {\n        \
         Result.Ok(value: n) => 0,\n        Result.Err(error: ds) => grow(ds),\n    };\n    \
         got * 10 + vec_len(mine)\n}\n",
    ));
    assert_eq!(r.value, "33");
    clean(&r);
    // `diags` makes one vector; nothing on the way copies it.
    assert_eq!(r.sequences.allocated, 1, "{r:?}");
}

// --- `?`: what it leaves behind is cleaned ---------------------------------------------------

#[test]
fn locals_are_released_once_when_an_error_propagates() {
    // §83: a heap string, a `Vec[Item]`, a channel and an enum with an owning payload, all
    // live when `?` leaves.
    let r = both(&with(
        "fn fails() -> Result[Int, Str] { Result[Int, Str].Err(error: int_to_str(404)) }\n\
         fn holder() -> Result[Int, Str] {\n    let s = str_concat(\"held-\", int_to_str(1));\n    \
         let xs = vec_new[Item]();\n    vec_push(xs, item(2));\n    vec_push(xs, item(3));\n    let c = chan_new(3);\n    \
         let sh = Shape.Named(label: int_to_str(77));\n    let n = fails()?;\n    \
         Result[Int, Str].Ok(value: n + str_len(s) + vec_len(xs) + chan_close(c) + match sh { Shape.Named(label: l) => 1, Shape.Plain() => 0, })\n}\n\
         fn main() -> Int {\n    let r = holder();\n    churn();\n    match r { Result.Ok(value: n) => n, Result.Err(error: e) => str_len(e) * 7, }\n}\n",
    ));
    assert_eq!(r.value, "21");
    clean(&r);
    assert_eq!(
        r.channels,
        Class {
            allocated: 1,
            reclaimed: 1,
            live: 0
        }
    );
}

#[test]
fn an_earlier_argument_is_reclaimed_and_nothing_after_it_runs() {
    // §85, the permanent shape: `consume(int_to_str(i), may_fail()?, later())`.
    let r = both(&with(
        "fn may_fail(n: Int) -> Result[Int, Str] {\n    if n % 2 == 0 { return Result[Int, Str].Err(error: int_to_str(n)); }\n    \
         Result[Int, Str].Ok(value: n)\n}\n\
         fn later(log: Ints) -> Str {\n    ints_push(log, 1);\n    str_concat(\"late\", \"r\")\n}\n\
         fn consume(log: Ints, a: Str, b: Int, c: Str) -> Int {\n    ints_push(log, 2);\n    str_len(a) + b + str_len(c)\n}\n\
         fn f(log: Ints, i: Int) -> Result[Int, Str] {\n    \
         Result[Int, Str].Ok(value: consume(log, int_to_str(i * 1000), may_fail(i)?, later(log)))\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    let mut i = 0;\n    let mut total = 0;\n    \
         while i < 20 {\n        total = total + match f(log, i) { Result.Ok(value: n) => n, Result.Err(error: e) => 0 - str_len(e), };\n        \
         i = i + 1;\n    }\n    churn();\n    total * 100 + ints_len(log)\n}\n",
    ));
    // Odd i: consume runs, 4..5 chars + i + 5 ; even i: -len(i). Only ten calls ran
    // `later` and `consume`, so the log has twenty entries.
    let odd: i64 = (1..20)
        .step_by(2)
        .map(|i: i64| (i * 1000).to_string().len() as i64 + i + 5)
        .sum();
    let even: i64 = (0..20)
        .step_by(2)
        .map(|i: i64| -(i.to_string().len() as i64))
        .sum();
    assert_eq!(r.value, ((odd + even) * 100 + 20).to_string());
    clean(&r);
}

#[test]
fn a_half_built_record_or_variant_is_released_when_an_initialiser_propagates() {
    // §86 and §87: an owning first field, `?` in the second, and a third that never runs.
    let r = both(&with(
        "struct Three {\n    a: Str,\n    b: Int,\n    c: Ints,\n}\n\
         enum Held {\n    Three(a: Str, b: Int, c: Ints),\n    Nothing,\n}\n\
         fn may_fail(n: Int) -> Result[Int, Str] {\n    if n > 2 { return Result[Int, Str].Err(error: int_to_str(n)); }\n    \
         Result[Int, Str].Ok(value: n)\n}\n\
         fn rec(n: Int) -> Result[Three, Str] {\n    \
         Result[Three, Str].Ok(value: Three(a: int_to_str(n * 11), b: may_fail(n)?, c: ints_new()))\n}\n\
         fn var(n: Int) -> Result[Held, Str] {\n    \
         Result[Held, Str].Ok(value: Held.Three(a: int_to_str(n * 11), b: may_fail(n)?, c: ints_new()))\n}\n\
         fn main() -> Int {\n    let mut i = 0;\n    let mut t = 0;\n    while i < 6 {\n        \
         t = t + match rec(i) { Result.Ok(value: r) => str_len(r.a) + r.b, Result.Err(error: e) => 100, };\n        \
         t = t + match var(i) { Result.Ok(value: h) => match h { Held.Three(a: a, b: b, c: c) => str_len(a) + b, Held.Nothing() => 0, }, \
         Result.Err(error: e) => 1000, };\n        i = i + 1;\n    }\n    churn();\n    t\n}\n",
    ));
    // i = 0..2 succeed: len("0")+0, len("11")+1, len("22")+2 = 1 + 3 + 4 = 8, twice ;
    // i = 3..5 fail: 3 * (100 + 1000).
    assert_eq!(r.value, (8 * 2 + 3 * 1100).to_string());
    clean(&r);
    // The three that failed built no `Ints`; the three that succeeded built one each, twice.
    // `churn` makes the other two.
    assert_eq!(r.sequences.allocated, 6 + 2, "{r:?}");
}

#[test]
fn a_match_on_a_propagated_value_consumes_one_result_layer() {
    // §88: `match make()? { … }` over an owning enum.
    let r = both(&with(
        "fn make(n: Int) -> Result[Shape, Str] {\n    if n == 0 { return Result[Shape, Str].Err(error: int_to_str(12345)); }\n    \
         Result[Shape, Str].Ok(value: Shape.Named(label: str_concat(\"shape-\", int_to_str(n))))\n}\n\
         fn f(n: Int) -> Result[Int, Str] {\n    let k = match make(n)? {\n        Shape.Named(label: l) => str_len(l),\n        \
         Shape.Plain() => 0,\n    };\n    churn();\n    Result[Int, Str].Ok(value: k)\n}\n\
         fn main() -> Int {\n    let a = match f(7) { Result.Ok(value: k) => k, Result.Err(error: e) => 0, };\n    \
         let b = match f(0) { Result.Ok(value: k) => 0, Result.Err(error: e) => str_len(e), };\n    a * 10 + b\n}\n",
    ));
    assert_eq!(r.value, "75");
    clean(&r);
}

#[test]
fn propagation_out_of_a_scope_joins_the_task_and_cleans_up() {
    // §84: the task still owes a send when `?` leaves the scope.
    let r = both(&with(
        "fn worker(out: Chan, tag: Str) -> Int {\n    let mut i = 0;\n    let mut s = 0;\n    \
         while i < 200000 { s = s + i % 3; i = i + 1; }\n    chan_send(out, str_len(tag));\n    s\n}\n\
         fn fails() -> Result[Int, Str] { Result[Int, Str].Err(error: int_to_str(9)) }\n\
         fn run(out: Chan, tasks: SpawnCap) -> Result[Int, Str] {\n    let tag = str_concat(\"task-\", int_to_str(1));\n    \
         scope {\n        spawn worker(out, tag);\n        let held = int_to_str(555);\n        let x = fails()?;\n        \
         chan_send(out, x + str_len(held));\n    }\n    Result[Int, Str].Ok(value: 0)\n}\n\
         fn main(tasks: SpawnCap) -> Int {\n    let out = chan_new(4);\n    let r = run(out, tasks);\n    chan_close(out);\n    \
         let buf = ints_new();\n    let got = chan_recv(out, buf);\n    let more = chan_recv(out, buf);\n    \
         match r { Result.Ok(value: n) => 0, Result.Err(error: e) => str_len(e) * 1000, } + ints_len(buf) * 100 + ints_get(buf, 0)\n}\n",
    ));
    // The error "9", one delivered value, and it is len("task-1") = 6.
    assert_eq!(r.value, "1106");
    clean(&r);
}

#[test]
fn explicit_match_and_propagation_agree_on_value_and_on_every_counter() {
    // §97: the same program twice, once spelled with `match` and `return`, once with `?`.
    let body = |spelled: &str| {
        with(&format!(
            "fn leaf(n: Int) -> Result[Item, Vec[Diag]] {{\n    if n % 3 == 0 {{ return Result[Item, Vec[Diag]].Err(error: diags(n % 5 + 1)); }}\n    \
             Result[Item, Vec[Diag]].Ok(value: item(n))\n}}\n\
             fn step(n: Int) -> Result[Int, Vec[Diag]] {{\n    let i = {spelled};\n    \
             Result[Int, Vec[Diag]].Ok(value: str_len(i.name) + ints_get(i.tags, 0))\n}}\n\
             fn main() -> Int {{\n    let mut i = 0;\n    let mut t = 0;\n    while i < 30 {{\n        \
             t = t + match step(i) {{ Result.Ok(value: n) => n, Result.Err(error: ds) => 0 - vec_len(ds), }};\n        \
             i = i + 1;\n    }}\n    churn();\n    t\n}}\n"
        ))
    };
    // `return` is a statement, so an arm leaves through an `if` whose branches both
    // transfer — the idiom N10.1's transfer cases use, and the only one `nazm build` had
    // when this was written. Since N12.1 the arm could be `{ return …; }` instead; the
    // spelling is kept because what is compared is the program, not the idiom.
    let by_match = both(&body(
        "match leaf(n) {\n        Result.Ok(value: v) => v,\n        \
         Result.Err(error: e) => if true { return Result[Int, Vec[Diag]].Err(error: e); } \
         else { return Result[Int, Vec[Diag]].Err(error: e); },\n    }",
    ));
    let by_question = both(&body("leaf(n)?"));
    assert_eq!(by_match, by_question);
    clean(&by_question);
}

#[test]
fn a_generic_propagation_is_reclaimed_in_every_instance() {
    let r = both(&with(
        "fn pass[T, E](r: Result[T, E]) -> Result[T, E] {\n    let value = r?;\n    Result[T, E].Ok(value: value)\n}\n\
         fn main() -> Int {\n    \
         let a = match pass(Result[Int, Str].Err(error: int_to_str(123))) { Result.Ok(value: n) => n, Result.Err(error: s) => str_len(s), };\n    \
         let b = match pass(Result[Item, Vec[Diag]].Ok(value: item(4))) { Result.Ok(value: i) => ints_get(i.tags, 0), Result.Err(error: ds) => 0, };\n    \
         let c = match pass(Result[Vec[Item], Shape].Err(error: Shape.Named(label: int_to_str(55555)))) {\n        \
         Result.Ok(value: xs) => 0,\n        Result.Err(error: sh) => match sh { Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, },\n    };\n    \
         churn();\n    a * 100 + b * 10 + c\n}\n",
    ));
    assert_eq!(r.value, "345");
    clean(&r);
}
