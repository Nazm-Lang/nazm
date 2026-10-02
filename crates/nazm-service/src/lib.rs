//! The compiler's driver library. N16.
//!
//! What needs the filesystem and is needed by more than one front door:
//!
//! - [`root`] — where a compilation's durable identities are measured from;
//! - [`load`] — which files a compilation has, read from disk or from open buffers;
//! - [`service`] — the long-lived language service an editor talks to through `nazm lsp`;
//! - [`rename`] — its validated, stale-safe rename plans (N18);
//! - [`completion`] — what may be written at a position, and completion from it (N19);
//! - [`signature`] — the call at a position, as the checker checked it, and signature help
//!   from it (N20);
//! - [`structure`] — the field, variant or label at a position, as the checker identified
//!   its record, enum or variant, and completion and constructor signature help from it
//!   (N21);
//! - [`symbols`] — the declarations the current analyses know, one document's as an
//!   outline and every analysed file's as a searchable list (N22);
//! - [`semantic`] — what each identifier of a document is, as the checker decided it:
//!   class, declaration or reference, and identity (N23);
//! - [`fixes`] — the fixes current diagnostics carry, as edits validated against the
//!   current source, for an editor's quick fixes (N24);
//! - [`hierarchy`] — a source function and the direct calls the checker recorded into and
//!   out of it, in the compilation it was prepared from (N25);
//! - [`context`] — one source definition's exact source and compact semantic links, as a
//!   `nazm.context/3` packet (N27);
//! - [`snapshot`] — one root compilation's durable definitions, with a digest per recorded
//!   semantic section, as `nazm.snapshot/3` (N28);
//! - [`delta`] — a baseline snapshot against the current compilation of its root: what was
//!   added, removed, or changed in which recorded section, as `nazm.delta/3` (N28);
//! - [`patch`] — a validated rename or a compiler-owned fix as minimal exact-byte edits,
//!   bound to the root's snapshot, each file's digest and each edit's expected bytes, as
//!   `nazm.patch/1`; planned, never applied (N29);
//! - [`diagnostics`] — every current diagnostic as a compact, prose-free index, and one
//!   selected diagnostic in full on request, as `nazm.diagnostic-index/1` and
//!   `nazm.diagnostic-detail/1` (N30);
//! - [`summary`] — what a `check` or a `build` concluded, with a reference to each diagnostic
//!   it reported — N30's id where N30 indexes it — as `nazm.command-summary/1` (N31);
//! - [`durable`] — the snapshot's durable definitions with each one's name range, and the
//!   files a compilation loaded, for the repository planner (N33).
//!
//! The first two were `nazm-cli`'s until N16 and moved here unchanged in what they decide,
//! so the command line and the language service cannot disagree about a module graph.
//! Nothing here decides what a program means: that is `nazm-core`'s, reached through
//! [`nazm_core::analyse`], the same parse and check every command runs.

pub mod bounds;
pub mod completion;
pub mod context;
pub mod delta;
pub mod diagnostics;
pub mod durable;
pub mod fixes;
pub mod hierarchy;
mod incomplete;
pub mod load;
pub mod patch;
pub mod profile;
pub mod rename;
pub mod root;
pub mod semantic;
pub mod service;
pub mod signature;
pub mod snapshot;
pub mod structure;
pub mod summary;
pub mod symbols;
pub mod vectorize;

pub use completion::{Completion, CompletionCandidate, Context, NoCompletion};
pub use context::{ContextPacket, ContextResult, NoContext};
pub use delta::{DeltaResult, NoDelta, SemanticDelta};
pub use diagnostics::{
    DetailResult, DiagnosticDetail, DiagnosticIndex, IndexResult, NoDiagnostics,
};
pub use durable::{DurableDefinition, DurableDefinitions};
pub use fixes::{FixPlan, NoFixes};
pub use hierarchy::{HierarchyFunction, IncomingCall, Locator, NoHierarchy, OutgoingCall};
pub use patch::{NoPatch, PatchResult, SemanticPatch};
pub use rename::{Edit, FileEdit, Refusal, RenamePlan, Renameable};
pub use semantic::{
    NoIdentifiers, Role, SemanticClass, SemanticIdentifier, SemanticIdentifiers, SemanticTarget,
};
pub use service::{Definition, Hover, LanguageService, Location, Published, References, Target};
pub use signature::{
    CallInfo, Help, NoCall, Parameter, Rendered, Signature, SignatureHelp, TypeParameter,
};
pub use snapshot::{NoSnapshot, SemanticSnapshot, SnapshotResult};
pub use structure::{Constructor, ConstructorHelp, Member, NoStructure, Structure, StructureSite};
pub use symbols::{DocumentSymbols, NoSymbols, Symbol, SymbolKind, WorkspaceSymbol};
