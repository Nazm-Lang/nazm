//! N82: the runtime as a built, versioned, verified artifact.
//!
//! `docs/architecture.md` §7.83. What the commands show: `nazm runtime build` writes an archive and
//! a `nazm.runtime/1` manifest, the same bytes twice; `nazm runtime verify` accepts it; a program
//! reaching every service, linked against it, behaves exactly as with a generated runtime — output,
//! ending and memory report — on both backends; every way an artifact can fail to fit is refused by
//! name before anything is written, with no fallback; a target's profile is its own; a freestanding
//! build links its board's archive; and the provenance record names the archive linked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every runtime service: strings, sequences, I/O, a closure, a task, a channel, a join and a
/// call cycle (the stack check).
const EVERY: &str = "fn count(n: Int) -> Int { if n == 0 { 0 } else { 1 + count(n - 1) } }\n\
fn work(io: IoCap, c: Chan) -> Int ! { io } { chan_send(c, 7); chan_close(c); 0 }\n\
fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } {\n\
    let xs = strs_new(); strs_push(xs, \"a\"); strs_push(xs, \"b\");\n\
    let joined = str_join(xs, \"-\");\n\
    let add = fn(x: Int) -> Int { x + 1 };\n\
    let c = chan_new(4);\n\
    scope { spawn work(io, c); }\n\
    let got = ints_new(); let ok = chan_recv(c, got);\n\
    print(joined); print(int_to_str(add(ints_pop(got)) + count(5) + args_count()));\n\
    0\n\
}\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn artifact(dir: &Path, name: &str, extra: &[&str]) -> Value {
    let mut args = vec!["runtime", "build", "-o", name];
    args.extend_from_slice(extra);
    let o = nazm(dir, &args);
    assert!(o.status.success(), "{}", text(&o));
    serde_json::from_str(
        &std::fs::read_to_string(dir.join(name).join("nazm-runtime.json")).unwrap(),
    )
    .unwrap()
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), EVERY).unwrap();
    d
}

/// Build `p.nz` with `extra` arguments and run it with the memory report on.
fn built_and_run(dir: &Path, out: &str, extra: &[&str]) -> (String, Option<i32>) {
    let mut args = vec!["build", "p.nz", "-o", out];
    args.extend_from_slice(extra);
    let o = nazm(dir, &args);
    assert!(o.status.success(), "{extra:?}: {}", text(&o));
    let r = Command::new(dir.join(out))
        .current_dir(dir)
        .env("NAZM_MEMORY_REPORT", "1")
        .output()
        .unwrap();
    (text(&r), r.status.code())
}

#[test]
fn an_artifact_is_built_described_and_the_same_bytes_twice() {
    let d = setup();
    let m = artifact(d.path(), "rt", &[]);
    assert_eq!(m["schema"], "nazm.runtime/1");
    assert_eq!(m["profile"], "hosted");
    assert!(m["abi"].as_u64().is_some());
    assert_eq!(m["runtime"].as_str().unwrap().len(), 64);
    let services: Vec<&str> = m["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    for s in [
        "tasks",
        "channels",
        "sequences",
        "io",
        "join",
        "strings",
        "shims",
        "stack",
        "closures",
    ] {
        assert!(services.contains(&s), "{s}: {services:?}");
    }
    assert!(
        !d.path().join("rt/.nazm-runtime-work").exists(),
        "the working files are gone"
    );
    let again = artifact(d.path(), "rt2", &[]);
    assert_eq!(
        m["archive"], again["archive"],
        "the same archive bytes twice"
    );
    assert_eq!(m["text"], again["text"]);
    let v = nazm(d.path(), &["runtime", "verify", "rt"]);
    assert!(v.status.success(), "{}", text(&v));
    assert!(text(&v).contains("fits this compiler"));
}

#[test]
fn a_program_reaching_every_service_behaves_the_same_with_the_artifact_on_both_backends() {
    let d = setup();
    artifact(d.path(), "rt", &[]);
    for backend in ["llvm", "cranelift"] {
        let generated = built_and_run(d.path(), &format!("gen-{backend}"), &["--backend", backend]);
        let linked = built_and_run(
            d.path(),
            &format!("art-{backend}"),
            &["--backend", backend, "--runtime", "rt"],
        );
        assert_eq!(generated, linked, "{backend}");
        assert!(linked.0.contains("a-b"), "{backend}: {}", linked.0);
        assert!(
            linked.0.contains("nazm-memory:"),
            "{backend}: the report is printed"
        );
        assert_eq!(linked.1, Some(0));
        // The artifact is what was linked: no runtime is generated beside it, which on a linker
        // that pulls archive members only when needed would leave the artifact silently unused.
        let objects = format!("objects-{backend}");
        let o = nazm(
            d.path(),
            &[
                "build",
                "p.nz",
                "--backend",
                backend,
                "--runtime",
                "rt",
                "--objects",
                &objects,
            ],
        );
        assert!(o.status.success(), "{}", text(&o));
        let names: Vec<String> = std::fs::read_dir(d.path().join(&objects))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with("libnazmrt.a"))
                && !names.iter().any(|n| n.ends_with("-runtime.o")),
            "{backend}: {names:?}"
        );
    }
}

#[test]
fn every_artifact_that_does_not_fit_is_refused_by_name_and_nothing_is_written() {
    let d = setup();
    artifact(d.path(), "rt", &[]);
    let manifest = d.path().join("rt/nazm-runtime.json");
    let original = std::fs::read_to_string(&manifest).unwrap();
    let refused = |why: &str| {
        // An artifact that cannot be read is 3, one that does not fit 1 (Q1-C1).
        let status = if why.contains("no readable") { 3 } else { 1 };
        let o = nazm(d.path(), &["build", "p.nz", "-o", "out", "--runtime", "rt"]);
        assert_eq!(o.status.code(), Some(status), "{why}: {}", text(&o));
        assert!(text(&o).contains(why), "{why}: {}", text(&o));
        assert!(!d.path().join("out").exists(), "{why}: nothing is written");
        let v = nazm(d.path(), &["runtime", "verify", "rt"]);
        if !why.contains("reaches") {
            assert_eq!(v.status.code(), Some(status), "verify, {why}");
        }
    };
    let edit = |from: &str, to: &str| {
        std::fs::write(&manifest, original.replacen(from, to, 1)).unwrap();
    };
    let m: Value = serde_json::from_str(&original).unwrap();
    edit("nazm.runtime/1", "nazm.runtime/2");
    refused("this compiler reads `nazm.runtime/1`");
    edit(&format!("\"abi\":{}", m["abi"]), "\"abi\":1");
    refused("ABI revision 1");
    edit(m["runtime"].as_str().unwrap(), &"0".repeat(64));
    refused("was built from runtime 0000");
    edit(m["target"].as_str().unwrap(), "x86_64-unknown-linux-gnu");
    refused("the runtime is for x86_64-unknown-linux-gnu");
    edit("\"tasks\",", "");
    refused("the program reaches tasks of the runtime");
    edit("\"tasks\",", "\"teleport\",");
    refused("a service this compiler does not know");
    std::fs::write(&manifest, &original).unwrap();
    // An archive whose bytes are not the ones built.
    let archive = d.path().join("rt/libnazmrt.a");
    let mut bytes = std::fs::read(&archive).unwrap();
    bytes.push(0);
    std::fs::write(&archive, &bytes).unwrap();
    refused("is not the archive that was built");
    std::fs::remove_file(&manifest).unwrap();
    refused("has no readable manifest");
}

#[test]
fn a_targets_profile_is_its_own_and_a_board_links_its_archive() {
    let d = setup();
    let m = artifact(d.path(), "board", &["--target", "aarch64-unknown-none"]);
    assert_eq!(m["profile"], "board");
    let w = artifact(d.path(), "wasm", &["--target", "wasm32-unknown-unknown"]);
    assert_eq!(w["profile"], "wasm");
    let o = nazm(
        d.path(),
        &[
            "runtime",
            "build",
            "--target",
            "aarch64-unknown-none",
            "--profile",
            "hosted",
            "-o",
            "x",
        ],
    );
    assert_eq!(o.status.code(), Some(5));
    assert!(
        text(&o).contains("links the `board` runtime, not `hosted`"),
        "{}",
        text(&o)
    );
    // A hosted build given the board's artifact is refused for its target.
    let o = nazm(
        d.path(),
        &["build", "p.nz", "-o", "h", "--runtime", "board"],
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o).contains("the runtime is for aarch64-unknown-none"),
        "{}",
        text(&o)
    );
    // A freestanding image's objects include the board's archive, in place of its runtime.
    std::fs::write(d.path().join("fs.nz"), "fn main() -> Int { 40 + 2 }\n").unwrap();
    let o = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--runtime",
            "board",
            "--objects",
            "objs",
            "fs.nz",
            "-o",
            "fs",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    let link = std::fs::read_to_string(d.path().join("objs/link.txt")).unwrap();
    assert!(link.contains("libnazmrt.a"), "{link}");
    assert!(!link.contains("runtime.o"), "no generated runtime: {link}");
}

#[test]
fn the_provenance_record_names_the_archive_linked() {
    let d = setup();
    let m = artifact(d.path(), "rt", &[]);
    let o = nazm(
        d.path(),
        &[
            "build",
            "p.nz",
            "-o",
            "out",
            "--runtime",
            "rt",
            "--provenance",
            "prov.json",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    let p: Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("prov.json")).unwrap())
            .unwrap();
    assert_eq!(
        p["runtime"]["archive"],
        format!("blake3:{}", m["archive"].as_str().unwrap())
    );
    // Without an artifact the record is as before.
    let o = nazm(
        d.path(),
        &["build", "p.nz", "-o", "out2", "--provenance", "prov2.json"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let p: Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("prov2.json")).unwrap())
            .unwrap();
    assert!(p["runtime"].get("archive").is_none());
}
