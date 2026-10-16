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
    // A target that is not this host's, whichever host runs the suite.
    let host = m["target"].as_str().unwrap();
    let other = if host == "x86_64-unknown-linux-gnu" {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    edit(host, other);
    refused(&format!("the runtime is for {other}"));
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

/// The archive's symbol index, as (name, member offset), in its order: the BSD `__.SYMDEF SORTED`
/// on a Mach-O target, the GNU `/` member elsewhere.
fn archive_index(a: &[u8]) -> Vec<(String, u32)> {
    assert_eq!(&a[..8], b"!<arch>\n");
    let u32_at = |at: usize, big: bool| {
        let b: [u8; 4] = a[at..at + 4].try_into().unwrap();
        if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    };
    let name_at = |at: usize| {
        let end = a[at..].iter().position(|&b| b == 0).unwrap();
        String::from_utf8(a[at..at + end].to_vec()).unwrap()
    };
    if &a[8..13] == b"#1/20" {
        assert_eq!(&a[68..84], b"__.SYMDEF SORTED");
        let body = 88;
        let size = u32_at(body, false) as usize;
        let strings = body + 4 + size + 4;
        (0..size)
            .step_by(8)
            .map(|i| {
                let e = body + 4 + i;
                (
                    name_at(strings + u32_at(e, false) as usize),
                    u32_at(e + 4, false),
                )
            })
            .collect()
    } else {
        assert_eq!(&a[8..10], b"/ ");
        let count = u32_at(68, true) as usize;
        let mut at = 72 + 4 * count;
        (0..count)
            .map(|i| {
                let name = name_at(at);
                at += name.len() + 1;
                (name, u32_at(72 + 4 * i, true))
            })
            .collect()
    }
}

/// Apple's Command Line Tools 27.0 write `ar`'s symbol index in a varying order, so nazm writes
/// the archive itself: the same runtime is the same bytes every time, its index sorted (Mach-O)
/// and naming the members that define each symbol.
#[test]
fn the_same_runtime_archives_to_the_same_bytes_ten_times() {
    let d = setup();
    artifact(d.path(), "r0", &[]);
    let first = std::fs::read(d.path().join("r0/libnazmrt.a")).unwrap();
    for i in 1..10 {
        let name = format!("r{i}");
        artifact(d.path(), &name, &[]);
        let again = std::fs::read(d.path().join(&name).join("libnazmrt.a")).unwrap();
        assert!(again == first, "build {i} differs from the first");
    }
    let index = archive_index(&first);
    assert!(
        index.iter().any(|(n, _)| n.ends_with("nz.chan_new")),
        "{index:?}"
    );
    if cfg!(target_os = "macos") {
        let mut sorted = index.clone();
        sorted.sort();
        assert_eq!(index, sorted, "the index is sorted by name, then member");
    }
    // Each entry names a member's header.
    for (_, off) in &index {
        let off = *off as usize;
        assert_eq!(&first[off + 58..off + 60], b"`\n", "{off}");
    }
}

/// An ELF board's, WebAssembly's and Windows' (COFF) runtime is the GNU format, its index naming
/// every defined global symbol — MinGW's, LLVM's and GNU linkers find members by it — and the
/// same bytes twice. Where MinGW-w64 is installed, a Windows program links against the archive.
#[test]
fn every_object_format_archives_with_an_index_and_the_same_bytes_twice() {
    let d = setup();
    for (target, symbol) in [
        ("aarch64-unknown-none", "nz.stack_check"),
        ("wasm32-unknown-unknown", "nz.stack_check"),
        ("x86_64-pc-windows-gnu", "nz_os_args"),
    ] {
        let one = format!("{target}-1");
        let two = format!("{target}-2");
        artifact(d.path(), &one, &["--target", target]);
        artifact(d.path(), &two, &["--target", target]);
        let a = std::fs::read(d.path().join(&one).join("libnazmrt.a")).unwrap();
        let b = std::fs::read(d.path().join(&two).join("libnazmrt.a")).unwrap();
        assert!(a == b, "{target}: two builds differ");
        let index = archive_index(&a);
        assert!(
            index.iter().any(|(n, _)| n == symbol),
            "{target}: {index:?}"
        );
    }
    const GCC: &str = "x86_64-w64-mingw32-gcc";
    let mingw = Command::new(GCC)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if crate::common::skip_without(mingw, "no MinGW-w64 gcc on this machine") {
        return;
    }
    let o = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "x86_64-pc-windows-gnu",
            "--runtime",
            "x86_64-pc-windows-gnu-1",
            "--objects",
            "wobj",
            "p.nz",
            "-o",
            "p.exe",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    let link = std::fs::read_to_string(d.path().join("wobj/link.txt")).unwrap();
    let field = |key: &str| -> Vec<String> {
        link.lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("{link}"))
            .split(' ')
            .map(str::to_owned)
            .collect()
    };
    let objects = field("objects ");
    assert!(objects.iter().any(|o| o.ends_with("libnazmrt.a")), "{link}");
    let out = Command::new(GCC)
        .current_dir(d.path().join("wobj"))
        .arg("-o")
        .arg("../p.exe")
        .args(&objects)
        .args(field("libraries "))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// A program for Intel macOS links against its target's archive and runs (under Rosetta on Apple
/// silicon), as it does with a generated runtime.
#[test]
fn an_intel_mac_program_links_against_its_archive_and_runs() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let runs = Command::new("arch")
        .args(["-x86_64", "/usr/bin/true"])
        .output()
        .is_ok_and(|o| o.status.success());
    if crate::common::skip_without(runs, "this Mac cannot run x86_64 programs") {
        return;
    }
    let d = setup();
    artifact(d.path(), "intel", &["--target", "x86_64-apple-darwin"]);
    let intel = ["--target", "x86_64-apple-darwin"];
    let with_artifact = built_and_run(
        d.path(),
        "pi",
        &[&intel[..], &["--runtime", "intel"]].concat(),
    );
    let generated = built_and_run(d.path(), "pg", &intel);
    assert_eq!(with_artifact.1, Some(0), "{}", with_artifact.0);
    assert_eq!(with_artifact, generated);
}
