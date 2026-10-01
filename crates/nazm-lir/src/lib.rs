//! Validated MIR laid out and emitted as the LLVM IR text a native build compiles.
//!
//! # Where this sits
//!
//! `docs/architecture.md` §2 describes eight IR levels. Three exist: Core IR (`nazm-cir`, N39),
//! MIR (`nazm-mir`, N40), and the LLVM text written here. This crate reads **validated MIR and
//! nothing of the checker** — it depends on neither `nazm-cir` nor `nazm-core`, and takes no
//! `Resolution` — and what it adds is what MIR deliberately leaves out: layout (canonical by
//! name), symbols spelled from MIR's durable identities, linkage, one unit per module and per
//! generic instance, and the instructions each MIR statement and terminator means
//! (`docs/architecture.md` §7.42).
//!
//! It is named for level 7 and is no longer shaped like anything of its own: until N40 it held
//! a structured typed tree over numbered slots between Core IR and the text, and the emitter
//! decided every exit's cleanup. MIR decides both now; what remains here is level 9's work.
//! Reading the crate list as a progress bar against §2 is what produced four contradictory
//! status claims in September 2026; `docs/master-architecture.md` §3 states the rule that
//! replaced it.
//!
//! # Scope
//!
//! Every checked program except three shapes, each refused by name in [`lower`] with no output:
//! recursion (a call cycle, direct or mutual), a `main` taking anything but capabilities or not
//! returning `Int`, and a task handed an `Ints` or a `Strs`. The authority is [`lower`] and
//! `nazm capabilities`, both read from the implementation; `docs/capability-matrix.md` carries
//! the architectural view.

pub mod backend;
mod debug;
mod emit;
pub mod ir;
pub mod lower;
pub mod names;
pub mod runtime;
pub mod symbol;

pub use emit::{emit_unit, Needs, MEMORY_FORMAT, MEMORY_VARIABLE, STACK_EXHAUSTED, TARGET_NOTE};
pub use ir::{
    ArithOp, CmpOp, EnumLayout, EnumTy, External, Owns, RecordLayout, RecordTy, Ty, Unit,
    VariantLayout,
};
pub use lower::{lower, LowerError, Native, Sig};
pub use names::SourceNames;
pub use runtime::{emit_entry, emit_runtime};
pub use symbol::{Linkage, NativeSymbol};
