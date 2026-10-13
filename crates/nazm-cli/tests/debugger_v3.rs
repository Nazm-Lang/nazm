//! N91: debugger and profiler v3 — lexical scopes, Cranelift's debug information, and sampling.
//!
//! `docs/architecture.md` §7.92. What a real debugger and a real sampler show of a Nazm program:
//! a binding visible only in its own block (LLVM); a Cranelift build's functions and lines found
//! by `lldb` — and, in the `nazm-debug` image, a live `gdb` session on either backend's build;
//! a sampled run's samples attributed to Nazm functions and `.nz` lines by macOS's `sample`.
//! Tools a host lacks are skipped by name, never stood in for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use crate::common;

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Three blocks: the function's own (`outer`), and each branch's (`inner`, `other`).
const SCOPES: &str = "fn f(n: Int) -> Int {\n    let outer = n + 1;\n    if n > 0 {\n        \
                      let inner = outer * 2;\n        inner + 1\n    } else {\n        \
                      let other = outer - 1;\n        other\n    }\n}\nfn main() -> Int { f(3) }\n";

/// A loop long enough to be sampled: about a second of work.
const SPIN: &str = "fn work(n: Int) -> Int {\n    let mut i = 0;\n    let mut acc = 0;\n    \
                    while i < n {\n        acc = (acc + i * 7) % 1000003;\n        i = i + 1;\n    \
                    }\n    acc\n}\nfn main() -> Int { work(120000000) }\n";

fn nazm(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn with(file: &str, text: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(file), text).unwrap();
    d
}

/// Whether `lldb` runs here.
fn lldb() -> bool {
    Command::new("lldb")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `lldb`'s answer to static queries — nothing is launched.
fn query(dir: &Path, exe: &str, commands: &[&str]) -> String {
    let mut c = Command::new("lldb");
    c.current_dir(dir).arg("--batch");
    for q in commands {
        c.args(["-o", q]);
    }
    let o = c.arg(exe).output().unwrap();
    out(&o)
}

#[test]
fn a_binding_is_scoped_to_its_own_block() {
    let d = with("s.nz", SCOPES);
    let b = nazm(
        d.path(),
        &[
            "build",
            "--debug",
            "--no-cache",
            "--emit-ir",
            "s.nz",
            "-o",
            "s",
        ],
    );
    assert!(b.status.success(), "{}", err(&b));
    let ir = std::fs::read_to_string(d.path().join("s.m0.ll")).unwrap();
    // One lexical block per branch, each opening where its `{` is, inside the subprogram.
    let blocks: Vec<&str> = ir
        .lines()
        .filter(|l| l.contains("DILexicalBlock("))
        .collect();
    assert_eq!(blocks.len(), 2, "{ir}");
    assert!(blocks[0].contains("line: 3, column: 14"), "{blocks:?}");
    assert!(blocks[1].contains("line: 6, column: 12"), "{blocks:?}");
    let node = |l: &str| l.split(' ').next().unwrap().to_owned();
    let scope_of = |var: &str| {
        let line = ir
            .lines()
            .find(|l| l.contains(&format!("DILocalVariable(name: \"{var}\"")))
            .unwrap();
        let at = line.find("scope: ").unwrap() + "scope: ".len();
        line[at..].split(',').next().unwrap().to_owned()
    };
    assert_eq!(scope_of("inner"), node(blocks[0]));
    assert_eq!(scope_of("other"), node(blocks[1]));
    // A binding of the function's own body, and a parameter, are the subprogram's.
    let sp = ir
        .lines()
        .find(|l| l.contains("DISubprogram(name: \"f\""))
        .map(node)
        .unwrap();
    assert_eq!(scope_of("outer"), sp);
    assert_eq!(scope_of("n"), sp);
    // An instruction of `inner`'s line is in `inner`'s block.
    assert!(
        ir.contains(&format!(
            "DILocation(line: 5, column: 15, scope: {})",
            node(blocks[0])
        )),
        "{ir}"
    );
    // The scopes change nothing the program does.
    let r = common::bounded_exe(&d.path().join("s")).output().unwrap();
    assert_eq!(out(&r), "9\n");
}

#[test]
fn a_cranelift_build_carries_debug_information_only_when_asked() {
    let d = with("s.nz", SCOPES);
    for (dir, debug) in [("plain", false), ("debug", true)] {
        let mut args = vec![
            "build",
            "--no-cache",
            "--backend",
            "cranelift",
            "--objects",
            dir,
            "s.nz",
        ];
        if debug {
            args.insert(1, "--debug");
        }
        let b = nazm(d.path(), &args);
        assert!(b.status.success(), "{}", err(&b));
        let object = std::fs::read(d.path().join(dir).join("00-m0.o")).unwrap();
        let has = object.windows(10).any(|w| w == b"debug_info");
        let line = object.windows(10).any(|w| w == b"debug_line");
        assert_eq!((has, line), (debug, debug), "{dir}");
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "drives lldb on macOS")]
fn a_debugger_finds_a_cranelift_build_s_functions_and_lines() {
    if common::skip_without(lldb(), "no lldb on this machine") {
        return;
    }
    let d = with("s.nz", SCOPES);
    let b = nazm(
        d.path(),
        &[
            "build",
            "--debug",
            "--backend",
            "cranelift",
            "s.nz",
            "-o",
            "s",
        ],
    );
    assert!(b.status.success(), "{}", err(&b));
    // Each subprogram's address in the gathered DWARF is its symbol's in the executable: an
    // address left as an offset into its section would still let a breakpoint by name resolve
    // through the symbol table, and say nothing true about lines.
    if cfg!(target_os = "macos") {
        let dwarf = Command::new("dwarfdump")
            .args(["--debug-info", "s.dSYM"])
            .current_dir(d.path())
            .output()
            .unwrap();
        let dwarf = out(&dwarf);
        let symbols = Command::new("nm")
            .arg("s")
            .current_dir(d.path())
            .output()
            .unwrap();
        let symbols = out(&symbols);
        for f in ["f", "main"] {
            let at = dwarf
                .split("DW_TAG_subprogram")
                .find(|die| die.contains(&format!("DW_AT_name\t(\"{f}\")")))
                .and_then(|die| die.split("DW_AT_low_pc\t(0x").nth(1))
                .and_then(|r| r.split(')').next())
                .map(|h| u64::from_str_radix(h, 16).unwrap())
                .unwrap_or_else(|| panic!("no subprogram {f}: {dwarf}"));
            let symbol = symbols
                .lines()
                .find(|l| l.ends_with(&format!(".{f}")) && l.contains("nz.m."))
                .and_then(|l| u64::from_str_radix(l.split_whitespace().next()?, 16).ok())
                .unwrap_or_else(|| panic!("no symbol for {f}: {symbols}"));
            assert_eq!(at, symbol, "{f}: DWARF {at:#x}, symbol {symbol:#x}");
        }
    }
    let text = query(
        d.path(),
        "./s",
        &["breakpoint set -n f", "breakpoint set -f s.nz -l 5"],
    );
    // By function: after the prologue, on the first statement.
    assert!(
        text.contains("`f + ") && text.contains("at s.nz:2:"),
        "{text}"
    );
    assert!(text.contains("at s.nz:5:"), "a breakpoint by line: {text}");
    // A debug object's lines follow the source even where its MIR does not change: a line
    // added above the function moves every line, and the next build says so — no debug object
    // is reused from the cache.
    std::fs::write(d.path().join("s.nz"), format!("// a comment\n{SCOPES}")).unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--debug",
            "--backend",
            "cranelift",
            "s.nz",
            "-o",
            "s",
        ],
    );
    assert!(b.status.success(), "{}", err(&b));
    let text = query(d.path(), "./s", &["breakpoint set -n f"]);
    assert!(text.contains("at s.nz:3:"), "{text}");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "macOS's `sample` is the sampler")]
fn a_sampled_run_is_attributed_to_nazm_functions_and_lines() {
    if common::skip_without(
        Path::new("/usr/bin/sample").is_file(),
        "no sampler on this machine",
    ) {
        return;
    }
    let d = with("spin.nz", SPIN);
    for backend in ["llvm", "cranelift"] {
        let o = nazm(
            d.path(),
            &["profile", "--sample", "--backend", backend, "spin.nz"],
        );
        assert!(o.status.success(), "{}", err(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["schema"], "nazm.profile/2");
        assert_eq!(v["exit"], 0);
        let s = &v["samples"];
        assert_eq!(s["sampler"], "sample");
        assert_eq!(s["interval_ms"], 1);
        let total = s["total"].as_u64().unwrap();
        // Every sample is in exactly one place.
        let sum = |key: &str| -> u64 {
            s[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["samples"].as_u64().unwrap())
                .sum()
        };
        assert_eq!(
            total,
            sum("nazm")
                + sum("runtime")
                + sum("foreign")
                + sum("system")
                + s["unattributed"].as_u64().unwrap(),
            "{v}"
        );
        // The loop is where the program's own time is: `work`, on its lines — the loop's
        // condition, body and increment — or on code no statement made (line 0).
        let nazm = s["nazm"].as_array().unwrap();
        let in_work: u64 = nazm
            .iter()
            .filter(|e| e["function"] == "work" && e["file"] == "spin.nz")
            .map(|e| e["samples"].as_u64().unwrap())
            .sum();
        assert!(in_work > 100, "{backend}: {v}");
        assert!(
            nazm.iter()
                .all(|e| [0, 4, 5, 6, 10].contains(&e["line"].as_u64().unwrap())),
            "{backend}: {v}"
        );
        assert!(nazm.iter().any(|e| e["line"] == 5), "{backend}: {v}");
    }
}

#[test]
fn an_interval_without_sampling_is_refused() {
    let d = with("spin.nz", SPIN);
    let o = nazm(d.path(), &["profile", "--interval-ms", "5", "spin.nz"]);
    assert_eq!(o.status.code(), Some(5), "{}", err(&o));
    assert!(err(&o).contains("--sample"), "{}", err(&o));
}

/// gdb on each backend's build, in the Linux container: a breakpoint by function and by line,
/// a backtrace with `.nz` lines, stepping by statement; under LLVM, a branch's binding visible
/// in its block and not before it. Needs Docker and the `nazm-debug:n58` image.
#[test]
#[ignore = "needs Docker and the nazm-debug image (the gate image plus gdb); run with --ignored"]
fn a_live_session_on_either_backend_shows_nazm_frames_and_lines() {
    let d = with("s.nz", SCOPES);
    for (backend, dir) in [("llvm", "lo"), ("cranelift", "co")] {
        let b = nazm(
            d.path(),
            &[
                "build",
                "--debug",
                "--no-cache",
                "--backend",
                backend,
                "--target",
                "aarch64-unknown-linux-gnu",
                "--objects",
                dir,
                "s.nz",
            ],
        );
        assert!(b.status.success(), "{}", err(&b));
    }
    let script = "for b in lo co; do echo \"== $b\"; cc /src/$b/*.o -lpthread -o /tmp/s && \
                  gdb -q -batch -ex 'break f' -ex 'break s.nz:5' -ex run -ex 'info locals' -ex bt \
                  -ex continue -ex 'info locals' -ex next /tmp/s; done";
    let o = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "--cap-add=SYS_PTRACE",
            "--security-opt",
            "seccomp=unconfined",
            "-v",
        ])
        .arg(format!("{}:/src:ro", d.path().display()))
        .args(["nazm-debug:n58", "sh", "-c", script])
        .output()
        .unwrap();
    let text = out(&o);
    let (llvm, clif) = text.split_once("== co").unwrap();
    for (name, part) in [("llvm", llvm), ("cranelift", clif)] {
        for want in [
            "Breakpoint 1 at",
            "file s.nz, line 2.",
            "Breakpoint 2 at",
            "file s.nz, line 5.",
            "at s.nz:2",
            "in main () at s.nz:11",
            "at s.nz:5",
            "5\t        inner + 1",
        ] {
            assert!(part.contains(want), "{name}: {want}\n{part}\n{}", err(&o));
        }
    }
    // LLVM: at the function's first statement only the body's binding is in scope; in the
    // branch, the branch's too.
    let (before, after) = llvm.split_once("Breakpoint 2,").unwrap();
    assert!(
        before.contains("outer = ") && !before.contains("inner = "),
        "{llvm}"
    );
    assert!(
        after.contains("inner = 8") && after.contains("outer = 4"),
        "{llvm}"
    );
    // Cranelift: lines and frames; its variables are not described.
    assert!(clif.contains("No locals."), "{clif}");
}

/// For `docs/performance.md` (N91): a sampled run's wall time against the same executable alone,
/// at two intervals, and a Cranelift build with and without `--debug`. Medians of seven and five.
#[test]
#[ignore = "a measurement, for docs/performance.md; run with --ignored --nocapture"]
fn what_debugging_and_sampling_cost() {
    if !Path::new("/usr/bin/sample").is_file() {
        eprintln!("SKIP: no sampler on this machine");
        return;
    }
    let median = |mut xs: Vec<f64>| {
        xs.sort_by(f64::total_cmp);
        xs[(xs.len() - 1) / 2]
    };
    let d = with("spin.nz", SPIN);
    let b = nazm(
        d.path(),
        &["build", "--debug", "--no-cache", "spin.nz", "-o", "spin"],
    );
    assert!(b.status.success(), "{}", err(&b));
    let exe = d.path().join("spin");
    // The first launch of a fresh executable is not the program's time.
    Command::new(&exe).output().unwrap();
    let run = |interval: Option<&str>| {
        let start = std::time::Instant::now();
        let mut child = Command::new(&exe)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let sampler = interval.map(|i| {
            Command::new("/usr/bin/sample")
                .args([&child.id().to_string(), "3600", i, "-file", "/dev/null"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        });
        child.wait().unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if let Some(mut s) = sampler {
            s.wait().unwrap();
        }
        ms
    };
    for (label, interval) in [
        ("alone", None),
        ("every 1 ms", Some("1")),
        ("every 10 ms", Some("10")),
    ] {
        let ms = median((0..7).map(|_| run(interval)).collect());
        println!("sampling {label}: {ms:.0} ms");
    }
    let program = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/programs/strings.nz"),
    )
    .unwrap();
    std::fs::write(d.path().join("strings.nz"), program).unwrap();
    for (label, debug) in [("ordinary", false), ("--debug", true)] {
        let mut args = vec![
            "build",
            "--no-cache",
            "--backend",
            "cranelift",
            "strings.nz",
            "-o",
            "s",
        ];
        if debug {
            args.insert(1, "--debug");
        }
        let ms = median(
            (0..5)
                .map(|_| {
                    let start = std::time::Instant::now();
                    assert!(nazm(d.path(), &args).status.success());
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .collect(),
        );
        println!("cranelift build, {label}: {ms:.0} ms");
    }
}
