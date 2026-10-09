//! Semantic identifiers: what each identifier of a document is, as the checker decided it —
//! its kind, whether it declares or refers, and which entity it is. N23.
//!
//! # The contract
//!
//! **An identifier is classified by what the checker recorded at its exact span, and by
//! nothing else.** The concrete tree lists the document's identifier tokens; for each, the
//! answer is read from the compiler's facts about that span, in this order:
//!
//! | Fact at the span | Class | Role |
//! | --- | --- | --- |
//! | a [`ReferenceTarget`] in the reference index (N17) | its kind — a local is a parameter when its slot is one of its function's parameters, which the checker allocates first | declaration when the index records the entity declared at this span, else reference |
//! | a call to a built-in (`Callee::Builtin`) | intrinsic | reference |
//! | a written built-in type name ([`WrittenType::Builtin`]) | built-in type | reference |
//! | a written type parameter ([`WrittenType::Param`], [`WrittenType::ParamDeclaration`]) | type parameter | reference, or declaration in its `[…]` |
//! | nothing | no identifier | — |
//!
//! An identifier the checker resolved to nothing — an unknown name, a name in a body that
//! did not parse, a keyword, a word in a comment or a string — has no fact and is not
//! classified: where it sits, and how it is spelled, decide nothing. So a call head that did
//! not resolve is not a function, and a name after `.` that did not resolve is not a field.
//!
//! This is not syntax highlighting. Keywords, comments, strings, numbers, operators and
//! punctuation are an editor grammar's; this layer adds what only the checker knows.
//!
//! # What is kept
//!
//! Nothing. The view is derived when asked, from the document's current analysis, and
//! dropped. One identifier token is one span, so no span is classified twice, and the
//! identifiers come in source order.

use std::path::Path;
use std::sync::Arc;

use nazm_core::__syntax::SyntaxKind;
use nazm_core::sema::{Callee, Intrinsic, ReferenceTarget, TypeParam, WrittenType};
use nazm_core::Type;
use nazm_span::Span;

use crate::service::{LanguageService, Snapshot};

/// What kind of name an identifier is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticClass {
    Function,
    /// A built-in function: compiler-owned, with no declaration in any source.
    Intrinsic,
    Parameter,
    /// A `let` or a pattern binding.
    Variable,
    Record,
    Enum,
    Variant,
    Field,
    PayloadField,
    TypeParameter,
    /// A built-in type: compiler-owned, with no declaration in any source.
    BuiltinType,
    /// A trait (N78).
    Trait,
    /// A trait's method, at its declaration or a call through a bound (N78).
    TraitMethod,
    /// A module's constant (Gate 2).
    Constant,
}

/// Whether an identifier declares what it names or refers to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Declaration,
    Reference,
}

/// Which thing an identifier names, by the compiler's own identity for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticTarget {
    /// Something a program declared: the identity references and rename use.
    Entity(ReferenceTarget),
    Intrinsic(Intrinsic),
    /// A built-in type, as the type the checker resolved it to.
    BuiltinType(Type),
    TypeParameter(TypeParam),
}

/// One classified identifier of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticIdentifier {
    /// The identifier, in the document's text.
    pub start: u32,
    pub end: u32,
    pub class: SemanticClass,
    pub role: Role,
    pub target: SemanticTarget,
}

/// A document's classified identifiers, and the text their offsets are into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIdentifiers {
    pub text: Arc<str>,
    /// In source order, no two over one span, none overlapping.
    pub identifiers: Vec<SemanticIdentifier>,
}

/// Why there are no semantic identifiers. There are never historical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoIdentifiers {
    /// The document is not open at that version.
    NotCurrent,
}

impl LanguageService {
    /// Every identifier of `path`, at `version`, that the checker resolved, classified.
    ///
    /// # Errors
    ///
    /// [`NoIdentifiers::NotCurrent`] unless the document is open at `version` and analysed
    /// at the current generation.
    pub fn semantic_identifiers(
        &self,
        path: &Path,
        version: i32,
    ) -> Result<SemanticIdentifiers, NoIdentifiers> {
        let s = self
            .current(path, version)
            .ok_or(NoIdentifiers::NotCurrent)?;
        let text = s.texts.first().cloned().unwrap_or_else(|| Arc::from(""));
        let identifiers = s
            .cst
            .leaves()
            .into_iter()
            .filter(|l| l.kind == SyntaxKind::Ident)
            .filter_map(|l| classify(s, l.span))
            .collect();
        Ok(SemanticIdentifiers { text, identifiers })
    }
}

/// What the checker recorded at exactly `at`, as a classified identifier.
fn classify(s: &Snapshot, at: Span) -> Option<SemanticIdentifier> {
    let res = &s.analysis.resolution;
    let (class, role, target) = if let Some(entity) = s.index.target(at) {
        let role = if s.index.declaration(entity) == Some(at) {
            Role::Declaration
        } else {
            Role::Reference
        };
        let class = match entity {
            ReferenceTarget::Function(_) => SemanticClass::Function,
            ReferenceTarget::Local(l) => {
                let parameters = res.function(l.function).map_or(0, |f| f.params.len());
                if (l.slot as usize) < parameters {
                    SemanticClass::Parameter
                } else {
                    SemanticClass::Variable
                }
            }
            ReferenceTarget::Record(_) => SemanticClass::Record,
            ReferenceTarget::Enum(_) => SemanticClass::Enum,
            ReferenceTarget::Variant(_) => SemanticClass::Variant,
            ReferenceTarget::Field(_) => SemanticClass::Field,
            ReferenceTarget::PayloadField(_) => SemanticClass::PayloadField,
            ReferenceTarget::Trait(_) => SemanticClass::Trait,
            ReferenceTarget::TraitMethod(..) => SemanticClass::TraitMethod,
            ReferenceTarget::Const(_) => SemanticClass::Constant,
        };
        (class, role, SemanticTarget::Entity(entity))
    } else if let Some(Callee::Builtin(i)) = res.callee(at) {
        (
            SemanticClass::Intrinsic,
            Role::Reference,
            SemanticTarget::Intrinsic(i),
        )
    } else {
        match res.written_type(at)? {
            WrittenType::Builtin(t) => (
                SemanticClass::BuiltinType,
                Role::Reference,
                SemanticTarget::BuiltinType(t),
            ),
            WrittenType::Param(p) => (
                SemanticClass::TypeParameter,
                Role::Reference,
                SemanticTarget::TypeParameter(p),
            ),
            WrittenType::ParamDeclaration(p) => (
                SemanticClass::TypeParameter,
                Role::Declaration,
                SemanticTarget::TypeParameter(p),
            ),
        }
    };
    Some(SemanticIdentifier {
        start: at.start,
        end: at.end,
        class,
        role,
        target,
    })
}

#[cfg(test)]
mod tests {
    /// The structural half of "by what the checker recorded, and nothing else": this module
    /// reads facts by span and never compares a spelling, looks a name up, or keeps a list of
    /// built-in names.
    #[test]
    fn identifiers_are_never_classified_by_spelling() {
        let source = include_str!("semantic.rs");
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
            ".text(",
            "text ==",
            "from_name(",
            "by_name(",
            ".definition(",
            ".declared(",
            ".name ==",
            "== \"",
            "is_uppercase",
            "starts_with(",
            "SyntaxKind::Dot",
            "SyntaxKind::CallExpr",
            "HashMap<String",
            "BTreeMap<String",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in semantic: it would be classifying by spelling or position"
            );
        }
    }
}
