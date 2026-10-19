//! The dependency-source policy (Q1, `docs/internal/security-qualification.md`, *Supply chain*): every
//! crate the compiler is built from is either this workspace's own or comes from crates.io with
//! the checksum Cargo verifies, read off `Cargo.lock`. `deny.toml` states the same policy for
//! `cargo deny`, which needs a network for its advisory database; this check needs nothing, so it
//! runs in `cargo xtask check` and fails before a git or foreign-registry dependency lands.

use std::path::Path;

/// The one registry a dependency may come from.
pub(crate) const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|e| vec![format!("Cargo.lock: {e}")])?;
    let problems = check_lock(&lock);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every package with a source names crates.io and carries a checksum; one with no source is a
/// workspace member, which Cargo records without one.
pub(crate) fn check_lock(lock: &str) -> Vec<String> {
    let parsed: toml::Value = match toml::from_str(lock) {
        Ok(v) => v,
        Err(e) => return vec![format!("Cargo.lock does not parse: {e}")],
    };
    let Some(packages) = parsed.get("package").and_then(toml::Value::as_array) else {
        return vec!["Cargo.lock lists no packages".to_owned()];
    };
    let mut problems = Vec::new();
    for p in packages {
        let field = |k: &str| p.get(k).and_then(toml::Value::as_str);
        let name = format!(
            "{} {}",
            field("name").unwrap_or("<unnamed>"),
            field("version").unwrap_or("<unversioned>")
        );
        match field("source") {
            None => {}
            Some(CRATES_IO) => {
                if field("checksum").is_none() {
                    problems.push(format!(
                        "Cargo.lock: {name} comes from crates.io with no checksum to verify"
                    ));
                }
            }
            Some(other) => problems.push(format!(
                "Cargo.lock: {name} comes from `{other}`: only crates.io and this workspace \
                 are allowed sources (deny.toml, [sources])"
            )),
        }
    }
    problems
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const OK: &str = r#"
version = 4

[[package]]
name = "nazm-core"
version = "1.0.0"

[[package]]
name = "serde"
version = "1.0.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"#;

    #[test]
    fn the_repository_lockfile_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(check(root), Ok(()));
    }

    #[test]
    fn crates_io_with_a_checksum_and_workspace_members_pass() {
        assert!(check_lock(OK).is_empty());
    }

    #[test]
    fn a_git_dependency_is_refused() {
        let lock = OK.replace(
            "registry+https://github.com/rust-lang/crates.io-index",
            "git+https://example.com/serde?rev=1#1",
        );
        let p = check_lock(&lock);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p.iter()
                .all(|m| m.contains("serde 1.0.1") && m.contains("git+")),
            "{p:?}"
        );
    }

    #[test]
    fn another_registry_is_refused() {
        let lock = OK.replace("github.com/rust-lang/crates.io-index", "example.com/index");
        assert_eq!(check_lock(&lock).len(), 1);
    }

    #[test]
    fn a_registry_crate_without_a_checksum_is_refused() {
        let lock = OK.replace("checksum = \"00\"\n", "");
        let p = check_lock(&lock);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p.iter().all(|m| m.contains("no checksum")), "{p:?}");
    }

    #[test]
    fn an_unreadable_lockfile_is_a_failure_not_a_pass() {
        assert!(!check_lock("[[package").is_empty());
        assert!(!check_lock("version = 4").is_empty());
    }
}
