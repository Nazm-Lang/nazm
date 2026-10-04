//! N12: `Result`, `Option`, and `?`, as the checker and the interpreter see them.
//!
//! `docs/spec.md`, *The core prelude* and *Propagation*, is the law. The claims held here:
//!
//! * `Result` and `Option` are ordinary generic enums every module sees without a `use`;
//! * `?` recognises the core prelude's `Result` by **definition**, never by name or shape;
//! * `expr?` is the operand's success value, or a `return` of its error in the enclosing
//!   function's own `Result`, whose error type must be exactly the operand's;
//! * the operand is evaluated once, and whatever the propagation leaves unevaluated stays
//!   unevaluated.
//!
//! Every refusal names its code **and its span**, because the Nazm-written checker is held
//! to the same pair.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::{check_source, run, Outcome, Value};
use nazm_diag::codes;

fn value(src: &str) -> Value {
    match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}\nfor {src}"),
    }
}

fn int(src: &str) -> i64 {
    match value(src) {
        Value::Int(n) => n,
        other => panic!("expected an Int, got {other:?}"),
    }
}

/// Every diagnostic, as `(code, start, end)`.
fn diagnostics(src: &str) -> Vec<(String, u32, u32)> {
    check_source(src)
        .into_iter()
        .map(|d| {
            (
                d.code.as_str().to_owned(),
                d.primary.span.start,
                d.primary.span.end,
            )
        })
        .collect()
}

/// The span of the `n`th occurrence (from 0) of `needle` in `src`.
fn at(src: &str, needle: &str, n: usize) -> (u32, u32) {
    let mut from = 0;
    for _ in 0..n {
        from = src[from..].find(needle).unwrap() + from + 1;
    }
    let start = src[from..]
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?}"))
        + from;
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

/// The program fails with exactly one diagnostic, this code, at this text.
fn refused(src: &str, code: nazm_diag::Code, needle: &str, n: usize) {
    let got = diagnostics(src);
    let (start, end) = at(src, needle, n);
    assert_eq!(
        got,
        vec![(code.as_str().to_owned(), start, end)],
        "for {src}"
    );
}

/// A leaf that fails when asked to, and the error type it fails with.
const LEAF: &str = "struct Oops {\n    code: Int,\n    why: Str,\n}\n\
    fn leaf(fail: Bool, n: Int) -> Result[Int, Oops] {\n    \
    if fail {\n        return Result[Int, Oops].Err(error: Oops(code: n, why: \"leaf\"));\n    }\n    \
    Result[Int, Oops].Ok(value: n)\n}\n";

/// `r` taken apart: the success value, or `-(error code)`.
const READ: &str = "fn read(r: Result[Int, Oops]) -> Int {\n    match r {\n        \
    Result.Ok(value: v) => v,\n        Result.Err(error: e) => 0 - e.code,\n    }\n}\n";

fn program(body: &str) -> String {
    format!("{LEAF}{READ}{body}")
}

// --- the prelude ----------------------------------------------------------------------------

#[test]
fn result_and_option_are_visible_in_every_module_without_a_use() {
    let src = "fn main() -> Int {\n    let r = Result[Int, Str].Ok(value: 40);\n    \
               let o = Option[Str].Some(value: \"ab\");\n    \
               let a = match r { Result.Ok(value: v) => v, Result.Err(error: e) => str_len(e), };\n    \
               let b = match o { Option.None() => 0, Option.Some(value: s) => str_len(s), };\n    \
               a + b\n}\n";
    assert_eq!(int(src), 42);
}

#[test]
fn option_none_is_a_variant_not_a_sentinel() {
    // `None` of an `Option[Int]` is not `0`, and `None` of an `Option[Str]` is not an
    // empty string: each is its own variant, told apart by `match` and nothing else.
    let src = "fn get(o: Option[Int]) -> Int {\n    match o {\n        Option.None() => 100,\n        \
               Option.Some(value: n) => n,\n    }\n}\n\
               fn main() -> Int {\n    get(Option[Int].None()) + get(Option[Int].Some(value: 0))\n}\n";
    assert_eq!(int(src), 100);
}

#[test]
fn a_project_may_not_declare_a_prelude_type() {
    for (decl, name) in [
        (
            "enum Result[T, E] {\n    Ok(value: T),\n    Err(error: E),\n}\n",
            "Result",
        ),
        ("struct Option {\n    x: Int,\n}\n", "Option"),
        ("enum Result {\n    A,\n}\n", "Result"),
    ] {
        let src = format!("{decl}fn main() -> Int {{ 0 }}\n");
        refused(&src, codes::PRELUDE_TYPE_REDECLARED, name, 0);
    }
}

#[test]
fn a_project_result_is_refused_and_is_not_what_question_mark_propagates() {
    // Refused at its declaration — and `?` on a value of it is still `N0360`, because
    // what `?` recognises is the prelude's definition, not whatever `Result` names here.
    let src = "enum Result[T, E] {\n    Ok(value: T),\n    Err(error: E),\n}\n\
               fn f(r: Result[Int, Str]) -> Result[Int, Str] {\n    let n = r?;\n    \
               Result[Int, Str].Ok(value: n)\n}\nfn main() -> Int { 0 }\n";
    let mut got = diagnostics(src);
    got.sort();
    let (a, b) = at(src, "Result", 0);
    let (q, r) = at(src, "?", 0);
    let mut want = vec![
        (codes::PRELUDE_TYPE_REDECLARED.as_str().to_owned(), a, b),
        (codes::PROPAGATE_NON_RESULT.as_str().to_owned(), q, r),
    ];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn a_prelude_name_is_still_free_in_the_function_namespace() {
    // Types and functions are separate namespaces, and the prelude declares only types.
    assert_eq!(
        int("fn Result() -> Int { 3 }\nfn main() -> Int { Result() }\n"),
        3
    );
}

// --- `?`, working --------------------------------------------------------------------------

#[test]
fn propagation_yields_the_success_value_and_returns_the_error() {
    let src = program(
        "fn twice(fail: Bool) -> Result[Int, Oops] {\n    let n = leaf(fail, 21)?;\n    \
         Result[Int, Oops].Ok(value: n * 2)\n}\n\
         fn main() -> Int {\n    read(twice(false)) * 1000 + read(twice(true))\n}\n",
    );
    // 42 on success; on failure the error's code comes back negated.
    assert_eq!(int(&src), 42 * 1000 - 21);
}

#[test]
fn the_function_may_succeed_with_a_different_type_than_the_operand() {
    // `leaf` succeeds with an `Int`, `name` with a `Str`; only the error crosses.
    let src = program(
        "fn name(fail: Bool) -> Result[Str, Oops] {\n    let n = leaf(fail, 7)?;\n    \
         Result[Str, Oops].Ok(value: int_to_str(n * 111))\n}\n\
         fn main() -> Int {\n    match name(false) {\n        Result.Ok(value: s) => str_len(s),\n        \
         Result.Err(error: e) => e.code,\n    }\n}\n",
    );
    assert_eq!(int(&src), 3);
}

#[test]
fn the_error_propagates_through_three_functions_unchanged() {
    let src = program(
        "fn middle(fail: Bool) -> Result[Int, Oops] {\n    Result[Int, Oops].Ok(value: leaf(fail, 5)? + 1)\n}\n\
         fn outer(fail: Bool) -> Result[Int, Oops] {\n    Result[Int, Oops].Ok(value: middle(fail)? * 10)\n}\n\
         fn main() -> Int {\n    let e = match outer(true) {\n        Result.Ok(value: v) => v,\n        \
         Result.Err(error: e) => e.code * 100 + str_len(e.why),\n    };\n    \
         e * 1000 + read(outer(false))\n}\n",
    );
    assert_eq!(int(&src), 504 * 1000 + 60);
}

#[test]
fn one_question_mark_removes_exactly_one_layer() {
    let src = "fn inner(n: Int) -> Result[Result[Int, Str], Str] {\n    \
               Result[Result[Int, Str], Str].Ok(value: Result[Int, Str].Ok(value: n))\n}\n\
               fn once(n: Int) -> Result[Result[Int, Str], Str] {\n    let r = inner(n)?;\n    \
               Result[Result[Int, Str], Str].Ok(value: r)\n}\n\
               fn twice(n: Int) -> Result[Int, Str] {\n    let a = inner(n)??;\n    let b = (inner(n)?)?;\n    \
               Result[Int, Str].Ok(value: a + b)\n}\n\
               fn main() -> Int {\n    let x = match twice(4) { Result.Ok(value: v) => v, Result.Err(error: e) => 0, };\n    \
               let y = match once(3) {\n        Result.Ok(value: r) => match r { Result.Ok(value: v) => v, Result.Err(error: e) => 0, },\n        \
               Result.Err(error: e) => 0,\n    };\n    x * 10 + y\n}\n";
    assert_eq!(int(src), 83);
}

#[test]
fn a_generic_function_propagates_parametrically() {
    // Nothing is asked of `T` or `E` but to be moved: `?` needs no operation on either.
    let src = "struct Diag {\n    code: Str,\n}\n\
               fn pass[T, E](r: Result[T, E]) -> Result[T, E] {\n    let value = r?;\n    \
               Result[T, E].Ok(value: value)\n}\n\
               fn main() -> Int {\n    let a = match pass(Result[Int, Str].Ok(value: 40)) {\n        \
               Result.Ok(value: v) => v,\n        Result.Err(error: e) => 0,\n    };\n    \
               let errs = vec_new[Diag]();\n    vec_push(errs, Diag(code: \"N1\"));\n    \
               let b = match pass(Result[Str, Vec[Diag]].Err(error: errs)) {\n        \
               Result.Ok(value: s) => 0,\n        Result.Err(error: ds) => vec_len(ds) + str_len(vec_get(ds, 0).code),\n    };\n    \
               a + b - 1\n}\n";
    assert_eq!(int(src), 42);
}

#[test]
fn propagation_parses_as_a_postfix_beside_projection() {
    // `-r?` is `-(r?)`, and `make()?.x` projects out of the success value.
    let src = "struct P {\n    x: Int,\n}\n\
               fn make(n: Int) -> Result[P, Str] {\n    Result[P, Str].Ok(value: P(x: n))\n}\n\
               fn f() -> Result[Int, Str] {\n    let a = -make(5)?.x;\n    let b = make(50)?.x;\n    \
               Result[Int, Str].Ok(value: a + b)\n}\n\
               fn main() -> Int {\n    match f() { Result.Ok(value: v) => v, Result.Err(error: e) => 0, }\n}\n";
    assert_eq!(int(src), 45);
}

#[test]
fn explicit_match_and_propagation_mean_the_same_thing() {
    // The pair the spec promises: everything `?` does is expressible as a `match`, and the
    // two agree on both outcomes.
    let written = program(
        "fn by_match(fail: Bool) -> Result[Int, Oops] {\n    let n = match leaf(fail, 9) {\n        \
         Result.Ok(value: v) => v,\n        Result.Err(error: e) => { return Result[Int, Oops].Err(error: e); }\n    };\n    \
         Result[Int, Oops].Ok(value: n + 1)\n}\n\
         fn by_question(fail: Bool) -> Result[Int, Oops] {\n    let n = leaf(fail, 9)?;\n    \
         Result[Int, Oops].Ok(value: n + 1)\n}\n\
         fn main() -> Int {\n    read(by_match(false)) * 1000000 + read(by_question(false)) * 10000\n        \
         + (read(by_match(true)) + 100) * 100 + read(by_question(true)) + 100\n}\n",
    );
    assert_eq!(int(&written), 10 * 1_000_000 + 10 * 10_000 + 91 * 100 + 91);
}

// --- evaluation order ----------------------------------------------------------------------

/// A log every step writes to, so order and count are observable.
const LOG: &str = "fn step(log: Ints, n: Int) -> Int {\n    ints_push(log, n);\n    n\n}\n\
    fn maybe(log: Ints, fail: Bool) -> Result[Int, Oops] {\n    ints_push(log, 9);\n    leaf(fail, 2)\n}\n\
    fn trail(log: Ints) -> Int {\n    let mut out = 0;\n    let mut i = 0;\n    \
    while i < ints_len(log) {\n        out = out * 10 + ints_get(log, i);\n        i = i + 1;\n    }\n    out\n}\n";

fn logged(body: &str) -> String {
    format!("{LEAF}{READ}{LOG}{body}")
}

#[test]
fn the_operand_is_evaluated_exactly_once() {
    let src = logged(
        "fn f(log: Ints, fail: Bool) -> Result[Int, Oops] {\n    let n = maybe(log, fail)?;\n    \
         Result[Int, Oops].Ok(value: n)\n}\n\
         fn main() -> Int {\n    let a = ints_new();\n    f(a, false);\n    let b = ints_new();\n    f(b, true);\n    \
         trail(a) * 10 + trail(b)\n}\n",
    );
    assert_eq!(int(&src), 99);
}

#[test]
fn a_failing_argument_stops_the_call_and_every_later_argument() {
    // `consume(step(1), maybe()?, step(3))`: 1 and 9 run, 3 does not, and neither does
    // `consume` — the analogue of N10.1's half-evaluated call.
    let src = logged(
        "fn consume(log: Ints, a: Int, b: Int, c: Int) -> Int {\n    ints_push(log, 7);\n    a + b + c\n}\n\
         fn f(log: Ints, fail: Bool) -> Result[Int, Oops] {\n    \
         Result[Int, Oops].Ok(value: consume(log, step(log, 1), maybe(log, fail)?, step(log, 3)))\n}\n\
         fn main() -> Int {\n    let a = ints_new();\n    let good = read(f(a, false));\n    \
         let b = ints_new();\n    let bad = read(f(b, true));\n    \
         trail(a) * 1000 + trail(b) * 10 + good + bad\n}\n",
    );
    // a: 1 9 3 7 ; b: 1 9 ; good = 6 ; bad = -2
    assert_eq!(int(&src), 1937 * 1000 + 19 * 10 + 6 - 2);
}

#[test]
fn a_failing_operand_skips_the_arithmetic() {
    let src = logged(
        "fn f(log: Ints, fail: Bool) -> Result[Int, Oops] {\n    \
         Result[Int, Oops].Ok(value: step(log, 1) + maybe(log, fail)? * step(log, 3))\n}\n\
         fn main() -> Int {\n    let a = ints_new();\n    let b = ints_new();\n    \
         let good = read(f(a, false));\n    let bad = read(f(b, true));\n    \
         good * 100000 + trail(a) * 100 + trail(b) + bad * 0\n}\n",
    );
    assert_eq!(int(&src), 7 * 100_000 + 193 * 100 + 19);
}

#[test]
fn a_failing_assignment_leaves_its_destination_alone() {
    let src = logged(
        "fn f(log: Ints, fail: Bool, cell: Ints) -> Result[Int, Oops] {\n    \
         let mut x = 5;\n    ints_push(cell, x);\n    x = maybe(log, fail)?;\n    ints_push(cell, x);\n    \
         Result[Int, Oops].Ok(value: x)\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    let cell = ints_new();\n    \
         let r = read(f(log, true, cell));\n    ints_len(cell) * 100 + ints_get(cell, 0) * 10 - r\n}\n",
    );
    assert_eq!(int(&src), 100 + 50 + 2);
}

#[test]
fn a_failing_initialiser_completes_no_record_and_runs_no_later_one() {
    let src = logged(
        "struct Three {\n    a: Str,\n    b: Int,\n    c: Int,\n}\n\
         fn f(log: Ints, fail: Bool) -> Result[Three, Oops] {\n    \
         Result[Three, Oops].Ok(value: Three(a: int_to_str(step(log, 1)), b: maybe(log, fail)?, c: step(log, 3)))\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    match f(log, true) {\n        \
         Result.Ok(value: t) => 0,\n        Result.Err(error: e) => trail(log) * 10 + e.code,\n    }\n}\n",
    );
    assert_eq!(int(&src), 192);
}

#[test]
fn a_failing_payload_initialiser_completes_no_variant() {
    let src = logged(
        "enum Shape {\n    Box(label: Str, w: Int, h: Int),\n    Dot,\n}\n\
         fn f(log: Ints, fail: Bool) -> Result[Shape, Oops] {\n    \
         Result[Shape, Oops].Ok(value: Shape.Box(label: int_to_str(step(log, 1)), w: maybe(log, fail)?, h: step(log, 3)))\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    match f(log, true) {\n        \
         Result.Ok(value: s) => 0,\n        Result.Err(error: e) => trail(log),\n    }\n}\n",
    );
    assert_eq!(int(&src), 19);
}

#[test]
fn a_failing_scrutinee_inspects_nothing() {
    let src = logged(
        "enum Two {\n    A(n: Int),\n    B,\n}\n\
         fn load(log: Ints, fail: Bool) -> Result[Two, Oops] {\n    ints_push(log, 9);\n    \
         if fail {\n        return Result[Two, Oops].Err(error: Oops(code: 4, why: \"x\"));\n    }\n    \
         Result[Two, Oops].Ok(value: Two.A(n: 8))\n}\n\
         fn f(log: Ints, fail: Bool) -> Result[Int, Oops] {\n    let n = match load(log, fail)? {\n        \
         Two.A(n: n) => step(log, n),\n        Two.B() => step(log, 5),\n    };\n    Result[Int, Oops].Ok(value: n)\n}\n\
         fn main() -> Int {\n    let a = ints_new();\n    let b = ints_new();\n    \
         let good = read(f(a, false));\n    let bad = read(f(b, true));\n    \
         good * 10000 + trail(a) * 10 + trail(b) + bad * 0\n}\n",
    );
    assert_eq!(int(&src), 8 * 10000 + 98 * 10 + 9);
}

#[test]
fn propagation_from_an_arm_or_a_branch_is_a_return() {
    let src = logged(
        "fn f(log: Ints, which: Int) -> Result[Int, Oops] {\n    \
         let x = if which == 0 { maybe(log, true)? } else { step(log, 4) };\n    \
         let y = match leaf(which == 1, 6) {\n        Result.Ok(value: v) => maybe(log, which == 2)? + v,\n        \
         Result.Err(error: e) => { return Result[Int, Oops].Err(error: e); }\n    };\n    \
         Result[Int, Oops].Ok(value: x + y)\n}\n\
         fn main() -> Int {\n    let l = ints_new();\n    \
         read(f(l, 0)) * 1000000 + read(f(l, 1)) * 10000 + read(f(l, 2)) * 100 + read(f(l, 3))\n}\n",
    );
    // which=0: -2 ; 1: -6 ; 2: -2 ; 3: 4 + (2 + 6) = 12
    assert_eq!(int(&src), -2 * 1_000_000 - 6 * 10_000 - 2 * 100 + 12);
}

#[test]
fn propagation_inside_a_loop_returns_from_the_function() {
    // Not `break`: the loop is left because the function is.
    let src = logged(
        "fn f(log: Ints) -> Result[Int, Oops] {\n    let mut i = 0;\n    let mut sum = 0;\n    \
         while i < 10 {\n        sum = sum + leaf(i == 3, i)?;\n        ints_push(log, i);\n        i = i + 1;\n    }\n    \
         Result[Int, Oops].Ok(value: sum)\n}\n\
         fn main() -> Int {\n    let log = ints_new();\n    let r = read(f(log));\n    trail(log) * 10 - r\n}\n",
    );
    assert_eq!(int(&src), 12 * 10 + 3);
}

#[test]
fn propagation_inside_a_scope_joins_its_tasks_first() {
    // The worker is still running when `?` leaves. It must have finished — and delivered —
    // before `run` returns, or `main` closes the channel under it and receives nothing.
    let src = format!(
        "{LEAF}fn worker(out: Chan) -> Int {{\n    let mut i = 0;\n    let mut s = 0;\n    \
         while i < 50000 {{ s = s + i % 7; i = i + 1; }}\n    chan_send(out, 7);\n    s\n}}\n\
         fn run(out: Chan, tasks: SpawnCap) -> Result[Int, Oops] {{\n    scope {{\n        spawn worker(out);\n        \
         let x = leaf(true, 3)?;\n        chan_send(out, x);\n    }}\n    Result[Int, Oops].Ok(value: 1)\n}}\n\
         fn main(tasks: SpawnCap) -> Int {{\n    let out = chan_new(4);\n    let code = match run(out, tasks) {{\n        \
         Result.Ok(value: v) => 0,\n        Result.Err(error: e) => e.code,\n    }};\n    chan_close(out);\n    \
         let buf = ints_new();\n    chan_recv(out, buf);\n    code * 10 + ints_get(buf, 0)\n}}\n"
    );
    assert_eq!(int(&src), 37);
}

// --- `?`, refused --------------------------------------------------------------------------

#[test]
fn propagation_needs_a_result() {
    let src =
        "fn f() -> Result[Int, Str] {\n    let n = 4?;\n    Result[Int, Str].Ok(value: n)\n}\n\
               fn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_NON_RESULT, "?", 0);
}

#[test]
fn propagation_does_not_apply_to_option() {
    // Deliberately: `Option` has no `?` in N12, and it is refused by the same rule as any
    // other enum rather than being a second carrier.
    let src = "fn f(o: Option[Int]) -> Result[Int, Str] {\n    let n = o?;\n    Result[Int, Str].Ok(value: n)\n}\n\
               fn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_NON_RESULT, "?", 0);
}

#[test]
fn an_enum_shaped_like_result_is_not_result() {
    // Nominal: `Ok` and `Err` variants make `MyResult` look like one and nothing more.
    let src = "enum MyResult[T, E] {\n    Ok(value: T),\n    Err(error: E),\n}\n\
               fn f(r: MyResult[Int, Str]) -> Result[Int, Str] {\n    let n = r?;\n    Result[Int, Str].Ok(value: n)\n}\n\
               fn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_NON_RESULT, "?", 0);
}

#[test]
fn propagation_needs_a_function_that_returns_result() {
    let src = "fn f(r: Result[Int, Str]) -> Int {\n    r?\n}\nfn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_OUTSIDE_RESULT, "?", 0);
    // `main` included: its return type is unchanged, so a program handles its own errors.
    let src = "fn main() -> Int {\n    Result[Int, Str].Ok(value: 1)?\n}\n";
    refused(src, codes::PROPAGATE_OUTSIDE_RESULT, "?", 0);
    // A lookalike return type is not a `Result` either.
    let src = "enum MyResult[T, E] {\n    Ok(value: T),\n    Err(error: E),\n}\n\
               fn f(r: Result[Int, Str]) -> MyResult[Int, Str] {\n    MyResult[Int, Str].Ok(value: r?)\n}\n\
               fn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_OUTSIDE_RESULT, "?", 0);
}

#[test]
fn propagation_needs_exactly_the_same_error_type() {
    // Two records with identical fields are still two error types.
    let src = "struct ErrorA {\n    code: Int,\n}\nstruct ErrorB {\n    code: Int,\n}\n\
               fn f(r: Result[Int, ErrorA]) -> Result[Int, ErrorB] {\n    let n = r?;\n    \
               Result[Int, ErrorB].Ok(value: n)\n}\nfn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_ERROR_MISMATCH, "?", 0);
}

#[test]
fn two_error_parameters_are_not_known_to_be_equal() {
    // No constraint is inferred: `E1` and `E2` are different parameters.
    let src = "fn outer[T, E1, E2](r: Result[T, E1]) -> Result[T, E2] {\n    let v = r?;\n    \
               Result[T, E2].Ok(value: v)\n}\nfn main() -> Int { 0 }\n";
    refused(src, codes::PROPAGATE_ERROR_MISMATCH, "?", 0);
}

#[test]
fn the_success_type_is_the_operands_not_the_functions() {
    // `r?` is an `Int`; using it as the function's `Str` is an ordinary mismatch.
    let src = "fn f(r: Result[Int, Str]) -> Result[Str, Str] {\n    \
               Result[Str, Str].Ok(value: r?)\n}\nfn main() -> Int { 0 }\n";
    let got = diagnostics(src);
    let (start, end) = at(src, "r?", 0);
    assert_eq!(
        got,
        vec![(codes::TYPE_MISMATCH.as_str().to_owned(), start, end)],
        "an Int given where a Str is wanted is an ordinary mismatch"
    );
}

// --- ordinary enum laws, through the prelude's types -----------------------------------------

#[test]
fn a_result_holding_a_vec_does_not_cross_into_a_task() {
    let src = "struct Diag {\n    code: Str,\n}\n\
               fn work(r: Result[Int, Vec[Diag]]) -> Int { 0 }\n\
               fn main(tasks: SpawnCap) -> Int {\n    let r = Result[Int, Vec[Diag]].Ok(value: 1);\n    \
               scope { spawn work(r); }\n    0\n}\n";
    let got = diagnostics(src);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, codes::ILLEGAL_TASK_CAPTURE.as_str(), "{got:?}");
}

#[test]
fn a_result_of_task_safe_types_crosses_into_a_task() {
    let src = "fn work(r: Result[Int, Str], out: Chan) -> Int {\n    \
               match r {\n        Result.Ok(value: v) => chan_send(out, v),\n        Result.Err(error: e) => chan_send(out, 0),\n    };\n    0\n}\n\
               fn main(tasks: SpawnCap) -> Int {\n    let out = chan_new(1);\n    \
               scope { spawn work(Result[Int, Str].Ok(value: 42), out); }\n    \
               let buf = ints_new();\n    chan_recv(out, buf);\n    ints_get(buf, 0)\n}\n";
    assert_eq!(int(src), 42);
}

#[test]
fn an_option_can_close_an_ownership_cycle_through_a_vec() {
    let src = "struct Node {\n    next: Option[Vec[Node]],\n}\nfn main() -> Int { 0 }\n";
    let got = diagnostics(src);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, codes::OWNERSHIP_CYCLE.as_str(), "{got:?}");
}

#[test]
fn an_inline_option_of_itself_has_no_layout() {
    let src = "struct Node {\n    next: Option[Node],\n}\nfn main() -> Int { 0 }\n";
    let got = diagnostics(src);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, codes::RECURSIVE_RECORD.as_str(), "{got:?}");
}
