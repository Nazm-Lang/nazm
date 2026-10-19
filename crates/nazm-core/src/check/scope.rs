//! Scopes and variables: slots, definitions, lookups, and naming a function as a value.

use super::generic::polymorphic;
use super::*;

impl Checker<'_> {
    /// Give a binding its slot in the current function's storage.
    ///
    /// **Parameters first, in order, then body bindings as they are declared, and never
    /// reused** — including for a name that shadows another. That ordering is a contract
    /// the backend relies on to store incoming arguments without a separate notion of what
    /// a parameter is, and it lives here now because this is where bindings are made.
    pub(super) fn allocate(&mut self, name: &str, span: Span, ty: Type, mutable: bool) -> u32 {
        let Some(id) = self.current else {
            return 0;
        };
        self.res.record_mention(id, ty);
        self.res
            .bind(
                id,
                LocalDef {
                    name: name.to_owned(),
                    ty,
                    mutable,
                    span,
                },
            )
            .unwrap_or(0)
    }

    /// Push a lexical scope over `span`, and record it (N19).
    pub(super) fn open_scope(&mut self, span: Span) {
        self.scopes.push(false);
        self.res.scopes_mut().open(span, self.current);
    }

    /// Push a lexical scope over `span` that continues the one outside it as far as shadowing
    /// goes — a pattern `let`'s arm and the rest of its block ([`Scopes::joined`]).
    pub(super) fn open_joined_scope(&mut self, span: Span) {
        self.scopes.push(true);
        self.res.scopes_mut().open(span, self.current);
    }

    /// Pop the innermost lexical scope, and record that it ended.
    pub(super) fn close_scope(&mut self) {
        self.scopes.pop();
        self.res.scopes_mut().close();
    }

    /// Bind a name in the innermost scope, visible from byte `visible_from` — and record,
    /// from this checker's own lookup, which binding it hides and whether it collides
    /// with one in the same scope, so that what is visible where can be read back without
    /// comparing a single name.
    pub(super) fn define(
        &mut self,
        name: &str,
        ty: Type,
        mutable: bool,
        declared_at: Option<Span>,
        slot: u32,
        visible_from: u32,
    ) {
        if let Some(function) = self.current {
            let shadows = self.scopes.lookup(name).map(|b| LocalRef {
                function,
                slot: b.slot,
            });
            let duplicate = self.scopes.defined_here(name);
            self.res.scopes_mut().bind(
                LocalRef { function, slot },
                name,
                visible_from,
                shadows,
                duplicate,
            );
        }
        self.scopes.define(name, ty, mutable, declared_at, slot);
    }

    /// Record that the variable written at `span` is the binding in `slot` of the function
    /// being checked.
    ///
    /// The owner is the function this body belongs to, which is known here and nowhere
    /// more cheaply. Outside a declared function there is no binding to name, and nothing
    /// is recorded — the same condition under which [`Self::allocate`] allocates nothing.
    pub(super) fn record_var(&mut self, span: Span, slot: u32) {
        if let Some(function) = self.current {
            self.res.record_var(span, LocalRef { function, slot });
        }
    }

    /// Record what a call resolved to, and answer the same question for the caller.
    ///
    /// **Built-ins are resolved before user functions.** A user definition of a built-in's
    /// name is refused at declaration, so the two sets are disjoint and the order states
    /// intent rather than breaking a tie. Written once here because it is a language rule;
    /// the evaluator and the backend used to each apply their own copy of it.
    /// Resolve a call, record what it resolved to, and hand back the signature.
    ///
    /// One lookup, in **this module's** environment. Built-ins are in it, this module's
    /// own definitions are in it, and the exports of what it imports are in it; the order
    /// they were put there in is where "built-ins first" is now expressed, once, rather
    /// than at each call site.
    ///
    /// `None` is a name this module may not write. Whether that is because it does not
    /// exist, because it is private where it lives, or because it lives in a module this
    /// one did not import, is [`Self::why_invisible`]'s question — and getting that answer
    /// right is most of what makes the visibility rule usable.
    /// The authority the bindings visible here hold — inside a closure too (N51): a closure
    /// holds the capabilities visible where it is written, as captured, and its type states
    /// what it does with them.
    pub(super) fn held(&self) -> nazm_sema::CapabilitySet {
        self.scopes.held_from(0)
    }

    /// The binding `name` reaches here, captured into every closure between it and here
    /// (N50): a binding of a frame below a closure's boundary is copied into that closure
    /// when the closure is made, so inside it the name means the closure's copy. A capability
    /// may be (N51). A `let mut` binding is shared instead (R1b, L10): it is kept in a cell the
    /// function and every closure between name, unless its type can hold a function value
    /// (`N0386`).
    /// The closures between frame `frame` and here, outermost first.
    fn closures_crossing(&self, frame: usize) -> Vec<usize> {
        self.closures
            .iter()
            .enumerate()
            .filter(|(_, c)| frame < c.boundary)
            .map(|(i, _)| i)
            .collect()
    }

    pub(super) fn lookup_var(&mut self, name: &str, at: Span) -> Option<Binding> {
        let (frame, b) = self.scopes.lookup_at(name)?;
        let crossing: Vec<usize> = self
            .closures
            .iter()
            .enumerate()
            .filter(|(_, c)| frame < c.boundary)
            .map(|(i, _)| i)
            .collect();
        if crossing.is_empty() {
            return Some(b);
        }
        // A capability may be captured since N51: the closure holds it, and its type says
        // what it does. A mutable binding is shared since R1b (L10): a closure's copy of it would
        // stop tracking it, so the function and its closures name one cell instead. A cell that
        // can hold a function value could come to hold the closure that holds it, so such a
        // binding is still refused, as a `Vec` of them is (`N0616`).
        let cell = b.mutable.then(|| self.res.types_mut().cell_of(b.ty));
        if let Some(cell) = cell {
            if self.res.types().function_holder(cell).is_some() {
                let ty = self.res.types().show(b.ty);
                self.errors.push(
                    Diagnostic::error(
                        codes::CAPTURE_REFUSED,
                        at,
                        format!(
                            "a closure cannot capture `{name}`, a `let mut` binding of `{ty}`, \
                             which can hold a function value"
                        ),
                    )
                    .help(
                        "a captured `let mut` binding is shared with the closure, so the closure \
                         could be stored into it and own itself, which reference counting never \
                         reclaims; bind a copy with `let`, or pass the value as an argument",
                    ),
                );
                return Some(Binding {
                    mutable: false,
                    declared_at: None,
                    ..b
                });
            }
            // The binding's own function: the one the outermost crossing closure is written in.
            let owner = crossing
                .first()
                .and_then(|i| self.closures.get(*i))
                .and_then(|c| self.res.function(c.id))
                .and_then(|d| d.closure.as_ref())
                .map(|info| info.enclosing);
            if let Some(owner) = owner {
                self.res.record_shared(owner, b.slot, cell);
            }
        }
        // Gate 1-C1: a closure's environment owns what it captures, so capturing a container
        // that can hold a function value is the one way it could come to own itself
        // (`docs/user/spec.md`, *Cycles*). Decided by the captured binding's type, once per capture.
        let first_capture = crossing
            .first()
            .and_then(|i| self.closures.get(*i))
            .is_some_and(|c| !c.captured.contains_key(&b.slot));
        if first_capture {
            if let Some(holder) = self.res.types().function_holder(b.ty) {
                let ty = self.res.types().show(b.ty);
                let holder = self.res.types().show(holder);
                let through = if ty == holder {
                    String::new()
                } else {
                    format!(" through `{holder}`")
                };
                self.errors.push(
                    Diagnostic::error(
                        codes::CAPTURE_CYCLE,
                        at,
                        format!(
                            "a closure cannot capture `{name}`: a `{ty}` can hold function \
                             values{through}"
                        ),
                    )
                    .primary_label("captured here")
                    .help(
                        "a closure owns what it captures, so storing it into this value — or into \
                         any value sharing its storage — would make a value that owns itself, \
                         which reference counting never reclaims. Nazm refuses the capture rather \
                         than leak it, by the captured type alone. Pass the value to the function \
                         as an argument instead of capturing it",
                    ),
                );
            }
        }
        let mut slot = b.slot;
        for i in crossing {
            let Some(c) = self.closures.get(i) else {
                continue;
            };
            let id = c.id;
            if let Some(inner) = c.captured.get(&slot).copied() {
                slot = inner;
                continue;
            }
            let inner = self
                .res
                .bind(
                    id,
                    // Declared nowhere: the copy is the captured binding's, and the service
                    // reads it as that binding (`Resolution::binding_of`).
                    LocalDef {
                        name: name.to_owned(),
                        ty: b.ty,
                        mutable: false,
                        span: Span::synthetic(),
                    },
                )
                .unwrap_or(0);
            self.res.record_capture(
                id,
                nazm_sema::Capture {
                    outer: slot,
                    inner,
                    ty: b.ty,
                },
            );
            if let Some(c) = self.closures.get_mut(i) {
                c.captured.insert(slot, inner);
            }
            slot = inner;
        }
        // Every closure's copy of a shared binding names the same cell (R1b, L10).
        if let Some(cell) = cell {
            let mut at_slot = b.slot;
            for i in self.closures_crossing(frame) {
                let Some(c) = self.closures.get(i) else {
                    continue;
                };
                if let Some(inner) = c.captured.get(&at_slot).copied() {
                    self.res.record_shared(c.id, inner, cell);
                    at_slot = inner;
                }
            }
            return Some(Binding {
                mutable: true,
                declared_at: b.declared_at,
                slot,
                ty: b.ty,
            });
        }
        Some(Binding {
            mutable: false,
            declared_at: None,
            slot,
            ty: b.ty,
        })
    }

    /// A named function written as a value (N50): non-generic, not foreign, not a built-in.
    /// Its type is its signature's, with its declared effects or, undeclared, none — which
    /// effects analysis then holds the function to.
    pub(super) fn function_value(&mut self, name: &str, at: Span, expected: Option<Type>) -> Type {
        let Some(v) = self.env.get(name).cloned() else {
            return expected.unwrap_or(Type::Int);
        };
        let refuse = |me: &mut Self, why: &str| {
            me.errors.push(
                Diagnostic::error(
                    codes::NOT_A_FUNCTION_VALUE,
                    at,
                    format!("`{name}` is {why}, and cannot be used as a value"),
                )
                .help(format!(
                    "call it as `{name}(...)`, or write a closure that calls it: \
                     `fn(x: Int) -> Int {{ {name}(x) }}`"
                )),
            );
            expected.unwrap_or(Type::Int)
        };
        let Callee::Function(id) = v.callee else {
            return refuse(self, "a built-in");
        };
        let Some(def) = self.res.function(id) else {
            return expected.unwrap_or(Type::Int);
        };
        if def.foreign.is_some() {
            return refuse(self, "a foreign function");
        }
        if v.sig.generics > 0 {
            return refuse(self, "generic");
        }
        // Undeclared: another module's may do anything the bridge covers; this module's is
        // assumed to do nothing, which effects analysis then holds it to.
        // Its effect parameter is bound at each call, and a value has no call (N51).
        if polymorphic(def.declared_effects) {
            return refuse(self, "polymorphic in its effects");
        }
        let assumed = match def.declared_effects {
            Some(set) => set,
            None if def.module != self.module => crate::effects::assumed(id, self.res),
            None => nazm_sema::EffectSet::EMPTY,
        };
        let ty = self.res.types_mut().fn_of(nazm_sema::FnSignature::new(
            v.sig.params.clone(),
            v.sig.ret,
            assumed,
        ));
        let held = self.held();
        self.res.record_fn_value(nazm_sema::FnValueUse {
            function: id,
            at,
            assumed,
            within: self.current,
            held,
            ty,
        });
        if let Some(cur) = self.current {
            self.res.record_mention(cur, ty);
        }
        ty
    }
}
