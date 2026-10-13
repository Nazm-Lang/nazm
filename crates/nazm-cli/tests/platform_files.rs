//! R1d: the platform, further — standard output and errors as handles, and the file system (D4's
//! `os_stdout`/`os_stderr` and `os_lock`/`os_unlock`; P2's `os_lstat`, `os_chmod`, `os_realpath`,
//! `os_mkdtemp` and `os_try_lock`).
//!
//! `docs/spec.md`, *The platform, further*. What only a whole run shows: that raw bytes on standard
//! output keep their place among `print`'s and that closing the handle leaves the stream open; what a
//! path itself is — a link not followed, a hard link the same file by device and inode; permissions
//! set through a link; a canonical path; a temporary directory made private and writable under; and
//! advisory locks held by the open file — each in a fresh directory, alike under the interpreter,
//! LLVM `-O0`/`-O2` and Cranelift, with the same memory report and nothing live. And the authority
//! and provenance rules, a prelude name a program may still define, WASI's build and its refusal of
//! a process by name, and the Windows units built.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use std::path::Path;
use std::process::Command;

use common::{run_bounded, Ran};
use differential::{codes, memory_line};

/// `src` built by each backend at each optimisation level, then run — and run by the interpreter —
/// each in a fresh directory `setup` prepares, with the memory report on: each label and its run.
pub(crate) fn runs_in(src: &str, setup: &dyn Fn(&Path)) -> Vec<(String, Ran)> {
    let build = tempfile::tempdir().unwrap();
    let prog = build.path().join("prog.nz");
    std::fs::write(&prog, src).unwrap();
    let mut exes = Vec::new();
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = build.path().join(label.replace([' ', '-'], ""));
        let b = differential::nazm(
            build.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        exes.push((label, exe));
    }
    let mut out = Vec::new();
    {
        let d = tempfile::tempdir().unwrap();
        setup(d.path());
        let mut c = Command::new(differential::NAZM);
        c.current_dir(d.path())
            .arg("run")
            .arg(&prog)
            .env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push(("interpreter".to_owned(), run_bounded(&mut c, "nazm run")));
    }
    for (label, exe) in exes {
        let d = tempfile::tempdir().unwrap();
        setup(d.path());
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path()).env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every run ended with `code`, printed what `expected` says for its label, and wrote the
/// interpreter's memory report with nothing live.
pub(crate) fn alike(runs: &[(String, Ran)], code: i32, expected: &dyn Fn(&str) -> String) {
    let first = memory_line(&runs[0].1);
    for (label, r) in runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected(label), "{label}");
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
}

fn nothing(_: &Path) {}

const STREAMS: &str = r#"fn main(io: IoCap) -> Int ! { io } {
    let o = os_stdout();
    print("a\n");
    os_write(o, bytes_from_str("b\n"));
    print("c\n");
    let e = os_stderr();
    os_write(e, bytes_from_str("to errors\n"));
    let buf = bytes_new(4);
    print(f"read {os_read(o, buf)} seek {os_seek(o, 0, 0)} sync {os_sync(e)} list {os_dir_next(e, buf)}\n");
    print(f"lock {os_lock(o, true)} try {os_try_lock(e, false)} unlock {os_unlock(o)}\n");
    print(f"close {os_close(os_stdout())} again {os_status(o)} write {os_write(os_stdout(), buf)}\n");
    print("still printed\n");
    0
}
"#;

#[test]
fn standard_output_is_a_handle_in_order_with_print_and_closing_it_keeps_the_stream() {
    let runs = runs_in(STREAMS, &nothing);
    alike(&runs, 0, &|_| {
        "a\nb\nc\nread -9 seek -29 sync -22 list -20\nlock -22 try -22 unlock -22\n\
         close 0 again -65536 write -65536\nstill printed\n0"
            .to_owned()
    });
    for (label, r) in &runs {
        assert!(r.stderr.contains("to errors\n"), "{label}: {}", r.stderr);
    }
}

/// A file, a directory, a link to the file, a dangling link, a hard link to the file and a
/// directory with a file in it, with known permissions.
fn tree(d: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(d.join("file.txt"), "hello").unwrap();
    std::fs::set_permissions(d.join("file.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::create_dir(d.join("dir")).unwrap();
    std::fs::set_permissions(d.join("dir"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("file.txt", d.join("link")).unwrap();
    std::os::unix::fs::symlink("nowhere", d.join("dangling")).unwrap();
    std::fs::hard_link(d.join("file.txt"), d.join("hard")).unwrap();
}

const PATHS: &str = r#"use "@std/ioerror";

fn word(out: Bytes, i: Int) -> Int ! {} { bytes_read_le[Int](out, i * 8) }

fn kind(s: Int) -> Str ! {} { if s >= 0 { int_to_str(s) } else { io_kind_name(io_kind_of(s)) } }

fn show(io: IoCap, path: Str) -> Int ! { io } { shown(io, path, path) }

fn shown(io: IoCap, path: Str, name: Str) -> Int ! { io } {
    let out = bytes_new(48);
    let s = os_lstat(path, out);
    if s < 0 {
        print(f"{name}: {kind(s)}\n");
        return 0;
    }
    let k = word(out, 1);
    // A directory's size, and a link's permissions, are the file system's own.
    if k == 1 { print(f"{name}: dir perms {word(out, 3)}\n"); }
    if k == 0 { print(f"{name}: file size {word(out, 0)} perms {word(out, 3)} dated {word(out, 2) > 0}\n"); }
    if k == 3 { print(f"{name}: link size {word(out, 0)}\n"); }
    0
}

fn same(io: IoCap, a: Str, b: Str) -> Bool ! { io } {
    let x = bytes_new(48);
    let y = bytes_new(48);
    os_lstat(a, x);
    os_lstat(b, y);
    word(x, 4) == word(y, 4) && word(x, 5) == word(y, 5)
}

fn letters(s: Str, from: Int) -> Bool ! {} {
    let mut ok = true;
    for i in from..str_len(s) {
        let c = str_byte(s, i);
        if !((c >= 48 && c <= 57) || (c >= 97 && c <= 122)) { ok = false; }
    }
    ok
}

fn main(io: IoCap) -> Int ! { io } {
    show(io, "file.txt");
    show(io, "dir");
    show(io, "link");
    show(io, "dangling");
    show(io, "missing");
    let hard = same(io, "file.txt", "hard");
    let other = same(io, "file.txt", "dir");
    let itself = same(io, "file.txt", "link");
    print(f"same {hard} {other} {itself}\n");
    let c1 = os_chmod("file.txt", 384);
    let c2 = kind(os_chmod("file.txt", 4096));
    let c3 = kind(os_chmod("missing", 420));
    print(f"chmod {c1} {c2} {c3}\n");
    show(io, "file.txt");
    let through = os_chmod("link", 416);
    print(f"through the link {through}\n");
    show(io, "file.txt");
    show(io, "link");
    let here = bytes_new(4096);
    let n = os_realpath(".", here);
    let base = bytes_to_str(bytes_slice(here, 0, n));
    let full = bytes_new(4096);
    let m = os_realpath("dir/../link", full);
    let wanted = str_concat(base, "/file.txt");
    let resolved = bytes_to_str(bytes_slice(full, 0, m));
    print(f"real {str_byte(base, 0) == 47} {resolved == wanted}\n");
    let short = kind(os_realpath(".", bytes_new(1)));
    let gone = kind(os_realpath("missing", full));
    let dangling = kind(os_realpath("dangling", full));
    print(f"real short {short} missing {gone} dangling {dangling}\n");
    let t = os_mkdtemp(".");
    let u = os_mkdtemp("./");
    print(f"temp {str_len(t)} {str_slice(t, 0, 7)} {str_len(u)} {str_slice(u, 0, 7)} {t == u} {letters(t, 7)}\n");
    shown(io, t, "temp");
    let n1 = os_mkdtemp("");
    let n2 = os_mkdtemp("missing");
    let n3 = os_mkdtemp("file.txt");
    print(f"temp none [{n1}] [{n2}] [{n3}]\n");
    let inside = f"{t}/x";
    let h = os_open_write(inside, 2);
    let wrote = os_write(h, bytes_from_str("x"));
    print(f"written under it {wrote} {os_close(h)} {os_unlink(inside)}\n");
    print(f"removed {os_rmdir(t)} {os_rmdir(u)}\n");
    0
}
"#;

#[test]
fn a_path_itself_its_permissions_its_canonical_form_and_a_private_directory_alike_everywhere() {
    let runs = runs_in(PATHS, &tree);
    alike(&runs, 0, &|_| {
        "file.txt: file size 5 perms 420 dated true\n\
         dir: dir perms 493\n\
         link: link size 8\n\
         dangling: link size 7\n\
         missing: NotFound\n\
         same true false false\n\
         chmod 0 InvalidInput NotFound\n\
         file.txt: file size 5 perms 384 dated true\n\
         through the link 0\n\
         file.txt: file size 5 perms 416 dated true\n\
         link: link size 8\n\
         real true true\n\
         real short InvalidInput missing NotFound dangling NotFound\n\
         temp 19 ./nazm- 19 ./nazm- false true\n\
         temp: dir perms 448\n\
         temp none [] [] []\n\
         written under it 1 0 0\n\
         removed 0 0\n\
         0"
        .to_owned()
    });
}

#[test]
fn a_buffer_too_short_for_an_lstat_stops_the_program_everywhere() {
    let runs = runs_in(
        "fn main(io: IoCap) -> Int ! { io } { os_lstat(\"x\", bytes_new(47)) }\n",
        &nothing,
    );
    for (label, r) in &runs {
        assert_eq!(r.code, Some(2), "{label}: {}", r.stderr);
        assert!(
            r.stderr.contains("this buffer is too short for the answer"),
            "{label}: {}",
            r.stderr
        );
    }
    assert!(runs[0].1.stderr.contains("N0405"), "{}", runs[0].1.stderr);
}

fn lockable(d: &Path) {
    std::fs::write(d.join("lockme"), "data").unwrap();
}

const LOCKS: &str = r#"use "@std/ioerror";
use "@std/file";

fn kind(s: Int) -> Str ! {} { if s >= 0 { int_to_str(s) } else { io_kind_name(io_kind_of(s)) } }

fn held(r: Result[Bool, IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: b) => if b { "taken" } else { "held elsewhere" },
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn main(io: IoCap) -> Int ! { io } {
    let a = os_open_read("lockme");
    let b = os_open_read("lockme");
    let c = os_open_write("lockme", 3);
    print(f"a exclusive {kind(os_lock(a, true))}; b {kind(os_try_lock(b, true))} {kind(os_try_lock(b, false))}\n");
    print(f"a shared {kind(os_lock(a, false))}; b shared {kind(os_try_lock(b, false))}; c {kind(os_try_lock(c, true))}\n");
    print(f"unlocked {kind(os_unlock(a))} {kind(os_unlock(b))}; c {kind(os_try_lock(c, true))}; a {kind(os_try_lock(a, false))}\n");
    print(f"closing c lets go {kind(os_close(c))}; a {kind(os_try_lock(a, false))}\n");
    print(f"unlock unheld {kind(os_unlock(b))}\n");
    let d = os_dir_open(".");
    print(f"directory {kind(os_lock(d, true))} stdin {kind(os_lock(os_stdin(), true))}\n");
    os_close(b);
    print(f"closed {kind(os_lock(b, true))} {kind(os_unlock(b))}\n");
    os_unlock(a);
    match file_open(io, "lockme") {
        Result.Ok(value: f) => match file_open(io, "lockme") {
            Result.Ok(value: g) => {
                print(f"library {held(file_try_lock(f, true))} {held(file_try_lock(g, false))}\n");
                file_unlock(f);
                print(f"library after {held(file_try_lock(g, false))}\n");
            },
            Result.Err(error: e) => { print("open failed\n"); },
        },
        Result.Err(error: e) => { print("open failed\n"); },
    };
    0
}
"#;

#[test]
fn an_advisory_lock_belongs_to_the_open_file_alike_everywhere() {
    let runs = runs_in(LOCKS, &lockable);
    alike(&runs, 0, &|_| {
        "a exclusive 0; b WouldBlock WouldBlock\n\
         a shared 0; b shared 0; c WouldBlock\n\
         unlocked 0 0; c 0; a WouldBlock\n\
         closing c lets go 0; a 0\n\
         unlock unheld 0\n\
         directory IsADirectory stdin InvalidInput\n\
         closed Closed Closed\n\
         library taken held elsewhere\n\
         library after taken\n\
         0"
        .to_owned()
    });
}

const LIBRARY: &str = r#"use "@std/ioerror";
use "@std/file";
use "@std/fs";

fn named(k: EntryKind) -> Str ! {} {
    match k {
        EntryKind.Regular() => "regular",
        EntryKind.Directory() => "directory",
        EntryKind.Symlink() => "symlink",
        EntryKind.Other() => "other",
    }
}

fn main(io: IoCap) -> Int ! { io } {
    file_write_str(file_stdout(io), "raw\n");
    file_write_str(file_stderr(io), "raw errors\n");
    match file_symlink_metadata(io, "link") {
        Result.Ok(value: m) => { print(f"link {named(m.kind)} {m.size}\n"); },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match file_symlink_metadata(io, "file.txt") {
        Result.Ok(value: m) => match file_symlink_metadata(io, "hard") {
            Result.Ok(value: h) => { print(f"file {named(m.kind)} same {m.device == h.device && m.inode == h.inode}\n"); },
            Result.Err(error: e) => { print(io_kind_name(e.kind)); },
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match file_set_permissions(io, "file.txt", 256) {
        Result.Ok(value: z) => match file_metadata(io, "file.txt") {
            Result.Ok(value: m) => { print(f"permissions {m.permissions}\n"); },
            Result.Err(error: e) => { print(io_kind_name(e.kind)); },
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match file_canonical(io, "dir/../link") {
        Result.Ok(value: p) => { print(f"canonical ends {str_slice(p, str_len(p) - 9, str_len(p))}\n"); },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match file_canonical(io, "missing") {
        Result.Ok(value: p) => { print("found\n"); },
        Result.Err(error: e) => { print(f"canonical missing {io_kind_name(e.kind)}\n"); },
    };
    match dir_create_temp(io, ".") {
        Result.Ok(value: t) => {
            match file_create(io, f"{t}/inside") {
                Result.Ok(value: f) => { file_close(f); },
                Result.Err(error: e) => { print(io_kind_name(e.kind)); },
            };
            print(f"temp {str_len(t)} {file_exists_at(io, f"{t}/inside")}\n");
            file_remove(io, f"{t}/inside");
            dir_remove(io, t);
        },
        Result.Err(error: e) => { print(io_kind_name(e.kind)); },
    };
    match dir_create_temp(io, "missing") {
        Result.Ok(value: t) => { print("made\n"); },
        Result.Err(error: e) => { print(f"temp missing {io_kind_name(e.kind)}\n"); },
    };
    match fs_temp_dir(io) {
        Result.Ok(value: t) => { print(f"fs temp {str_len(t) > 17}\n"); os_rmdir(t); },
        Result.Err(error: e) => { print(e); },
    };
    0
}
"#;

#[test]
fn the_library_wraps_them_alike_everywhere() {
    let runs = runs_in(LIBRARY, &tree);
    alike(&runs, 0, &|_| {
        "raw\nlink symlink 8\nfile regular same true\npermissions 256\ncanonical ends /file.txt\n\
         canonical missing NotFound\ntemp 19 true\ntemp missing Other\nfs temp true\n0"
            .to_owned()
    });
    for (label, r) in &runs {
        assert!(r.stderr.contains("raw errors\n"), "{label}: {}", r.stderr);
    }
}

#[test]
fn naming_a_path_needs_an_io_cap_and_a_file_does_not_choose_one() {
    for (src, code) in [
        (
            "fn main() -> Int ! { io } { os_lstat(\"x\", bytes_new(48)) }\n",
            "N0369",
        ),
        (
            "fn main() -> Int ! { io } { os_chmod(\"x\", 420) }\n",
            "N0369",
        ),
        (
            "fn main() -> Int ! { io } { os_realpath(\"x\", bytes_new(8)) }\n",
            "N0369",
        ),
        (
            "fn main() -> Int ! { io } { str_len(os_mkdtemp(\".\")) }\n",
            "N0369",
        ),
        (
            "fn main() -> Int ! { io } { os_status(os_stdout()) }\n",
            "N0369",
        ),
        (
            "fn main() -> Int ! { io } { os_status(os_stderr()) }\n",
            "N0369",
        ),
        (
            "fn main(io: IoCap) -> Int ! { io } { os_chmod(read_file(\"cfg\"), 420) }\n",
            "N0372",
        ),
        (
            "fn main(io: IoCap) -> Int ! { io } { str_len(os_mkdtemp(read_file(\"cfg\"))) }\n",
            "N0372",
        ),
    ] {
        assert!(
            codes(src).contains(&code.to_owned()),
            "{src}: {:?}",
            codes(src)
        );
    }
    // A handle already open is its own authority: locking one needs nothing held.
    let held = "fn lock(h: OsHandle) -> Int ! { io } { os_lock(h, true) + os_try_lock(h, false) + os_unlock(h) }\n\
                fn main(io: IoCap) -> Int ! { io } { lock(os_open_read(\"x\")) }\n";
    assert!(codes(held).is_empty(), "{:?}", codes(held));
    // A temporary directory's path is as clean as its parent: a program writes under it.
    let under = "fn main(io: IoCap) -> Int ! { io } { let t = os_mkdtemp(\".\"); \
                 write_file(str_concat(t, \"/x\"), \"y\"); os_unlink(str_concat(t, \"/x\")); os_rmdir(t) }\n";
    assert!(codes(under).is_empty(), "{:?}", codes(under));
}

#[test]
fn a_program_that_names_a_function_like_one_keeps_its_own() {
    let runs = runs_in(
        "fn os_lstat(a: Int) -> Int ! {} { a + 1 }\nfn os_mkdtemp() -> Int ! {} { 5 }\n\
         fn main() -> Int ! {} { os_lstat(1) + os_mkdtemp() }\n",
        &nothing,
    );
    alike(&runs, 0, &|_| "7".to_owned());
}

#[test]
fn wasi_builds_the_file_system_and_refuses_a_process_by_name() {
    // WASI has the streams' handles, a path's own facts and a temporary directory; what it keeps
    // no notion of — permissions, canonical paths, locks — answers ENOSYS there, as listing a
    // directory does; a process it has not got at all, and a program reaching one is refused.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("files.nz"),
        "use \"@std/file\";\n\
         fn main(io: IoCap) -> Int ! { io } {\n\
         let out = bytes_new(48);\n\
         let a = os_write(os_stdout(), bytes_from_str(\"x\")) + os_lstat(\"x\", out) + str_len(os_mkdtemp(\".\"));\n\
         let b = os_chmod(\"x\", 420) + os_realpath(\"x\", out) + os_lock(os_open_read(\"x\"), true);\n\
         a + b\n\
         }\n",
    )
    .unwrap();
    let o = differential::nazm(
        d.path(),
        &[
            "build",
            "--target",
            "wasm32-wasip1",
            "--objects",
            "files",
            "files.nz",
        ],
    );
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    for (src, symbol) in [
        (
            "fn main(io: IoCap) -> Int ! { io } { os_pid() }\n",
            "networking (`nz.os_pid`)",
        ),
        (
            "fn main(p: ProcessCap) -> Int ! { io } { os_status(os_spawn_in(\"x\", strs_new(), \"\", strs_new())) }\n",
            "networking (`nz.os_spawn_in`)",
        ),
    ] {
        std::fs::write(d.path().join("prog.nz"), src).unwrap();
        let o = differential::nazm(
            d.path(),
            &[
                "build",
                "--target",
                "wasm32-wasip1",
                "--objects",
                "out",
                "prog.nz",
            ],
        );
        assert_eq!(o.code, Some(1), "{src}: {}", o.stderr);
        assert!(o.stderr.contains("is WASI preview 1"), "{}", o.stderr);
        assert!(o.stderr.contains(symbol), "{symbol}: {}", o.stderr);
        assert!(!d.path().join("out").exists(), "objects written for {src}");
    }
}

#[test]
fn the_windows_units_are_built_for_every_one() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main(io: IoCap, p: ProcessCap) -> Int ! { io } {\n\
         let out = bytes_new(48);\n\
         let h = os_spawn_with(\"x\", strs_new(), \"d\", strs_new(), 3);\n\
         let a = os_lstat(\"x\", out) + os_chmod(\"x\", 420) + os_realpath(\"x\", out) + str_len(os_mkdtemp(\".\"));\n\
         let b = os_lock(os_stdout(), true) + os_try_lock(os_stderr(), false) + os_unlock(h) + os_pid();\n\
         let c = os_child_pid(h) + os_try_wait(h) + os_wait_until(h, 0) + os_child_rusage(h);\n\
         let e = os_kill_group(2, 0) + os_status(os_spawn_in(\"x\", strs_new(), \"\", strs_new()));\n\
         a + b + c + e\n\
         }\n",
    )
    .unwrap();
    for backend in ["llvm", "cranelift"] {
        let o = differential::nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "--target",
                "x86_64-pc-windows-gnu",
                "--objects",
                backend,
                "prog.nz",
            ],
        );
        assert_eq!(o.code, Some(0), "{backend}: {}", o.stderr);
        assert!(
            d.path().join(backend).join("link.txt").exists(),
            "{backend}"
        );
    }
}
