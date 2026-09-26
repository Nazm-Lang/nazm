//! Structure at a position: the field, variant or label a position names a member of, and
//! completion and constructor signature help from it. N21.
//!
//! # The contract
//!
//! **Structure is identified by what the checker decided, never by spelling.** The concrete
//! tree says which syntactic site a position is in — the name after a projection's `.`, the
//! variant after `E.`, a construction's label, a pattern's payload label — and the
//! resolution says what that site is a member *of*:
//!
//! | Site | Anchor, as the checker recorded it |
//! | --- | --- |
//! | `receiver.name`, and a field step of an assignment's place | the field's [`FieldRef`] (its record), or the record an unknown field was looked for on |
//! | `E.name` in a variant construction or pattern | the variant's [`VariantRef`] (its enum), or the enum an unknown variant was looked for in |
//! | `E.name` with no `(` yet, `E` no value | the enum the checker found `E` names, when it is not generic |
//! | a record construction's label | the record the construction builds |
//! | a variant construction's or pattern's payload label | the variant it names |
//!
//! Every one of those is keyed by the span of a name the checker checked — the record and
//! enum identities of an applied generic are its instance's, whose fields and payloads are
//! already substituted. The candidates are that record's fields, that enum's variants or
//! that variant's payload fields, in the definition's **canonical** order (sorted by name,
//! the order the backend lays them out in): declaration order is not part of what a record
//! or enum means, so it is not part of what a tool shows. A site with no anchor has no
//! answer; nothing is looked up by name.
//!
//! # When the answer is complete
//!
//! A candidate list comes from a definition, and its anchor from the checker's reading of
//! the site's function and of the module's names. Both are complete when every file of the
//! compilation but the document parsed without recovery, and in the document every
//! recovery is inside the body of a function other than the one the position is in: a
//! recovered statement in another body can neither lose a declaration the module's names
//! need nor change a local this site reads. Anything else — a recovery at the top level, in
//! a declaration, in the site's own function, or in another file — has no answer.

use nazm_core::__syntax::{CstNode, Leaf, SyntaxKind};
use nazm_core::sema::{
    Candidate, EnumId, FieldRef, PayloadRef, RecordId, ReferenceTarget, VariantRef,
};
use nazm_core::{Resolution, Type};
use nazm_span::{FileId, Span};

use crate::completion::{Completion, CompletionCandidate, Context, NoCompletion};
use crate::service::{name_at, LanguageService, Snapshot};
use crate::signature::{push_list, NoCall, Rendered};

/// What a structure site is a member of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureSite {
    /// A field name after `.`: a field of this record.
    Member { record: RecordId },
    /// A variant name after `E.`: a variant of this enum.
    Variant { enumeration: EnumId },
    /// A label of a construction of this record.
    RecordLabel { record: RecordId },
    /// A payload label of this variant, in a construction or (`pattern`) a pattern.
    PayloadLabel { variant: VariantRef, pattern: bool },
}

/// The structure site at a position: what it is a member of, and which identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub site: StructureSite,
    /// The identifier at the position, which a completion replaces whole.
    pub start: u32,
    pub end: u32,
    /// For a label: the fields the same construction or pattern already gives elsewhere,
    /// by the identity the checker recorded for each other label. The label at the
    /// position is not among them — it is the one being replaced.
    pub provided: Vec<ReferenceTarget>,
}

/// Why there is no structure site. There is never a guessed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoStructure {
    /// The document is not open at that version.
    NotCurrent,
    /// The position is not on an identifier.
    NotAName,
    /// The identifier is not a field, variant or label position.
    NotAStructureSite,
    /// The checker identified no record, enum or variant for the site.
    Unanchored,
    /// A recovery could have changed the anchor or lost a candidate.
    RecoveredSyntax,
}

impl LanguageService {
    /// The structure site at byte `offset` of `path`, at `version`.
    ///
    /// # Errors
    ///
    /// The [`NoStructure`] that applies.
    pub fn structure_at(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<Structure, NoStructure> {
        let s = self.current(path, version).ok_or(NoStructure::NotCurrent)?;
        let at = name_at(&s.cst, offset).ok_or(NoStructure::NotAName)?;
        structure(s, at)
    }

    /// Constructor signature help at byte `offset` of `path`, at `version`: the record or
    /// variant construction whose parentheses hold the position.
    ///
    /// # Errors
    ///
    /// [`NoCall::NotCurrent`]; [`NoCall::NotInACall`] outside every construction's
    /// parentheses; [`NoCall::NotChecked`] where the checker identified no record or
    /// variant; [`NoCall::RecoveredSyntax`] by this module's completeness rule.
    pub fn constructor_help(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<ConstructorHelp, NoCall> {
        let s = self.current(path, version).ok_or(NoCall::NotCurrent)?;
        let (node, _) = construction_at(s.cst.root(), offset).ok_or(NoCall::NotInACall)?;
        constructor_help(s, node, offset)
    }
}

/// The site `at` is in, anchored.
fn structure(s: &Snapshot, at: Span) -> Result<Structure, NoStructure> {
    let path = path_to(s.cst.root(), at);
    let node = *path.last().ok_or(NoStructure::NotAStructureSite)?;
    let leaves = significant(node);
    let index = leaves
        .iter()
        .position(|l| l.span == at)
        .ok_or(NoStructure::NotAStructureSite)?;
    let res = &s.analysis.resolution;
    let after_dot = index > 0 && leaves.get(index - 1).map(|l| l.kind) == Some(SyntaxKind::Dot);
    let labelled = leaves.get(index + 1).map(|l| l.kind) == Some(SyntaxKind::Colon);
    let site_kind = matches!(
        (node.kind(), after_dot, index == 0 && labelled),
        (
            SyntaxKind::FieldExpr
                | SyntaxKind::Place
                | SyntaxKind::VariantExpr
                | SyntaxKind::Pattern,
            true,
            _
        ) | (SyntaxKind::FieldInit | SyntaxKind::PatternField, _, true)
    );
    if !site_kind {
        return Err(NoStructure::NotAStructureSite);
    }
    // Complete before anchored: where the rule fails, no anchor is trusted either way.
    complete(&s.analysis.syntax_errors, s.cst.root(), at.start)?;
    let site = match node.kind() {
        SyntaxKind::FieldExpr | SyntaxKind::Place if after_dot => {
            match (res.field(at), res.looked_up_on(at)) {
                (Some(f), _) => StructureSite::Member { record: f.record },
                (None, Some(Type::Record(record))) => StructureSite::Member { record },
                (None, Some(Type::Enum(enumeration))) => StructureSite::Variant { enumeration },
                _ => return Err(NoStructure::Unanchored),
            }
        }
        SyntaxKind::VariantExpr | SyntaxKind::Pattern if after_dot => {
            match (res.variant(at), res.looked_up_on(at)) {
                (Some(v), _) => StructureSite::Variant {
                    enumeration: v.enumeration,
                },
                (None, Some(Type::Enum(enumeration))) => StructureSite::Variant { enumeration },
                _ => return Err(NoStructure::Unanchored),
            }
        }
        SyntaxKind::FieldInit | SyntaxKind::PatternField if index == 0 && labelled => {
            label_site(res, &path)?
        }
        _ => return Err(NoStructure::NotAStructureSite),
    };
    let provided = match site {
        StructureSite::RecordLabel { .. } | StructureSite::PayloadLabel { .. } => path
            .get(path.len().wrapping_sub(2))
            .map(|list| others(res, *list, at))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    Ok(Structure {
        site,
        start: at.start,
        end: at.end,
        provided,
    })
}

/// A label's site: the construction or pattern two levels up names what it labels.
fn label_site(res: &Resolution, path: &[CstNode<'_>]) -> Result<StructureSite, NoStructure> {
    let owner = path
        .get(path.len().wrapping_sub(3))
        .ok_or(NoStructure::NotAStructureSite)?;
    match owner.kind() {
        SyntaxKind::ConstructExpr => {
            let head = significant(*owner)
                .into_iter()
                .find(|l| l.kind == SyntaxKind::Ident)
                .ok_or(NoStructure::NotAStructureSite)?;
            let record = res.construction(head.span).ok_or(NoStructure::Unanchored)?;
            Ok(StructureSite::RecordLabel { record })
        }
        SyntaxKind::VariantExpr | SyntaxKind::Pattern => {
            let variant = variant_name(*owner).ok_or(NoStructure::NotAStructureSite)?;
            let variant = res.variant(variant).ok_or(NoStructure::Unanchored)?;
            Ok(StructureSite::PayloadLabel {
                variant,
                pattern: owner.kind() == SyntaxKind::Pattern,
            })
        }
        _ => Err(NoStructure::NotAStructureSite),
    }
}

/// The identities of every label in `list` other than the one at `at`, as the checker
/// recorded them. A label it could not resolve has none, and excludes nothing.
fn others(res: &Resolution, list: CstNode<'_>, at: Span) -> Vec<ReferenceTarget> {
    list.children()
        .into_iter()
        .filter(|c| matches!(c.kind(), SyntaxKind::FieldInit | SyntaxKind::PatternField))
        .filter_map(|c| significant(c).into_iter().next())
        .filter(|l| l.kind == SyntaxKind::Ident && l.span != at)
        .filter_map(|l| match (res.field(l.span), res.payload(l.span)) {
            (Some(f), _) => Some(ReferenceTarget::Field(f)),
            (None, Some(p)) => Some(ReferenceTarget::PayloadField(p)),
            _ => None,
        })
        .collect()
}

/// The name after `E.` of a variant construction or pattern.
fn variant_name(node: CstNode<'_>) -> Option<Span> {
    let leaves = significant(node);
    let dot = leaves.iter().position(|l| l.kind == SyntaxKind::Dot)?;
    leaves
        .get(dot + 1)
        .filter(|l| l.kind == SyntaxKind::Ident)
        .map(|l| l.span)
}

/// A node's own leaves, without trivia.
fn significant(node: CstNode<'_>) -> Vec<Leaf<'_>> {
    node.child_leaves()
        .into_iter()
        .filter(|l| !l.kind.is_trivia())
        .collect()
}

/// The nodes from the root down to the innermost one holding `span`.
fn path_to(root: CstNode<'_>, span: Span) -> Vec<CstNode<'_>> {
    let mut path = Vec::new();
    let mut at = Some(root);
    while let Some(node) = at {
        path.push(node);
        at = node.children().into_iter().find(|c| {
            let n = c.span();
            n.start <= span.start && span.end <= n.end
        });
    }
    path
}

/// This module's completeness rule, at `offset` of the document: over the files with a
/// syntax error, and the document's concrete tree. A completion probe (N26) applies it to
/// its own parse, in which its hole is not a recovery.
pub(crate) fn complete(
    syntax_errors: &[FileId],
    root: CstNode<'_>,
    offset: u32,
) -> Result<(), NoStructure> {
    if syntax_errors.is_empty() {
        return Ok(());
    }
    if syntax_errors.iter().any(|f| *f != FileId::ROOT) {
        return Err(NoStructure::RecoveredSyntax);
    }
    for item in root.children() {
        let broken = item
            .descendants()
            .into_iter()
            .filter(|n| n.kind() == SyntaxKind::Error)
            .collect::<Vec<_>>();
        if broken.is_empty() {
            continue;
        }
        let span = item.span();
        let here = span.start <= offset && offset <= span.end;
        let body = item
            .children()
            .into_iter()
            .find(|c| c.kind() == SyntaxKind::Block)
            .map(|b| b.span());
        let inside_body = |e: &CstNode<'_>| {
            body.is_some_and(|b| b.start <= e.span().start && e.span().end <= b.end)
        };
        if item.kind() != SyntaxKind::FnItem || here || !broken.iter().all(inside_body) {
            return Err(NoStructure::RecoveredSyntax);
        }
    }
    Ok(())
}

/// One member of a structure, in canonical order: its identity, name and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub target: ReferenceTarget,
    pub name: String,
    /// A field's or payload field's type, as the record or variant has it — an applied
    /// generic's substituted. `None` for a variant, which has payload fields instead.
    pub ty: Option<Type>,
}

/// The members a site offers, canonical order, before filtering.
pub(crate) fn members(res: &Resolution, site: StructureSite) -> Vec<Member> {
    match site {
        StructureSite::Member { record } | StructureSite::RecordLabel { record } => {
            record_fields(res, record)
        }
        StructureSite::Variant { enumeration } => res
            .enums()
            .get(enumeration)
            .map(|e| {
                e.in_tag_order()
                    .map(|(i, v)| Member {
                        target: ReferenceTarget::Variant(VariantRef {
                            enumeration,
                            variant: i,
                        }),
                        name: v.name.clone(),
                        ty: None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        StructureSite::PayloadLabel { variant, .. } => payload_fields(res, variant),
    }
}

fn record_fields(res: &Resolution, record: RecordId) -> Vec<Member> {
    res.records()
        .get(record)
        .map(|r| {
            r.in_layout_order()
                .map(|(i, f)| Member {
                    target: ReferenceTarget::Field(FieldRef { record, field: i }),
                    name: f.name.clone(),
                    ty: Some(f.ty),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn payload_fields(res: &Resolution, variant: VariantRef) -> Vec<Member> {
    res.enums()
        .get(variant.enumeration)
        .and_then(|e| e.variants.get(variant.variant as usize))
        .map(|v| {
            v.in_layout_order()
                .map(|(i, f)| Member {
                    target: ReferenceTarget::PayloadField(PayloadRef {
                        enumeration: variant.enumeration,
                        variant: variant.variant,
                        field: i,
                    }),
                    name: f.name.clone(),
                    ty: Some(f.ty),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Completion of the structure site at `at`, for [`LanguageService::completion`]: the
/// site's members whose names begin with what is written up to `offset`, less the labels
/// given elsewhere, in canonical order.
pub(crate) fn completion(s: &Snapshot, at: Span, offset: u32) -> Result<Completion, NoCompletion> {
    let found = structure(s, at).map_err(|e| match e {
        NoStructure::NotCurrent => NoCompletion::NotCurrent,
        NoStructure::NotAName => NoCompletion::NotAName,
        NoStructure::NotAStructureSite => NoCompletion::NotACompletionSite,
        NoStructure::Unanchored => NoCompletion::Unanchored,
        NoStructure::RecoveredSyntax => NoCompletion::RecoveredSyntax,
    })?;
    let res = &s.analysis.resolution;
    let text = s.texts.first().map_or("", |t| &**t);
    let prefix = text
        .get(at.start as usize..offset.min(at.end) as usize)
        .unwrap_or_default();
    let (context, items) = candidates(res, found.site, prefix, &found.provided, |t| {
        crate::completion::declaration(s, Candidate::Entity(t))
    });
    Ok(Completion {
        context,
        start: found.start,
        end: found.end,
        items,
    })
}

/// A site's candidates: its members whose names begin with `prefix`, less the labels in
/// `provided`, in canonical order, each with its identity, detail and declaration — one
/// policy for a name being written (N21) and for a hole with nothing written yet (N26).
pub(crate) fn candidates(
    res: &Resolution,
    site: StructureSite,
    prefix: &str,
    provided: &[ReferenceTarget],
    declaration: impl Fn(ReferenceTarget) -> Option<crate::Location>,
) -> (Context, Vec<CompletionCandidate>) {
    let (context, kind) = match site {
        StructureSite::Member { .. } => (Context::Member, "field"),
        StructureSite::Variant { .. } => (Context::Variant, "variant"),
        StructureSite::RecordLabel { .. } => (Context::RecordLabel, "field"),
        StructureSite::PayloadLabel { .. } => (Context::PayloadLabel, "payload field"),
    };
    let items = members(res, site)
        .into_iter()
        .filter(|m| m.name.starts_with(prefix))
        .filter(|m| !provided.contains(&m.target))
        .map(|m| CompletionCandidate {
            detail: Some(detail(res, &m)),
            declaration: declaration(m.target),
            candidate: Candidate::Entity(m.target),
            kind,
            name: m.name,
        })
        .collect();
    (context, items)
}

/// `name: Int` for a field, `Done(code: Int)` for a variant — with the types the record
/// or variant has, which for an applied generic are substituted.
fn detail(res: &Resolution, m: &Member) -> String {
    match (m.target, m.ty) {
        (ReferenceTarget::Variant(v), _) => {
            let mut label = m.name.clone();
            let fields = payload_fields(res, v);
            push_list(
                &mut label,
                res,
                fields
                    .iter()
                    .filter_map(|f| f.ty.map(|t| (Some(f.name.as_str()), t))),
            );
            label
        }
        (_, Some(t)) => format!("{}: {}", m.name, res.types().show(t)),
        (_, None) => m.name.clone(),
    }
}

// ---------------------------------------------------------------------------------------
// Constructor signature help.
// ---------------------------------------------------------------------------------------

/// What a construction builds: a record, or one variant of an enum. Not a call — a
/// construction names its fields, has no callee and runs no function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constructor {
    Record(RecordId),
    Variant(VariantRef),
}

/// Signature help for a construction: the shape the checker identified, and which of its
/// fields the position is giving a value to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructorHelp {
    pub constructor: Constructor,
    /// The construction.
    pub start: u32,
    pub end: u32,
    /// Its fields or payload fields, in canonical order, with substituted types.
    pub fields: Vec<Member>,
    /// The field whose initialiser the position is in, by the identity the checker
    /// recorded for its label. `None` between initialisers and on a label that did not
    /// resolve.
    pub active_field: Option<ReferenceTarget>,
    /// `Point(label: Str, x: Int)`, `Maybe[Int].Some(value: Int)`.
    pub rendered: Rendered,
    /// The active field's position in [`Self::fields`] and in the rendered list.
    pub active_parameter: Option<u32>,
}

/// The innermost record or variant construction whose parentheses hold `offset`, and
/// where its `(` ends — strictly after the `(` and no later than the `)`, by the tree's
/// nesting, as a call's argument list is found.
pub(crate) fn construction_at(root: CstNode<'_>, offset: u32) -> Option<(CstNode<'_>, u32)> {
    let mut found = None;
    let mut at = Some(root);
    while let Some(node) = at {
        if matches!(
            node.kind(),
            SyntaxKind::ConstructExpr | SyntaxKind::VariantExpr
        ) {
            let inits = node
                .children()
                .into_iter()
                .find(|c| c.kind() == SyntaxKind::FieldInitList);
            let bounds = inits.as_ref().map(|l| significant(*l)).and_then(|ls| {
                let open = ls.iter().find(|l| l.kind == SyntaxKind::LParen)?.span;
                let close = ls.iter().rev().find(|l| l.kind == SyntaxKind::RParen)?.span;
                Some((open, close))
            });
            if let Some((open, close)) = bounds {
                if open.end <= offset && close.start >= offset {
                    found = Some((node, open.end));
                }
            }
        }
        at = node.children().into_iter().find(|c| {
            let span = c.span();
            span.start <= offset && offset < span.end
        });
    }
    found
}

pub(crate) fn constructor_help(
    s: &Snapshot,
    node: CstNode<'_>,
    offset: u32,
) -> Result<ConstructorHelp, NoCall> {
    complete(&s.analysis.syntax_errors, s.cst.root(), offset)
        .map_err(|_| NoCall::RecoveredSyntax)?;
    let res = &s.analysis.resolution;
    let (constructor, fields, mut label) = if node.kind() == SyntaxKind::ConstructExpr {
        let head = significant(node)
            .into_iter()
            .find(|l| l.kind == SyntaxKind::Ident)
            .ok_or(NoCall::NotInACall)?;
        let record = res.construction(head.span).ok_or(NoCall::NotChecked)?;
        (
            Constructor::Record(record),
            record_fields(res, record),
            res.types().show(Type::Record(record)),
        )
    } else {
        let written = variant_name(node).ok_or(NoCall::NotInACall)?;
        let variant = res.variant(written).ok_or(NoCall::NotChecked)?;
        let owner = res.types().show(Type::Enum(variant.enumeration));
        let named = res
            .enums()
            .get(variant.enumeration)
            .and_then(|e| e.variants.get(variant.variant as usize))
            .map(|v| v.name.clone())
            .unwrap_or_default();
        (
            Constructor::Variant(variant),
            payload_fields(res, variant),
            format!("{owner}.{named}"),
        )
    };
    let params = push_list(
        &mut label,
        res,
        fields
            .iter()
            .filter_map(|f| f.ty.map(|t| (Some(f.name.as_str()), t))),
    );
    // The initialiser the position is in, by its label's recorded identity — never by
    // how many initialisers precede it, since initialisers are named and in any order.
    let active_field = node
        .children()
        .into_iter()
        .find(|c| c.kind() == SyntaxKind::FieldInitList)
        .and_then(|list| {
            list.children().into_iter().find(|c| {
                let span = c.span();
                c.kind() == SyntaxKind::FieldInit && span.start <= offset && offset <= span.end
            })
        })
        .and_then(|init| significant(init).into_iter().next())
        .and_then(|l| match (res.field(l.span), res.payload(l.span)) {
            (Some(f), _) => Some(ReferenceTarget::Field(f)),
            (None, Some(p)) => Some(ReferenceTarget::PayloadField(p)),
            _ => None,
        });
    let active_parameter = active_field
        .and_then(|a| fields.iter().position(|f| f.target == a))
        .and_then(|i| u32::try_from(i).ok());
    let span = node.span();
    Ok(ConstructorHelp {
        constructor,
        start: span.start,
        end: span.end,
        fields,
        active_field,
        rendered: Rendered { label, params },
        active_parameter,
    })
}

#[cfg(test)]
mod tests {
    /// The structural half of "never by spelling": this module takes a site to its record,
    /// enum or variant only through identities the checker recorded, and never looks a
    /// field, variant or type up by its name.
    #[test]
    fn structure_is_never_found_by_spelling() {
        let source = include_str!("structure.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            ".field(&",
            ".field(name",
            ".variant(&",
            ".variant(name",
            "by_name(",
            "definition(",
            "visible_at(",
            "substitute",
            ".name ==",
            ".text ==",
            "l.text",
            "HashMap<String",
            "BTreeMap<String",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in structure: it would be deciding what a name means"
            );
        }
    }
}
