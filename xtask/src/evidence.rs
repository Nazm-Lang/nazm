//! `cargo xtask evidence [--verify] DIR` — an evidence bundle's index and the compiler's bill of
//! materials (N73, `docs/internal/architecture.md` §7.75).
//!
//! A bundle is a directory someone else is meant to check: provenance records, gate logs, mutation
//! journals, measurements, artefacts. Writing adds two files to it. `sbom.json` (`nazm.sbom/1`) lists
//! every crate the compiler is built from as `Cargo.lock` names it, and the toolchain pin — read
//! offline, so it is exactly the lockfile's claim and nothing more. `index.json` (`nazm.evidence/1`)
//! lists every other file by relative path with its size, BLAKE3 digest and a kind read from its name.
//! Nothing in either depends on the time, the host or the directory's absolute path.
//!
//! Verifying recomputes every digest and refuses, by name, a file that changed, went missing or was
//! added without being indexed. Who vouches for a bundle is a signature over `index.json`, made by
//! whatever local signer the reader trusts; the index never says who it is from.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};

const INDEX: &str = "index.json";
const SBOM: &str = "sbom.json";

fn digest(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// What a file is evidence of, from its name alone, so the same bundle always indexes the same way.
fn kind(rel: &str) -> &'static str {
    let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_lowercase();
    if name == SBOM {
        "sbom"
    } else if name.contains("prov") {
        "provenance"
    } else if name.contains("mutat") || name.contains("journal") {
        "mutation"
    } else if name.contains("test") || name.contains("selfhost") || name.contains("bootstrap") {
        "tests"
    } else if name.contains("repro") || name.contains("bench") || name.contains("timing") {
        "measurement"
    } else {
        "artefact"
    }
}

/// Every file under `dir` but the index, by `/`-separated relative path, sorted.
fn files(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let here = dir.join(&rel);
        let entries =
            std::fs::read_dir(&here).map_err(|e| format!("cannot read {}: {e}", here.display()))?;
        for e in entries.flatten() {
            let r = rel.join(e.file_name());
            let t = e
                .file_type()
                .map_err(|err| format!("{}: {err}", r.display()))?;
            if t.is_dir() {
                stack.push(r);
            } else if t.is_symlink() {
                // A link's target could change under an unchanged index.
                return Err(format!(
                    "{} is a symbolic link; a bundle holds files",
                    r.display()
                ));
            } else {
                let s = r.to_string_lossy().replace('\\', "/");
                if s != INDEX {
                    out.push(s);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn sbom() -> Result<Value, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repository root")?;
    let lock =
        std::fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    let parsed: toml::Value = toml::from_str(&lock).map_err(|e| format!("Cargo.lock: {e}"))?;
    let mut crates: Vec<Value> = parsed
        .get("package")
        .and_then(toml::Value::as_array)
        .map(|ps| {
            ps.iter()
                .map(|p| {
                    let field = |k: &str| p.get(k).and_then(toml::Value::as_str).map(str::to_owned);
                    json!({
                        "name": field("name"),
                        // A crate with no source is one of this workspace's own.
                        "source": field("source").unwrap_or_else(|| "workspace".to_owned()),
                        "version": field("version"),
                        "checksum": field("checksum"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    crates.sort_by_key(|c| {
        (
            c["name"].as_str().map(str::to_owned),
            c["version"].as_str().map(str::to_owned),
        )
    });
    let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|t| {
            t.get("toolchain")?
                .get("channel")?
                .as_str()
                .map(str::to_owned)
        });
    Ok(json!({
        "schema": "nazm.sbom/1",
        "subject": "the nazm compiler and its tools, as built from this repository",
        "lockfile_digest": digest(lock.as_bytes()),
        "rust_toolchain": toolchain,
        "crates": crates,
    }))
}

fn write(dir: &Path) -> Result<usize, String> {
    let s = sbom()?;
    std::fs::write(dir.join(SBOM), format!("{s:#}\n")).map_err(|e| format!("{SBOM}: {e}"))?;
    let mut entries = Vec::new();
    for rel in files(dir)? {
        let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        entries.push(json!({ "path": rel, "kind": kind(&rel), "size": bytes.len(), "digest": digest(&bytes) }));
    }
    let n = entries.len();
    let index = json!({
        "schema": "nazm.evidence/1",
        "digest_algorithm": "blake3-256 over the file's bytes",
        "files": entries,
    });
    std::fs::write(dir.join(INDEX), format!("{index:#}\n")).map_err(|e| format!("{INDEX}: {e}"))?;
    Ok(n)
}

/// Every problem, each naming its file; empty when the bundle is what its index says.
fn verify(dir: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(dir.join(INDEX)).map_err(|e| format!("{INDEX}: {e}"))?;
    let index: Value = serde_json::from_str(&text).map_err(|e| format!("{INDEX}: {e}"))?;
    if index.get("schema").and_then(Value::as_str) != Some("nazm.evidence/1") {
        return Err(format!("{INDEX} is not nazm.evidence/1"));
    }
    let listed: Vec<(String, String)> = index
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{INDEX} lists no files"))?
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap_or_default().to_owned(),
                f["digest"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let present = files(dir)?;
    let mut problems = Vec::new();
    for (path, want) in &listed {
        match std::fs::read(dir.join(path)) {
            Ok(bytes) if &digest(&bytes) == want => {}
            Ok(_) => problems.push(format!("changed: {path}")),
            Err(_) => problems.push(format!("missing: {path}")),
        }
    }
    for p in &present {
        if !listed.iter().any(|(l, _)| l == p) {
            problems.push(format!("not indexed: {p}"));
        }
    }
    Ok(problems)
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--verify");
    let Some(dir) = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    else {
        eprintln!("evidence: usage: cargo xtask evidence [--verify] DIR");
        return ExitCode::from(2);
    };
    if !dir.is_dir() {
        eprintln!("evidence: {} is not a directory", dir.display());
        return ExitCode::from(2);
    }
    if check {
        match verify(&dir) {
            Ok(problems) if problems.is_empty() => {
                println!("evidence: {} matches {INDEX}", dir.display());
                ExitCode::SUCCESS
            }
            Ok(problems) => {
                for p in &problems {
                    eprintln!("evidence: {p}");
                }
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("evidence: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        match write(&dir) {
            Ok(n) => {
                println!("evidence: indexed {n} files in {}/{INDEX}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("evidence: {e}");
                ExitCode::FAILURE
            }
        }
    }
}
