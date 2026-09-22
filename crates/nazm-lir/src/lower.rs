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
//! # The one `if`, in two positions
//!
//! `docs/spec.md` has a single `if` whose meaning is decided by checking, not by syntax,
//! and this file does not add a second. The same `AstExpr::If` becomes `Stmt::If` when it
//! is found where no value is wanted and `Expr::If` when it is found where one is — which
//! is the position, not the syntax, exactly as checking decides it. There are three such
//! places and they are named, not discovered: a statement (`AstStmt::Expr`), the tail of
//! a body that wants no value ([`lower_body`]), and anywhere an expression is lowered
//! ([`lower_expr`]).
//!
//! What differs between the two is **completion**, and that is represented rather than
//! implied. A statement `if` without `else` always falls through; one whose branches both
//! transfer never does; a value `if` with one transferring branch takes its value from
//! the other. [`Ends`] records which, for statement sequences, and `Option` on
//! [`Block::result`] and on `Expr::If`'s type records it for values.
//!
//! # Blocks that produce a value, and blocks that do not
//!
//! A function body and a value-position `if` branch produce a value; a loop body and a
//! statement-`if` branch do not. Those are different obligations, so they are different
//! functions here — [`lower_block`] returns a [`Block`] and [`lower_body`] returns a
//! `Vec<Stmt>` — over a shared [`lower_stmts`] that has no opinion about either.
//!
//! # Where `break` and `continue` may appear
//!
//! Only inside a loop **body**. A loop's *condition* is outside that loop: a transfer
//! raised there targets the enclosing one. That is the existing language's rule, taken
//! from `check.rs` (which raises `loop_depth` around the body alone) and from `eval.rs`
//! (whose `take!` lets a `Flow::Break` from the condition leave `while_loop`), not
//! invented here. [`Ctx::loop_depth`] reproduces it so that a `break` this backend cannot
//! place is refused rather than emitted somewhere plausible.

use nazm_diag::{codes, Diagnostic};
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_span::Span;
use nazm_syntax::{
    BinOp as AstOp, Block as AstBlock, Expr as AstExpr, Function as AstFn, Program,
    Spawn as AstSpawn, Stmt as AstStmt,
};

use crate::ir::{
    fits, ArithOp, Block, Builtin, CmpOp, Expr, FuncId, Function, Local, Lowering as _, Module,
    Stmt, Ty,
};

/// What the subset is, in one sentence, for every refusal to point at.
const SUBSET: &str = "`nazm build` currently compiles functions over `Int`, `Bool` and \
                      `Str` — literals, `let` and `let mut` bindings, parameters, \
                      assignment, `while` with `break` and `continue`, `return`, unary \
                      `-`, the operators `+ - * / %`, the comparisons `== != < <= > >=`, \
                      parentheses, `if` in both statement and value position, the \
                      built-ins `str_len` `str_byte` `str_slice` `str_concat` \
                      `int_to_str`, non-recursive direct calls, and structured \
                      concurrency (`scope`, `spawn` and `Chan`) — with \
                      `fn main() -> Int` as the entry point. Everything else still runs \
                      under `nazm run`";

/// Constructs outside the native subset, with the spans that named them.
#[derive(Debug)]
pub struct LowerError {
    pub diagnostics: Vec<Diagnostic>,
}

/// What is known about a function before any body is lowered.
#[derive(Clone)]
struct Sig {
    id: FuncId,
    params: Vec<Ty>,
    ret: Ty,
}

struct Ctx<'a> {
    /// What the checker resolved. **Read, never recomputed.**
    ///
    /// This crate used to carry its own scope stack, its own slot allocator and its own
    /// name-keyed signature table, because `xtask/src/rules.rs` forbids depending on
    /// `nazm-core` and the resolution was thrown away before it could be handed over. Two
    /// implementations of one rule, and the comments here said as much — the scope stack's
    /// doc noted that "reproducing that is the point; inventing a shadowing rule here
    /// would make the two disagree". They cannot disagree now: there is one.
    res: &'a Resolution,
    /// The backend's own numbering, by `FnId`. `None` where this backend refused a
    /// signature — which is what tells a refused user function apart from a built-in it
    /// does not lower, since both otherwise reach the same arm and deserve different
    /// sentences.
    sig_of: Vec<Option<Sig>>,
    /// Locals of the function being lowered, in slot order, taken from the resolution.
    locals: Vec<Local>,
    /// Which functions each function calls, for the cycle check.
    calls: Vec<Vec<(FuncId, Span)>>,
    /// The function whose body is being lowered, for recording edges.
    current: FuncId,
    /// The declared return type of the function being lowered, for checking `return`.
    /// Reset per function, like everything else about a function's body.
    ret_ty: Ty,
    /// How many loop **bodies** enclose the statement being lowered.
    ///
    /// Mirrors the checker's field of the same name, including where it is raised: around
    /// a loop's body and not around its condition. Reset per function, because a loop
    /// target cannot cross a function boundary.
    loop_depth: u32,
    errors: Vec<Diagnostic>,
}

/// How a statement sequence finishes. The checker's `Completion`, minus the value case
/// that a statement sequence cannot have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ends {
    /// Reaches the end; whatever follows can run.
    FallsThrough,
    /// Every path leaves through `break` or `continue`. Nothing after it can run, and the
    /// checker has already said so (`N0313`).
    Transfers,
}

impl Ctx<'_> {
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

    /// Which slot the name written at `span` refers to, and its type.
    ///
    /// A lookup, not a search. Scope rules — shadowing, a branch's bindings dying at the
    /// join, a local not shadowing a function — were applied once, by the checker, and
    /// this reads the answer.
    fn slot_at(&self, span: Span) -> Option<(usize, Ty)> {
        let slot = self.res.var(span)? as usize;
        self.locals.get(slot).map(|l| (slot, l.ty))
    }

    /// The backend's signature for a `FnId`, if it accepted one.
    fn sig(&self, id: FnId) -> Option<&Sig> {
        self.sig_of.get(id.index()).and_then(Option::as_ref)
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
pub fn lower(program: &Program, res: &Resolution) -> Result<Module, LowerError> {
    let mut ctx = Ctx {
        res,
        sig_of: vec![None; res.functions().len()],
        locals: Vec::new(),
        calls: Vec::new(),
        current: FuncId(0),
        ret_ty: Ty::Int,
        loop_depth: 0,
        errors: Vec::new(),
    };

    // Pass 1: which of the resolved functions this backend can represent.
    //
    // It no longer decides *which functions exist* or *what a name means* — the
    // resolution did — only whether each one's types are in the native subset. Duplicates
    // never reach here: the checker reports `N0203` and declares the first, so the
    // "first of a repeated name wins" rule this loop used to implement is upstream now.
    //
    // Driven by the **resolution**, not by the syntax tree: each definition says which
    // declaration it came from, so pairing the two needs no name. The last two name
    // lookups in this crate were here, and a name stopped being able to do the job when
    // two modules became able to define the same one.
    let mut order: Vec<(FnId, &AstFn)> = Vec::new();
    for (i, def) in res.functions().iter().enumerate() {
        let id = FnId::from_index(i);
        let Some(f) = program.functions.get(def.decl as usize) else {
            continue;
        };
        let Some(sig) = signature(&mut ctx, id, f) else {
            continue;
        };
        if let Some(slot) = ctx.sig_of.get_mut(id.index()) {
            *slot = Some(sig);
        }
        ctx.calls.push(Vec::new());
        order.push((id, f));
    }

    let entry = match res.entry().and_then(|id| ctx.sig(id)).map(|s| s.id) {
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
                    Span::synthetic(),
                    "this program has no `main` to compile",
                )],
            })
        }
    };

    // Pass 2: bodies.
    let mut functions = Vec::new();
    for (id, f) in order {
        if let Some(lowered) = lower_function(&mut ctx, id, f) {
            functions.push(lowered);
        }
    }

    // The call graph is complete only now, so the cycle check comes last.
    report_cycles(&mut ctx, &functions);

    if ctx.errors.is_empty() && functions.len() == ctx.calls.len() {
        Ok(Module { functions, entry })
    } else if !ctx.errors.is_empty() {
        Err(LowerError {
            diagnostics: ctx.errors,
        })
    } else {
        Err(LowerError {
            diagnostics: vec![Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                Span::synthetic(),
                "`nazm build` could not lower this program and did not say why",
            )
            .help("this is a bug in the compiler; `nazm run` still works")],
        })
    }
}

/// This backend's view of an already-resolved signature.
///
/// It no longer reads type *names* out of the syntax tree — `native_ty("Int")` and friends
/// were a second copy of `Type::from_name`, and a name that resolved differently in the
/// two would have produced a backend that disagreed with the checker about what a program
/// said. The types come from the resolution; this only maps them into [`Ty`] and refuses
/// what the native subset cannot represent.
fn signature(ctx: &mut Ctx<'_>, id: FnId, f: &AstFn) -> Option<Sig> {
    let def = ctx.res.function(id)?;
    let params: Vec<Ty> = def.params.iter().map(|t| Ty::of(*t)).collect();
    let ret = Ty::of(def.ret);
    let mut ok = true;

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
    // The backend's own numbering, dense over the functions it accepted, because a
    // `FuncId` becomes a symbol in the emitted module.
    let native = FuncId(ctx.calls.len());
    ok.then_some(Sig {
        id: native,
        params,
        ret,
    })
}

/// Lower one function body.
///
/// # The slot space arrives, it is not built
///
/// This used to reset a scope stack, walk the parameters binding each into a fresh slot,
/// and nest the body inside the parameter scope — reproducing, carefully and separately,
/// the rules the checker had already applied. The comment on the nesting said why:
/// "keeps the two implementations agreeing for the same reason rather than by
/// coincidence". There is one implementation now. The slots, their order and their types
/// come from the resolution, **parameters first**, and nothing here can disagree with the
/// checker about which binding a name means.
fn lower_function(ctx: &mut Ctx<'_>, id: FnId, f: &AstFn) -> Option<Function> {
    let Sig {
        id: native, ret, ..
    } = ctx.sig(id)?.clone();
    let def = ctx.res.function(id)?;
    ctx.locals = def
        .locals
        .iter()
        .map(|l| Local {
            name: l.name.clone(),
            ty: Ty::of(l.ty),
        })
        .collect();
    ctx.current = native;
    ctx.ret_ty = ret;
    // No loop target crosses a function boundary, so the depth starts at zero here and
    // not wherever the previous function left it.
    ctx.loop_depth = 0;

    let params = f.params.len();
    let id = native;
    let body = f.body.as_ref()?;
    let block = lower_block(ctx, body)?;
    // No tail is legitimate now: a body every path of which returns has nothing to fall
    // off the end with, and `check_function` accepts exactly that by treating
    // `Completion::Diverges` as satisfying the signature. Emission writes no terminator
    // for such a body, because the `ret` is already there.
    let Some(result) = block.result.as_ref() else {
        return Some(Function {
            id,
            name: f.name.clone(),
            locals: std::mem::take(&mut ctx.locals),
            params,
            ret,
            body: block,
        });
    };
    if !fits(result, ret) {
        // Unreachable after checking, which requires the body to produce the declared
        // type. Reported rather than assumed away.
        ctx.unsupported(
            body.span,
            &format!(
                "a `{}` whose body produces `{}` but is declared `{}`",
                f.name,
                result.ty().map_or("nothing", Ty::name),
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
/// increment does not make one; see `docs/master-architecture.md` §2, *a refusal is
/// never disguised as a result*.
///
/// Direct and mutual recursion are the same condition here, found the same way, rather
/// than one being a special case of the other.
fn report_cycles(ctx: &mut Ctx<'_>, functions: &[Function]) {
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
/// Reached for a function body and for a value-position `if` branch, and for nothing
/// else. A bare `{ … }` in expression position is refused in [`lower_expr`], and a block
/// that wants no value goes to [`lower_body`] — so "which blocks may this backend
/// compile" has one answer written in one place.
fn lower_block(ctx: &mut Ctx<'_>, b: &AstBlock) -> Option<Block> {
    let (stmts, ends) = lower_stmts(ctx, &b.statements);

    if ends == Ends::Transfers {
        // Every path through the statements leaves, so the block has no value. There is
        // no tail to lower either: one written after a transfer is `N0313`, which the
        // checker reports before lowering runs.
        return Some(Block {
            stmts,
            result: None,
        });
    }

    let Some(tail) = &b.tail else {
        ctx.unsupported(b.span, "a block with no final result expression");
        return None;
    };

    let result = lower_expr(ctx, tail)?;
    Some(Block {
        stmts,
        result: Some(Box::new(result)),
    })
}

/// A body that wants no value: a loop body, or a branch of a statement-position `if`.
///
/// The caller decides the scope. A loop body and an `if` branch are both scopes in the
/// language, and both callers wrap this in [`Ctx::scoped`]; doing it here instead would
/// hide which construct owns the scope.
///
/// # The tail
///
/// A block in this position may still have a tail, and what it means depends on what it
/// is. An `if` is a **statement-position `if`** — `while c { … if d { x = 1; } }` parses
/// the trailing `if` as the body's tail, and `check_while` accepts it because its
/// completion is Unit. Anything else is a value the language discards: `check_expr` for
/// an `if` with no `else` returns Unit whatever its branch produced, so `if c { 1 }`
/// evaluates the `1` and drops it, and `nazm run` does exactly that.
fn lower_body(ctx: &mut Ctx<'_>, b: &AstBlock) -> (Vec<Stmt>, Ends) {
    let (mut stmts, ends) = lower_stmts(ctx, &b.statements);

    if let Some(tail) = &b.tail {
        if ends == Ends::Transfers {
            // Unreachable: a tail after a transfer is `N0313`. Nothing is lowered for it,
            // which is also what emission would do with it.
            return (stmts, ends);
        }
        if let AstExpr::If {
            cond,
            then_block,
            else_block,
            span,
        } = tail.as_ref()
        {
            let (stmt, tail_ends) =
                lower_statement_if(ctx, cond, then_block, else_block.as_ref(), *span);
            stmts.extend(stmt);
            return (stmts, tail_ends);
        }
        if let Some(value) = lower_expr(ctx, tail) {
            stmts.push(Stmt::Discard(value));
        }
    }

    (stmts, ends)
}

/// The statements every kind of block shares.
///
/// A statement that cannot be lowered records a diagnostic and contributes nothing, which
/// is safe because [`lower`] returns `Err` whenever any diagnostic was recorded — no
/// module is ever built out of a partially lowered body.
fn lower_stmts(ctx: &mut Ctx<'_>, statements: &[AstStmt]) -> (Vec<Stmt>, Ends) {
    let mut stmts = Vec::new();
    for s in statements {
        match s {
            // `let` and `let mut` lower identically. Mutability is a *checking* property:
            // it decides whether an assignment to this binding is legal, and the checker
            // has already decided that (`N0310`). Encoding it again here would be a
            // second mutability rule, and two rules are two chances to disagree.
            AstStmt::Let(l) => {
                // The initialiser is lowered *before* the name is bound, so `let x = x;`
                // reads the outer `x` exactly as the checker says it does.
                let value = lower_expr(ctx, &l.value);
                // A binding whose initialiser always transfers is typed `Int`, which is
                // not a guess: `check_stmt` asks `value_type(&l.value, None)`, and that
                // returns `expected.unwrap_or(Int)` for a `Completion::Diverges`. The
                // binding is never written, so the choice is observable only in whether
                // the two implementations agree about the program being legal.
                let _ = value.as_ref().and_then(Expr::ty).unwrap_or(Ty::Int);
                // The slot this `let` introduces, as resolution assigned it. The backend
                // used to allocate one here from its own counter.
                let Some((slot, _)) = ctx.slot_at(l.name_span) else {
                    ctx.errors.push(
                        Diagnostic::error(
                            codes::UNDEFINED_VARIABLE,
                            l.name_span,
                            format!("`{}` has no resolved binding", l.name),
                        )
                        .help("this is a compiler bug: checking assigned no slot here"),
                    );
                    continue;
                };
                if let Some(value) = value {
                    stmts.push(Stmt::Let { slot, value });
                }
            }
            AstStmt::Assign(a) => stmts.extend(lower_assign(ctx, a)),
            AstStmt::While(w) => stmts.extend(lower_while(ctx, w)),
            AstStmt::Scope(sc) => {
                // Its own scope in the language too: a binding made inside `scope { … }`
                // does not outlive it, exactly as in any other block.
                let (body, ends) = lower_body(ctx, &sc.body);
                stmts.push(Stmt::Scope {
                    body,
                    span: sc.span,
                });
                // A `scope` whose body always transfers transfers too — after the join,
                // which emission is what arranges. Nothing after it can run.
                if ends == Ends::Transfers {
                    return (stmts, Ends::Transfers);
                }
            }
            AstStmt::Spawn(sp) => {
                if let Some(stmt) = lower_spawn(ctx, sp) {
                    stmts.push(stmt);
                }
            }
            AstStmt::Return(r) => {
                if let Some(stmt) = lower_return(ctx, r) {
                    stmts.push(stmt);
                }
                // A `return` leaves the function whether or not its operand lowered, so
                // the rest of the block is unreachable either way — and the checker has
                // already reported anything written after it (`N0313`).
                return (stmts, Ends::Transfers);
            }
            AstStmt::Break { span } => {
                if loop_target(ctx, "break", *span) {
                    stmts.push(Stmt::Break);
                    return (stmts, Ends::Transfers);
                }
            }
            AstStmt::Continue { span } => {
                if loop_target(ctx, "continue", *span) {
                    stmts.push(Stmt::Continue);
                    return (stmts, Ends::Transfers);
                }
            }
            // A statement-position `if`: the same `if` as everywhere else, found where
            // no value is wanted.
            AstStmt::Expr(AstExpr::If {
                cond,
                then_block,
                else_block,
                span,
            }) => {
                let (stmt, ends) =
                    lower_statement_if(ctx, cond, then_block, else_block.as_ref(), *span);
                stmts.extend(stmt);
                if ends == Ends::Transfers {
                    return (stmts, ends);
                }
            }
            // Every other expression statement: evaluated, and its value dropped —
            // which is what `nazm run` does with it, and what the language means by
            // putting a `;` after an expression. A sequence operation is written this
            // way (`ints_push(a, 1);`), and having to bind a result nobody wants would
            // be a workaround for the language's lack of a unit type rather than a
            // rule worth having.
            //
            // A bare block is still refused, because `lower_expr` refuses
            // `AstExpr::Block` — the refusal lives with the construct rather than with
            // the position it was found in.
            AstStmt::Expr(e) => {
                if let Some(value) = lower_expr(ctx, e) {
                    stmts.push(Stmt::Discard(value));
                }
            }
        }
    }
    (stmts, Ends::FallsThrough)
}

/// `return e;` — leave the function with `e`.
///
/// No loop target is consulted: a `return` inside a loop leaves the *function*, which is
/// what both existing implementations do (`eval.rs` lets `Flow::Return` travel out
/// through `while_loop`) and why this needs nothing from the target stack.
fn lower_return(ctx: &mut Ctx<'_>, r: &nazm_syntax::Return) -> Option<Stmt> {
    let value = lower_expr(ctx, &r.value)?;
    if !fits(&value, ctx.ret_ty) {
        // Unreachable after checking, which requires a `return` to match the signature
        // (`N0300`). Reported rather than assumed away: a `ret` of the wrong type is a
        // module clang rejects for a reason that names none of this.
        ctx.unsupported(
            r.span,
            &format!(
                "a `return` of `{}` from a function declared `{}`",
                value.ty().map_or("nothing", Ty::name),
                ctx.ret_ty.name()
            ),
        );
        return None;
    }
    Some(Stmt::Return(value))
}

/// Whether a `break` or `continue` has a loop to affect.
///
/// The checker reports `N0312` for one that does not, so a `false` here means this
/// resolver disagrees with the checker — a compiler bug, reported as one. Emission relies
/// on it: a `Stmt::Break` reaches the emitter only from inside a `Stmt::While` body, so
/// the emitter's target stack is never empty when it reads one.
fn loop_target(ctx: &mut Ctx<'_>, word: &str, span: Span) -> bool {
    if ctx.loop_depth > 0 {
        return true;
    }
    ctx.errors.push(
        Diagnostic::error(
            codes::NOT_IN_A_LOOP,
            span,
            format!("`{word}` reached the native backend with no loop to affect"),
        )
        .help("this is a bug in the compiler; `nazm run` still works"),
    );
    false
}

/// Statement-position `if`, with or without `else`.
///
/// # Completion
///
/// Without an `else` the statement **always falls through**, whatever the branch does,
/// because the condition may be false — which is exactly what `check_expr` says by
/// returning `Completion::Unit` for a no-`else` `if` regardless of its branch. With an
/// `else` it transfers only when *both* branches do.
fn lower_statement_if(
    ctx: &mut Ctx<'_>,
    cond: &AstExpr,
    then_block: &AstBlock,
    else_block: Option<&AstBlock>,
    span: Span,
) -> (Option<Stmt>, Ends) {
    // The condition is lowered outside both branch scopes: it is evaluated before either
    // and cannot see a binding made inside one.
    let cond = lower_expr(ctx, cond);

    let (then_body, then_ends) = lower_body(ctx, then_block);
    let else_lowered = else_block.map(|b| lower_body(ctx, b));

    let Some(cond) = cond else {
        return (None, Ends::FallsThrough);
    };
    if !fits(&cond, Ty::Bool) {
        // Unreachable after checking, which requires a `Bool` condition (`N0301`).
        ctx.unsupported(span, "an `if` whose condition is not a `Bool`");
        return (None, Ends::FallsThrough);
    }

    let ends = match else_lowered.as_ref().map(|(_, e)| *e) {
        Some(Ends::Transfers) if then_ends == Ends::Transfers => Ends::Transfers,
        _ => Ends::FallsThrough,
    };

    (
        Some(Stmt::If {
            cond,
            then_body,
            else_body: else_lowered.map(|(b, _)| b),
        }),
        ends,
    )
}

/// `name = value;` — a store into the slot `name` resolves to *now*.
///
/// # Which binding
///
/// Resolution is the same innermost-first walk a read uses, so an assignment reaches
/// whatever the name means at that point in the text. Inside a loop body that has
/// shadowed an outer binding, an assignment written *before* the inner `let` reaches the
/// outer slot and one written after reaches the inner — which is exactly what the
/// interpreter does, because each iteration enters the body with a fresh frame and the
/// inner name is not in it until its `let` runs again.
///
/// # Order
///
/// The value is lowered first and stored second, and the emitter keeps that order: a
/// right-hand side that fails at run time, or that transfers control, takes its own path
/// and the slot is never written. An assignment is not a place where a half-evaluated
/// expression can leave a local holding something.
fn lower_assign(ctx: &mut Ctx<'_>, a: &nazm_syntax::Assign) -> Option<Stmt> {
    let value = lower_expr(ctx, &a.value);

    let Some((slot, ty)) = ctx.slot_at(a.name_span) else {
        // Unreachable: the checker resolved this name and recorded the slot. Reaching
        // here means the resolution is missing an entry it should have, which is a
        // compiler bug and worth reporting as one rather than as "unsupported".
        ctx.errors.push(
            Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                a.name_span,
                format!("`{}` is not a local the native backend can see", a.name),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    };

    let value = value?;
    if !fits(&value, ty) {
        // Unreachable after checking, which requires an assignment to match the binding's
        // declared type (`N0300`). Reported rather than assumed away: a mismatched store
        // is a miscompile, and LLVM would reject the module for a reason that named none
        // of this.
        ctx.unsupported(
            a.span,
            &format!(
                "an assignment of `{}` to `{}`, which is `{}`",
                value.ty().map_or("nothing", Ty::name),
                a.name,
                ty.name()
            ),
        );
        return None;
    }

    Some(Stmt::Assign { slot, value })
}

/// `while cond { body }`.
///
/// The condition is lowered **outside** the body's scope and **outside its loop depth**.
/// Outside the scope because a binding made in the body is not visible to it; outside the
/// depth because a `break` written in a condition belongs to the *enclosing* loop, which
/// is the existing language's rule in both implementations. Raising the depth around the
/// condition too would silently retarget such a `break` at this loop — a miscompile of a
/// program that runs correctly today.
fn lower_while(ctx: &mut Ctx<'_>, w: &nazm_syntax::While) -> Option<Stmt> {
    let cond = lower_expr(ctx, &w.cond);

    ctx.loop_depth += 1;
    // Lowered whether or not the condition did, so a program with problems in both
    // reports both.
    let (body, _) = lower_body(ctx, &w.body);
    ctx.loop_depth -= 1;

    let cond = cond?;
    if !fits(&cond, Ty::Bool) {
        // Unreachable after checking, which requires a `Bool` condition (`N0301`) — there
        // is no truthiness in this language.
        ctx.unsupported(w.cond.span(), "a `while` whose condition is not a `Bool`");
        return None;
    }

    Some(Stmt::While { cond, body })
}

fn lower_expr(ctx: &mut Ctx<'_>, e: &AstExpr) -> Option<Expr> {
    match e {
        AstExpr::Int { value, .. } => Some(Expr::Int(*value)),
        AstExpr::Bool { value, .. } => Some(Expr::Bool(*value)),
        AstExpr::Str { value, .. } => Some(Expr::Str(value.clone().into_bytes())),
        AstExpr::Var { name, span } => match ctx.slot_at(*span) {
            Some((slot, ty)) => Some(Expr::Load { slot, ty }),
            None => {
                // Unreachable after a successful check, which records a slot for every
                // mention. Reported as a compiler bug rather than as "unsupported".
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
            if !fits(&operand, Ty::Int) {
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

/// `spawn f(args);`
///
/// Shaped like [`lower_call`] and deliberately not sharing its body: a spawn resolves a
/// **name**, not an expression, produces no value, and can never be a built-in. Folding
/// the two together would mean a call path carrying a "was this a spawn" flag through
/// every branch of it.
fn lower_spawn(ctx: &mut Ctx<'_>, sp: &AstSpawn) -> Option<Stmt> {
    // Every argument, even when the callee turns out to be unusable, so a spawn with two
    // bad arguments reports both.
    let lowered: Vec<Option<Expr>> = sp.args.iter().map(|a| lower_expr(ctx, a)).collect();

    if matches!(ctx.res.callee(sp.callee_span), Some(Callee::Builtin(_))) {
        // The checker refuses this first. Saying it here too costs one line and means a
        // lowering reached without the checker still reports rather than emitting a call
        // to a symbol that does not exist.
        ctx.unsupported(
            sp.callee_span,
            &format!(
                "`spawn {}`, which is a built-in rather than a function",
                sp.callee
            ),
        );
        return None;
    }

    // What this spawn's callee resolved to was decided once, by the checker.
    let Some(Callee::Function(fid)) = ctx.res.callee(sp.callee_span) else {
        // A built-in, or unresolved. `check_spawn` rejects both (`N0201`, and spawning a
        // built-in has no signature to run), so this is a coverage statement rather than
        // a repeat of the checker.
        ctx.unsupported(sp.callee_span, &format!("a task running `{}`", sp.callee));
        return None;
    };
    let Some(Sig { id, params, ret }) = ctx.sig(fid).cloned() else {
        ctx.unsupported(
            sp.callee_span,
            &format!(
                "a task running `{}`, whose signature this backend refused",
                sp.callee
            ),
        );
        return None;
    };

    // A task is an edge in the call graph like any other. `fn f() { scope { spawn f(); } }`
    // is recursion, and the fact that it goes through a thread does not make the stack
    // depth bounded.
    if let Some(edges) = ctx.calls.get_mut(ctx.current.0) {
        edges.push((id, sp.callee_span));
    }

    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;
    if args.len() != params.len() || !args.iter().zip(&params).all(|(a, want)| fits(a, *want)) {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                sp.callee_span,
                format!(
                    "`nazm build` lowered a task running `{}` with the wrong arguments",
                    sp.callee
                ),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    }
    // The capture rule is the checker's (`N0321`): a sequence may not cross into a task.
    // It is not re-decided here, and it is not silently relied on either — a `Ty::Ints`
    // argument would produce a task sharing unsynchronised storage, so emission refuses
    // one by name rather than lowering it.
    if let Some(bad) = args.iter().find_map(|a| match a.ty() {
        Some(t @ (Ty::Ints | Ty::Strs)) => Some(t),
        _ => None,
    }) {
        ctx.unsupported(
            sp.callee_span,
            &format!(
                "a task given a `{}`, which may not cross into one",
                bad.name()
            ),
        );
        return None;
    }

    Some(Stmt::Spawn {
        callee: id,
        args,
        ret,
        span: sp.span,
    })
}

fn lower_binary(
    ctx: &mut Ctx<'_>,
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
        if !fits(&lhs, Ty::Int) || !fits(&rhs, Ty::Int) {
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
    if let (Some(l), Some(r)) = (lhs.ty(), rhs.ty()) {
        if l != r {
            // Unreachable after checking, which requires both sides of `==` to be one
            // type. An operand that always transfers has no type to disagree with.
            ctx.unsupported(op_span, "a comparison of two different types");
            return None;
        }
    }
    // Whichever operand has a type; they agree when both do. When *neither* does — both
    // sides always transfer — no `icmp` is ever emitted, because emission stops at the
    // left operand, so the recorded type is never read. `Int` is recorded rather than
    // left unrepresentable, and this is the one place in the file where that matters.
    let operand_ty = lhs.ty().or_else(|| rhs.ty()).unwrap_or(Ty::Int);
    if cmp.is_ordering() && !fits(&lhs, Ty::Int) {
        ctx.unsupported(op_span, &format!("ordering `{}` values", operand_ty.name()));
        return None;
    }
    Some(Expr::Compare {
        op: cmp,
        operand_ty,
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
fn lower_call(
    ctx: &mut Ctx<'_>,
    callee: &str,
    callee_span: Span,
    args: &[AstExpr],
) -> Option<Expr> {
    // Arguments first, left to right, and **all of them** even when the callee turns out
    // to be unusable — a program with problems in two arguments should report two.
    let lowered: Vec<Option<Expr>> = args.iter().map(|a| lower_expr(ctx, a)).collect();

    // A built-in is not a `FuncId` and never enters the call graph, so the cycle check
    // does not see it and cannot: it is emitted as instructions, and instructions do not
    // recurse.
    // **Built-in or user function is read, not decided again.** The rule that a built-in
    // resolves first is the language's, and it lived in three places: the checker, the
    // evaluator, and here. It lives in the checker now, and this reads the answer — so a
    // program cannot mean one thing interpreted and another compiled because two copies of
    // one rule drifted.
    let target = ctx.res.callee(callee_span);
    if let Some(Callee::Builtin(op)) = target {
        return lower_builtin(ctx, op, lowered, callee_span);
    }

    // By id. The name is not consulted here and could not be: two modules may define
    // the same one, and this backend stopped being able to tell them apart by spelling on
    // 2026-09-22.
    let resolved = match target {
        Some(Callee::Function(fid)) => ctx.sig(fid).cloned(),
        _ => None,
    };
    let Some(Sig { id, ret, params }) = resolved else {
        // The checker reports an unknown function (`N0201`) and a mismatched signature
        // (`N0303`, `N0300`) already. Reaching here means the callee resolved to a
        // function whose signature *this backend* refused — a different fact, and worth
        // saying so rather than repeating the checker.
        if target.is_some() {
            ctx.unsupported(
                callee_span,
                &format!("a call to `{callee}`, whose signature this backend refused"),
            );
        } else {
            ctx.unsupported(callee_span, &format!("the built-in `{callee}`"));
        }
        return None;
    };

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
    if args.len() != params.len() || !args.iter().zip(&params).all(|(a, want)| fits(a, *want)) {
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

/// A built-in, checked against this backend's own table.
///
/// The front end has already established arity and argument types, so a disagreement
/// here is a *lowering* bug rather than a program error — and it is reported as one,
/// because a wrongly typed argument would otherwise reach clang as a malformed call.
fn lower_builtin(
    ctx: &mut Ctx<'_>,
    op: Builtin,
    lowered: Vec<Option<Expr>>,
    span: Span,
) -> Option<Expr> {
    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;
    let params = op.lowered_params();
    if args.len() != params.len() || !args.iter().zip(params).all(|(a, want)| fits(a, want)) {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                span,
                format!(
                    "`nazm build` lowered `{}` with the wrong arguments",
                    op.name()
                ),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    }
    Some(Expr::Builtin { op, args, span })
}

/// Value-position `if`, and only that.
fn lower_if(
    ctx: &mut Ctx<'_>,
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
    let then_lowered = lower_block(ctx, then_block);
    let else_lowered = lower_block(ctx, else_block);

    let cond = cond?;
    if !fits(&cond, Ty::Bool) {
        // Unreachable after checking, which requires a `Bool` condition — there is no
        // truthiness in this language.
        ctx.unsupported(span, "an `if` whose condition is not a `Bool`");
        return None;
    }
    let (then_lowered, else_lowered) = (then_lowered?, else_lowered?);

    // A branch that transfers contributes no type, and the checker says the same:
    // `(Diverges, other) => other`. So the `if`'s type is whichever branch has one, and
    // `None` — both branches transfer — means the expression never produces a value at
    // all. Nothing downstream is allowed to invent one for it.
    let (then_ty, else_ty) = (block_ty(&then_lowered), block_ty(&else_lowered));
    if let (Some(a), Some(b)) = (then_ty, else_ty) {
        if a != b {
            // Also unreachable after checking, which requires the branches to agree
            // unless one of them leaves.
            ctx.unsupported(span, "an `if` whose branches produce different types");
            return None;
        }
    }

    Some(Expr::If {
        cond: Box::new(cond),
        then_block: Box::new(then_lowered),
        else_block: Box::new(else_lowered),
        ty: then_ty.or(else_ty),
    })
}

/// The type a value block produces, or `None` when it never produces one.
fn block_ty(b: &Block) -> Option<Ty> {
    b.result.as_ref().and_then(|e| e.ty())
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
