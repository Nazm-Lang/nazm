//! Which MILESTONE evidence a set of changed paths calls for (N32-H2).
//!
//! Deterministic and inspectable: a fixed table from path prefixes to what they can
//! affect, applied to the paths the caller names. It never reads Git history; a caller that
//! wants the working diff passes it. It narrows exactly one thing — whether the whole
//! lifecycle suite runs or only its invariant set — and every path it does not recognise
//! widens the gate rather than narrowing it. Every other stage of the MILESTONE gate runs
//! whatever the paths say.

/// The lifecycle suite, whole or its invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    /// Every test in `xtask/tests/lifecycle.rs`.
    Full,
    /// The invariant set: a verdict, the warm session's per-mutant bytes, and a fresh copy
    /// against a persistent target.
    Invariant,
}

/// The lifecycle tests every MILESTONE gate runs, whatever changed.
pub(crate) const INVARIANT: &[&str] = &[
    "one_entry_runs_report_caught_survived_and_did_not_build",
    "a_warm_session_verifies_every_killer_against_each_mutants_own_bytes",
    "a_fresh_copy_against_a_persistent_target_measures_only_its_own_bytes",
];

/// What a gate needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) lifecycle: Lifecycle,
    /// The MCP server changed: its shared mutations belong in the campaign.
    pub(crate) mcp: bool,
    /// The benchmark families the paths can move (N32-H3): `compiler` (`cargo xtask
    /// bench`), `docs` (documentation retrieval), `mcp` (the server's frames and memory),
    /// `harness` (the gate's own timing). An unknown path, or none, brings every family.
    pub(crate) benches: std::collections::BTreeSet<&'static str>,
    /// Why, one line per path.
    pub(crate) reasons: Vec<String>,
}

/// What one path can affect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The harness, its catalogue or its environment: every lifecycle test.
    Harness,
    /// The MCP server: its shared mutations, and nothing of the harness.
    Mcp,
    /// Language, service, tools or documentation: nothing the harness runs on.
    Product,
    /// Not in the table. Treated as the harness.
    Unknown,
}

fn reach(path: &str) -> Reach {
    const HARNESS: &[&str] = &[
        "xtask/",
        "docker/",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/",
    ];
    const PRODUCT: &[&str] = &[
        "crates/",
        "compiler/",
        "library/",
        "docs/",
        "schema/",
        "examples/",
        "corpus/",
        "README.md",
    ];
    let path = path.strip_prefix("./").unwrap_or(path);
    if HARNESS.iter().any(|p| path == *p || path.starts_with(p)) {
        Reach::Harness
    } else if path.starts_with("crates/nazm-mcp/") {
        Reach::Mcp
    } else if PRODUCT.iter().any(|p| path == *p || path.starts_with(p)) {
        Reach::Product
    } else {
        Reach::Unknown
    }
}

/// Every benchmark family.
pub(crate) const BENCHES: &[&str] = &["compiler", "docs", "harness", "mcp"];

/// The benchmark families one path can move. Explicit: a path the table does not name is
/// given every family by the caller.
fn benches_of(path: &str) -> Option<&'static [&'static str]> {
    const TABLE: &[(&str, &[&str])] = &[
        ("crates/nazm-docs/", &["docs", "mcp"]),
        ("crates/nazm-mcp/", &["mcp"]),
        ("docs/", &["docs"]),
        ("README.md", &[]),
        ("schema/", &[]),
        ("xtask/", &["harness"]),
        ("docker/", &["harness"]),
        ("crates/nazm-service/", &["compiler", "mcp"]),
        ("crates/", &["compiler"]),
        ("compiler/", &["compiler"]),
        ("library/", &["compiler"]),
        ("examples/", &["compiler"]),
        ("corpus/", &["compiler"]),
    ];
    let path = path.strip_prefix("./").unwrap_or(path);
    TABLE
        .iter()
        .find(|(prefix, _)| path == *prefix || path.starts_with(prefix))
        .map(|(_, b)| *b)
}

/// The gate `paths` call for. No paths is not "nothing changed": it is an unknown change,
/// and gets the full lifecycle and every benchmark.
pub(crate) fn plan(paths: &[String]) -> Plan {
    let mut lifecycle = Lifecycle::Invariant;
    let mut mcp = false;
    let mut reasons = Vec::new();
    let mut benches = std::collections::BTreeSet::new();
    if paths.is_empty() {
        lifecycle = Lifecycle::Full;
        benches.extend(BENCHES.iter().copied());
        reasons.push(
            "no changed paths were given: the full lifecycle suite and every benchmark".to_owned(),
        );
    }
    for p in paths {
        match benches_of(p) {
            Some(b) => benches.extend(b.iter().copied()),
            None => benches.extend(BENCHES.iter().copied()),
        }
    }
    for p in paths {
        let r = reach(p);
        match r {
            Reach::Harness => lifecycle = Lifecycle::Full,
            Reach::Unknown => lifecycle = Lifecycle::Full,
            Reach::Mcp => mcp = true,
            Reach::Product => {}
        }
        reasons.push(format!(
            "{p}: {}",
            match r {
                Reach::Harness => "the harness or its environment — full lifecycle",
                Reach::Unknown => "not in the table — full lifecycle, conservatively",
                Reach::Mcp => "the MCP server — its shared mutations",
                Reach::Product => "no harness effect — invariant lifecycle",
            }
        ));
    }
    Plan {
        lifecycle,
        mcp,
        benches,
        reasons,
    }
}

/// The stages every MILESTONE gate runs, whatever changed.
pub(crate) const CORE: &[&str] = &["mutation", "workspace", "selfhost", "bootstrap", "gates"];

impl Plan {
    /// Every stage and responsibility this plan runs: the core stages, the lifecycle tests
    /// (`lifecycle:*` for the whole suite), the MCP shared mutations and each benchmark.
    pub(crate) fn stages(&self) -> std::collections::BTreeSet<String> {
        let mut out: std::collections::BTreeSet<String> =
            CORE.iter().map(|s| (*s).to_owned()).collect();
        match self.lifecycle {
            Lifecycle::Full => {
                out.insert("lifecycle:*".to_owned());
            }
            Lifecycle::Invariant => {
                out.extend(INVARIANT.iter().map(|t| format!("lifecycle:{t}")));
            }
        }
        if self.mcp {
            out.insert("mutations:mcp".to_owned());
        }
        out.extend(self.benches.iter().map(|b| format!("bench:{b}")));
        out
    }

    /// Whether this plan runs every stage `other` runs: the whole lifecycle suite covers
    /// each of its tests. A gate run under this plan is then an upper bound on `other`'s
    /// work and evidence, and `other` need not be run separately to bound its time.
    pub(crate) fn covers(&self, other: &Plan) -> bool {
        let mine = self.stages();
        other.stages().iter().all(|s| {
            mine.contains(s) || (s.starts_with("lifecycle:") && mine.contains("lifecycle:*"))
        })
    }

    /// The `cargo test` arguments for the lifecycle suite this plan asks for.
    pub(crate) fn lifecycle_args(&self) -> Vec<String> {
        let mut v: Vec<String> = ["test", "-p", "xtask", "--test", "lifecycle"]
            .map(str::to_owned)
            .to_vec();
        v.push("--".to_owned());
        if self.lifecycle == Lifecycle::Invariant {
            v.extend(INVARIANT.iter().map(|t| (*t).to_owned()));
            v.push("--exact".to_owned());
        }
        v.extend(["--ignored", "--test-threads", "1"].map(str::to_owned));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(paths: &[&str]) -> Plan {
        plan(&paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_harness_calls_for_the_whole_lifecycle_and_the_product_for_its_invariants() {
        assert_eq!(of(&["xtask/src/mutate.rs"]).lifecycle, Lifecycle::Full);
        assert_eq!(
            of(&["xtask/mutations/mutations.toml"]).lifecycle,
            Lifecycle::Full
        );
        assert_eq!(of(&["Cargo.lock"]).lifecycle, Lifecycle::Full);
        assert_eq!(
            of(&["docker/contained.Dockerfile"]).lifecycle,
            Lifecycle::Full
        );
        assert_eq!(of(&["docs/user/spec.md"]).lifecycle, Lifecycle::Invariant);
        assert_eq!(
            of(&[
                "crates/nazm-core/src/lib.rs",
                "compiler/emit.nz",
                "README.md"
            ])
            .lifecycle,
            Lifecycle::Invariant
        );
        // One harness path among product paths is enough.
        assert_eq!(
            of(&["docs/user/spec.md", "xtask/src/contained.rs"]).lifecycle,
            Lifecycle::Full
        );
    }

    #[test]
    fn an_unknown_path_or_no_path_widens_the_gate() {
        assert_eq!(of(&["somewhere/new.rs"]).lifecycle, Lifecycle::Full);
        assert_eq!(of(&[".github/workflows/ci.yml"]).lifecycle, Lifecycle::Full);
        assert_eq!(of(&[]).lifecycle, Lifecycle::Full);
    }

    #[test]
    fn the_mcp_server_brings_its_shared_mutations_and_nothing_of_the_harness() {
        let p = of(&["crates/nazm-mcp/src/main.rs"]);
        assert!(p.mcp);
        assert_eq!(p.lifecycle, Lifecycle::Invariant);
        assert!(!of(&["crates/nazm-docs/src/lib.rs"]).mcp);
    }

    #[test]
    fn benchmarks_follow_the_paths_and_an_unknown_path_brings_them_all() {
        let set = |p: &[&str]| of(p).benches.into_iter().collect::<Vec<_>>();
        assert_eq!(set(&["docs/user/spec.md"]), ["docs"]);
        assert_eq!(set(&["crates/nazm-docs/src/lib.rs"]), ["docs", "mcp"]);
        assert_eq!(set(&["crates/nazm-core/src/lib.rs"]), ["compiler"]);
        assert_eq!(set(&["xtask/src/suite.rs"]), ["harness"]);
        assert!(set(&["README.md"]).is_empty());
        assert_eq!(set(&["somewhere/new.rs"]), BENCHES);
        assert_eq!(set(&[]), BENCHES);
    }

    /// N32-H3's acceptance rests on this: a gate for a product or docs change runs a subset
    /// of the gate for the same change plus a harness change — the invariant lifecycle
    /// tests inside the whole suite, its benchmarks among the larger set, the same core
    /// stages — so measuring the larger gate bounds the smaller one.
    #[test]
    fn a_normal_milestone_plan_is_covered_by_the_harness_changing_plan_over_the_same_paths() {
        let normals: [&[&str]; 5] = [
            &["docs/user/spec.md"],
            &["crates/nazm-docs/src/lib.rs", "docs/user/spec.md"],
            &["crates/nazm-mcp/src/main.rs"],
            &["crates/nazm-core/src/lib.rs", "compiler/emit.nz"],
            &["README.md"],
        ];
        for normal in normals {
            let small = of(normal);
            let mut both: Vec<&str> = normal.to_vec();
            both.extend(["xtask/src/mutate.rs", "xtask/src/suite.rs"]);
            let large = of(&both);
            assert!(
                large.covers(&small),
                "{normal:?}: {:?} vs {:?}",
                small.stages(),
                large.stages()
            );
            assert_eq!(large.lifecycle, Lifecycle::Full);
            // And the relation is not vacuous: the harness plan runs strictly more.
            assert!(!small.covers(&large), "{normal:?}");
            for core in CORE {
                assert!(small.stages().contains(*core), "{normal:?} lacks {core}");
            }
        }
    }

    #[test]
    fn the_invariant_set_is_named_exactly_and_the_full_set_is_not_filtered() {
        let inv = of(&["docs/user/spec.md"]).lifecycle_args();
        assert!(inv.contains(&"--exact".to_owned()));
        for t in INVARIANT {
            assert!(inv.contains(&(*t).to_owned()), "{t}");
        }
        let full = of(&["xtask/src/plan.rs"]).lifecycle_args();
        assert!(!full.contains(&"--exact".to_owned()));
        assert!(full.contains(&"--ignored".to_owned()));
    }
}
