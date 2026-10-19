//! R1d: the platform, further — process control (D4's `os_spawn_in`; P1's `os_spawn_with` with a
//! new process group and a null standard input, `os_pid`, `os_child_pid`, `os_kill_group`,
//! `os_try_wait`, `os_wait_until` and `os_child_rusage`).
//!
//! `docs/user/spec.md`, *The platform, further*. What only a whole run shows: that a child gets exactly
//! the environment given, ordered by name, in the directory given, its program found on this
//! process's `PATH`; that a malformed environment, a flag, a directory or a program that cannot be is
//! refused before anything starts; that a null standard input reads as empty; that a bounded wait
//! answers `WouldBlock` or `TimedOut` and leaves the child running, and a signal to the group the
//! child leads ends it and its own child; that the child's id survives its wait and its peak memory
//! is known once it has ended (natively; the interpreter answers `ENOSYS`). Alike under the
//! interpreter, LLVM `-O0`/`-O2` and Cranelift with nothing live; every child bounded, killed and
//! reaped, and every group checked gone afterwards. And the authority and provenance rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;
use crate::platform_files::{alike, runs_in};

use std::path::Path;

use differential::codes;

/// A directory `sub` with an executable script in it that says where it ran.
fn scripted(d: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir(d.join("sub")).unwrap();
    let script = d.join("sub/run.sh");
    std::fs::write(
        &script,
        // `$0` is the path the program was started by: made absolute beside the directory, so
        // where it was looked for was not the system's choice.
        // One write, so the parent reads it in one piece everywhere.
        "#!/bin/sh\ncase \"$0\" in /*/sub/./run.sh) how=absolutely;; *) how=\"$0\";; esac\n\
         printf 'ran in %s\\nnamed %s\\n' \"$(basename \"$(pwd -P)\")\" \"$how\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// What `os_child_rusage` answers once a child has ended: a positive count natively, `-ENOSYS` in
/// the interpreter, which cannot ask without C.
fn peak(label: &str) -> &'static str {
    if label != "interpreter" {
        "positive"
    } else if cfg!(target_os = "linux") {
        "-38"
    } else {
        "-78"
    }
}

/// Each process group a run reported on its standard error, as `group N`, is gone.
fn groups_gone(runs: &[(String, common::Ran)]) {
    for (label, r) in runs {
        for g in r.stderr.lines().filter_map(|l| l.strip_prefix("group ")) {
            let alive = std::process::Command::new("kill")
                .args(["-0", "--", &format!("-{g}")])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if alive {
                let _ = std::process::Command::new("kill")
                    .args(["-9", "--", &format!("-{g}")])
                    .status();
            }
            assert!(!alive, "{label}: group {g} outlived the program");
        }
    }
}

const SPAWNING: &str = r#"use "@std/ioerror";

fn kind(s: Int) -> Str ! {} { if s >= 0 { int_to_str(s) } else { io_kind_name(io_kind_of(s)) } }

fn list(a: Str, b: Str) -> Strs ! {} {
    let x = strs_new();
    if str_len(a) > 0 { strs_push(x, a); }
    if str_len(b) > 0 { strs_push(x, b); }
    x
}

// Everything the child writes to its output, to its end.
fn output(h: OsHandle) -> Str ! { io } {
    let pipe = os_child_pipe(h, 1);
    let buf = bytes_new(4096);
    let mut all = "";
    let mut n = os_read(pipe, buf);
    while n > 0 {
        all = str_concat(all, bytes_to_str(bytes_slice(buf, 0, n)));
        n = os_read(pipe, buf);
    }
    os_close(pipe);
    os_close(os_child_pipe(h, 2));
    os_close(os_child_pipe(h, 0));
    all
}

fn run(io: IoCap, h: OsHandle) -> Int ! { io } {
    print(output(h));
    print(f"exit {kind(os_wait(h))}\n");
    os_close(h);
    0
}

fn main(p: ProcessCap, io: IoCap) -> Int ! { io } {
    let env = strs_new();
    strs_push(env, "ZED=last");
    strs_push(env, "A=first");
    strs_push(env, "PATH=/usr/bin:/bin");
    strs_push(env, "MID=x=y");
    strs_push(env, "AB=second");
    // Exactly that environment, ordered by name, from a program found on this process's PATH.
    run(io, os_spawn_in("env", strs_new(), "", env));
    // A program named with a `/` is taken relative to the directory.
    run(io, os_spawn_in("./run.sh", strs_new(), "sub", env));
    run(io, os_spawn_in("sh", list("-c", "pwd -P | sed 's|.*/||'"), "sub", env));
    // Refused before anything starts.
    let noeq = kind(os_status(os_spawn_in("env", strs_new(), "", list("NOEQ", ""))));
    let unnamed = kind(os_status(os_spawn_in("env", strs_new(), "", list("=x", ""))));
    let twice = kind(os_status(os_spawn_in("env", strs_new(), "", list("A=1", "A=2"))));
    let flag = kind(os_status(os_spawn_with("env", strs_new(), "", env, 4)));
    print(f"no = {noeq}\nempty name {unnamed}\ntwice {twice}\nflag {flag}\n");
    let nodir = kind(os_status(os_spawn_in("env", strs_new(), "nowhere", env)));
    let afile = kind(os_status(os_spawn_in("env", strs_new(), "sub/run.sh", env)));
    let noprog = kind(os_status(os_spawn_in("no-such-program-nazm", strs_new(), "", env)));
    print(f"no dir {nodir}\ndir a file {afile}\nno program {noprog}\n");
    // A null standard input reads as empty, and its pipe is closed.
    let h = os_spawn_with("sh", list("-c", "cat; echo read nothing"), "", env, 2);
    print(f"input {kind(os_status(os_child_pipe(h, 0)))}\n");
    run(io, h);
    0
}
"#;

#[test]
fn a_child_runs_in_its_directory_with_exactly_its_environment_alike_everywhere() {
    let runs = runs_in(SPAWNING, &scripted);
    alike(&runs, 0, &|_| {
        "A=first\nAB=second\nMID=x=y\nPATH=/usr/bin:/bin\nZED=last\nexit 0\n\
         ran in sub\nnamed absolutely\nexit 0\n\
         sub\nexit 0\n\
         no = InvalidInput\nempty name InvalidInput\ntwice InvalidInput\nflag InvalidInput\n\
         no dir NotFound\ndir a file NotADirectory\nno program NotFound\n\
         input Closed\nread nothing\nexit 0\n0"
            .to_owned()
    });
}

const WAITING: &str = r#"use "@std/ioerror";

fn kind(s: Int) -> Str ! {} { if s >= 0 { int_to_str(s) } else { io_kind_name(io_kind_of(s)) } }

fn list(a: Str, b: Str) -> Strs ! {} {
    let x = strs_new();
    strs_push(x, a);
    strs_push(x, b);
    x
}

// One string either way, so the interpreter's `ENOSYS` allocates what a count does natively.
fn peak(r: Int) -> Str ! {} {
    let text = int_to_str(r);
    if r > 0 { "positive" } else { text }
}

fn main(p: ProcessCap, io: IoCap, t: TimeCap) -> Int ! { io } {
    let env = strs_new();
    strs_push(env, "PATH=/usr/bin:/bin");
    // A child that ends at once: every wait answers its status, the same every time.
    let quick = os_spawn_with("sh", list("-c", "exit 3"), "", env, 2);
    let far = time_now_ms() + 30000;
    let id = os_child_pid(quick);
    print(f"until {kind(os_wait_until(quick, far))} try {kind(os_try_wait(quick))} wait {kind(os_wait(quick))}\n");
    print(f"id kept {os_child_pid(quick) == id} {id > 1} {id != os_pid()} {os_pid() > 1}\n");
    print(f"peak {peak(os_child_rusage(quick))}\n");
    os_close(quick);
    // A child leading its own group, with a child of its own: still running at the deadline.
    let long = os_spawn_with("sh", list("-c", "sleep 30 & sleep 30; wait"), "", env, 3);
    let group = os_child_pid(long);
    eprint(str_concat(str_concat("group ", int_to_str(group)), "\n"));
    print(f"try {kind(os_try_wait(long))} peak before {kind(os_child_rusage(long))}\n");
    let start = time_now_ms();
    print(f"until {kind(os_wait_until(long, start + 100))} waited {time_now_ms() - start >= 100}\n");
    print(f"past deadline {kind(os_wait_until(long, start))}\n");
    print(f"asked {kind(os_kill_group(group, 0))} terminated {kind(os_kill_group(group, 15))}\n");
    print(f"ended {kind(os_wait(long))} again {kind(os_try_wait(long))} {kind(os_wait_until(long, 0))}\n");
    // The group's other process ended with it; wait until the system has let it go.
    let mut gone = os_kill_group(group, 0);
    let mut tries = 0;
    while gone == 0 && tries < 500 {
        time_sleep_ms(10);
        gone = os_kill_group(group, 0);
        tries = tries + 1;
    }
    // Gone: the system says so as `ESRCH`, or — macOS, for a group whose last process is a zombie
    // another parent will reap — `EPERM`.
    let said = gone == 0 - 3 || gone == 0 - 1;
    print(f"group gone {said} after {tries < 500}\n");
    print(f"refused {kind(os_kill_group(1, 0))} {kind(os_kill_group(0, 9))} {kind(os_kill_group(group, 32))} {kind(os_kill_group(group, 0 - 1))} {kind(os_kill_group(2147483648, 0))}\n");
    os_close(long);
    // Not a child.
    print(f"not a child {kind(os_try_wait(os_stdin()))} {kind(os_child_pid(os_stdin()))} {kind(os_child_rusage(os_stdin()))}\n");
    0
}
"#;

#[test]
fn bounded_waits_a_group_s_signal_and_a_child_s_peak_memory_alike_everywhere() {
    let runs = runs_in(WAITING, &|_| {});
    groups_gone(&runs);
    alike(&runs, 0, &|label| {
        format!(
            "until 3 try 3 wait 3\nid kept true true true true\npeak {}\n\
             try WouldBlock peak before InvalidInput\nuntil TimedOut waited true\n\
             past deadline TimedOut\nasked 0 terminated 0\nended 271 again 271 271\n\
             group gone true after true\n\
             refused InvalidInput InvalidInput InvalidInput InvalidInput InvalidInput\n\
             not a child InvalidInput InvalidInput InvalidInput\n0",
            peak(label)
        )
    });
}

const LIBRARY: &str = r#"use "@std/ioerror";
use "@std/file";
use "@std/process";
use "@std/time";

fn status(r: Result[Option[Int], IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: Option.Some(value: s)) => int_to_str(s),
        Result.Ok(value: Option.None()) => "running",
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn main(p: ProcessCap, io: IoCap, t: TimeCap) -> Int ! { io } {
    let args = strs_new();
    strs_push(args, "-c");
    strs_push(args, "echo $GREETING from $(basename $(pwd -P)); exit 4");
    let env = strs_new();
    strs_push(env, "GREETING=hello");
    strs_push(env, "PATH=/usr/bin:/bin");
    match process_spawn_in(p, "sh", args, "sub", env) {
        Result.Ok(value: c) => {
            match file_read_rest(process_stdout(c)) {
                Result.Ok(value: b) => { print(bytes_to_str(b)); },
                Result.Err(error: e) => { print(io_kind_name(e.kind)); },
            };
            let deadline = time_deadline(t, 30000);
            print(f"waited {status(process_wait_until(c, deadline))} {status(process_try_wait(c))}\n");
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    let sleeper = strs_new();
    strs_push(sleeper, "30");
    match process_spawn_with(p, "sleep", sleeper, "", env, true, true) {
        Result.Ok(value: c) => {
            let g = process_child_id(c);
            eprint(str_concat(str_concat("group ", int_to_str(g)), "\n"));
            let soon = time_deadline(t, 50);
            print(f"sleeping {status(process_try_wait(c))} {status(process_wait_until(c, soon))}\n");
            match process_kill_group(p, g, 9) {
                Result.Ok(value: z) => { print(f"killed {z}\n"); },
                Result.Err(error: e) => { print(io_kind_name(e.kind)); },
            };
            match process_wait(c) {
                Result.Ok(value: s) => { print(f"ended {s}\n"); },
                Result.Err(error: e) => { print(io_kind_name(e.kind)); },
            };
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match process_spawn_in(p, "env", strs_new(), "", strs_new()) {
        Result.Ok(value: c) => {
            match file_read_rest(process_stdout(c)) {
                Result.Ok(value: b) => { print(f"empty environment [{bytes_to_str(b)}]\n"); },
                Result.Err(error: e) => { print(io_kind_name(e.kind)); },
            };
            process_wait(c);
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    print(f"id {process_id(io) > 1}\n");
    0
}
"#;

#[test]
fn the_process_library_wraps_them_alike_everywhere() {
    let runs = runs_in(LIBRARY, &scripted);
    groups_gone(&runs);
    alike(&runs, 0, &|label| {
        let _ = label;
        "hello from sub\nwaited 4 4\nsleeping running running\nkilled 0\n\
         ended 265\nempty environment []\nid true\n0"
            .to_owned()
    });
}

const PEAK: &str = r#"use "@std/ioerror";
use "@std/process";

fn main(p: ProcessCap, io: IoCap) -> Int ! { io } {
    let args = strs_new();
    strs_push(args, "-c");
    strs_push(args, "exit 0");
    match process_spawn_with(p, "sh", args, "", strs_new(), false, true) {
        Result.Ok(value: c) => {
            process_wait(c);
            match process_peak_memory(c) {
                Result.Ok(value: m) => { print(f"peak {m > 0}\n"); },
                Result.Err(error: e) => { print(f"peak {io_kind_name(e.kind)}\n"); },
            };
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    0
}
"#;

#[test]
fn a_child_s_peak_memory_is_known_natively_once_it_has_ended() {
    // The interpreter's answer is an error, whose message is a string a count is not: the
    // implementations agree on what each says, and each reclaims all it made.
    for (label, r) in runs_in(PEAK, &|_| {}) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        let want = if label == "interpreter" {
            "peak Other\n0"
        } else {
            "peak true\n0"
        };
        assert_eq!(r.stdout.trim_end(), want, "{label}");
        let line = differential::memory_line(&r);
        assert_eq!(line.matches("live=0").count(), 4, "{label}: {line}");
    }
}

#[test]
fn starting_a_child_anywhere_and_signalling_a_group_need_a_process_cap() {
    for (src, code) in [
        (
            "fn main(io: IoCap) -> Int ! { io } { os_status(os_spawn_in(\"ls\", strs_new(), \"\", strs_new())) }\n",
            "N0369",
        ),
        (
            "fn main(io: IoCap) -> Int ! { io } { os_status(os_spawn_with(\"ls\", strs_new(), \"\", strs_new(), 0)) }\n",
            "N0369",
        ),
        ("fn main(io: IoCap) -> Int ! { io } { os_kill_group(2, 0) }\n", "N0369"),
        ("fn main() -> Int ! { io } { os_pid() }\n", "N0369"),
        (
            "fn main(p: ProcessCap, io: IoCap) -> Int ! { io } { os_status(os_spawn_in(\"ls\", strs_new(), read_file(\"cfg\"), strs_new())) }\n",
            "N0372",
        ),
        (
            "fn main(p: ProcessCap, io: IoCap) -> Int ! { io } { os_status(os_spawn_with(read_file(\"cfg\"), strs_new(), \"\", strs_new(), 0)) }\n",
            "N0372",
        ),
    ] {
        assert!(codes(src).contains(&code.to_owned()), "{src}: {:?}", codes(src));
    }
    // A child already started is its handle's own authority.
    let held = "fn look(h: OsHandle) -> Int ! { io } { os_child_pid(h) + os_try_wait(h) + os_wait_until(h, 0) + os_child_rusage(h) }\n\
                fn main(p: ProcessCap) -> Int ! { io } { look(os_spawn_in(\"ls\", strs_new(), \"\", strs_new())) }\n";
    assert!(codes(held).is_empty(), "{:?}", codes(held));
}
