//! Exhaustiveness and reachability of patterns (R1a, `docs/spec.md`, *Patterns*).
//!
//! The usefulness question — *does this row of patterns match a value no earlier row matches?* —
//! answered over a matrix of patterns by specialising on constructors, the standard algorithm.
//! An arm is unreachable when its pattern is not useful against the arms above it; a `match` is
//! exhaustive when `_` is not useful against all of its arms, and the witness that makes it
//! useful is the value a message names.
//!
//! Pure: the column types and what a constructor's fields are come in through [`Signatures`],
//! so nothing here reads the checker's tables, and every walk is over the patterns written.

use nazm_sema::{EnumId, Type};

/// A pattern as this module sees it: what it matches, never what it binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pat {
    /// `_` or a name.
    Wild,
    /// `true` or `false`.
    Bool(bool),
    /// An integer or string literal, by a canonical key of its value: an infinite type's
    /// constructor, never all of them.
    Lit(String),
    /// A variant, its payload fields' patterns in declaration order.
    Ctor {
        enumeration: EnumId,
        variant: u32,
        args: Vec<Pat>,
    },
    Or(Vec<Pat>),
    /// A tuple (L5), its elements' patterns: the one constructor of a tuple type.
    Tuple(Vec<Pat>),
}

/// What a column's type is made of.
pub(super) trait Signatures {
    /// The variants of an enum type, each with its payload fields' types in declaration order;
    /// `None` for any other type.
    fn variants(&self, t: Type) -> Option<(EnumId, Vec<Vec<Type>>)>;
    /// A variant's spelling and its fields' names, for a witness.
    fn spell(&self, enumeration: EnumId, variant: u32) -> (String, Vec<String>);
    /// A tuple type's element types (L5); `None` for any other type.
    fn tuple(&self, t: Type) -> Option<Vec<Type>>;
}

/// A head constructor a row can be specialised on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Head {
    Bool(bool),
    Lit(String),
    Variant(u32),
    Tuple,
}

fn head_of(p: &Pat) -> Option<Head> {
    match p {
        Pat::Bool(b) => Some(Head::Bool(*b)),
        Pat::Lit(k) => Some(Head::Lit(k.clone())),
        Pat::Ctor { variant, .. } => Some(Head::Variant(*variant)),
        Pat::Tuple(_) => Some(Head::Tuple),
        Pat::Wild | Pat::Or(_) => None,
    }
}

/// Rows with every or-pattern at their head spread into one row per alternative.
fn spread(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    let mut out = Vec::new();
    let mut work: Vec<Vec<Pat>> = rows.iter().rev().cloned().collect();
    while let Some(row) = work.pop() {
        match row.first() {
            Some(Pat::Or(alts)) => {
                for alt in alts.iter().rev() {
                    let mut r = vec![alt.clone()];
                    r.extend(row.iter().skip(1).cloned());
                    work.push(r);
                }
            }
            _ => out.push(row),
        }
    }
    out
}

/// The rows whose head matches `head`, with its arguments — `arity` wildcards for a `_` —
/// in front of the rest.
fn specialise(rows: &[Vec<Pat>], head: &Head, arity: usize) -> Vec<Vec<Pat>> {
    spread(rows)
        .into_iter()
        .filter_map(|row| {
            let (first, rest) = row.split_first()?;
            let mut out = match first {
                Pat::Wild => vec![Pat::Wild; arity],
                p if head_of(p).as_ref() == Some(head) => match p {
                    Pat::Ctor { args, .. } | Pat::Tuple(args) => args.clone(),
                    _ => Vec::new(),
                },
                _ => return None,
            };
            out.extend(rest.iter().cloned());
            Some(out)
        })
        .collect()
}

/// The rows whose head is `_`, without it.
fn default(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    spread(rows)
        .into_iter()
        .filter(|row| matches!(row.first(), Some(Pat::Wild)))
        .map(|row| row.into_iter().skip(1).collect())
        .collect()
}

/// A value a pattern matrix does not match, as a pattern of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Witness {
    Any,
    Bool(bool),
    Tuple(Vec<Witness>),
    Variant {
        enumeration: EnumId,
        variant: u32,
        args: Vec<Witness>,
    },
}

impl Witness {
    pub(super) fn show(&self, sig: &dyn Signatures) -> String {
        match self {
            Self::Any => "_".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Tuple(args) => {
                let inner: Vec<String> = args.iter().map(|a| a.show(sig)).collect();
                format!("({})", inner.join(", "))
            }
            Self::Variant {
                enumeration,
                variant,
                args,
            } => {
                let (name, fields) = sig.spell(*enumeration, *variant);
                let inner: Vec<String> = fields
                    .iter()
                    .zip(args)
                    .map(|(f, a)| format!("{f}: {}", a.show(sig)))
                    .collect();
                format!("{name}({})", inner.join(", "))
            }
        }
    }
}

/// Bounds the search: a pattern nested deeper than this is treated as covering everything
/// below it. No pattern a program writes comes near it; it keeps a malformed matrix finite.
const DEPTH: usize = 256;

/// A row of values, one per column of `tys`, that no row of `rows` matches; `None` when every
/// value is matched.
pub(super) fn witness(
    rows: &[Vec<Pat>],
    tys: &[Type],
    sig: &dyn Signatures,
) -> Option<Vec<Witness>> {
    witness_at(rows, tys, sig, 0)
}

/// One constructor of a finite type: its head, its fields' types, and the enum it belongs to.
type Constructor = (Head, Vec<Type>, Option<EnumId>);

fn witness_at(
    rows: &[Vec<Pat>],
    tys: &[Type],
    sig: &dyn Signatures,
    depth: usize,
) -> Option<Vec<Witness>> {
    let Some((ty, rest_tys)) = tys.split_first() else {
        return rows.is_empty().then(Vec::new);
    };
    if depth > DEPTH {
        return None;
    }
    let rows = spread(rows);
    // The constructors of a finite type: every variant, or `true` and `false`.
    let finite: Option<Vec<Constructor>> = if let Some((id, variants)) = sig.variants(*ty) {
        Some(
            variants
                .into_iter()
                .enumerate()
                .map(|(i, f)| {
                    (
                        Head::Variant(u32::try_from(i).unwrap_or(u32::MAX)),
                        f,
                        Some(id),
                    )
                })
                .collect(),
        )
    } else if *ty == Type::Bool {
        Some(vec![
            (Head::Bool(true), Vec::new(), None),
            (Head::Bool(false), Vec::new(), None),
        ])
    } else {
        sig.tuple(*ty).map(|elems| vec![(Head::Tuple, elems, None)])
    };
    let present: Vec<Head> = rows
        .iter()
        .filter_map(|r| r.first().and_then(head_of))
        .collect();
    if let Some(all) = &finite {
        let complete = all.iter().all(|(h, _, _)| present.contains(h));
        if complete {
            for (head, fields, id) in all {
                let mut sub_tys = fields.clone();
                sub_tys.extend(rest_tys.iter().copied());
                let sub = specialise(&rows, head, fields.len());
                if let Some(w) = witness_at(&sub, &sub_tys, sig, depth + 1) {
                    let (args, rest) = w.split_at(fields.len().min(w.len()));
                    let mut out = vec![build(head, args.to_vec(), *id)];
                    out.extend(rest.iter().cloned());
                    return Some(out);
                }
            }
            return None;
        }
    }
    // Some constructor is missing (or there are infinitely many): what `_` rows leave.
    let w = witness_at(&default(&rows), rest_tys, sig, depth + 1)?;
    let first = finite
        .and_then(|all| {
            all.into_iter()
                .find(|(h, _, _)| !present.contains(h))
                .map(|(h, fields, id)| build(&h, vec![Witness::Any; fields.len()], id))
        })
        .unwrap_or(Witness::Any);
    let mut out = vec![first];
    out.extend(w);
    Some(out)
}

fn build(head: &Head, args: Vec<Witness>, id: Option<EnumId>) -> Witness {
    match (head, id) {
        (Head::Bool(b), _) => Witness::Bool(*b),
        (Head::Tuple, _) => Witness::Tuple(args),
        (Head::Variant(v), Some(enumeration)) => Witness::Variant {
            enumeration,
            variant: *v,
            args,
        },
        _ => Witness::Any,
    }
}

/// Whether `row` matches some value no row of `rows` matches.
pub(super) fn useful(rows: &[Vec<Pat>], row: &[Pat], tys: &[Type], sig: &dyn Signatures) -> bool {
    useful_at(rows, row, tys, sig, 0)
}

fn useful_at(
    rows: &[Vec<Pat>],
    row: &[Pat],
    tys: &[Type],
    sig: &dyn Signatures,
    depth: usize,
) -> bool {
    let (Some((first, rest)), Some((ty, rest_tys))) = (row.split_first(), tys.split_first()) else {
        return rows.is_empty();
    };
    if depth > DEPTH {
        return false;
    }
    match first {
        Pat::Or(alts) => alts.iter().any(|a| {
            let mut r = vec![a.clone()];
            r.extend(rest.iter().cloned());
            useful_at(rows, &r, tys, sig, depth + 1)
        }),
        Pat::Wild => {
            let rows = spread(rows);
            let present: Vec<Head> = rows
                .iter()
                .filter_map(|r| r.first().and_then(head_of))
                .collect();
            let finite: Option<Vec<(Head, Vec<Type>)>> =
                if let Some((_, variants)) = sig.variants(*ty) {
                    Some(
                        variants
                            .into_iter()
                            .enumerate()
                            .map(|(i, f)| (Head::Variant(u32::try_from(i).unwrap_or(u32::MAX)), f))
                            .collect(),
                    )
                } else if *ty == Type::Bool {
                    Some(vec![
                        (Head::Bool(true), Vec::new()),
                        (Head::Bool(false), Vec::new()),
                    ])
                } else {
                    sig.tuple(*ty).map(|elems| vec![(Head::Tuple, elems)])
                };
            match finite {
                Some(all) if all.iter().all(|(h, _)| present.contains(h)) => {
                    all.into_iter().any(|(head, fields)| {
                        let mut r = vec![Pat::Wild; fields.len()];
                        r.extend(rest.iter().cloned());
                        let mut t = fields.clone();
                        t.extend(rest_tys.iter().copied());
                        useful_at(
                            &specialise(&rows, &head, fields.len()),
                            &r,
                            &t,
                            sig,
                            depth + 1,
                        )
                    })
                }
                _ => useful_at(&default(&rows), rest, rest_tys, sig, depth + 1),
            }
        }
        p => {
            let Some(head) = head_of(p) else { return false };
            let (args, fields): (Vec<Pat>, Vec<Type>) = match p {
                Pat::Tuple(args) => (
                    args.clone(),
                    sig.tuple(*ty)
                        .unwrap_or_else(|| vec![Type::Int; args.len()]),
                ),
                Pat::Ctor { args, variant, .. } => {
                    let fields = sig
                        .variants(*ty)
                        .and_then(|(_, v)| v.get(*variant as usize).cloned())
                        .unwrap_or_else(|| vec![Type::Int; args.len()]);
                    (args.clone(), fields)
                }
                _ => (Vec::new(), Vec::new()),
            };
            let mut r = args;
            r.extend(rest.iter().cloned());
            let mut t = fields;
            t.extend(rest_tys.iter().copied());
            let arity = r.len() - rest.len();
            useful_at(&specialise(rows, &head, arity), &r, &t, sig, depth + 1)
        }
    }
}
