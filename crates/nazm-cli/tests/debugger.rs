//! N58: debugger and profiler v2 — variables, a live session, and the runtime's counters as a
//! profile.
//!
//! `docs/architecture.md` §7.60. The live session needs a debugger allowed to trace a process,
//! which this host's developer mode does not grant a non-interactive run; it is the ignored test
//! below, run in the `nazm-debug` image (the gate's image plus gdb) with `--ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROGRAM: &str = "fn greet(name: Str, times: Int, loud: Bool) -> Int {\n    \
                       let line = str_concat(\"hello \", name);\n    \
                       let n = str_len(line) * times;\n    \
                       if loud { n + 1 } else { n }\n}\n\n\
                       fn main() -> Int {\n    let who = str_concat(\"wor\", \"ld\");\n    \
                       greet(who, 2, true)\n}\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn parameters_and_bindings_are_described_and_temporaries_are_not() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--debug",
            "--no-cache",
            "--emit-ir",
            "prog.nz",
            "-o",
            "prog",
        ],
    );
    assert!(b.status.success(), "{}", err(&b));
    let ir = std::fs::read_to_string(d.path().join("prog.m0.ll")).unwrap();
    for want in [
        "!DILocalVariable(name: \"name\", arg: 1,",
        "!DILocalVariable(name: \"times\", arg: 2,",
        "!DILocalVariable(name: \"loud\", arg: 3,",
        "!DILocalVariable(name: \"line\", scope:",
        "!DILocalVariable(name: \"n\", scope:",
        "!DILocalVariable(name: \"who\", scope:",
        "!DIBasicType(name: \"Int\", size: 64, encoding: DW_ATE_signed)",
        "!DIBasicType(name: \"Bool\", size: 8, encoding: DW_ATE_boolean)",
        "!DICompositeType(tag: DW_TAG_structure_type, name: \"Str\", size: 192,",
        "name: \"data\"",
        "name: \"len\"",
        "name: \"owner\"",
        "declare void @llvm.dbg.declare(metadata, metadata, metadata)",
        // A function is found by its Nazm name.
        "DISubprogram(name: \"greet\", scope:",
    ] {
        assert!(ir.contains(want), "{want}\n{ir}");
    }
    // Every described variable is one the source wrote: no compiler-made temporary.
    for line in ir.lines().filter(|l| l.contains("DILocalVariable")) {
        assert!(!line.contains('#'), "a temporary described: {line}");
    }
    assert_eq!(ir.matches("DILocalVariable(").count(), 6, "{ir}");
    // Each declared on its slot, where the debugger reads it.
    assert_eq!(
        ir.matches("call void @llvm.dbg.declare(metadata ptr %slot")
            .count(),
        6,
        "{ir}"
    );
    // Without `--debug`, none of it.
    let plain = nazm(
        d.path(),
        &["build", "--no-cache", "--emit-ir", "prog.nz", "-o", "plain"],
    );
    assert!(plain.status.success());
    let plain_ir = std::fs::read_to_string(d.path().join("plain.m0.ll")).unwrap();
    assert!(!plain_ir.contains("DILocalVariable") && !plain_ir.contains("nz.var"));
}

#[test]
fn a_profile_reports_what_one_run_did_under_both_backends() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn one(c: Chan[Int]) -> Bool ! {} { chan_send_of(c, 1) }\n\
         fn main(io: IoCap, sp: SpawnCap) -> Int ! { io, spawn } {\n    \
         let c = chan_new_of[Int](4);\n    scope { spawn one(c); spawn one(c); }\n    \
         let cs = vec_new[Chan[Int]]();\n    vec_push(cs, c);\n    \
         let got = vec_new[Int]();\n    chan_select_of(cs, got);\n    print(\"ok\");\n    0\n}\n",
    )
    .unwrap();
    for backend in ["llvm", "cranelift"] {
        let o = nazm(d.path(), &["profile", "--backend", backend, "prog.nz"]);
        assert!(o.status.success(), "{}", err(&o));
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["schema"], "nazm.profile/1");
        assert_eq!(v["backend"], backend);
        assert_eq!(v["exit"], 0);
        // `ok`, and main's result line.
        assert_eq!(v["stdout_bytes"], 4, "{v}");
        assert_eq!(v["scheduler"]["spawned"], 2, "{v}");
        assert_eq!(v["scheduler"]["joined"], 2, "{v}");
        assert_eq!(v["scheduler"]["selects"], 1, "{v}");
        assert_eq!(v["scheduler"]["timeouts"], 0, "{v}");
        for kind in ["sequences", "strings", "channels", "closures"] {
            assert_eq!(v["memory"][kind]["live"], 0, "{kind}: {v}");
        }
        assert_eq!(v["memory"]["channels"]["allocated"], 1, "{v}");
        assert!(v["wall_ms"].as_u64().is_some(), "{v}");
    }
}

/// gdb on the Nazm program, in the Linux container: a breakpoint by function name, a backtrace
/// through two Nazm functions with their lines, parameters and bindings read while live, and
/// stepping by statement. Needs Docker and the `nazm-debug:n58` image.
#[test]
#[ignore = "needs Docker and the nazm-debug image (the gate image plus gdb); run with --ignored"]
fn a_live_session_shows_nazm_frames_lines_and_variables() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("v.nz"), PROGRAM).unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--debug",
            "--no-cache",
            "--target",
            "aarch64-unknown-linux-gnu",
            "--objects",
            "o",
            "v.nz",
        ],
    );
    assert!(b.status.success(), "{}", err(&b));
    let script = "cd /src && cc /src/o/00-m0.o /src/o/01-runtime.o /src/o/02-entry.o -lpthread -o /tmp/v && \
                  gdb -q -batch -ex 'break greet' -ex run -ex bt -ex 'info args' -ex 'info locals' \
                  -ex next -ex next -ex 'info locals' /tmp/v";
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
    let out = String::from_utf8_lossy(&o.stdout);
    for want in [
        "Breakpoint 1, greet (name=..., times=2, loud=true) at v.nz:2",
        "in main () at v.nz:9",
        "name = {data = ",
        "\"world\", len = 5,",
        "times = 2",
        "loud = true",
        // Before its statement the binding is its zeroed slot, never stale storage.
        "line = {data = 0x0, len = 0, owner = 0x0}",
        "3\t    let n = str_len(line) * times;",
        "\"hello world\", len = 11,",
        "n = 22",
    ] {
        assert!(
            out.contains(want),
            "{want}\n{out}\n{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}
