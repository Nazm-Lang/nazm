//! The call at a position, and signature help from it. N20.
//!
//! # The contract
//!
//! **Signature help reports the call the checker already checked.** It resolves no callee
//! and infers no type argument: the callee, the signature its arguments were checked
//! against and what a generic call settled are read from the [`CheckedCall`] the checker
//! recorded under the callee's name ([`nazm_core::Resolution::checked_call`]). The concrete
//! tree decides only *where* the cursor is:
//!
//! - **which call**: the innermost call or `spawn` whose argument list contains the cursor
//!   — strictly after its `(` and no later than its `)` — by the tree's nesting, never by
//!   the nearest name;
//! - **which argument**: how many of *that* argument list's own `,` separators end at or
//!   before the cursor. A comma inside a nested call, a construction, a type argument
//!   list, a parenthesised expression, a string or a comment belongs to another node or
//!   is not a separator at all, so it is never counted.
//!
//! A position in no argument list — on the callee, before the `(`, after the `)` — is in
//! no call. A call the checker did not record — its callee did not resolve, or it is a
//! record or variant construction, which is not a call — has no signature help. Neither is
//! guessed.
//!
//! # What is not claimed
//!
//! One document at its current version, from that document's own compilation; and only
//! where every file of that compilation parsed without recovery, because a lost
//! declaration could have made the same call resolve differently. There is no answer from
//! an earlier, clean version.

use nazm_core::__syntax::{CstNode, SyntaxKind};
use nazm_core::sema::{Callee, CheckedCall, FnId, Instantiation, ParamOwner, TypeParam};
use nazm_core::{Resolution, Type};
use nazm_span::Span;

use crate::service::{LanguageService, Snapshot};

/// A callable's signature, as the compiler knows it: structured, so that no consumer has to
/// read a formatted string back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub callee: Callee,
    pub name: String,
    pub type_params: Vec<TypeParameter>,
    pub params: Vec<Parameter>,
    pub ret: Type,
    /// The declared effect set — a function's `! { … }`, or a built-in's own effect — where
    /// there is one to show (N36). `None` for a function that declares none and for a
    /// built-in with no effect: the signature is what is declared, and an inferred set is
    /// not a declaration.
    pub effects: Option<nazm_core::sema::EffectSet>,
}

/// One declared type parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParameter {
    pub name: String,
    /// Whether its declaration requires `Equality` (N14).
    pub requires_equality: bool,
}

/// One declared parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// The name its declaration gives it. `None` for a built-in's, which the compiler's
    /// inventory types but does not name.
    pub name: Option<String>,
    pub ty: Type,
}

/// A signature as text, with where each parameter is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub label: String,
    /// Each parameter's byte range in [`Self::label`], in order.
    pub params: Vec<(u32, u32)>,
}

impl Signature {
    /// The declared signature of a function of this compilation.
    #[must_use]
    pub fn of_function(res: &Resolution, id: FnId) -> Option<Self> {
        let f = res.function(id)?;
        Some(Self {
            callee: Callee::Function(id),
            name: f.name.clone(),
            type_params: type_parameters(res, ParamOwner::Function(id), f.type_params.len()),
            params: named(res, Callee::Function(id), &f.params),
            ret: f.ret,
            effects: f.declared_effects,
        })
    }

    /// The signature a call's arguments were checked against: the callee's declared names,
    /// with the parameter types the checker read from the module's environment.
    #[must_use]
    pub fn of_call(res: &Resolution, call: &CheckedCall) -> Option<Self> {
        let (name, type_params, effects) = match call.callee {
            Callee::Function(id) => {
                let f = res.function(id)?;
                let n = f.type_params.len();
                (
                    f.name.clone(),
                    type_parameters(res, ParamOwner::Function(id), n),
                    f.declared_effects,
                )
            }
            Callee::Builtin(i) => (
                i.name().to_owned(),
                type_parameters(res, ParamOwner::Builtin(i), i.generics() as usize),
                nazm_core::sema::Effect::of_intrinsic(i).map(nazm_core::sema::EffectSet::of),
            ),
        };
        Some(Self {
            callee: call.callee,
            name,
            type_params,
            params: named(res, call.callee, &call.params),
            ret: call.ret,
            effects,
        })
    }

    /// `fn name[T: Equality](a: T) -> T ! { io }`: the one spelling of a signature, which
    /// hover, completion and signature help all show — the effect set as the formatter
    /// writes it, when there is a declared one.
    ///
    /// With `instance`, each type parameter, parameter and the result is shown as that call
    /// settled it — `fn identity[Int](x: Int) -> Int` — and whatever it did not settle as
    /// declared.
    #[must_use]
    pub fn render(&self, res: &Resolution, instance: Option<&Instantiation>) -> Rendered {
        let types = res.types();
        let mut label = format!("fn {}", self.name);
        if !self.type_params.is_empty() {
            let shown: Vec<String> = self
                .type_params
                .iter()
                .enumerate()
                .map(
                    |(i, p)| match instance.and_then(|x| x.args.get(i).copied().flatten()) {
                        Some(t) => types.show(t),
                        None if p.requires_equality => {
                            format!("{}: {}", p.name, nazm_core::check::EQUALITY)
                        }
                        None => p.name.clone(),
                    },
                )
                .collect();
            label.push('[');
            label.push_str(&shown.join(", "));
            label.push(']');
        }
        let params = push_list(
            &mut label,
            res,
            self.params.iter().enumerate().map(|(i, p)| {
                let ty = instance
                    .and_then(|x| x.params.get(i).copied().flatten())
                    .unwrap_or(p.ty);
                (p.name.as_deref(), ty)
            }),
        );
        let ret = instance.and_then(|x| x.ret).unwrap_or(self.ret);
        label.push_str(" -> ");
        label.push_str(&types.show(ret));
        if let Some(effects) = self.effects {
            label.push_str(" ! ");
            label.push_str(&effects.to_string());
        }
        Rendered { label, params }
    }
}

/// `(a: Int, b: Str)` onto `label`, returning where each entry is in it: the one spelling
/// of a parenthesised list of named, typed entries, shared by a function's parameters and
/// a construction's fields (N21) — which are different things, rendered alike.
pub(crate) fn push_list<'n>(
    label: &mut String,
    res: &Resolution,
    entries: impl Iterator<Item = (Option<&'n str>, Type)>,
) -> Vec<(u32, u32)> {
    let types = res.types();
    label.push('(');
    let mut at = Vec::new();
    for (i, (name, ty)) in entries.enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = offset(label);
        if let Some(name) = name {
            label.push_str(name);
            label.push_str(": ");
        }
        label.push_str(&types.show(ty));
        at.push((start, offset(label)));
    }
    label.push(')');
    at
}

fn offset(s: &str) -> u32 {
    u32::try_from(s.len()).unwrap_or(u32::MAX)
}

fn type_parameters(res: &Resolution, owner: ParamOwner, n: usize) -> Vec<TypeParameter> {
    (0..n)
        .map(|i| {
            let p = TypeParam {
                owner,
                index: u32::try_from(i).unwrap_or(u32::MAX),
            };
            TypeParameter {
                name: res.types().param_name(p),
                requires_equality: res.types().requires_equality(p),
            }
        })
        .collect()
}

/// Parameter types with their declared names: a function's parameters are its first
/// slots, in declaration order, so the names are the ones the checker bound. A built-in's
/// parameters have none.
fn named(res: &Resolution, callee: Callee, params: &[Type]) -> Vec<Parameter> {
    let locals = match callee {
        Callee::Function(id) => res.function(id).map(|f| f.locals.as_slice()),
        Callee::Builtin(_) => None,
    };
    params
        .iter()
        .enumerate()
        .map(|(i, ty)| Parameter {
            name: locals.and_then(|l| l.get(i)).map(|l| l.name.clone()),
            ty: *ty,
        })
        .collect()
}

/// The call a position is in: which call the checker checked, with which signature, and
/// which of its arguments the position is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInfo {
    pub callee: Callee,
    /// The callee's name, as written.
    pub callee_start: u32,
    pub callee_end: u32,
    /// The whole call.
    pub start: u32,
    pub end: u32,
    /// Each argument that parsed, as the checker checked it.
    pub arguments: Vec<(u32, u32)>,
    /// Which argument the position is in, counting from zero, by the argument list's own
    /// separators — whether or not the callee has a parameter there.
    pub active_argument: u32,
    /// The signature the arguments were checked against, as declared.
    pub signature: Signature,
    /// What a generic call settled; `None` for a callee that is not generic.
    pub instance: Option<Instantiation>,
    pub spawned: bool,
}

impl CallInfo {
    /// The parameter the position is at: [`Self::active_argument`], when the callee has a
    /// parameter there. `None` for a callee with no parameters, and for an argument past
    /// the last one.
    #[must_use]
    pub fn active_parameter(&self) -> Option<u32> {
        (usize::try_from(self.active_argument).ok()? < self.signature.params.len())
            .then_some(self.active_argument)
    }
}

/// Signature help: the call, its signature as text, and the active parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    pub call: CallInfo,
    /// The signature as this call instantiated it, where it is generic.
    pub rendered: Rendered,
    pub active_parameter: Option<u32>,
}

/// Why there is no call at a position. There is never a guessed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCall {
    /// The document is not open at that version.
    NotCurrent,
    /// The position is in no call's argument list.
    NotInACall,
    /// The position is in an argument list the checker recorded no checked call for: its
    /// callee did not resolve.
    NotChecked,
    /// A file of the compilation is a syntax recovery, so a lost declaration could have
    /// changed what the call resolves to.
    RecoveredSyntax,
}

impl LanguageService {
    /// The call whose argument list contains byte `offset` of `path`, at `version`, as the
    /// checker checked it.
    ///
    /// # Errors
    ///
    /// The [`NoCall`] that applies.
    pub fn call_at(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<CallInfo, NoCall> {
        let s = self.current(path, version).ok_or(NoCall::NotCurrent)?;
        call_in(s, offset)
    }

    /// Signature help at byte `offset` of `path`, at `version`.
    ///
    /// # Errors
    ///
    /// The [`NoCall`] that applies.
    pub fn signature_help(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<SignatureHelp, NoCall> {
        let s = self.current(path, version).ok_or(NoCall::NotCurrent)?;
        let call = call_in(s, offset)?;
        let rendered = call
            .signature
            .render(&s.analysis.resolution, call.instance.as_ref());
        Ok(SignatureHelp {
            active_parameter: call.active_parameter(),
            rendered,
            call,
        })
    }
}

/// What signature help shows at a position: a call's signature, or a construction's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Help {
    Call(SignatureHelp),
    Constructor(crate::structure::ConstructorHelp),
}

impl LanguageService {
    /// Signature help for the innermost call or construction whose parentheses hold byte
    /// `offset` of `path`, at `version` (N21): a function call's, by [`Self::signature_help`],
    /// or a record or variant construction's, by [`Self::constructor_help`]. The two are
    /// found by the same rule — strictly after the `(`, no later than the `)` — so the
    /// innermost is the one whose parentheses open later; a call inside an initialiser
    /// is answered as a call, and a construction inside an argument as a construction.
    ///
    /// # Errors
    ///
    /// The [`NoCall`] of whichever is innermost; never the outer one's answer instead.
    pub fn help_at(
        &self,
        path: &std::path::Path,
        version: i32,
        offset: u32,
    ) -> Result<Help, NoCall> {
        let s = self.current(path, version).ok_or(NoCall::NotCurrent)?;
        let root = s.cst.root();
        let call = innermost(root, offset).map(|r| r.callee.start);
        match (call, crate::structure::construction_at(root, offset)) {
            (Some(c), Some((node, open))) if open > c => {
                crate::structure::constructor_help(s, node, offset).map(Help::Constructor)
            }
            (Some(_), _) => self.signature_help(path, version, offset).map(Help::Call),
            (None, Some((node, _))) => {
                crate::structure::constructor_help(s, node, offset).map(Help::Constructor)
            }
            (None, None) => Err(NoCall::NotInACall),
        }
    }
}

fn call_in(s: &Snapshot, offset: u32) -> Result<CallInfo, NoCall> {
    if !s.analysis.syntax_errors.is_empty() {
        return Err(NoCall::RecoveredSyntax);
    }
    let region = innermost(s.cst.root(), offset).ok_or(NoCall::NotInACall)?;
    let res = &s.analysis.resolution;
    let checked = res.checked_call(region.callee).ok_or(NoCall::NotChecked)?;
    let signature = Signature::of_call(res, checked).ok_or(NoCall::NotChecked)?;
    Ok(CallInfo {
        callee: checked.callee,
        callee_start: region.callee.start,
        callee_end: region.callee.end,
        start: checked.span.start,
        end: checked.span.end,
        arguments: checked.args.iter().map(|a| (a.start, a.end)).collect(),
        active_argument: region.active,
        signature,
        instance: checked.instance.clone(),
        spawned: checked.spawned,
    })
}

/// A call's argument list, as the tree has it.
struct Region {
    callee: Span,
    active: u32,
}

/// The innermost call or `spawn` whose argument list holds `offset`, and which of its
/// arguments `offset` is in.
///
/// Down the one path of nodes that contain `offset`: a node whose argument list holds it
/// starts before it and ends after it, so the node to descend into at each level is the
/// only child with `start <= offset < end` — children do not overlap. The last call met on
/// the way down is the innermost.
fn innermost(root: CstNode<'_>, offset: u32) -> Option<Region> {
    let mut best = None;
    let mut at = Some(root);
    while let Some(node) = at {
        if matches!(node.kind(), SyntaxKind::CallExpr | SyntaxKind::SpawnStmt) {
            if let Some(r) = region(node, offset) {
                best = Some(r);
            }
        }
        at = node.children().into_iter().find(|c| {
            let span = c.span();
            span.start <= offset && offset < span.end
        });
    }
    best
}

/// The argument list of the call `node`, if it holds `offset`: strictly after its `(` and
/// no later than its `)`.
fn region(node: CstNode<'_>, offset: u32) -> Option<Region> {
    let list = node
        .children()
        .into_iter()
        .find(|c| c.kind() == SyntaxKind::ArgList)?;
    let own = list.child_leaves();
    let open = own.iter().find(|l| l.kind == SyntaxKind::LParen)?;
    let close = own.iter().rev().find(|l| l.kind == SyntaxKind::RParen)?;
    if !(open.span.end <= offset && offset <= close.span.start) {
        return None;
    }
    let callee = node
        .child_leaves()
        .into_iter()
        .find(|l| l.kind == SyntaxKind::Ident)?;
    let separators = own
        .iter()
        .filter(|l| l.kind == SyntaxKind::Comma && l.span.end <= offset)
        .count();
    Some(Region {
        callee: callee.span,
        active: u32::try_from(separators).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    /// The structural half of "reports the call the checker checked": this module reads a
    /// call's callee, signature and instantiation from the checker's record, and computes
    /// none of them. It looks no name up, searches no definitions, and substitutes or
    /// unifies no type — the checker did each once, and `checked_call` is its answer. (The
    /// whole-program definition search is the `one resolver` gate's to forbid.)
    #[test]
    fn signature_help_resolves_and_infers_nothing() {
        let source = include_str!("signature.rs");
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
            "by_name(",
            "definition(",
            "visible_at(",
            ".callee(",
            "substitute",
            "unify",
            "mentions(",
            ".text ==",
            ".name ==",
            "HashMap<String",
            "BTreeMap<String",
            "hover(",
            "completion(",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in signature help: it would be deciding what a call means"
            );
        }
    }
}
