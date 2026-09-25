//! What names are available at a position: the checker's scopes, written down. N19.
//!
//! # What this is
//!
//! The checker decides what a name means by looking it up — in the innermost of its
//! lexical frames that has it, then in the module's environment. Until N19 those frames
//! existed only while a body was being checked, and the environment only while its module
//! was. This records them as the checker builds them, so a tool can ask the other
//! question: **at this byte, which names could be written, and what would each mean?**
//!
//! - **Frames** — one per scope the checker pushes: a function's parameters, every block
//!   (a body, a branch, a loop, `scope { … }`, a block expression) and every match arm —
//!   each with its source range, its parent, and the bindings the checker defined in it.
//! - **Bindings** — each with the byte from which it is visible, which is where the
//!   checker introduced it (after a `let`'s whole statement, so not in its own
//!   initializer; after a parameter's or a pattern field's name), and the binding it
//!   shadows, **which the checker looked up** as it defined it.
//! - **Type scopes** — the type parameters of a function, record or enum, over its
//!   declaration's range, each with the type name it hides, looked up the same way.
//! - **Module environments** — the functions (built-ins, the module's own, its direct
//!   imports' exports) and the types (its own, its direct imports' nameable ones, the
//!   built-ins) the module may name, minus every name the checker refused as an ambiguity
//!   or a collision.
//!
//! # What this is not
//!
//! **A resolver.** Nothing here decides what an occurrence means — that is
//! [`crate::Resolution`]'s, and it is not consulted. Nothing here compares two names to
//! decide which one wins: shadowing is the checker's recorded answer, not a string
//! search, and a name refused as ambiguous is left out rather than ranked.

use nazm_span::{FileId, Span};

use crate::intrinsic::Intrinsic;
use crate::module::ModuleId;
use crate::references::ReferenceTarget;
use crate::resolve::{FnId, LocalRef};
use crate::types::{Type, TypeParam};

/// Something a name at a position could mean.
///
/// Wider than [`ReferenceTarget`], which is what a program declares and a rename can
/// change: a built-in and a type parameter can be written, and are neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Candidate {
    /// A local, a function, a record or an enum the program declares.
    Entity(ReferenceTarget),
    Intrinsic(Intrinsic),
    /// `Int`, `Str` and the rest: [`Type::ALL`].
    BuiltinType(Type),
    /// `Vec`, the one built-in type written with an argument.
    BuiltinVec,
    TypeParameter(TypeParam),
}

/// A candidate and the name it is written with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub candidate: Candidate,
}

/// One binding, as the checker defined it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    local: LocalRef,
    name: String,
    /// The first byte at which the name means this binding.
    visible_from: u32,
    /// The binding that the checker's own lookup found under this name when it defined
    /// this one — the one this hides from here on.
    shadows: Option<LocalRef>,
    /// Defined where the name was already bound in the same frame: a refused duplicate.
    /// Neither it nor the one it collides with is a candidate.
    duplicate: bool,
}

/// One scope the checker pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    span: Span,
    parent: Option<u32>,
    /// The function whose body this is in. `None` while checking something that is not a
    /// declared function — a duplicate, a refused name — whose bindings have no identity.
    function: Option<FnId>,
    /// False for a function's frame when its body did not parse: no bindings of it are
    /// known, so no scope inside it is either.
    complete: bool,
    bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeScope {
    span: Span,
    /// Each parameter, and the type name the checker found it hides, if any.
    params: Vec<(Named, Option<Candidate>)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ModuleNames {
    callables: Vec<Named>,
    types: Vec<Named>,
}

/// Every scope fact one compilation's checker recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeTrace {
    frames: Vec<Frame>,
    /// The frames pushed and not yet popped, innermost last, while checking.
    open: Vec<u32>,
    type_scopes: Vec<TypeScope>,
    /// By [`ModuleId`] index.
    modules: Vec<Option<ModuleNames>>,
}

/// Why a position has no complete scope. A list that might be missing names is never
/// offered as though it were complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The position's module was not checked.
    ModuleNotChecked,
    /// The position is in a function whose body did not parse.
    BodyDidNotParse,
    /// The position is in a function the checker did not declare (a duplicate).
    NotADeclaredFunction,
}

/// What may be written at a position, by namespace, each sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visible {
    /// Locals: parameters, `let`s and pattern bindings, the innermost of each name.
    pub values: Vec<Named>,
    /// Functions and built-ins.
    pub callables: Vec<Named>,
    /// Type parameters, records, enums and built-in types.
    pub types: Vec<Named>,
}

impl ScopeTrace {
    // ---- recording, for the checker ----

    /// A scope the checker has just pushed, over `span`.
    pub fn open(&mut self, span: Span, function: Option<FnId>) {
        let id = u32::try_from(self.frames.len()).unwrap_or(u32::MAX);
        self.frames.push(Frame {
            span,
            parent: self.open.last().copied(),
            function,
            complete: true,
            bindings: Vec::new(),
        });
        self.open.push(id);
    }

    /// The scope the checker has just popped.
    pub fn close(&mut self) {
        self.open.pop();
    }

    /// The innermost open scope is a function whose body did not parse.
    pub fn incomplete(&mut self) {
        if let Some(f) = self
            .open
            .last()
            .and_then(|i| self.frames.get_mut(*i as usize))
        {
            f.complete = false;
        }
    }

    /// A binding the checker defined in the innermost open scope.
    pub fn bind(
        &mut self,
        local: LocalRef,
        name: &str,
        visible_from: u32,
        shadows: Option<LocalRef>,
        duplicate: bool,
    ) {
        if let Some(f) = self
            .open
            .last()
            .and_then(|i| self.frames.get_mut(*i as usize))
        {
            f.bindings.push(Binding {
                local,
                name: name.to_owned(),
                visible_from,
                shadows,
                duplicate,
            });
        }
    }

    /// A definition's type parameters, over its declaration, each with the type name the
    /// checker found it hides.
    pub fn type_scope(&mut self, span: Span, params: Vec<(String, TypeParam, Option<Candidate>)>) {
        if params.is_empty() {
            return;
        }
        self.type_scopes.push(TypeScope {
            span,
            params: params
                .into_iter()
                .map(|(name, p, hides)| {
                    (
                        Named {
                            name,
                            candidate: Candidate::TypeParameter(p),
                        },
                        hides,
                    )
                })
                .collect(),
        });
    }

    /// What `module` may name, as the checker built it: every name mapped to what it
    /// means there, and the names refused as ambiguous or colliding already left out.
    pub fn module(&mut self, module: ModuleId, callables: Vec<Named>, types: Vec<Named>) {
        let m = module.index();
        if self.modules.len() <= m {
            self.modules.resize_with(m + 1, || None);
        }
        let sorted = |mut v: Vec<Named>| {
            v.sort_by(|a, b| (&a.name, a.candidate).cmp(&(&b.name, b.candidate)));
            v
        };
        if let Some(slot) = self.modules.get_mut(m) {
            *slot = Some(ModuleNames {
                callables: sorted(callables),
                types: sorted(types),
            });
        }
    }

    // ---- the query ----

    /// Every name that may be written at byte `offset` of `file`, in `module`, and what
    /// each would mean there.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when the checker's account of the position is not complete.
    pub fn visible_at(
        &self,
        module: ModuleId,
        file: FileId,
        offset: u32,
    ) -> Result<Visible, Unavailable> {
        let names = self
            .modules
            .get(module.index())
            .and_then(Option::as_ref)
            .ok_or(Unavailable::ModuleNotChecked)?;
        let inside = |s: Span| s.file == file && s.start <= offset && offset < s.end;

        // The innermost frame containing the position, and its chain outwards.
        let innermost = self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, f)| inside(f.span))
            .max_by_key(|(i, f)| (f.span.start, std::cmp::Reverse(f.span.end), *i))
            .map(|(i, _)| i);
        let mut chain: Vec<&Frame> = Vec::new();
        let mut at = innermost.and_then(|i| u32::try_from(i).ok());
        while let Some(i) = at {
            let Some(f) = self.frames.get(i as usize) else {
                break;
            };
            chain.push(f);
            at = f.parent;
        }
        if let Some(function) = chain.last() {
            if function.function.is_none() {
                return Err(Unavailable::NotADeclaredFunction);
            }
            if !function.complete {
                return Err(Unavailable::BodyDidNotParse);
            }
        }

        // Every binding visible here: in a frame of the chain, and introduced before it.
        let visible: Vec<&Binding> = chain
            .iter()
            .flat_map(|f| f.bindings.iter())
            .filter(|b| b.visible_from <= offset)
            .collect();
        // What those hide — the checker's recorded answer, never a comparison of names —
        // and the refused duplicates, which name nothing unambiguously.
        let mut hidden: Vec<LocalRef> = Vec::new();
        for b in &visible {
            hidden.extend(b.shadows);
            if b.duplicate {
                hidden.push(b.local);
            }
        }
        let mut values: Vec<Named> = visible
            .iter()
            .filter(|b| !hidden.contains(&b.local))
            .map(|b| Named {
                name: b.name.clone(),
                candidate: Candidate::Entity(ReferenceTarget::Local(b.local)),
            })
            .collect();
        values.sort_by(|a, b| (&a.name, a.candidate).cmp(&(&b.name, b.candidate)));

        // Type parameters in scope here, and the module's types less those they hide.
        let params: Vec<&(Named, Option<Candidate>)> = self
            .type_scopes
            .iter()
            .filter(|t| inside(t.span))
            .flat_map(|t| t.params.iter())
            .collect();
        let hidden_types: Vec<Candidate> = params.iter().filter_map(|(_, h)| *h).collect();
        let mut types: Vec<Named> = params.iter().map(|(n, _)| n.clone()).collect();
        types.extend(
            names
                .types
                .iter()
                .filter(|n| !hidden_types.contains(&n.candidate))
                .cloned(),
        );
        types.sort_by(|a, b| (&a.name, a.candidate).cmp(&(&b.name, b.candidate)));

        Ok(Visible {
            values,
            callables: names.callables.clone(),
            types,
        })
    }

    /// How many frames and bindings are recorded: a function of the program, for tests
    /// that hold retained state constant.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (
            self.frames.len(),
            self.frames.iter().map(|f| f.bindings.len()).sum(),
        )
    }
}

/// A record or enum as a candidate.
#[must_use]
pub fn defined(node: crate::udt::TypeNode) -> Candidate {
    match node {
        crate::udt::TypeNode::Record(id) => Candidate::Entity(ReferenceTarget::Record(id)),
        crate::udt::TypeNode::Enum(id) => Candidate::Entity(ReferenceTarget::Enum(id)),
    }
}

#[cfg(test)]
mod tests {
    /// The structural half of "not a resolver": what is visible where is read from the
    /// checker's recorded facts — positions, parents, the binding each one hides — and never
    /// decided here by comparing a name with another, or by looking one up.
    #[test]
    fn the_trace_never_compares_or_looks_up_a_name() {
        let source = include_str!("scope.rs");
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
            "== name",
            ".name !=",
            "lookup(",
            ".definition(",
            ".declared(",
            "HashMap<String",
            "BTreeMap<String",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in the scope trace: it would be resolving names itself"
            );
        }
    }
}
