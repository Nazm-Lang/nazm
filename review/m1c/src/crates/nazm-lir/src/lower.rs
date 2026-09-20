//! Syntax tree to [`crate::ir`], with everything outside the native subset refused by name.
//!
//! # Refusal, not partial compilation
//!
//! Lowering runs *after* `nazm_core::check` has passed, so a program reaching here is
//! well-typed. That is exactly why the refusals below are about the backend's coverage
//! and not about the program being wrong: `while` is a perfectly good Nazm construct that
//! this increment cannot compile, and saying so is different from saying it is invalid.
//! Every refusal points at `nazm run`, which still executes all of it.
//!
//! Every unsupported construct produces a diagnostic and **no output**. A compiler that
//! emits something for a program it did not fully understand is worse than one that
//! stops, because the something runs.
//!
//! # The one `if`
//!
//! `docs/spec.md` has a single `if` whose meaning is decided by checking, not by syntax.
//! This backend compiles it **only in value position**, and the way that stays true is
//! that a statement-position `if` never arrives here as an `if` at all — it arrives as
//! `Stmt::Expr`, which is refused. There is no arm below that handles an `if` generically
//! and then discovers what else it covered.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use nazm_syntax::{
    BinOp as AstOp, Block as AstBlock, Expr as AstExpr, Function as AstFn, Program, Stmt as AstStmt,
};

use std::collections::HashMap;

use crate::ir::{ArithOp, Block, CmpOp, Expr, FuncId, Function, Local, Module, Stmt, Ty};

/// What the subset is, in one sentence, for every refusal to point at.
const SUBSET: &str = "`nazm build` currently compiles functions over `Int` and `Bool` — \
                      literals, immutable `let` bindings and parameters, unary `-`, the \
                      operators `+ - * / %`, the comparisons `== != < <= > >=`, \
                      parentheses, `if`/`else` used as a value, and non-recursive direct \
                      calls — with `fn main() -> Int` as the entry point. Everything else \
                      still runs under `nazm run`";

/// Constructs outside the native subset, with the spans that named them.
#[derive(Debug)]
pub struct LowerError {
    pub diagnostics: Vec<Diagnostic>,
}

/// What is known about a function before any body is lowered.
struct Sig {
    id: FuncId,
    params: Vec<Ty>,
    ret: Ty,
}

struct Ctx {
    /// Innermost-last. Each `if` branch pushes one, so a binding made inside a branch is
    /// invisible after the join — which is what stops a value that exists on only one
    /// path being read on both.
    ///
    /// Reset for each function: a callee cannot see a caller's locals, and two functions
    /// that use the same name get different slots in different spaces.
    scopes: Vec<Vec<(String, usize)>>,
    /// The function currently being lowered. Also reset per function.
    locals: Vec<Local>,
    /// Every function's signature, collected before any body. This is what makes a
    /// forward reference work without a second pass over the syntax tree.
    ///
    /// Keyed by name because that is how the *checker* resolves a call — and functions
    /// live in a namespace of their own there, separate from `Scopes`, so a local named
    /// `f` does not shadow a function `f`. Reproducing that is the point; inventing a
    /// shadowing rule here would make the two disagree.
    sigs: HashMap<String, Sig>,
    /// Which functions each function calls, for the cycle check.
    calls: Vec<Vec<(FuncId, Span)>>,
    /// The function whose body is being lowered, for recording edges.
    current: FuncId,
    errors: Vec<Diagnostic>,
}

impl Ctx {
    /// `what` is a short noun phrase. The sentence supplies the rest, and the help
    /// supplies the subset — a phrase carrying its own clause reads as two sentences
    /// spliced together, which is how the first version of these messages read.
    fn unsupported(&mut self, span: Span, what: &str) {
        self.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                span,
                format!("{what} is not in `nazm build`'s subset yet"),
            )
            .primary_label("not in the native subset")
            .help(SUBSET),
        );
    }

    fn resolve(&self, name: &str) -> Option<(usize, Ty)> {
        self.scopes.iter().rev().find_map(|s| {
            s.iter()
                .rev()
                .find(|(n, _)| n == name)
                .and_then(|(_, i)| self.locals.get(*i).map(|l| (*i, l.ty)))
        })
    }

    /// A fresh slot. Never reused, even for a name that shadows another: two bindings
    /// that are live at once must not share storage, and "is the old one still live?" is
    /// a question the emitter should never have to ask.
    fn bind(&mut self, name: &str, ty: Ty) -> usize {
        let slot = self.locals.len();
        self.locals.push(Local {
            name: name.to_owned(),
            ty,
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_owned(), slot));
        }
        slot
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Vec::new());
        let out = f(self);
        self.scopes.pop();
        out
    }
}

/// Lower a checked program, or explain what stopped it.
///
/// # Two passes, and why
///
/// Signatures for **every** function are collected first, then bodies are lowered. A
/// forward reference is then ordinary: `main` may call a helper declared below it without
/// the lowering having to guess a type it has not read yet. It also means a call resolves
/// to a [`FuncId`] once, here — emission never looks a name up again, so there is only
/// one place the two could disagree.
///
/// # Errors
///
/// *Every* construct outside the subset, not just the first: one run of `nazm build`
/// should say everything that would have to change, which is the same
/// one-mistake-one-error discipline the rest of the front end holds to.
pub fn lower(program: &Program) -> Result<Module, LowerError> {
    let mut ctx = Ctx {
        scopes: vec![Vec::new()],
        locals: Vec::new(),
        sigs: HashMap::new(),
        calls: Vec::new(),
        current: FuncId(0),
        errors: Vec::new(),
    };

    // Pass 1: signatures. Duplicates are the checker's business (`N0203`) and it has
    // already reported them, so the first of a repeated name wins here rather than
    // producing a second diagnostic about the same mistake.
    let mut order: Vec<&AstFn> = Vec::new();
    for f in &program.functions {
        if ctx.sigs.contains_key(&f.name) {
            continue;
        }
        let Some(sig) = signature(&mut ctx, f) else {
            continue;
        };
        ctx.sigs.insert(f.name.clone(), sig);
        ctx.calls.push(Vec::new());
        order.push(f);
    }

    let entry = match ctx.sigs.get("main").map(|s| s.id) {
        Some(id) => id,
        // `main` has no signature here for one of two reasons, and they are different
        // facts. Either the program genuinely has none — unreachable after a successful
        // check, which reports `N0204` itself — or this backend *refused* `main`'s
        // signature, in which case the refusal is already in `errors` and reporting a
        // missing `main` on top of it would be a second, false diagnostic about a
        // function the reader can see.
        None if !ctx.errors.is_empty() => {
            return Err(LowerError {
                diagnostics: ctx.errors,
            })
        }
        None => {
            return Err(LowerError {
                diagnostics: vec![Diagnostic::error(
                    codes::MISSING_MAIN,
                    Span::new(0, 0),
                    "this program has no `main` to compile",
                )],
            })
        }
    };

    // Pass 2: bodies.
    let mut functions = Vec::new();
    for f in order {
        if let Some(lowered) = lower_function(&mut ctx, f) {
            functions.push(lowered);
        }
    }

    // The call graph is complete only now, so the cycle check comes last.
    report_cycles(&mut ctx, &functions);

    if ctx.errors.is_empty() && functions.len() == ctx.sigs.len() {
        Ok(Module { functions, entry })
    } else if !ctx.errors.is_empty() {
        Err(LowerError {
            diagnostics: ctx.errors,
        })
    } else {
        Err(LowerError {
            diagnostics: vec![Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                Span::new(0, 0),
                "`nazm build` could not lower this program and did not say why",
            )
            .help("this is a bug in the compiler; `nazm run` still works")],
        })
    }
}

/// A function's signature, or a refusal naming the type this backend cannot represent.
fn signature(ctx: &mut Ctx, f: &AstFn) -> Option<Sig> {
    let id = FuncId(ctx.sigs.len());
    let mut params = Vec::new();
    let mut ok = true;
    for p in &f.params {
        match native_ty(&p.ty.name) {
            Some(t) => params.push(t),
            None => {
                ctx.unsupported(p.span, &format!("a parameter of type `{}`", p.ty.name));
                ok = false;
            }
        }
    }
    let Some(ret) = native_ty(&f.return_type.name) else {
        ctx.unsupported(
            f.return_type.span,
            &format!("a function returning `{}`", f.return_type.name),
        );
        return None;
    };
    if f.name == "main" {
        if !f.params.is_empty() {
            ctx.unsupported(f.span, "a `main` with parameters");
            ok = false;
        }
        if ret != Ty::Int {
            ctx.unsupported(f.return_type.span, "a `main` that does not return `Int`");
            ok = false;
        }
    }
    ok.then_some(Sig { id, params, ret })
}

fn native_ty(name: &str) -> Option<Ty> {
    match name {
        "Int" => Some(Ty::Int),
        "Bool" => Some(Ty::Bool),
        _ => None,
    }
}

/// Lower one function body, in a namespace of its own.
fn lower_function(ctx: &mut Ctx, f: &AstFn) -> Option<Function> {
    let &Sig { id, ret, .. } = ctx.sigs.get(&f.name)?;
    // A fresh slot space and a fresh scope stack. Nothing from the previous function is
    // visible, which is what makes a caller's local and a callee's local with the same
    // spelling different storage rather than the same.
    ctx.scopes = vec![Vec::new()];
    ctx.locals = Vec::new();
    ctx.current = id;

    // Parameters are the first slots, in order, so the emitter can store the incoming
    // arguments into them without a separate notion of what a parameter is. They are
    // never stored to again: the language has no mutable parameter — `fn f(mut x: Int)`
    // is refused by the parser — so one write at entry is the whole story.
    let params = f.params.len();
    for p in &f.params {
        let ty = native_ty(&p.ty.name)?;
        ctx.bind(&p.name, ty);
    }

    // The body is a scope *inside* the parameter scope, which is how the checker treats
    // it: `fn f(x: Int) -> Int { let x = x + 1; x }` is accepted, because the `let` is in
    // a nested scope rather than the same one. Nesting here rather than relying on
    // last-wins resolution keeps the two implementations agreeing for the same reason
    // rather than by coincidence.
    let body = f.body.as_ref()?;
    let block = ctx.scoped(|ctx| lower_block(ctx, body))?;
    if block.result.ty() != ret {
        // Unreachable after checking, which requires the body to produce the declared
        // type. Reported rather than assumed away.
        ctx.unsupported(
            body.span,
            &format!(
                "a `{}` whose body produces `{}` but is declared `{}`",
                f.name,
                block.result.ty().name(),
                ret.name()
            ),
        );
        return None;
    }

    Some(Function {
        id,
        name: f.name.clone(),
        locals: std::mem::take(&mut ctx.locals),
        params,
        ret,
        body: block,
    })
}

/// Refuse any function that can reach itself.
///
/// # Every declared function, not only the reachable ones
///
/// A cycle between two functions `main` never calls is still refused. Checking only what
/// is reachable would introduce a dead-code reachability rule as a side effect of a
/// backend limitation — so deleting a call somewhere else would silently change whether a
/// function is diagnosed. That is a language-visible rule arriving by accident, and this
/// increment does not make one. See `docs/roadmap.md`, *M1c subset*.
///
/// Direct and mutual recursion are the same condition here, found the same way, rather
/// than one being a special case of the other.
fn report_cycles(ctx: &mut Ctx, functions: &[Function]) {
    let n = ctx.calls.len();
    // Iterative depth-first search with an explicit stack: the graph is as deep as the
    // program has functions, and a recursive walk over it would be this compiler doing
    // the very thing it is about to refuse to compile.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    let mut mark = vec![Mark::New; n];
    let mut reported = vec![false; n];

    for root in 0..n {
        if mark.get(root).copied() != Some(Mark::New) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        while let Some(&mut (node, ref mut next)) = stack.last_mut().map(|e| &mut *e) {
            if *next == 0 {
                if let Some(m) = mark.get_mut(node) {
                    *m = Mark::Open;
                }
            }
            let edge = ctx.calls.get(node).and_then(|e| e.get(*next)).copied();
            match edge {
                Some((FuncId(to), span)) => {
                    *next += 1;
                    match mark.get(to).copied() {
                        // Back edge: `node` can reach itself through `to`.
                        Some(Mark::Open) => {
                            if !reported.get(to).copied().unwrap_or(true) {
                                if let Some(r) = reported.get_mut(to) {
                                    *r = true;
                                }
                                let name = functions
                                    .iter()
                                    .find(|f| f.id == FuncId(to))
                                    .map_or("this function", |f| f.name.as_str());
                                let what = if to == node {
                                    format!("a call to `{name}` from inside itself")
                                } else {
                                    format!("a call cycle through `{name}`")
                                };
                                ctx.errors.push(
                                    Diagnostic::error(
                                        codes::UNSUPPORTED_CONSTRUCT,
                                        span,
                                        format!("{what} is not in `nazm build`'s subset yet"),
                                    )
                                    .primary_label("recursion")
                                    .help(
                                        "the interpreter runs recursive programs with a \
                                         bounded call depth; this backend has no plan for \
                                         the stack a recursive call needs, so it refuses \
                                         rather than emitting one it cannot bound. \
                                         `nazm run` still works",
                                    ),
                                );
                            }
                        }
                        Some(Mark::New) => stack.push((to, 0)),
                        _ => {}
                    }
                }
                None => {
                    if let Some(m) = mark.get_mut(node) {
                        *m = Mark::Done;
                    }
                    stack.pop();
                }
            }
        }
    }
}

/// A block used as a value: statements, then a tail.
///
/// Reached for `main`'s body and for an `if` branch, and for nothing else. A bare
/// `{ … }` in expression position is refused in [`lower_expr`] rather than routed here,
/// so "which blocks may this backend compile" has one answer written in one place.
fn lower_block(ctx: &mut Ctx, b: &AstBlock) -> Option<Block> {
    let mut stmts = Vec::new();
    for s in &b.statements {
        match s {
            AstStmt::Let(l) if !l.mutable => {
                // The initialiser is lowered *before* the name is bound, so `let x = x;`
                // reads the outer `x` exactly as the checker says it does.
                let value = lower_expr(ctx, &l.value);
                let ty = value.as_ref().map_or(Ty::Int, Expr::ty);
                let slot = ctx.bind(&l.name, ty);
                if let Some(value) = value {
                    stmts.push(Stmt::Let { slot, value });
                }
            }
            AstStmt::Let(l) => {
                ctx.unsupported(l.span, "`let mut`");
                // Bound anyway. The binding is real in the source, and leaving it out
                // would make every later mention of the name report a second error about
                // a name that is perfectly well defined — one mistake, several errors,
                // which is the thing this front end is built to avoid.
                let value = lower_expr(ctx, &l.value);
                let ty = value.as_ref().map_or(Ty::Int, Expr::ty);
                ctx.bind(&l.name, ty);
            }
            AstStmt::Assign(a) => ctx.unsupported(a.span, "assignment"),
            AstStmt::While(w) => ctx.unsupported(w.span, "`while`"),
            AstStmt::Return(r) => ctx.unsupported(r.span, "`return`"),
            AstStmt::Break { span } => ctx.unsupported(*span, "`break`"),
            AstStmt::Continue { span } => ctx.unsupported(*span, "`continue`"),
            // Where a statement-position `if` lands, and it stays refused.
            AstStmt::Expr(e) => ctx.unsupported(e.span(), "an expression statement"),
        }
    }

    let Some(tail) = &b.tail else {
        ctx.unsupported(b.span, "a block with no final result expression");
        return None;
    };

    let result = lower_expr(ctx, tail)?;
    Some(Block {
        stmts,
        result: Box::new(result),
    })
}

fn lower_expr(ctx: &mut Ctx, e: &AstExpr) -> Option<Expr> {
    match e {
        AstExpr::Int { value, .. } => Some(Expr::Int(*value)),
        AstExpr::Bool { value, .. } => Some(Expr::Bool(*value)),
        AstExpr::Str { span, .. } => {
            ctx.unsupported(*span, "a `Str` value");
            None
        }
        AstExpr::Var { name, span } => match ctx.resolve(name) {
            Some((slot, ty)) => Some(Expr::Load { slot, ty }),
            None => {
                // Checking already rejects a genuinely undefined name, so reaching here
                // means the resolver above disagrees with the checker — worth reporting
                // as itself rather than as "unsupported".
                ctx.errors.push(
                    Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not a local the native backend can see"),
                    )
                    .help("this is a bug in the compiler; `nazm run` still works"),
                );
                None
            }
        },
        AstExpr::Neg {
            operand, op_span, ..
        } => {
            let operand = lower_expr(ctx, operand)?;
            if operand.ty() != Ty::Int {
                // Unreachable after checking, which rejects `-true`. Reported rather than
                // assumed away: the emitter has no `i1` negation to fall back on.
                ctx.unsupported(*op_span, "negation of a non-`Int` value");
                return None;
            }
            Some(Expr::Neg {
                operand: Box::new(operand),
                span: *op_span,
            })
        }
        AstExpr::Binary {
            op,
            lhs,
            rhs,
            op_span,
            ..
        } => lower_binary(ctx, *op, lhs, rhs, *op_span),
        AstExpr::Call {
            callee,
            callee_span,
            args,
            ..
        } => lower_call(ctx, callee, *callee_span, args),
        AstExpr::If {
            cond,
            then_block,
            else_block,
            span,
        } => lower_if(ctx, cond, then_block, else_block.as_ref(), *span),
        // Only an `if` branch may be a block. A bare one is a different construct with a
        // different scoping question, and this increment does not answer it.
        AstExpr::Block(b) => {
            ctx.unsupported(b.span, "a block used as an expression");
            None
        }
    }
}

fn lower_binary(
    ctx: &mut Ctx,
    op: AstOp,
    lhs: &AstExpr,
    rhs: &AstExpr,
    op_span: Span,
) -> Option<Expr> {
    // Left, then right, always — including when the operator itself turns out to be
    // unsupported, so a program with problems on both sides reports both.
    let lhs = lower_expr(ctx, lhs);
    let rhs = lower_expr(ctx, rhs);
    let (lhs, rhs) = (lhs?, rhs?);

    if let Some(op) = arithmetic(op) {
        if lhs.ty() != Ty::Int || rhs.ty() != Ty::Int {
            // Unreachable after checking: arithmetic is `Int` only.
            ctx.unsupported(op_span, "arithmetic on a non-`Int` value");
            return None;
        }
        return Some(Expr::Arith {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: op_span,
        });
    }

    let cmp = comparison(op)?;
    if lhs.ty() != rhs.ty() {
        // Unreachable after checking, which requires both sides of `==` to be one type.
        ctx.unsupported(op_span, "a comparison of two different types");
        return None;
    }
    if cmp.is_ordering() && lhs.ty() != Ty::Int {
        ctx.unsupported(op_span, &format!("ordering `{}` values", lhs.ty().name()));
        return None;
    }
    Some(Expr::Compare {
        op: cmp,
        operand_ty: lhs.ty(),
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

/// A direct call to a named function.
///
/// The name is resolved against the signature table — the same namespace the checker
/// uses, which is separate from local bindings there. So a local called `f` does not
/// shadow a function `f`, exactly as `nazm run` behaves. No new shadowing rule is
/// invented here, and none should be.
fn lower_call(ctx: &mut Ctx, callee: &str, callee_span: Span, args: &[AstExpr]) -> Option<Expr> {
    // Arguments first, left to right, and **all of them** even when the callee turns out
    // to be unusable — a program with problems in two arguments should report two.
    let lowered: Vec<Option<Expr>> = args.iter().map(|a| lower_expr(ctx, a)).collect();

    let Some(Sig { id, ret, params }) = ctx.sigs.get(callee) else {
        // The checker reports an unknown function (`N0201`) and a mismatched signature
        // (`N0303`, `N0300`) already. Reaching here means the callee exists in the source
        // but this backend refused its signature — a different fact, and worth saying so
        // rather than repeating the checker.
        ctx.unsupported(
            callee_span,
            &format!("a call to `{callee}`, whose signature this backend refused"),
        );
        return None;
    };

    let (id, ret, params) = (*id, *ret, params.clone());

    // Recorded whether or not the arguments lowered: the cycle check is about the call
    // graph, and an edge exists as soon as the source names one.
    if let Some(edges) = ctx.calls.get_mut(ctx.current.0) {
        edges.push((id, callee_span));
    }

    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;

    // The checker has already established arity and argument types (`N0303`, `N0300`),
    // so this agreeing is the normal case. It is checked anyway because the one thing it
    // would catch is a *lowering* bug — an argument that acquired the wrong type on the
    // way here — and that is a miscompile rather than a diagnostic. LLVM would reject
    // the call too, but "clang rejected our IR" is a much worse way to find out.
    if args.len() != params.len() || args.iter().zip(&params).any(|(a, want)| a.ty() != *want) {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                callee_span,
                format!("`nazm build` lowered a call to `{callee}` with the wrong arguments"),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    }

    Some(Expr::Call {
        callee: id,
        args,
        ret,
    })
}

/// Value-position `if`, and only that.
fn lower_if(
    ctx: &mut Ctx,
    cond: &AstExpr,
    then_block: &AstBlock,
    else_block: Option<&AstBlock>,
    span: Span,
) -> Option<Expr> {
    // The condition is lowered first, and outside both branch scopes: it is evaluated
    // before either branch and cannot see a binding made inside one.
    let cond = lower_expr(ctx, cond);

    let Some(else_block) = else_block else {
        // In value position the checker rejects this already; in statement position it
        // never reaches here. Refused rather than assumed unreachable.
        ctx.unsupported(span, "an `if` with no `else` used as a value");
        return None;
    };

    // Each branch gets its own scope, so a binding made in one is invisible in the other
    // and after the join. Both are lowered even when the condition failed, so all the
    // problems in the program are reported at once.
    let then_lowered = ctx.scoped(|ctx| lower_block(ctx, then_block));
    let else_lowered = ctx.scoped(|ctx| lower_block(ctx, else_block));

    let cond = cond?;
    if cond.ty() != Ty::Bool {
        // Unreachable after checking, which requires a `Bool` condition — there is no
        // truthiness in this language.
        ctx.unsupported(span, "an `if` whose condition is not a `Bool`");
        return None;
    }
    let (then_lowered, else_lowered) = (then_lowered?, else_lowered?);

    let ty = then_lowered.result.ty();
    if ty != else_lowered.result.ty() {
        // Also unreachable after checking, which requires the branches to agree unless
        // one diverges — and every diverging construct is refused above.
        ctx.unsupported(span, "an `if` whose branches produce different types");
        return None;
    }

    Some(Expr::If {
        cond: Box::new(cond),
        then_block: Box::new(then_lowered),
        else_block: Box::new(else_lowered),
        ty,
    })
}

fn arithmetic(op: AstOp) -> Option<ArithOp> {
    match op {
        AstOp::Add => Some(ArithOp::Add),
        AstOp::Sub => Some(ArithOp::Sub),
        AstOp::Mul => Some(ArithOp::Mul),
        AstOp::Div => Some(ArithOp::Div),
        AstOp::Rem => Some(ArithOp::Rem),
        AstOp::Eq | AstOp::NotEq | AstOp::Lt | AstOp::LtEq | AstOp::Gt | AstOp::GtEq => None,
    }
}

fn comparison(op: AstOp) -> Option<CmpOp> {
    match op {
        AstOp::Eq => Some(CmpOp::Eq),
        AstOp::NotEq => Some(CmpOp::NotEq),
        AstOp::Lt => Some(CmpOp::Lt),
        AstOp::LtEq => Some(CmpOp::LtEq),
        AstOp::Gt => Some(CmpOp::Gt),
        AstOp::GtEq => Some(CmpOp::GtEq),
        AstOp::Add | AstOp::Sub | AstOp::Mul | AstOp::Div | AstOp::Rem => None,
    }
}
