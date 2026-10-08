//! `nazm attest sign` and `nazm attest verify` (N94, `docs/architecture.md` §7.95).
//!
//! A provenance record becomes an in-toto Statement v1 — one subject per artefact, with its BLAKE3
//! digest, the record as the predicate — signed with OpenSSH's `ssh-keygen -Y sign`, the SSHSIG
//! format, in the namespace [`NAMESPACE`]. Verification is the reverse and refuses on the first
//! thing that does not hold: the signature for the named identity, the statement's shape, and every
//! artefact's digest among the subjects. Keys and trust are the caller's; Nazm holds none.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde_json::{json, Value};

use crate::exit;

/// The SSHSIG namespace: a signature made for anything else does not verify as an attestation.
pub(crate) const NAMESPACE: &str = "nazm-attestation";
const STATEMENT: &str = "https://in-toto.io/Statement/v1";
const PREDICATE: &str = "urn:nazm:provenance:1";

fn blake3_of(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|b| blake3::hash(&b).to_hex().to_string())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A refusal with its v1 status (Q1-C1): a file that cannot be read is 3, a statement or
/// signature that does not hold is 1, `ssh-keygen` that cannot run or sign, or an output that
/// cannot be written, is 4, and a command line naming no artefact is 5.
fn refuse(code: u8, what: &str) -> ExitCode {
    eprintln!("nazm: {what}");
    ExitCode::from(code)
}

/// An artefact's or record's problem: unreadable (3), or not what it should be (1).
fn read_or_invalid(e: &str) -> u8 {
    if e.starts_with("cannot read") {
        exit::NO_SUCH_FILE
    } else {
        exit::CHECK_FAILED
    }
}

/// The statement for `artefacts`, each checked against the digests `record` names.
// serde_json's `Index` on a `Value` never panics: a missing key or a non-object reads as `Null`.
#[allow(clippy::indexing_slicing)]
fn statement(record: &Value, artefacts: &[PathBuf]) -> Result<Value, String> {
    if record["schema"] != "nazm.provenance/1" {
        return Err("the record is not `nazm.provenance/1`".to_owned());
    }
    let named: Vec<&str> = record["output"]["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["digest"].as_str()?.strip_prefix("blake3:"))
        .collect();
    let mut subjects = Vec::new();
    for a in artefacts {
        let d = blake3_of(a)?;
        if !named.contains(&d.as_str()) {
            return Err(format!(
                "{} is not an output the provenance record names (blake3:{d})",
                a.display()
            ));
        }
        subjects.push(json!({ "name": name_of(a), "digest": { "blake3": d } }));
    }
    if subjects.is_empty() {
        return Err("no artefact to attest".to_owned());
    }
    Ok(json!({
        "_type": STATEMENT,
        "subject": subjects,
        "predicateType": PREDICATE,
        "predicate": record,
    }))
}

/// `nazm attest sign --key KEY [--out FILE] PROVENANCE ARTEFACT…`: write the statement, then
/// `FILE.sig` beside it.
pub(crate) fn sign(
    key: &Path,
    out: Option<&Path>,
    provenance: &Path,
    artefacts: &[PathBuf],
) -> ExitCode {
    let record: Value = match std::fs::read_to_string(provenance)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => {
            return refuse(
                exit::NO_SUCH_FILE,
                &format!("cannot read {}: {e}", provenance.display()),
            )
        }
    };
    // Every artefact is checked before any key is used.
    let doc = match statement(&record, artefacts) {
        Ok(d) => d,
        Err(e) => return refuse(read_or_invalid(&e), &e),
    };
    let file = out.map_or_else(
        || PathBuf::from(format!("{}.intoto.json", provenance.display())),
        Path::to_path_buf,
    );
    if let Err(e) = std::fs::write(&file, format!("{doc:#}\n")) {
        return refuse(
            exit::TOOLCHAIN_FAILED,
            &format!("cannot write {}: {e}", file.display()),
        );
    }
    let sig = PathBuf::from(format!("{}.sig", file.display()));
    let _ = std::fs::remove_file(&sig);
    let ran = Command::new("ssh-keygen")
        .args(["-q", "-Y", "sign", "-n", NAMESPACE, "-f"])
        .arg(key)
        .arg(&file)
        .stdin(Stdio::null())
        .output();
    match ran {
        Ok(o) if o.status.success() && sig.is_file() => {
            eprintln!("nazm: signed {} ({})", file.display(), sig.display());
            ExitCode::from(exit::OK)
        }
        Ok(o) => refuse(
            exit::TOOLCHAIN_FAILED,
            &format!(
                "ssh-keygen could not sign: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        ),
        Err(e) => refuse(
            exit::TOOLCHAIN_FAILED,
            &format!("cannot run ssh-keygen: {e}"),
        ),
    }
}

/// `nazm attest verify --allowed-signers FILE --identity ID STATEMENT ARTEFACT…`.
// serde_json's `Index` on a `Value` never panics: a missing key or a non-object reads as `Null`.
#[allow(clippy::indexing_slicing)]
pub(crate) fn verify(
    allowed: &Path,
    identity: &str,
    statement_file: &Path,
    artefacts: &[PathBuf],
) -> ExitCode {
    let sig = PathBuf::from(format!("{}.sig", statement_file.display()));
    let Ok(text) = std::fs::read(statement_file) else {
        return refuse(
            exit::NO_SUCH_FILE,
            &format!("cannot read {}", statement_file.display()),
        );
    };
    let child = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-n", NAMESPACE, "-I", identity, "-f"])
        .arg(allowed)
        .arg("-s")
        .arg(&sig)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let verified = child.and_then(|mut c| {
        if let Some(mut input) = c.stdin.take() {
            use std::io::Write as _;
            input.write_all(&text)?;
        }
        c.wait_with_output()
    });
    match verified {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            return refuse(
                exit::CHECK_FAILED,
                &format!(
                    "the signature does not verify for `{identity}`: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
            )
        }
        Err(e) => {
            return refuse(
                exit::TOOLCHAIN_FAILED,
                &format!("cannot run ssh-keygen: {e}"),
            )
        }
    }
    let Ok(doc) = serde_json::from_slice::<Value>(&text) else {
        return refuse(exit::CHECK_FAILED, "the statement is not JSON");
    };
    if doc["_type"] != STATEMENT || doc["predicateType"] != PREDICATE {
        return refuse(
            exit::CHECK_FAILED,
            "the statement is not an in-toto Statement v1 of a Nazm provenance record",
        );
    }
    let subjects: Vec<&str> = doc["subject"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["digest"]["blake3"].as_str())
        .collect();
    if artefacts.is_empty() {
        return refuse(exit::USAGE, "no artefact to verify");
    }
    for a in artefacts {
        match blake3_of(a) {
            Ok(d) if subjects.contains(&d.as_str()) => {}
            Ok(d) => {
                return refuse(
                    exit::CHECK_FAILED,
                    &format!(
                        "{} (blake3:{d}) is not a subject of the statement",
                        a.display()
                    ),
                )
            }
            Err(e) => return refuse(read_or_invalid(&e), &e),
        }
    }
    println!(
        "verified: {} artefact{} signed by `{identity}`",
        artefacts.len(),
        if artefacts.len() == 1 { "" } else { "s" }
    );
    ExitCode::from(exit::OK)
}
