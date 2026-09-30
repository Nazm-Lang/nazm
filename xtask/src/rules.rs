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
        // What a program *means*, separately from how it was written. It depends on
        // spans and nothing else — deliberately not on `nazm-syntax`, so that the
        // vocabulary the checker uses to record its decisions cannot start describing
        // the tree those decisions were read off. See the crate documentation for the
        // source-level evidence that moved these types out of the syntax crate in N2.
        "nazm-sema" => &["nazm-span"],
        // What a module publishes, durably. It sits above `nazm-sema` and holds the serde
        // this project deliberately keeps out of it: a crate with no serde dependency
        // cannot serialise a session-local id by accident, and the `sema purity` gate
        // makes that a build failure rather than a convention.
        "nazm-iface" => &["nazm-span", "nazm-sema"],
        // What semantic checking depends on, and a store for results keyed on it.
        // Deliberately **not** `nazm-core`: it cannot check anything, so it cannot become
        // a second place where checking happens. The driver joins the two.
        "nazm-cache" => &["nazm-sema", "nazm-iface"],
        // Core IR, N39: level 3 of the table in `docs/architecture.md` §2 by responsibility.
        // Spans and meaning, and deliberately nothing that describes how a program was
        // written: not `nazm-syntax`, and not `nazm-core`, where the one lowering to it lives
        // behind checking. `docs/architecture.md` §7.41.
        "nazm-cir" => &["nazm-span", "nazm-sema"],
        "nazm-core" => &[
            "nazm-span",
            "nazm-diag",
            "nazm-syntax",
            "nazm-sema",
            "nazm-cir",
        ],
        // Level 7 of the table in `docs/architecture.md` §2. It lowers **Core IR** — not the
        // syntax tree, which it cannot see since N39 — and produces LLVM IR *as text*, so it
        // needs no code-generator dependency, which is why `may_depend_on_codegen` below
        // stays false for it.
        // MIR, N40: level 6 of the table in `docs/architecture.md` §2 by responsibility. It is
        // lowered from Core IR and the checker's tables, and describes nothing about how a
        // program was written. `docs/architecture.md` §7.42.
        "nazm-mir" => &["nazm-span", "nazm-diag", "nazm-sema", "nazm-cir"],
        "nazm-lir" => &["nazm-span", "nazm-diag", "nazm-sema", "nazm-mir"],
        // The Cranelift backend, N41: validated MIR and the backend boundary `nazm-lir` holds,
        // and nothing above them — not Core IR, not the checker, not the syntax tree. The one
        // crate besides `xtask` that may depend on a code generator (`may_depend_on_codegen`).
        "nazm-codegen-clif" => &[
            "nazm-span",
            "nazm-diag",
            "nazm-sema",
            "nazm-mir",
            "nazm-lir",
        ],
        // The driver library and the language service, N16. Through `nazm-core` only, like
        // the CLI and for the same reason: the service consumes the compiler's answers,
        // and a direct dependency on the parser or on `nazm-sema` would be the first step
        // to it computing its own. Not `nazm-cache` either: an unsaved buffer is not state
        // the cache may record.
        "nazm-service" => &["nazm-span", "nazm-diag", "nazm-core", "nazm-package"],
        "nazm-package" => &["nazm-span", "nazm-diag"],
        // Nazm's own documentation as sections, N32. It reads the corpus's files and nothing
        // of the compiler: answering a documentation query must not need a parse, a check
        // or a backend.
        "nazm-docs" => &[],
        // The repository map and the task planner, N33: an index over the authorities that
        // exist. It reads the compiler's facts through the service, and the documentation
        // through `nazm-docs` — never a parser, a checker or a reader of its own.
        "nazm-repo" => &["nazm-core", "nazm-service", "nazm-docs"],
        // Token measurement, N34: a tool beside the compiler. Its library depends on no
        // internal crate; these four are the benchmark's dev-dependencies, which Cargo's
        // metadata lists with the rest. No crate may depend on it — it is in no list here —
        // so no tokenizer can reach parsing, checking, identity or selection.
        "nazm-tokens" => &["nazm-core", "nazm-service", "nazm-docs", "nazm-repo"],
        // The agent task benchmark, N35: evaluation tooling. It reads the planner's answer and
        // the authorities its oracles score against, and counts locally through
        // `nazm-tokens` — the one crate allowed to (`TOKEN_EVALUATORS`). No crate may depend on
        // it, so no provider, price or model answer can reach the language.
        "nazm-agent-bench" => &[
            "nazm-core",
            "nazm-service",
            "nazm-docs",
            "nazm-repo",
            "nazm-tokens",
        ],
        // The CLI deliberately does NOT depend on nazm-syntax: it goes through nazm-core,
        // so there is one entry point into the front end rather than two that can drift.
        "nazm-cli" => &[
            "nazm-span",
            "nazm-diag",
            "nazm-core",
            "nazm-lir",
            "nazm-mir",
            "nazm-codegen-clif",
            "nazm-iface",
            "nazm-cache",
            "nazm-service",
            "nazm-package",
            "nazm-docs",
            "nazm-repo",
        ],

        // A read-only Model Context Protocol server, N27. It serialises what the language
        // service answers and nothing else: no parser, no checker of its own, no cache.
        "nazm-mcp" => &["nazm-core", "nazm-service", "nazm-docs", "nazm-repo"],
        // The measurement instrument answers to nothing. It must stay usable — and
        // auditable — with no compiler present, because it has to run against arms A1 and
        // A2 before Nazm exists, and its results have to be defensible independently of it.
        "nazm-bench" => &[],
        // Automation is allowed to know about everything, and nothing may depend on it.
        // The one exception is the corpus gate, which validates the documentation with the
        // reader every other consumer uses rather than a second one.
        "xtask" => &["nazm-docs"],
        _ => return None,
    };
    Some(deps)
}

/// The tokenizer libraries, and the one crate that may depend on them (N34). A count is a
/// measurement of text; nothing that decides what a program means or what a context holds
/// may reach one.
pub(crate) const TOKENIZER_CRATES: &[&str] = &["tiktoken-rs", "tokenizers"];

/// The only crate that may depend on a tokenizer library.
pub(crate) const TOKENIZER_OWNER: &str = "nazm-tokens";

/// The evaluators that may depend on [`TOKENIZER_OWNER`] (N35). Nothing may depend on them.
pub(crate) const TOKEN_EVALUATORS: &[&str] = &["nazm-agent-bench"];

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
        ("crates/nazm-cache", &[]),
        // Core IR is a representation; it emits nothing and runs nothing.
        ("crates/nazm-cir", &[]),
        ("crates/nazm-mir", &[]),
        ("crates/nazm-lir", &["emits LLVM IR"]),
        // Object code, in process; it never runs a tool and never writes LLVM IR.
        ("crates/nazm-codegen-clif", &[]),
        // The language service answers questions; it never emits or runs anything.
        ("crates/nazm-service", &[]),
        // Nor does the repository planner, which only composes the service's answers.
        ("crates/nazm-repo", &[]),
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
    fn the_backend_cannot_see_the_syntax_tree_and_core_ir_cannot_see_the_compiler() {
        // N39: Core IR is the one lowering, and everything below it starts from it. The
        // native backend reading the tree again would be a second lowering; Core IR reaching
        // the parser or the checker would make it describe how a program was written.
        //
        // N40: the backend reads MIR and nothing above it — not Core IR, not the checker —
        // and MIR is lowered from Core IR, never from the tree.
        let lir = allowed_internal_deps("nazm-lir").unwrap();
        assert!(!lir.contains(&"nazm-syntax") && !lir.contains(&"nazm-core"));
        assert!(!lir.contains(&"nazm-cir"));
        assert!(lir.contains(&"nazm-mir"));
        let mir = allowed_internal_deps("nazm-mir").unwrap();
        assert!(mir.contains(&"nazm-cir"));
        assert!(!mir.contains(&"nazm-syntax") && !mir.contains(&"nazm-core"));
        assert_eq!(
            allowed_internal_deps("nazm-cir"),
            Some(&["nazm-span", "nazm-sema"][..])
        );
    }

    #[test]
    fn an_unplaced_crate_is_a_failure_not_a_default() {
        assert!(allowed_internal_deps("nazm-codegen-llvm").is_none());
    }

    #[test]
    fn the_cranelift_backend_reads_mir_and_nothing_above_it() {
        let clif = allowed_internal_deps("nazm-codegen-clif").unwrap();
        for above in ["nazm-cir", "nazm-core", "nazm-syntax", "nazm-service"] {
            assert!(!clif.contains(&above), "{above}");
        }
        assert!(clif.contains(&"nazm-mir") && clif.contains(&"nazm-lir"));
        assert!(may_depend_on_codegen("nazm-codegen-clif"));
        assert!(!may_depend_on_codegen("nazm-lir") && !may_depend_on_codegen("nazm-cli"));
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
    fn only_an_evaluator_reaches_token_measurement_and_nothing_reaches_an_evaluator() {
        for c in [
            "nazm-span",
            "nazm-diag",
            "nazm-syntax",
            "nazm-sema",
            "nazm-iface",
            "nazm-cache",
            "nazm-cir",
            "nazm-core",
            "nazm-mir",
            "nazm-lir",
            "nazm-codegen-clif",
            "nazm-service",
            "nazm-docs",
            "nazm-repo",
            "nazm-cli",
            "nazm-mcp",
            "nazm-bench",
            "xtask",
            "nazm-tokens",
            "nazm-agent-bench",
        ] {
            let deps = allowed_internal_deps(c).expect("every crate is placed");
            assert!(!deps.contains(&"nazm-agent-bench"), "{c}");
            assert_eq!(
                deps.contains(&TOKENIZER_OWNER),
                TOKEN_EVALUATORS.contains(&c),
                "{c}"
            );
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
