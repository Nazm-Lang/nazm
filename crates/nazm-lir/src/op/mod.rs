//! The instruction-level LIR (N105, `docs/architecture.md` §7.106): the one lowering both native
//! backends consume.
//!
//! [`lower_unit`] turns one unit of validated MIR into a [`Module`]; [`validate`] holds it to the
//! rules every backend relies on; [`print`] writes it as text, which is also what its digest is
//! of; [`interp`] executes it, as the oracle the backends are held to.

mod arrays;
mod atomics;
mod bytes;
mod files;
mod func;
mod helpers;
pub mod interp;
pub mod ir;
pub mod llvm;
mod lower;
mod numeric;
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

/// `m` calling the counted heap (R1d, D5; `nazm_runtime::heap`) wherever it calls C's `malloc`,
/// `realloc` or `free`: what a build with `--heap-limit` generates. Only imports are renamed —
/// a module never defines one of C's names.
pub fn count_heap(m: &mut Module) {
    for s in &mut m.symbols {
        if s.linkage != ir::Linkage::Import {
            continue;
        }
        let counted = match s.name.as_str() {
            "malloc" => "nz.heap_malloc",
            "realloc" => "nz.heap_realloc",
            "free" => "nz.heap_free",
            _ => continue,
        };
        counted.clone_into(&mut s.name);
    }
}
