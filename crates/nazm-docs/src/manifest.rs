//! Which documents are the corpus, and what authority each has.
//!
//! The one place the allowlist is written. It carries no title, heading or text — only
//! each document's stable id, its path from the repository root, the authority
//! `docs/master-architecture.md` §5 gives it, and which structural reader it takes. The
//! order here is the corpus order: the order of that section's authority table, with
//! `mcp.md` beside `diagnostics.md` and the grammar beside the guide generated from it.
//!
//! Not in it, deliberately: `README.md` (an entry point that summarises and links),
//! `docs/releases/` (frozen evidence for one tested commit each, never updated), and
//! `docs/research/` (a draft protocol with frozen thresholds). Nothing is discovered by
//! walking a directory.

use serde::Serialize;

/// What a document's text is an authority for. A reader must never take one kind for
/// another: a goal is not a capability, and a plan is not a rule of the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    /// Why Nazm exists and where it is ultimately going. Aspiration — never evidence of what
    /// exists.
    Goal,
    /// Durable engineering law: the evidence vocabulary and what must stay true.
    Law,
    /// How the compiler is built.
    Architecture,
    /// What a Nazm program means: the language specification.
    Normative,
    /// The canonical, machine-checked grammar.
    Grammar,
    /// The human guide, part of it generated from the grammar.
    Guide,
    /// A machine-interface contract: schemas, spans, fixes, exit statuses, tools.
    Interface,
    /// What exists today, or what was measured, with its evidence.
    Evidence,
    /// What comes next, and why in that order. Not a rule and not a capability.
    Planning,
    /// Hypotheses with falsifiers. Deliberately not commitments.
    Research,
    /// Containment, resource safety and recovery.
    Operational,
}

/// How a document is read into sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// ATX headings.
    Markdown,
    /// One section per production.
    Ebnf,
}

/// One document of the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Document {
    /// Its stable id, the prefix of every one of its sections' ids.
    pub id: &'static str,
    /// From the repository root, with `/`.
    pub path: &'static str,
    pub authority: Authority,
    pub format: Format,
}

const fn doc(
    id: &'static str,
    path: &'static str,
    authority: Authority,
    format: Format,
) -> Document {
    Document {
        id,
        path,
        authority,
        format,
    }
}

/// Nazm's documentation corpus, in corpus order.
pub const CORPUS: &[Document] = &[
    doc(
        "goals",
        "docs/NAZM_LANGUAGE_GOALS.md",
        Authority::Goal,
        Format::Markdown,
    ),
    doc(
        "master",
        "docs/master-architecture.md",
        Authority::Law,
        Format::Markdown,
    ),
    doc(
        "architecture",
        "docs/architecture.md",
        Authority::Architecture,
        Format::Markdown,
    ),
    doc(
        "spec",
        "docs/spec.md",
        Authority::Normative,
        Format::Markdown,
    ),
    doc(
        "capabilities",
        "docs/capability-matrix.md",
        Authority::Evidence,
        Format::Markdown,
    ),
    doc(
        "roadmap",
        "docs/roadmap.md",
        Authority::Planning,
        Format::Markdown,
    ),
    doc(
        "research",
        "docs/research-register.md",
        Authority::Research,
        Format::Markdown,
    ),
    doc(
        "bootstrap",
        "docs/bootstrap.md",
        Authority::Evidence,
        Format::Markdown,
    ),
    doc(
        "diagnostics",
        "docs/diagnostics.md",
        Authority::Interface,
        Format::Markdown,
    ),
    doc("mcp", "docs/mcp.md", Authority::Interface, Format::Markdown),
    doc(
        "performance",
        "docs/performance.md",
        Authority::Evidence,
        Format::Markdown,
    ),
    doc(
        "runbook",
        "docs/runbook.md",
        Authority::Operational,
        Format::Markdown,
    ),
    doc("guide", "docs/guide.md", Authority::Guide, Format::Markdown),
    doc(
        "grammar",
        "docs/grammar.ebnf",
        Authority::Grammar,
        Format::Ebnf,
    ),
];
