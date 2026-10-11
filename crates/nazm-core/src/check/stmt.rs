//! Blocks and statements: `let`, assignment, `spawn`, `return`, loops and loop control.

use super::clauses::shown_path;
use super::*;

impl Checker<'_> {
    pub(super) fn check_block(&mut self, b: &Block, expected: Option<Type>) -> Completion {
        self.open_scope(b.span);
        let completion = self.check_block_inner(b, expected);
        self.close_scope();
        completion
    }

    fn check_block_inner(&mut self, b: &Block, expected: Option<Type>) -> Completion {
        let mut diverged_at: Option<nazm_span::Span> = None;

        for stmt in &b.statements {
            if let Some(at) = diverged_at {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNREACHABLE_STATEMENT,
                        stmt.span(),
                        "this can never run",
                    )
                    .secondary(at, "control leaves the block here"),
                );
                // Keep checking the rest for type errors, but report unreachability once.
                diverged_at = None;
            }
            if self.check_stmt(stmt) {
                diverged_at = Some(stmt.span());
            }
        }

        // A block whose statements diverge never reaches its tail.
        if diverged_at.is_some() {
            if let Some(tail) = &b.tail {
                self.errors.push(Diagnostic::error(
                    codes::UNREACHABLE_STATEMENT,
                    tail.span(),
                    "this can never run",
                ));
            }
            return Completion::Diverges;
        }

        match &b.tail {
            Some(tail) => self.check_expr(tail, expected),
            None => Completion::Unit,
        }
    }

    /// Check one statement. Returns whether it diverges.
    fn check_stmt(&mut self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let(l) => {
                let ty = self.value_type(&l.value, None);
                if self.scopes.defined_here(&l.name) {
                    self.errors.push(
                        Diagnostic::error(
                            codes::ALREADY_DEFINED_IN_SCOPE,
                            l.name_span,
                            format!("`{}` is already defined in this scope", l.name),
                        )
                        .help(
                            "Nazm allows shadowing in a nested scope but not in the same \
                             one, so a later mention of a name is never ambiguous",
                        ),
                    );
                }
                let slot = self.allocate(&l.name, l.name_span, ty, l.mutable);
                // The declaration site resolves to the binding it introduces, so the
                // backend can find the slot for a `let` the same way it finds one for a
                // mention.
                self.record_var(l.name_span, slot);
                // Visible from the end of the statement: its own initializer, checked
                // above, could not see it, and nothing between the name and the `;` can.
                self.define(&l.name, ty, l.mutable, Some(l.name_span), slot, l.span.end);
                false
            }
            Stmt::Assign(a) => {
                self.check_assign(a);
                false
            }
            Stmt::While(w) => {
                self.check_while(w);
                // A `while` always completes: its condition may be false on entry, and
                // there is no way to tell that it will not be.
                false
            }
            Stmt::For(f) => {
                self.check_for(f);
                // A `for` always completes, as the `while` it means does.
                false
            }
            Stmt::Return(r) => {
                self.check_return(r);
                true
            }
            Stmt::Break { span } => {
                self.check_loop_control("break", *span);
                true
            }
            Stmt::Continue { span } => {
                self.check_loop_control("continue", *span);
                true
            }
            Stmt::Scope(s) => {
                // A scope joins every task it started, so it always completes — and a
                // `break` inside one still leaves the enclosing loop, exactly as a
                // nested block does.
                self.scope_depth += 1;
                let completion = self.check_block(&s.body, None);
                self.scope_depth -= 1;
                if let (Completion::Value(t), Some(tail)) = (completion, &s.body.tail) {
                    self.errors.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            tail.span(),
                            format!(
                                "a `scope` has no value, but this produces `{}`",
                                self.show(t)
                            ),
                        )
                        .help("add `;` to discard it, or send it through a channel"),
                    );
                }
                false
            }
            Stmt::Spawn(s) => {
                self.check_spawn(s);
                false
            }
            Stmt::Expr(e) => self.check_expr(e, None).diverges(),
        }
    }

    /// `spawn f(args);`
    ///
    /// The call itself is checked by the ordinary rule — a task is a function call that
    /// happens elsewhere, not a different kind of call. What is extra is *where* it may
    /// appear and *what* may cross into it.
    fn check_spawn(&mut self, s: &Spawn) {
        if self.scope_depth == 0 {
            self.errors.push(
                Diagnostic::error(
                    codes::SPAWN_OUTSIDE_SCOPE,
                    s.span,
                    "`spawn` is only valid inside a `scope`",
                )
                .help(
                    "a task outside a scope would be detached work, and nothing would \
                     join it; wrap it in `scope { … }`",
                ),
            );
        }

        let Some(sig) = self.resolve_callee(&s.callee, s.callee_span) else {
            let d = self
                .why_invisible(&s.callee, s.callee_span)
                .unwrap_or_else(|| {
                    Diagnostic::error(
                        codes::UNDEFINED_FUNCTION,
                        s.callee_span,
                        format!("`{}` is not a function", s.callee),
                    )
                });
            self.errors.push(d);
            for a in &s.args {
                self.check_expr(a, None);
            }
            return;
        };

        // A task runs one function with one signature. A generic one would need its type
        // arguments settled at the `spawn`, which N11 does not do: `docs/spec.md`, *Calling a
        // generic function*.
        if sig.generics > 0 {
            self.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    s.callee_span,
                    format!(
                        "`spawn {}` runs a generic function, which is not supported yet",
                        s.callee
                    ),
                )
                .help(
                    "a task runs a function that is not generic. Spawn a non-generic wrapper \
                     that calls the generic one",
                ),
            );
            for a in &s.args {
                self.check_expr(a, None);
            }
            return;
        }
        self.record_site(s.callee_span, Vec::new(), true);

        if s.args.len() != sig.params.len() {
            self.errors.push(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    s.span,
                    format!(
                        "`{}` takes {} argument{}, but {} {} given",
                        s.callee,
                        sig.params.len(),
                        if sig.params.len() == 1 { "" } else { "s" },
                        s.args.len(),
                        if s.args.len() == 1 { "was" } else { "were" }
                    ),
                )
                .secondary(sig.name_span, "defined here"),
            );
        }

        for (i, a) in s.args.iter().enumerate() {
            let want = sig.params.get(i).copied();
            let got = self.value_type(a, want);
            if let Some(want) = want {
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{}` is `{}`, but this is `{}`",
                            i + 1,
                            s.callee,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
            }
            // The capture rule. A sequence is a handle to unsynchronised shared mutable
            // storage, so letting one cross into a task would be a data race the
            // language had invited — see `docs/spec.md`.
            // Derived rather than asked of the type: a record's legality is its fields',
            // recursively, so the table answers and the answer carries the field path
            // that made it illegal. `docs/spec.md`, *Task safety is derived*.
            if let Err(why) = self.res.types().task_safety(got) {
                let shown = self.show(got);
                let named = self.show(why.ty);
                let message = if why.path.is_empty() {
                    format!("a `{shown}` may not be passed to a task")
                } else {
                    format!(
                        "a `{shown}` may not be passed to a task: its `{}` is a `{named}`",
                        why.path.join(".")
                    )
                };
                self.errors.push(
                    Diagnostic::error(codes::ILLEGAL_TASK_CAPTURE, a.span(), message).help(
                        "`Ints`, `Strs` and every `Vec[T]` are handles to shared mutable \
                         storage with no synchronisation, and a record or enum containing \
                         one at any depth inherits that — as does a type parameter, about \
                         which nothing is known. For an enum it is every variant that counts, not \
                         the one the value currently holds: what a task is given is a \
                         value of a type, and the guarantee is about the type. Pass \
                         `Int`, `Bool` or `Str` values, a `Chan`, or a record or enum \
                         whose every field may cross",
                    ),
                );
            }
        }
        self.record_checked_call(s.callee_span, s.span, &s.args, &sig, None, true);
    }

    fn check_return(&mut self, r: &Return) {
        let Some(declared) = self.return_type else {
            // Unreachable: every expression is checked inside some function.
            self.check_expr(&r.value, None);
            return;
        };
        let actual = self.value_type(&r.value, Some(declared));
        if !self.assignable(actual, declared) {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    r.value.span(),
                    format!(
                        "this returns `{}`, but the function returns `{}`",
                        self.show(actual),
                        self.show(declared)
                    ),
                )
                .help("`return` leaves the function, so its value must match the signature"),
            );
        }
    }

    fn check_assign(&mut self, a: &Assign) {
        let place = &a.place;
        let binding = self.lookup_var(&place.name, place.span);
        // A static is read, never assigned (Gate 3): there are no mutable global variables.
        if binding.is_none() && self.types.consts.statics.contains(&place.name) {
            let mut d = Diagnostic::error(
                codes::STATIC_ASSIGNED,
                place.name_span,
                format!(
                    "`{}` is a static, and a static is never assigned",
                    place.name
                ),
            )
            .help(
                "a static is read-only; one shared, changing value is a static `Atomic`, \
                 changed by `atomic_store` and the other atomic operations",
            );
            if let Some(decl) = self.types.consts.decls.get(&place.name) {
                d = d.secondary(*decl, "declared here");
            }
            self.errors.push(d);
            self.check_expr(&a.value, None);
            return;
        }
        let Some(binding) = binding else {
            self.errors.push(
                Diagnostic::error(
                    codes::UNDEFINED_VARIABLE,
                    place.name_span,
                    format!("`{}` is not defined", place.name),
                )
                .help("assignment changes an existing binding; `let` introduces one"),
            );
            self.check_expr(&a.value, None);
            return;
        };

        self.record_var(place.name_span, binding.slot);

        // Mutability is checked at the **root binding**, whatever the path. `p.x = 2`
        // replaces part of what `p` holds, so it needs `let mut p` for the same reason
        // `p = q` does — a record is a value, not a handle to one. `docs/spec.md`,
        // *Binding mutability reaches every field*.
        if !binding.mutable {
            let written = if place.path.is_empty() {
                format!("`{}` is not mutable", place.name)
            } else {
                format!(
                    "`{}` is not mutable, so its `{}` cannot be replaced",
                    place.name,
                    shown_path(&place.path)
                )
            };
            let d = Diagnostic::error(codes::ASSIGN_TO_IMMUTABLE, place.name_span, written);
            // The edit belongs at the `let`, not here. A parameter has no `let` to edit
            // — `mut` on one is refused — so it gets the other repair as prose instead
            // of an edit that would not compile.
            self.errors.push(match binding.declared_at {
                Some(at) => d
                    .help("bindings are immutable unless written `let mut`")
                    .secondary(at, "declared here")
                    .fix_automatically(
                        at,
                        format!("mut {}", place.name),
                        "make the binding mutable",
                        "`mut` is the only thing making this assignment illegal, and \
                         adding it to a declaration changes nothing else about the \
                         program",
                    ),
                None => d.help(
                    "this is a parameter, and parameters are never mutable. Bind a \
                     local from it instead: `let mut n = n;`",
                ),
            });
        }

        // Walk the projection path to the type actually being written. Each step is
        // recorded exactly as a read would be, because it is one: `a.b.c = v` reads `a.b`
        // and replaces `c` in it.
        let mut target = binding.ty;
        let mut declared_at = binding.declared_at;
        for step in &place.path {
            // An element of an array (Gate 2).
            let step = match step {
                nazm_syntax::PlaceStep::Field(f) => f,
                nazm_syntax::PlaceStep::Index { index, span } => {
                    let info = match target {
                        Type::Array(arr) => self.res.types().array_info(arr),
                        _ => None,
                    };
                    self.check_index_value(index, info.map(|(_, n)| n));
                    match info {
                        Some((elem, _)) => {
                            target = elem;
                            continue;
                        }
                        None => {
                            self.errors.push(Diagnostic::error(
                                codes::ARRAY_INDEX,
                                *span,
                                format!(
                                    "`[…]` indexes an array, and this is `{}`",
                                    self.show(target)
                                ),
                            ));
                            self.check_expr(&a.value, None);
                            return;
                        }
                    }
                }
            };
            match self.check_projection(target, &step.name, step.name_span) {
                Some(t) => {
                    declared_at = self
                        .res
                        .field(step.name_span)
                        .and_then(|r| self.res.records().get(r.record))
                        .and_then(|d| {
                            d.fields.get(
                                self.res
                                    .field(step.name_span)
                                    .map_or(usize::MAX, |r| r.field as usize),
                            )
                        })
                        .map(|f| f.name_span);
                    target = t;
                }
                None => {
                    self.check_expr(&a.value, None);
                    return;
                }
            }
        }

        // Checked against the place's type, so a reassignment cannot change it. A
        // variable whose type depends on how far through the function you are is exactly
        // the thing a reader cannot predict.
        let vt = self.value_type(&a.value, Some(target));
        if vt != target {
            let written = if place.path.is_empty() {
                place.name.clone()
            } else {
                format!("{}.{}", place.name, shown_path(&place.path))
            };
            let (starget, svt) = (self.show(target), self.show(vt));
            let mut d = Diagnostic::error(
                codes::TYPE_MISMATCH,
                a.value.span(),
                format!("`{written}` is `{starget}`, but this is `{svt}`"),
            );
            if let Some(at) = declared_at {
                d = d.secondary(at, format!("declared `{starget}` here"));
            }
            self.errors.push(d);
        }
    }

    fn check_while(&mut self, w: &While) {
        let ct = self.value_type(&w.cond, Some(Type::Bool));
        if ct != Type::Bool {
            self.errors.push(
                Diagnostic::error(
                    codes::CONDITION_NOT_BOOL,
                    w.cond.span(),
                    format!(
                        "a `while` condition must be `Bool`, but this is `{}`",
                        self.show(ct)
                    ),
                )
                .help("Nazm has no truthiness: use a comparison such as `i < n`"),
            );
        }
        self.loop_depth += 1;
        let body = self.check_block(&w.body, None);
        self.loop_depth -= 1;

        // A loop body is run for its effect. A tail value there is silently discarded,
        // which is almost always a missing `;` or a missing assignment.
        if let (Completion::Value(t), Some(tail)) = (body, &w.body.tail) {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    tail.span(),
                    format!(
                        "a loop body has no value, but this produces `{}`",
                        self.show(t)
                    ),
                )
                .help("add `;` to discard it, or assign it to something")
                .fix_with_review(
                    nazm_span::Span::new(tail.span().file, tail.span().end, tail.span().end),
                    ";",
                    "discard the value",
                    "the value was meant to be discarded rather than used — assigning \
                     it to something is the other repair, and this cannot tell which \
                     was meant",
                ),
            );
        }
    }

    /// A `for` loop (R1a, `docs/spec.md`, *`for` loops*): what it iterates, then its body with
    /// the name bound in the body's own scope — so the name is new each step, immutable, gone
    /// after the loop, and a `let` of it in the body is a same-scope duplicate (`N0202`).
    fn check_for(&mut self, f: &nazm_syntax::For) {
        let element = match &f.iter {
            nazm_syntax::ForIter::Each(e) => {
                let t = self.value_type(e, None);
                let element = match t {
                    Type::Ints => Some(Type::Int),
                    Type::Strs => Some(Type::Str),
                    Type::Bytes => Some(Type::of_num(NumKind::U8)),
                    Type::Vec(v) => self.res.types().vec_element(v),
                    _ => None,
                };
                if element.is_some() {
                    self.res.record_for_seq(f.name_span, t);
                }
                if element.is_none() {
                    self.errors.push(
                        Diagnostic::error(
                            codes::NOT_ITERABLE,
                            e.span(),
                            format!("a `for` loop cannot iterate `{}`", self.show(t)),
                        )
                        .help(
                            "a `for` loop iterates an `Ints`, a `Strs`, a `Vec[T]` or a `Bytes`, \
                             or integers written `a..b`",
                        ),
                    );
                }
                element.unwrap_or(Type::Int)
            }
            nazm_syntax::ForIter::Range { start, end } => {
                for bound in [start, end] {
                    let t = self.value_type(bound, Some(Type::Int));
                    if t != Type::Int {
                        self.errors.push(
                            Diagnostic::error(
                                codes::TYPE_MISMATCH,
                                bound.span(),
                                format!(
                                    "a range's bounds are `Int`s, but this is `{}`",
                                    self.show(t)
                                ),
                            )
                            .help("convert it with `Int(…)`"),
                        );
                    }
                }
                Type::Int
            }
        };
        self.loop_depth += 1;
        self.open_scope(f.body.span);
        let slot = self.allocate(&f.name, f.name_span, element, false);
        self.record_var(f.name_span, slot);
        self.define(
            &f.name,
            element,
            false,
            Some(f.name_span),
            slot,
            f.body.span.start,
        );
        let body = self.check_block_inner(&f.body, None);
        self.close_scope();
        self.loop_depth -= 1;
        if let (Completion::Value(t), Some(tail)) = (body, &f.body.tail) {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    tail.span(),
                    format!(
                        "a loop body has no value, but this produces `{}`",
                        self.show(t)
                    ),
                )
                .help("add `;` to discard it, or assign it to something"),
            );
        }
    }

    fn check_loop_control(&mut self, word: &str, span: Span) {
        if self.loop_depth == 0 {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_IN_A_LOOP,
                    span,
                    format!("`{word}` is only valid inside a loop"),
                )
                .help(format!(
                    "there is no enclosing `while` for this `{word}` to affect"
                )),
            );
        }
    }

    /// The type of an expression used where a value is required.
    ///
    /// A diverging expression can stand in for any type: control never reaches the point
    /// where the value would be used.
    pub(super) fn value_type(&mut self, e: &Expr, expected: Option<Type>) -> Type {
        match self.check_expr(e, expected) {
            Completion::Value(t) => t,
            Completion::Diverges => expected.unwrap_or(Type::Int),
            Completion::Unit => {
                self.errors.push(
                    Diagnostic::error(codes::TYPE_MISMATCH, e.span(), "this produces no value")
                        .help(
                            "an `if` used as a value needs an `else`, and a block used as one \
                         needs a final expression with no `;` after it",
                        ),
                );
                expected.unwrap_or(Type::Int)
            }
        }
    }
}
