//! The mutation catalogue, read as data.
//!
//! Which mutations exist and where each one's defect is (`file` and its exact `find`), which
//! tests were *verified* to kill it (`killers`), and which tests each profile says observe
//! the paths it `owns`. `cargo xtask mutate` is what validates an entry and verifies a
//! killer; this reads what it wrote and adds nothing — a test is related to a definition
//! only when a catalogue entry says so, never because its name looks related, and killing a
//! mutant is evidence the test observes that defect, not that the code is correct.

use std::path::Path;

use serde::Deserialize;

use crate::cargo::Package;
use crate::repository::Inputs;
use crate::Problem;

#[derive(Debug, Default, Deserialize)]
struct Raw {
    #[serde(default)]
    mutation: Vec<RawMutation>,
    #[serde(default)]
    profile: Vec<RawProfile>,
}

#[derive(Debug, Deserialize)]
struct RawMutation {
    name: String,
    file: String,
    find: String,
    #[serde(default)]
    killers: Vec<RawTarget>,
    #[serde(default)]
    profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    name: String,
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    targets: Vec<RawTarget>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    package: String,
    #[serde(default)]
    lib: bool,
    #[serde(default)]
    test: Option<String>,
    #[serde(default)]
    bin: Option<String>,
    /// A killer's full libtest path; absent for a profile's target.
    #[serde(default)]
    name: Option<String>,
}

/// One mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mutation {
    pub(crate) name: String,
    /// From the repository root.
    pub(crate) file: String,
    pub(crate) find: String,
    /// Each verified killer's test id.
    pub(crate) killers: Vec<String>,
    /// The profile it names, overriding the one its file would choose.
    pub(crate) profile: Option<String>,
}

/// One profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Profile {
    pub(crate) name: String,
    pub(crate) owns: Vec<String>,
    /// Target ids.
    pub(crate) targets: Vec<String>,
}

/// A killer: the test id, and the target it runs in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Test {
    pub(crate) id: String,
    pub(crate) target: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Catalogue {
    pub(crate) mutations: Vec<Mutation>,
    pub(crate) profiles: Vec<Profile>,
    /// Every distinct killer, by id.
    pub(crate) tests: Vec<Test>,
}

impl Profile {
    pub(crate) fn id(&self) -> String {
        format!("profile:{}", self.name)
    }
}

impl Mutation {
    pub(crate) fn id(&self) -> String {
        format!("mutation:{}", self.name)
    }
}

impl Catalogue {
    /// The profile whose `owns` is the longest prefix of `path` — the catalogue's own rule
    /// for which tests observe a file.
    pub(crate) fn profile_for(&self, path: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .filter_map(|p| {
                p.owns
                    .iter()
                    .filter(|o| path.starts_with(o.as_str()))
                    .map(String::len)
                    .max()
                    .map(|n| (n, p))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.name.cmp(&a.1.name)))
            .map(|(_, p)| p)
    }

    pub(crate) fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub(crate) fn test(&self, id: &str) -> Option<&Test> {
        self.tests
            .binary_search_by(|t| t.id.as_str().cmp(id))
            .ok()
            .and_then(|i| self.tests.get(i))
    }
}

/// The catalogue at `path` under `root`, its every target resolved against `packages`.
pub(crate) fn load(
    root: &Path,
    path: Option<&str>,
    packages: &[Package],
    inputs: &mut Inputs,
    problems: &mut Vec<Problem>,
) -> Catalogue {
    let Some(path) = path else {
        return Catalogue::default();
    };
    let Some(text) = inputs.read(root, path, "mutation_catalogue") else {
        problems.push(Problem::new("missing_catalogue", path));
        return Catalogue::default();
    };
    let raw: Raw = match toml::from_str(&text) {
        Ok(r) => r,
        Err(_) => {
            problems.push(Problem::new("malformed_catalogue", path));
            return Catalogue::default();
        }
    };
    let mut tests = Vec::new();
    let mut mutations = Vec::new();
    for m in raw.mutation {
        let mut killers = Vec::new();
        for k in &m.killers {
            match (target(k, packages), &k.name) {
                (Some(target), Some(name)) => {
                    let id = format!("test:{}::{name}", target.trim_start_matches("target:"));
                    killers.push(id.clone());
                    tests.push(Test {
                        id,
                        target,
                        name: name.clone(),
                    });
                }
                _ => problems.push(Problem::new("unresolved_killer", m.name.clone())),
            }
        }
        killers.sort();
        killers.dedup();
        mutations.push(Mutation {
            name: m.name,
            file: m.file,
            find: m.find,
            killers,
            profile: m.profile,
        });
    }
    let mut profiles = Vec::new();
    for p in raw.profile {
        let mut targets = Vec::new();
        for t in &p.targets {
            match target(t, packages) {
                Some(id) => targets.push(id),
                None => problems.push(Problem::new("unresolved_profile_target", p.name.clone())),
            }
        }
        targets.sort();
        targets.dedup();
        profiles.push(Profile {
            name: p.name,
            owns: p.owns,
            targets,
        });
    }
    tests.sort();
    tests.dedup();
    let mut names: Vec<&str> = mutations.iter().map(|m| m.name.as_str()).collect();
    names.sort_unstable();
    for w in names.windows(2) {
        if let [a, b] = w {
            if a == b {
                problems.push(Problem::new("duplicate_mutation", *a));
            }
        }
    }
    for m in &mutations {
        if let Some(p) = &m.profile {
            if !profiles.iter().any(|q| &q.name == p) {
                problems.push(Problem::new("unknown_profile", m.name.clone()));
            }
        }
    }
    mutations.sort_by(|a, b| a.name.cmp(&b.name));
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Catalogue {
        mutations,
        profiles,
        tests,
    }
}

/// A catalogue target as the Cargo target it names, if the workspace has it.
fn target(t: &RawTarget, packages: &[Package]) -> Option<String> {
    let p = packages.iter().find(|p| p.name == t.package)?;
    let (kind, name) = match (t.lib, &t.test, &t.bin) {
        (true, None, None) => ("lib", None),
        (false, Some(n), None) => ("test", Some(n.as_str())),
        (false, None, Some(n)) => ("bin", Some(n.as_str())),
        _ => return None,
    };
    p.targets
        .iter()
        .find(|x| x.kind == kind && name.is_none_or(|n| x.name == n))
        .map(|x| x.id(&p.name))
}
