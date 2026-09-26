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
//!   outline and every analysed file's as a searchable list (N22).
//!
//! The first two were `nazm-cli`'s until N16 and moved here unchanged in what they decide,
//! so the command line and the language service cannot disagree about a module graph.
//! Nothing here decides what a program means: that is `nazm-core`'s, reached through
//! [`nazm_core::analyse`], the same parse and check every command runs.

pub mod completion;
pub mod load;
pub mod rename;
pub mod root;
pub mod service;
pub mod signature;
pub mod structure;
pub mod symbols;

pub use completion::{Completion, CompletionCandidate, Context, NoCompletion};
pub use rename::{Edit, FileEdit, Refusal, RenamePlan, Renameable};
pub use service::{Definition, Hover, LanguageService, Location, Published, References, Target};
pub use signature::{
    CallInfo, Help, NoCall, Parameter, Rendered, Signature, SignatureHelp, TypeParameter,
};
pub use structure::{Constructor, ConstructorHelp, Member, NoStructure, Structure, StructureSite};
pub use symbols::{DocumentSymbols, NoSymbols, Symbol, SymbolKind, WorkspaceSymbol};
