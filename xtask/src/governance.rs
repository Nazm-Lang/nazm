//! The **agent governance** gate (M1): the repository's agent-instruction and change-governance
//! files stay structurally consistent with `docs/internal/agent-governance.md`.
//!
//! A pure read of the tree — nothing is built, nothing is fetched, Node is not needed. Each rule
//! names the sentence of `docs/internal/agent-governance.md` it protects. It checks structure only:
//! existence, links resolving, required fields present, forbidden allowances absent. Whether an
//! instruction is *right* is review's question, not this gate's.

use std::path::Path;

/// The root agent map, and the shim every vendor file must defer to it through.
const ROOT: &str = "AGENTS.md";
const SHIM: &str = "CLAUDE.md";

/// The agent's local working state: named by the map, never committed (§6).
const LOCAL_STATE: &str = ".agent/state.md";

/// Subtrees with their own instructions (`docs/internal/agent-governance.md` §1, level 3).
const NESTED: &[&str] = &[
    "compiler/AGENTS.md",
    "crates/nazm-runtime/AGENTS.md",
    "xtask/AGENTS.md",
];

/// Allowances a nested file may never grant, since the root forbids them (§4, *Forbidden* and
/// *Requires human approval*). Matched case-insensitively; a nested file states prohibitions in
/// its own words, but none of these phrasings.
const RELAXATIONS: &[&str] = &[
    "may push",
    "may tag",
    "may publish",
    "may prune",
    "may stash",
    "may reset",
    "may set nazm_contained",
    "nazm_allow_uncontained=1 is allowed",
    "without containment is allowed",
    "may skip containment",
];

/// Each template, and the fields its convention requires (§5, §6; `.agent/PLANS.md`).
const TEMPLATES: &[(&str, &[&str])] = &[
    (
        ".agent/templates/exec-plan.md",
        &[
            "Starting HEAD",
            "## Non-goals",
            "## Invariants affected",
            "## Design",
            "## Alternatives considered",
            "## Compatibility impact",
            "## Security impact",
            "## Tests",
            "## Mutation strategy",
            "## Rollback / recovery",
            "## Acceptance criteria",
            "## Stop conditions",
            "## Evidence outputs",
            "## Handoff state",
        ],
    ),
    (
        ".agent/templates/handoff.md",
        &[
            "Starting HEAD",
            "Current / final HEAD",
            "Commits",
            "Working tree",
            "## Tests",
            "## Mutations",
            "## Known limitations",
            "## Exact blockers",
            "## Owned resources still active",
            "## Next exact action",
            "## Prohibited actions not taken",
        ],
    ),
    (
        ".agent/templates/decision.md",
        &[
            "**Problem:**",
            "**Options:**",
            "**Chosen:**",
            "**Rejected alternatives",
            "**Compatibility impact:**",
            "**Security impact:**",
            "**Evidence, and what would falsify",
            "**Rollback / revisit trigger:**",
        ],
    ),
    (
        ".agent/templates/task-contract.md",
        &[
            "**Goal:**",
            "**Starting HEAD:**",
            "**Scope:**",
            "**Non-goals:**",
            "**Authorities:**",
            "**Invariants",
            "**Allowed actions:**",
            "**Prohibited actions:**",
            "**Acceptance criteria:**",
            "**Evidence",
            "**Stop conditions:**",
        ],
    ),
    (
        ".agent/templates/state.md",
        &[
            "**HEAD:**",
            "**Active change / task ID:**",
            "**Uncommitted task-owned files:**",
            "**User-owned files not to touch:**",
            "**Owned temporary resources:**",
            "**Next exact action / command:**",
            "**Heavy workload owns the tree:**",
        ],
    ),
];

/// The documents the governance layer is made of.
const DOCUMENTS: &[&str] = &[
    "docs/internal/agent-governance.md",
    "docs/internal/change-governance.md",
    "docs/internal/agent-evals.md",
    ".agent/PLANS.md",
    "openspec/config.yaml",
];

/// The capabilities `openspec/specs/` may hold: repository process, never language or toolchain
/// behaviour, whose text belongs in the Nazm authorities (`docs/internal/change-governance.md`).
const PROCESS_CAPABILITIES: &[&str] = &["repository-governance"];

/// The gate, over the repository at `root`.
pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut bad = Vec::new();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();

    for doc in DOCUMENTS {
        if !root.join(doc).is_file() {
            bad.push(format!(
                "{doc} is missing (docs/internal/agent-governance.md names it)"
            ));
        }
    }

    // §1, level 3: the root map exists, and every repository path it links to does.
    match read(ROOT) {
        None => bad.push(format!("{ROOT} is missing: it is the agents' entry point")),
        Some(text) => {
            for target in link_targets(&text) {
                // Local working state, deliberately not committed (§6).
                if target == LOCAL_STATE {
                    continue;
                }
                if !root.join(&target).exists() {
                    bad.push(format!("{ROOT} links to `{target}`, which does not exist"));
                }
            }
        }
    }

    // A3: the vendor shim defers to the root rather than governing on its own.
    match read(SHIM) {
        None => bad.push(format!(
            "{SHIM} is missing: the Claude shim that defers to {ROOT}"
        )),
        Some(text) if !text.contains(ROOT) => {
            bad.push(format!("{SHIM} does not defer to {ROOT}"));
        }
        Some(_) => {}
    }

    // §1: a nested file specialises the root and never relaxes it.
    for nested in NESTED {
        match read(nested) {
            None => bad.push(format!("{nested} is missing")),
            Some(text) => {
                if !text.contains("root `AGENTS.md`") {
                    bad.push(format!(
                        "{nested} does not say it specialises the root `AGENTS.md`"
                    ));
                }
                let lower = text.to_lowercase();
                for r in RELAXATIONS {
                    if lower.contains(r) {
                        bad.push(format!("{nested} grants what the root forbids: \"{r}\""));
                    }
                }
            }
        }
    }

    // §5, §6: every template exists with its required fields.
    for (path, fields) in TEMPLATES {
        match read(path) {
            None => bad.push(format!("{path} is missing")),
            Some(text) => {
                for f in *fields {
                    if !text.contains(f) {
                        bad.push(format!("{path} lacks the required field `{f}`"));
                    }
                }
            }
        }
    }

    bad.extend(openspec(root));
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

/// OpenSpec holds changes, not a second specification (`docs/internal/change-governance.md`).
fn openspec(root: &Path) -> Vec<String> {
    let mut bad = Vec::new();
    let dirs = |rel: &str| -> Vec<String> {
        std::fs::read_dir(root.join(rel))
            .map(|it| {
                it.filter_map(Result::ok)
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    for change in dirs("openspec/changes") {
        if change == "archive" {
            continue;
        }
        if !root
            .join("openspec/changes")
            .join(&change)
            .join("proposal.md")
            .is_file()
        {
            bad.push(format!("the OpenSpec change `{change}` has no proposal.md"));
        }
    }
    for cap in dirs("openspec/specs") {
        if !PROCESS_CAPABILITIES.contains(&cap.as_str()) {
            bad.push(format!(
                "openspec/specs/{cap} is not a repository-process capability: a change to the \
                 language or toolchain archives with --skip-specs, and its text belongs in \
                 docs/user/spec.md and the other Nazm authorities"
            ));
        }
    }
    bad
}

/// The repository-relative targets of a Markdown file's inline links: `](path)`, without a
/// scheme or an in-page anchor, the fragment dropped.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        rest = rest.get(at + 2..).unwrap_or("");
        let end = rest.find(')').unwrap_or(rest.len());
        let target = rest.get(..end).unwrap_or("");
        let path = target.split('#').next().unwrap_or("");
        if !path.is_empty() && !path.contains("://") && !path.starts_with("mailto:") {
            out.push(path.to_owned());
        }
    }
    // Backticked repository paths in the authority table count as links too: a renamed
    // authority must not leave the map pointing at nothing.
    for piece in text.split('`').skip(1).step_by(2) {
        let looks_like_path = (piece.starts_with("docs/") || piece.starts_with(".agent/"))
            && piece.ends_with(".md")
            && !piece.contains(' ')
            && !piece.contains('<');
        if looks_like_path {
            out.push(piece.to_owned());
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    /// A copy of the governance files, to break one at a time.
    fn copy() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let src = repo();
        let mut files: Vec<String> = [ROOT, SHIM]
            .iter()
            .chain(NESTED)
            .chain(DOCUMENTS)
            .map(|s| (*s).to_owned())
            .collect();
        files.extend(TEMPLATES.iter().map(|(p, _)| (*p).to_owned()));
        let root_text = std::fs::read_to_string(src.join(ROOT)).unwrap();
        files.extend(link_targets(&root_text));
        for f in files {
            let to = d.path().join(&f);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            if src.join(&f).is_file() {
                std::fs::copy(src.join(&f), &to).unwrap();
            } else {
                std::fs::create_dir_all(&to).unwrap();
            }
        }
        d
    }

    #[test]
    fn the_repository_passes() {
        assert_eq!(check(&repo()), Ok(()));
    }

    #[test]
    fn each_rule_fails_on_its_own_breakage() {
        let fails = |edit: &dyn Fn(&Path), expect: &str| {
            let d = copy();
            assert_eq!(check(d.path()), Ok(()), "the copy passes before `{expect}`");
            edit(d.path());
            let err = check(d.path()).expect_err(expect).join("\n");
            assert!(err.contains(expect), "wanted `{expect}`, got: {err}");
        };
        fails(
            &|p| std::fs::write(p.join(SHIM), "Claude notes only.\n").unwrap(),
            "does not defer",
        );
        fails(
            &|p| {
                let t = std::fs::read_to_string(p.join(ROOT)).unwrap();
                std::fs::write(p.join(ROOT), format!("{t}\nSee [x](docs/nowhere.md).\n")).unwrap();
            },
            "docs/nowhere.md",
        );
        fails(
            &|p| {
                let f = p.join("xtask/AGENTS.md");
                let t = std::fs::read_to_string(&f).unwrap();
                std::fs::write(f, format!("{t}\nAn agent may push the branch when done.\n"))
                    .unwrap();
            },
            "grants what the root forbids",
        );
        fails(
            &|p| {
                let f = p.join(".agent/templates/handoff.md");
                let t = std::fs::read_to_string(&f).unwrap();
                std::fs::write(f, t.replace("## Next exact action", "## Next")).unwrap();
            },
            "Next exact action",
        );
        fails(
            &|p| std::fs::create_dir_all(p.join("openspec/specs/arrays")).unwrap(),
            "openspec/specs/arrays",
        );
        fails(
            &|p| std::fs::create_dir_all(p.join("openspec/changes/half-written")).unwrap(),
            "half-written",
        );
    }
}
