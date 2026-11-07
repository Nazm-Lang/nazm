//! The **seed freeze** gate (R8 slice 19): the Rust implementation that builds the seed `nazm`
//! changes only by a recorded exemption.
//!
//! The seed is the Rust `nazm` that builds the first Nazm-built stage (`docs/internal/bootstrap.md`,
//! *The chain after the cut-over*). Its paths are the crates `nazm-cli` needs to build (the closure of
//! its normal and build dependencies under `crates/`, read from the manifests, not listed by hand), plus
//! `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and `.cargo/config.toml`. `xtask/`, `fuzz/` and the
//! crates outside that closure are not the seed: they are the harness and tests *of* it.
//!
//! A pure read of the tree. `docs/internal/seed-freeze.txt` records `base DIGEST` (the digest of the
//! seed paths when frozen) and one `exempt DATE | PATH | REASON | DIGEST` line per accepted change,
//! each carrying the digest it produced. The gate recomputes the digest and requires it to equal the
//! last line's. A seed change with no exemption line, or an exemption line with no seed change, fails.
//! Only the copyright holder edits the record; that is a review rule, not something this gate can see.

use std::path::{Path, PathBuf};

pub(crate) const RECORD: &str = "docs/internal/seed-freeze.txt";
const ROOT_CRATE: &str = "nazm-cli";
const FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".cargo/config.toml",
];

/// The crates `ROOT_CRATE` builds with, by manifest, in name order.
fn closure(root: &Path) -> Result<Vec<String>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut todo = vec![ROOT_CRATE.to_owned()];
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let manifest = root.join("crates").join(&name).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;
        let doc: toml::Table = text
            .parse()
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        let mut tables = Vec::new();
        for key in ["dependencies", "build-dependencies"] {
            tables.extend(doc.get(key).and_then(|v| v.as_table()));
        }
        if let Some(targets) = doc.get("target").and_then(|v| v.as_table()) {
            for t in targets.values().filter_map(|v| v.as_table()) {
                for key in ["dependencies", "build-dependencies"] {
                    tables.extend(t.get(key).and_then(|v| v.as_table()));
                }
            }
        }
        for t in tables {
            for dep in t.keys() {
                if root.join("crates").join(dep).join("Cargo.toml").is_file() {
                    todo.push(dep.clone());
                }
            }
        }
    }
    Ok(seen.into_iter().collect())
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&p, base, out)?;
        } else {
            out.push(p.strip_prefix(base).unwrap_or(&p).to_path_buf());
        }
    }
    Ok(())
}

/// The files git tracks under `dir` (root-relative), or `None` where `root` is not a git work tree
/// or git cannot say. The seed is what is committed: a test run leaves ignored cache directories
/// inside the crates (`crates/nazm-cli/tests/*/.nazm/`), and a digest of the disk would change with
/// every such run and differ on every host.
fn tracked(root: &Path, dir: &Path) -> Option<Vec<PathBuf>> {
    if !root.join(".git").exists() {
        return None;
    }
    let rel = dir.strip_prefix(root).ok()?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--"])
        .arg(rel)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let files: Vec<PathBuf> = text
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(PathBuf::from)
        .collect();
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

/// Every seed file, root-relative, sorted.
pub(crate) fn seed_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for c in closure(root)? {
        let dir = root.join("crates").join(c);
        match tracked(root, &dir) {
            Some(files) => out.extend(files),
            None => walk(&dir, root, &mut out).map_err(|e| e.to_string())?,
        }
    }
    out.extend(
        FILES
            .iter()
            .map(PathBuf::from)
            .filter(|f| root.join(f).is_file()),
    );
    out.sort();
    Ok(out)
}

/// One digest over every seed file's path, length and BLAKE3.
pub(crate) fn digest(root: &Path) -> Result<String, String> {
    let mut h = blake3::Hasher::new();
    for f in seed_files(root)? {
        let bytes = std::fs::read(root.join(&f)).map_err(|e| format!("{}: {e}", f.display()))?;
        let line = format!(
            "{}\0{}\0{}\n",
            f.to_string_lossy().replace('\\', "/"),
            bytes.len(),
            blake3::hash(&bytes).to_hex()
        );
        h.update(line.as_bytes());
    }
    Ok(h.finalize().to_hex().to_string())
}

fn is_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let Ok(record) = std::fs::read_to_string(root.join(RECORD)) else {
        return Err(vec![format!(
            "{RECORD} is missing: it records the seed's digest"
        )]);
    };
    let mut bad = Vec::new();
    let mut recorded: Option<String> = None;
    for (n, line) in record.lines().enumerate().map(|(i, l)| (i + 1, l.trim())) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(h) = line.strip_prefix("base ") {
            if recorded.is_some() || !is_hex(h.trim()) {
                bad.push(format!(
                    "{RECORD}:{n}: `base` must come first, once, with a 64-digit digest"
                ));
            }
            recorded = Some(h.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("exempt ") {
            let f: Vec<&str> = rest.split('|').map(str::trim).collect();
            match f.as_slice() {
                [date, path, reason, hex]
                    if is_date(date)
                        && !path.is_empty()
                        && !reason.is_empty()
                        && is_hex(hex)
                        && recorded.is_some() =>
                {
                    recorded = Some((*hex).to_owned());
                }
                _ => bad.push(format!(
                    "{RECORD}:{n}: an exemption is `exempt DATE | PATH | REASON | DIGEST` after `base`"
                )),
            }
        } else if !line.starts_with("frozen-at ") {
            bad.push(format!("{RECORD}:{n}: unknown line"));
        }
    }
    let Some(recorded) = recorded else {
        bad.push(format!("{RECORD} has no `base` line"));
        return Err(bad);
    };
    match digest(root) {
        Err(e) => bad.push(e),
        Ok(now) if now != recorded => bad.push(format!(
            "a seed path changed (digest now {} recorded {}): the Rust implementation is frozen; the \
             copyright holder records an exemption in {RECORD} (path, reason, date, new digest), \
             or the change is reverted. `git diff` over crates/{ROOT_CRATE}'s closure, Cargo.toml, \
             Cargo.lock, rust-toolchain.toml and .cargo/config.toml shows it",
            now, recorded
        )),
        Ok(_) => {}
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let f = d.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        };
        w(
            "crates/nazm-cli/Cargo.toml",
            "[dependencies]\nnazm-core = { workspace = true }\n",
        );
        w("crates/nazm-cli/src/main.rs", "fn main() {}\n");
        w(
            "crates/nazm-core/Cargo.toml",
            "[dependencies]\nserde = \"1\"\n",
        );
        w("crates/nazm-core/src/lib.rs", "pub fn f() {}\n");
        w(
            "crates/nazm-bench/Cargo.toml",
            "[dependencies]\nnazm-cli = { workspace = true }\n",
        );
        w("crates/nazm-bench/src/lib.rs", "// not the seed\n");
        w("Cargo.toml", "[workspace]\n");
        w("xtask/src/main.rs", "// not the seed\n");
        let rec = format!("base {}\n", digest(d.path()).unwrap());
        w(RECORD, &rec);
        d
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        // A contained run mounts a tree without `.git`: the digest would then read the disk,
        // build output included, and could not equal the committed record.
        if !root.join(".git").exists() {
            return;
        }
        assert_eq!(check(root), Ok(()));
    }

    #[test]
    fn the_seed_is_the_build_closure() {
        let d = tree();
        let files: Vec<String> = seed_files(d.path())
            .unwrap()
            .iter()
            .map(|f| f.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(files.contains(&"crates/nazm-core/src/lib.rs".to_owned()));
        assert!(!files
            .iter()
            .any(|f| f.starts_with("crates/nazm-bench") || f.starts_with("xtask")));
    }

    #[test]
    fn a_seed_change_is_refused_and_a_recorded_exemption_accepts_it() {
        let d = tree();
        assert_eq!(check(d.path()), Ok(()));
        // outside the seed: no effect
        std::fs::write(d.path().join("xtask/src/main.rs"), "// changed\n").unwrap();
        std::fs::write(
            d.path().join("crates/nazm-bench/src/lib.rs"),
            "// changed\n",
        )
        .unwrap();
        assert_eq!(check(d.path()), Ok(()));
        // inside the seed: refused
        std::fs::write(
            d.path().join("crates/nazm-core/src/lib.rs"),
            "pub fn g() {}\n",
        )
        .unwrap();
        let err = check(d.path()).expect_err("seed change").join("\n");
        assert!(err.contains("a seed path changed"), "{err}");
        let new = digest(d.path()).unwrap();
        let rec = std::fs::read_to_string(d.path().join(RECORD)).unwrap();
        std::fs::write(
            d.path().join(RECORD),
            format!("{rec}exempt 2026-11-05 | crates/nazm-core/src/lib.rs | test | {new}\n"),
        )
        .unwrap();
        assert_eq!(check(d.path()), Ok(()));
        // a malformed exemption is refused
        std::fs::write(
            d.path().join(RECORD),
            format!("{rec}exempt today | x | | {new}\n"),
        )
        .unwrap();
        assert!(check(d.path()).is_err());
    }
}
