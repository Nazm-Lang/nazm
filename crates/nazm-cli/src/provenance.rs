//! `nazm build --provenance FILE` and `--attest-with CMD` (N73, `docs/architecture.md` §7.75).
//!
//! Written after a build has succeeded, never instead of one: `nazm.provenance/1` names every
//! source module and package with its digest, the target, the compiler's version, the C compiler's
//! identity, the runtime's ABI revision and digest, the backend and its options, the profiles in
//! force and the output's digest. Every digest is plain BLAKE3 over the file's bytes — `blake3:` and
//! 64 hex digits, as a package's — so `b3sum` checks it without Nazm. No absolute path, time or host
//! name enters it: two builds of one program with one toolchain write the same file.
//!
//! **Evidence, not a guarantee.** The record says what this compiler read and wrote; it is not a
//! proof that the output does what the source says, and a C compiler the record names is identified
//! by what it reports about itself. An attestation is whatever the local command printed: Nazm runs
//! it and keeps its output beside the record, and neither signs nor checks anything itself.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::{json, Value};

use crate::exit;

/// What a build was asked to do, as far as it reaches its output.
pub(crate) struct Request<'a> {
    pub source: &'a Path,
    pub out: Option<&'a Path>,
    pub opt_level: u8,
    pub backend: &'a str,
    pub target: Option<&'a str>,
    pub layout: &'a str,
    pub debug: bool,
    pub library: bool,
    pub objects: Option<&'a Path>,
    pub stack_watermark: bool,
    pub link: &'a [PathBuf],
    pub profile: Option<&'a str>,
    /// The runtime artifact linked instead of a generated runtime (N82), if one was.
    pub runtime: Option<&'a Path>,
}

/// The runtime the build linked: its ABI revision and digest, and — when an artifact was linked
/// instead of a generated runtime (N82) — that archive's own digest.
fn runtime_record(artifact: Option<&Path>) -> Value {
    let mut r = json!({
        "abi_revision": nazm_lir::runtime::RUNTIME_ABI_REVISION,
        "digest": format!("blake3:{}", nazm_lir::runtime::digest()),
    });
    if let (Some(dir), Some(map)) = (artifact, r.as_object_mut()) {
        map.insert(
            "archive".into(),
            file_digest(&dir.join(crate::runtime_artifact::ARCHIVE)),
        );
    }
    r
}

fn digest(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

fn file_digest(path: &Path) -> Value {
    std::fs::read(path).map_or(Value::Null, |b| json!(digest(&b)))
}

/// The output: an executable or an archive is one file; `--objects DIR` is every file in it.
fn outputs(r: &Request<'_>) -> Value {
    if let Some(dir) = r.objects {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|e| {
                e.flatten()
                    .map(|f| f.path())
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let each: Vec<Value> = files
            .iter()
            .map(|f| {
                json!({
                    "name": f.file_name().map(|n| n.to_string_lossy().into_owned()),
                    "digest": file_digest(f),
                })
            })
            .collect();
        return json!({ "kind": "objects", "files": each });
    }
    let path = output_path(r);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let kind = if r.library { "library" } else { "executable" };
    let mut files = vec![json!({ "name": name, "digest": file_digest(&path) })];
    // A library's header is part of what it publishes.
    if r.library {
        let header = path.with_extension("h");
        files.push(json!({
            "name": header.file_name().map(|n| n.to_string_lossy().into_owned()),
            "digest": file_digest(&header),
        }));
    }
    json!({ "kind": kind, "files": files })
}

/// Where `nazm build` wrote its output: `-o`, or the source's stem.
fn output_path(r: &Request<'_>) -> PathBuf {
    r.out.map_or_else(
        || PathBuf::from(r.source.file_stem().unwrap_or_default()),
        Path::to_path_buf,
    )
}

/// The C compiler, by the first line of what it says it is. `null` when it cannot be asked: a
/// Cranelift build still links with it, and the record says so rather than guessing.
fn c_compiler() -> Value {
    let report = Command::new(crate::backend::DRIVER)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    report.map_or(Value::Null, |text| {
        json!({
            "driver": crate::backend::DRIVER,
            "version": text.lines().next().unwrap_or("").trim(),
            "report_digest": digest(text.as_bytes()),
        })
    })
}

/// The record, as one deterministic JSON value.
fn record(r: &Request<'_>) -> Result<Value, String> {
    let source_root = crate::root::decide(r.source, None).map_err(|e| e.to_string())?;
    let loaded = crate::load::load(r.source, source_root.path())
        .map_err(|_| "the sources could not be read again".to_owned())?;
    let (map, graph) = (&loaded.map, &loaded.graph);
    let mut modules: Vec<Value> = map
        .ids()
        .map(|id| {
            let key = graph
                .module_of(id)
                .and_then(|m| graph.key(m))
                .map(|k| k.as_str().to_owned());
            json!({
                "module": key.unwrap_or_else(|| map.name(id).to_owned()),
                "digest": digest(map.text(id).as_bytes()),
            })
        })
        .collect();
    modules.sort_by(|a, b| a["module"].as_str().cmp(&b["module"].as_str()));
    let graph_of = crate::package::graph_for(r.source).and_then(Result::ok);
    let packages: Vec<Value> = graph_of.as_ref().map_or_else(Vec::new, |g| {
        g.packages
            .iter()
            .map(
                |p| json!({ "name": p.name, "version": p.version.to_string(), "digest": p.digest }),
            )
            .collect()
    });
    let mut profiles: Vec<String> = graph_of.as_ref().map_or_else(Vec::new, |g| {
        g.packages
            .iter()
            .filter_map(|p| p.profile.clone())
            .collect()
    });
    profiles.extend(r.profile.map(str::to_owned));
    profiles.sort();
    profiles.dedup();
    let target = match r.target {
        Some(t) => t.to_owned(),
        None => nazm_lir::backend::Target::host()
            .map(|t| t.triple)
            .map_err(|e| e.to_string())?,
    };
    let link: Vec<Value> = r
        .link
        .iter()
        .map(|f| {
            json!({
                "name": f.file_name().map(|n| n.to_string_lossy().into_owned()),
                "digest": file_digest(f),
            })
        })
        .collect();
    Ok(json!({
        "schema": "nazm.provenance/1",
        "digest_algorithm": "blake3-256 over the file's bytes",
        "sources": modules,
        "packages": packages,
        "target": target,
        "compiler": { "name": "nazm", "version": env!("CARGO_PKG_VERSION") },
        "semantic_epoch": nazm_cache::SEMANTIC_EPOCH,
        "c_compiler": c_compiler(),
        "runtime": runtime_record(r.runtime),
        "backend": {
            "name": r.backend,
            "opt_level": r.opt_level,
            "layout": r.layout,
            "debug": r.debug,
            "stack_watermark": r.stack_watermark,
            "cranelift": nazm_codegen_clif::CRANELIFT_VERSION,
        },
        "linked": link,
        "profiles": profiles,
        "output": outputs(r),
    }))
}

/// A `blake3:HEX` digest's hex, for a format that names the algorithm apart.
fn hex(digest: &Value) -> Option<&str> {
    digest.as_str()?.strip_prefix("blake3:")
}

/// The build as a CycloneDX 1.5 SBOM (N94, §7.95), read off the provenance record so the two cannot
/// describe different builds: the output as the metadata component, every package, every source
/// module as a file, the runtime, and the tools. Deterministic — no serial number or timestamp,
/// both of which CycloneDX makes optional.
// serde_json's `Index` on a `Value` never panics: a missing key or a non-object reads as `Null`.
#[allow(clippy::indexing_slicing)]
pub(crate) fn sbom(record: &Value) -> Value {
    let hashes = |d: &Value| -> Value {
        hex(d).map_or_else(|| json!([]), |h| json!([{ "alg": "BLAKE3", "content": h }]))
    };
    let files = record["output"]["files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let main = files.first().cloned().unwrap_or(Value::Null);
    let mut components: Vec<Value> = Vec::new();
    for p in record["packages"].as_array().into_iter().flatten() {
        let (name, version) = (
            p["name"].as_str().unwrap_or(""),
            p["version"].as_str().unwrap_or(""),
        );
        components.push(json!({
            "type": "library",
            "bom-ref": format!("package:{name}@{version}"),
            "name": name,
            "version": version,
            "purl": format!("pkg:generic/{name}@{version}"),
            "hashes": hashes(&p["digest"]),
        }));
    }
    for m in record["sources"].as_array().into_iter().flatten() {
        let name = m["module"].as_str().unwrap_or("");
        components.push(json!({
            "type": "file",
            "bom-ref": format!("source:{name}"),
            "name": name,
            "hashes": hashes(&m["digest"]),
        }));
    }
    components.push(json!({
        "type": "library",
        "bom-ref": "runtime",
        "name": "nazm-runtime",
        "version": format!("abi-{}", record["runtime"]["abi_revision"]),
        "hashes": hashes(&record["runtime"]["digest"]),
    }));
    for f in files.iter().skip(1) {
        components.push(json!({
            "type": "file",
            "bom-ref": format!("output:{}", f["name"].as_str().unwrap_or("")),
            "name": f["name"],
            "hashes": hashes(&f["digest"]),
        }));
    }
    let mut tools = vec![
        json!({ "type": "application", "name": "nazm", "version": record["compiler"]["version"] }),
        json!({ "type": "library", "name": "cranelift", "version": record["backend"]["cranelift"] }),
    ];
    if let Some(v) = record["c_compiler"]["version"].as_str() {
        tools.push(
            json!({ "type": "application", "name": record["c_compiler"]["driver"], "version": v }),
        );
    }
    json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "tools": { "components": tools },
            "component": {
                "type": if record["output"]["kind"] == "executable" { "application" } else { "library" },
                "bom-ref": "output",
                "name": main["name"],
                "hashes": hashes(&main["digest"]),
                "properties": [
                    { "name": "nazm:target", "value": record["target"] },
                    { "name": "nazm:backend", "value": record["backend"]["name"] },
                    { "name": "nazm:semantic_epoch", "value": record["semantic_epoch"].to_string() },
                ],
            },
        },
        "components": components,
        "dependencies": [{
            "ref": "output",
            "dependsOn": record["packages"].as_array().into_iter().flatten()
                .map(|p| format!("package:{}@{}", p["name"].as_str().unwrap_or(""), p["version"].as_str().unwrap_or("")))
                .chain(std::iter::once("runtime".to_owned()))
                .collect::<Vec<_>>(),
        }],
    })
}

/// Write the record, the SBOM and the attestation, as asked. Runs only after the build has
/// succeeded; a failure here fails the command, and the output stays where the build put it.
pub(crate) fn write(
    r: &Request<'_>,
    file: Option<&Path>,
    sbom_file: Option<&Path>,
    attest_with: Option<&str>,
) -> ExitCode {
    let doc = match record(r) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nazm: no provenance: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    if let Some(path) = sbom_file {
        if let Err(e) = std::fs::write(path, format!("{:#}\n", sbom(&doc))) {
            eprintln!("nazm: cannot write {}: {e}", path.display());
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    let Some(file) = file else {
        return ExitCode::from(exit::OK);
    };
    if let Err(e) = std::fs::write(file, format!("{doc:#}\n")) {
        eprintln!("nazm: cannot write {}: {e}", file.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let Some(cmd) = attest_with else {
        return ExitCode::from(exit::OK);
    };
    // No shell: the words of CMD, then the record's path. What the command does with it is its own.
    let mut words = cmd.split_whitespace();
    let Some(program) = words.next() else {
        eprintln!("nazm: --attest-with names no command");
        return ExitCode::from(exit::USAGE);
    };
    let ran = Command::new(program).args(words).arg(file).output();
    let sig = PathBuf::from(format!("{}.sig", file.display()));
    match ran {
        Ok(o) if o.status.success() => {
            if let Err(e) = std::fs::write(&sig, &o.stdout) {
                eprintln!("nazm: cannot write {}: {e}", sig.display());
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
            ExitCode::from(exit::OK)
        }
        Ok(o) => {
            eprintln!(
                "nazm: the attesting command `{cmd}` failed ({}); the provenance is written, no attestation is:\n{}",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim_end()
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Err(e) => {
            eprintln!("nazm: cannot run the attesting command `{program}`: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}
