//! Core IR: the checked meaning of a program, once.
//!
//! `docs/architecture.md` §7.41 is this crate's constitution, written before it. In one
//! paragraph: a checked program is lowered here exactly once — by `nazm-core`, only after
//! checking succeeded — into typed functions whose names are identities, whose control flow is
//! explicit and whose sugar is gone. The interpreter runs it and the native backend lowers it;
//! neither reads the syntax tree or asks the checker what a position meant.
//!
//! # What is here, and what is not
//!
//! The representation ([`ir`]), its invariants checked ([`verify`]), a deterministic printed
//! form ([`print`], `nazm.core-ir/1`), each function's digests ([`digest`]), and the durable
//! names the last two use ([`names`]). **Not** the lowering: that reads the syntax tree, and
//! nothing about how a program was written may reach this crate — it depends on spans and on
//! `nazm-sema`, the vocabulary of meaning, and on nothing else of the compiler's.
//!
//! # Level 3, by name
//!
//! `docs/architecture.md` §2 calls level 3 "Core" and describes it as ANF with traits as
//! dictionaries. This is level 3 by responsibility — the first representation after checking,
//! typed, with effects explicit — and not by that shape: it is a tree of structured regions,
//! and the language has no traits. `docs/master-architecture.md` §3 states the rule for
//! reading a crate list against that table.

pub mod digest;
pub mod ir;
pub mod names;
pub mod print;
pub mod verify;

pub use digest::{digests, Digests};
pub use ir::{
    ArithOp, Arm, Binding, BitOp, Block, CmpOp, Const, ConstId, Effects, Ends, Expr, ExprKind,
    Function, ImplRow, Init, Law, Local, LocalId, LocalOrigin, LoopId, Match, MatchOrigin,
    PathStep, Program, Step, Stmt, Temp,
};
pub use names::Names;
pub use print::{print, Options};
pub use verify::verify;

/// The printed form's version. Internal: it names what this build writes, and promises no
/// reader outside the workspace that a later build writes the same.
/// `/2` since N78: a call through a type parameter's bound, `call_trait`, which a `/1`
/// reader would misread as a call it does not know.
pub const SCHEMA: &str = "nazm.core-ir/2";
