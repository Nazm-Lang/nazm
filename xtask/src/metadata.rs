//! Minimal `cargo metadata` reader.
//!
//! Deliberately not depending on the `cargo_metadata` crate: xtask is the thing that
//! checks the dependency graph, so it keeps its own graph to two well-understood crates.

use std::process::Command;

#[derive(Debug)]
pub(crate) struct Workspace {
    pub(crate) members: Vec<Member>,
}

#[derive(Debug)]
pub(crate) struct Member {
    pub(crate) name: String,
    /// Dependencies that are themselves workspace members.
    pub(crate) internal_deps: Vec<String>,
    /// Every direct dependency, internal or not. Used by the codegen-reach gate.
    pub(crate) all_deps: Vec<String>,
}

pub(crate) fn workspace() -> Result<Workspace, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;

    let packages = json
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "cargo metadata had no packages array".to_owned())?;

    let names: Vec<String> = packages
        .iter()
        .filter_map(|p| p.get("name")?.as_str().map(str::to_owned))
        .collect();

    let members = packages
        .iter()
        .filter_map(|pkg| {
            let name = pkg.get("name")?.as_str()?.to_owned();
            let all_deps: Vec<String> = pkg
                .get("dependencies")?
                .as_array()?
                .iter()
                .filter_map(|d| d.get("name")?.as_str().map(str::to_owned))
                .collect();
            let internal_deps = all_deps
                .iter()
                .filter(|d| names.contains(d))
                .cloned()
                .collect();
            Some(Member {
                name,
                internal_deps,
                all_deps,
            })
        })
        .collect();

    Ok(Workspace { members })
}
