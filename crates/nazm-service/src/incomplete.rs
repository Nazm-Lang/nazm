//! Completion at a structured hole: right after a `.` or a `(` with nothing written yet.
//! N26.
//!
//! # Why a probe
//!
//! `point.`, `State.`, `Point(` and `State.Done(` are unfinished statements. The parser
//! recovers from an unfinished statement by giving its function no body at all, so the
//! ordinary analysis has no receiver, no local and no scope to anchor anything to — and it
//! cannot be told otherwise, because which `.` or `(` is the one being completed is decided
//! by the editor's cursor, which only the request knows. A `.` two hundred lines away is
//! just broken code.
//!
//! So a request here runs a **probe**: the compilation's current sources, exactly as the
//! document's analysis loaded them, parsed by the same parser told the cursor's byte offset
//! and checked by the same checker ([`nazm_core::probe`]). At exactly that offset the
//! parser reads a zero-width, empty name — which no source can spell — and takes the `;`
//! or `)` the unfinished statement leaves unwritten as written; nothing else is changed,
//! no text is inserted, and every other mistake is parsed and reported as ever. The
//! checker then decides what the hole is a member of at the decision points it already
//! has:
//!
//! | Hole | Anchor, as the checker recorded it |
//! | --- | --- |
//! | `receiver.` | the type an unknown member was looked for on: a record's fields |
//! | `E.`, `E` naming an enum and no value | the enum, when it is not generic (N21's rule) |
//! | `E[T, …].` | the applied enum an unknown variant was looked for in |
//! | `Name(` | the record a construction head `Name` would build — only when no function is called `Name` |
//! | `E.V(`, in a construction or a pattern | the variant |
//!
//! Its candidates are then N21's, from the same function: identities, canonical order,
//! substituted types, details. The probe's diagnostics describe a program with a hole in it
//! and are never published; its resolution is read for this one answer and dropped with
//! it. Nothing is kept.
//!
//! # When the answer is complete
//!
//! N21's rule, unchanged, applied to the probe's own parse — in which the hole is not a
//! recovery: every other file parsed cleanly, and in the document every other recovery is
//! inside the body of a function other than the hole's.
//!
//! # A bare `(`
//!
//! Without a label written, `Name(` is a call as far as the grammar is concerned, and the
//! probe reads it as one. If the module can call a function `Name`, it is that call —
//! signature help's, not this — and nothing is offered, whether or not a record is also
//! called `Name`. Only where no function is, the record the module's types name `Name`
//! anchors its labels, read as a construction's head is read.

use nazm_core::__syntax::SyntaxKind;
use nazm_core::sema::ReferenceTarget;
use nazm_core::{Resolution, Type};
use nazm_span::Span;

use crate::completion::{Completion, NoCompletion};
use crate::service::{Location, Snapshot, Target};
use crate::structure::{candidates, complete, StructureSite};

/// Completion at the hole ending at byte `offset` of the document, if a `.` or `(` ends
/// exactly there.
pub(crate) fn completion(s: &Snapshot, offset: u32) -> Result<Completion, NoCompletion> {
    // The punctuation, as the concrete tree has it: a `.` or `(` token ending at the cursor.
    // A dot in a string or a comment is inside a leaf of another kind, and is never one.
    // A `..` (R1a) is two dots to completion, as it was before it was one token: with the cursor
    // between them, `a.|.c`, a member chain missing a name; after them, a second dot.
    let punctuation = s
        .cst
        .leaves()
        .into_iter()
        .find(|l| {
            !l.kind.is_trivia()
                && (l.span.end == offset
                    || (l.kind == SyntaxKind::DotDot && l.span.start + 1 == offset))
        })
        .map(|l| match l.kind {
            SyntaxKind::DotDot => SyntaxKind::Dot,
            k => k,
        });
    if !matches!(punctuation, Some(SyntaxKind::Dot | SyntaxKind::LParen)) {
        return Err(NoCompletion::NotAName);
    }
    let probe = nazm_core::probe(&s.map, &s.graph, offset);
    let res = &probe.analysis.resolution;
    let hole = probe.hole.ok_or(NoCompletion::NotACompletionSite)?;
    let root = probe
        .root
        .as_ref()
        .ok_or(NoCompletion::NotACompletionSite)?;
    complete(&probe.analysis.syntax_errors, root.root(), offset)
        .map_err(|_| NoCompletion::RecoveredSyntax)?;
    let site = anchor(res, hole)?;
    let (context, items) = candidates(res, site, "", &[], |t| declaration(s, res, t));
    Ok(Completion {
        context,
        start: offset,
        end: offset,
        items,
    })
}

/// What the checker made the hole a member of.
fn anchor(
    res: &Resolution,
    hole: nazm_core::__syntax::HoleSite,
) -> Result<StructureSite, NoCompletion> {
    use nazm_core::__syntax::HoleSite;
    match hole {
        HoleSite::Member { name } => match res.looked_up_on(name) {
            Some(Type::Record(record)) => Ok(StructureSite::Member { record }),
            Some(Type::Enum(enumeration)) => Ok(StructureSite::Variant { enumeration }),
            _ => Err(NoCompletion::Unanchored),
        },
        HoleSite::Call { callee } => {
            if res.callee(callee).is_some() {
                // A function is what `Name(` calls: its arguments, not a record's labels.
                return Err(NoCompletion::NotACompletionSite);
            }
            let record = res.construction(callee).ok_or(NoCompletion::Unanchored)?;
            Ok(StructureSite::RecordLabel { record })
        }
        HoleSite::Payload { variant, pattern } => {
            let variant = res.variant(variant).ok_or(NoCompletion::Unanchored)?;
            Ok(StructureSite::PayloadLabel { variant, pattern })
        }
    }
}

/// Where a member of the probe's resolution is declared: its definition's name, a position
/// in the same files the document's analysis holds.
fn declaration(s: &Snapshot, res: &Resolution, t: ReferenceTarget) -> Option<Location> {
    let span: Span = match t {
        ReferenceTarget::Field(f) => {
            res.records()
                .get(f.record)?
                .fields
                .get(f.field as usize)?
                .name_span
        }
        ReferenceTarget::Variant(v) => {
            res.enums()
                .get(v.enumeration)?
                .variants
                .get(v.variant as usize)?
                .name_span
        }
        ReferenceTarget::PayloadField(p) => {
            res.enums()
                .get(p.enumeration)?
                .variants
                .get(p.variant as usize)?
                .fields
                .get(p.field as usize)?
                .name_span
        }
        _ => return None,
    };
    match s.target(span)? {
        Target::File(l) => Some(l),
        Target::Prelude { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    /// The structural half of "the checker decides": this module never looks a record,
    /// enum, function, field or variant up by its spelling, never reads the text around the
    /// hole, and never publishes or keeps the probe.
    #[test]
    fn a_hole_is_never_anchored_by_spelling() {
        let source = include_str!("incomplete.rs");
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
            ".name ==",
            "by_name(",
            "definition(",
            "declared(",
            "visible_at(",
            ".field(",
            ".variant(&",
            "texts",
            "l.text",
            "documents",
            "diagnostics",
            "Published",
            "static ",
            "thread_local",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in incomplete: a hole would not be the checker's"
            );
        }
    }
}
