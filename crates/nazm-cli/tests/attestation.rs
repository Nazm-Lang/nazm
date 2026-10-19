//! N94: reproducibility v2 — the model's first two claims, an SBOM, and signed statements.
//!
//! `docs/architecture.md` §7.95. Identical input gives identical artefacts, and so does the same
//! input in another directory, under either backend; the SBOM lists the build's packages, sources
//! and runtime with their digests; a statement signed with `ssh-keygen` verifies for its signer
//! and for the artefacts it names, and for nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");
const PROGRAM: &str = "fn twice(n: Int) -> Int { n * 2 }\nfn main() -> Int { twice(21) }\n";

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

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Build `p.nz` in `dir` with its provenance and SBOM.
fn build(dir: &Path, backend: &str) {
    let o = nazm(
        dir,
        &[
            "build",
            "--no-cache",
            "--backend",
            backend,
            "p.nz",
            "-o",
            "p",
            "--provenance",
            "p.prov.json",
            "--sbom",
            "p.cdx.json",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn identical_input_gives_identical_artefacts_here_and_in_another_directory() {
    for backend in ["llvm", "cranelift"] {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // A second checkout, one directory deeper: no path reaches an artefact.
        let deeper = b.path().join("checkout/elsewhere");
        std::fs::create_dir_all(&deeper).unwrap();
        for d in [a.path(), deeper.as_path()] {
            std::fs::write(d.join("p.nz"), PROGRAM).unwrap();
        }
        build(a.path(), backend);
        let exe = crate::common::exe_name("p");
        let files = [exe.as_str(), "p.prov.json", "p.cdx.json"];
        let first: Vec<Vec<u8>> = files.iter().map(|f| read(&a.path().join(f))).collect();
        // The same input again, in place.
        build(a.path(), backend);
        // And elsewhere.
        build(&deeper, backend);
        for (i, f) in files.iter().enumerate() {
            assert!(
                read(&a.path().join(f)) == first[i],
                "{backend}: {f} changed on a rebuild"
            );
            assert!(
                read(&deeper.join(f)) == first[i],
                "{backend}: {f} differs in another directory"
            );
        }
    }
}

#[test]
fn the_sbom_lists_every_package_source_and_the_runtime_with_their_digests() {
    let d = tempfile::tempdir().unwrap();
    let lib = d.path().join("shapes");
    std::fs::create_dir_all(lib.join("src")).unwrap();
    std::fs::write(
        lib.join("nazm.toml"),
        "schema = \"nazm.package/1\"\n\n[package]\nname = \"shapes\"\nversion = \"1.2.0\"\n",
    )
    .unwrap();
    std::fs::write(lib.join("src/poly.nz"), "pub fn sides() -> Int { 4 }\n").unwrap();
    let app = d.path().join("app");
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(
        app.join("nazm.toml"),
        "schema = \"nazm.package/1\"\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n\n\
         [dependencies]\nshapes = { path = \"../shapes\", version = \"1.2.0\" }\n",
    )
    .unwrap();
    std::fs::write(
        app.join("src/main.nz"),
        "use \"shapes:poly.nz\";\nfn main() -> Int { sides() }\n",
    )
    .unwrap();
    let o = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "app",
            "-o",
            "out",
            "--provenance",
            "out.prov.json",
            "--sbom",
            "out.cdx.json",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let bom: Value = serde_json::from_slice(&read(&d.path().join("out.cdx.json"))).unwrap();
    let prov: Value = serde_json::from_slice(&read(&d.path().join("out.prov.json"))).unwrap();
    assert_eq!(bom["bomFormat"], "CycloneDX");
    assert_eq!(bom["specVersion"], "1.5");
    let exe = blake3::hash(&read(&d.path().join(crate::common::exe_name("out"))))
        .to_hex()
        .to_string();
    assert_eq!(bom["metadata"]["component"]["hashes"][0]["alg"], "BLAKE3");
    assert_eq!(bom["metadata"]["component"]["hashes"][0]["content"], exe);
    let components = bom["components"].as_array().unwrap();
    let refs: Vec<&str> = components
        .iter()
        .map(|c| c["bom-ref"].as_str().unwrap())
        .collect();
    // Every package the provenance names, and only those.
    let packages: Vec<String> = prov["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            format!(
                "package:{}@{}",
                p["name"].as_str().unwrap(),
                p["version"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(packages.len(), 2, "{prov}");
    for p in &packages {
        assert!(refs.contains(&p.as_str()), "{p} missing: {refs:?}");
    }
    assert_eq!(
        refs.iter().filter(|r| r.starts_with("package:")).count(),
        packages.len()
    );
    // Every source module, and the runtime.
    for s in prov["sources"].as_array().unwrap() {
        let r = format!("source:{}", s["module"].as_str().unwrap());
        assert!(refs.contains(&r.as_str()), "{r} missing: {refs:?}");
    }
    assert!(refs.contains(&"runtime"));
    let shapes = components.iter().find(|c| c["name"] == "shapes").unwrap();
    assert_eq!(shapes["purl"], "pkg:generic/shapes@1.2.0");
    assert_eq!(shapes["hashes"][0]["alg"], "BLAKE3");
    for c in components {
        let h = c["hashes"][0]["content"].as_str().unwrap();
        assert_eq!(h.len(), 64, "{c}");
    }
    let deps = bom["dependencies"][0]["dependsOn"].as_array().unwrap();
    assert!(
        deps.iter().any(|d| d == "package:shapes@1.2.0") && deps.iter().any(|d| d == "runtime")
    );
}

// The contained Linux image has no ssh-keygen: there it is ignored, not a pass (Q1-D-02).
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "signs with the host's ssh-keygen")]
fn a_signed_statement_verifies_for_its_signer_and_its_artefacts_only() {
    if common::skip_without(
        Command::new("ssh-keygen").arg("-?").output().is_ok(),
        "no ssh-keygen on this machine",
    ) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    std::fs::write(p.join("p.nz"), PROGRAM).unwrap();
    build(p, "llvm");
    for (key, who) in [("key", "builder"), ("other", "someone")] {
        let k = Command::new("ssh-keygen")
            .current_dir(p)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", who, "-f", key])
            .output()
            .unwrap();
        assert!(k.status.success());
    }
    let public = String::from_utf8(read(&p.join("key.pub"))).unwrap();
    std::fs::write(p.join("allowed"), format!("builder@example {public}")).unwrap();
    // An artefact the record does not name is refused before any key is used.
    std::fs::write(p.join("stray"), "not the output").unwrap();
    let stray = nazm(
        p,
        &[
            "attest",
            "sign",
            "--key",
            "missing-key",
            "p.prov.json",
            "stray",
        ],
    );
    assert!(!stray.status.success());
    assert!(
        err(&stray).contains("not an output the provenance record names"),
        "{}",
        err(&stray)
    );
    let s = nazm(p, &["attest", "sign", "--key", "key", "p.prov.json", "p"]);
    assert!(s.status.success(), "{}", err(&s));
    let statement: Value =
        serde_json::from_slice(&read(&p.join("p.prov.json.intoto.json"))).unwrap();
    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v1");
    assert_eq!(statement["predicateType"], "urn:nazm:provenance:1");
    assert_eq!(statement["subject"][0]["name"], "p");
    assert_eq!(statement["predicate"]["schema"], "nazm.provenance/1");
    let verify = |id: &str| {
        nazm(
            p,
            &[
                "attest",
                "verify",
                "--allowed-signers",
                "allowed",
                "--identity",
                id,
                "p.prov.json.intoto.json",
                "p",
            ],
        )
    };
    let ok = verify("builder@example");
    assert!(ok.status.success(), "{}", err(&ok));
    // Another identity than the one the key is allowed for.
    assert!(!verify("someone@example").status.success());
    // The artefact changed after signing.
    let exe = read(&p.join(crate::common::exe_name("p")));
    let mut changed = exe.clone();
    changed.push(0);
    std::fs::write(p.join(crate::common::exe_name("p")), &changed).unwrap();
    let after = verify("builder@example");
    assert!(!after.status.success());
    assert!(
        err(&after).contains("is not a subject of the statement"),
        "{}",
        err(&after)
    );
    std::fs::write(p.join(crate::common::exe_name("p")), &exe).unwrap();
    // The statement changed after signing.
    let text = String::from_utf8(read(&p.join("p.prov.json.intoto.json"))).unwrap();
    std::fs::write(
        p.join("p.prov.json.intoto.json"),
        text.replace("\"name\": \"p\"", "\"name\": \"q\""),
    )
    .unwrap();
    let edited = verify("builder@example");
    assert!(!edited.status.success());
    assert!(err(&edited).contains("does not verify"), "{}", err(&edited));
    std::fs::write(p.join("p.prov.json.intoto.json"), text).unwrap();
    // A statement signed by a key nobody allowed.
    let o = nazm(p, &["attest", "sign", "--key", "other", "p.prov.json", "p"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(!verify("builder@example").status.success());
}
