//! R1d, P3: streaming standard input (`docs/user/spec.md`, *Files and handles*, **Lines of standard
//! input**; `@std/io`).
//!
//! What a whole run shows: a header's lines, the frame they announce, a line longer than the
//! buffer, a last line without a newline and the end read the same bytes, in the same pieces, in
//! the interpreter and in both native backends at both optimisation levels — standard input fed
//! all at once and a byte at a time — with the same memory report and nothing live; a line read of
//! any other handle is a value, never a failure; and a line read has `os_read`'s authority and
//! provenance. The LIR oracle runs no `os_` built-in (it refuses files by name), so it has no
//! answer here.

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

use differential::{agrees, codes, memory_line, nazm};

/// An LSP-shaped exchange and then MCP-shaped lines: two header lines ending `\r\n`, a frame of
/// five bytes read exactly, a line read three bytes at a time, two whole lines, a last line with
/// no newline, and the end — twice, since the end stays the end.
const FRAMES: &str = r#"use "@std/ioerror";
use "@std/io";

fn show(r: Result[Option[Str], IoError]) -> Str ! {} {
    match r {
        Result.Ok(value: o) => match o {
            Option.Some(value: s) => str_concat("line ", marked(s)),
            Option.None() => "end",
        },
        Result.Err(error: e) => io_kind_name(e.kind),
    }
}

fn marked(s: Str) -> Str ! {} {
    let n = str_len(s);
    if n > 0 {
        if str_byte(s, n - 1) == 13 {
            return str_concat(str_slice(s, 0, n - 1), "<CR>");
        }
    }
    s
}

fn main(io: IoCap) -> Int ! { io } {
    print(f"{show(io_stdin_read_line(io))}\n");
    print(f"{show(io_stdin_read_line(io))}\n");
    let body = bytes_new(5);
    match io_stdin_read_exact(io, body) {
        Result.Ok(value: n) => { print(f"frame {n} {bytes_to_str(body)}\n"); },
        Result.Err(error: e) => { print(f"frame {io_kind_name(e.kind)}\n"); },
    };
    let small = bytes_new(3);
    let h = os_stdin();
    let mut n = os_read_line(h, small);
    while n == 3 {
        print(f"piece {bytes_to_str(small)}\n");
        if bytes_get(small, 2) == 10 {
            n = 0 - 1;
        } else {
            n = os_read_line(h, small);
        }
    }
    if n >= 0 {
        print(f"piece {bytes_to_str(bytes_slice(small, 0, n))}|\n");
    }
    let one = bytes_new(1);
    match io_stdin_read(io, one) {
        Result.Ok(value: k) => { print(f"byte {k} {bytes_to_str(bytes_slice(one, 0, k))}\n"); },
        Result.Err(error: e) => { print(f"byte {io_kind_name(e.kind)}\n"); },
    };
    print(f"{show(io_stdin_read_line(io))}\n");
    print(f"{show(io_stdin_read_line(io))}\n");
    print(f"{show(io_stdin_read_line(io))}\n");
    print(f"{show(io_stdin_read_line(io))}\n");
    let rest = bytes_new(4);
    match io_stdin_read_exact(io, rest) {
        Result.Ok(value: k) => { print(f"rest {k}\n"); },
        Result.Err(error: e) => { print(f"rest {io_kind_name(e.kind)}\n"); },
    };
    0
}
"#;

const INPUT: &[u8] = b"Content-Length: 5\r\n\r\nhelloabcdefgh\nxsecond line\nthird\nlast";

const EXPECTED: &str = "line Content-Length: 5<CR>\nline <CR>\nframe 5 hello\npiece abc\n\
piece def\npiece gh\n\nbyte 1 x\nline second line\nline third\nline last\nend\nrest \
UnexpectedEof\n0";

/// `cmd` run with `INPUT` on its standard input, written whole or a byte at a time.
fn fed(mut cmd: Command, trickle: bool) -> (Option<i32>, String, String) {
    let mut child = cmd
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        if trickle {
            for b in INPUT {
                // A broken pipe means the program stopped reading, which the asserts report.
                if stdin.write_all(&[*b]).is_err() {
                    break;
                }
                let _ = stdin.flush();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        } else {
            let _ = stdin.write_all(INPUT);
        }
    });
    let o = child.wait_with_output().unwrap();
    writer.join().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn headers_frames_and_lines_read_alike_everywhere() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), FRAMES).unwrap();
    let mut runs: Vec<(String, Command)> = Vec::new();
    for trickle in [false, true] {
        let mut interp = Command::new(differential::NAZM);
        interp
            .current_dir(d.path())
            .args(["run", "--interpret", "prog.nz"]);
        runs.push((format!("interpreter trickle={trickle}"), interp));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = nazm(
            d.path(),
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
        for trickle in [false, true] {
            runs.push((
                format!("{label} trickle={trickle}"),
                common::bounded_exe(&exe),
            ));
        }
    }
    let mut first: Option<String> = None;
    for (label, mut c) in runs {
        c.current_dir(d.path());
        let trickle = label.ends_with("true");
        let (code, stdout, stderr) = fed(c, trickle);
        assert_eq!(code, Some(0), "{label}: {stderr}");
        assert_eq!(stdout.trim_end(), EXPECTED, "{label}");
        let line = memory_line(&common::Ran {
            code,
            stdout,
            stderr,
        });
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
        match &first {
            None => first = Some(line),
            Some(f) => assert_eq!(
                &line, f,
                "{label} and the interpreter disagree about storage"
            ),
        }
    }
}

#[test]
fn a_line_read_of_another_handle_is_a_value() {
    // A file, a directory and a closed handle: `InvalidInput`, `IsADirectory`, `Closed` — the
    // same in every implementation, and nothing is read.
    agrees(
        r#"use "@std/ioerror";

fn main(io: IoCap) -> Int ! { io } {
    write_file("lines.txt", "one\ntwo\n");
    let buf = bytes_new(8);
    let f = os_open_read("lines.txt");
    print(f"{io_kind_name(io_kind_of(os_read_line(f, buf)))}\n");
    print(f"{os_read(f, buf)}\n");
    os_close(f);
    print(f"{io_kind_name(io_kind_of(os_read_line(f, buf)))}\n");
    let d = os_dir_open(".");
    print(f"{io_kind_name(io_kind_of(os_read_line(d, buf)))}\n");
    os_close(d);
    os_unlink("lines.txt");
    0
}
"#,
        "InvalidInput\n8\nClosed\nIsADirectory\n0",
    );
}

#[test]
fn a_line_read_has_os_reads_authority_and_provenance() {
    // The handle is its own authority: a function handed standard input reads its lines with no
    // capability of its own.
    assert!(!codes(
        "fn lines(h: OsHandle, buf: Bytes) -> Int ! { io } { os_read_line(h, buf) }\n\
         fn main(io: IoCap) -> Int ! { io } { lines(os_stdin(), bytes_new(8)) }\n"
    )
    .iter()
    .any(|c| c.starts_with("N03")));
    // It performs `io`: a function that declares none cannot read a line.
    assert!(codes(
        "fn lines(h: OsHandle, buf: Bytes) -> Int ! {} { os_read_line(h, buf) }\n\
         fn main(io: IoCap) -> Int ! { io } { lines(os_stdin(), bytes_new(8)) }\n"
    )
    .contains(&"N0366".to_owned()));
    // What it reads is the outside world's: it may not name the file a program writes.
    assert!(codes(
        "fn main(io: IoCap) -> Int ! { io } {\n\
             let buf = bytes_new(64);\n\
             let n = os_read_line(os_stdin(), buf);\n\
             write_file(bytes_to_str(bytes_slice(buf, 0, n)), \"x\");\n\
             0\n\
         }\n"
    )
    .contains(&"N0372".to_owned()));
}
