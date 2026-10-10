//! MIR: each concrete function of a native program as a typed control-flow graph with its
//! ownership and cleanup written out.
//!
//! `docs/architecture.md` §7.42 is this crate's constitution, written before it. In one
//! paragraph: for `nazm build`, after checking and Core IR, every concrete function — each
//! non-generic definition and each generic instance a program needs — is lowered here once into
//! basic blocks whose terminators are explicit, whose managed values are copied, moved and
//! dropped by statements that say so, and whose failing operations name the cleanup their
//! failure takes. The native backend reads MIR and decides layout, symbols and instructions; it
//! makes no language decision.
//!
//! # What is here
//!
//! The representation ([`ir`]), the one lowering from Core IR ([`lower`]) with the instance
//! planner it needs ([`instance`]), the validator ([`verify`]), a canonical text ([`text`]), the
//! printed form ([`print`], `nazm.mir/1`), each function's executable digest ([`digest`]), and
//! the durable names all of them use ([`names`]).
//!
//! # Level 6, by responsibility
//!
//! `docs/architecture.md` §2 describes MIR as a CFG of places with explicit drops and panics,
//! for borrow checking and optimisation. This is that level by responsibility — the first CFG,
//! with ownership and failure explicit — and not an SSA form: locals are mutable, and there are
//! no phi nodes, because nothing that reads MIR needs them.

pub mod cost;
pub mod digest;
pub mod instance;
pub mod ir;
pub mod lower;
pub mod names;
pub mod print;
pub mod text;
pub mod verify;

pub use ir::{
    ArithOp, ArrayInfo, BitOp, Block, BlockId, BlockOrigin, CmpOp, Const, EnumInfo, FieldInfo,
    FieldRef, FuncId, Function, Identity, Law, LocalDecl, LocalId, LocalOrigin, LocalTy, Operand,
    Owner, PathRef, Program, RecordInfo, Rvalue, Statement, StatementKind, Static, Terminator,
    TypeIdentity, Types, VariantInfo, VecInfo,
};
pub use lower::lower;
pub use nazm_sema::{ConvertMode, NumKind};
pub use print::{print, Options};
pub use verify::verify;

/// The printed form's version. Internal: it names what this build writes, and promises no
/// reader outside the workspace that a later build writes the same.
pub const SCHEMA: &str = "nazm.mir/1";
