//! N73: build provenance — `nazm build --provenance FILE` and `--attest-with CMD`. A record of
//! what a build read and wrote; `provenance.rs` is N38's data-flow provenance, another subject.
//!
//! `docs/architecture.md` §7.75. What the commands show: the record names every source module and
//! the output with digests anyone can recompute (plain BLAKE3 over the bytes), the toolchain and the
//! options that reach the output; two builds of one program in two clean directories write the same
//! record; a changed source changes its own digest and the output's; an attestation is the local
//! command's standard output, kept beside the record, and a failing one fails the command without
//! leaving a signature behind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROGRAM: &str = "fn twice(x: Int) -> Int { x * 2 }\nfn main() -> Int { twice(21) }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    let mut c = Command::new(NAZM);
    c.current_dir(dir).args(args);
    // Nazm's own code generator (R9, D15), built once per test process, where it is asked for.
    if args.windows(2).any(|w| w == ["--backend", "nazm"]) {
        c.env(nazm_codegen_nazm::VARIABLE, crate::differential::nzcg());
    }
    c.output().unwrap()
}

fn build(dir: &Path, src: &str, extra: &[&str]) -> serde_json::Value {
    std::fs::write(dir.join("p.nz"), src).unwrap();
    let mut args = vec!["build", "p.nz", "-o", "p", "--provenance", "p.json"];
    args.extend_from_slice(extra);
    let o = nazm(dir, &args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_str(&std::fs::read_to_string(dir.join("p.json")).unwrap()).unwrap()
}

fn b3(path: &Path) -> String {
    format!(
        "blake3:{}",
        blake3::hash(&std::fs::read(path).unwrap()).to_hex()
    )
}

fn source<'a>(v: &'a serde_json::Value, module: &str) -> &'a serde_json::Value {
    v["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["module"] == module)
        .unwrap()
}

#[test]
fn the_record_names_what_reached_the_output_with_digests_anyone_can_recompute() {
    let d = tempfile::tempdir().unwrap();
    let v = build(
        d.path(),
        PROGRAM,
        &["--opt-level", "2", "--profile", "general"],
    );
    assert_eq!(v["schema"], "nazm.provenance/1");
    assert_eq!(source(&v, "p.nz")["digest"], b3(&d.path().join("p.nz")));
    // The prelude every module sees is a source of the build like any other.
    assert!(source(&v, "@core/prelude")["digest"]
        .as_str()
        .unwrap()
        .starts_with("blake3:"));
    assert_eq!(v["output"]["kind"], "executable");
    assert_eq!(
        v["output"]["files"][0]["digest"],
        b3(&d.path().join(crate::common::exe_name("p")))
    );
    assert_eq!(v["backend"]["name"], "llvm");
    assert_eq!(v["backend"]["opt_level"], 2);
    assert_eq!(v["profiles"], serde_json::json!(["general"]));
    assert!(v["runtime"]["abi_revision"].as_u64().unwrap() > 0);
    assert!(v["runtime"]["digest"]
        .as_str()
        .unwrap()
        .starts_with("blake3:"));
    assert!(v["c_compiler"]["version"]
        .as_str()
        .unwrap()
        .contains("clang"));
    // Nothing of this machine: no absolute path anywhere in the record.
    let text = std::fs::read_to_string(d.path().join("p.json")).unwrap();
    assert!(!text.contains(d.path().to_str().unwrap()), "{text}");
}

#[test]
fn two_clean_builds_write_the_same_record_and_a_change_is_seen() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let x = build(a.path(), PROGRAM, &[]);
    let y = build(b.path(), PROGRAM, &[]);
    assert_eq!(
        x, y,
        "one program, one toolchain, two directories: one record"
    );
    let c = tempfile::tempdir().unwrap();
    let z = build(c.path(), &PROGRAM.replace("21", "20"), &[]);
    assert_ne!(source(&z, "p.nz")["digest"], source(&x, "p.nz")["digest"]);
    assert_ne!(
        z["output"]["files"][0]["digest"],
        x["output"]["files"][0]["digest"]
    );
    assert_eq!(source(&z, "@core/prelude"), source(&x, "@core/prelude"));
}

#[test]
fn the_backend_that_built_it_is_the_one_recorded() {
    let d = tempfile::tempdir().unwrap();
    let v = build(d.path(), PROGRAM, &["--backend", "cranelift"]);
    assert_eq!(v["backend"]["name"], "cranelift");
    assert_eq!(
        v["output"]["files"][0]["digest"],
        b3(&d.path().join(crate::common::exe_name("p")))
    );
    // Only Nazm's own code generator is named by its digest, and only when it built the program.
    assert!(v["backend"].get("nzcg").is_none(), "{v}");
}

/// R9, D15: a program `--backend nazm` built names the generator that compiled it by its digest —
/// the file `NAZM_NZCG` named, as anyone can recompute — and the SBOM lists it among the tools.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn a_program_nazms_own_generator_built_names_it_by_its_digest() {
    let d = tempfile::tempdir().unwrap();
    let v = build(
        d.path(),
        PROGRAM,
        &["--backend", "nazm", "--sbom", "p.cdx.json"],
    );
    assert_eq!(v["backend"]["name"], "nazm");
    assert_eq!(v["backend"]["nzcg"], b3(crate::differential::nzcg()), "{v}");
    assert_eq!(
        v["output"]["files"][0]["digest"],
        b3(&d.path().join(crate::common::exe_name("p")))
    );
    let sbom: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("p.cdx.json")).unwrap())
            .unwrap();
    let tools = sbom["metadata"]["tools"]["components"].as_array().unwrap();
    let nzcg = tools.iter().find(|t| t["name"] == "nzcg").expect("nzcg");
    assert_eq!(
        format!("blake3:{}", nzcg["hashes"][0]["content"].as_str().unwrap()),
        v["backend"]["nzcg"]
    );
    // The LLVM build of the same program names none.
    let e = tempfile::tempdir().unwrap();
    let w = build(e.path(), PROGRAM, &[]);
    assert!(w["backend"].get("nzcg").is_none(), "{w}");
}

#[test]
fn an_attestation_is_the_local_commands_output_and_a_failing_one_leaves_none() {
    // Windows has no `cat` or `false`: `cmd /c type` and `cmd /c exit 1` stand in for them.
    let (cat, fails) = if cfg!(windows) {
        ("cmd /c type", "cmd /c exit 1")
    } else {
        ("cat", "false")
    };
    let d = tempfile::tempdir().unwrap();
    // `cat` stands in for a signer: its output is the record itself, so the test knows it exactly.
    build(d.path(), PROGRAM, &["--attest-with", cat]);
    assert_eq!(
        std::fs::read(d.path().join("p.json.sig")).unwrap(),
        std::fs::read(d.path().join("p.json")).unwrap()
    );
    let e = tempfile::tempdir().unwrap();
    std::fs::write(e.path().join("p.nz"), PROGRAM).unwrap();
    let o = nazm(
        e.path(),
        &[
            "build",
            "p.nz",
            "-o",
            "p",
            "--provenance",
            "p.json",
            "--attest-with",
            fails,
        ],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr)
        .contains(&format!("the attesting command `{fails}` failed")));
    assert!(e.path().join("p.json").exists() && !e.path().join("p.json.sig").exists());
    // An attestation needs a record to attest.
    let o = nazm(
        e.path(),
        &["build", "p.nz", "-o", "p", "--attest-with", cat],
    );
    assert!(!o.status.success());
}

#[test]
fn a_failed_build_writes_no_record() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), "fn main() -> Int { true }\n").unwrap();
    let o = nazm(
        d.path(),
        &["build", "p.nz", "-o", "p", "--provenance", "p.json"],
    );
    assert!(!o.status.success());
    assert!(!d.path().join("p.json").exists());
}
