//! Gate 3: Windows x86_64 (`docs/spec.md`, *Windows x86_64*; `docs/systems-domains.md` Part A-W).
//!
//! What a whole build shows: every program in `tests/windows/` builds for `x86_64-pc-windows-gnu`
//! to objects whose `link.txt` names MinGW-w64's libraries and Winsock's. That the executables
//! *run* — files, sockets and the reactor, processes and pipes, the clocks, the arguments as
//! UTF-8, a trap and an exhausted stack — and print what they print natively is the ignored tests
//! at the end, run by hand against the `nazm-wine:g3` image (`docker/wine.Dockerfile`): a test does
//! not start a container by itself. Their evidence is *run-verified under Wine*, in a Linux
//! container, and never evidence from a Windows host.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use differential::nazm;

const TARGET: &str = "x86_64-pc-windows-gnu";
const IMAGE: &str = "nazm-wine:g3";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/windows")
}

const BACKENDS: [&str; 2] = ["llvm", "cranelift"];

/// `name.nz` from the fixtures, built for Windows by `backend` to `name-backend/` in `dir`.
fn objects(dir: &Path, name: &str, backend: &str) -> String {
    std::fs::copy(
        fixtures().join(format!("{name}.nz")),
        dir.join(format!("{name}.nz")),
    )
    .unwrap();
    let out = format!("{name}-{backend}");
    let r = nazm(
        dir,
        &[
            "build",
            "--backend",
            backend,
            "--target",
            TARGET,
            "--objects",
            &out,
            &format!("{name}.nz"),
        ],
    );
    assert_eq!(r.code, Some(0), "{out}: {}", r.stderr);
    out
}

/// One run: the program, its arguments, its environment, the code it ends with and what it
/// prints — the fixture's `.expected`, or for a failure a code its standard error names.
struct Case {
    program: &'static str,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    code: i32,
    stderr_names: Option<&'static str>,
}

const fn case(program: &'static str) -> Case {
    Case {
        program,
        args: &[],
        env: &[],
        code: 0,
        stderr_names: None,
    }
}

const ONE_WORKER: &[(&str, &str)] = &[("NAZM_WORKERS", "1")];

const CASES: &[Case] = &[
    case("files"),
    case("net_echo"),
    case("net_datagram"),
    case("net_failures"),
    case("net_close"),
    case("net_peer_gone"),
    // A task waiting on a socket, a sleep or a child's pipe releases the only worker.
    Case {
        env: ONE_WORKER,
        ..case("net_pool")
    },
    case("time"),
    Case {
        env: ONE_WORKER,
        ..case("time_pool")
    },
    Case {
        env: ONE_WORKER,
        args: &["child.exe"],
        ..case("pipe_pool")
    },
    // `os_kill` is `TerminateProcess` with exit code 1, where POSIX answers 265.
    Case {
        args: &["child.exe"],
        ..case("process")
    },
    Case {
        args: &["plain", "two words", "", "naïve", "日本"],
        env: &[("LANG", "C.UTF-8")],
        ..case("args")
    },
    Case {
        code: 2,
        stderr_names: Some("error[N0400]"),
        ..case("trap")
    },
    Case {
        code: 2,
        stderr_names: Some("error[N0408]"),
        ..case("deep")
    },
    Case {
        code: 2,
        stderr_names: Some("error[N0408]"),
        ..case("task_deep")
    },
];

/// POSIX shell's single quotes.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[test]
fn every_program_builds_for_windows_with_mingw_and_winsock() {
    let d = tempfile::tempdir().unwrap();
    for name in CASES.iter().map(|c| c.program).chain(["child"]) {
        for backend in BACKENDS {
            let out = objects(d.path(), name, backend);
            let link = std::fs::read_to_string(d.path().join(&out).join("link.txt")).unwrap();
            let libraries = link
                .lines()
                .find_map(|l| l.strip_prefix("libraries "))
                .unwrap_or_else(|| panic!("{out}: {link}"));
            for lib in ["-static", "-lws2_32", "-lbcrypt"] {
                assert!(libraries.split(' ').any(|l| l == lib), "{out}: {libraries}");
            }
        }
    }
}

#[test]
fn a_dll_is_refused_by_name_and_a_static_library_is_built() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub extern \"C\" fn nz_add(a: Int, b: Int) -> Int ! {} = \"nz_add\" { a + b }\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--lib",
            "--shared",
            "--target",
            TARGET,
            "--objects",
            "o",
            "lib.nz",
        ],
    );
    assert_eq!(r.code, Some(5), "{}", r.stderr);
    assert!(r.stderr.contains("builds no DLL"), "{}", r.stderr);
    let r = nazm(
        d.path(),
        &[
            "build",
            "--lib",
            "--target",
            TARGET,
            "--objects",
            "s",
            "lib.nz",
        ],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// Every case under Wine in one container, built by each backend: each linked by MinGW-w64's gcc
/// as `link.txt` says and run in a directory of its own, beside `child.exe` (LLVM's).
#[test]
#[ignore = "starts a container"]
fn the_programs_run_under_wine_as_they_do_natively() {
    let d = tempfile::tempdir().unwrap();
    objects(d.path(), "child", "llvm");
    let mut script = String::from(
        "set -u\nlink() { (cd \"/w/$1\" && x86_64-w64-mingw32-gcc $(sed -n 's/^objects //p' link.txt) \
         -o \"/w/$1.exe\" $(sed -n 's/^libraries //p' link.txt)); }\n\
         link child-llvm || exit 90\ncp /w/child-llvm.exe /w/child.exe\nmkdir -p /w/out\n",
    );
    for (c, backend) in CASES.iter().flat_map(|c| BACKENDS.map(|b| (c, b))) {
        let p = objects(d.path(), c.program, backend);
        let env: String = c
            .env
            .iter()
            .map(|(k, v)| format!("{k}={} ", quoted(v)))
            .collect();
        let args: Vec<String> = c.args.iter().map(|a| quoted(a)).collect();
        let _ = writeln!(
            script,
            "if link {p}; then mkdir -p /w/run/{p} && cp /w/{p}.exe /w/child.exe /w/run/{p}/ && \
             (cd /w/run/{p} && {env}timeout 180 /usr/lib/wine/wine64 {p}.exe {args} \
             > /w/out/{p}.out 2> /w/out/{p}.err; echo $? > /w/out/{p}.code); \
             else echo link > /w/out/{p}.code; fi",
            args = args.join(" ")
        );
    }
    std::fs::write(d.path().join("run.sh"), script).unwrap();
    let o = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--platform",
            "linux/amd64",
            "--network",
            "none",
            "-v",
        ])
        .arg(format!("{}:/w", d.path().display()))
        .args([IMAGE, "sh", "/w/run.sh"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let read = |p: &str, ext: &str| {
        std::fs::read_to_string(d.path().join("out").join(format!("{p}.{ext}")))
            .unwrap_or_default()
            .replace("\r\n", "\n")
    };
    for (c, backend) in CASES.iter().flat_map(|c| BACKENDS.map(|b| (c, b))) {
        let p = &format!("{}-{backend}", c.program);
        let (code, out, err) = (read(p, "code"), read(p, "out"), read(p, "err"));
        assert_eq!(code.trim(), c.code.to_string(), "{p}: {err}");
        match c.stderr_names {
            Some(name) => assert!(err.contains(name), "{p}: {err}"),
            None => {
                let want =
                    std::fs::read_to_string(fixtures().join(format!("{}.expected", c.program)))
                        .unwrap();
                assert_eq!(out, want, "{p}: {err}");
            }
        }
    }
}

/// The conformance programs, each built for Windows by each backend and run under Wine, print
/// what they expect.
#[test]
#[ignore = "starts a container"]
fn the_conformance_programs_run_under_wine() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compiler/conformance");
    let d = tempfile::tempdir().unwrap();
    let mut names = Vec::new();
    for e in std::fs::read_dir(&root).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_some_and(|x| x == "nz") && path.with_extension("expected").exists() {
            let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            std::fs::copy(&path, d.path().join(format!("{name}.nz"))).unwrap();
            for backend in BACKENDS {
                let out = format!("{name}-{backend}");
                let r = nazm(
                    d.path(),
                    &[
                        "build",
                        "--backend",
                        backend,
                        "--target",
                        TARGET,
                        "--objects",
                        &out,
                        &format!("{name}.nz"),
                    ],
                );
                assert_eq!(r.code, Some(0), "{out}: {}", r.stderr);
                names.push((name.clone(), out));
            }
        }
    }
    assert!(names.len() >= 40, "{names:?}");
    let mut script = String::from("set -u\nmkdir -p /w/out\n");
    for (_, n) in &names {
        let _ = writeln!(
            script,
            "(cd /w/{n} && x86_64-w64-mingw32-gcc $(sed -n 's/^objects //p' link.txt) -o /w/{n}.exe \
             $(sed -n 's/^libraries //p' link.txt)) && \
             (cd /w && timeout 180 /usr/lib/wine/wine64 {n}.exe > /w/out/{n}.out 2>&1)"
        );
    }
    std::fs::write(d.path().join("run.sh"), script).unwrap();
    let o = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--platform",
            "linux/amd64",
            "--network",
            "none",
            "-v",
        ])
        .arg(format!("{}:/w", d.path().display()))
        .args([IMAGE, "sh", "/w/run.sh"])
        .output()
        .unwrap();
    assert!(
        d.path().join("out").is_dir(),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    for (name, n) in &names {
        let got = std::fs::read_to_string(d.path().join("out").join(format!("{n}.out")))
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let want = std::fs::read_to_string(root.join(format!("{name}.expected"))).unwrap();
        assert_eq!(got.trim_end(), want.trim_end(), "{n}");
    }
}
