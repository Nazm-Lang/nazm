//! Which of the repository's inputs the map is built from.
//!
//! The one place the repository's Nazm compilations are written down. A Cargo workspace
//! names its own packages and the documentation corpus is `nazm-docs`'s; neither is
//! repeated here. What no other file says is which `.nz` files are compilation roots, and
//! from which directory their durable identities are measured — so this says it, as rows,
//! with nothing discovered by walking a directory.

/// A directory durable module keys are measured from (N3), and the compilation roots in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRoot {
    /// Its stable id: the second part of every module and definition id under it.
    pub id: &'static str,
    /// From the repository root, with `/`, without a trailing one.
    pub dir: &'static str,
    /// The compilation roots, file names in `dir`, in the order a definition's compilation
    /// is chosen: the first root whose compilation loads the definition's module.
    pub roots: &'static [&'static str],
    /// The documentation sections that are its authority, by N32 id.
    pub docs: &'static [&'static str],
}

/// Where each input is, relative to a repository root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub sources: &'static [SourceRoot],
    pub documents: &'static [nazm_docs::Document],
    /// The mutation catalogue, if the repository has one.
    pub catalogue: Option<&'static str>,
    /// The directory of published machine schemas, if the repository has one.
    pub schemas: Option<&'static str>,
}

/// This repository.
pub const NAZM: Layout = Layout {
    sources: SOURCES,
    documents: nazm_docs::CORPUS,
    catalogue: Some("xtask/mutations/mutations.toml"),
    schemas: Some("schema"),
};

/// The compiler written in Nazm, the example programs, and the programs every diagnostic
/// example is. The compiler's four drivers are its roots; its other files are modules.
pub const SOURCES: &[SourceRoot] = &[
    SourceRoot {
        id: "compiler",
        dir: "compiler",
        roots: &["emit.nz", "check.nz", "parser.nz", "lexer.nz"],
        docs: &["bootstrap", "bootstrap:2"],
    },
    SourceRoot {
        id: "examples",
        dir: "examples",
        roots: &[
            "max.nz",
            "pipeline.nz",
            "primes.nz",
            "search.nz",
            "sum.nz",
            "worker-pool.nz",
        ],
        docs: &["guide:a-whole-program"],
    },
    SourceRoot {
        id: "examples-bad",
        dir: "examples/bad",
        roots: &[
            "assign_to_immutable.nz",
            "bare_return.nz",
            "break_as_value.nz",
            "break_outside_loop.nz",
            "condition_not_bool.nz",
            "deep_recursion.nz",
            "division_by_zero.nz",
            "invalid_operands.nz",
            "long_chain.nz",
            "mutable_parameter.nz",
            "mutual_recursion.nz",
            "nesting_too_deep.nz",
            "paren_nesting.nz",
            "return_type_mismatch.nz",
            "runaway_loop.nz",
            "runaway_recursion.nz",
            "undefined_variable.nz",
            "unreachable_after_break.nz",
            "unsupported_construct.nz",
            "wrong_argument_count.nz",
        ],
        docs: &["diagnostics"],
    },
];
