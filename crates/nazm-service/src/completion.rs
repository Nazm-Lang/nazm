//! Scope at a position, and completion from it. N19.
//!
//! # The contract
//!
//! **Completion does not resolve names. It enumerates the candidates the checker recorded
//! as available** — its lexical frames, its bindings with the byte each becomes visible
//! from and the binding each hides, its module environments — through
//! [`nazm_core::sema::ScopeTrace::visible_at`]. The concrete tree decides only which *kind*
//! of name the position expects:
//!
//! | Where the cursor's identifier is | Context | Candidates |
//! | --- | --- | --- |
//! | a name used as a value (`NameRef`), or the root of an assignment's place | value | locals |
//! | the head of a call (`CallExpr`) | call | functions and built-ins |
//! | a written type (`Type`) | type | type parameters, records, enums, built-in types |
//!
//! Anywhere else — a field after `.`, a variant after `E.`, a construction or payload
//! label, a pattern, a declaration's own name, an import path — is not a completion site
//! in N19, and has no answer rather than a guessed one.
//!
//! # What is not claimed
//!
//! A completion is a suggestion for one document at its current version, from that
//! document's own compilation only — never merged with another open program's — and it
//! is not stale-safe in [`crate::RenamePlan`]'s sense: it proves nothing about an edit. A
//! position whose scope the checker could not establish in full (a body that did not
//! parse) has no completion rather than a list that might be missing names.

use nazm_core::__syntax::SyntaxKind;
use nazm_core::sema::{Candidate, ReferenceTarget, Unavailable, Visible};
use nazm_span::FileId;

use crate::service::{name_at, LanguageService, Snapshot};
use crate::{Location, Target};

/// Which kind of name a completion site expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Value,
    Call,
    Type,
}

/// One suggestion: a name, what it means, and what the compiler already knows about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub name: String,
    /// `local`, `function`, `built-in function`, `record`, `enum`, `built-in type` or
    /// `type parameter`.
    pub kind: &'static str,
    pub candidate: Candidate,
    /// A local's type or a function's signature, as hover shows them.
    pub detail: Option<String>,
    /// Where a program's own candidate is declared, when that is a file; `None` for a
    /// built-in, a type parameter, or a core-prelude definition.
    pub declaration: Option<Location>,
}

/// A completion: the token it replaces and every candidate that fits, in canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub context: Context,
    /// The identifier being completed, which an accepted suggestion replaces whole.
    pub start: u32,
    pub end: u32,
    /// Ordered by name, then by what the name means; complete for the context.
    pub items: Vec<CompletionCandidate>,
}

/// Why there is no completion. There is never a partial one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCompletion {
    /// The document is not open at that version.
    NotCurrent,
    /// The position is not on an identifier.
    NotAName,
    /// The identifier is not in a value, call or type position.
    NotACompletionSite,
    /// The checker could not establish the scope there.
    Unavailable(Unavailable),
    /// The source is a syntax recovery where this answer depends on it: the function the
    /// position is in, for its locals; any file of the compilation, for the functions and
    /// types a module may name. A recovered tree may have lost a declaration, so the
    /// checker's account of what is visible could be missing a name.
    RecoveredSyntax,
}

impl LanguageService {
    /// Every name that may be written at byte `offset` of `path`, at `version`, by
    /// namespace — at any position, not only on a name — from the document's own
    /// compilation.
    ///
    /// # Errors
    ///
    /// [`NoCompletion::NotCurrent`], or [`NoCompletion::Unavailable`] where the checker's
    /// account of the position is not complete.
    pub fn scope_at(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<Visible, NoCompletion> {
        let s = self
            .current(path, version)
            .ok_or(NoCompletion::NotCurrent)?;
        locals_are_complete(s, offset)?;
        module_names_are_complete(s)?;
        visible(s, offset)
    }

    /// The completion of the identifier at byte `offset` of `path`, at `version`: the
    /// candidates of its context whose names begin with what is written of it up to
    /// `offset`.
    ///
    /// # Errors
    ///
    /// The [`NoCompletion`] that applies.
    pub fn completion(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<Completion, NoCompletion> {
        let s = self
            .current(path, version)
            .ok_or(NoCompletion::NotCurrent)?;
        let at = name_at(&s.cst, offset).ok_or(NoCompletion::NotAName)?;
        let context = context(s, at).ok_or(NoCompletion::NotACompletionSite)?;
        match context {
            Context::Value => locals_are_complete(s, at.start)?,
            Context::Call | Context::Type => module_names_are_complete(s)?,
        }
        // The scope where the identifier starts: what could be written in its place.
        let names = visible(s, at.start)?;
        let text = s.texts.first().map_or("", |t| &**t);
        let prefix = text
            .get(at.start as usize..offset.min(at.end) as usize)
            .unwrap_or_default();
        let pool = match context {
            Context::Value => names.values,
            Context::Call => names.callables,
            Context::Type => names.types,
        };
        let mut items: Vec<CompletionCandidate> = pool
            .into_iter()
            .filter(|n| n.name.starts_with(prefix))
            .map(|n| CompletionCandidate {
                kind: kind(n.candidate),
                detail: detail(s, n.candidate),
                declaration: declaration(s, n.candidate),
                candidate: n.candidate,
                name: n.name,
            })
            .collect();
        items.sort_by(|a, b| (&a.name, a.candidate).cmp(&(&b.name, b.candidate)));
        items.dedup();
        Ok(Completion {
            context,
            start: at.start,
            end: at.end,
            items,
        })
    }
}

/// The scope at `offset` of the document's own file, in its own compilation.
fn visible(s: &Snapshot, offset: u32) -> Result<Visible, NoCompletion> {
    let module = s
        .graph
        .module_of(FileId::ROOT)
        .ok_or(NoCompletion::Unavailable(Unavailable::ModuleNotChecked))?;
    s.analysis
        .resolution
        .scopes()
        .visible_at(module, FileId::ROOT, offset)
        .map_err(NoCompletion::Unavailable)
}

/// Whether the function containing `offset`, if any, parsed without recovery: a recovered
/// statement may have been a `let` the checker never saw.
fn locals_are_complete(s: &Snapshot, offset: u32) -> Result<(), NoCompletion> {
    let here = nazm_span::Span::new(FileId::ROOT, offset, offset);
    let item = s
        .cst
        .root()
        .descendants()
        .into_iter()
        .filter(|n| n.kind() == SyntaxKind::FnItem)
        .find(|n| n.span().start <= here.start && here.start < n.span().end);
    match item {
        Some(f)
            if f.descendants()
                .iter()
                .any(|n| n.kind() == SyntaxKind::Error) =>
        {
            Err(NoCompletion::RecoveredSyntax)
        }
        _ => Ok(()),
    }
}

/// Whether every file of the compilation parsed without recovery: a lost item would be
/// missing from the environment the checker built.
fn module_names_are_complete(s: &Snapshot) -> Result<(), NoCompletion> {
    if s.analysis.syntax_errors.is_empty() {
        Ok(())
    } else {
        Err(NoCompletion::RecoveredSyntax)
    }
}

/// Which kind of name the identifier at `at` stands in, by the node the tree puts it in.
fn context(s: &Snapshot, at: nazm_span::Span) -> Option<Context> {
    let node = s.cst.root().covering(at)?;
    match node.kind() {
        SyntaxKind::NameRef => Some(Context::Value),
        SyntaxKind::CallExpr => Some(Context::Call),
        SyntaxKind::Type => Some(Context::Type),
        // `x.f = v`: the root binding is a value; the names after `.` are fields.
        SyntaxKind::Place => {
            let first = node
                .child_leaves()
                .into_iter()
                .find(|l| l.kind == SyntaxKind::Ident)?;
            (first.span == at).then_some(Context::Value)
        }
        _ => None,
    }
}

/// Where a program's own candidate is declared, read from the reference index.
fn declaration(s: &Snapshot, c: Candidate) -> Option<Location> {
    let Candidate::Entity(t) = c else { return None };
    match s.target(s.index.declaration(t)?)? {
        Target::File(l) => Some(l),
        Target::Prelude { .. } => None,
    }
}

fn kind(c: Candidate) -> &'static str {
    match c {
        Candidate::Entity(ReferenceTarget::Local(_)) => "local",
        Candidate::Entity(ReferenceTarget::Function(_)) => "function",
        Candidate::Entity(ReferenceTarget::Record(_)) => "record",
        Candidate::Entity(ReferenceTarget::Enum(_)) => "enum",
        Candidate::Entity(t) => t.kind(),
        Candidate::Intrinsic(_) => "built-in function",
        Candidate::BuiltinType(_) | Candidate::BuiltinVec => "built-in type",
        Candidate::TypeParameter(_) => "type parameter",
    }
}

/// What the resolution already says about a candidate, rendered as hover renders it.
fn detail(s: &Snapshot, c: Candidate) -> Option<String> {
    let res = &s.analysis.resolution;
    let types = res.types();
    match c {
        Candidate::Entity(ReferenceTarget::Local(l)) => {
            let def = res.function(l.function)?.locals.get(l.slot as usize)?;
            Some(format!(
                "{}{}: {}",
                if def.mutable { "mut " } else { "" },
                def.name,
                types.show(def.ty)
            ))
        }
        Candidate::Entity(ReferenceTarget::Function(id)) => Some(
            crate::Signature::of_function(res, id)?
                .render(res, None)
                .label,
        ),
        _ => None,
    }
}
