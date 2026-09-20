//! The architectural rules CI enforces.
//!
//! This file is the machine-readable form of the crate table in `docs/architecture.md` §7.
//! Compiler workspaces rot in a specific way: a dependency gets added because it was
//! convenient, and two years later the type checker imports the backend and nobody can say
//! when that started. Writing the layering down and failing the build on violations is
//! cheaper than discovering it later.
//!
//! The rules are near-trivial today because the workspace is two crates. They are here now
//! because adding a crate must be a deliberate act that includes deciding where it sits.

/// Which internal crates each workspace member may depend on.
///
/// `None` means "unknown crate", which is itself a failure: a new crate has to be placed
/// in the layering before it can build.
pub(crate) fn allowed_internal_deps(crate_name: &str) -> Option<&'static [&'static str]> {
    let deps: &[&str] = match crate_name {
        // --- the language front end, in dependency order ---
        // Spans depend on nothing: every later layer reports positions through them, so a
        // dependency here would reach the whole compiler.
        "nazm-span" => &[],
        "nazm-diag" => &["nazm-span"],
        "nazm-syntax" => &["nazm-span", "nazm-diag"],
        "nazm-core" => &["nazm-span", "nazm-diag", "nazm-syntax"],
        // Level 7 of the table in `docs/architecture.md` §2, and the only level built.
        // It reads the syntax tree and produces LLVM IR *as text*, so it needs no
        // code-generator dependency — which is why `may_depend_on_codegen` below stays
        // false for it.
        "nazm-lir" => &["nazm-span", "nazm-diag", "nazm-syntax"],
        // The CLI deliberately does NOT depend on nazm-syntax: it goes through nazm-core,
        // so there is one entry point into the front end rather than two that can drift.
        "nazm-cli" => &["nazm-span", "nazm-diag", "nazm-core", "nazm-lir"],

        // The measurement instrument answers to nothing. It must stay usable — and
        // auditable — with no compiler present, because it has to run against arms A1 and
        // A2 before Nazm exists, and its results have to be defensible independently of it.
        "nazm-bench" => &[],
        // Automation is allowed to know about everything, and nothing may depend on it.
        "xtask" => &[],
        _ => return None,
    };
    Some(deps)
}

/// Crates permitted to contain `unsafe`, with the justification.
///
/// Empty today: `unsafe_code = "forbid"` holds at the workspace level. When `nazm-rt`
/// arrives it needs stack switching and a custom allocator, at which point the workspace
/// lint relaxes to `deny` and the exception is listed here rather than becoming folklore.
pub(crate) fn unsafe_allowed(crate_name: &str) -> Option<&'static str> {
    // Empty by construction. When `nazm-rt` arrives it needs stack switching and a custom
    // allocator; at that point this gains an entry AND the workspace lint relaxes from
    // `forbid` to `deny`. `check_unsafe_policy` fails if those two ever disagree, so the
    // exception cannot be granted silently in one place only.
    const ALLOWED: &[(&str, &str)] = &[];
    ALLOWED
        .iter()
        .find(|(name, _)| *name == crate_name)
        .map(|(_, why)| *why)
}

/// Which crates may emit a backend artefact or invoke the native toolchain.
///
/// Separate from [`may_depend_on_codegen`] because they establish different things.
/// That one reads the dependency graph; **this one is about capability**, which is what
/// actually matters once a backend emits LLVM IR as *text* and therefore has no codegen
/// dependency to find.
///
/// `nazm-lir` emits IR and nothing else — it never runs a tool. `nazm-cli` is the driver
/// and runs `clang`, but does not itself generate IR. Splitting them this way is what
/// keeps `nazm check` structurally unable to reach either.
pub(crate) fn emission_allowlist() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("crates/nazm-span", &[]),
        ("crates/nazm-diag", &[]),
        ("crates/nazm-syntax", &[]),
        // The checker and evaluator. Empty on purpose: this is the check-only path.
        ("crates/nazm-core", &[]),
        ("crates/nazm-lir", &["emits LLVM IR"]),
        (
            "crates/nazm-cli",
            &["emits LLVM IR", "invokes the native toolchain"],
        ),
        ("crates/nazm-bench", &[]),
    ]
}

/// Crates that may depend on a code generator.
///
/// The check this enables: nothing in the front end or the checker may **depend on**
/// Cranelift or LLVM. It is written before either exists, because the rule is cheap now
/// and contentious later. It says nothing about text emission — see
/// [`emission_allowlist`] for that.
pub(crate) fn may_depend_on_codegen(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "nazm-codegen-clif" | "nazm-codegen-llvm" | "nazm-driver" | "xtask"
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn the_harness_depends_on_no_internal_crate() {
        // It must run with no compiler present. A dependency here would mean benchmark
        // results could not be reproduced without building Nazm.
        assert_eq!(allowed_internal_deps("nazm-bench"), Some(&[][..]));
    }

    #[test]
    fn an_unplaced_crate_is_a_failure_not_a_default() {
        assert!(allowed_internal_deps("nazm-mir").is_none());
    }

    #[test]
    fn nothing_may_use_unsafe_yet() {
        for c in ["nazm-bench", "xtask"] {
            assert!(unsafe_allowed(c).is_none());
        }
    }

    #[test]
    fn the_check_only_path_may_neither_emit_ir_nor_run_the_toolchain() {
        // The separation the emission gate exists to keep. `nazm-core` is where `nazm
        // check` lives, and it must not be able to reach either capability.
        let list = emission_allowlist();
        let find = |name: &str| {
            list.iter()
                .find(|(c, _)| *c == name)
                .map(|(_, a)| *a)
                .expect("every crate must be placed")
        };
        assert!(find("crates/nazm-core").is_empty());
        assert!(find("crates/nazm-syntax").is_empty());
        assert!(find("crates/nazm-diag").is_empty());
        assert!(find("crates/nazm-bench").is_empty());
        assert!(find("crates/nazm-lir").contains(&"emits LLVM IR"));
        assert!(
            !find("crates/nazm-lir").contains(&"invokes the native toolchain"),
            "the emitter should produce text, not run tools"
        );
        assert!(find("crates/nazm-cli").contains(&"invokes the native toolchain"));
    }

    #[test]
    fn the_front_end_may_not_reach_a_code_generator() {
        for c in ["nazm-syntax", "nazm-ty", "nazm-mir", "nazm-bench"] {
            assert!(!may_depend_on_codegen(c), "{c} must not reach codegen");
        }
        assert!(may_depend_on_codegen("nazm-codegen-llvm"));
    }
}
