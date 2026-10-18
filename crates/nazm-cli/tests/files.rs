//! Gate 2: files and handles.
//!
//! `docs/general-purpose.md` §8 and the spec's *Files and handles*. What only a whole run shows:
//! that `@std/file` opens, writes, appends, seeks, reads, lists, renames, removes and replaces
//! atomically, and reports every failure as a value of the kind the platform's number means — the
//! same lines in the interpreter and in both native backends at both optimisation levels, with
//! everything reclaimed; that a closed handle never reaches a descriptor the system has since
//! reused; that standard input reads the same everywhere; and that the authority, equality and
//! provenance rules refuse what they must. Every expected line was worked out from the spec.

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

use differential::{agrees, codes, nazm};

/// Every operation `@std/file` offers, in a directory the program makes and removes, so each of
/// the four runs starts from the same tree.
const SCENARIO: &str = r#"use "@std/ioerror";
use "@std/file";

fn line(io: IoCap, s: Str) -> Int ! { io } { print(str_concat(s, "\n")) }

fn kind(r: Result[Int, IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: n) => str_concat("ok ", int_to_str(n)),
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn opened(r: Result[File, IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: f) => "opened",
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn text(r: Result[Bytes, IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: b) => bytes_to_str(b),
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn main(io: IoCap) -> Int ! { io } {
    line(io, kind(dir_create_all(io, "w/a/b")));
    line(io, kind(dir_create_all(io, "w/a/b")));
    match file_create(io, "w/one.txt") {
        Result.Ok(value: f) => {
            line(io, kind(file_write_str(f, "alpha\nbeta\n")));
            line(io, kind(file_position(f)));
            line(io, kind(file_sync(f)));
            line(io, kind(file_close(f)));
            line(io, kind(file_close(f)));
            line(io, kind(file_write_str(f, "late")));
        },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    match file_append(io, "w/one.txt") {
        Result.Ok(value: f) => { file_write_str(f, "gamma\n"); file_close(f); },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    line(io, text(file_read_all(io, "w/one.txt")));
    match file_open(io, "w/one.txt") {
        Result.Ok(value: f) => {
            line(io, kind(file_seek(f, 6)));
            let buf = bytes_new(4);
            line(io, kind(file_read(f, buf)));
            line(io, bytes_to_str(buf));
            line(io, kind(file_seek_end(f, 0 - 6)));
            line(io, text(file_read_rest(f)));
            line(io, kind(file_write_str(f, "x")));
            file_close(f);
        },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    match file_metadata(io, "w/one.txt") {
        Result.Ok(value: m) => {
            line(io, int_to_str(m.size));
            line(io, if m.kind == FileKind.Regular() { "regular" } else { "not regular" });
            line(io, if m.modified_ns > 0 { "has a time" } else { "no time" });
        },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    match file_metadata(io, "w/a") {
        Result.Ok(value: m) => { line(io, if m.kind == FileKind.Directory() { "directory" } else { "not a directory" }); },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    line(io, kind(file_replace_atomic(io, "w/one.txt", bytes_from_str("replaced"))));
    line(io, text(file_read_all(io, "w/one.txt")));
    line(io, kind(file_replace_atomic(io, "w/fresh.txt", bytes_from_str("new"))));
    file_create(io, "w/c.txt");
    file_create(io, "w/B.txt");
    match dir_list(io, "w") {
        Result.Ok(value: names) => { line(io, str_join(names, ",")); },
        Result.Err(error: e) => { line(io, io_error_show(e)); },
    };
    line(io, kind(file_rename(io, "w/c.txt", "w/a/c.txt")));
    line(io, opened(file_open(io, "w/c.txt")));
    line(io, opened(file_open(io, "w/a/c.txt")));
    line(io, opened(file_create_new(io, "w/a/c.txt")));
    line(io, kind(dir_remove(io, "w/a")));
    line(io, kind(dir_create(io, "w/a")));
    line(io, text(file_read_all(io, "w/a")));
    line(io, kind(file_remove(io, "w/a/c.txt")));
    line(io, kind(dir_remove(io, "w/a/b")));
    line(io, kind(dir_remove(io, "w/a")));
    match dir_list(io, "w/one.txt") {
        Result.Ok(value: names) => { line(io, "listed a file"); },
        Result.Err(error: e) => { line(io, io_kind_name(e.kind)); },
    };
    file_remove(io, "w/one.txt");
    file_remove(io, "w/fresh.txt");
    file_remove(io, "w/B.txt");
    line(io, kind(dir_remove(io, "w")));
    line(io, opened(file_open(io, "w/one.txt")));
    0
}
"#;

#[test]
fn the_library_opens_writes_seeks_lists_renames_and_replaces_alike_everywhere() {
    agrees(
        SCENARIO,
        "ok 0\nok 0\nok 11\nok 11\nok 0\nok 0\nClosed\nClosed\nalpha\nbeta\ngamma\n\nok 6\nok 4\nbeta\nok 11\ngamma\n\nOther\n17\nregular\nhas a time\ndirectory\nok 8\nreplaced\nok 3\nB.txt,a,c.txt,fresh.txt,one.txt\nok 0\nNotFound\nopened\nAlreadyExists\nDirectoryNotEmpty\nAlreadyExists\nIsADirectory\nok 0\nok 0\nok 0\nNotADirectory\nok 0\nNotFound\n0",
    );
}

#[test]
fn a_closed_handle_answers_closed_and_never_reaches_a_reused_descriptor() {
    // `b` is opened after `a` is closed, so the system may give it `a`'s descriptor: a write
    // through the closed `a` must reach nothing, and `b` must hold only its own bytes.
    let src = "use \"@std/ioerror\";\n\
        fn line(io: IoCap, s: Str) -> Int ! { io } { print(str_concat(s, \"\\n\")) }\n\
        fn main(io: IoCap) -> Int ! { io } {\n\
            let a = os_open_write(\"a.txt\", 0);\n\
            line(io, int_to_str(os_close(a)));\n\
            let b = os_open_write(\"b.txt\", 0);\n\
            let s = os_write(a, bytes_from_str(\"stray\"));\n\
            line(io, io_kind_name(io_kind_of(s)));\n\
            line(io, os_error_text(s));\n\
            line(io, int_to_str(os_status(a)));\n\
            line(io, int_to_str(os_write(b, bytes_from_str(\"own\"))));\n\
            os_close(b);\n\
            line(io, read_file(\"b.txt\"));\n\
            line(io, int_to_str(os_close(a)));\n\
            let missing = os_open_read(\"missing.txt\");\n\
            line(io, io_kind_name(io_kind_of(os_status(missing))));\n\
            line(io, if str_len(os_error_text(os_status(missing))) > 0 { \"a message\" } else { \"no message\" });\n\
            line(io, io_kind_name(io_kind_of(os_status(os_open_write(\"x\", 7)))));\n\
            line(io, io_kind_name(io_kind_of(os_seek(b, 0, 9))));\n\
            line(io, int_to_str(str_len(os_error_text(0))));\n\
            os_unlink(\"a.txt\");\n\
            os_unlink(\"b.txt\");\n\
            0\n\
        }\n";
    agrees(
        src,
        "0\nClosed\nthe handle is closed\n-65536\n3\nown\n-65536\nNotFound\na message\nInvalidInput\nClosed\n0\n0",
    );
}

#[test]
fn a_function_handed_a_file_uses_it_with_no_other_authority() {
    // Opening names a path and needs an `IoCap`; reading what was opened needs only the file.
    let src = "use \"@std/ioerror\";\nuse \"@std/file\";\n\
        fn first(f: File) -> Int ! { io } {\n\
            let b = bytes_new(3);\n\
            match file_read(f, b) { Result.Ok(value: n) => n, Result.Err(error: e) => 0 - 1, }\n\
        }\n\
        fn main(io: IoCap) -> Int ! { io } {\n\
            match file_open(io, \"prog.nz\") { Result.Ok(value: f) => first(f), Result.Err(error: e) => 0 - 1, }\n\
        }\n";
    agrees(src, "3");
    // And it can open nothing.
    let opens = "use \"@std/ioerror\";\nuse \"@std/file\";\n\
        fn sneak(f: File) -> Int ! { io } { os_status(os_open_read(\"other\")) }\n\
        fn main() -> Int { 0 }\n";
    assert!(
        codes(opens).contains(&"N0369".to_owned()),
        "{:?}",
        codes(opens)
    );
}

#[test]
fn the_platform_family_is_the_hosts() {
    let src = "fn main(io: IoCap) -> Int ! { io } { print(int_to_str(os_family())); 0 }\n";
    let family = if cfg!(target_os = "linux") { "1" } else { "0" };
    agrees(src, &format!("{family}0"));
}

#[test]
fn a_buffer_too_short_for_a_stat_stops_the_program_everywhere() {
    let src = "fn main(io: IoCap) -> Int ! { io } { os_stat(\"prog.nz\", bytes_new(31)) }\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(r.code, Some(2), "{}", r.stderr);
    assert!(r.stderr.contains("N0405"), "{}", r.stderr);
    assert!(
        r.stderr.contains("this buffer is too short for the answer"),
        "{}",
        r.stderr
    );
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
        let o = common::bounded_exe(&exe)
            .current_dir(d.path())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{backend}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            err.contains("this buffer is too short for the answer"),
            "{backend}: {err}"
        );
    }
}

#[test]
fn standard_input_is_read_alike_everywhere() {
    let src = "use \"@std/ioerror\";\n\
        fn main(io: IoCap) -> Int ! { io } {\n\
            let h = os_stdin();\n\
            let buf = bytes_new(5);\n\
            let mut total = 0;\n\
            let mut n = os_read(h, buf);\n\
            while n > 0 {\n\
                print(bytes_to_str(bytes_slice(buf, 0, n)));\n\
                total = total + n;\n\
                n = os_read(h, buf);\n\
            }\n\
            print(str_concat(\"|\", int_to_str(total)));\n\
            print(str_concat(\"|\", io_kind_name(io_kind_of(os_write(h, buf)))));\n\
            print(str_concat(\"|\", io_kind_name(io_kind_of(os_seek(h, 0, 0)))));\n\
            print(str_concat(\"|\", int_to_str(os_status(os_stdin()))));\n\
            0\n\
        }\n";
    let input = "first line\nsecond\n";
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
        runs.push((backend.to_owned(), common::bounded_exe(&exe)));
    }
    for (label, mut c) in runs {
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
        assert_eq!(
            o.status.code(),
            Some(0),
            "{label}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim_end(),
            "first line\nsecond\n|18|Other|Other|00",
            "{label}"
        );
    }
}

#[test]
fn a_handle_needs_authority_has_no_equality_and_a_file_does_not_choose_where_to_write() {
    // Every `os_` built-in that reaches the system needs an `IoCap` held.
    assert!(codes(
        "fn f() -> Int ! { io } { os_status(os_open_read(\"a\")) }\nfn main() -> Int { 0 }\n"
    )
    .contains(&"N0369".to_owned()));
    // A handle is not a value with an identity a program may compare.
    assert!(codes(
        "fn main(io: IoCap) -> Int ! { io } { if os_stdin() == os_stdin() { 1 } else { 0 } }\n"
    )
    .contains(&"N0304".to_owned()));
    // What a file says may not name the file a program writes, renames or removes (N38's sink).
    for call in [
        "os_status(os_open_write(p, 0))",
        "os_mkdir(p)",
        "os_unlink(p)",
        "os_rmdir(p)",
        "os_rename(\"a\", p)",
    ] {
        let src = format!(
            "fn main(io: IoCap) -> Int ! {{ io }} {{ let p = read_file(\"cfg\"); {call} }}\n"
        );
        assert!(
            codes(&src).contains(&"N0372".to_owned()),
            "{call}: {:?}",
            codes(&src)
        );
    }
    // A path to read may come from anywhere.
    assert!(codes(
        "fn main(io: IoCap) -> Int ! { io } { os_status(os_open_read(read_file(\"cfg\"))) }\n"
    )
    .is_empty());
}
