//! The instruction-level LIR (N105, `docs/architecture.md` §7.106): the one lowering both native
//! backends consume.
//!
//! [`lower_unit`] turns one unit of validated MIR into a [`Module`]; [`validate`] holds it to the
//! rules every backend relies on; [`print`] writes it as text, which is also what its digest is
//! of; [`interp`] executes it, as the oracle the backends are held to.

mod func;
mod helpers;
pub mod interp;
pub mod ir;
pub mod llvm;
mod lower;
mod prims;
pub mod print;
mod soa;
pub mod validate;

pub use ir::Module;
pub use lower::{class, in_memory, lower_unit, rt};

/// A module's digest: what its text is, and so what any object generated from it is a function of.
#[must_use]
pub fn digest(m: &Module) -> String {
    blake3::hash(print::module(m).as_bytes())
        .to_hex()
        .to_string()
}
