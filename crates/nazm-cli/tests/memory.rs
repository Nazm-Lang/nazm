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

/// What one run reported: its value, and what it did with sequence storage.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    value: String,
    allocated: u64,
    reclaimed: u64,
    live: u64,
}

fn counts(stderr: &str, value: String) -> Ran {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report in: {stderr}"));
    let field = |name: &str| {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(name))
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("no `{name}` in: {line}"))
    };
    Ran {
        value,
        allocated: field("allocated="),
        reclaimed: field("reclaimed="),
        live: field("live="),
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

fn reclaimed(n: u64, source: &str) -> Ran {
    let r = both(source);
    assert_eq!(r.allocated, n, "{r:?}");
    assert_eq!(r.reclaimed, n, "{r:?}");
    assert_eq!(r.live, 0, "{r:?}");
    r
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
    assert_eq!(r.allocated, 2, "{r:?}");
    assert_eq!(r.reclaimed, 2, "{r:?}");
    assert_eq!(r.live, 0, "{r:?}");
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
// What the slice does not cover, stated as a test so the claim cannot drift
// ---------------------------------------------------------------------------

#[test]
fn a_program_with_no_sequences_reports_nothing_and_reclaims_nothing() {
    // `Str` storage is outside this slice, and the constitution says so. A string-heavy
    // program is not reported on at all rather than reported as clean.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("m.nz"),
        "fn main() -> Int {\n    let mut s = \"\";\n    let mut i = 0;\n    \
         while i < 50 {\n        s = str_concat(s, \"x\");\n        i = i + 1;\n    }\n    \
         str_len(s)\n}\n",
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
    assert_eq!(
        String::from_utf8_lossy(&native.stdout).trim(),
        "50",
        "the program should still work"
    );
    assert!(
        !String::from_utf8_lossy(&native.stderr).contains("nazm-memory:"),
        "a program with no sequences should report nothing"
    );
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
