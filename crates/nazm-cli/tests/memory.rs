//! Reclamation: that it happens, that it happens on every way out, and that both
//! implementations agree it happened.
//!
//! # Why a counter and not resident memory
//!
//! `free` returns storage to the allocator, not to the operating system, so a program can
//! reclaim everything it allocated and show no change in resident size at all. Peak memory
//! is the right instrument for *whether a workload still grows* — `docs/performance.md`
//! uses it for that — and the wrong one for *whether this particular value was reclaimed*.
//! So the runtime counts allocations and reclamations, both implementations report the same
//! three numbers under `NAZM_MEMORY_REPORT`, and these tests read them.
//!
//! # Why every case is run twice
//!
//! `docs/spec.md`'s memory constitution is a statement about the language, not about a
//! backend. A rule the compiled program obeys and the interpreter does not is a rule
//! neither of them really has, so each case asserts the same value **and** the same counts
//! from both.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use nazm_lir::MEMORY_VARIABLE;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// What one class of storage did in one run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Class {
    allocated: u64,
    reclaimed: u64,
    live: u64,
}

/// What one run reported: its value, and what it did with each class of storage.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    value: String,
    sequences: Class,
    strings: Class,
    channels: Class,
}

/// Read `<class> allocated=… reclaimed=… live=…`, three times, in order.
///
/// Positional rather than by-name search, because the three classes use the same three
/// field names — the class word is what tells them apart, and a parser that looked for
/// the first `allocated=` would answer every question with the sequence count.
fn counts(stderr: &str, value: String) -> Ran {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report in: {stderr}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    let class = |name: &str| {
        let at = words
            .iter()
            .position(|w| *w == name)
            .unwrap_or_else(|| panic!("no `{name}` in: {line}"));
        let field = |offset: usize, prefix: &str| {
            words
                .get(at + offset)
                .and_then(|w| w.strip_prefix(prefix))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("no `{prefix}` after `{name}` in: {line}"))
        };
        Class {
            allocated: field(1, "allocated="),
            reclaimed: field(2, "reclaimed="),
            live: field(3, "live="),
        }
    };
    Ran {
        value,
        sequences: class("sequences"),
        strings: class("strings"),
        channels: class("channels"),
    }
}

/// Evaluate `source`, and compile and run it, and require the two to agree about
/// everything.
fn both(source: &str) -> Ran {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let interpreted = Command::new(NAZM)
        .args(["run", "m.nz"])
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    assert!(
        interpreted.status.success(),
        "interpreting failed: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    let a = counts(
        &String::from_utf8_lossy(&interpreted.stderr),
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
    let b = counts(
        &String::from_utf8_lossy(&native.stderr),
        String::from_utf8_lossy(&native.stdout).trim().to_owned(),
    );

    assert_eq!(a, b, "the two implementations disagree");
    a
}

/// The same, with process arguments, which is the one string provenance a program cannot
/// produce for itself.
fn both_with_args(source: &str, args: &[&str]) -> Ran {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let mut run = Command::new(NAZM);
    run.args(["run", "m.nz", "--"]).args(args);
    let interpreted = run
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    assert!(
        interpreted.status.success(),
        "interpreting failed: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    let a = counts(
        &String::from_utf8_lossy(&interpreted.stderr),
        String::from_utf8_lossy(&interpreted.stdout)
            .trim()
            .to_owned(),
    );

    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(built.status.success(), "{:?}", built);
    let native = Command::new(dir.path().join("prog"))
        .args(args)
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("execute");
    let b = counts(
        &String::from_utf8_lossy(&native.stderr),
        String::from_utf8_lossy(&native.stdout).trim().to_owned(),
    );

    assert_eq!(a, b, "the two implementations disagree");
    a
}

/// The same, for a program that is expected to fail at runtime.
fn both_failing(source: &str) -> (Ran, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), source).expect("write");

    let interpreted = Command::new(NAZM)
        .args(["run", "m.nz"])
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("run");
    assert_eq!(interpreted.status.code(), Some(2), "expected a failure");
    let interpreted_err = String::from_utf8_lossy(&interpreted.stderr).into_owned();
    let a = counts(&interpreted_err, String::new());

    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(built.status.success(), "{:?}", built);
    let native = Command::new(dir.path().join("prog"))
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("execute");
    assert_eq!(native.status.code(), Some(2), "expected a failure");
    let native_err = String::from_utf8_lossy(&native.stderr).into_owned();
    let b = counts(&native_err, String::new());

    assert_eq!(a, b, "the two implementations disagree");
    (a, native_err)
}

/// Run it, and require exactly `n` sequences created and every one of them reclaimed.
fn reclaimed(n: u64, source: &str) -> Ran {
    let r = both(source);
    assert_eq!(r.sequences, expected(n), "{r:?}");
    r
}

/// The same, for string backings.
fn strings(n: u64, source: &str) -> Ran {
    let r = both(source);
    assert_eq!(r.strings, expected(n), "{r:?}");
    r
}

/// The same, for channels.
fn channels(n: u64, source: &str) -> Ran {
    let r = both(source);
    assert_eq!(r.channels, expected(n), "{r:?}");
    r
}

/// `n` created, `n` reclaimed, nothing left.
const fn expected(n: u64) -> Class {
    Class {
        allocated: n,
        reclaimed: n,
        live: 0,
    }
}

// ---------------------------------------------------------------------------
// The twelve shapes
// ---------------------------------------------------------------------------

#[test]
fn a_sequence_created_and_left_behind_is_reclaimed() {
    let r = reclaimed(
        1,
        "fn main() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 7)\n}\n",
    );
    assert_eq!(r.value, "1");
}

#[test]
fn a_binding_that_is_reassigned_lets_go_of_what_it_held() {
    // Two sequences, one binding. The first has no other reference once the second is
    // stored, so it goes then rather than at the end of the function.
    let r = reclaimed(
        2,
        "fn main() -> Int {\n    let mut xs = ints_new();\n    ints_push(xs, 1);\n    \
         xs = ints_new();\n    ints_push(xs, 2);\n    ints_get(xs, 0)\n}\n",
    );
    assert_eq!(r.value, "2");
}

#[test]
fn a_parameter_is_borrowed_and_the_callee_does_not_reclaim_it() {
    // Rule 2. If the callee released what it borrowed, the caller's next read would be a
    // use-after-free — which is why the frame release skips parameter slots, and why this
    // reads the sequence again after the call rather than only counting.
    let r = reclaimed(
        1,
        "fn fill(xs: Ints) -> Int { ints_push(xs, 9) }\n\
         fn main() -> Int {\n    let xs = ints_new();\n    fill(xs);\n    \
         ints_get(xs, 0) + ints_len(xs)\n}\n",
    );
    assert_eq!(r.value, "10");
}

#[test]
fn a_returned_sequence_survives_the_function_that_made_it() {
    // Rule 3. The callee's frame release runs on the way out and must not take the value
    // it is returning with it.
    let r = reclaimed(
        1,
        "fn make() -> Ints {\n    let xs = ints_new();\n    ints_push(xs, 4);\n    xs\n}\n\
         fn main() -> Int {\n    let got = make();\n    ints_get(got, 0)\n}\n",
    );
    assert_eq!(r.value, "4");
}

#[test]
fn an_explicit_return_of_a_sequence_transfers_it() {
    // The same rule as the tail above, on a different edge — and they are different
    // edges: the tail is emitted by `emit_function` and a `return` by the statement
    // lowering. A mutation that broke only the second survived a suite that tested only
    // the first, which is how this test came to exist.
    //
    // The noise allocation after the call is deliberate. If the callee released what it
    // was returning, the caller would be reading storage the allocator has since handed
    // out again, and a use-after-free that nothing reuses is a use-after-free that reads
    // the right answer.
    let r = reclaimed(
        3,
        "fn make() -> Ints {\n    let xs = ints_new();\n    let mut i = 0;\n    \
         while i < 64 {\n        ints_push(xs, i + 100);\n        i = i + 1;\n    }\n    \
         return xs;\n}\n\
         fn noise() -> Int {\n    let ys = ints_new();\n    let mut i = 0;\n    \
         while i < 64 {\n        ints_push(ys, 7);\n        i = i + 1;\n    }\n    \
         ints_len(ys)\n}\n\
         fn main() -> Int {\n    let got = make();\n    noise();\n    noise();\n    \
         ints_get(got, 0) + ints_len(got)\n}\n",
    );
    assert_eq!(r.value, "164");
}

#[test]
fn a_sequence_created_in_one_branch_is_reclaimed_from_either() {
    // Both arms allocate, only one runs, and the count says exactly one was ever made.
    let r = reclaimed(
        1,
        "fn main() -> Int {\n    let xs = if 1 < 2 { ints_new() } else { ints_new() };\n    \
         ints_push(xs, 3)\n}\n",
    );
    assert_eq!(r.value, "1");
}

#[test]
fn a_sequence_created_in_one_branch_and_bound_outside_it_is_reclaimed() {
    // The escape the language does allow: an inner block's sequence assigned to a binding
    // that outlives the block. It is reclaimed when that binding dies, not when the block
    // ends, which is what "the last reference" means rather than "the closing brace".
    let r = reclaimed(
        2,
        "fn main() -> Int {\n    let mut outer = ints_new();\n    if 1 < 2 {\n        \
         let inner = ints_new();\n        ints_push(inner, 5);\n        outer = inner;\n    \
         } else {\n        ints_push(outer, 0);\n    }\n    ints_get(outer, 0)\n}\n",
    );
    assert_eq!(r.value, "5");
}

#[test]
fn a_loop_that_makes_one_sequence_an_iteration_holds_one_at_a_time() {
    // The shape the leak was worst in: the live set is one sequence and the allocation
    // count is the iteration count.
    let r = reclaimed(
        200,
        "fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
         while i < 200 {\n        let each = ints_new();\n        ints_push(each, i);\n        \
         total = total + ints_get(each, 0);\n        i = i + 1;\n    }\n    total\n}\n",
    );
    assert_eq!(r.value, "19900");
}

#[test]
fn an_early_return_reclaims_what_the_frame_was_holding() {
    // The exit a lexical rule would have missed. The sequence is created, the `return`
    // leaves from inside a loop inside an `if`, and the frame is released on that edge.
    let r = reclaimed(
        1,
        "fn find() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 1);\n    \
         let mut i = 0;\n    while i < 10 {\n        if i == 3 { return ints_len(xs) + i; }\n        \
         i = i + 1;\n    }\n    0\n}\n\
         fn main() -> Int { find() }\n",
    );
    assert_eq!(r.value, "4");
}

#[test]
fn a_failed_operation_reclaims_on_its_way_out() {
    // §18: cleanup may not exist only on the path that succeeds. The division fails after
    // the sequence exists, the failure unwinds through every frame, and every frame
    // releases what it held on the way.
    let (r, rendered) = both_failing(
        "fn inner(xs: Ints) -> Int {\n    let mine = ints_new();\n    ints_push(mine, 1);\n    \
         ints_len(xs) / 0\n}\n\
         fn main() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 2);\n    inner(xs)\n}\n",
    );
    assert_eq!(r.sequences, expected(2), "{r:?}");
    assert!(rendered.contains("N0401"), "{rendered}");
}

#[test]
fn a_nested_scope_reclaims_what_was_made_inside_it() {
    // A `scope` with no task in it is still a scope, and the sequence created inside is
    // still reclaimed — through the enclosing function's exit, because a block is not a
    // lifetime in this language and the constitution does not pretend it is.
    let r = reclaimed(
        1,
        "fn main() -> Int {\n    let mut n = 0;\n    scope {\n        \
         let xs = ints_new();\n        ints_push(xs, 6);\n        n = ints_get(xs, 0);\n    }\n    n\n}\n",
    );
    assert_eq!(r.value, "6");
}

#[test]
fn two_bindings_on_one_sequence_reclaim_it_once() {
    // Aliasing is specified and unchanged: the push through one is visible through the
    // other, and the storage goes exactly once when both are gone.
    let r = reclaimed(
        1,
        "fn main() -> Int {\n    let a = ints_new();\n    let b = a;\n    \
         ints_push(a, 7);\n    ints_get(b, 0)\n}\n",
    );
    assert_eq!(r.value, "7");
}

#[test]
fn mutation_through_an_alias_is_unchanged_by_reclamation() {
    // The property N7 promised not to break, checked rather than asserted: three bindings,
    // a mutation through the third, read through the first.
    let r = reclaimed(
        1,
        "fn main() -> Int {\n    let a = ints_new();\n    let b = a;\n    let c = b;\n    \
         ints_push(c, 1);\n    ints_push(b, 2);\n    ints_len(a) * 10 + ints_get(a, 1)\n}\n",
    );
    assert_eq!(r.value, "22");
}

#[test]
fn a_sequence_still_may_not_cross_into_a_task() {
    // Rule 5, and the reason the reference count needs no atomic. If this ever became
    // legal, `nz.arr_retain` would have to become an atomic in the same commit.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("m.nz"),
        "fn use_seq(xs: Ints) -> Int { ints_push(xs, 1) }\n\
         fn main() -> Int {\n    let xs = ints_new();\n    scope { spawn use_seq(xs); }\n    \
         ints_len(xs)\n}\n",
    )
    .expect("write");
    let out = Command::new(NAZM)
        .args(["check", "m.nz"])
        .current_dir(dir.path())
        .output()
        .expect("check");
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("N0321"), "{text}");
}

// ---------------------------------------------------------------------------
// Strings: the four origins, and what each one owns
// ---------------------------------------------------------------------------

#[test]
fn a_string_heavy_loop_holds_one_string_at_a_time() {
    // The N7 boundary this replaces. Until N8 a program with no sequence printed no
    // report at all, and this exact loop — fifty concatenations for one live result — was
    // the thing the constitution said it could not account for. It accounts for it now:
    // fifty backings created, fifty reclaimed, nothing live at the end.
    let r = strings(
        50,
        "fn main() -> Int {\n    let mut s = \"\";\n    let mut i = 0;\n    \
         while i < 50 {\n        s = str_concat(s, \"x\");\n        i = i + 1;\n    }\n    \
         str_len(s)\n}\n",
    );
    assert_eq!(r.value, "50");
}

#[test]
fn a_literal_owns_nothing_and_is_never_counted() {
    // **Immortal.** A literal is in the object's read-only data; binding, slicing and
    // comparing one allocates nothing, and counting it would make every program leak.
    let r = strings(
        0,
        "fn main() -> Int {\n    let a = \"hello\";\n    let b = a;\n    \
         str_len(str_slice(b, 1, 4))\n}\n",
    );
    assert_eq!(r.value, "3");
}

#[test]
fn a_process_argument_is_external_storage_in_both_implementations() {
    // The fourth provenance, and the one a program cannot make for itself: the bytes
    // belong to the operating system. Neither implementation counts them and neither may
    // free them — so a program that reads an argument and slices it allocates **nothing**,
    // and both agree about that rather than one of them quietly copying.
    let r = both_with_args(
        "fn main() -> Int {\n    let a = arg(0);\n    let b = str_slice(a, 0, 1);\n    \
         str_len(a) * 1000 + str_byte(b, 0)\n}\n",
        &["hello"],
    );
    assert_eq!(r.value, "5104");
    assert_eq!(r.strings, expected(0), "{r:?}");
}

#[test]
fn a_slice_keeps_its_backing_alive_after_the_original_is_gone() {
    // The case `{ pointer, length }` could not express. `make` allocates, slices, and
    // returns the slice — so the value that leaves the function points into the middle of
    // a buffer whose only binding died on the way out. The churn afterwards is there to
    // make a use-after-free read reused storage rather than the right answer by luck.
    let r = strings(
        21,
        "fn make() -> Str {\n    let s = str_concat(\"ab\", \"cd\");\n    \
         str_slice(s, 1, 3)\n}\n\
         fn main() -> Int {\n    let x = make();\n    let mut i = 0;\n    \
         let mut noise = \"\";\n    while i < 20 {\n        \
         noise = str_concat(noise, \"zz\");\n        i = i + 1;\n    }\n    \
         str_byte(x, 0) * 1000 + str_byte(x, 1)\n}\n",
    );
    // `bc`, still, after the buffer it points into lost its only binding.
    assert_eq!(r.value, "98099");
}

#[test]
fn a_slice_of_a_slice_of_a_heap_string_is_safe() {
    let r = strings(
        1,
        "fn main() -> Int {\n    let s = str_concat(\"abcdef\", \"\");\n    \
         let a = str_slice(s, 1, 5);\n    let b = str_slice(a, 1, 3);\n    \
         str_byte(b, 0)\n}\n",
    );
    assert_eq!(r.value, "99");
}

#[test]
fn an_empty_slice_and_an_empty_string_are_ordinary() {
    // `docs/spec.md` gives an empty `Str` real storage so its pointer stays valid. An
    // empty *slice* of an immortal string owns nothing, and an empty concatenation owns a
    // backing like any other.
    let r = strings(
        1,
        "fn main() -> Int {\n    let e = str_slice(\"abc\", 1, 1);\n    \
         let f = str_concat(\"\", \"\");\n    str_len(e) + str_len(f) + \
         str_len(str_slice(f, 0, 0))\n}\n",
    );
    assert_eq!(r.value, "0");
}

#[test]
fn joining_an_empty_sequence_still_owns_its_result() {
    // §51. `str_join` allocates one byte more than it needs so that an empty result still
    // owns storage and its pointer stays valid to compare — which means an empty join is
    // an ordinary heap string with an ordinary owner, not a special case that later
    // cannot be released coherently.
    let r = strings(
        1,
        "fn main() -> Int {\n    let xs = strs_new();\n    \
         let e = str_join(xs, \",\");\n    \
         str_len(e) + if e == \"\" { 7 } else { 0 }\n}\n",
    );
    assert_eq!(r.value, "7");
}

#[test]
fn a_returned_borrowed_string_survives_the_call_that_borrowed_it() {
    // Rule 2 and rule 3 meeting. The parameter is borrowed, so the caller's value stays
    // valid; the result is a reference the caller receives, so it must be taken **before**
    // the callee's frame lets go. Both edges, because N7 proved they are different code.
    let r = strings(
        1,
        "fn same(s: Str) -> Str { s }\n\
         fn same_returned(s: Str) -> Str { return s; }\n\
         fn main() -> Int {\n    let s = str_concat(\"ab\", \"cd\");\n    \
         let a = same(s);\n    let b = same_returned(s);\n    \
         str_len(a) + str_len(b) + str_len(s)\n}\n",
    );
    assert_eq!(r.value, "12");
}

#[test]
fn a_string_stored_in_a_sequence_outlives_the_binding_it_came_from() {
    // §14. `strs_push` borrows its argument and keeps what it was given, so the stored
    // element needs a reference of its own. `s` is rebound afterwards, which releases
    // what it held — and the sequence's element must not go with it.
    let r = strings(
        21,
        "fn main() -> Int {\n    let xs = strs_new();\n    \
         let mut s = str_concat(\"ab\", \"cd\");\n    strs_push(xs, s);\n    \
         s = \"\";\n    let mut i = 0;\n    while i < 20 {\n        \
         s = str_concat(s, \"z\");\n        i = i + 1;\n    }\n    \
         str_len(strs_get(xs, 0)) + str_byte(strs_get(xs, 0), 0)\n}\n",
    );
    assert_eq!(r.value, "101");
}

#[test]
fn a_string_read_out_of_a_sequence_survives_being_overwritten() {
    // §15, and the behaviour `docs/spec.md` has promised since sequences existed: a `Str`
    // already read out of a sequence is unaffected by changing the sequence. It used to
    // be true because nothing was ever freed. It is true now because `strs_get` hands
    // back its own reference and `strs_set` releases the old element only after taking
    // the new one.
    let r = strings(
        22,
        "fn main() -> Int {\n    let xs = strs_new();\n    \
         strs_push(xs, str_concat(\"ab\", \"cd\"));\n    let s = strs_get(xs, 0);\n    \
         strs_set(xs, 0, str_concat(\"wx\", \"yz\"));\n    let mut i = 0;\n    \
         let mut noise = \"\";\n    while i < 20 {\n        \
         noise = str_concat(noise, \"q\");\n        i = i + 1;\n    }\n    \
         str_byte(s, 0)\n}\n",
    );
    assert_eq!(r.value, "97");
}

#[test]
fn a_string_read_out_of_a_temporary_sequence_outlives_it() {
    // The case a mutation found, and the only one where `strs_get`'s own reference is
    // load-bearing.
    //
    // Everywhere else the binding compensates: `let s = strs_get(xs, 0);` retains what it
    // stores whether or not the built-in already did, so removing the retain from
    // `strs_get` changed nothing a test could see. It does not compensate here, and the
    // reason is the order. The sequence has **no binding** — it is the result of a call,
    // and a built-in's arguments are released as temporaries the moment it returns — so
    // without a reference taken *inside* `strs_get`, the sequence and every string in it
    // are freed first and the binding's retain then touches storage that is gone.
    //
    // The churn afterwards is so that a use-after-free reads reused storage rather than
    // the right answer by luck.
    let r = strings(
        21,
        "fn one() -> Strs {\n    let xs = strs_new();\n    \
         strs_push(xs, str_concat(\"ab\", \"cd\"));\n    xs\n}\n\
         fn main() -> Int {\n    let s = strs_get(one(), 0);\n    let mut i = 0;\n    \
         let mut noise = \"\";\n    while i < 20 {\n        \
         noise = str_concat(noise, \"qq\");\n        i = i + 1;\n    }\n    \
         str_len(s) * 100 + str_byte(s, 0)\n}\n",
    );
    assert_eq!(r.value, "497");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn replacing_an_element_with_itself_is_safe() {
    // §16's ordering case. If `strs_set` released the old element before taking the new
    // one, this would free the backing and then store a pointer into it.
    let r = strings(
        1,
        "fn main() -> Int {\n    let xs = strs_new();\n    \
         strs_push(xs, str_concat(\"ab\", \"cd\"));\n    \
         strs_set(xs, 0, strs_get(xs, 0));\n    str_byte(strs_get(xs, 0), 0)\n}\n",
    );
    assert_eq!(r.value, "97");
}

#[test]
fn a_sequence_of_strings_releases_its_contents_when_it_goes() {
    // §17. Freeing only the array of descriptors would turn a leaked string into a leaked
    // string inside a sequence, which is the same leak with a longer name.
    let r = strings(
        20,
        "fn build() -> Int {\n    let xs = strs_new();\n    let mut i = 0;\n    \
         while i < 20 {\n        strs_push(xs, int_to_str(i));\n        i = i + 1;\n    }\n    \
         strs_len(xs)\n}\n\
         fn main() -> Int { build() }\n",
    );
    assert_eq!(r.value, "20");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn a_popped_string_transfers_out_of_the_sequence() {
    // Popping shortens the sequence, so the sequence gives up its reference and the
    // expression takes that same one over. No retain, no release, and the value is still
    // valid after the sequence it came from is gone.
    let r = strings(
        4,
        "fn last() -> Str {\n    let xs = strs_new();\n    \
         strs_push(xs, int_to_str(11));\n    strs_push(xs, int_to_str(22));\n    \
         strs_pop(xs)\n}\n\
         fn main() -> Int { str_len(last()) + str_byte(last(), 0) }\n",
    );
    assert_eq!(r.value, "52");
}

#[test]
fn a_string_is_released_on_the_failure_path_too() {
    // §21. The shared unwind block is an exit edge like any other, and a program that
    // failed still gives back what it was holding.
    let (r, text) = both_failing(
        "fn main() -> Int {\n    let s = str_concat(\"ab\", \"cd\");\n    \
         let xs = ints_new();\n    ints_get(xs, 0)\n}\n",
    );
    assert!(text.contains("N0405"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn an_embedded_zero_is_still_an_ordinary_byte() {
    // §52. Ownership metadata must not quietly turn a `Str` into a C string: the length
    // is the length, and a NUL in the middle is data.
    let r = strings(
        3,
        "fn main() -> Int {\n    let z = str_from_byte(0);\n    \
         let s = str_concat(str_concat(\"a\", z), \"b\");\n    \
         str_len(s) * 1000 + str_byte(s, 1) * 10 + str_byte(str_slice(s, 1, 3), 1)\n}\n",
    );
    assert_eq!(r.value, "3098");
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

#[test]
fn a_channel_is_reclaimed_when_its_last_binding_goes() {
    let r = channels(
        1,
        "fn main() -> Int {\n    let c = chan_new(2);\n    chan_send(c, 7);\n    \
         let buf = ints_new();\n    chan_recv(c, buf);\n    ints_get(buf, 0)\n}\n",
    );
    assert_eq!(r.value, "7");
}

#[test]
fn closing_a_channel_is_not_destroying_it() {
    // §20. A closed channel still has live references and still answers; it is destroyed
    // when the last one goes, which is a different event.
    let r = channels(
        1,
        "fn main() -> Int {\n    let c = chan_new(2);\n    chan_send(c, 3);\n    \
         chan_close(c);\n    let buf = ints_new();\n    \
         let first = chan_recv(c, buf);\n    let second = chan_recv(c, buf);\n    \
         ints_get(buf, 0) + ints_len(buf)\n}\n",
    );
    assert_eq!(r.value, "4");
}

#[test]
fn many_short_lived_channels_hold_one_at_a_time() {
    // §43. Live count tracks simultaneously live channels, not total channels ever made.
    let r = channels(
        30,
        "fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
         while i < 30 {\n        let c = chan_new(1);\n        chan_send(c, i);\n        \
         let buf = ints_new();\n        chan_recv(c, buf);\n        \
         total = total + ints_get(buf, 0);\n        i = i + 1;\n    }\n    total\n}\n",
    );
    assert_eq!(r.value, "435");
}

#[test]
fn a_returned_channel_keeps_a_surviving_reference() {
    // §25, on both edges.
    let r = channels(
        2,
        "fn make() -> Chan { let c = chan_new(1); c }\n\
         fn made() -> Chan { let c = chan_new(1); return c; }\n\
         fn same(c: Chan) -> Chan { c }\n\
         fn main() -> Int {\n    let a = same(make());\n    let b = made();\n    \
         chan_send(a, 4);\n    chan_send(b, 5);\n    let buf = ints_new();\n    \
         chan_recv(a, buf);\n    chan_recv(b, buf);\n    \
         ints_get(buf, 0) + ints_get(buf, 1)\n}\n",
    );
    assert_eq!(r.value, "9");
}

#[test]
fn a_channel_shared_with_tasks_outlives_every_one_of_them() {
    // §26. The parent's binding and each task's argument are separate references, and the
    // channel goes when the last of them does — after the scope has joined.
    let r = channels(
        1,
        "fn feed(c: Chan, n: Int) -> Int { chan_send(c, n); 0 }\n\
         fn main() -> Int {\n    let c = chan_new(4);\n    \
         scope {\n        spawn feed(c, 1);\n        spawn feed(c, 2);\n    }\n    \
         chan_close(c);\n    let buf = ints_new();\n    \
         while chan_recv(c, buf) {\n    }\n    \
         ints_get(buf, 0) + ints_get(buf, 1)\n}\n",
    );
    assert_eq!(r.value, "3");
}

#[test]
fn a_blocked_receiver_woken_by_a_close_leaves_nothing_behind() {
    // §44. The receiver is inside `chan_recv` with the channel empty when the parent
    // closes it. It wakes, is told the channel is drained, and ends — and the channel is
    // destroyed only after the scope has joined, which is what makes destroying its mutex
    // and condition variable safe.
    let r = channels(
        1,
        "fn drain(c: Chan) -> Int {\n    let buf = ints_new();\n    \
         while chan_recv(c, buf) {\n    }\n    ints_len(buf)\n}\n\
         fn main() -> Int {\n    let c = chan_new(2);\n    \
         scope {\n        spawn drain(c);\n        chan_send(c, 1);\n        \
         chan_send(c, 2);\n        chan_close(c);\n    }\n    0\n}\n",
    );
    assert_eq!(r.value, "0");
}

#[test]
fn a_blocked_sender_woken_by_a_close_leaves_nothing_behind() {
    // The other half. Capacity one, so the task blocks on its second send; the parent
    // closes, the send refuses, and the task stops rather than queueing into something
    // nobody will drain.
    let r = channels(
        1,
        "fn feed(c: Chan) -> Int {\n    let mut i = 0;\n    \
         while i < 50 {\n        if chan_send(c, i) == false { return i; }\n        \
         i = i + 1;\n    }\n    i\n}\n\
         fn main() -> Int {\n    let c = chan_new(1);\n    let buf = ints_new();\n    \
         scope {\n        spawn feed(c);\n        chan_recv(c, buf);\n        \
         chan_close(c);\n    }\n    ints_len(buf)\n}\n",
    );
    assert_eq!(r.value, "1");
}

#[test]
fn a_task_that_fails_while_holding_a_channel_still_gives_it_back() {
    // A failure inside a task is reported at the scope's closing brace, after every
    // sibling has been joined — and on the way out the task's frame releases what it was
    // holding, exactly as a function's does. A channel left behind here would be a leak on
    // the one path where cleanup is hardest to reach.
    let (r, text) = both_failing(
        "fn bad(c: Chan) -> Int {\n    let xs = ints_new();\n    \
         chan_send(c, 1);\n    ints_get(xs, 0)\n}\n\
         fn main() -> Int {\n    let c = chan_new(2);\n    \
         scope {\n        spawn bad(c);\n    }\n    0\n}\n",
    );
    assert!(text.contains("N0405"), "{text}");
    assert_eq!(r.channels, expected(1), "{r:?}");
    assert_eq!(r.sequences.live, 0, "{r:?}");
}

#[test]
fn a_string_crossing_into_a_task_keeps_its_backing() {
    // §32. The task reads bytes the parent allocated, and the parent rebinds while the
    // task is running — so the reference the spawn site took is the only thing keeping
    // them valid.
    let r = both(
        "fn use_it(c: Chan, s: Str) -> Int { chan_send(c, str_byte(s, 0)); 0 }\n\
         fn main() -> Int {\n    let c = chan_new(4);\n    \
         let mut s = str_concat(\"ab\", \"cd\");\n    \
         scope {\n        spawn use_it(c, s);\n        spawn use_it(c, str_slice(s, 1, 3));\n        \
         s = \"\";\n    }\n    chan_close(c);\n    let buf = ints_new();\n    \
         while chan_recv(c, buf) {\n    }\n    ints_len(buf)\n}\n",
    );
    assert_eq!(r.value, "2");
    assert_eq!(r.strings.live, 0, "{r:?}");
    assert_eq!(r.channels, expected(1), "{r:?}");
}

#[test]
fn the_report_is_silent_unless_it_is_asked_for() {
    // A compiler that reports on itself on every run is a compiler whose output has to be
    // filtered, and this one is a diagnostic rather than a feature.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("m.nz"),
        "fn main() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 1)\n}\n",
    )
    .expect("write");
    for command in [vec!["run", "m.nz"], vec!["build", "m.nz", "-o", "prog"]] {
        let out = Command::new(NAZM)
            .args(&command)
            .current_dir(dir.path())
            .output()
            .expect("run");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("nazm-memory:"),
            "{command:?} reported without being asked"
        );
    }
    let quiet = Command::new(dir.path().join("prog"))
        .current_dir(dir.path())
        .output()
        .expect("execute");
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("nazm-memory:"));
}

#[test]
fn a_program_that_calls_exit_with_reclaims_nothing_and_says_nothing() {
    // A real boundary, found by measuring the self-hosted compiler and getting no report
    // at all: `compiler/emit.nz` ends with `exit_with` rather than returning, so the
    // process is gone before any frame unwinds.
    //
    // That is correct rather than a gap. Reclamation is about storage *while a program
    // runs*; a process that has ended has returned everything to the operating system, and
    // running cleanup on the way out would be work whose only observable effect is to make
    // the program slower. It is a test rather than a comment because the alternative —
    // someone later "fixing" this by draining the frame before `exit` — would cost real
    // time on the largest workload this project has.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("m.nz"),
        "fn main() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 1);\n    \
         exit_with(0)\n}\n",
    )
    .expect("write");
    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(built.status.success(), "{:?}", built);
    let native = Command::new(dir.path().join("prog"))
        .env(MEMORY_VARIABLE, "1")
        .current_dir(dir.path())
        .output()
        .expect("execute");
    assert_eq!(native.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&native.stderr).contains("nazm-memory:"),
        "a process that exited should not have reached the entry wrapper"
    );
}

// --- records -------------------------------------------------------------------------
//
// A record allocates nothing of its own: it has no header, no count and no storage. What
// these cases are about is its **fields** — that copying one takes a reference per owning
// field, that destroying one gives every reference back, and that both happen at the same
// moment in both implementations. `docs/spec.md`, *Records are values whose fields keep
// their own semantics*.

#[test]
fn a_record_of_scalars_allocates_nothing() {
    // The floor. If this ever reports an allocation, a record has acquired storage of its
    // own and every case below is measuring something else.
    let r = both(
        "struct Point {\n    x: Int,\n    y: Int,\n}\n\
         fn main() -> Int {\n    let mut p = Point(x: 1, y: 2);\n    \
         let q = p;\n    p.x = 10;\n    p.x + q.x + p.y\n}\n",
    );
    assert_eq!(r.value, "13");
    assert_eq!(r.strings, expected(0), "{r:?}");
    assert_eq!(r.sequences, expected(0), "{r:?}");
}

#[test]
fn a_record_holding_a_string_gives_the_backing_back() {
    let r = strings(
        1,
        "struct Holder {\n    name: Str,\n}\n\
         fn main() -> Int {\n    let h = Holder(name: str_concat(\"ab\", \"cd\"));\n    \
         str_len(h.name)\n}\n",
    );
    assert_eq!(r.value, "4");
}

#[test]
fn a_record_holding_a_slice_keeps_the_buffer_it_points_into() {
    // A sliced `Str` in a field is a view: the field holds the *same* backing, so the
    // record keeps alive what it is looking at rather than a copy of it. One allocation,
    // one reclamation, and the slice outlives the binding it was cut from.
    let r = strings(
        1,
        "struct Holder {\n    part: Str,\n}\n\
         fn main() -> Int {\n    let h = Holder(part: str_slice(str_concat(\"abcd\", \"ef\"), 1, 4));\n    \
         str_len(h.part) * 100 + str_byte(h.part, 0)\n}\n",
    );
    assert_eq!(r.value, "398");
}

#[test]
fn copying_a_record_takes_one_reference_per_owning_field() {
    // Two bindings on one record, and the string behind it reclaimed once. That the
    // second copy did not allocate is the compositional claim; that it did not
    // double-release is the one a missing retain would break.
    let r = strings(
        2,
        "struct Two {\n    a: Str,\n    b: Str,\n}\n\
         fn main() -> Int {\n    let x = Two(a: int_to_str(11), b: int_to_str(22));\n    \
         let y = x;\n    let z = y;\n    str_len(x.a) + str_len(y.b) + str_len(z.a)\n}\n",
    );
    assert_eq!(r.value, "6");
}

#[test]
fn a_record_field_is_another_reference_to_the_same_sequence() {
    // `docs/spec.md`'s table, as a measurement: the `Ints` field is a handle, so both
    // copies name one storage — one allocation for two records — and it goes when the
    // last of them does.
    let r = reclaimed(
        1,
        "struct Work {\n    values: Ints,\n}\n\
         fn main() -> Int {\n    let a = Work(values: ints_new());\n    \
         let b = a;\n    ints_push(a.values, 7);\n    \
         ints_get(b.values, 0) + ints_len(b.values)\n}\n",
    );
    assert_eq!(r.value, "8");
}

#[test]
fn a_record_field_of_strings_releases_its_contents() {
    let r = both(
        "struct Bag {\n    items: Strs,\n}\n\
         fn main() -> Int {\n    let g = Bag(items: strs_new());\n    \
         strs_push(g.items, int_to_str(1));\n    strs_push(g.items, int_to_str(2));\n    \
         let h = g;\n    strs_len(h.items)\n}\n",
    );
    assert_eq!(r.value, "2");
    assert_eq!(r.sequences, expected(1), "{r:?}");
    assert_eq!(r.strings, expected(2), "{r:?}");
}

#[test]
fn a_record_holding_a_channel_reclaims_it() {
    let r = channels(
        1,
        "struct Line {\n    c: Chan,\n}\n\
         fn main() -> Int {\n    let l = Line(c: chan_new(2));\n    \
         let m = l;\n    chan_send(m.c, 5);\n    chan_close(l.c);\n    \
         let got = ints_new();\n    while chan_recv(m.c, got) {\n    }\n    \
         ints_get(got, 0)\n}\n",
    );
    assert_eq!(r.value, "5");
}

#[test]
fn a_nested_record_releases_through_both_levels() {
    // The composition, one level in: `Outer` owns nothing directly, and destroying one
    // still gives back the string two levels down. A drop that did not recurse would
    // leave it live.
    let r = both(
        "struct Inner {\n    text: Str,\n}\n\
         struct Outer {\n    inner: Inner,\n    values: Ints,\n}\n\
         fn main() -> Int {\n    let o = Outer(inner: Inner(text: int_to_str(77)), values: ints_new());\n    \
         ints_push(o.values, 1);\n    str_len(o.inner.text) + ints_len(o.values)\n}\n",
    );
    assert_eq!(r.value, "3");
    assert_eq!(r.strings, expected(1), "{r:?}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn replacing_a_whole_record_releases_the_one_it_replaced() {
    // Two strings allocated, one live at a time, both reclaimed — and the self-assignment
    // in the middle must not free what it is about to store.
    let r = strings(
        2,
        "struct Holder {\n    name: Str,\n}\n\
         fn main() -> Int {\n    let mut h = Holder(name: int_to_str(111));\n    \
         h = h;\n    h = Holder(name: int_to_str(2222));\n    str_len(h.name)\n}\n",
    );
    assert_eq!(r.value, "4");
}

#[test]
fn replacing_one_field_releases_the_one_it_replaced() {
    let r = strings(
        3,
        "struct Holder {\n    name: Str,\n    n: Int,\n}\n\
         fn main() -> Int {\n    let mut h = Holder(name: int_to_str(1), n: 0);\n    \
         h.name = int_to_str(22);\n    h.name = str_concat(h.name, \"x\");\n    \
         str_len(h.name)\n}\n",
    );
    assert_eq!(r.value, "3");
}

#[test]
fn replacing_a_field_with_something_backed_by_the_same_allocation_is_safe() {
    // `p.name = p.name` and `p.name = str_slice(p.name, …)` both produce a new value that
    // shares the old one's backing. Releasing the old field before taking the new one
    // would free storage the new value points into — so the order is what this measures,
    // and a wrong order is a use-after-free rather than a leak.
    let r = strings(
        1,
        "struct Holder {\n    name: Str,\n}\n\
         fn main() -> Int {\n    let mut h = Holder(name: str_concat(\"abcdef\", \"\"));\n    \
         h.name = h.name;\n    h.name = str_slice(h.name, 1, 4);\n    \
         str_len(h.name) * 100 + str_byte(h.name, 0)\n}\n",
    );
    assert_eq!(r.value, "398");
}

#[test]
fn a_record_returned_as_a_tail_and_by_return_reclaims_the_same() {
    // N7 and N8 both found these to be different implementation edges, so both are here.
    let r = strings(
        2,
        "struct Holder {\n    name: Str,\n}\n\
         fn tail() -> Holder { Holder(name: int_to_str(1)) }\n\
         fn explicit() -> Holder {\n    let h = Holder(name: int_to_str(22));\n    return h;\n}\n\
         fn main() -> Int { str_len(tail().name) + str_len(explicit().name) }\n",
    );
    assert_eq!(r.value, "3");
}

#[test]
fn a_field_read_out_of_a_temporary_outlives_it() {
    // The case `docs/spec.md` names: the record `make()` returned is destroyed on this
    // line, and the string read out of it is still valid. A projection that borrowed
    // rather than taking its own reference would leave `s` pointing at freed bytes — and
    // because the binding retains what it stores, only a shape where *nothing else* holds
    // the value can see it.
    let r = strings(
        1,
        "struct Holder {\n    name: Str,\n}\n\
         fn make() -> Holder { Holder(name: str_concat(\"ab\", \"cd\")) }\n\
         fn main() -> Int {\n    let s = make().name;\n    \
         str_len(s) * 100 + str_byte(s, 3)\n}\n",
    );
    assert_eq!(r.value, "500");
}

#[test]
fn a_construction_that_fails_part_way_through_cleans_what_it_built() {
    // §65's case. The first initialiser allocates a string, the second fails, and the
    // record never exists — so the string is nobody's and must still be given back. A
    // value held only in a register is not reachable from the frame's cleanup, which is
    // why a construction with an owning field is assembled in a slot.
    let (r, text) = both_failing(
        "struct Two {\n    a: Str,\n    b: Int,\n}\n\
         fn main() -> Int {\n    let n = 0;\n    \
         let t = Two(a: str_concat(\"ab\", \"cd\"), b: 1 / n);\n    t.b\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
}

#[test]
fn a_nested_construction_that_fails_cleans_the_inner_record_too() {
    // The same, one level in: the inner record was completed and moved into no outer one.
    let (r, text) = both_failing(
        "struct Inner {\n    text: Str,\n}\n\
         struct Outer {\n    inner: Inner,\n    values: Ints,\n    n: Int,\n}\n\
         fn main() -> Int {\n    let z = 0;\n    \
         let o = Outer(inner: Inner(text: int_to_str(5)), values: ints_new(), n: 1 / z);\n    o.n\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn a_failing_replacement_leaves_the_old_field_valid() {
    // §66. The right-hand side fails, so nothing is stored — and the old field is still
    // the record's and is released by the frame on the way out, exactly once.
    let (r, text) = both_failing(
        "struct Holder {\n    name: Str,\n}\n\
         fn main() -> Int {\n    let mut h = Holder(name: str_concat(\"ab\", \"cd\"));\n    \
         let z = 0;\n    h.name = int_to_str(1 / z);\n    str_len(h.name)\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
}

#[test]
fn a_record_is_released_on_the_failure_path_too() {
    let (r, text) = both_failing(
        "struct Holder {\n    name: Str,\n    values: Ints,\n}\n\
         fn main() -> Int {\n    let h = Holder(name: int_to_str(9), values: ints_new());\n    \
         ints_get(h.values, 0)\n}\n",
    );
    assert!(text.contains("N0405"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn a_task_safe_record_crosses_into_siblings_and_comes_back() {
    // A record with a heap string and a channel, handed to two tasks at once. Each task
    // slices what it was given, which is what makes the backing genuinely shared between
    // threads — the reason the count on it is atomic.
    let r = both(
        "struct Message {\n    id: Int,\n    text: Str,\n    reply: Chan,\n}\n\
         fn worker(m: Message) -> Int {\n    let t = str_slice(m.text, 0, 2);\n    \
         chan_send(m.reply, m.id + str_len(t));\n    0\n}\n\
         fn main() -> Int {\n    let c = chan_new(4);\n    \
         let m = Message(id: 1, text: str_concat(\"ab\", \"cd\"), reply: c);\n    \
         scope {\n        spawn worker(m);\n        spawn worker(m);\n    }\n    \
         chan_close(c);\n    let got = ints_new();\n    \
         while chan_recv(c, got) {\n    }\n    ints_get(got, 0) + ints_get(got, 1)\n}\n",
    );
    assert_eq!(r.value, "6");
    assert_eq!(r.strings, expected(1), "{r:?}");
    assert_eq!(r.channels, expected(1), "{r:?}");
}

#[test]
fn many_short_lived_records_hold_only_what_is_live() {
    // The scaling claim, as a count rather than as resident memory: four hundred string
    // backings created, four hundred reclaimed, and the live set constant. A record that
    // leaked its fields would report a live count that grew with the loop.
    let r = strings(
        400,
        "struct Pair {\n    a: Str,\n    b: Str,\n}\n\
         fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
         while i < 200 {\n        let p = Pair(a: int_to_str(i), b: int_to_str(i * 2));\n        \
         total = total + str_len(p.a);\n        i = i + 1;\n    }\n    total\n}\n",
    );
    assert_eq!(r.value, "490");
}

// --- a temporary outstanding when a failure leaves ------------------------------------

#[test]
fn a_temporary_still_in_hand_when_something_fails_is_released() {
    // Not about records at all, and found by a record case.
    //
    // The shared unwind block releases the frame's **slots**. The inner `str_concat` is
    // not in a slot — it is a value this expression is holding, on its way into the outer
    // one — and the operation that would have consumed it is on a path control never
    // takes. So it was leaked by every failing program of this shape, from N8 until here.
    //
    // It stayed invisible because a failure ends the process, so nothing downstream could
    // tell; because the two implementations were never compared on a *failing* program
    // holding a temporary; and because no test had this exact shape. A record projection
    // made the shape ordinary, and `a_record_is_released_on_the_failure_path_too` found it.
    let (r, text) = both_failing(
        "fn main() -> Int {\n    let z = 0;\n    \
         str_len(str_concat(str_concat(\"a\", \"b\"), int_to_str(1 / z)))\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
}

#[test]
fn a_temporary_is_released_when_a_callee_fails_too() {
    // The other failure edge: not a guard in this function, but the check after a call
    // that found the failure flag set. It took the short path straight to the function's
    // exit, which is right when nothing is outstanding and wrong when something is.
    let (r, text) = both_failing(
        "fn boom(n: Int) -> Int { 1 / n }\n\
         fn main() -> Int {\n    let z = 0;\n    \
         str_len(str_concat(str_concat(\"a\", \"b\"), int_to_str(boom(z))))\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.strings, expected(1), "{r:?}");
}

#[test]
fn a_sequence_temporary_is_released_when_a_failure_leaves() {
    // The same, for a handle rather than a string backing: `ints_new()` is born with one
    // reference and this expression is holding it when the division fails.
    let (r, text) =
        both_failing("fn main() -> Int {\n    let z = 0;\n    ints_len(ints_new()) + 1 / z\n}\n");
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn a_failure_in_one_branch_does_not_reach_across_to_the_other() {
    // The dominance half of the rule above. A value created in the `then` block does not
    // dominate the `else` block, so a failure over there may not release it — and the
    // first attempt at releasing outstanding temporaries did exactly that, which clang
    // refused as `Instruction does not dominate all uses`. A refusal, rather than a wrong
    // program, because the backend's output is checked by an assembler.
    //
    // The shape: a value-position `if` whose branches each produce a reference, and a
    // failure after the join.
    let (r, text) = both_failing(
        "fn main() -> Int {\n    let z = 0;\n    let c = z == 0;\n    \
         let xs = if c { ints_new() } else { ints_new() };\n    \
         ints_push(xs, 1);\n    ints_get(xs, 0) / z\n}\n",
    );
    assert!(text.contains("N0401"), "{text}");
    assert_eq!(r.sequences, expected(1), "{r:?}");
}

#[test]
fn a_record_built_in_a_branch_is_reclaimed_from_either() {
    let r = strings(
        1,
        "struct Holder {\n    name: Str,\n}\n\
         fn main() -> Int {\n    let c = 1 > 0;\n    \
         let h = if c { Holder(name: int_to_str(11)) } else { Holder(name: int_to_str(2)) };\n    \
         str_len(h.name)\n}\n",
    );
    assert_eq!(r.value, "2");
}
