//! Byte layout, as this backend reads it (`docs/architecture.md` §7.43, §7.82).
//!
//! Since N81 every byte layout and the ownership table are `nazm-lir`'s ([`nazm_lir::abi`]):
//! this backend decides no offset of its own, only which values live in memory.

pub(crate) use nazm_lir::abi::{ByteLayouts as Layouts, Lay, PAYLOAD};
use nazm_lir::Ty;

/// Whether a type is held in memory rather than in a register: a `Str`, a record, an enum.
#[must_use]
pub(crate) const fn in_memory(t: Ty) -> bool {
    matches!(t, Ty::Str | Ty::Record(_) | Ty::Enum(_))
}
