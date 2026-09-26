//! Fix plans: the fixes current compiler diagnostics already carry, as edits validated
//! against the current source. N24.
//!
//! # The contract
//!
//! **A fix plan is a projection of a [`Fix`] the current compiler diagnostic owns, and
//! nothing else.** The compiler attaches a fix — a span, a replacement, an
//! [`Applicability`] and a precondition — to the diagnostic it repairs; the service takes
//! the diagnostics of the document's current analysis, keeps those whose position the
//! request touches, and turns each of their fixes into one plan. Nothing here reads a
//! diagnostic's code or message to decide an edit, searches the source for a repair, or
//! trusts a diagnostic a client sends: a diagnostic with no fix has no plan.
//!
//! # What a plan promises
//!
//! It is one edit to one open document — a [`Fix`] has one span, and a span one file — at
//! the version and with the exact text the analysis read, with the bytes it replaces
//! recorded. [`LanguageService::fix_plan_is_current`] holds it to all of that, and to the
//! generation it was made at, with the same freshness law a rename answers to. A fix whose
//! file is not an open document, or whose span does not name a range of that text, has no
//! plan: there is no version to protect it with, or it would not be the edit it claims.
//!
//! Returning a plan applies nothing. The service writes no file and changes no buffer; an
//! editor that applies it sends the changed text back, and that is what changes the
//! service's state.
//!
//! # Which diagnostics a request covers
//!
//! Those whose primary span the requested range touches, by one rule, half-open: a
//! non-empty range and a non-empty span touch when they overlap (`[a, b)` and `[b, c)` do
//! not); an empty range — a cursor — touches a span it is inside or at either end of; and an
//! empty span touches a range that contains its position, ends included. The fix's own span
//! — often elsewhere: `mut` goes on the `let`, not on the assignment the diagnostic is about
//! — is where the edit goes, never how relevance is decided.

use std::path::Path;

use nazm_diag::{Applicability, Diagnostic, Fix};
use nazm_span::{FileId, Span};

use crate::rename::{Edit, FileEdit};
use crate::service::{FileKey, LanguageService, Snapshot, Target};

/// One current fix, as a validated edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlan {
    /// The current diagnostic the fix belongs to, as the analysis produced it.
    pub diagnostic: Diagnostic,
    /// Which of that diagnostic's fixes this is, in the order the compiler attached them.
    pub index: usize,
    /// How many fixes that diagnostic carries: the alternatives this one is among.
    pub alternatives: usize,
    pub description: String,
    pub applicability: Applicability,
    /// When the edit is the right one — present for every fix, automatic ones included.
    pub precondition: String,
    /// The service generation the plan was made at.
    pub generation: u64,
    /// The one file the fix edits, at its open version, with its exact text and the one
    /// edit.
    pub file: FileEdit,
}

/// Why there are no fix plans. There are never historical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoFixes {
    /// The document is not open at that version.
    NotCurrent,
}

impl LanguageService {
    /// The fixes of the current diagnostics of `path`, at `version`, whose primary span the
    /// byte range `start..end` touches — each as a plan, in diagnostic order and then the
    /// order the compiler attached them.
    ///
    /// # Errors
    ///
    /// [`NoFixes::NotCurrent`] unless the document is open at `version` and analysed at
    /// the current generation.
    pub fn fix_plans(
        &self,
        path: &Path,
        version: i32,
        start: u32,
        end: u32,
    ) -> Result<Vec<FixPlan>, NoFixes> {
        let s = self.current(path, version).ok_or(NoFixes::NotCurrent)?;
        let mut plans = Vec::new();
        for d in &s.analysis.diagnostics {
            let at = d.primary.span;
            if at.file != FileId::ROOT || !touches(at.start, at.end, start, end) {
                continue;
            }
            for (index, fix) in d.fixes.iter().enumerate() {
                if let Some(file) = self.planned(s, fix) {
                    plans.push(FixPlan {
                        diagnostic: d.clone(),
                        index,
                        alternatives: d.fixes.len(),
                        description: fix.description.clone(),
                        applicability: fix.applicability,
                        precondition: fix.precondition.clone(),
                        generation: self.generation,
                        file,
                    });
                }
            }
        }
        Ok(plans)
    }

    /// Whether `plan` still describes the current program: the same generation, its file
    /// open at the same version with the same text, and the bytes it replaces still there.
    #[must_use]
    pub fn fix_plan_is_current(&self, plan: &FixPlan) -> bool {
        plan.generation == self.generation
            && plan.file.version.is_some()
            && self.file_is_current(&plan.file)
    }

    /// The fix as an edit to an open document, validated against the text the analysis
    /// read — or nothing, when its file is not open or the span names no range of it.
    fn planned(&self, s: &Snapshot, fix: &Fix) -> Option<FileEdit> {
        let span: Span = fix.span;
        let FileKey::Path(key) = s.files.get(span.file.index())? else {
            return None;
        };
        let Target::File(at) = s.target(span)? else {
            return None;
        };
        // Only an open document can carry a version, and only a version lets an editor
        // refuse the edit if its buffer has moved on.
        let open = self.documents.get(key)?;
        if *open.text != *at.text {
            return None;
        }
        let expected = at.text.get(span.start as usize..span.end as usize)?;
        Some(FileEdit {
            path: open.path.clone(),
            version: Some(open.version),
            text: at.text.clone(),
            edits: vec![Edit {
                start: span.start,
                end: span.end,
                expected: expected.to_owned(),
                replacement: fix.replacement.clone(),
            }],
        })
    }
}

/// Whether the requested range `a..b` touches the span `s..e`, by the half-open rule in the
/// module documentation.
fn touches(s: u32, e: u32, a: u32, b: u32) -> bool {
    if a == b {
        s <= a && a <= e
    } else if s == e {
        a <= s && s <= b
    } else {
        a < e && s < b
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_request_touches_what_it_overlaps_and_a_cursor_what_it_is_on() {
        // A diagnostic over bytes 10..20.
        assert!(touches(10, 20, 15, 15), "a cursor inside");
        assert!(touches(10, 20, 10, 10), "a cursor at its start");
        assert!(touches(10, 20, 20, 20), "a cursor at its end");
        assert!(!touches(10, 20, 21, 21), "a cursor past it");
        assert!(touches(10, 20, 5, 11), "a range overlapping its start");
        assert!(touches(10, 20, 19, 30), "a range overlapping its end");
        assert!(!touches(10, 20, 20, 30), "a range adjacent after it");
        assert!(!touches(10, 20, 0, 10), "a range adjacent before it");
        assert!(!touches(10, 20, 30, 40), "a range elsewhere");
        // An empty span: an insertion point.
        assert!(touches(7, 7, 7, 7));
        assert!(touches(7, 7, 0, 7));
        assert!(touches(7, 7, 7, 9));
        assert!(!touches(7, 7, 8, 9));
    }

    fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                .spawn_scoped(scope, f)
                .expect("spawn")
                .join()
                .expect("join")
        })
    }

    #[test]
    fn alternatives_are_separate_plans_and_a_file_not_open_is_never_edited() {
        // No diagnostic the compiler produces today has more than one fix, or a fix in
        // another file, so one is attached to the current analysis here: three fixes for
        // one diagnostic — two overlapping alternatives in the document, and one in
        // `lib.nz`, which is not open.
        const LIB: &str = "pub fn one() -> Int { 1 }\n";
        const MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { one() }\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.nz"), LIB).unwrap();
        std::fs::write(dir.path().join("main.nz"), MAIN).unwrap();
        let (lib, main) = (dir.path().join("lib.nz"), dir.path().join("main.nz"));
        on_the_service_stack(move || {
            let mut s = LanguageService::new();
            s.open(&main, 1, MAIN.to_owned());
            let at = u32::try_from(MAIN.find("one()").unwrap()).unwrap();
            let lib_key = FileKey::Path(crate::service::identity(&lib));
            let snapshot = s
                .documents
                .get_mut(&crate::service::identity(&main))
                .and_then(|d| d.analysis.as_mut())
                .unwrap();
            let lib_file =
                FileId::from_index(snapshot.files.iter().position(|k| *k == lib_key).unwrap());
            let here = Span::new(FileId::ROOT, at, at + 3);
            let d = Diagnostic::error(nazm_diag::codes::TYPE_MISMATCH, here, "m")
                .fix_automatically(here, "two", "rename it", "it was meant to be `two`")
                .fix_with_review(
                    Span::new(FileId::ROOT, at, at + 5),
                    "0",
                    "replace the call",
                    "the value was all that was meant",
                )
                .fix_automatically(
                    Span::new(lib_file, 7, 10),
                    "fn",
                    "edit the library",
                    "the library was wrong",
                );
            snapshot.analysis.diagnostics.push(d);

            let plans = s.fix_plans(&main, 1, at, at).unwrap();
            let shown: Vec<(usize, usize, (u32, u32))> = plans
                .iter()
                .map(|p| {
                    let e = &p.file.edits[0];
                    (p.index, p.alternatives, (e.start, e.end))
                })
                .collect();
            // Two plans, one per fix, overlapping and neither merged into the other; the
            // fix in the closed file is not one of them.
            assert_eq!(shown, [(0, 3, (at, at + 3)), (1, 3, (at, at + 5))]);
            assert!(plans.iter().all(|p| s.fix_plan_is_current(p)));
            assert_eq!(plans[0].applicability, Applicability::Automatic);
            assert_eq!(plans[1].applicability, Applicability::NeedsReview);
            assert_eq!(plans[1].precondition, "the value was all that was meant");
            assert!(plans.iter().all(|p| p.file.edits.len() == 1));
        });
    }

    /// The structural half of "a projection, and nothing else": this module never matches
    /// a diagnostic's code or message, never searches text for a repair, and never writes.
    #[test]
    fn a_fix_plan_is_never_invented() {
        let source = include_str!("fixes.rs");
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
            ".code",
            ".message",
            "codes::",
            ".help",
            "find(\"",
            "contains(\"",
            "fs::write",
            "Disk",
            "parse_source(",
            "check(",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in fixes: it would be inventing or applying a repair"
            );
        }
    }
}
