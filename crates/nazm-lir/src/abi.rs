//! The target's data layout, every type's bytes on it, and what owns anything (N81).
//!
//! `docs/architecture.md` §7.82. Before N81 the Cranelift backend decided byte layouts in a module
//! of its own, with an 8-byte pointer assumed, and both backends computed separately whether a
//! value of a type owns anything a drop must give back. Here is the one place either is decided:
//!
//! - [`DataLayout`]: a pointer's width on the target, and the fixed widths of the rest — an `Int`
//!   is 8 bytes, a `Bool` 1, a `Str` three words `{pointer, length, owner}` with an 8-byte length.
//! - [`ByteLayouts`]: every record of a unit at its fields' offsets in the canonical order
//!   [`crate::lower`] fixed, each at its natural alignment; every enum a tag word followed by one
//!   payload area at [`PAYLOAD`], as large as its largest variant, each variant laid out as a
//!   record. The same numbers on every 64-bit target; on `wasm32` a pointer is 4 bytes.
//! - [`owns`]: whether a type owns anything, closed over the records and enums it contains.
//!
//! The LLVM backend writes named aggregates and lets LLVM lay them out; it reads ownership from
//! here, and every native build validates these tables ([`ByteLayouts::verify`]) before any code is
//! generated. What remains per backend is stated in §7.82: operations, not bytes.

use std::collections::BTreeMap;

use crate::backend::{Arch, Target};
use crate::ir::{ArrayLayout, ArrayTy, EnumLayout, EnumTy, Owns, RecordLayout, RecordTy, Ty, Unit};

/// The offset of an enum's payload area: after its tag word, which is 8 bytes, and every
/// payload's alignment divides 8.
pub const PAYLOAD: u32 = 8;

/// A target's widths, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataLayout {
    /// A pointer — and so every handle, a function value, and a `Str`'s pointer and owner.
    pub pointer: u32,
}

impl DataLayout {
    /// Every 64-bit target this compiler generates code for.
    pub const LP64: Self = Self { pointer: 8 };

    /// `wasm32-unknown-unknown`: 4-byte pointers, and still an 8-byte `Int`.
    pub const WASM32: Self = Self { pointer: 4 };

    /// The data layout of `target`.
    #[must_use]
    pub const fn of(target: &Target) -> Self {
        match target.arch {
            // A Cortex-M board's pointers are 4 bytes too (Gate 3): the same layout as wasm32's.
            Arch::Wasm32 | Arch::Thumbv7m => Self::WASM32,
            Arch::Aarch64 | Arch::X86_64 | Arch::Riscv64 => Self::LP64,
        }
    }

    /// The data layout whose pointer is `bits` wide.
    #[must_use]
    pub const fn with_pointer_bits(bits: u32) -> Self {
        Self { pointer: bits >> 3 }
    }

    /// A `Str`: `{pointer, length, owner}`, the length an 8-byte `Int`.
    #[must_use]
    pub const fn str(self) -> Lay {
        let len = round(self.pointer, 8);
        let owner = len + 8;
        Lay::new(round(owner + self.pointer, 8), 8)
    }

    /// The offsets of a `Str`'s three fields.
    #[must_use]
    pub const fn str_fields(self) -> [u32; 3] {
        let len = round(self.pointer, 8);
        [0, len, len + 8]
    }
}

/// A size and an alignment, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lay {
    pub size: u32,
    pub align: u32,
}

impl Lay {
    #[must_use]
    pub const fn new(size: u32, align: u32) -> Self {
        Self { size, align }
    }
}

const fn round(n: u32, align: u32) -> u32 {
    if align <= 1 {
        n
    } else {
        n.div_ceil(align) * align
    }
}

/// A structure's fields at their offsets, and the whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Struct {
    pub offsets: Vec<u32>,
    pub fields: Vec<Ty>,
    pub lay: Lay,
}

/// An enum: its tag word, then its payload area at [`PAYLOAD`]; each variant's payload by tag,
/// its offsets relative to the payload area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sum {
    pub variants: Vec<Struct>,
    pub lay: Lay,
}

/// Whether a value of `t` owns anything a drop must give back, over `records` and `enums` —
/// the one answer both backends read (N81).
#[must_use]
pub fn owns(records: &[RecordLayout], enums: &[EnumLayout], arrays: &[ArrayLayout], t: Ty) -> bool {
    let mut seen: Vec<Ty> = Vec::new();
    let mut work = vec![t];
    while let Some(next) = work.pop() {
        if seen.contains(&next) {
            continue;
        }
        seen.push(next);
        let inner: Vec<Ty> = match next.owns() {
            None => continue,
            Some(Owns::Record(id)) => records
                .iter()
                .find(|r| r.id == id)
                .map(|l| l.fields.clone())
                .unwrap_or_default(),
            Some(Owns::Enum(id)) => enums
                .iter()
                .find(|e| e.id == id)
                .map(|l| l.variants.iter().flat_map(|v| v.fields.clone()).collect())
                .unwrap_or_default(),
            Some(Owns::Array(id)) => arrays
                .iter()
                .find(|a| a.id == id)
                .map(|a| vec![a.elem])
                .unwrap_or_default(),
            Some(_) => return true,
        };
        work.extend(inner);
    }
    false
}

/// Every layout one unit needs, on one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteLayouts {
    pub data: DataLayout,
    /// By record id.
    pub records: BTreeMap<u32, Struct>,
    /// By enum id.
    pub enums: BTreeMap<u32, Sum>,
    owning_records: BTreeMap<u32, bool>,
    owning_enums: BTreeMap<u32, bool>,
    /// Array element types and lengths, by array id (Gate 2).
    pub arrays: BTreeMap<u32, (Ty, u32)>,
}

impl ByteLayouts {
    /// The layouts of a unit's types on a target, from the canonical tables. Computed in
    /// dependency order without recursion: a type's fields are laid out before it. A unit's type
    /// graph is acyclic by value — the checker refuses a type that contains itself — so it ends.
    #[must_use]
    pub fn of(unit: &Unit, data: DataLayout) -> Self {
        let mut out = Self {
            data,
            records: BTreeMap::new(),
            enums: BTreeMap::new(),
            owning_records: unit
                .records
                .iter()
                .map(|r| {
                    (
                        r.id.0,
                        owns(&unit.records, &unit.enums, &unit.arrays, Ty::Record(r.id)),
                    )
                })
                .collect(),
            owning_enums: unit
                .enums
                .iter()
                .map(|e| {
                    (
                        e.id.0,
                        owns(&unit.records, &unit.enums, &unit.arrays, Ty::Enum(e.id)),
                    )
                })
                .collect(),
            arrays: unit
                .arrays
                .iter()
                .map(|a| (a.id.0, (a.elem, a.len)))
                .collect(),
        };
        let mut pending: Vec<(bool, u32)> = unit
            .records
            .iter()
            .map(|r| (true, r.id.0))
            .chain(unit.enums.iter().map(|e| (false, e.id.0)))
            .collect();
        let bound = pending.len() + 1;
        for _ in 0..bound {
            if pending.is_empty() {
                break;
            }
            pending.retain(|(record, id)| {
                if *record {
                    let Some(r) = unit.records.iter().find(|r| r.id.0 == *id) else {
                        return false;
                    };
                    if let Some(s) = out.structure(&r.fields) {
                        out.records.insert(*id, s);
                        return false;
                    }
                    true
                } else {
                    let Some(e) = unit.enums.iter().find(|e| e.id.0 == *id) else {
                        return false;
                    };
                    let variants: Option<Vec<Struct>> = e
                        .variants
                        .iter()
                        .map(|v| out.structure(&v.fields))
                        .collect();
                    let Some(variants) = variants else {
                        return true;
                    };
                    let area = variants.iter().map(|v| v.lay.size).max().unwrap_or(0);
                    let align = variants
                        .iter()
                        .map(|v| v.lay.align)
                        .max()
                        .unwrap_or(1)
                        .max(8);
                    let lay = Lay::new(round(PAYLOAD + area, align), align);
                    out.enums.insert(*id, Sum { variants, lay });
                    false
                }
            });
        }
        out
    }

    fn structure(&self, fields: &[Ty]) -> Option<Struct> {
        let mut offsets = Vec::with_capacity(fields.len());
        let mut at = 0u32;
        let mut align = 1u32;
        for f in fields {
            let l = self.lay(*f)?;
            at = round(at, l.align);
            offsets.push(at);
            at += l.size;
            align = align.max(l.align);
        }
        Some(Struct {
            offsets,
            fields: fields.to_vec(),
            lay: Lay::new(round(at, align), align),
        })
    }

    /// A type's size and alignment, if it is known.
    #[must_use]
    pub fn lay(&self, t: Ty) -> Option<Lay> {
        let p = self.data.pointer;
        match t {
            Ty::Int => Some(Lay::new(8, 8)),
            // Its own width and C's alignment for it (Gate 2): bits over eight, exactly — but a
            // 128-bit integer (R1b, L13b) is aligned to 8, as every payload is (`PAYLOAD`); it
            // never crosses into C, and its loads and stores state the alignment.
            #[allow(clippy::integer_division)]
            Ty::Num(k) => Some(Lay::new(k.bits() / 8, (k.bits() / 8).min(8))),
            Ty::Ints | Ty::Strs | Ty::Chan | Ty::Vec(_) | Ty::Fn(_) | Ty::ChanOf(_) => {
                Some(Lay::new(p, p))
            }
            Ty::Bool => Some(Lay::new(1, 1)),
            Ty::Str => Some(self.data.str()),
            Ty::Record(RecordTy(id)) => self.records.get(&id).map(|s| s.lay),
            Ty::Enum(EnumTy(id)) => self.enums.get(&id).map(|s| s.lay),
            // Its elements, contiguous, each at a multiple of its own size, which its
            // alignment divides (Gate 2).
            Ty::Array(crate::ir::ArrayTy(id)) => {
                let (elem, len) = self.arrays.get(&id).copied()?;
                let l = self.lay(elem)?;
                Some(Lay::new(l.size.saturating_mul(len), l.align))
            }
        }
    }

    /// Whether a value of `t` owns anything a drop must give back.
    #[must_use]
    pub fn owns(&self, t: Ty) -> bool {
        match t {
            Ty::Int | Ty::Bool | Ty::Num(_) => false,
            // What its element owns (L8).
            Ty::Array(ArrayTy(id)) => self.arrays.get(&id).is_some_and(|(e, _)| self.owns(*e)),
            Ty::Str | Ty::Ints | Ty::Strs | Ty::Chan | Ty::Vec(_) | Ty::Fn(_) | Ty::ChanOf(_) => {
                true
            }
            Ty::Record(RecordTy(id)) => self.owning_records.get(&id).copied().unwrap_or(false),
            Ty::Enum(EnumTy(id)) => self.owning_enums.get(&id).copied().unwrap_or(false),
        }
    }

    /// Every invariant of these tables (N81): each structure's fields ascending, inside it and
    /// aligned, its size a multiple of its alignment; each enum's variants inside its payload
    /// area; ownership closed under containment. A failure is a compiler defect, named.
    ///
    /// # Errors
    ///
    /// The first invariant that does not hold.
    pub fn verify(&self) -> Result<(), String> {
        let check = |what: &str, s: &Struct| -> Result<(), String> {
            if s.offsets.len() != s.fields.len() {
                return Err(format!(
                    "{what}: {} offsets for {} fields",
                    s.offsets.len(),
                    s.fields.len()
                ));
            }
            if s.lay.align == 0 || s.lay.size % s.lay.align != 0 {
                return Err(format!(
                    "{what}: size {} is not a multiple of its alignment {}",
                    s.lay.size, s.lay.align
                ));
            }
            let mut end = 0u32;
            for (i, (off, t)) in s.offsets.iter().zip(&s.fields).enumerate() {
                let l = self
                    .lay(*t)
                    .ok_or_else(|| format!("{what}: field {i} has no layout"))?;
                if *off < end {
                    return Err(format!("{what}: field {i} at {off} overlaps the field before it, which ends at {end}"));
                }
                if off % l.align != 0 {
                    return Err(format!(
                        "{what}: field {i} at {off} is not aligned to {}",
                        l.align
                    ));
                }
                if l.align > s.lay.align {
                    return Err(format!(
                        "{what}: field {i} is aligned to {}, more than the whole",
                        l.align
                    ));
                }
                end = off + l.size;
            }
            if end > s.lay.size {
                return Err(format!(
                    "{what}: its fields end at {end}, past its size {}",
                    s.lay.size
                ));
            }
            Ok(())
        };
        for (id, s) in &self.records {
            check(&format!("record {id}"), s)?;
            let owned = s.fields.iter().any(|t| self.owns(*t));
            if owned != self.owns(Ty::Record(RecordTy(*id))) {
                return Err(format!(
                    "record {id}: owns {} but its fields say {owned}",
                    !owned
                ));
            }
        }
        for (id, e) in &self.enums {
            if e.lay.align < 8 || e.lay.size % e.lay.align != 0 {
                return Err(format!(
                    "enum {id}: size {} and alignment {} do not hold a tag word",
                    e.lay.size, e.lay.align
                ));
            }
            for (tag, v) in e.variants.iter().enumerate() {
                check(&format!("enum {id} variant {tag}"), v)?;
                if PAYLOAD % v.lay.align != 0 || PAYLOAD + v.lay.size > e.lay.size {
                    return Err(format!(
                        "enum {id}: variant {tag} does not fit its payload area"
                    ));
                }
            }
            let owned = e
                .variants
                .iter()
                .any(|v| v.fields.iter().any(|t| self.owns(*t)));
            if owned != self.owns(Ty::Enum(EnumTy(*id))) {
                return Err(format!(
                    "enum {id}: owns {} but its variants say {owned}",
                    !owned
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_str_is_three_words_with_an_eight_byte_length_on_every_target() {
        assert_eq!(DataLayout::LP64.str(), Lay::new(24, 8));
        assert_eq!(DataLayout::LP64.str_fields(), [0, 8, 16]);
        assert_eq!(DataLayout::WASM32.str(), Lay::new(24, 8));
        assert_eq!(DataLayout::WASM32.str_fields(), [0, 8, 16]);
        assert_eq!(DataLayout::with_pointer_bits(64), DataLayout::LP64);
        assert_eq!(DataLayout::with_pointer_bits(32), DataLayout::WASM32);
    }

    fn tables(records: Vec<(u32, Struct)>, enums: Vec<(u32, Sum)>) -> ByteLayouts {
        ByteLayouts {
            data: DataLayout::LP64,
            records: records.into_iter().collect(),
            enums: enums.into_iter().collect(),
            owning_records: BTreeMap::new(),
            owning_enums: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn st(offsets: &[u32], fields: &[Ty], size: u32, align: u32) -> Struct {
        Struct {
            offsets: offsets.to_vec(),
            fields: fields.to_vec(),
            lay: Lay::new(size, align),
        }
    }

    #[test]
    fn the_validator_accepts_a_sound_table_and_names_every_broken_one() {
        let sound = tables(vec![(0, st(&[0, 8], &[Ty::Bool, Ty::Int], 16, 8))], vec![]);
        assert_eq!(sound.verify(), Ok(()));
        let broken = [
            (st(&[0, 4], &[Ty::Int, Ty::Int], 16, 8), "overlaps"),
            (st(&[0, 9], &[Ty::Bool, Ty::Int], 24, 8), "not aligned"),
            (
                st(&[0, 8], &[Ty::Int, Ty::Int], 12, 8),
                "multiple of its alignment",
            ),
            (st(&[0, 8], &[Ty::Int, Ty::Int], 8, 8), "past its size"),
            (st(&[0], &[Ty::Int, Ty::Int], 16, 8), "offsets for"),
        ];
        for (s, why) in broken {
            let e = tables(vec![(0, s)], vec![]).verify().unwrap_err();
            assert!(e.contains(why), "{e}");
        }
        // A record that owns through a `Str` field, but is not marked as owning.
        let e = tables(vec![(0, st(&[0], &[Ty::Str], 24, 8))], vec![])
            .verify()
            .unwrap_err();
        assert!(e.contains("owns"), "{e}");
        // A variant that does not fit its enum's payload area.
        let e = tables(
            vec![],
            vec![(
                0,
                Sum {
                    variants: vec![st(&[0, 8], &[Ty::Int, Ty::Int], 16, 8)],
                    lay: Lay::new(16, 8),
                },
            )],
        )
        .verify()
        .unwrap_err();
        assert!(e.contains("does not fit its payload area"), "{e}");
    }
}
