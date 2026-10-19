//! What every numeric operation means (Gate 2, `docs/internal/general-purpose.md` §1–§3), as one Rust
//! implementation both interpreters call.
//!
//! The native backends implement the same operations in machine instructions and the runtime,
//! and the differential tests hold them to this. A value is its kind and 128 bits: an integer
//! sign- or zero-extended to 128 bits by its kind, a `Float64` as its IEEE bits, a `Float32` as
//! its IEEE bits in the low 32. Canonical by construction — every function here returns a
//! value in that form — so two equal values have equal bits, NaNs aside. Since R1b (L13b) an
//! integer's exact value is a [`Wide`], which holds every kind's range, `UInt128`'s included.

// Numbers are what this module is about: exact float comparison is IEEE 754's equality, the casts
// are the conversions it defines with their ranges checked, and a width's byte count is its bits
// divided by eight.
#![allow(
    clippy::float_cmp,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use crate::types::{ConvertMode, NumKind};

/// A constant's value (Gate 2, `docs/user/spec.md`, *Constants*): what the checker computed, and what
/// every implementation then uses as a literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConstValue {
    /// A number of any numeric type, `Int` included.
    Num(NumVal),
    Bool(bool),
    Str(Vec<u8>),
    /// A record or tuple (R1a, L8): its fields' values in declaration order.
    Record {
        ty: crate::types::Type,
        fields: Vec<ConstValue>,
    },
    /// An enum value (L8): its variant by declaration index, and the payload in declaration
    /// order.
    Variant {
        ty: crate::types::Type,
        variant: u32,
        fields: Vec<ConstValue>,
    },
    /// An array (L8): its elements in order.
    Array {
        ty: crate::types::Type,
        elems: Vec<ConstValue>,
    },
}

impl ConstValue {
    /// Its type.
    #[must_use]
    pub fn ty(&self) -> crate::types::Type {
        match self {
            Self::Num(n) => crate::types::Type::of_num(n.kind),
            Self::Bool(_) => crate::types::Type::Bool,
            Self::Str(_) => crate::types::Type::Str,
            Self::Record { ty, .. } | Self::Variant { ty, .. } | Self::Array { ty, .. } => *ty,
        }
    }

    /// The value as an `Int`, for an array's length.
    #[must_use]
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Self::Num(n) if n.kind == NumKind::I64 => Some(n.as_i128()),
            _ => None,
        }
    }
}

/// A numeric value: its kind and its canonical bits.
///
/// Aligned to 8, not to `u128`'s 16 (R1b, L13b): 24 bytes rather than 32, so a `Value` holding
/// one is no larger than before 128-bit integers, nor is every interpreter frame that holds
/// values — which `STACK_BYTES_PER_CALL` budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(Rust, packed(8))]
pub struct NumVal {
    pub kind: NumKind,
    pub bits: u128,
}

/// An exact integer in `[-2^127, 2^128 - 1]`: every value of every integer kind, `Int128`'s and
/// `UInt128`'s together (R1b, L13b), as a sign and a magnitude. An operation past that range
/// is past every kind's range too, and gives `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wide {
    neg: bool,
    mag: u128,
}

impl Wide {
    pub const ZERO: Self = Self { neg: false, mag: 0 };

    /// Zero is never negative, so equal values are equal structs.
    const fn new(neg: bool, mag: u128) -> Self {
        Self {
            neg: neg && mag != 0,
            mag,
        }
    }

    #[must_use]
    pub const fn from_i128(v: i128) -> Self {
        Self::new(v < 0, v.unsigned_abs())
    }

    #[must_use]
    pub const fn from_u128(v: u128) -> Self {
        Self::new(false, v)
    }

    /// The value as an `i128`, if it is one.
    #[must_use]
    pub fn to_i128(self) -> Option<i128> {
        if self.neg {
            // `2^127` itself is `i128::MIN`, which `wrapping_neg` leaves alone.
            (self.mag <= 1 << 127).then(|| (self.mag as i128).wrapping_neg())
        } else {
            i128::try_from(self.mag).ok()
        }
    }

    /// The value as a `u128`, if it is one.
    #[must_use]
    pub const fn to_u128(self) -> Option<u128> {
        if self.neg {
            None
        } else {
            Some(self.mag)
        }
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.neg
    }

    /// The low 128 bits of the value in two's complement.
    #[must_use]
    pub const fn bits(self) -> u128 {
        if self.neg {
            self.mag.wrapping_neg()
        } else {
            self.mag
        }
    }

    #[must_use]
    pub const fn negate(self) -> Self {
        Self::new(!self.neg, self.mag)
    }

    #[must_use]
    pub fn checked_add(self, o: Self) -> Option<Self> {
        Some(if self.neg == o.neg {
            Self::new(self.neg, self.mag.checked_add(o.mag)?)
        } else if self.mag >= o.mag {
            Self::new(self.neg, self.mag - o.mag)
        } else {
            Self::new(o.neg, o.mag - self.mag)
        })
    }

    #[must_use]
    pub fn checked_sub(self, o: Self) -> Option<Self> {
        self.checked_add(o.negate())
    }

    #[must_use]
    pub fn checked_mul(self, o: Self) -> Option<Self> {
        Some(Self::new(self.neg != o.neg, self.mag.checked_mul(o.mag)?))
    }

    /// Truncating division; `o` is not zero.
    #[must_use]
    pub fn quotient(self, o: Self) -> Self {
        Self::new(self.neg != o.neg, self.mag / o.mag)
    }

    /// The remainder of truncating division, with the dividend's sign; `o` is not zero.
    #[must_use]
    pub fn remainder(self, o: Self) -> Self {
        Self::new(self.neg, self.mag % o.mag)
    }

    /// Rounded to nearest, ties to even — once, from the exact value.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        let m = self.mag as f64;
        if self.neg {
            -m
        } else {
            m
        }
    }

    /// Rounded to nearest, ties to even — once, from the exact value, never through `f64`.
    #[must_use]
    pub fn to_f32(self) -> f32 {
        let m = self.mag as f32;
        if self.neg {
            -m
        } else {
            m
        }
    }
}

impl From<i128> for Wide {
    fn from(v: i128) -> Self {
        Self::from_i128(v)
    }
}

impl From<u128> for Wide {
    fn from(v: u128) -> Self {
        Self::from_u128(v)
    }
}

impl From<i64> for Wide {
    fn from(v: i64) -> Self {
        Self::from_i128(i128::from(v))
    }
}

impl Ord for Wide {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        match (self.neg, o.neg) {
            (false, true) => std::cmp::Ordering::Greater,
            (true, false) => std::cmp::Ordering::Less,
            (false, false) => self.mag.cmp(&o.mag),
            (true, true) => o.mag.cmp(&self.mag),
        }
    }
}

impl PartialOrd for Wide {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl std::fmt::Display for Wide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.neg {
            write!(f, "-{}", self.mag)
        } else {
            write!(f, "{}", self.mag)
        }
    }
}

/// Why a numeric operation stops the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    /// `N0400`: the result does not fit the type, or a conversion's value does not.
    Overflow,
    /// `N0401`: an integer division or remainder by zero.
    DivideByZero,
    /// `N0408`: a shift or rotation by a negative amount, or a shift by the width or more.
    ShiftRange,
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// A bit operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit {
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

/// A comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

/// `2^(bits)` as an `f64`, exactly: every power of two this file needs is representable.
fn pow2(bits: u32) -> f64 {
    f64::from(2u32).powi(i32::try_from(bits).unwrap_or(0))
}

impl NumVal {
    /// An integer value of `kind`, or `None` when `v` is outside its range.
    #[must_use]
    pub fn int(kind: NumKind, v: impl Into<Wide>) -> Option<Self> {
        let v = v.into();
        let (lo, hi) = kind.int_range()?;
        if v < lo || v > hi {
            return None;
        }
        // Two's complement in 128 bits: for a signed kind its sign extension, and for an
        // unsigned one, never negative in range, its zero extension.
        Some(Self {
            kind,
            bits: v.bits(),
        })
    }

    /// A `Float64`.
    #[must_use]
    pub fn f64(v: f64) -> Self {
        Self {
            kind: NumKind::F64,
            bits: u128::from(v.to_bits()),
        }
    }

    /// A `Float32`.
    #[must_use]
    pub fn f32(v: f32) -> Self {
        Self {
            kind: NumKind::F32,
            bits: u128::from(v.to_bits()),
        }
    }

    /// An integer's exact value; zero for a float.
    #[must_use]
    pub fn value(self) -> Wide {
        if self.kind.is_float() {
            Wide::ZERO
        } else if self.kind.is_signed() {
            Wide::from_i128(self.bits as i128)
        } else {
            Wide::from_u128(self.bits)
        }
    }

    /// An integer's value as an `i128`; `0` for a float. Every kind's but `UInt128`'s fits, and
    /// a `UInt128` past `i128::MAX` reads as its two's complement.
    #[must_use]
    pub fn as_i128(self) -> i128 {
        if self.kind.is_float() {
            0
        } else {
            self.bits as i128
        }
    }

    /// A float's value, widened to `f64` exactly; an integer's, rounded.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self.kind {
            NumKind::F64 => f64::from_bits(self.bits as u64),
            NumKind::F32 => f64::from(f32::from_bits(self.bits as u32)),
            _ => self.value().to_f64(),
        }
    }

    /// A `Float32`'s own value.
    #[must_use]
    pub fn as_f32(self) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        f32::from_bits(self.bits as u32)
    }

    /// The value `bits` hold as `kind`, put into canonical form — the low `width` bits,
    /// extended by the kind's signedness. For a wrapping result.
    #[must_use]
    pub fn wrapped(kind: NumKind, bits: u128) -> Self {
        let w = kind.bits();
        let bits = if w == 128 {
            bits
        } else if kind.is_float() {
            bits & ((1u128 << w) - 1)
        } else {
            let low = bits & ((1u128 << w) - 1);
            if kind.is_signed() && low >> (w - 1) == 1 {
                low | !((1u128 << w) - 1)
            } else {
                low
            }
        };
        Self { kind, bits }
    }

    /// An integer result: the value if it fits, else [`NumError::Overflow`].
    fn checked(kind: NumKind, v: Option<Wide>) -> Result<Self, NumError> {
        v.and_then(|v| Self::int(kind, v)).ok_or(NumError::Overflow)
    }
}

/// `a op b` for two values of one kind.
///
/// # Errors
///
/// Overflow and division by zero, for integers; floats never fail.
pub fn arith(op: Arith, a: NumVal, b: NumVal) -> Result<NumVal, NumError> {
    let k = a.kind;
    if k == NumKind::F32 {
        // In `f32` itself, so each result is rounded once, as the hardware rounds it.
        let (x, y) = (a.as_f32(), b.as_f32());
        return Ok(NumVal::f32(match op {
            Arith::Add => x + y,
            Arith::Sub => x - y,
            Arith::Mul => x * y,
            Arith::Div => x / y,
            Arith::Rem => x % y,
        }));
    }
    if k == NumKind::F64 {
        let (x, y) = (a.as_f64(), b.as_f64());
        return Ok(NumVal::f64(match op {
            Arith::Add => x + y,
            Arith::Sub => x - y,
            Arith::Mul => x * y,
            Arith::Div => x / y,
            Arith::Rem => x % y,
        }));
    }
    let (x, y) = (a.value(), b.value());
    // A result past `Wide` is past every range (Q1-B-03: `UInt64::MAX` squared no longer
    // escapes, and `UInt128::MAX` squared is out of `Wide` too).
    let v = match op {
        Arith::Add => x.checked_add(y),
        Arith::Sub => x.checked_sub(y),
        Arith::Mul => x.checked_mul(y),
        Arith::Div | Arith::Rem if y == Wide::ZERO => return Err(NumError::DivideByZero),
        // Truncating; `MIN / -1` is out of range and so is reported as overflow, and
        // `MIN % -1` is 0.
        Arith::Div => Some(x.quotient(y)),
        Arith::Rem => Some(x.remainder(y)),
    };
    NumVal::checked(k, v)
}

/// `-a`. An unsigned value other than zero has no negation and overflows.
///
/// # Errors
///
/// Overflow.
pub fn neg(a: NumVal) -> Result<NumVal, NumError> {
    match a.kind {
        NumKind::F64 => Ok(NumVal::f64(-a.as_f64())),
        NumKind::F32 => Ok(NumVal::f32(-a.as_f32())),
        k => NumVal::checked(k, Some(a.value().negate())),
    }
}

/// `a op b` for the bit operators; for a shift, `b` is the amount, an `Int`.
///
/// # Errors
///
/// [`NumError::ShiftRange`] for a shift by a negative amount or by the width or more.
pub fn bit(op: Bit, a: NumVal, b: NumVal) -> Result<NumVal, NumError> {
    let k = a.kind;
    let w = k.bits();
    match op {
        Bit::And => Ok(NumVal::wrapped(k, a.bits & b.bits)),
        Bit::Or => Ok(NumVal::wrapped(k, a.bits | b.bits)),
        Bit::Xor => Ok(NumVal::wrapped(k, a.bits ^ b.bits)),
        Bit::Shl | Bit::Shr => {
            let n = b.as_i128();
            if n < 0 || n >= i128::from(w) {
                return Err(NumError::ShiftRange);
            }
            let n = n as u32;
            Ok(if op == Bit::Shl {
                NumVal::wrapped(k, a.bits << n)
            } else if k.is_signed() {
                // Canonical signed bits are sign-extended to 128, so this shift is exact.
                NumVal::wrapped(k, ((a.bits as i128) >> n) as u128)
            } else {
                // Canonical unsigned bits are zero-extended, so a logical shift is exact.
                NumVal::wrapped(k, a.bits >> n)
            })
        }
    }
}

/// `~a`.
#[must_use]
pub fn complement(a: NumVal) -> NumVal {
    NumVal::wrapped(a.kind, !a.bits)
}

/// `a op b`: integers by value, floats by IEEE 754 — every comparison with a NaN is false but
/// `!=`, and `-0.0 == 0.0`.
#[must_use]
pub fn compare(op: Cmp, a: NumVal, b: NumVal) -> bool {
    if a.kind.is_float() {
        let (x, y) = (a.as_f64(), b.as_f64());
        return match op {
            Cmp::Eq => x == y,
            Cmp::NotEq => x != y,
            Cmp::Lt => x < y,
            Cmp::LtEq => x <= y,
            Cmp::Gt => x > y,
            Cmp::GtEq => x >= y,
        };
    }
    let (x, y) = (a.value(), b.value());
    match op {
        Cmp::Eq => x == y,
        Cmp::NotEq => x != y,
        Cmp::Lt => x < y,
        Cmp::LtEq => x <= y,
        Cmp::Gt => x > y,
        Cmp::GtEq => x >= y,
    }
}

/// The integer a float truncates to, if `to` can hold it.
fn truncated(to: NumKind, x: f64) -> Option<Wide> {
    if !x.is_finite() {
        return None;
    }
    let t = x.trunc();
    let w = to.bits();
    // The bounds are powers of two, so the comparisons are exact.
    let (lo, hi) = if to.is_signed() {
        (-pow2(w - 1), pow2(w - 1))
    } else {
        (0.0, pow2(w))
    };
    if t < lo || t >= hi {
        return None;
    }
    Some(if t < 0.0 {
        Wide::from_i128(t as i128)
    } else {
        Wide::from_u128(t as u128)
    })
}

/// Whether `v` converts to `to` exactly — what `try_convert` asks.
#[must_use]
pub fn fits(to: NumKind, v: NumVal) -> bool {
    convert(to, ConvertMode::Exact, v).is_ok()
}

/// `v` as a value of `to`, by `mode` ([`ConvertMode::Try`] is treated as `Exact`: its `None`
/// is [`fits`]'s answer).
///
/// # Errors
///
/// [`NumError::Overflow`] where `Exact` cannot represent the value.
pub fn convert(to: NumKind, mode: ConvertMode, v: NumVal) -> Result<NumVal, NumError> {
    let from = v.kind;
    if to.is_float() {
        // From an integer: rounded to nearest, ties to even — Rust's `as`. From a float: exact
        // to `Float64`, rounded to `Float32`, overflowing to an infinity.
        return Ok(match (to, from.is_float()) {
            (NumKind::F64, true) => NumVal::f64(v.as_f64()),
            #[allow(clippy::cast_possible_truncation)]
            (NumKind::F32, true) => NumVal::f32(v.as_f64() as f32),
            (NumKind::F64, false) => NumVal::f64(v.value().to_f64()),
            (_, false) => NumVal::f32(v.value().to_f32()),
            (_, true) => NumVal::f64(v.as_f64()),
        });
    }
    let (lo, hi) = to.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
    if from.is_float() {
        let x = v.as_f64();
        return match mode {
            ConvertMode::Exact | ConvertMode::Try | ConvertMode::Wrap => {
                NumVal::checked(to, truncated(to, x))
            }
            ConvertMode::Saturate => {
                let t = if x.is_nan() {
                    Wide::ZERO
                } else if let Some(t) = truncated(to, x) {
                    t
                } else if x < 0.0 {
                    lo
                } else {
                    hi
                };
                NumVal::checked(to, Some(t))
            }
        };
    }
    let x = v.value();
    match mode {
        ConvertMode::Exact | ConvertMode::Try => NumVal::checked(to, Some(x)),
        ConvertMode::Wrap => Ok(NumVal::wrapped(to, v.bits)),
        ConvertMode::Saturate => NumVal::checked(to, Some(x.clamp(lo, hi))),
    }
}

/// `wrapping_add`, `wrapping_sub` and `wrapping_mul`.
#[must_use]
pub fn wrapping(op: Arith, a: NumVal, b: NumVal) -> NumVal {
    let bits = match op {
        Arith::Add => a.bits.wrapping_add(b.bits),
        Arith::Sub => a.bits.wrapping_sub(b.bits),
        _ => a.bits.wrapping_mul(b.bits),
    };
    NumVal::wrapped(a.kind, bits)
}

/// `saturating_add`, `saturating_sub` and `saturating_mul`.
#[must_use]
pub fn saturating(op: Arith, a: NumVal, b: NumVal) -> NumVal {
    let (lo, hi) = a.kind.int_range().unwrap_or((Wide::ZERO, Wide::ZERO));
    let (x, y) = (a.value(), b.value());
    // Past `Wide` the result is past either bound, on the side its sign says (Q1-B-03).
    let past = |negative: bool| if negative { lo } else { hi };
    let v = match op {
        Arith::Add => x.checked_add(y).unwrap_or_else(|| past(x.is_negative())),
        Arith::Sub => x.checked_sub(y).unwrap_or_else(|| past(x.is_negative())),
        _ => x
            .checked_mul(y)
            .unwrap_or_else(|| past(x.is_negative() != y.is_negative())),
    };
    NumVal::int(a.kind, v.clamp(lo, hi)).unwrap_or(a)
}

/// The value's bits as an unsigned integer of its width.
fn raw(a: NumVal) -> u128 {
    let w = a.kind.bits();
    if w == 128 {
        a.bits
    } else {
        a.bits & ((1u128 << w) - 1)
    }
}

/// `count_ones`, `leading_zeros` and `trailing_zeros`, counted within the type's width.
#[must_use]
pub fn count(which: Count, a: NumVal) -> i64 {
    let w = a.kind.bits();
    let r = raw(a);
    i64::from(match which {
        Count::Ones => r.count_ones(),
        Count::LeadingZeros => r.leading_zeros() - (128 - w),
        Count::TrailingZeros => {
            if r == 0 {
                w
            } else {
                r.trailing_zeros()
            }
        }
    })
}

/// Which bit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Ones,
    LeadingZeros,
    TrailingZeros,
}

/// `rotate_left` and `rotate_right` by `n` modulo the width.
///
/// # Errors
///
/// [`NumError::ShiftRange`] for a negative `n`.
pub fn rotate(left: bool, a: NumVal, n: i64) -> Result<NumVal, NumError> {
    if n < 0 {
        return Err(NumError::ShiftRange);
    }
    let w = a.kind.bits();
    let n = (n as u64 % u64::from(w)) as u32;
    let r = raw(a);
    let mask = if w == 128 {
        u128::MAX
    } else {
        (1u128 << w) - 1
    };
    let out = if n == 0 {
        r
    } else if left {
        ((r << n) | (r >> (w - n))) & mask
    } else {
        ((r >> n) | (r << (w - n))) & mask
    };
    Ok(NumVal::wrapped(a.kind, out))
}

/// `swap_bytes`.
#[must_use]
pub fn swap_bytes(a: NumVal) -> NumVal {
    let w = a.kind.bits();
    let r = raw(a).swap_bytes() >> (128 - w);
    NumVal::wrapped(a.kind, r)
}

/// `to_big_endian` and its kin, on a little-endian host — the only kind Nazm targets: big
/// endian is a swap, little endian the identity.
#[must_use]
pub fn endian(big: bool, a: NumVal) -> NumVal {
    if big {
        swap_bytes(a)
    } else {
        a
    }
}

/// A float function of one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    Sqrt,
    Abs,
    Floor,
    Ceil,
    Trunc,
    Round,
    Sin,
    Cos,
    Tan,
    Exp,
    Ln,
    Log2,
    Log10,
}

/// A float function of one argument. The exactly specified ones — `Sqrt` to `Round` — are
/// computed in the value's own precision; the C library's are computed as the host computes
/// them, in `f64` for a `Float64` and `f32` for a `Float32`.
#[must_use]
pub fn unary(f: Unary, a: NumVal) -> NumVal {
    if a.kind == NumKind::F32 {
        let x = a.as_f32();
        return NumVal::f32(match f {
            Unary::Sqrt => x.sqrt(),
            Unary::Abs => x.abs(),
            Unary::Floor => x.floor(),
            Unary::Ceil => x.ceil(),
            Unary::Trunc => x.trunc(),
            Unary::Round => x.round(),
            Unary::Sin => x.sin(),
            Unary::Cos => x.cos(),
            Unary::Tan => x.tan(),
            Unary::Exp => x.exp(),
            Unary::Ln => x.ln(),
            Unary::Log2 => x.log2(),
            Unary::Log10 => x.log10(),
        });
    }
    let x = a.as_f64();
    NumVal::f64(match f {
        Unary::Sqrt => x.sqrt(),
        Unary::Abs => x.abs(),
        Unary::Floor => x.floor(),
        Unary::Ceil => x.ceil(),
        Unary::Trunc => x.trunc(),
        Unary::Round => x.round(),
        Unary::Sin => x.sin(),
        Unary::Cos => x.cos(),
        Unary::Tan => x.tan(),
        Unary::Exp => x.exp(),
        Unary::Ln => x.ln(),
        Unary::Log2 => x.log2(),
        Unary::Log10 => x.log10(),
    })
}

/// A float function of two arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Pow,
    Atan2,
    Min,
    Max,
}

/// A float function of two arguments. `Min` and `Max` return the number when one side is a
/// NaN, as C's `fmin` and `fmax` do.
#[must_use]
pub fn binary(f: Binary, a: NumVal, b: NumVal) -> NumVal {
    if a.kind == NumKind::F32 {
        let (x, y) = (a.as_f32(), b.as_f32());
        return NumVal::f32(match f {
            Binary::Pow => x.powf(y),
            Binary::Atan2 => x.atan2(y),
            Binary::Min => x.min(y),
            Binary::Max => x.max(y),
        });
    }
    let (x, y) = (a.as_f64(), b.as_f64());
    NumVal::f64(match f {
        Binary::Pow => x.powf(y),
        Binary::Atan2 => x.atan2(y),
        Binary::Min => x.min(y),
        Binary::Max => x.max(y),
    })
}

/// A float with exactly `digits` decimals (R1a, L4): correctly rounded from its exact binary
/// value, ties to even — C's `%.*f`, which the native runtime calls (`nz.format_fixed`) — and
/// `nan`, `inf` and `-inf` spelled as [`to_str`] spells them.
#[must_use]
pub fn to_fixed(a: NumVal, digits: usize) -> String {
    let x = a.as_f64();
    if x.is_nan() {
        return "nan".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    format!("{x:.digits$}")
}

/// The decimal text of a number. Integers are written in full, with `-` when negative. A float
/// is written by ECMAScript's `Number::toString` rule over the shortest digits that read back to
/// the same value in its own precision: `100`, `1.5`, `0.001`, `1e+21`, `1.5e-7`, `-0`,
/// `inf`, `-inf` and `nan`.
#[must_use]
pub fn to_str(a: NumVal) -> String {
    if !a.kind.is_float() {
        return a.value().to_string();
    }
    let x = a.as_f64();
    if x.is_nan() {
        return "nan".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    // The shortest digits that read back to the same value in its own type, found as the native
    // runtime finds them (`nz.format_float`): the first precision whose correctly rounded
    // `{:.p$e}` — ties to even, as C's `%.*e` and ECMAScript's rule — parses back exactly. Rust's
    // own shortest `{:e}` breaks a tie between two equally short candidates the other way, so
    // `Float32(3135501.25)` printed `3135501.3` here and `3135501.2` natively (Q1-B-02).
    let single = a.kind == NumKind::F32;
    let sci = (0..17)
        .map(|p| format!("{x:.p$e}"))
        .find(|t| {
            if single {
                t.parse::<f32>().is_ok_and(|r| r == a.as_f32())
            } else {
                t.parse::<f64>().is_ok_and(|r| r == x)
            }
        })
        .unwrap_or_else(|| format!("{x:.16e}"));
    let (negative, sci) = match sci.strip_prefix('-') {
        Some(rest) => (true, rest.to_owned()),
        None => (false, sci),
    };
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let e: i64 = exponent.parse().unwrap_or(0);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&layout(&digits, e + 1));
    out
}

/// ECMAScript's layout of `digits` × 10^(n − k), where `k` is the digit count.
#[must_use]
pub fn layout(digits: &str, n: i64) -> String {
    let k = i64::try_from(digits.len()).unwrap_or(0);
    let zeros = |c: i64| "0".repeat(usize::try_from(c).unwrap_or(0));
    if k <= n && n <= 21 {
        format!("{digits}{}", zeros(n - k))
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(usize::try_from(n).unwrap_or(0));
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", zeros(-n))
    } else {
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let exp = (n - 1).abs();
        if k == 1 {
            format!("{digits}e{sign}{exp}")
        } else {
            let (a, b) = digits.split_at(1);
            format!("{a}.{b}e{sign}{exp}")
        }
    }
}

/// Whether `s` is a floating-point number as `float_parses` reads one:
/// `[+-]? (D+ (. D*)? | . D+) ([eE] [+-]? D+)?`, or `[+-]? (inf | nan)`, with nothing else.
#[must_use]
pub fn parses(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let rest = s.get(i..).unwrap_or("");
    if rest == "inf" || rest == "nan" {
        return true;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let whole = digits(&mut i);
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac = digits(&mut i);
    }
    if whole == 0 && frac == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// The `Float64` `s` writes, correctly rounded, or NaN when [`parses`] refuses it.
#[must_use]
pub fn parse(s: &str) -> f64 {
    if !parses(s) {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// The value a floating-point literal writes, in `kind`: correctly rounded, once, from the
/// decimal text — never through `f64` for a `Float32`.
#[must_use]
pub fn literal(kind: NumKind, text: &str) -> NumVal {
    if kind == NumKind::F32 {
        NumVal::f32(text.parse::<f32>().unwrap_or(f32::NAN))
    } else {
        NumVal::f64(text.parse::<f64>().unwrap_or(f64::NAN))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn i(k: NumKind, v: i128) -> NumVal {
        NumVal::int(k, v).unwrap()
    }

    /// Q1-B-02: of two equally short digit strings that both read back, the one ECMAScript and
    /// C's `%.*e` choose — the even one at an exact tie — as the native runtime writes it.
    #[test]
    fn a_tie_between_equally_short_digits_goes_to_even() {
        let f32v = |v: f32| NumVal::f32(v);
        // 3135501 and a quarter, and three quarters: exact in `f32`, and ties at eight digits.
        assert_eq!(to_str(f32v(3_135_501.0 + 0.25)), "3135501.2");
        assert_eq!(to_str(f32v(3_135_501.0 + 0.75)), "3135501.8");
        assert_eq!(to_str(NumVal::f64(0.1)), "0.1");
        assert_eq!(to_str(NumVal::f64(1e21)), "1e+21");
        assert_eq!(to_str(NumVal::f64(-1.5e-7)), "-1.5e-7");
        assert_eq!(to_str(NumVal::f64(f64::MAX)), "1.7976931348623157e+308");
        assert_eq!(to_str(NumVal::f64(5e-324)), "5e-324");
    }

    /// Q1-B-03: a `UInt64` product can pass `i128`; it is an overflow, and saturates to the
    /// top, never a panic.
    #[test]
    fn the_largest_unsigned_products_overflow_rather_than_panic() {
        let max = i(NumKind::U64, i128::from(u64::MAX));
        let big = i(NumKind::U64, 13_043_817_825_332_782_213);
        for (a, b) in [(max, max), (big, big), (max, i(NumKind::U64, 2))] {
            assert_eq!(arith(Arith::Mul, a, b), Err(NumError::Overflow));
            assert_eq!(saturating(Arith::Mul, a, b), max);
        }
        assert_eq!(
            arith(
                Arith::Mul,
                i(NumKind::I64, i128::from(i64::MIN)),
                i(NumKind::I64, i128::from(i64::MIN))
            ),
            Err(NumError::Overflow)
        );
        // Past `Wide` itself (R1b, L13b): `UInt128::MAX` times 2 overflows, and so do the two
        // `Int128` products whose magnitudes pass 2^128.
        let umax = NumVal::int(NumKind::U128, u128::MAX).unwrap();
        let two = i(NumKind::U128, 2);
        assert_eq!(arith(Arith::Mul, umax, two), Err(NumError::Overflow));
        assert_eq!(saturating(Arith::Mul, umax, two), umax);
        let imin = i(NumKind::I128, i128::MIN);
        assert_eq!(arith(Arith::Mul, imin, imin), Err(NumError::Overflow));
        assert_eq!(
            saturating(Arith::Mul, imin, imin),
            i(NumKind::I128, i128::MAX)
        );
        // And what fits, exactly: 2^127 is `UInt128`'s, one past `Int128`'s.
        let half = i(NumKind::U128, 1i128 << 126);
        assert_eq!(
            arith(Arith::Mul, half, two),
            Ok(NumVal::int(NumKind::U128, 1u128 << 127).unwrap())
        );
    }

    #[test]
    fn integer_arithmetic_traps_at_each_width() {
        assert_eq!(
            arith(Arith::Add, i(NumKind::U8, 255), i(NumKind::U8, 1)),
            Err(NumError::Overflow)
        );
        assert_eq!(
            arith(Arith::Sub, i(NumKind::U8, 0), i(NumKind::U8, 1)),
            Err(NumError::Overflow)
        );
        assert_eq!(
            arith(Arith::Div, i(NumKind::I8, -128), i(NumKind::I8, -1)),
            Err(NumError::Overflow)
        );
        assert_eq!(
            arith(Arith::Rem, i(NumKind::I8, -128), i(NumKind::I8, -1)),
            Ok(i(NumKind::I8, 0))
        );
        assert_eq!(
            arith(Arith::Div, i(NumKind::U32, 7), i(NumKind::U32, 0)),
            Err(NumError::DivideByZero)
        );
        let big = i(NumKind::U64, i128::from(u64::MAX));
        assert_eq!(
            arith(Arith::Add, big, i(NumKind::U64, 1)),
            Err(NumError::Overflow)
        );
        assert_eq!(
            arith(Arith::Div, big, i(NumKind::U64, 2))
                .unwrap()
                .as_i128(),
            i128::from(u64::MAX / 2)
        );
        assert_eq!(neg(i(NumKind::U16, 0)), Ok(i(NumKind::U16, 0)));
        assert_eq!(neg(i(NumKind::U16, 1)), Err(NumError::Overflow));
    }

    #[test]
    fn bits_wrap_within_the_width_and_shifts_are_bounded() {
        let a = i(NumKind::U8, 0b1010_0101);
        assert_eq!(complement(a).as_i128(), 0b0101_1010);
        assert_eq!(complement(i(NumKind::I8, 0)).as_i128(), -1);
        assert_eq!(
            bit(Bit::Shl, a, i(NumKind::I64, 1)).unwrap().as_i128(),
            0b0100_1010
        );
        assert_eq!(
            bit(Bit::Shr, i(NumKind::I8, -128), i(NumKind::I64, 7))
                .unwrap()
                .as_i128(),
            -1
        );
        assert_eq!(
            bit(Bit::Shr, i(NumKind::U8, 128), i(NumKind::I64, 7))
                .unwrap()
                .as_i128(),
            1
        );
        assert_eq!(
            bit(Bit::Shl, a, i(NumKind::I64, 8)),
            Err(NumError::ShiftRange)
        );
        assert_eq!(
            bit(Bit::Shl, a, i(NumKind::I64, -1)),
            Err(NumError::ShiftRange)
        );
        assert_eq!(count(Count::LeadingZeros, i(NumKind::U16, 1)), 15);
        assert_eq!(count(Count::TrailingZeros, i(NumKind::U16, 0)), 16);
        assert_eq!(count(Count::Ones, i(NumKind::I8, -1)), 8);
        assert_eq!(
            rotate(true, i(NumKind::U8, 0b1000_0001), 1)
                .unwrap()
                .as_i128(),
            0b0000_0011
        );
        assert_eq!(swap_bytes(i(NumKind::U16, 0x1234)).as_i128(), 0x3412);
        assert_eq!(
            wrapping(Arith::Add, i(NumKind::U8, 250), i(NumKind::U8, 10)).as_i128(),
            4
        );
        assert_eq!(
            saturating(Arith::Sub, i(NumKind::U8, 3), i(NumKind::U8, 10)).as_i128(),
            0
        );
        assert_eq!(
            wrapping(Arith::Mul, i(NumKind::I8, 64), i(NumKind::I8, 2)).as_i128(),
            -128
        );
    }

    #[test]
    fn conversions_trap_wrap_and_saturate_as_stated() {
        let x = i(NumKind::I64, 300);
        assert_eq!(
            convert(NumKind::U8, ConvertMode::Exact, x),
            Err(NumError::Overflow)
        );
        assert_eq!(
            convert(NumKind::U8, ConvertMode::Wrap, x)
                .unwrap()
                .as_i128(),
            44
        );
        assert_eq!(
            convert(NumKind::U8, ConvertMode::Saturate, x)
                .unwrap()
                .as_i128(),
            255
        );
        assert!(!fits(NumKind::U8, x));
        let f = NumVal::f64(-2.9);
        assert_eq!(
            convert(NumKind::I64, ConvertMode::Exact, f)
                .unwrap()
                .as_i128(),
            -2
        );
        assert_eq!(
            convert(NumKind::U8, ConvertMode::Exact, f),
            Err(NumError::Overflow)
        );
        assert_eq!(
            convert(NumKind::I32, ConvertMode::Saturate, NumVal::f64(f64::NAN))
                .unwrap()
                .as_i128(),
            0
        );
        assert_eq!(
            convert(
                NumKind::I64,
                ConvertMode::Exact,
                NumVal::f64(9.223_372_036_854_776e18)
            ),
            Err(NumError::Overflow)
        );
        assert_eq!(
            convert(
                NumKind::F64,
                ConvertMode::Exact,
                i(NumKind::I64, (1 << 53) + 1)
            )
            .unwrap()
            .as_f64(),
            9_007_199_254_740_992.0
        );
        assert!(
            convert(NumKind::F32, ConvertMode::Exact, NumVal::f64(1e300))
                .unwrap()
                .as_f32()
                .is_infinite()
        );
    }

    #[test]
    fn floats_compare_by_ieee() {
        let nan = NumVal::f64(f64::NAN);
        assert!(!compare(Cmp::Eq, nan, nan));
        assert!(compare(Cmp::NotEq, nan, nan));
        assert!(!compare(Cmp::Lt, nan, NumVal::f64(1.0)));
        assert!(compare(Cmp::Eq, NumVal::f64(-0.0), NumVal::f64(0.0)));
    }

    #[test]
    fn a_float_is_written_by_the_shortest_round_trip_digits() {
        for (x, s) in [
            (100.0, "100"),
            (1.5, "1.5"),
            (0.1, "0.1"),
            (0.001, "0.001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123_456.789, "123456.789"),
            (-2.5, "-2.5"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
        ] {
            assert_eq!(to_str(NumVal::f64(x)), s, "{x}");
        }
        assert_eq!(to_str(NumVal::f64(-0.0)), "-0");
        assert_eq!(to_str(NumVal::f64(f64::INFINITY)), "inf");
        assert_eq!(to_str(NumVal::f64(f64::NAN)), "nan");
        assert_eq!(to_str(NumVal::f32(0.1)), "0.1");
        assert_eq!(to_str(NumVal::f32(16_777_216.0)), "16777216");
    }

    #[test]
    fn parsing_accepts_exactly_the_stated_forms() {
        for ok in [
            "1", "-1.5", "+0.5", ".5", "5.", "1e9", "2.5E-3", "inf", "-inf", "nan",
        ] {
            assert!(parses(ok), "{ok}");
        }
        for bad in [
            "", ".", "e5", "1e", "1e+", " 1", "1 ", "0x10", "infinity", "NaN", "1_0",
        ] {
            assert!(!parses(bad), "{bad}");
        }
        assert_eq!(parse("0.1"), 0.1);
        assert!(parse("x").is_nan());
        assert_eq!(literal(NumKind::F32, "0.1").as_f32(), 0.1_f32);
    }
}
