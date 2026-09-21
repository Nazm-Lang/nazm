//! The one representation between the syntax tree and a backend, and the LLVM IR it emits.
//!
//! # Why there is one level here and not eight
//!
//! `docs/architecture.md` §2 describes eight IR levels. This crate is the only one that
//! exists. The levels in between exist to carry information the language has none of:
//! there are no generics to monomorphise, no traits to lower to dictionaries, no regions
//! to infer, and no effects to track. Building them now would produce a chain of
//! pass-throughs that matches the diagram and says nothing.
//!
//! **It is named for level 7, and it is not shaped like level 7.** §2 describes that level
//! as SSA with concrete layouts over a CFG of places; what is here is a structured typed
//! tree over numbered slots — `While`, `If` and `Scope` nest, and nothing is in SSA form.
//! Reading the crate list as a progress bar against §2 is what produced four contradictory
//! status claims in September 2026; `docs/master-architecture.md` §3 states the rule that
//! replaced it.
//!
//! What this level *does* carry is the thing the AST does not: which operations can fail,
//! and where the source said so. That is the whole reason for a separate representation
//! at this stage — the emitter must not have to re-derive from syntax whether a `/` needs
//! two guards or one.
//!
//! # Scope
//!
//! The bootstrap subset: functions over `Int`, `Bool`, `Str`, `Ints`, `Strs` and `Chan`;
//! `let` and `let mut` and assignment; `while` with `break` and `continue`; `return`;
//! `if`/`else` in **both** statement and value position; non-recursive direct calls; all
//! 31 built-ins; modules; and structured concurrency. Everything else — recursion, a bare
//! block used as an expression, a `main` with parameters — is rejected by name in
//! [`lower`], after the program has passed ordinary checking, and never partially
//! compiled.
//!
//! This paragraph described the M1b subset (one `fn main() -> Int` and scalar arithmetic)
//! until 2026-09-21, long after the code grew past it. The authority is
//! `SUBSET` in [`lower`] and `nazm capabilities`, both read from the implementation;
//! `docs/capability-matrix.md` carries the architectural view.

pub mod emit;
pub mod ir;
pub mod lower;

pub use emit::{emit_llvm, TARGET_NOTE};
pub use ir::{ArithOp, Block, CmpOp, Expr, Function, Local, Module, Stmt, Ty};
pub use lower::{lower, LowerError};
