//! The one representation between the syntax tree and a backend, and the LLVM IR it emits.
//!
//! # Why there is one level here and not eight
//!
//! `docs/internal/architecture.md` §2 describes eight IR levels. This crate is level 7 and nothing
//! else. The levels in between exist to carry information M1a has none of: there are no
//! generics to monomorphise, no traits to lower to dictionaries, no regions to infer, no
//! effects to track, and one scalar type whose layout is not selectable. Building them
//! now would produce a chain of pass-throughs that matches the diagram and says nothing.
//!
//! What this level *does* carry is the thing the AST does not: which operations can fail,
//! and where the source said so. That is the whole reason for a separate representation
//! at this stage — the emitter must not have to re-derive from syntax whether a `/` needs
//! two guards or one.
//!
//! # Scope
//!
//! M1b: a single `fn main() -> Int`; `Int` and `Bool` literals; immutable `let` bindings
//! of either and references to them; unary negation; `+ - * / %`; the comparisons
//! `== != < <= > >=`; parentheses; and `if`/`else` **used as a value**, with branch-local
//! bindings and nesting. Everything else is rejected by name in [`lower`], after the
//! program has passed ordinary checking — never partially compiled.
//!
//! The accepted/rejected table is in `docs/user/roadmap.md` under *M1b subset*.

pub mod emit;
pub mod ir;
pub mod lower;

pub use emit::{emit_llvm, TARGET_NOTE};
pub use ir::{ArithOp, Block, CmpOp, Expr, Function, Local, Module, Stmt, Ty};
pub use lower::{lower, LowerError};
