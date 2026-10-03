//! What a module publishes, in a form that outlives the process that computed it.
//!
//! # Why this is not in `nazm-sema`
//!
//! `Span` taught the rule and N3 applies it a second time: **a semantic type and a wire
//! representation are different responsibilities**, and putting the second inside the
//! first makes every use of the first lossy. A `Span` that could serialise itself dropped
//! its file; a `UnitInterface` that could serialise itself would write `FnId(3)` into a
//! file, and `FnId(3)` means nothing to the run that reads it.
//!
//! So `nazm-sema` has no serde dependency — it cannot serialise anything, by construction
//! and by a `cargo xtask check` gate — and this crate converts explicitly:
//!
//! ```text
//! UnitInterface        in memory: FnId, Span, ModuleId
//!         ↓ published(..)  needs the module's ModuleKey, and fails without one
//! PersistentUnitInterface   durable keys, canonical type names, no spans
//!         ↓ to_json()
//! bytes                deterministic, and the same bytes for the same interface
//!         ↓ InterfaceHash::of()
//! a fingerprint
//! ```
//!
//! # What this is not
//!
//! Not an IR: it holds no expression, no statement and no body. Not a cache: nothing here
//! reads or writes a build directory, decides to reuse anything, or knows what a compiler
//! flag is. It is the artefact a cache would one day be built on, and
//! `capability-matrix.md` says incremental compilation is absent.

pub mod facts;
pub mod fingerprint;
pub mod resolved;
pub mod wire;

pub use facts::PersistentFacts;
pub use fingerprint::InterfaceHash;
pub use resolved::ResolvedUnit;
pub use wire::{
    PersistentC, PersistentExport, PersistentType, PersistentUnitInterface, ReadError, Rehydrated,
    SCHEMA,
};
