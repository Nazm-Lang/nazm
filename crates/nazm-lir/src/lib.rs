//! Validated MIR lowered to LIR — the one native lowering — and printed as the LLVM IR text a
//! native build compiles (N105, `docs/architecture.md` §7.106).
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

pub mod abi;
pub mod backend;
mod debug;
pub mod ir;
pub mod lower;
pub mod names;
pub mod op;
pub mod symbol;

pub use nazm_runtime::{Needs, MEMORY_FORMAT, MEMORY_VARIABLE, TARGET_NOTE};

/// What a program sees when a native call stack is exhausted (N49): `N0408`'s message.
pub const STACK_EXHAUSTED: &str = "the native call stack is exhausted";

/// One unit as the LLVM text the default backend compiles, on a 64-bit target: its LIR,
/// lowered, validated and printed (N105). For a caller that measures or inspects the text; a
/// build goes through [`backend::LlvmText`].
///
/// # Panics
///
/// Never on validated MIR: a lowering defect is reported as an empty text and no needs.
#[must_use]
pub fn emit_unit(native: &Native, unit: &Unit, names: &SourceNames<'_>) -> (String, Needs) {
    match op::lower_unit(native, unit, names, abi::DataLayout::LP64, false, false) {
        Ok(m) => (op::llvm::module(&m), m.needs),
        Err(_) => (String::new(), Needs::default()),
    }
}
pub use ir::{
    ArithOp, CmpOp, EnumLayout, EnumTy, External, Owns, RecordLayout, RecordTy, Ty, Unit,
    VariantLayout,
};
pub use lower::{lower, lower_as, LowerError, Native, Sig};
pub use names::SourceNames;
/// The runtime component (N53), which both backends depend on and neither owns.
pub use nazm_runtime as runtime;
pub use nazm_runtime::emit_runtime;

/// The platform entry calling `entry`, `main`'s symbol.
#[must_use]
pub fn emit_entry(entry: &NativeSymbol, roots: usize, needs: Needs) -> String {
    nazm_runtime::emit_entry(&entry.to_string(), roots, needs)
}
pub use symbol::{Linkage, NativeSymbol};

/// Every `Vec` type of a program, by its source spelling, and whether `--layout soa` lays it out
/// one array per field — `None` — or why not (N68, `docs/architecture.md` §7.70). The same rule
/// the emitter applies, stated once for the report.
#[must_use]
pub fn layouts(native: &Native) -> Vec<(String, Option<&'static str>)> {
    let mut out: std::collections::BTreeMap<String, Option<&'static str>> =
        std::collections::BTreeMap::new();
    for u in native.units.iter().chain(native.instances.iter()) {
        for v in &u.vecs {
            let why = match v.elem {
                ir::Ty::Record(r) => match u.records.iter().find(|l| l.id == r) {
                    Some(l) if l.fields.is_empty() => Some("its record has no fields"),
                    Some(l) if l.fields.iter().all(|f| matches!(f, ir::Ty::Int | ir::Ty::Bool)) => None,
                    Some(_) => Some("a field is not an `Int` or a `Bool`: the element owns storage or holds a value of its own layout"),
                    None => Some("its record's layout is not in this unit"),
                },
                _ => Some("its element is not a record"),
            };
            out.entry(v.source.clone()).or_insert(why);
        }
    }
    out.into_iter().collect()
}
