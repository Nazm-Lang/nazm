//! Numbers other than `Int` (Gate 2, `docs/general-purpose.md` §1–§3), as instructions.
//!
//! Every operation here means what `nazm_sema::numeric` says, the semantics the interpreter
//! runs: an integer of any width traps where its value leaves its type (`N0400`), divides by
//! zero (`N0401`) or shifts out of `0..width` (`N0412`), with the same sentences; a float is IEEE
//! 754 and never fails. An integer class has no signedness, so each operation chooses the signed
//! or unsigned instruction from the kind.

// The casts write a constant's canonical bits at its class's width, which is what they are.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use nazm_mir as mir;
use nazm_sema::numeric::Wide;
use nazm_sema::{ConvertMode, Intrinsic, NumKind};

use super::func::Fx;
use super::ir::{BinOp, Class, Cmp, ConvOp, Inst, OvfOp, UnOp, V};
use super::lower::num_class;
use crate::ir::Ty;

/// A type's numeric kind: `Int` is `I64`.
#[must_use]
pub(crate) const fn kind_of(t: Ty) -> Option<NumKind> {
    match t {
        Ty::Int => Some(NumKind::I64),
        Ty::Num(k) => Some(k),
        _ => None,
    }
}

/// `2^n` as an `f64`, exactly.
fn pow2(n: u32) -> f64 {
    f64::from(2u32).powi(i32::try_from(n).unwrap_or(0))
}

impl Fx<'_, '_> {
    pub(crate) fn un(&mut self, op: UnOp, a: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Un { dst, op, a });
        dst
    }

    pub(crate) fn conv(&mut self, op: ConvOp, to: Class, src: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Conv { dst, op, to, src });
        dst
    }

    /// A float constant of `class`.
    pub(crate) fn fconst(&mut self, class: Class, value: f64) -> V {
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        let bits = if class == Class::F32 {
            i64::from((value as f32).to_bits())
        } else {
            value.to_bits() as i64
        };
        self.iconst_c(class, bits)
    }

    /// A constant of a numeric kind, from its canonical bits.
    pub(crate) fn num_const(&mut self, kind: NumKind, bits: u128) -> V {
        let c = num_class(kind);
        // A narrow integer is written sign-extended, which every backend reads as its low bits.
        let value = match c {
            Class::I8 => i128::from(bits as u8 as i8),
            Class::I16 => i128::from(bits as u16 as i16),
            Class::I32 | Class::F32 => i128::from(bits as u32 as i32),
            Class::I128 => bits as i128,
            _ => i128::from(bits as u64 as i64),
        };
        self.iconst_wide(c, value)
    }

    /// A bound of an integer kind as a constant of `class`: its two's complement there.
    fn bound(&mut self, class: Class, v: Wide) -> V {
        self.iconst_wide(class, v.bits() as i128)
    }

    /// `x` and `y` stored to fresh slots, for a runtime entry that takes 128-bit values by
    /// address (R1b, L13b); and a slot for its result.
    fn wide_operands(&mut self, x: V, y: Option<V>) -> (V, V, V) {
        let out = self.scratch(16);
        let px = self.scratch(16);
        self.store(px, 0, x);
        let py = match y {
            Some(y) => {
                let py = self.scratch(16);
                self.store(py, 0, y);
                py
            }
            None => px,
        };
        (out, px, py)
    }

    /// `x / y` or `x % y` at 128 bits, guarded already: `nz.i128_divrem`.
    fn wide_divrem(&mut self, op: BinOp, x: V, y: V) -> V {
        let (out, px, py) = self.wide_operands(x, Some(y));
        let code = self.iconst(match op {
            BinOp::SDiv => 0,
            BinOp::SRem => 1,
            BinOp::UDiv => 2,
            _ => 3,
        });
        self.rt("nz.i128_divrem", &[out, px, py, code]);
        self.load(Class::I128, out, 0)
    }

    /// `x * y` at 128 bits, wrapped, and whether the exact product overflowed `kind`:
    /// `nz.i128_mul`.
    fn wide_mul(&mut self, kind: NumKind, x: V, y: V) -> (V, V) {
        let (out, px, py) = self.wide_operands(x, Some(y));
        let signed = self.iconst(i64::from(kind.is_signed()));
        let flag = self.rt_v("nz.i128_mul", &[out, px, py, signed], Class::I32);
        let over = self.cmp_imm(Cmp::Ne, flag, Class::I32, 0);
        (self.load(Class::I128, out, 0), over)
    }

    fn overflows(kind: NumKind, what: &str) -> String {
        format!("this {what} overflows `{}`", kind.name())
    }

    /// `-x`.
    pub(crate) fn num_neg(&mut self, kind: NumKind, x: V) -> V {
        let c = num_class(kind);
        if kind.is_float() {
            return self.un(UnOp::FNeg, x);
        }
        let z = self.iconst_c(c, 0);
        let op = if kind.is_signed() {
            OvfOp::Sub
        } else {
            OvfOp::USub
        };
        self.checked(op, z, x, &Self::overflows(kind, "negation"))
    }

    /// `x op y` on two values of `kind`.
    pub(crate) fn num_arith(&mut self, op: mir::ArithOp, kind: NumKind, x: V, y: V) -> V {
        let c = num_class(kind);
        if kind.is_float() {
            return match op {
                mir::ArithOp::Add => self.bin(BinOp::FAdd, x, y),
                mir::ArithOp::Sub => self.bin(BinOp::FSub, x, y),
                mir::ArithOp::Mul => self.bin(BinOp::FMul, x, y),
                mir::ArithOp::Div => self.bin(BinOp::FDiv, x, y),
                mir::ArithOp::Rem => {
                    let f = if c == Class::F32 { "fmodf" } else { "fmod" };
                    self.rt_v(f, &[x, y], c)
                }
            };
        }
        let signed = kind.is_signed();
        let ovf = |s: OvfOp, u: OvfOp| if signed { s } else { u };
        match op {
            mir::ArithOp::Add => self.checked(
                ovf(OvfOp::Add, OvfOp::UAdd),
                x,
                y,
                &Self::overflows(kind, "addition"),
            ),
            mir::ArithOp::Sub => self.checked(
                ovf(OvfOp::Sub, OvfOp::USub),
                x,
                y,
                &Self::overflows(kind, "subtraction"),
            ),
            mir::ArithOp::Mul if c == Class::I128 => {
                let (product, over) = self.wide_mul(kind, x, y);
                self.guard(over, "N0400", &Self::overflows(kind, "multiplication"));
                product
            }
            mir::ArithOp::Mul => self.checked(
                ovf(OvfOp::Mul, OvfOp::UMul),
                x,
                y,
                &Self::overflows(kind, "multiplication"),
            ),
            mir::ArithOp::Div | mir::ArithOp::Rem => {
                let zero = self.cmp_imm(Cmp::Eq, y, c, 0);
                self.guard(zero, "N0401", "division by zero");
                let divide = |fx: &mut Self, o: BinOp, x: V, y: V| {
                    if c == Class::I128 {
                        fx.wide_divrem(o, x, y)
                    } else {
                        fx.bin(o, x, y)
                    }
                };
                if !signed {
                    let o = if op == mir::ArithOp::Div {
                        BinOp::UDiv
                    } else {
                        BinOp::URem
                    };
                    return divide(self, o, x, y);
                }
                let (lo, _) = kind.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
                let min = self.bound(c, lo);
                let is_min = self.cmp(Cmp::Eq, x, min);
                let is_m1 = self.cmp_imm(Cmp::Eq, y, c, -1);
                let special = self.bin(BinOp::And, is_min, is_m1);
                if op == mir::ArithOp::Div {
                    self.guard(special, "N0400", &Self::overflows(kind, "division"));
                    divide(self, BinOp::SDiv, x, y)
                } else {
                    // `MIN % -1` is 0: divided by 1 instead, which the machine does not fault on.
                    let one = self.iconst_c(c, 1);
                    let divisor = self.select(special, one, y);
                    divide(self, BinOp::SRem, x, divisor)
                }
            }
        }
    }

    /// An `Int` shift or rotation amount checked against `0..bits` (or only `>= 0` for a
    /// rotation), then in the operand's class.
    fn amount(&mut self, kind: NumKind, n: V, rotation: bool) -> V {
        let c = num_class(kind);
        let w = kind.bits();
        let bad = if rotation {
            self.cmp_imm(Cmp::Lt, n, Class::I64, 0)
        } else {
            // Unsigned, so a negative amount is out of range too.
            self.cmp_imm(Cmp::UGe, n, Class::I64, i64::from(w))
        };
        self.guard(
            bad,
            "N0412",
            &format!("this shift amount is outside `0..{w}`"),
        );
        let n = if rotation {
            self.bin_imm_and(n, i64::from(w - 1))
        } else {
            n
        };
        match c {
            Class::I64 => n,
            Class::I128 => self.ext(c, n, false),
            _ => self.trunc(c, n),
        }
    }

    fn bin_imm_and(&mut self, a: V, k: i64) -> V {
        let m = self.iconst(k);
        self.bin(BinOp::And, a, m)
    }

    /// A bit operation; for a shift, `y` is the `Int` amount.
    pub(crate) fn num_bits(&mut self, op: mir::BitOp, kind: NumKind, x: V, y: V) -> V {
        match op {
            mir::BitOp::And => self.bin(BinOp::And, x, y),
            mir::BitOp::Or => self.bin(BinOp::Or, x, y),
            mir::BitOp::Xor => self.bin(BinOp::Xor, x, y),
            mir::BitOp::Shl => {
                let n = self.amount(kind, y, false);
                self.bin(BinOp::Shl, x, n)
            }
            mir::BitOp::Shr => {
                let n = self.amount(kind, y, false);
                let o = if kind.is_signed() {
                    BinOp::AShr
                } else {
                    BinOp::LShr
                };
                self.bin(o, x, n)
            }
        }
    }

    /// `~x`.
    pub(crate) fn num_complement(&mut self, kind: NumKind, x: V) -> V {
        let ones = self.iconst_c(num_class(kind), -1);
        self.bin(BinOp::Xor, x, ones)
    }

    /// A comparison of two values of `kind`.
    pub(crate) fn num_compare(&mut self, op: mir::CmpOp, kind: NumKind, x: V, y: V) -> V {
        let cc = if kind.is_float() {
            match op {
                mir::CmpOp::Eq => Cmp::FEq,
                mir::CmpOp::NotEq => Cmp::FNe,
                mir::CmpOp::Lt => Cmp::FLt,
                mir::CmpOp::LtEq => Cmp::FLe,
                mir::CmpOp::Gt => Cmp::FGt,
                mir::CmpOp::GtEq => Cmp::FGe,
            }
        } else if kind.is_signed() {
            match op {
                mir::CmpOp::Eq => Cmp::Eq,
                mir::CmpOp::NotEq => Cmp::Ne,
                mir::CmpOp::Lt => Cmp::Lt,
                mir::CmpOp::LtEq => Cmp::Le,
                mir::CmpOp::Gt => Cmp::Gt,
                mir::CmpOp::GtEq => Cmp::Ge,
            }
        } else {
            match op {
                mir::CmpOp::Eq => Cmp::Eq,
                mir::CmpOp::NotEq => Cmp::Ne,
                mir::CmpOp::Lt => Cmp::ULt,
                mir::CmpOp::LtEq => Cmp::ULe,
                mir::CmpOp::Gt => Cmp::UGt,
                mir::CmpOp::GtEq => Cmp::UGe,
            }
        };
        self.cmp(cc, x, y)
    }

    /// An integer of `from` widened to `I64` exactly — not for `UInt64`, which has no wider
    /// class.
    fn widen(&mut self, from: NumKind, x: V) -> V {
        if num_class(from) == Class::I64 {
            x
        } else {
            self.ext(Class::I64, x, from.is_signed())
        }
    }

    /// The bits of integer `x` of `from` as an integer of `to`: truncated, or extended by the
    /// source's signedness.
    fn resize(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        let (fc, tc) = (num_class(from), num_class(to));
        if fc == tc {
            x
        } else if fc.bits() > tc.bits() {
            self.trunc(tc, x)
        } else {
            self.ext(tc, x, from.is_signed())
        }
    }

    /// An integer of `from` widened to `I128` exactly — not for `UInt128`, which has no wider
    /// class (R1b, L13b).
    fn widen_wide(&mut self, from: NumKind, x: V) -> V {
        if num_class(from) == Class::I128 {
            x
        } else {
            self.ext(Class::I128, x, from.is_signed())
        }
    }

    /// [`Self::int_fits`] where either side is 128 bits wide.
    fn wide_fits(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        let (lo, hi) = to.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
        if from == NumKind::U128 {
            if to == NumKind::U128 {
                return self.bit(true);
            }
            let max = self.bound(Class::I128, hi);
            return self.cmp(Cmp::ULe, x, max);
        }
        let w = self.widen_wide(from, x);
        let zero = self.iconst_c(Class::I128, 0);
        if to == NumKind::U128 {
            return self.cmp(Cmp::Ge, w, zero);
        }
        let (min, max) = (self.bound(Class::I128, lo), self.bound(Class::I128, hi));
        let above = self.cmp(Cmp::Ge, w, min);
        let below = self.cmp(Cmp::Le, w, max);
        self.bin(BinOp::And, above, below)
    }

    /// [`Self::int_saturate`] where either side is 128 bits wide.
    fn wide_saturate(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        let (lo, hi) = to.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
        let tc = num_class(to);
        if from == NumKind::U128 {
            if to == NumKind::U128 {
                return x;
            }
            let max = self.bound(Class::I128, hi);
            let over = self.cmp(Cmp::UGt, x, max);
            let clamped = self.select(over, max, x);
            return self.resize(NumKind::U128, to, clamped);
        }
        let w = self.widen_wide(from, x);
        let zero = self.iconst_c(Class::I128, 0);
        if to == NumKind::U128 {
            let neg = self.cmp(Cmp::Lt, w, zero);
            return self.select(neg, zero, w);
        }
        let (min, max) = (self.bound(Class::I128, lo), self.bound(Class::I128, hi));
        let under = self.cmp(Cmp::Lt, w, min);
        let over = self.cmp(Cmp::Gt, w, max);
        let a = self.select(under, min, w);
        let b = self.select(over, max, a);
        if tc == Class::I128 {
            b
        } else {
            self.trunc(tc, b)
        }
    }

    /// Whether integer `x` of `from` is a value of integer kind `to`.
    fn int_fits(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        if from.bits() == 128 || to.bits() == 128 {
            return self.wide_fits(from, to, x);
        }
        let (lo, hi) = to.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
        let (lo64, hi64) = (
            lo.to_i128().unwrap_or(0) as i64,
            hi.to_i128().unwrap_or(0) as i64,
        );
        if from == NumKind::U64 {
            // Read unsigned: in range below the target's maximum.
            if to == NumKind::U64 {
                return self.bit(true);
            }
            return self.cmp_imm(Cmp::ULe, x, Class::I64, hi64);
        }
        let w = self.widen(from, x);
        if to == NumKind::U64 {
            return self.cmp_imm(Cmp::Ge, w, Class::I64, 0);
        }
        let above = self.cmp_imm(Cmp::Ge, w, Class::I64, lo64);
        let below = self.cmp_imm(Cmp::Le, w, Class::I64, hi64);
        self.bin(BinOp::And, above, below)
    }

    /// A float of `from` as an `F64`, exactly.
    fn as_f64(&mut self, from: NumKind, x: V) -> V {
        if from == NumKind::F32 {
            self.conv(ConvOp::FExt, Class::F64, x)
        } else {
            x
        }
    }

    /// Whether float `x` truncates to a value of integer kind `to`: finite, and inside the
    /// bounds, which are powers of two and so compared exactly in `f64`.
    fn float_fits(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        let d = self.as_f64(from, x);
        let w = to.bits();
        let (lower, upper) = if to.is_signed() {
            (-pow2(w - 1), pow2(w - 1))
        } else {
            (-1.0, pow2(w))
        };
        // A signed lower bound is inclusive at 64 bits and wider, where `MIN - 1` is not a
        // double, and exclusive of `MIN - 1` below: `-128.5` truncates to `-128`.
        let lo = if to.is_signed() && w < 64 {
            self.fconst(Class::F64, lower - 1.0)
        } else {
            self.fconst(Class::F64, lower)
        };
        let hi = self.fconst(Class::F64, upper);
        let above = if to.is_signed() && w >= 64 {
            self.cmp(Cmp::FGe, d, lo)
        } else {
            self.cmp(Cmp::FGt, d, lo)
        };
        let below = self.cmp(Cmp::FLt, d, hi);
        self.bin(BinOp::And, above, below)
    }

    /// Whether `x` of `from` converts to `to` exactly.
    pub(crate) fn num_fits(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        match (from.is_float(), to.is_float()) {
            (_, true) => self.bit(true),
            (true, false) => self.float_fits(from, to, x),
            (false, false) => self.int_fits(from, to, x),
        }
    }

    /// A float truncated toward zero to an integer of `to`, saturating — which the caller has
    /// either range-checked or wants.
    fn float_to_int(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        if num_class(to) == Class::I128 {
            // By the runtime (R1b, L13b), saturating as the instruction does: `nz.f64_to_i128`.
            let d = self.as_f64(from, x);
            let out = self.scratch(16);
            let signed = self.iconst(i64::from(to.is_signed()));
            self.rt("nz.f64_to_i128", &[out, d, signed]);
            return self.load(Class::I128, out, 0);
        }
        let op = if to.is_signed() {
            ConvOp::FToS
        } else {
            ConvOp::FToU
        };
        self.conv(op, num_class(to), x)
    }

    /// `x` of `from` converted to `to` by `mode`.
    pub(crate) fn num_convert(&mut self, mode: ConvertMode, from: NumKind, to: NumKind, x: V) -> V {
        let tc = num_class(to);
        match (from.is_float(), to.is_float()) {
            (false, true) if num_class(from) == Class::I128 => {
                // Rounded once, from the exact value, by the runtime (R1b, L13b).
                let (_, px, _) = self.wide_operands(x, None);
                let signed = self.iconst(i64::from(from.is_signed()));
                let f = if tc == Class::F32 {
                    "nz.i128_to_f32"
                } else {
                    "nz.i128_to_f64"
                };
                self.rt_v(f, &[px, signed], tc)
            }
            (false, true) => {
                let op = if from.is_signed() {
                    ConvOp::SToF
                } else {
                    ConvOp::UToF
                };
                self.conv(op, tc, x)
            }
            (true, true) => match (from, to) {
                (NumKind::F32, NumKind::F64) => self.conv(ConvOp::FExt, tc, x),
                (NumKind::F64, NumKind::F32) => self.conv(ConvOp::FTrunc, tc, x),
                _ => x,
            },
            (true, false) => {
                if mode != ConvertMode::Saturate {
                    let fits = self.float_fits(from, to, x);
                    let bad = self.not(fits);
                    self.guard(bad, "N0400", &Self::overflows(to, "conversion"));
                }
                self.float_to_int(from, to, x)
            }
            (false, false) => match mode {
                ConvertMode::Wrap => self.resize(from, to, x),
                ConvertMode::Saturate => self.int_saturate(from, to, x),
                ConvertMode::Exact | ConvertMode::Try => {
                    let fits = self.int_fits(from, to, x);
                    let bad = self.not(fits);
                    self.guard(bad, "N0400", &Self::overflows(to, "conversion"));
                    self.resize(from, to, x)
                }
            },
        }
    }

    fn not(&mut self, b: V) -> V {
        let t = self.bit(true);
        self.bin(BinOp::Xor, b, t)
    }

    /// An integer clamped into `to`'s range.
    fn int_saturate(&mut self, from: NumKind, to: NumKind, x: V) -> V {
        if from.bits() == 128 || to.bits() == 128 {
            return self.wide_saturate(from, to, x);
        }
        let (lo, hi) = to.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
        let (lo64, hi64) = (
            lo.to_i128().unwrap_or(0) as i64,
            hi.to_i128().unwrap_or(0) as i64,
        );
        let tc = num_class(to);
        if from == NumKind::U64 {
            if to == NumKind::U64 {
                return x;
            }
            let over = self.cmp_imm(Cmp::UGt, x, Class::I64, hi64);
            let max = self.iconst_c(tc, hi64);
            let low = self.trunc_or_same(tc, x);
            return self.select(over, max, low);
        }
        let w = self.widen(from, x);
        if to == NumKind::U64 {
            let neg = self.cmp_imm(Cmp::Lt, w, Class::I64, 0);
            let zero = self.iconst(0);
            return self.select(neg, zero, w);
        }
        let under = self.cmp_imm(Cmp::Lt, w, Class::I64, lo64);
        let over = self.cmp_imm(Cmp::Gt, w, Class::I64, hi64);
        let min = self.iconst(lo64);
        let max = self.iconst(hi64);
        let a = self.select(under, min, w);
        let b = self.select(over, max, a);
        self.trunc_or_same(tc, b)
    }

    fn trunc_or_same(&mut self, to: Class, x: V) -> V {
        if to == Class::I64 {
            x
        } else {
            self.trunc(to, x)
        }
    }

    /// A numeric built-in (Gate 2), its result; `None` for every other built-in.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn num_primitive(
        &mut self,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) -> Option<V> {
        let kind = elem
            .and_then(kind_of)
            .or_else(|| args.first().map(|a| self.operand_ty(a)).and_then(kind_of));
        let arg = |fx: &mut Self, i: usize| match args.get(i) {
            Some(a) => fx.value(a),
            None => fx.iconst(0),
        };
        let k = kind.unwrap_or(NumKind::I64);
        let c = num_class(k);
        let single = c == Class::F32;
        let libm = |d: &'static str, f: &'static str| if single { f } else { d };
        Some(match op {
            Intrinsic::WrappingAdd | Intrinsic::WrappingSub | Intrinsic::WrappingMul => {
                let (x, y) = (arg(self, 0), arg(self, 1));
                let o = match op {
                    Intrinsic::WrappingAdd => BinOp::Add,
                    Intrinsic::WrappingSub => BinOp::Sub,
                    _ => BinOp::Mul,
                };
                self.bin(o, x, y)
            }
            Intrinsic::SaturatingAdd | Intrinsic::SaturatingSub | Intrinsic::SaturatingMul => {
                let (x, y) = (arg(self, 0), arg(self, 1));
                self.saturating(op, k, x, y)
            }
            Intrinsic::CountOnes | Intrinsic::LeadingZeros | Intrinsic::TrailingZeros => {
                let x = arg(self, 0);
                let u = match op {
                    Intrinsic::CountOnes => UnOp::Popcnt,
                    Intrinsic::LeadingZeros => UnOp::Clz,
                    _ => UnOp::Ctz,
                };
                let r = self.un(u, x);
                match c {
                    Class::I64 => r,
                    Class::I128 => self.trunc(Class::I64, r),
                    _ => self.ext(Class::I64, r, false),
                }
            }
            Intrinsic::RotateLeft | Intrinsic::RotateRight => {
                let (x, n) = (arg(self, 0), arg(self, 1));
                let n = self.amount(k, n, true);
                let o = if op == Intrinsic::RotateLeft {
                    BinOp::Rotl
                } else {
                    BinOp::Rotr
                };
                self.bin(o, x, n)
            }
            Intrinsic::SwapBytes | Intrinsic::ToBigEndian | Intrinsic::FromBigEndian => {
                let x = arg(self, 0);
                self.un(UnOp::Bswap, x)
            }
            Intrinsic::ToLittleEndian | Intrinsic::FromLittleEndian => arg(self, 0),
            Intrinsic::FloatSqrt => {
                let x = arg(self, 0);
                self.un(UnOp::FSqrt, x)
            }
            Intrinsic::FloatAbs => {
                let x = arg(self, 0);
                self.un(UnOp::FAbs, x)
            }
            Intrinsic::FloatFloor => {
                let x = arg(self, 0);
                self.un(UnOp::FFloor, x)
            }
            Intrinsic::FloatCeil => {
                let x = arg(self, 0);
                self.un(UnOp::FCeil, x)
            }
            Intrinsic::FloatTrunc => {
                let x = arg(self, 0);
                self.un(UnOp::FTrunc, x)
            }
            Intrinsic::FloatRound
            | Intrinsic::FloatSin
            | Intrinsic::FloatCos
            | Intrinsic::FloatTan
            | Intrinsic::FloatExp
            | Intrinsic::FloatLn
            | Intrinsic::FloatLog2
            | Intrinsic::FloatLog10 => {
                let x = arg(self, 0);
                let f = match op {
                    Intrinsic::FloatRound => libm("round", "roundf"),
                    Intrinsic::FloatSin => libm("sin", "sinf"),
                    Intrinsic::FloatCos => libm("cos", "cosf"),
                    Intrinsic::FloatTan => libm("tan", "tanf"),
                    Intrinsic::FloatExp => libm("exp", "expf"),
                    Intrinsic::FloatLn => libm("log", "logf"),
                    Intrinsic::FloatLog2 => libm("log2", "log2f"),
                    _ => libm("log10", "log10f"),
                };
                self.rt_v(f, &[x], c)
            }
            Intrinsic::FloatPow
            | Intrinsic::FloatAtan2
            | Intrinsic::FloatMin
            | Intrinsic::FloatMax => {
                let (x, y) = (arg(self, 0), arg(self, 1));
                let f = match op {
                    Intrinsic::FloatPow => libm("pow", "powf"),
                    Intrinsic::FloatAtan2 => libm("atan2", "atan2f"),
                    Intrinsic::FloatMin => libm("fmin", "fminf"),
                    _ => libm("fmax", "fmaxf"),
                };
                self.rt_v(f, &[x, y], c)
            }
            Intrinsic::FloatIsNan => {
                let x = arg(self, 0);
                self.cmp(Cmp::FNe, x, x)
            }
            Intrinsic::FloatIsInfinite | Intrinsic::FloatIsFinite => {
                let x = arg(self, 0);
                let a = self.un(UnOp::FAbs, x);
                let inf = self.fconst(c, f64::INFINITY);
                if op == Intrinsic::FloatIsInfinite {
                    self.cmp(Cmp::FEq, a, inf)
                } else {
                    self.cmp(Cmp::FLt, a, inf)
                }
            }
            // Exact (R1b, L13b): both widened unsigned, and a product that cannot overflow.
            Intrinsic::WideningMul => {
                let (x, y) = (arg(self, 0), arg(self, 1));
                let x = self.ext(Class::I128, x, false);
                let y = self.ext(Class::I128, y, false);
                self.bin(BinOp::Mul, x, y)
            }
            Intrinsic::Float64ToBits | Intrinsic::Float32ToBits => {
                let x = arg(self, 0);
                let to = if op == Intrinsic::Float64ToBits {
                    Class::I64
                } else {
                    Class::I32
                };
                self.conv(ConvOp::Bits, to, x)
            }
            Intrinsic::Float64FromBits | Intrinsic::Float32FromBits => {
                let x = arg(self, 0);
                let to = if op == Intrinsic::Float64FromBits {
                    Class::F64
                } else {
                    Class::F32
                };
                self.conv(ConvOp::Bits, to, x)
            }
            _ => return None,
        })
    }

    /// `saturating_add` and its kin: the checked result, or the bound it overflowed past.
    fn saturating(&mut self, op: Intrinsic, kind: NumKind, x: V, y: V) -> V {
        let c = num_class(kind);
        let (lo, hi) = kind.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
        let (min, max) = (self.bound(c, lo), self.bound(c, hi));
        let signed = kind.is_signed();
        let ovf_op = match (op, signed) {
            (Intrinsic::SaturatingAdd, true) => OvfOp::Add,
            (Intrinsic::SaturatingAdd, false) => OvfOp::UAdd,
            (Intrinsic::SaturatingSub, true) => OvfOp::Sub,
            (Intrinsic::SaturatingSub, false) => OvfOp::USub,
            (_, true) => OvfOp::Mul,
            (_, false) => OvfOp::UMul,
        };
        let (dst, ovf) = if c == Class::I128 && matches!(ovf_op, OvfOp::Mul | OvfOp::UMul) {
            self.wide_mul(kind, x, y)
        } else {
            let dst = self.b.value();
            let ovf = self.b.value();
            self.b.push(Inst::Ovf {
                dst,
                ovf,
                op: ovf_op,
                a: x,
                b: y,
            });
            (dst, ovf)
        };
        // Which bound: unsigned, the maximum but for a subtraction's zero; signed, the sign of
        // the exact result — `y`'s for an addition, `-y`'s for a subtraction, `x * y`'s for a
        // product.
        let bound = if signed {
            let zero = self.iconst_c(c, 0);
            let positive = match op {
                Intrinsic::SaturatingAdd => self.cmp(Cmp::Gt, y, zero),
                Intrinsic::SaturatingSub => self.cmp(Cmp::Lt, y, zero),
                _ => {
                    let xn = self.cmp(Cmp::Lt, x, zero);
                    let yn = self.cmp(Cmp::Lt, y, zero);
                    let differ = self.bin(BinOp::Xor, xn, yn);
                    self.not(differ)
                }
            };
            self.select(positive, max, min)
        } else if op == Intrinsic::SaturatingSub {
            min
        } else {
            max
        };
        self.select(ovf, bound, dst)
    }
}
