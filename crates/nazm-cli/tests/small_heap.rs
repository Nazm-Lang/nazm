//! R1.5 3.1 (design D13): the small-block heap (`docs/architecture.md` §7.119).
//!
//! What a whole run shows: a program that calls no C allocates through it — strings, lists grown
//! from a small block past 512 bytes and back, maps — and prints what the interpreter prints, with
//! the same memory report and nothing live, on every backend; its units and runtime call C's
//! allocator nowhere but the small-block heap's own unit and the stack unit; a program of tasks
//! keeps its lists per thread and a program of one thread in plain globals; a program that calls
//! C, and one built with a heap limit, keep C's allocator; and blocks a task allocates and another
//! thread frees go back to C past the bound, so a producer and a consumer do not grow without one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential::{agrees, nazm};

use std::path::Path;
use std::process::Command;

/// Strings, a list grown past a small block and emptied, a map: every size class and the large
/// blocks, `realloc` from small to small, small to large and large to large.
const CHURNS: &str = "fn main() -> Int {
    let mut total = 0;
    let mut round = 0;
    while round < 50 {
        let xs = ints_new();
        let mut i = 0;
        while i < 300 {
            ints_push(xs, i * round);
            let s = str_concat(\"item \", int_to_str(i));
            total = total + str_len(s);
            i = i + 1;
        }
        while ints_len(xs) > 0 { total = total + ints_pop(xs) % 7; }
        let m = hashmap_new[Str, Int]();
        let mut k = 0;
        while k < 40 { let key = int_to_str(k % 13); m[key] += k; k = k + 1; }
        total = total + m.len();
        round = round + 1;
    }
    print(f\"{total}\\n\");
    0
}
";

/// Builds and frees strings in four tasks: the lists are each worker's own.
const TASKS: &str = "fn work(c: Chan, k: Int) -> Int {
    let mut total = 0;
    let mut i = 0;
    while i < 5000 {
        let s = str_concat(\"piece \", int_to_str(i + k));
        total = total + str_len(s);
        i = i + 1;
    }
    chan_send(c, total);
    0
}

fn main(out: OutCap, t: SpawnCap) -> Int ! {io, spawn} {
    let c = chan_new(32);
    scope { spawn work(c, 1); spawn work(c, 2); spawn work(c, 3); spawn work(c, 4); }
    let got = ints_new();
    let mut n = 0;
    while n < 4 { chan_recv(c, got); n = n + 1; }
    let mut sum = 0;
    let mut j = 0;
    while j < ints_len(got) { sum = sum + ints_get(got, j); j = j + 1; }
    print(f\"{sum}\\n\");
    0
}
";

/// A task allocates 200,000 strings of 200 bytes, and `main`, on another thread, frees them.
const HANDED_OVER: &str = "fn produce(c: Chan[Str], n: Int) -> Int {
    let mut pad = \"\";
    while str_len(pad) < 190 { pad = str_concat(pad, \"xxxxxxxxxx\"); }
    let mut i = 0;
    while i < n {
        chan_send_of(c, str_concat(pad, int_to_str(i)));
        i = i + 1;
    }
    0
}

fn main(out: OutCap, t: SpawnCap) -> Int ! {io, spawn} {
    let c = chan_new_of[Str](16);
    let got = vec_new[Str]();
    let mut total = 0;
    scope {
        spawn produce(c, 200000);
        let mut n = 0;
        while n < 200000 {
            chan_recv_of(c, got);
            total = total + str_len(vec_get(got, 0));
            vec_pop(got);
            n = n + 1;
        }
    }
    print(f\"{total}\\n\");
    0
}
";

/// `src` built with `--emit-ir` and `args`: each unit's name (`small`, `m0`, …) and text.
fn units(src: &str, args: &[&str]) -> Vec<(String, String)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut all = vec!["build", "--no-cache", "--emit-ir", "prog.nz", "-o", "ir"];
    all.extend(args);
    let b = nazm(d.path(), &all);
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let mut out: Vec<(String, String)> = std::fs::read_dir(d.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("ir.") && n.ends_with(".ll"))
        .map(|n| {
            let text = std::fs::read_to_string(d.path().join(&n)).unwrap();
            (
                n.trim_start_matches("ir.")
                    .trim_end_matches(".ll")
                    .to_owned(),
                text,
            )
        })
        .collect();
    out.sort();
    out
}

/// Only the small-block heap's unit and the stack unit call C's allocator.
fn small_heap_everywhere(units: &[(String, String)]) {
    assert!(units.iter().any(|(n, _)| n == "small"), "{units:?}");
    for (name, text) in units {
        let c = text.contains("@malloc(") || text.contains("@free(") || text.contains("@realloc(");
        assert_eq!(
            c,
            name == "small" || name == "stack",
            "{name} reaches C's allocator past the small-block heap"
        );
    }
}

#[test]
fn a_program_that_calls_no_c_allocates_through_the_small_block_heap_on_every_backend() {
    agrees(CHURNS, "152929\n0");
    small_heap_everywhere(&units(CHURNS, &[]));
    small_heap_everywhere(&units(CHURNS, &["--opt-level", "2"]));
}

#[test]
fn a_program_of_tasks_keeps_its_lists_per_thread_and_one_of_one_thread_in_globals() {
    agrees(TASKS, "195590\n0");
    let tasks = units(TASKS, &[]);
    small_heap_everywhere(&tasks);
    let lists = |u: &[(String, String)]| {
        let small = &u.iter().find(|(n, _)| n == "small").unwrap().1;
        small
            .lines()
            .find(|l| l.starts_with("@nz.small_lists = "))
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        lists(&tasks),
        "@nz.small_lists = internal thread_local global [66 x ptr] zeroinitializer"
    );
    assert_eq!(
        lists(&units(CHURNS, &[])),
        "@nz.small_lists = internal global [66 x ptr] zeroinitializer"
    );
}

#[test]
fn a_program_that_calls_c_or_has_a_heap_limit_keeps_c_s_allocator() {
    let calls_c = "extern \"C\" fn labs(x: Int) -> Int = \"labs\";
fn main(f: ForeignCap) -> Int { let s = int_to_str(labs(0 - 3)); str_len(s) - 1 }
";
    for (src, args) in [(calls_c, &[][..]), (CHURNS, &["--heap-limit", "64M"][..])] {
        let u = units(src, args);
        for (name, text) in &u {
            assert!(!text.contains("nz.small_"), "{name}: {text}");
        }
        assert!(u.iter().any(
            |(_, t)| t.contains("call ptr @malloc(") || t.contains("call ptr @nz.heap_malloc(")
        ));
    }
}

/// The peak resident memory of a run of `exe`, in bytes.
fn peak(exe: &Path) -> f64 {
    let o = Command::new("/usr/bin/time")
        .arg(if cfg!(target_os = "macos") {
            "-l"
        } else {
            "-v"
        })
        .arg(exe)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "39088890\n0\n");
    // macOS reports bytes, GNU time kilobytes.
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .find_map(|l| {
            let l = l.trim();
            if let Some(n) = l.strip_suffix("maximum resident set size") {
                n.trim().parse::<f64>().ok()
            } else {
                l.strip_prefix("Maximum resident set size (kbytes):")
                    .and_then(|n| n.trim().parse::<f64>().ok())
                    .map(|k| k * 1024.0)
            }
        })
        .unwrap()
}

#[test]
fn blocks_freed_on_another_thread_go_back_to_c_past_the_bound() {
    // Without the bound, `main`'s list would keep every block the task allocated — 200,000 of
    // 224 bytes, some 45 MB — while the task, its own list empty, asked C for each new one.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), HANDED_OVER).unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--opt-level",
            "2",
            "prog.nz",
            "-o",
            "prog",
        ],
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let rss = peak(&d.path().join(crate::common::exe_name("prog")));
    assert!(rss < 16.0 * 1024.0 * 1024.0, "peak {rss} bytes");
}
