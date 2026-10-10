//! The instruction-level LIR (N105, `docs/architecture.md` §7.106).
//!
//! One module per unit. Everything a native program means is decided by the time a module
//! exists — which runtime entry an operation calls, every guard and its failure text, every
//! offset, every retain and release — so a backend translates instructions and decides nothing
//! a program can observe.
//!
//! # Values, variables and slots
//!
//! A [`V`] is defined once, by one instruction, and used only where its definition dominates the
//! use. A scalar that changes or crosses a join lives in a [`Var`]; anything held in memory lives
//! in a [`Slot`] of a stated size and alignment, reached through [`Inst::SlotAddr`].
//!
//! # Classes
//!
//! A value's machine class: [`Class::Bit`] a truth value (a `Bool`, a comparison, a runtime
//! entry's bit), stored as one byte; [`Class::I8`], [`Class::I16`] and [`Class::I32`] narrow
//! integers; [`Class::I64`] an `Int` or a `UInt64`; [`Class::F32`] and [`Class::F64`] IEEE 754
//! floats (Gate 2); [`Class::Ptr`] an address, as wide as the target's pointer. An integer class
//! has no signedness: the operation says how its bits are read.

/// A value's machine class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Bit,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Ptr,
}

impl Class {
    /// Its name in the printed form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bit => "bit",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Ptr => "ptr",
        }
    }

    /// Whether this is an integer class arithmetic applies to.
    #[must_use]
    pub const fn integer(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    /// Whether this is a float class (Gate 2).
    #[must_use]
    pub const fn float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Its width in bits, for an integer or float class.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            Self::Bit | Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 | Self::F32 => 32,
            Self::I64 | Self::F64 | Self::Ptr => 64,
        }
    }
}

/// A value, numbered within its function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct V(pub u32);

/// A variable: a mutable scalar of one class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Var(pub u32);

/// A stack slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(pub u32);

/// A block, by its position in its function. Block 0 is the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct B(pub u32);

/// A symbol of the module: a function or data, defined here or imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymId(pub u32);

impl V {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl B {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl SymId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// How a symbol is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Linkage {
    /// Defined elsewhere: another unit, the runtime, C.
    Import,
    /// Defined here, visible only in this module.
    Local,
    /// Defined here, visible to the program's other modules and not outside it.
    Hidden,
    /// Defined here, visible outside the program: an export's C entry (N55).
    Export,
}

/// A calling convention. The two differ only in how a narrow argument is extended: a
/// foreign or C-visible function receives a `Bool` as C's `bool`, zero-extended (N42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Conv {
    Nazm,
    C,
}

/// A function's signature in machine classes. An in-memory result is not a result here: the
/// lowering made it a pointer parameter, first, already.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sig {
    pub params: Vec<Class>,
    pub ret: Option<Class>,
    pub conv: Conv,
}

/// What a symbol is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymKind {
    Func {
        sig: Sig,
    },
    /// Data: a literal, a failure text, or — imported — a runtime global.
    Data {
        class: Option<Class>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub linkage: Linkage,
    pub kind: SymKind,
}

/// Bytes the module defines: read-only unless a static atomic's cell, at an alignment, with the
/// addresses of other symbols written into them at offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Data {
    pub sym: SymId,
    pub bytes: Vec<u8>,
    pub align: u32,
    /// A static `Atomic`'s cell (Gate 3): changed as the program runs. Everything else is
    /// read-only.
    pub writable: bool,
    /// A static's `@section` (Gate 3).
    pub section: Option<String>,
    /// `(offset, symbol)`: the symbol's address, written at the offset, pointer-wide.
    pub relocs: Vec<(u32, SymId)>,
}

/// A stack slot's size and alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDecl {
    pub size: u32,
    pub align: u32,
}

/// Wrapping integer operations, on two values of one integer class (`And`, `Or` and `Xor`
/// also on [`Class::Bit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// Signed; the lowering has already refused a zero divisor and `MIN / -1`.
    SDiv,
    /// Signed; the lowering has already refused a zero divisor and `MIN % -1`.
    SRem,
    And,
    Or,
    Xor,
    /// Unsigned (Gate 2); the lowering has already refused a zero divisor.
    UDiv,
    URem,
    /// Shifts by an amount of the same class, which the lowering has already checked is in
    /// `0..width`; `LShr` fills with zeros, `AShr` with the sign.
    Shl,
    LShr,
    AShr,
    /// Rotations by an amount of the same class, taken modulo the width.
    Rotl,
    Rotr,
    /// IEEE 754, round to nearest, ties to even; never failing.
    FAdd,
    FSub,
    FMul,
    FDiv,
}

/// The overflow-checked operations: a result and whether it overflowed, on any integer class;
/// the `U` forms read the operands as unsigned (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OvfOp {
    Add,
    Sub,
    Mul,
    UAdd,
    USub,
    UMul,
}

/// Operations of one operand (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// IEEE 754 negation: the sign flips, a NaN's included.
    FNeg,
    FSqrt,
    FAbs,
    FFloor,
    FCeil,
    FTrunc,
    /// Bit counts, in the operand's class.
    Popcnt,
    Clz,
    Ctz,
    /// The bytes reversed.
    Bswap,
}

/// Conversions between classes (Gate 2). The integer to float ones round to nearest, ties to
/// even; the float to integer ones truncate toward zero and are only made where the lowering
/// has already checked the value fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConvOp {
    /// A signed integer to a float.
    SToF,
    /// An unsigned integer to a float.
    UToF,
    /// A float to a signed integer.
    FToS,
    /// A float to an unsigned integer.
    FToU,
    /// `Float32` to `Float64`, exactly, or `Float64` to `Float32`, rounded.
    FExt,
    FTrunc,
    /// The same bits as another class of the same width: a float and its integer bits.
    Bits,
}

/// Comparisons. Integers compare signed by `Lt` to `Ge`, unsigned by `ULt` to `UGe`; a pointer
/// only for equality. Floats compare by the `F` forms, IEEE 754's: ordered — false with a NaN —
/// except `FNe`, which is true with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    ULt,
    ULe,
    UGt,
    UGe,
    FEq,
    FNe,
    FLt,
    FLe,
    FGt,
    FGe,
}

/// Where a debugger should say the instructions that follow come from (N48, N91).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    /// The `index`th parameter. In the entry block only.
    Param {
        dst: V,
        index: u32,
    },
    Const {
        dst: V,
        class: Class,
        value: i64,
    },
    /// A symbol's address.
    Addr {
        dst: V,
        sym: SymId,
    },
    SlotAddr {
        dst: V,
        slot: Slot,
    },
    /// A `class` value at `addr + offset`. A [`Class::Bit`] occupies one byte holding 0 or 1.
    Load {
        dst: V,
        class: Class,
        addr: V,
        offset: i32,
    },
    Store {
        addr: V,
        offset: i32,
        value: V,
    },
    Bin {
        dst: V,
        op: BinOp,
        a: V,
        b: V,
    },
    /// `dst` and whether it overflowed, a [`Class::Bit`]. On any integer class.
    Ovf {
        dst: V,
        ovf: V,
        op: OvfOp,
        a: V,
        b: V,
    },
    /// A [`Class::Bit`].
    Cmp {
        dst: V,
        op: Cmp,
        a: V,
        b: V,
    },
    Select {
        dst: V,
        cond: V,
        a: V,
        b: V,
    },
    /// One operand (Gate 2): its result is the operand's class, except a bit count's, which
    /// is too.
    Un {
        dst: V,
        op: UnOp,
        a: V,
    },
    /// A conversion to `to` (Gate 2).
    Conv {
        dst: V,
        op: ConvOp,
        to: Class,
        src: V,
    },
    /// Widen to `to`: zero-extended unless `signed`.
    Ext {
        dst: V,
        to: Class,
        src: V,
        signed: bool,
    },
    Trunc {
        dst: V,
        to: Class,
        src: V,
    },
    /// `base + offset`, `offset` an [`Class::I64`].
    PtrAdd {
        dst: V,
        base: V,
        offset: V,
    },
    /// An address as an [`Class::I64`], and back: where MIR holds a C pointer as a
    /// pointer-sized `Int` (N85).
    PtrToInt {
        dst: V,
        src: V,
    },
    IntToPtr {
        dst: V,
        src: V,
    },
    GetVar {
        dst: V,
        var: Var,
    },
    SetVar {
        var: Var,
        src: V,
    },
    Call {
        dst: Option<V>,
        callee: SymId,
        args: Vec<V>,
    },
    CallIndirect {
        dst: Option<V>,
        addr: V,
        sig: Sig,
        args: Vec<V>,
    },
    /// Copy `size` bytes; the regions do not overlap.
    Copy {
        dst: V,
        src: V,
        size: u32,
    },
    Zero {
        dst: V,
        size: u32,
    },
    Loc(Pos),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Jump(B),
    Branch {
        cond: V,
        then: B,
        otherwise: B,
    },
    /// On an integer value; `default` where no case matches.
    Switch {
        value: V,
        cases: Vec<(i64, B)>,
        default: B,
    },
    Return(Option<V>),
    /// Control never reaches here: after `exit`, and after `nz.report`.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

/// A local a debugger can show (N58): its storage, the parameter position it came from (1-based)
/// or 0 for a binding, its line, what kind of value it is, the lexical block it is visible in
/// as `(first line, first column, last line, last column)` or zeros for the whole function, and
/// its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugVar {
    pub storage: Storage,
    pub arg: u32,
    pub line: u32,
    pub kind: &'static str,
    pub scope: (u32, u32, u32, u32),
    pub name: String,
}

/// Where a local lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    Var(Var),
    Slot(Slot),
    /// A variable holding the value's address: a parameter held in memory the caller owns.
    Indirect(Var),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub sym: SymId,
    /// Display only: the source name, or what a generated function is.
    pub name: String,
    pub vars: Vec<Class>,
    pub slots: Vec<SlotDecl>,
    /// Display only: the source binding a variable or slot holds, `name : Type`, or empty.
    pub var_names: Vec<String>,
    pub slot_names: Vec<String>,
    pub blocks: Vec<Block>,
    /// How many values the function defines.
    pub values: u32,
    /// The function's own position, `(file, line)`, for debug information.
    pub at: Option<(u32, u32)>,
    pub debug_vars: Vec<DebugVar>,
    /// The section `@section` places it in (Gate 3): a board's image only.
    pub section: Option<String>,
}

/// One unit, as LIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The unit's source, for messages.
    pub source: String,
    /// A pointer's width on the module's target, in bytes: how wide a data relocation is.
    pub pointer: u32,
    pub symbols: Vec<Symbol>,
    pub functions: Vec<Function>,
    pub data: Vec<Data>,
    /// What of the runtime the module reaches.
    pub needs: crate::Needs,
}

impl Module {
    #[must_use]
    pub fn symbol(&self, s: SymId) -> Option<&Symbol> {
        self.symbols.get(s.index())
    }

    /// A function symbol's signature.
    #[must_use]
    pub fn sig(&self, s: SymId) -> Option<&Sig> {
        match &self.symbol(s)?.kind {
            SymKind::Func { sig } => Some(sig),
            SymKind::Data { .. } => None,
        }
    }
}
