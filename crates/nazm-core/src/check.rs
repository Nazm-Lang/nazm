//! Type and scope checking.
//!
//! Runs before evaluation and reports **every** error it can, not just the first: an
//! agent that fixes one error and recompiles to find the next spends a turn per mistake,
//! and the whole argument for this language rests on not doing that.
//!
//! Where an error makes a subexpression's type unknowable, checking continues with the
//! *expected* type rather than stopping. That produces one honest error instead of a
//! cascade of invented ones.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use nazm_syntax::{Assign, BinOp, Block, Expr, Function, Program, Return, Spawn, Stmt, While};

use nazm_syntax::intrinsic::Intrinsic;
use nazm_syntax::resolve::{Callee, FnDef, FnId, LocalDef, Resolution};
use nazm_syntax::types::Type;

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<Type>,
    pub ret: Type,
    pub name_span: Span,
}

/// What is known about a binding.
#[derive(Debug, Clone, Copy)]
struct Binding {
    ty: Type,
    mutable: bool,
    /// Where this binding lives in its function's storage.
    ///
    /// Assigned here, once, and read by **both** consumers. The backend used to allocate
    /// its own slots on a second walk; it now reads these, which is the whole point of
    /// `nazm_syntax::Resolution`.
    slot: u32,
    /// Where the name was written in the declaration, when there is a declaration to
    /// edit. `None` for a parameter.
    ///
    /// Carried only so that the "not mutable" diagnostic can offer an edit at the
    /// **`let`**. The obvious span — the name in the failing assignment — produces
    /// `mut x = 2;`, which is not a statement in this language: a fix that emits
    /// something that does not parse is worse than no fix, because a tool applies it.
    declared_at: Option<Span>,
}

/// Lexical scopes. Innermost last.
#[derive(Debug, Default)]
struct Scopes {
    frames: Vec<HashMap<String, Binding>>,
}

impl Scopes {
    fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.frames.iter().rev().find_map(|f| f.get(name).copied())
    }

    /// Whether the name is already bound **in the innermost scope**.
    ///
    /// Only the innermost: an inner scope may shadow an outer one, because there the
    /// reader can see the boundary. Two `let`s of the same name side by side is what
    /// makes a later mention ambiguous. See `docs/spec.md`.
    fn defined_here(&self, name: &str) -> bool {
        self.frames.last().is_some_and(|f| f.contains_key(name))
    }

    fn define(
        &mut self,
        name: &str,
        ty: Type,
        mutable: bool,
        declared_at: Option<Span>,
        slot: u32,
    ) {
        if let Some(f) = self.frames.last_mut() {
            f.insert(
                name.to_owned(),
                Binding {
                    ty,
                    mutable,
                    declared_at,
                    slot,
                },
            );
        }
    }
}

/// The facts `check_binary` needs about one operator application.
///
/// A struct rather than six positional arguments: at the call site `(*op, lt, rt,
/// *op_span, lhs.span(), rhs.span())` is four spans and two types in an order nothing
/// enforces, and swapping two of them compiles.
#[derive(Debug, Clone, Copy)]
struct BinaryCheck {
    op: BinOp,
    lt: Type,
    rt: Type,
    op_span: Span,
    lhs_span: Span,
    rhs_span: Span,
}

/// How a block or expression finishes.
///
/// Internal. The language has no `unit` or `never` type to write, and adding one to model
/// this would be a public decision made for an implementation's convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Falls off the end with a value.
    Value(Type),
    /// Falls off the end with no value.
    Unit,
    /// Never falls off the end: every path returns, breaks or continues.
    Diverges,
}

impl Completion {
    /// The type, when there is one.
    fn ty(self) -> Option<Type> {
        match self {
            Self::Value(t) => Some(t),
            Self::Unit | Self::Diverges => None,
        }
    }

    fn diverges(self) -> bool {
        matches!(self, Self::Diverges)
    }
}

#[derive(Debug)]
pub struct Checker {
    /// What resolution decided, and the single answer both consumers read.
    res: Resolution,
    /// The function whose body is being checked, for slot allocation.
    current: Option<FnId>,
    functions: HashMap<String, Signature>,
    scopes: Scopes,
    errors: Vec<Diagnostic>,
    /// The enclosing function's declared return type, for checking `return`.
    return_type: Option<Type>,
    /// How many `scope` blocks enclose the statement being checked.
    ///
    /// `spawn` needs one: a task with no scope would be detached work, and the model
    /// has none. Depth rather than a flag, so a nested scope does not lose the outer
    /// one's count when it closes.
    scope_depth: u32,
    /// How many loops enclose the statement being checked.
    ///
    /// `break` and `continue` need a loop to affect. Tracking depth rather than a flag
    /// costs nothing and means a loop inside a loop does not lose the outer one's count
    /// when it finishes.
    loop_depth: u32,
}

/// Check a program. Returns the signatures and every diagnostic found.
#[must_use]
pub fn check(program: &Program) -> (Resolution, Vec<Diagnostic>) {
    let mut c = Checker {
        functions: HashMap::new(),
        res: Resolution::default(),
        current: None,
        scopes: Scopes::default(),
        errors: Vec::new(),
        return_type: None,
        scope_depth: 0,
        loop_depth: 0,
    };

    // Built-ins first, so a user definition of one collides with it rather than
    // shadowing it. They share the ordinary function namespace deliberately: a call is
    // a call, and `str_len(s)` should resolve by the same rule as any other.
    for i in Intrinsic::ALL {
        c.functions.insert(
            i.name().to_owned(),
            Signature {
                params: i.params().to_vec(),
                ret: i.ret(),
                name_span: Span::synthetic(),
            },
        );
    }

    // Signatures next, so a call to a function defined later in the file resolves.
    for f in &program.functions {
        c.collect_signature(f);
    }

    if !c.functions.contains_key("main") {
        c.errors.push(
            Diagnostic::error(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "this program has no `main` function",
            )
            .help("`nazm run` evaluates `fn main() -> Int` and prints its value"),
        );
    }

    for f in &program.functions {
        c.check_function(f);
    }

    (c.res, c.errors)
}

impl Checker {
    /// Give a binding its slot in the current function's storage.
    ///
    /// **Parameters first, in order, then body bindings as they are declared, and never
    /// reused** — including for a name that shadows another. That ordering is a contract
    /// the backend relies on to store incoming arguments without a separate notion of what
    /// a parameter is, and it lives here now because this is where bindings are made.
    fn allocate(&mut self, name: &str, ty: Type, mutable: bool) -> u32 {
        let Some(id) = self.current else {
            return 0;
        };
        self.res
            .bind(
                id,
                LocalDef {
                    name: name.to_owned(),
                    ty,
                    mutable,
                },
            )
            .unwrap_or(0)
    }

    /// Record what a call resolved to, and answer the same question for the caller.
    ///
    /// **Built-ins are resolved before user functions.** A user definition of a built-in's
    /// name is refused at declaration, so the two sets are disjoint and the order states
    /// intent rather than breaking a tie. Written once here because it is a language rule;
    /// the evaluator and the backend used to each apply their own copy of it.
    fn resolve_callee(&mut self, name: &str, at: Span) -> Option<Callee> {
        let callee = Intrinsic::by_name(name)
            .map(Callee::Builtin)
            .or_else(|| self.res.function_id(name).map(Callee::Function))?;
        self.res.record_call(at, callee);
        Some(callee)
    }

    fn resolve_type(&mut self, name: &str, span: Span) -> Type {
        match Type::from_name(name) {
            Some(t) => t,
            None => {
                self.errors.push(
                    Diagnostic::error(codes::UNKNOWN_TYPE, span, format!("`{name}` is not a type"))
                        .help(format!("v0.1 has {}", Type::all_names())),
                );
                // Continue as `Int` rather than abandoning the function: one unknown type
                // should not suppress every other error in the body.
                Type::Int
            }
        }
    }

    fn collect_signature(&mut self, f: &Function) {
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| self.resolve_type(&p.ty.name, p.ty.span))
            .collect();
        let ret = self.resolve_type(&f.return_type.name, f.return_type.span);

        // A built-in has no source span to point at, so "first defined here" would aim
        // at byte zero. Said plainly instead.
        if Intrinsic::by_name(&f.name).is_some() {
            self.errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    format!("`{}` is a built-in and cannot be redefined", f.name),
                )
                .help(
                    "built-ins share the ordinary function namespace so that a call \
                     always means the same thing; choose another name",
                ),
            );
            return;
        }
        if let Some(existing) = self.functions.get(&f.name) {
            self.errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    format!("`{}` is defined more than once", f.name),
                )
                .secondary(existing.name_span, "first defined here"),
            );
            return;
        }
        // The same declaration, in the artefact the backend will read. Declared here and
        // not in a second pass, so the two cannot disagree about which functions exist.
        self.res.declare(FnDef {
            name: f.name.clone(),
            params: params.clone(),
            ret,
            name_span: f.name_span,
            locals: Vec::new(),
        });
        self.functions.insert(
            f.name.clone(),
            Signature {
                params,
                ret,
                name_span: f.name_span,
            },
        );
    }

    fn check_function(&mut self, f: &Function) {
        // Slot allocation starts fresh per function: a caller's local and a callee's
        // local with the same spelling are different storage, not the same.
        self.current = self.res.function_id(&f.name);
        self.scopes.push();
        for p in &f.params {
            let ty = self.resolve_type(&p.ty.name, p.ty.span);
            if self.scopes.defined_here(&p.name) {
                self.errors.push(Diagnostic::error(
                    codes::ALREADY_DEFINED_IN_SCOPE,
                    p.span,
                    format!("`{}` is listed twice in this parameter list", p.name),
                ));
            }
            // Parameters are never mutable. A mutable parameter is a local copy, which
            // reads as an out-parameter to anyone who has not checked the calling
            // convention; `let mut` in the body says the same thing unambiguously.
            // No declaration span: `mut` on a parameter is `N0311`, so there is no
            // edit at the declaration that would make this assignment legal.
            let slot = self.allocate(&p.name, ty, false);
            self.scopes.define(&p.name, ty, false, None, slot);
        }

        let declared = self.resolve_type(&f.return_type.name, f.return_type.span);
        let Some(body) = &f.body else {
            // The body failed to parse; the parse error is already reported. Anything
            // said about statements that were never built would be about the parser's
            // guess, not the program.
            self.scopes.pop();
            return;
        };
        let previous = self.return_type.replace(declared);
        let completion = self.check_block(body, Some(declared));
        self.return_type = previous;

        match completion {
            // Every path returned. No tail is needed, and demanding one would force a
            // dummy expression after code that cannot reach it.
            Completion::Diverges => {}
            Completion::Value(actual) if actual == declared => {}
            Completion::Value(actual) => {
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.tail.as_ref().map_or(body.span, |t| t.span()),
                        format!(
                            "`{}` returns `{actual}`, but its signature says `{declared}`",
                            f.name
                        ),
                    )
                    .secondary(f.return_type.span, format!("declared `{declared}` here")),
                );
            }
            Completion::Unit => {
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.span,
                        format!(
                            "`{}` must produce a `{declared}`, but this body does not",
                            f.name
                        ),
                    )
                    .secondary(f.return_type.span, format!("declared `{declared}` here"))
                    .help(
                        "a function's value is its final expression — one with no `;` \
                         after it — or it returns on every path",
                    ),
                );
            }
        }
        self.scopes.pop();
    }

    fn check_block(&mut self, b: &Block, expected: Option<Type>) -> Completion {
        self.scopes.push();
        let completion = self.check_block_inner(b, expected);
        self.scopes.pop();
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
                let slot = self.allocate(&l.name, ty, l.mutable);
                // The declaration site resolves to the binding it introduces, so the
                // backend can find the slot for a `let` the same way it finds one for a
                // mention.
                self.res.record_var(l.name_span, slot);
                self.scopes
                    .define(&l.name, ty, l.mutable, Some(l.name_span), slot);
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
                            format!("a `scope` has no value, but this produces `{t}`"),
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

        self.resolve_callee(&s.callee, s.callee_span);
        let Some(sig) = self.functions.get(&s.callee).cloned() else {
            self.errors.push(Diagnostic::error(
                codes::UNDEFINED_FUNCTION,
                s.callee_span,
                format!("`{}` is not a function", s.callee),
            ));
            for a in &s.args {
                self.check_expr(a, None);
            }
            return;
        };

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
                if got != want {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{}` is `{want}`, but this is `{got}`",
                            i + 1,
                            s.callee
                        ),
                    ));
                }
            }
            // The capture rule. A sequence is a handle to unsynchronised shared mutable
            // storage, so letting one cross into a task would be a data race the
            // language had invited — see `docs/spec.md`.
            if !got.may_cross_into_a_task() {
                self.errors.push(
                    Diagnostic::error(
                        codes::ILLEGAL_TASK_CAPTURE,
                        a.span(),
                        format!("a `{got}` may not be passed to a task"),
                    )
                    .help(
                        "`Ints` and `Strs` are handles to shared mutable storage with no \
                         synchronisation. Pass `Int`, `Bool` or `Str` values, or a `Chan`, \
                         which is the primitive designed to be shared",
                    ),
                );
            }
        }
    }

    fn check_return(&mut self, r: &Return) {
        let Some(declared) = self.return_type else {
            // Unreachable: every expression is checked inside some function.
            self.check_expr(&r.value, None);
            return;
        };
        let actual = self.value_type(&r.value, Some(declared));
        if actual != declared {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    r.value.span(),
                    format!("this returns `{actual}`, but the function returns `{declared}`"),
                )
                .help("`return` leaves the function, so its value must match the signature"),
            );
        }
    }

    fn check_assign(&mut self, a: &Assign) {
        let Some(binding) = self.scopes.lookup(&a.name) else {
            self.errors.push(
                Diagnostic::error(
                    codes::UNDEFINED_VARIABLE,
                    a.name_span,
                    format!("`{}` is not defined", a.name),
                )
                .help("assignment changes an existing binding; `let` introduces one"),
            );
            self.check_expr(&a.value, None);
            return;
        };

        self.res.record_var(a.name_span, binding.slot);

        if !binding.mutable {
            let d = Diagnostic::error(
                codes::ASSIGN_TO_IMMUTABLE,
                a.name_span,
                format!("`{}` is not mutable", a.name),
            );
            // The edit belongs at the `let`, not here. A parameter has no `let` to edit
            // — `mut` on one is refused — so it gets the other repair as prose instead
            // of an edit that would not compile.
            self.errors.push(match binding.declared_at {
                Some(at) => d
                    .help("bindings are immutable unless written `let mut`")
                    .secondary(at, "declared here")
                    .fix_automatically(
                        at,
                        format!("mut {}", a.name),
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

        // Checked against the binding's type, so a reassignment cannot change it. A
        // variable whose type depends on how far through the function you are is exactly
        // the thing a reader cannot predict.
        let vt = self.value_type(&a.value, Some(binding.ty));
        if vt != binding.ty {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    a.value.span(),
                    format!("`{}` is `{}`, but this is `{vt}`", a.name, binding.ty),
                )
                .secondary(a.name_span, format!("declared `{}` here", binding.ty)),
            );
        }
    }

    fn check_while(&mut self, w: &While) {
        let ct = self.value_type(&w.cond, Some(Type::Bool));
        if ct != Type::Bool {
            self.errors.push(
                Diagnostic::error(
                    codes::CONDITION_NOT_BOOL,
                    w.cond.span(),
                    format!("a `while` condition must be `Bool`, but this is `{ct}`"),
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
                    format!("a loop body has no value, but this produces `{t}`"),
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
    fn value_type(&mut self, e: &Expr, expected: Option<Type>) -> Type {
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

    /// Check an expression, returning how it completes.
    ///
    /// `expected` is used only for recovery: when an error makes the real type
    /// unknowable, it is returned so the caller does not then report a second, invented
    /// mismatch.
    fn check_expr(&mut self, e: &Expr, expected: Option<Type>) -> Completion {
        match e {
            Expr::Int { .. } => Completion::Value(Type::Int),
            Expr::Bool { .. } => Completion::Value(Type::Bool),
            Expr::Str { .. } => Completion::Value(Type::Str),

            Expr::Var { name, span } => match self.scopes.lookup(name) {
                Some(b) => {
                    // Which binding this mention refers to, recorded where the question
                    // was answered. The backend reads this instead of walking scopes again.
                    self.res.record_var(*span, b.slot);
                    Completion::Value(b.ty)
                }
                None => {
                    let mut d = Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    );
                    if self.functions.contains_key(name) {
                        d = d.help(format!("`{name}` is a function — call it as `{name}(...)`"));
                    }
                    self.errors.push(d);
                    Completion::Value(expected.unwrap_or(Type::Int))
                }
            },

            Expr::Neg {
                operand, op_span, ..
            } => {
                let t = self.value_type(operand, Some(Type::Int));
                if t != Type::Int {
                    self.errors.push(Diagnostic::error(
                        codes::INVALID_OPERANDS,
                        *op_span,
                        format!("`-` applies to `Int`, but this is `{t}`"),
                    ));
                }
                Completion::Value(Type::Int)
            }

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                let lt = self.value_type(lhs, None);
                let rt = self.value_type(rhs, None);
                Completion::Value(self.check_binary(
                    &BinaryCheck {
                        op: *op,
                        lt,
                        rt,
                        op_span: *op_span,
                        lhs_span: lhs.span(),
                        rhs_span: rhs.span(),
                    },
                    expected,
                ))
            }

            Expr::Call {
                callee,
                callee_span,
                args,
                span,
            } => {
                self.resolve_callee(callee, *callee_span);
                let Some(sig) = self.functions.get(callee).cloned() else {
                    self.errors.push(Diagnostic::error(
                        codes::UNDEFINED_FUNCTION,
                        *callee_span,
                        format!("`{callee}` is not a function"),
                    ));
                    for a in args {
                        self.check_expr(a, None);
                    }
                    return Completion::Value(expected.unwrap_or(Type::Int));
                };

                if args.len() != sig.params.len() {
                    self.errors.push(
                        Diagnostic::error(
                            codes::WRONG_ARGUMENT_COUNT,
                            *span,
                            format!(
                                "`{callee}` takes {} argument{}, but {} {} given",
                                sig.params.len(),
                                if sig.params.len() == 1 { "" } else { "s" },
                                args.len(),
                                if args.len() == 1 { "was" } else { "were" }
                            ),
                        )
                        .secondary(sig.name_span, "defined here"),
                    );
                }

                // Left to right, matching the evaluation order the spec fixes.
                for (i, a) in args.iter().enumerate() {
                    let want = sig.params.get(i).copied();
                    let got = self.value_type(a, want);
                    if let Some(want) = want {
                        if got != want {
                            self.errors.push(Diagnostic::error(
                                codes::TYPE_MISMATCH,
                                a.span(),
                                format!(
                                    "argument {} of `{callee}` is `{want}`, but this is `{got}`",
                                    i + 1
                                ),
                            ));
                        }
                    }
                }
                Completion::Value(sig.ret)
            }

            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let ct = self.value_type(cond, Some(Type::Bool));
                if ct != Type::Bool {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CONDITION_NOT_BOOL,
                            cond.span(),
                            format!("an `if` condition must be `Bool`, but this is `{ct}`"),
                        )
                        .help("Nazm has no truthiness: use a comparison such as `x != 0`"),
                    );
                }

                let then_c = self.check_block(then_block, expected);
                let Some(else_block) = else_block else {
                    // No `else`. Unit whatever the branch does — even when it returns on
                    // every path, the condition may be false, so the `if` itself always
                    // completes. Usable as a statement; the caller reports if it wanted a
                    // value, which is where the context to say so lives.
                    return Completion::Unit;
                };
                let else_c = self.check_block(else_block, expected.or(then_c.ty()));

                match (then_c, else_c) {
                    // One branch leaving means the other decides. Without this,
                    // `if c { return 0; } else { 1 }` would be an error for having
                    // "different" branches, which is exactly the shape `return` is for.
                    (Completion::Diverges, other) | (other, Completion::Diverges) => other,
                    (Completion::Value(a), Completion::Value(b)) if a == b => Completion::Value(a),
                    (Completion::Value(a), Completion::Value(b)) => {
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                else_block
                                    .tail
                                    .as_ref()
                                    .map_or(else_block.span, |t| t.span()),
                                format!("this branch is `{b}`, but the other is `{a}`"),
                            )
                            .secondary(
                                then_block
                                    .tail
                                    .as_ref()
                                    .map_or(then_block.span, |t| t.span()),
                                format!("this one is `{a}`"),
                            )
                            .help("both branches of an `if` must have the same type"),
                        );
                        Completion::Value(a)
                    }
                    // One branch produces a value and the other does not.
                    (Completion::Value(a), Completion::Unit)
                    | (Completion::Unit, Completion::Value(a)) => {
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                *e.span_ref(),
                                format!("one branch produces `{a}` and the other produces nothing"),
                            )
                            .help("both branches of an `if` must agree, or neither may produce a value"),
                        );
                        Completion::Value(a)
                    }
                    (Completion::Unit, Completion::Unit) => Completion::Unit,
                }
            }

            Expr::Block(b) => self.check_block(b, expected),
        }
    }

    fn check_binary(&mut self, b: &BinaryCheck, expected: Option<Type>) -> Type {
        let BinaryCheck {
            op,
            lt,
            rt,
            op_span,
            lhs_span,
            rhs_span,
        } = *b;
        let ok = match op {
            // Arithmetic is `Int` only. No string concatenation in v0.1, and `+` on two
            // `Bool`s is a mistake rather than something to define.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                lt == Type::Int && rt == Type::Int
            }
            // Ordering is `Int` only: comparing strings needs a collation decision, and
            // ordering booleans is almost always a mistake.
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => lt == Type::Int && rt == Type::Int,
            // Equality works on any single type, but not across two — and **not on a
            // sequence**, where it would have to mean either "the same handle" or "the
            // same contents" and the language has chosen neither. Offering one silently
            // is how a program comes to depend on the wrong answer.
            BinOp::Eq | BinOp::NotEq => lt == rt && !lt.is_sequence() && lt != Type::Chan,
        };

        if !ok {
            // "compare" is only right for a comparison. Saying it of `+` describes the
            // wrong operation and reads as a compiler that does not know what it is
            // looking at.
            let verb = if op.is_comparison() {
                "compare"
            } else {
                "combine"
            };
            let detail = if lt == rt {
                format!("`{}` does not apply to `{lt}`", op.symbol())
            } else {
                format!("`{}` cannot {verb} `{lt}` with `{rt}`", op.symbol())
            };
            self.errors.push(
                Diagnostic::error(codes::INVALID_OPERANDS, op_span, detail)
                    .secondary(lhs_span, format!("this is `{lt}`"))
                    .secondary(rhs_span, format!("this is `{rt}`")),
            );

            // The operands were wrong, so this expression has no honest type. Reporting
            // the operator's usual one produces a second error about a mismatch the
            // author never wrote — two diagnostics for one mistake, and the second is
            // fiction. Adopt what the context wanted instead.
            return expected.unwrap_or(if op.is_comparison() {
                Type::Bool
            } else {
                Type::Int
            });
        }

        if op.is_comparison() {
            Type::Bool
        } else {
            Type::Int
        }
    }
}
