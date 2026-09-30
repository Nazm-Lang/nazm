//! Core IR lowered to the representation a backend reads, and the LLVM IR it emits.
//!
//! # Where this sits
//!
//! `docs/architecture.md` §2 describes eight IR levels. Two exist: Core IR (`nazm-cir`, since
//! N39), the one lowering of a checked program, and this one, which lowers it. This crate
//! reads Core IR and the checker's semantic tables and never the syntax tree — it does not
//! depend on `nazm-syntax` — and what it adds is what Core IR deliberately leaves out: layout,
//! symbols, linkage, the hidden slots that let a partly built value be released, and one
//! concrete instance per generic function a program needs. The levels between the two still
//! exist to carry what the language has none of: no traits to lower to dictionaries, no
//! regions to infer. Building them now would produce pass-throughs that match the diagram and
//! say nothing.
//!
//! **It is named for level 7, and it is not shaped like level 7.** §2 describes that level
//! as SSA with concrete layouts over a CFG of places; what is here is a structured typed
//! tree over numbered slots — `While`, `If` and `Scope` nest, and nothing is in SSA form.
//! Reading the crate list as a progress bar against §2 is what produced four contradictory
//! status claims in September 2026; `docs/master-architecture.md` §3 states the rule that
//! replaced it.
//!
//! What this level *does* carry is the thing Core IR does not: which operations can fail at
//! the machine level, and where the source said so. That is the whole reason for a separate
//! representation at this stage — the emitter must not have to re-derive whether a `/` needs
//! two guards or one.
//!
//! # Scope
//!
//! The bootstrap subset: functions over `Int`, `Bool`, `Str`, `Ints`, `Strs` and `Chan`;
//! `let` and `let mut` and assignment; `while` with `break` and `continue`; `return`;
//! `if`/`else` in **both** statement and value position; non-recursive direct calls; all
//! 31 built-ins; modules; and structured concurrency — and records, enums with `match`,
//! generics, and a block used as a value in any position (N12.1). Everything else —
//! recursion, a `main` with parameters — is rejected by name in
//! [`lower`], after the program has passed ordinary checking, and never partially
//! compiled.
//!
//! This paragraph described the M1b subset (one `fn main() -> Int` and scalar arithmetic)
//! until 2026-09-21, long after the code grew past it. The authority is
//! `SUBSET` in [`lower`] and `nazm capabilities`, both read from the implementation;
//! `docs/capability-matrix.md` carries the architectural view.

pub mod emit;
pub mod instance;
pub mod ir;
pub mod lower;
pub mod names;
pub mod symbol;

pub use emit::{
    emit_entry, emit_runtime, emit_unit, Needs, MEMORY_FORMAT, MEMORY_VARIABLE, TARGET_NOTE,
};
pub use ir::{
    ArithOp, Block, CmpOp, EnumLayout, EnumTy, Expr, External, Function, Local, MatchArm, Native,
    Owns, RecordLayout, RecordTy, StatementArm, Stmt, Ty, Unit, VariantLayout,
};
pub use lower::{lower, lower_module, plan, LowerError, Lowered, Plan};
pub use names::SourceNames;
pub use symbol::{Linkage, NativeSymbol};
