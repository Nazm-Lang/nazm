//! Byte layout, as this backend decides it (`docs/architecture.md` §7.43).
//!
//! A word is 8 bytes, a `Bool` 1, a `Str` 24 (pointer, length, owner). A record is its fields in
//! the canonical order `nazm-lir` fixed, each at its natural alignment. An enum is an 8-byte
//! discriminant followed by one payload area as large as its largest variant, each variant laid
//! out as a record. Positions and discriminants are `nazm-lir`'s; only the bytes are decided here,
//! and no value laid out here ever meets one the LLVM backend laid out.

use std::collections::BTreeMap;

use nazm_lir::{EnumTy, RecordTy, Ty, Unit};

/// A size and an alignment, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lay {
    pub(crate) size: u32,
    pub(crate) align: u32,
}

impl Lay {
    const fn new(size: u32, align: u32) -> Self {
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

/// A struct's fields at their offsets, and the whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Struct {
    pub(crate) offsets: Vec<u32>,
    pub(crate) fields: Vec<Ty>,
    pub(crate) lay: Lay,
}

/// An enum: its discriminant, then its payload area; each variant's payload by tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sum {
    pub(crate) variants: Vec<Struct>,
    pub(crate) lay: Lay,
}

/// The offset of an enum's payload area.
pub(crate) const PAYLOAD: u32 = 8;

/// Every layout one unit needs.
#[derive(Debug, Clone, Default)]
pub(crate) struct Layouts {
    pub(crate) records: BTreeMap<u32, Struct>,
    pub(crate) enums: BTreeMap<u32, Sum>,
    /// By record or enum id, whether a value of it owns anything.
    owning_records: BTreeMap<u32, bool>,
    owning_enums: BTreeMap<u32, bool>,
}

impl Layouts {
    /// The layouts of a unit's types, from `nazm-lir`'s canonical tables. Computed in dependency
    /// order without recursion: a type's fields are laid out before it.
    #[must_use]
    pub(crate) fn of(unit: &Unit) -> Self {
        let mut out = Self::default();
        // Ownership first: a fixed point over the unit's own tables.
        let mut changed = true;
        for r in &unit.records {
            out.owning_records.insert(r.id.0, false);
        }
        for e in &unit.enums {
            out.owning_enums.insert(e.id.0, false);
        }
        while changed {
            changed = false;
            for r in &unit.records {
                let owns = r.fields.iter().any(|t| out.owns(*t));
                if owns && out.owning_records.get(&r.id.0) != Some(&true) {
                    out.owning_records.insert(r.id.0, true);
                    changed = true;
                }
            }
            for e in &unit.enums {
                let owns = e
                    .variants
                    .iter()
                    .any(|v| v.fields.iter().any(|t| out.owns(*t)));
                if owns && out.owning_enums.get(&e.id.0) != Some(&true) {
                    out.owning_enums.insert(e.id.0, true);
                    changed = true;
                }
            }
        }
        // Then layouts, repeatedly, until every type whose fields are laid out is laid out
        // itself. A unit's type graph is acyclic by value (the checker refuses a type that
        // contains itself), so this ends.
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

    /// A type's size and alignment, if it is known yet.
    #[must_use]
    pub(crate) fn lay(&self, t: Ty) -> Option<Lay> {
        match t {
            Ty::Int | Ty::Ints | Ty::Strs | Ty::Chan | Ty::Vec(_) => Some(Lay::new(8, 8)),
            Ty::Bool => Some(Lay::new(1, 1)),
            Ty::Str => Some(Lay::new(24, 8)),
            Ty::Record(RecordTy(id)) => self.records.get(&id).map(|s| s.lay),
            Ty::Enum(EnumTy(id)) => self.enums.get(&id).map(|s| s.lay),
        }
    }

    /// Whether a value of `t` owns anything a drop must give back.
    #[must_use]
    pub(crate) fn owns(&self, t: Ty) -> bool {
        match t {
            Ty::Int | Ty::Bool => false,
            Ty::Str | Ty::Ints | Ty::Strs | Ty::Chan | Ty::Vec(_) => true,
            Ty::Record(RecordTy(id)) => self.owning_records.get(&id).copied().unwrap_or(false),
            Ty::Enum(EnumTy(id)) => self.owning_enums.get(&id).copied().unwrap_or(false),
        }
    }
}

/// Whether a type is held in memory rather than in a register: a `Str`, a record, an enum.
#[must_use]
pub(crate) const fn in_memory(t: Ty) -> bool {
    matches!(t, Ty::Str | Ty::Record(_) | Ty::Enum(_))
}
