//! The Cargo workspace, as Cargo describes it.
//!
//! The root manifest's `[workspace] members`, each member's `[package]`, its explicit
//! targets and the ones Cargo's target auto-discovery finds — `src/lib.rs`, `src/main.rs`,
//! `src/bin/`, `tests/`, `benches/`, `examples/`, each a `*.rs` file or a directory with a
//! `main.rs` — its dependencies on other members, and the documentation it declares in
//! `[package.metadata.nazm] docs`. Only those fixed places are looked in. The test
//! `the_workspace_is_what_cargo_metadata_says` holds this reading to `cargo metadata`'s.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::Value;

use crate::repository::Inputs;
use crate::Problem;

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Package {
    pub(crate) name: String,
    /// From the repository root, without a trailing `/`; empty for a root package.
    pub(crate) dir: String,
    /// `dir/Cargo.toml`.
    pub(crate) manifest: String,
    /// By kind, then name.
    pub(crate) targets: Vec<Target>,
    /// Other members, by name: `depends_on` (normal and build) and `dev_depends_on`.
    pub(crate) dependencies: BTreeSet<(&'static str, String)>,
    /// N32 section ids, as the manifest declares them.
    pub(crate) docs: Vec<String>,
}

/// One Cargo target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Target {
    /// `lib`, `bin`, `test`, `bench` or `example`.
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    /// Its root source file, from the repository root.
    pub(crate) path: String,
}

impl Package {
    pub(crate) fn id(&self) -> String {
        format!("package:{}", self.name)
    }

    /// The directory a path must be under to be this package's, with its `/`.
    pub(crate) fn prefix(&self) -> String {
        if self.dir.is_empty() {
            String::new()
        } else {
            format!("{}/", self.dir)
        }
    }
}

impl Target {
    pub(crate) fn id(&self, package: &str) -> String {
        format!("target:{package}:{}:{}", self.kind, self.name)
    }
}

/// The workspace under `root`, or nothing if it has no root manifest.
pub(crate) fn load(root: &Path, inputs: &mut Inputs, problems: &mut Vec<Problem>) -> Vec<Package> {
    let Some(text) = inputs.read(root, "Cargo.toml", "cargo_manifest") else {
        return Vec::new();
    };
    let Some(top) = parse(&text, "Cargo.toml", problems) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    if top.contains_key("package") {
        dirs.push(String::new());
    }
    if let Some(members) = top
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(Value::as_array)
    {
        for m in members {
            match m.as_str() {
                Some(m) if m.contains(['*', '?', '[']) => {
                    problems.push(Problem::new("unsupported_member_pattern", m));
                }
                Some(m) if !relative(m) => problems.push(Problem::new("member_outside", m)),
                Some(m) => dirs.push(m.trim_end_matches('/').to_owned()),
                None => problems.push(Problem::new("malformed_manifest", "Cargo.toml")),
            }
        }
    }
    let mut packages = Vec::new();
    for dir in dirs {
        let manifest = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        let table = if dir.is_empty() {
            Some(top.clone())
        } else {
            inputs
                .read(root, &manifest, "cargo_manifest")
                .and_then(|t| parse(&t, &manifest, problems))
                .or_else(|| {
                    if !root.join(&manifest).is_file() {
                        problems.push(Problem::new("missing_manifest", manifest.clone()));
                    }
                    None
                })
        };
        if let Some(table) = table {
            if let Some(p) = package(root, &dir, &manifest, &table, problems) {
                packages.push(p);
            }
        }
    }
    let names: BTreeSet<String> = packages.iter().map(|p| p.name.clone()).collect();
    let mut seen = BTreeSet::new();
    for p in &packages {
        if !seen.insert(p.name.clone()) {
            problems.push(Problem::new("duplicate_package", p.name.clone()));
        }
    }
    // Dependencies need every member's name, so they are read in a second pass.
    for p in &mut packages {
        let manifest = p.manifest.clone();
        let table = if p.dir.is_empty() {
            Some(top.clone())
        } else {
            inputs
                .text(&manifest)
                .and_then(|t| t.parse::<toml::Table>().ok())
        };
        if let Some(table) = table {
            p.dependencies = dependencies(&table, &names);
            p.dependencies.retain(|(_, n)| *n != p.name);
        }
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    for p in &packages {
        for t in &p.targets {
            inputs.frame(
                "cargo_target",
                &format!("{}\0{}\0{}", t.id(&p.name), t.path, p.dir),
            );
        }
    }
    packages
}

fn parse(text: &str, path: &str, problems: &mut Vec<Problem>) -> Option<toml::Table> {
    match text.parse::<toml::Table>() {
        Ok(t) => Some(t),
        Err(_) => {
            problems.push(Problem::new("malformed_manifest", path));
            None
        }
    }
}

/// A repository-relative path that stays inside the repository.
fn relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.split('/').any(|s| s == "..")
}

fn package(
    root: &Path,
    dir: &str,
    manifest: &str,
    table: &toml::Table,
    problems: &mut Vec<Problem>,
) -> Option<Package> {
    let Some(pkg) = table.get("package").and_then(Value::as_table) else {
        problems.push(Problem::new("not_a_package", manifest));
        return None;
    };
    let Some(name) = pkg.get("name").and_then(Value::as_str) else {
        problems.push(Problem::new("malformed_manifest", manifest));
        return None;
    };
    let join = |p: &str| {
        if dir.is_empty() {
            p.to_owned()
        } else {
            format!("{dir}/{p}")
        }
    };
    let exists = |p: &str| root.join(join(p)).is_file();
    let auto = |key: &str| pkg.get(key).and_then(Value::as_bool).unwrap_or(true);

    let mut targets: Vec<Target> = Vec::new();
    let lib_table = table.get("lib").and_then(Value::as_table);
    if lib_table.is_some() || exists("src/lib.rs") {
        let lib_name = lib_table
            .and_then(|l| l.get("name"))
            .and_then(Value::as_str)
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let path = lib_table
            .and_then(|l| l.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("src/lib.rs");
        targets.push(Target {
            kind: "lib",
            name: lib_name,
            path: join(path),
        });
    }
    for (kind, key, auto_key, default_dir) in [
        ("bin", "bin", "autobins", "src/bin"),
        ("test", "test", "autotests", "tests"),
        ("bench", "bench", "autobenches", "benches"),
        ("example", "example", "autoexamples", "examples"),
    ] {
        let mut explicit: Vec<Target> = Vec::new();
        for t in table
            .get(key)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(t_name) = t.get("name").and_then(Value::as_str) else {
                problems.push(Problem::new("malformed_manifest", manifest));
                continue;
            };
            let path = t.get("path").and_then(Value::as_str).map_or_else(
                || {
                    if kind == "bin" && t_name == name && exists("src/main.rs") {
                        "src/main.rs".to_owned()
                    } else if exists(&format!("{default_dir}/{t_name}.rs")) {
                        format!("{default_dir}/{t_name}.rs")
                    } else {
                        format!("{default_dir}/{t_name}/main.rs")
                    }
                },
                str::to_owned,
            );
            explicit.push(Target {
                kind,
                name: t_name.to_owned(),
                path: join(&path),
            });
        }
        let mut found: Vec<Target> = Vec::new();
        if auto(auto_key) {
            if kind == "bin" && exists("src/main.rs") {
                found.push(Target {
                    kind,
                    name: name.to_owned(),
                    path: join("src/main.rs"),
                });
            }
            for (stem, path) in discover(root, &join(default_dir)) {
                found.push(Target {
                    kind,
                    name: stem,
                    path,
                });
            }
        }
        for f in found {
            if !explicit
                .iter()
                .any(|e| e.name == f.name || e.path == f.path)
            {
                explicit.push(f);
            }
        }
        targets.extend(explicit);
    }
    targets.sort();
    targets.dedup();

    let docs = pkg
        .get("metadata")
        .and_then(|m| m.get("nazm"))
        .and_then(|n| n.get("docs"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(Package {
        name: name.to_owned(),
        dir: dir.to_owned(),
        manifest: manifest.to_owned(),
        targets,
        dependencies: BTreeSet::new(),
        docs,
    })
}

/// `dir/*.rs` and `dir/*/main.rs`, by name — the only shapes auto-discovery takes.
fn discover(root: &Path, dir: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
        return Vec::new();
    };
    let mut out = BTreeMap::new();
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let path = e.path();
        if path.is_file() {
            if let Some(stem) = name.strip_suffix(".rs") {
                out.insert(stem.to_owned(), format!("{dir}/{name}"));
            }
        } else if path.join("main.rs").is_file() {
            out.insert(name.clone(), format!("{dir}/{name}/main.rs"));
        }
    }
    out.into_iter().collect()
}

/// Which other members a manifest depends on, by the table each is in.
fn dependencies(
    table: &toml::Table,
    members: &BTreeSet<String>,
) -> BTreeSet<(&'static str, String)> {
    let mut out = BTreeSet::new();
    let mut read = |deps: Option<&Value>, relation: &'static str| {
        for (key, spec) in deps.and_then(Value::as_table).into_iter().flatten() {
            let name = spec
                .get("package")
                .and_then(Value::as_str)
                .unwrap_or(key.as_str());
            if members.contains(name) {
                out.insert((relation, name.to_owned()));
            }
        }
    };
    read(table.get("dependencies"), "depends_on");
    read(table.get("build-dependencies"), "depends_on");
    read(table.get("dev-dependencies"), "dev_depends_on");
    if let Some(targets) = table.get("target").and_then(Value::as_table) {
        for t in targets.values() {
            read(t.get("dependencies"), "depends_on");
            read(t.get("build-dependencies"), "depends_on");
            read(t.get("dev-dependencies"), "dev_depends_on");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_member_path_stays_inside_the_repository() {
        assert!(super::relative("crates/nazm-docs"));
        assert!(super::relative("xtask"));
        assert!(!super::relative("../elsewhere"));
        assert!(!super::relative("crates/../../x"));
        assert!(!super::relative("/etc"));
        assert!(!super::relative(""));
    }
}
