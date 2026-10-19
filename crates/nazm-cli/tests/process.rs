//! Gate 2: process and environment.
//!
//! `docs/internal/general-purpose.md` §12 and the spec's *Process and environment*. What only a whole run
//! shows: that `@std/process` runs a program with input, collecting its output, errors and status —
//! a signal's ending as 256 and the signal — even when both streams carry more than a pipe holds;
//! that a missing program is `NotFound`; that a task reading a child's pipe parks on the pool; that
//! the environment and standard input read alike; and that running a program needs a `ProcessCap`
//! and may not be chosen by what a file says — in the interpreter and both native backends.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use std::io::Write as _;
use std::process::{Command, Stdio};

use differential::{codes, nazm};

/// `prog.nz` under the interpreter and built by each backend, run with `env` set and `input` on
/// standard input: each implementation's label, exit code and standard output.
fn everywhere_with(
    src: &str,
    env: &[(&str, &str)],
    input: &str,
) -> Vec<(String, Option<i32>, String)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut runs: Vec<(String, Command)> = Vec::new();
    let mut interp = Command::new(differential::NAZM);
    interp
        .current_dir(d.path())
        .args(["run", "--interpret", "prog.nz"]);
    runs.push(("interpreter".to_owned(), interp));
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(crate::common::exe_name(backend));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path());
        runs.push((backend.to_owned(), c));
    }
    runs.into_iter()
        .map(|(label, mut c)| {
            for (k, v) in env {
                c.env(k, v);
            }
            let mut child = c
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let o = child.wait_with_output().unwrap();
            (
                label,
                o.status.code(),
                String::from_utf8_lossy(&o.stdout).into_owned(),
            )
        })
        .collect()
}

const RUN: &str = "use \"@std/ioerror\";\nuse \"@std/process\";\n\
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, \"\\n\")) }\n\
fn args(a: Str, b: Str) -> Strs { let x = strs_new(); strs_push(x, a); strs_push(x, b); x }\n\
fn run(p: ProcessCap, s: SpawnCap, out: OutCap, script: Str, input: Str) -> Int ! { io, spawn } {\n\
    match process_run(p, s, \"sh\", args(\"-c\", script), bytes_from_str(input)) {\n\
        Result.Ok(value: r) => {\n\
            line(out, int_to_str(r.status));\n\
            line(out, if str_len(r.stdout) > 40 { int_to_str(str_len(r.stdout)) } else { r.stdout });\n\
            line(out, if str_len(r.stderr) > 40 { int_to_str(str_len(r.stderr)) } else { r.stderr });\n\
        },\n\
        Result.Err(error: e) => { line(out, io_kind_name(e.kind)); },\n\
    };\n\
    0\n\
}\n\
fn main(p: ProcessCap, s: SpawnCap, out: OutCap) -> Int {\n\
    run(p, s, out, \"cat; echo oops >&2; exit 3\", \"hello child|\");\n\
    run(p, s, out, \"head -c 300000 /dev/zero; head -c 200000 /dev/zero >&2\", \"\");\n\
    run(p, s, out, \"kill -9 $$\", \"\");\n\
    match process_spawn(p, \"no-such-program-nazm\", strs_new()) {\n\
        Result.Ok(value: c) => { line(out, \"started\"); },\n\
        Result.Err(error: e) => { line(out, io_kind_name(e.kind)); },\n\
    };\n\
    0\n\
}\n";

#[cfg_attr(
    windows,
    ignore = "the child is a POSIX shell script ending itself with `kill -9`: Windows has no signals, and no `sh` of its own, so the 256 + signal status cannot arise"
)]
#[test]
fn a_program_runs_with_input_and_its_output_errors_and_status_come_back_everywhere() {
    // How a pipe splits 300 KB into reads is the system's timing, so the buffers a run allocates
    // vary; what the program says and how it ends are compared.
    for (label, r) in differential::everywhere(&[("prog.nz", RUN)]) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(
            r.stdout.trim_end(),
            "3\nhello child|\noops\n\n0\n300000\n200000\n265\n\n\nNotFound\n0",
            "{label}"
        );
    }
}

#[test]
fn the_environment_and_standard_input_read_alike_everywhere() {
    let src = "use \"@std/ioerror\";\nuse \"@std/io\";\nuse \"@std/env\";\n\
        fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, \"\\n\")) }\n\
        fn main(io: IoCap) -> Int {\n\
            line(io, match env_get(io, \"NAZM_TEST_VALUE\") { Option.Some(value: v) => v, Option.None() => \"unset\" });\n\
            line(io, match env_get(io, \"NAZM_SURELY_UNSET\") { Option.Some(value: v) => v, Option.None() => \"unset\" });\n\
            let vars = env_vars(io);\n\
            let mut found = 0;\n\
            let mut i = 0;\n\
            while i < strs_len(vars) { if strs_get(vars, i) == \"NAZM_TEST_VALUE=a=b c\" { found = found + 1; } i = i + 1; }\n\
            line(io, int_to_str(found));\n\
            match io_stdin_lines(io) {\n\
                Result.Ok(value: lines) => { line(io, str_join(lines, \"|\")); },\n\
                Result.Err(error: e) => { line(io, io_kind_name(e.kind)); },\n\
            };\n\
            0\n\
        }\n";
    for (label, code, out) in
        everywhere_with(src, &[("NAZM_TEST_VALUE", "a=b c")], "one\ntwo\nthree")
    {
        assert_eq!(code, Some(0), "{label}");
        assert_eq!(
            out.trim_end(),
            "a=b c\nunset\n1\none|two|three\n0",
            "{label}"
        );
    }
}

const POOL: &str = "use \"@std/ioerror\";\nuse \"@std/file\";\nuse \"@std/process\";\n\
fn reader(c: OsHandle, done: Chan) -> Int ! { io } {\n\
    let buf = bytes_new(16);\n\
    let n = os_read(os_child_pipe(c, 1), buf);\n\
    chan_send(done, 1);\n\
    0\n\
}\n\
fn busy(done: Chan) -> Int { let mut i = 0; let mut s = 0; while i < 100000 { s = s + i % 3; i = i + 1; } chan_send(done, 2); 0 }\n\
fn main(p: ProcessCap, sp: SpawnCap, out: OutCap) -> Int {\n\
    let a = strs_new();\n\
    strs_push(a, \"-c\");\n\
    strs_push(a, \"sleep 0.3; echo done\");\n\
    let c = os_spawn(\"sh\", a);\n\
    let done = chan_new(4);\n\
    let got = ints_new();\n\
    scope {\n\
        spawn reader(c, done);\n\
        spawn busy(done);\n\
        chan_recv(done, got);\n\
        chan_recv(done, got);\n\
    }\n\
    os_wait(c);\n\
    print(str_concat(int_to_str(ints_get(got, 0)), int_to_str(ints_get(got, 1))));\n\
    0\n\
}\n";

#[test]
fn a_task_reading_a_childs_pipe_does_not_hold_its_worker() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), POOL).unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(crate::common::exe_name(backend));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path()).env("NAZM_WORKERS", "1");
        let r = common::run_bounded(&mut c, backend);
        assert_eq!(r.code, Some(0), "{backend}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), "210", "{backend}");
    }
}

#[test]
fn running_a_program_needs_its_own_authority_and_a_file_does_not_choose_it() {
    let no_cap = "fn main(io: IoCap) -> Int ! { io } { os_status(os_spawn(\"ls\", strs_new())) }\n";
    assert!(
        codes(no_cap).contains(&"N0369".to_owned()),
        "{:?}",
        codes(no_cap)
    );
    let held =
        "fn main(p: ProcessCap) -> Int ! { io } { os_status(os_spawn(\"ls\", strs_new())) }\n";
    assert!(codes(held).is_empty(), "{:?}", codes(held));
    let chosen = "fn main(p: ProcessCap, io: IoCap) -> Int ! { io } { os_status(os_spawn(read_file(\"cfg\"), strs_new())) }\n";
    assert!(
        codes(chosen).contains(&"N0372".to_owned()),
        "{:?}",
        codes(chosen)
    );
    let env = "fn main() -> Int ! { io } { str_len(os_env_get(\"HOME\")) }\n";
    assert!(codes(env).contains(&"N0369".to_owned()), "{:?}", codes(env));
}
