//! Gate 3: the cybersecurity reference workload (`docs/systems-domains.md` Part K; `docs/spec.md`,
//! *Domain profiles*).
//!
//! `tests/domains/cyber.nz` is a TCP service held to the `cyber` profile as a locked package. These
//! tests serve real clients over the loopback; show the profile refusing what it is there to refuse;
//! build the service twice, in two directories, and get the same executable and the same record;
//! and sign that record and verify it. No sandboxing, no non-interference and no certification is
//! claimed: the profile is compiler-enforced evidence about its rules and nothing more.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn service() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/domains/cyber.nz"))
        .unwrap()
}

/// `src` as the package `svc` in `dir`, locked.
fn package_in(dir: &Path, src: &str) {
    let pkg = dir.join("svc");
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(
        pkg.join("nazm.toml"),
        "schema = \"nazm.package/1\"\n[package]\nname = \"svc\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n",
    )
    .unwrap();
    std::fs::write(pkg.join("src/main.nz"), src).unwrap();
    let r = nazm(dir, &["lock", "svc"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// The service built under `cyber` in `dir`, with its provenance record and SBOM.
fn build_in(dir: &Path) {
    let r = nazm(
        dir,
        &[
            "build",
            "--no-cache",
            "--locked",
            "--profile",
            "cyber",
            "svc",
            "-o",
            "svc.bin",
            "--provenance",
            "svc.prov.json",
            "--sbom",
            "svc.cdx.json",
        ],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// One client's request and the answer it gets.
fn ask(addr: &str, request: &[u8]) -> Vec<u8> {
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(request).unwrap();
    let mut answer = Vec::new();
    c.read_to_end(&mut answer).unwrap();
    answer
}

#[test]
fn the_secure_service_answers_its_clients_and_shows_none_of_what_they_sent() {
    let d = tempfile::tempdir().unwrap();
    package_in(d.path(), &service());
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "cyber", "svc"],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    build_in(d.path());
    let mut child = Command::new(d.path().join("svc.bin"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    let addr = first
        .trim()
        .strip_prefix("listening ")
        .unwrap_or_else(|| panic!("{first}"))
        .to_owned();
    assert!(addr.starts_with("127.0.0.1:"), "{addr}");
    assert_eq!(ask(&addr, b"NAZMS"), b"SMZAN");
    // Anything but five capital letters is denied, and never echoed.
    assert_eq!(ask(&addr, b"hello"), b"DENY!");
    assert_eq!(ask(&addr, b"A\0B;C"), b"DENY!");
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert_eq!(rest, "served 3\n0\n");
}

#[test]
fn cyber_refuses_what_the_service_is_written_without() {
    let src = service();
    // Each variant locked as it is, then checked: what it refuses is the variant's alone.
    let refused = |src: &str, rule: &str| {
        let d = tempfile::tempdir().unwrap();
        package_in(d.path(), src);
        let r = nazm(d.path(), &["check", "--profile", "cyber", "svc"]);
        assert_eq!(r.code, Some(1), "{rule}: {r:?}");
        assert!(r.stderr.contains("N0510"), "{rule}: {}", r.stderr);
        assert!(
            r.stderr.contains(&format!("`{rule}`")),
            "{rule}: {}",
            r.stderr
        );
    };
    // A source changed after it was locked: the lockfile no longer verifies.
    let d = tempfile::tempdir().unwrap();
    package_in(d.path(), &src);
    std::fs::write(
        d.path().join("svc/src/main.nz"),
        src.replace("\"DENY!\"", "\"NOPE!\""),
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "cyber", "svc"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("`locked-build`"), "{}", r.stderr);
    // A request shown on standard output, where `serve` holds no authority to write: the
    // language's own rule, before any profile's (N0369).
    let shown = src.replace(
        "                        tcp_write(c, answer(req));",
        "                        tcp_write(c, answer(req));\n                        \
         print(bytes_to_str(req));",
    );
    let d = tempfile::tempdir().unwrap();
    package_in(d.path(), &shown);
    let r = nazm(d.path(), &["check", "svc"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0369"), "{}", r.stderr);
    // Handed the authority, it may write — and `cyber` refuses it: what a client sent is a
    // handle's contents, which stay off standard output.
    refused(
        &shown
            .replace(
                "fn serve(l: TcpListener, n: Int) -> Int ! { io } {",
                "fn serve(l: TcpListener, n: Int, out: OutCap) -> Int ! { io } {",
            )
            .replace("serve(l, clients);", "serve(l, clients, out);"),
        "contents-stay-in-files",
    );
    // A function that does not say what it may do.
    refused(
        &src.replace(
            "fn valid(b: Bytes) -> Bool ! {} {",
            "fn valid(b: Bytes) -> Bool {",
        ),
        "declared-effects",
    );
    // A call into C, which `main` makes.
    refused(
        &format!(
            "extern \"C\" fn c_check(a: Int) -> Int = \"c_check\";\n\
             fn checked(f: ForeignCap) -> Int ! {{ foreign }} {{ c_check(1) }}\n{}",
            src.replace(
                "fn main(net: NetCap, out: OutCap) -> Int ! { io } {",
                "fn main(net: NetCap, out: OutCap, f: ForeignCap) -> Int ! { io, foreign } {\n    \
                 checked(f);"
            )
        ),
        "no-foreign",
    );
    // A call through a function value, whose callee nothing here knows.
    refused(
        &src.replace(
            "                        tcp_write(c, answer(req));",
            "                        let reply = answer;\n                        \
             tcp_write(c, reply(req));",
        ),
        "no-unknown-calls",
    );
}

#[test]
fn the_services_build_is_reproduced_in_another_directory() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for d in [&a, &b] {
        package_in(d.path(), &service());
        build_in(d.path());
    }
    let read = |d: &tempfile::TempDir, f: &str| std::fs::read(d.path().join(f)).unwrap();
    assert_eq!(read(&a, "svc.bin"), read(&b, "svc.bin"), "the executable");
    assert_eq!(
        read(&a, "svc.prov.json"),
        read(&b, "svc.prov.json"),
        "the provenance record"
    );
    assert_eq!(
        read(&a, "svc.cdx.json"),
        read(&b, "svc.cdx.json"),
        "the SBOM"
    );
    let prov: serde_json::Value = serde_json::from_slice(&read(&a, "svc.prov.json")).unwrap();
    assert!(prov.to_string().contains("cyber"), "{prov}");
}

// The contained Linux image has no ssh-keygen: there it is ignored, not a pass (Q1-D-02).
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "signs with the host's ssh-keygen")]
fn the_services_record_is_signed_and_verifies_for_it_alone() {
    if common::skip_without(
        Command::new("ssh-keygen").arg("-?").output().is_ok(),
        "no ssh-keygen on this machine",
    ) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    package_in(p, &service());
    build_in(p);
    let k = Command::new("ssh-keygen")
        .current_dir(p)
        .args([
            "-q", "-t", "ed25519", "-N", "", "-C", "builder", "-f", "key",
        ])
        .output()
        .unwrap();
    assert!(k.status.success());
    let public = std::fs::read_to_string(p.join("key.pub")).unwrap();
    std::fs::write(p.join("allowed"), format!("builder@example {public}")).unwrap();
    let s = nazm(
        p,
        &["attest", "sign", "--key", "key", "svc.prov.json", "svc.bin"],
    );
    assert_eq!(s.code, Some(0), "{}", s.stderr);
    let verify = || {
        nazm(
            p,
            &[
                "attest",
                "verify",
                "--allowed-signers",
                "allowed",
                "--identity",
                "builder@example",
                "svc.prov.json.intoto.json",
                "svc.bin",
            ],
        )
    };
    let ok = verify();
    assert_eq!(ok.code, Some(0), "{}", ok.stderr);
    // The service changed after it was signed for.
    let mut exe = std::fs::read(p.join("svc.bin")).unwrap();
    exe.push(0);
    std::fs::write(p.join("svc.bin"), &exe).unwrap();
    let after = verify();
    assert_ne!(after.code, Some(0));
    assert!(
        after.stderr.contains("is not a subject of the statement"),
        "{}",
        after.stderr
    );
}
