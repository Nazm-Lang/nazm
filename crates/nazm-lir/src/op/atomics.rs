//! `Atomic` (Gate 3, `docs/user/spec.md`, *Atomics*): each built-in as the runtime calls it means.
//!
//! An atomic is `Str`'s triple, `{ptr, len, owner}`: `ptr` is the 8-byte cell, inside a backing
//! of a string's shape that `nz.atomic_cell` allocates and the triple counts — one the memory
//! report does not count as a string (runtime ABI 38) — or a static's, whose owner is null.
//! Every access is one of the runtime's sequentially consistent operations — LIR has no atomic
//! instruction, and a call is the same on both backends. `atomic_add` is a loop of them, so
//! that a sum leaving `Int` stops the program with the cell as it was.

use nazm_mir as mir;
use nazm_sema::Intrinsic;

use super::func::Fx;
use super::ir::{Class, OvfOp};
use super::lower::rt;

impl Fx<'_, '_> {
    /// `dst = op(args)` for an atomic built-in; `false` for any other.
    pub(crate) fn atomics(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        args: &[mir::Operand],
    ) -> bool {
        match op {
            Intrinsic::AtomicNew => {
                // The runtime initialises the cell, in a backing a string's release frees
                // without counting it: an atomic is not a string (runtime ABI 38).
                let v = self.arg(args, 0);
                let owner = self.rt_v("nz.atomic_cell", &[v], Class::Ptr);
                let null = self.is_null(owner);
                self.guard(null, "N0406", "out of memory");
                let cell = self.ptr_off(owner, rt::STRING_HEADER);
                let eight = self.iconst(8);
                self.write_str(dst, cell, eight, owner);
            }
            Intrinsic::AtomicLoad => {
                let cell = self.cell(args);
                let v = self.rt_v("nz.atomic_load", &[cell], Class::I64);
                self.set(dst, v);
            }
            Intrinsic::AtomicStore => {
                let cell = self.cell(args);
                let v = self.arg(args, 1);
                self.rt("nz.atomic_store", &[cell, v]);
                let zero = self.iconst(0);
                self.set(dst, zero);
            }
            Intrinsic::AtomicSwap => {
                let cell = self.cell(args);
                let v = self.arg(args, 1);
                let old = self.rt_v("nz.atomic_swap", &[cell, v], Class::I64);
                self.set(dst, old);
            }
            Intrinsic::AtomicCompareSwap => {
                let cell = self.cell(args);
                let expected = self.arg(args, 1);
                let new = self.arg(args, 2);
                let ok = self.rt_v("nz.atomic_cas", &[cell, expected, new], Class::Bit);
                self.set(dst, ok);
            }
            // Read, add with the overflow check `+` has, and store only if nothing changed the
            // cell in between; otherwise again.
            Intrinsic::AtomicAdd => {
                let cell = self.cell(args);
                let d = self.arg(args, 1);
                let again = self.b.block();
                let done = self.b.block();
                self.jump(again);
                self.b.switch(again);
                let old = self.rt_v("nz.atomic_load", &[cell], Class::I64);
                let sum = self.checked(OvfOp::Add, old, d, "this addition overflows `Int`");
                let stored = self.rt_v("nz.atomic_cas", &[cell, old, sum], Class::Bit);
                self.branch(stored, done, again);
                self.b.switch(done);
                self.set(dst, old);
            }
            _ => return false,
        }
        true
    }

    /// The cell an atomic argument names.
    fn cell(&mut self, args: &[mir::Operand]) -> super::ir::V {
        let a = self.arg_place(args, 0);
        let (cell, _) = self.str_parts(a);
        cell
    }
}
