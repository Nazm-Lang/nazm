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

/// Crates that may depend on a code generator.
///
/// The check this enables: nothing in the front end or the checker may reach Cranelift or
/// LLVM. It is written before either exists, because the rule is cheap now and contentious
/// later.
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
    fn the_front_end_may_not_reach_a_code_generator() {
        for c in ["nazm-syntax", "nazm-ty", "nazm-mir", "nazm-bench"] {
            assert!(!may_depend_on_codegen(c), "{c} must not reach codegen");
        }
        assert!(may_depend_on_codegen("nazm-codegen-llvm"));
    }
}
