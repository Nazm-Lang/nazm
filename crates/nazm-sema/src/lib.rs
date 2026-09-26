//! What a Nazm program *means*, separately from how it was written.
//!
//! # Why this crate exists
//!
//! `Type`, `Intrinsic` and `Resolution` lived in `nazm-syntax` between 2026-09-21 and
//! 2026-09-22. That was a deliberate and defensible choice at the time: N1 needed one
//! place both the checker and the backend could see, both already depended on the syntax
//! crate, and `xtask/src/rules.rs` forbids the backend depending on the interpreter. A
//! crate created to hold three types that a later milestone might move is worse than a
//! module in an existing one.
//!
//! N2 is the milestone that moved them, and the evidence was in the source rather than in
//! a diagram: **the lexer, the parser, the formatter and the AST referred to those three
//! modules exactly zero times.** They were not syntax, and adding module identity,
//! interfaces and visibility to the same crate would have made "syntax" the name of the
//! place semantic things go.
//!
//! So the split is by responsibility and the dependency runs one way:
//!
//! ```text
//! nazm-span    a file, and a range inside it
//! nazm-syntax  tokens, the tree, the layout            → span, diag
//! nazm-sema    types, built-ins, modules, interfaces,  → span
//!              what a name refers to
//! nazm-core    checking and evaluation                 → span, diag, syntax, sema
//! nazm-lir     lowering and emission                   → span, diag, syntax, sema
//! ```
//!
//! This crate depends on neither `nazm-syntax` nor `nazm-diag`. It holds no AST node and
//! raises no diagnostic; it is the vocabulary a checker uses to say what it decided, and
//! a backend reads that vocabulary without being able to reach the checker.
//!
//! # Everything here is session-local
//!
//! [`ModuleId`], [`FnId`] and the ids inside them are indices valid for **one compilation
//! in one process** — the same rule [`nazm_span::FileId`] states and for the same reason.
//! None of them is derived from content, and none may be persisted, cached, or handed to
//! a build system as a key. What durable identity would have to be is
//! `docs/research-register.md` R2 and R3, and it is deliberately not this.

pub mod enums;
pub mod generic;
pub mod interface;
pub mod intrinsic;
pub mod key;
pub mod module;
pub mod prelude;
pub mod record;
pub mod references;
pub mod resolve;
pub mod scope;
pub mod types;
pub mod udt;

pub use enums::{EnumDef, EnumId, Enums, VariantDef};
pub use interface::{reachable_types, EnumExport, Export, RecordExport, UnitInterface, Visibility};
pub use intrinsic::{Intrinsic, Shape};
pub use key::{DefKey, DefKind, ModuleKey};
pub use module::{Dependency, ModuleGraph, ModuleId};
pub use record::{FieldDef, RecordDef, RecordId, Records, TaskUnsafe};
pub use references::{Coverage, ReferenceIndex, ReferenceTarget};
pub use resolve::{
    CallSite, Callee, CheckedCall, FieldRef, FnDef, FnId, Instantiation, LocalDef, LocalRef,
    PayloadRef, Propagation, Resolution, VariantRef, WrittenType,
};
pub use scope::{Candidate, Named, ScopeTrace, Unavailable, Visible};
pub use types::{ParamOwner, Type, TypeParam, VecId};
pub use udt::{
    CycleEdge, LayoutCycle, NoEquality, OwnershipCycle, Reach, TypeNode, UserTypes, MAX_TYPE_DEPTH,
};
