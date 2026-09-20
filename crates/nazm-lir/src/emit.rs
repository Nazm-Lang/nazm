//! [`crate::ir`] to LLVM IR text.
//!
//! # The contract this file exists to keep
//!
//! `docs/spec.md` says overflow **traps, identically in every build**. The obvious way to
//! write signed arithmetic in LLVM is `add nsw`, and it is exactly wrong: `nsw` does not
//! check for overflow, it *promises overflow cannot happen* and makes the result poison
//! if it does. An optimiser is then entitled to assume the promise and delete the branch
//! that was meant to catch it — so `nsw` would turn a specified trap into whatever
//! `-O2` felt like. Every fallible operation here goes through
//! `llvm.{sadd,ssub,smul}.with.overflow.i64` and an explicit branch, which is a value the
//! optimiser has to respect.
//!
//! `llvm.trap` is not used either. It raises `SIGILL`, and a process killed by a signal
//! is not the runtime-error contract Nazm promises: the spec says the program *stops with
//! a diagnostic*. So a failing operation calls a generated helper that writes the
//! diagnostic to standard error and exits 2 — the same status `nazm run` uses for a
//! runtime failure.
//!
//! The four cases that are easy to get wrong, and what is emitted for each:
//!
//! | Case | Emitted |
//! | --- | --- |
//! | `a / 0`, `a % 0` | divisor compared to zero *before* any division instruction |
//! | `Int::MIN / -1` | both operands compared, and the failure reported as an overflow — the quotient has no representation |
//! | `Int::MIN % -1` | **`0`**, via a `phi`. Never reaches `srem`, which faults in hardware on exactly this pair |
//! | `-Int::MIN` | `ssub.with.overflow(0, x)`, the only negation without a representable result |
//!
//! `sdiv` and `srem` are reached only on operands already proved safe, so the guards are
//! not belt-and-braces — they are what stops the hardware fault that `docs/spec.md`
//! describes.
//!
//! # Blocks, and why the current one is tracked
//!
//! Emission is linear: every instruction is appended, and opening a label continues into
//! it. That works for straight-line code and for the guards above, and it would quietly
//! produce *wrong* `phi` nodes for `if` without [`Emitter::current`].
//!
//! A `phi` names the block each incoming value came from. The naive answer is the label
//! the branch started at — and that is wrong whenever the branch emitted any block of its
//! own, which happens for a nested `if`, and for any checked arithmetic, since a guard
//! opens a continuation block. The predecessor is the block holding the terminator that
//! jumps to the join, which is whatever block emission *ended* in. So [`Emitter::open`]
//! records it and the `phi` reads it back after each branch.

use std::fmt::Write as _;

use crate::ir::{ArithOp, Block, Expr, Function, Module, Stmt, Ty};

/// What this emitter has been verified against, recorded so a reader does not have to
/// infer portability from silence.
pub const TARGET_NOTE: &str = "verified on aarch64-apple-darwin with Apple clang; other \
                               targets are unverified rather than unsupported";

/// The diagnostic a failing operation reports, matching the interpreter's wording.
struct Failure {
    code: &'static str,
    message: String,
}

struct Emitter {
    out: String,
    /// Next SSA value number. LLVM requires unnamed values to be numbered consecutively
    /// within a function, so this also numbers blocks to keep them distinct.
    next: u32,
    /// The block instructions are currently being appended to — the predecessor a `phi`
    /// must name. See the module documentation.
    current: String,
    /// Error strings, interned so a program with the same failure twice carries it once.
    strings: Vec<(String, String)>,
    /// Where the source came from, for the runtime message.
    file: String,
    /// Byte offset of the start of each line, for turning a span into a position.
    lines: Vec<usize>,
    source_len: usize,
    /// Active loops, innermost last. Pushed around a loop's **body** only — a `break` in
    /// a loop's *condition* belongs to the enclosing loop, which is the existing
    /// language's rule (`docs/roadmap.md`, *Where a loop target binds*). Cleared at every
    /// function boundary, so no target can leak from one function into another.
    loops: Vec<Targets>,
}

/// Where `break` and `continue` go for one loop.
///
/// `continue` targets the **condition**, never the body: re-evaluating the condition is
/// what `continue` means, and jumping to the body would skip it — including any call or
/// checked arithmetic it contains.
struct Targets {
    head: String,
    exit: String,
}

/// What emitting an expression produced.
///
/// The checker's `Completion`, as emission sees it. The type travels with the value
/// because every producer knows it locally, and deriving it later from the node would be
/// a second answer to a question already answered.
#[derive(Debug, Clone, Copy)]
enum Val {
    Of {
        value: u32,
        ty: Ty,
    },
    /// Control left along this path. There is no value, the current block already holds
    /// its terminator, and **nothing may be invented for the caller to consume**.
    Transferred,
}

/// Whether emission may keep appending to the current block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    FallsThrough,
    /// A terminator has been written. Nothing may follow it, and this path is not a
    /// predecessor of any join.
    Transferred,
}

/// One branch's contribution to a join.
///
/// The value and the block it was produced in travel together because they are the two
/// halves of one `phi` incoming; a `phi` naming a block that did not produce the value is
/// precisely the defect this type makes unrepresentable. A branch that transferred
/// contributes nothing at all, rather than a value nobody produced.
enum Incoming {
    Value { value: u32, ty: Ty, from: String },
    Transferred,
}

/// Take an operand's value, or stop emitting this expression because control left it.
///
/// The emitter's counterpart to the interpreter's `take!`, and for the same reason: a
/// `break` can sit inside anything used as a value, and when it does the enclosing
/// expression has no value and never will. Everything already emitted keeps its effects;
/// nothing after it is emitted at all.
macro_rules! operand {
    ($e:expr) => {
        match $e {
            Val::Of { value, .. } => value,
            Val::Transferred => return Val::Transferred,
        }
    };
}

impl Emitter {
    fn fresh(&mut self) -> u32 {
        let n = self.next;
        self.next += 1;
        n
    }

    /// Start a block and make it the current one.
    fn open(&mut self, label: &str) {
        let _ = writeln!(self.out, "{label}:");
        self.current = label.to_owned();
    }

    /// `line:column`, 1-based, from a byte offset. Mirrors the renderer's convention so a
    /// runtime error from a compiled program points where the same error from
    /// `nazm run` would.
    fn position(&self, offset: u32) -> (usize, usize) {
        let offset = (offset as usize).min(self.source_len);
        let line = self.lines.partition_point(|&start| start <= offset);
        let line_start = self.lines.get(line.saturating_sub(1)).copied().unwrap_or(0);
        (line.max(1), offset - line_start + 1)
    }

    /// Intern a NUL-terminated constant and return `(symbol, byte length)`.
    fn intern(&mut self, text: &str) -> (String, usize) {
        if let Some((sym, _)) = self.strings.iter().find(|(_, t)| t == text) {
            return (sym.clone(), text.len() + 1);
        }
        let sym = format!("@.msg{}", self.strings.len());
        self.strings.push((sym.clone(), text.to_owned()));
        (sym, text.len() + 1)
    }

    /// Emit the branch that reports `failure` and never returns, then continue after it.
    ///
    /// The failure block ends in `unreachable`, so it is never a predecessor of anything:
    /// a `phi` downstream cannot receive an incoming value from a path that terminated.
    fn guard(&mut self, cond: u32, failure: &Failure, span_offset: u32) {
        let (line, col) = self.position(span_offset);
        let text = format!(
            "nazm: error[{}]: {} at {}:{line}:{col}\n",
            failure.code, failure.message, self.file
        );
        let (sym, len) = self.intern(&text);
        let fail = self.fresh();
        let cont = self.fresh();
        let _ = writeln!(self.out, "  br i1 %{cond}, label %b{fail}, label %b{cont}");
        self.open(&format!("b{fail}"));
        let _ = writeln!(self.out, "  call void @nz.fail(ptr {sym}, i64 {len})");
        let _ = writeln!(self.out, "  unreachable");
        self.open(&format!("b{cont}"));
    }

    /// Statements, then the block's value.
    fn block(&mut self, b: &Block) -> Val {
        if self.stmts(&b.stmts) == Reach::Transferred {
            return Val::Transferred;
        }
        match &b.result {
            Some(e) => self.expr(e),
            // Unreachable: lowering leaves this empty only when the statements transfer,
            // and that path returned above. Reported as a transfer rather than as a
            // value, because inventing one is the single thing this file must not do —
            // the block left without a terminator is then a module clang rejects loudly,
            // which is a much better failure than a program that runs and is wrong.
            None => Val::Transferred,
        }
    }

    /// A statement sequence: a loop body, a statement-`if` branch, or a block's
    /// statements. Shared by all three so they cannot acquire different semantics.
    ///
    /// Stops at the first statement that transfers. Anything after it is unreachable, and
    /// the checker has already refused it (`N0313`) — but appending to a block that
    /// already has a terminator is invalid IR, so this does not rely on that.
    fn stmts(&mut self, stmts: &[Stmt]) -> Reach {
        for stmt in stmts {
            let reach = match stmt {
                // One path for both, deliberately. A declaration and a reassignment are
                // the same store once the slot is known, and giving them two arms here
                // would be two places to keep a store correct.
                //
                // The value is emitted **before** the store. An operation that fails
                // inside it branches to a block ending in `unreachable`; an operation
                // that transfers branches to a loop target. Either way the store is not
                // on a path from there, so an interrupted right-hand side never reaches
                // the slot.
                Stmt::Let { slot, value } | Stmt::Assign { slot, value } => {
                    match self.expr(value) {
                        Val::Of { value: v, ty } => {
                            let _ =
                                writeln!(self.out, "  store {} %{v}, ptr %slot{slot}", ty.llvm());
                            Reach::FallsThrough
                        }
                        Val::Transferred => Reach::Transferred,
                    }
                }
                // Evaluated for its effects; the value is dropped, exactly as `nazm run`
                // drops it. What is not dropped is a transfer raised inside it.
                Stmt::Discard(e) => match self.expr(e) {
                    Val::Of { .. } => Reach::FallsThrough,
                    Val::Transferred => Reach::Transferred,
                },
                Stmt::While { cond, body } => self.while_loop(cond, body),
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => self.statement_if(cond, then_body, else_body.as_deref()),
                Stmt::Break => {
                    self.transfer(|t| &t.exit);
                    Reach::Transferred
                }
                Stmt::Continue => {
                    self.transfer(|t| &t.head);
                    Reach::Transferred
                }
            };
            if reach == Reach::Transferred {
                return Reach::Transferred;
            }
        }
        Reach::FallsThrough
    }

    /// Branch to the innermost loop's exit or condition.
    ///
    /// The stack is never empty here: lowering refuses a `break` or `continue` outside a
    /// loop, and the checker refuses it first (`N0312`), so a `Stmt::Break` reaches this
    /// point only from inside a `Stmt::While` body. If it somehow were empty, emitting
    /// nothing would leave the block without a terminator and clang would reject the
    /// module — a loud failure rather than a jump to the wrong place.
    fn transfer(&mut self, pick: impl Fn(&Targets) -> &String) {
        if let Some(target) = self.loops.last() {
            let label = pick(target).clone();
            let _ = writeln!(self.out, "  br label %{label}");
        }
    }

    /// `while`, as a condition block, a body block, a back edge and an exit block.
    ///
    /// # The back edge targets the condition, not the body
    ///
    /// That is the whole of the loop's meaning, and the one thing easy to get wrong.
    /// Jumping back to the body would evaluate the condition once and then loop on its
    /// first answer forever. `continue` targets the same label for the same reason.
    ///
    /// # The condition is not inside the loop
    ///
    /// The targets are pushed **after** the condition is emitted and popped before the
    /// exit block is opened, so a `break` written in a condition leaves the *enclosing*
    /// loop. That is the existing language's rule in both implementations, and pushing
    /// earlier would silently retarget such a `break` at this loop.
    ///
    /// # No `alloca` here
    ///
    /// Every local has a slot in [`emit_function`]'s entry block, including locals
    /// declared inside this body — a slot is per *binding*, not per execution of one.
    ///
    /// # Terminators
    ///
    /// The conditional branch and the back edge are appended to whatever block emission
    /// has reached, never to the label opened for it: a condition or body may open blocks
    /// of its own for a nested `if`, a checked-arithmetic guard, or a nested loop. And
    /// the back edge is written **only if the body's path still falls through** — a body
    /// ending in `break` or `continue` has its terminator already.
    fn while_loop(&mut self, cond: &Expr, body: &[Stmt]) -> Reach {
        let head = format!("b{}", self.fresh());
        let body_label = format!("b{}", self.fresh());
        let exit = format!("b{}", self.fresh());

        // The block before the loop needs a terminator: control does not fall into a
        // label in LLVM.
        let _ = writeln!(self.out, "  br label %{head}");

        self.open(&head);
        let c = match self.expr(cond) {
            Val::Of { value, .. } => value,
            // The condition transferred unconditionally, so it left through an enclosing
            // loop. Neither this body nor this exit can be reached, and emitting either
            // would produce blocks with no predecessors.
            Val::Transferred => return Reach::Transferred,
        };
        let _ = writeln!(self.out, "  br i1 %{c}, label %{body_label}, label %{exit}");

        self.open(&body_label);
        self.loops.push(Targets {
            head: head.clone(),
            exit: exit.clone(),
        });
        let reach = self.stmts(body);
        self.loops.pop();
        if reach == Reach::FallsThrough {
            let _ = writeln!(self.out, "  br label %{head}");
        }

        // Always reachable: the condition's false edge goes here, whatever the body did.
        self.open(&exit);
        Reach::FallsThrough
    }

    /// Statement-position `if`: no value, and either branch may transfer.
    ///
    /// # Which continuations exist
    ///
    /// With no `else`, the false edge goes straight to the continuation, so the statement
    /// **always falls through** however the branch ends. With an `else`, the continuation
    /// is opened only if at least one branch still falls through — if both transfer, no
    /// continuation is fabricated and the enclosing sequence stops here.
    fn statement_if(
        &mut self,
        cond: &Expr,
        then_body: &[Stmt],
        else_body: Option<&[Stmt]>,
    ) -> Reach {
        let c = match self.expr(cond) {
            Val::Of { value, .. } => value,
            Val::Transferred => return Reach::Transferred,
        };
        let then_label = format!("b{}", self.fresh());
        let cont = format!("b{}", self.fresh());

        let Some(else_body) = else_body else {
            let _ = writeln!(self.out, "  br i1 %{c}, label %{then_label}, label %{cont}");
            self.open(&then_label);
            if self.stmts(then_body) == Reach::FallsThrough {
                let _ = writeln!(self.out, "  br label %{cont}");
            }
            self.open(&cont);
            return Reach::FallsThrough;
        };

        let else_label = format!("b{}", self.fresh());
        let _ = writeln!(
            self.out,
            "  br i1 %{c}, label %{then_label}, label %{else_label}"
        );

        self.open(&then_label);
        let then_reach = self.stmts(then_body);
        if then_reach == Reach::FallsThrough {
            let _ = writeln!(self.out, "  br label %{cont}");
        }

        self.open(&else_label);
        let else_reach = self.stmts(else_body);
        if else_reach == Reach::FallsThrough {
            let _ = writeln!(self.out, "  br label %{cont}");
        }

        if then_reach == Reach::FallsThrough || else_reach == Reach::FallsThrough {
            self.open(&cont);
            Reach::FallsThrough
        } else {
            // Both branches left. Nothing branches to `cont`, so it is never opened.
            Reach::Transferred
        }
    }

    fn expr(&mut self, e: &Expr) -> Val {
        match e {
            Expr::Int(v) => {
                // Materialised through an `add` rather than used inline so that every
                // expression has a value number and the emitter has one shape to reason
                // about. `-O2` folds it; `-O0` does not, which is what makes the
                // arithmetic below genuinely execute during testing.
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = add i64 {v}, 0");
                Val::Of {
                    value: n,
                    ty: Ty::Int,
                }
            }
            Expr::Bool(v) => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = or i1 {v}, false");
                Val::Of {
                    value: n,
                    ty: Ty::Bool,
                }
            }
            Expr::Load { slot, ty } => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = load {}, ptr %slot{slot}", ty.llvm());
                Val::Of { value: n, ty: *ty }
            }
            Expr::Neg { operand, span } => {
                let v = operand!(self.expr(operand));
                let r = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{r} = call {{i64, i1}} @llvm.ssub.with.overflow.i64(i64 0, i64 %{v})"
                );
                let ov = self.fresh();
                let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
                self.guard(
                    ov,
                    &Failure {
                        code: "N0400",
                        message: "this negation overflows `Int`".to_owned(),
                    },
                    span.start,
                );
                let out = self.fresh();
                let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
                Val::Of {
                    value: out,
                    ty: Ty::Int,
                }
            }
            Expr::Arith { op, lhs, rhs, span } => {
                // Left, then right. `docs/spec.md` specifies the order, and here it is
                // observable twice over: it decides which diagnostic a program with two
                // failing operands reports, and — since either side may contain a
                // `break` — whether the right side is evaluated at all.
                let a = operand!(self.expr(lhs));
                let b = operand!(self.expr(rhs));
                let value = match op {
                    ArithOp::Add | ArithOp::Sub | ArithOp::Mul => {
                        self.checked(*op, a, b, span.start)
                    }
                    ArithOp::Div => self.divide(a, b, span.start),
                    ArithOp::Rem => self.remainder(a, b, span.start),
                };
                Val::Of { value, ty: Ty::Int }
            }
            Expr::Compare {
                op,
                operand_ty,
                lhs,
                rhs,
            } => {
                // Same left-to-right order, for the same reasons.
                let a = operand!(self.expr(lhs));
                let b = operand!(self.expr(rhs));
                let out = self.fresh();
                // No guard, because no comparison can fail: `icmp` on two `i64`s or two
                // `i1`s is total.
                let _ = writeln!(
                    self.out,
                    "  %{out} = icmp {} {} %{a}, %{b}",
                    op.predicate(),
                    operand_ty.llvm()
                );
                Val::Of {
                    value: out,
                    ty: Ty::Bool,
                }
            }
            Expr::Call { callee, args, ret } => {
                // Every argument is evaluated, left to right, **before** the call
                // instruction is written. So an argument that fails stops the program
                // there and an argument that transfers leaves through its loop target:
                // either way the later arguments are never evaluated and the callee is
                // never entered. That is not an optimisation — the call instruction is
                // simply not on a path from where control went.
                let mut vals = Vec::new();
                for a in args {
                    match self.expr(a) {
                        Val::Of { value, ty } => vals.push((value, ty)),
                        Val::Transferred => return Val::Transferred,
                    }
                }
                let list = vals
                    .iter()
                    .map(|(v, t)| format!("{} %{v}", t.llvm()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {} {}({list})",
                    ret.llvm(),
                    callee.symbol()
                );
                Val::Of {
                    value: out,
                    ty: *ret,
                }
            }
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => self.conditional(cond, then_block, else_block),
        }
    }

    /// Value-position `if`, as a conditional branch and a join with a `phi`.
    ///
    /// The condition is emitted once, before either branch, and only the selected
    /// branch's block is ever entered — so a failure in the branch not taken cannot run.
    /// That is not an optimisation; it is what a conditional branch means.
    ///
    /// # Which branches reach the join
    ///
    /// Only the ones that produce a value. A branch ending in `break` or `continue` has
    /// already branched somewhere else, contributes no `phi` incoming, and is not named
    /// by one. When **one** branch transfers the `phi` has a single incoming, which is
    /// how the value comes from the branch that actually arrives; when **both** do, there
    /// is no join, no `phi`, and no value — the expression is emitted for its effects and
    /// the caller is told control left.
    fn conditional(&mut self, cond: &Expr, then_b: &Block, else_b: &Block) -> Val {
        let c = operand!(self.expr(cond));
        let then_label = format!("b{}", self.fresh());
        let else_label = format!("b{}", self.fresh());
        let join_label = format!("b{}", self.fresh());
        let _ = writeln!(
            self.out,
            "  br i1 %{c}, label %{then_label}, label %{else_label}"
        );

        self.open(&then_label);
        let then_in = self.value_branch(then_b, &join_label);

        self.open(&else_label);
        let else_in = self.value_branch(else_b, &join_label);

        let incomings: Vec<(u32, Ty, String)> = [then_in, else_in]
            .into_iter()
            .filter_map(|i| match i {
                Incoming::Value { value, ty, from } => Some((value, ty, from)),
                Incoming::Transferred => None,
            })
            .collect();

        let Some((_, ty, _)) = incomings.first() else {
            // Neither branch arrives. Nothing branched to the join, so it is never
            // opened, and there is no value to hand back.
            return Val::Transferred;
        };
        let ty = *ty;

        self.open(&join_label);
        let out = self.fresh();
        let list = incomings
            .iter()
            .map(|(v, _, from)| format!("[ %{v}, %{from} ]"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(self.out, "  %{out} = phi {} {list}", ty.llvm());
        Val::Of { value: out, ty }
    }

    /// Emit one branch of a value-position `if`, and branch to the join if it arrives.
    ///
    /// The predecessor is read **after** the branch is lowered, not before: the branch
    /// may have opened blocks of its own — a nested `if`, any checked arithmetic, whose
    /// guard opens a continuation, a nested loop — and the block a `phi` must name is the
    /// one holding the jump to the join. Reading it here, next to the jump, is what keeps
    /// the two from drifting apart.
    fn value_branch(&mut self, b: &Block, join: &str) -> Incoming {
        match self.block(b) {
            Val::Of { value, ty } => {
                let from = self.current.clone();
                let _ = writeln!(self.out, "  br label %{join}");
                Incoming::Value { value, ty, from }
            }
            Val::Transferred => Incoming::Transferred,
        }
    }

    fn checked(&mut self, op: ArithOp, a: u32, b: u32, offset: u32) -> u32 {
        let intrinsic = match op {
            ArithOp::Add => "sadd",
            ArithOp::Sub => "ssub",
            ArithOp::Mul => "smul",
            // Division is guarded by comparison, not by an intrinsic: there is no
            // `sdiv.with.overflow`.
            ArithOp::Div | ArithOp::Rem => unreachable!("guarded separately"),
        };
        let r = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{r} = call {{i64, i1}} @llvm.{intrinsic}.with.overflow.i64(i64 %{a}, i64 %{b})"
        );
        let ov = self.fresh();
        let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
        self.guard(
            ov,
            &Failure {
                code: "N0400",
                message: format!("this {} overflows `Int`", op.overflow_noun()),
            },
            offset,
        );
        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
        out
    }

    /// Both of division's failures, in the order the interpreter checks them.
    fn divide(&mut self, a: u32, b: u32, offset: u32) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            offset,
        );

        let overflows = self.min_over_minus_one(a, b);
        self.guard(
            overflows,
            &Failure {
                code: "N0400",
                message: "this division overflows `Int`".to_owned(),
            },
            offset,
        );

        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = sdiv i64 %{a}, %{b}");
        out
    }

    /// Remainder, whose *only* failure is a zero divisor.
    ///
    /// `Int::MIN % -1` is mathematically zero and zero is representable, so `docs/spec.md`
    /// specifies it as `0`. The `srem` instruction faults on that pair anyway — it
    /// computes the quotient — so the pair is answered with a constant and never reaches
    /// the instruction. Getting this wrong would give `%` a second failure case the
    /// language says it does not have.
    fn remainder(&mut self, a: u32, b: u32, offset: u32) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            offset,
        );

        let special = self.min_over_minus_one(a, b);
        let is_zero = format!("b{}", self.fresh());
        let compute = format!("b{}", self.fresh());
        let join = format!("b{}", self.fresh());
        let _ = writeln!(
            self.out,
            "  br i1 %{special}, label %{is_zero}, label %{compute}"
        );
        self.open(&compute);
        let r = self.fresh();
        let _ = writeln!(self.out, "  %{r} = srem i64 %{a}, %{b}");
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&is_zero);
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&join);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi i64 [ %{r}, %{compute} ], [ 0, %{is_zero} ]"
        );
        out
    }

    /// `a == Int::MIN && b == -1` — the one operand pair `sdiv` and `srem` fault on.
    fn min_over_minus_one(&mut self, a: u32, b: u32) -> u32 {
        let is_min = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{is_min} = icmp eq i64 %{a}, -9223372036854775808"
        );
        let is_m1 = self.fresh();
        let _ = writeln!(self.out, "  %{is_m1} = icmp eq i64 %{b}, -1");
        let both = self.fresh();
        let _ = writeln!(self.out, "  %{both} = and i1 %{is_min}, %{is_m1}");
        both
    }
}

/// Emit a complete, self-contained LLVM IR module.
///
/// The result is text on purpose: `docs/architecture.md` §1 says not to link against LLVM
/// in year one, and `clang` reads IR directly, so this needs no LLVM dependency at all.
#[must_use]
pub fn emit_llvm(module: &Module, file: &str, source: &str) -> String {
    let mut line_starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let mut e = Emitter {
        out: String::new(),
        next: 0,
        current: "entry".to_owned(),
        strings: Vec::new(),
        file: file.to_owned(),
        lines: line_starts,
        source_len: source.len(),
        loops: Vec::new(),
    };

    // One definition per function, in `FuncId` order so the IR reads in the order the
    // signatures were collected.
    for f in &module.functions {
        emit_function(&mut e, f);
    }

    // The platform entry point is a **wrapper**, separate from Nazm's `main`. Nazm's
    // `main` is an ordinary `@nz.f<N>` like every other function, so a program can
    // neither collide with `@main` nor with the runtime helper — and **only this wrapper
    // prints**. A helper function returns a value and writes nothing.
    let _ = writeln!(e.out, "define i32 @main() {{");
    let _ = writeln!(e.out, "entry:");
    let _ = writeln!(e.out, "  %result = call i64 {}()", module.entry.symbol());
    let _ = writeln!(
        e.out,
        "  %print = call i32 (ptr, ...) @printf(ptr @.fmt, i64 %result)"
    );
    let _ = writeln!(e.out, "  ret i32 0");
    let _ = writeln!(e.out, "}}");

    let mut head = String::new();
    let _ = writeln!(head, "; Generated by nazm build. Do not edit.");
    let _ = writeln!(head, "; {TARGET_NOTE}");
    let _ = writeln!(head, "source_filename = \"{file}\"");
    head.push('\n');
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.sadd.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.ssub.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.smul.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(head, "declare i32 @printf(ptr noundef, ...)");
    let _ = writeln!(head, "declare i64 @write(i32, ptr, i64)");
    let _ = writeln!(head, "declare void @exit(i32) noreturn");
    head.push('\n');
    // `%lld\n`: the full signed 64-bit value in decimal. Encoding the result in the exit
    // status instead would truncate it to 8 bits, which for a language whose `Int` is
    // 64-bit would be a lie in the common case.
    let _ = writeln!(
        head,
        "@.fmt = private unnamed_addr constant [6 x i8] c\"%lld\\0A\\00\""
    );
    for (sym, text) in &e.strings {
        let _ = writeln!(
            head,
            "{sym} = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            text.len() + 1,
            escape(text)
        );
    }
    head.push('\n');
    // Not `llvm.trap`: a process killed by SIGILL is not "stops with a diagnostic".
    // Named in the same reserved `@nz.` space as the generated functions, so a Nazm
    // program cannot reach it and cannot collide with it.
    let _ = writeln!(
        head,
        "define internal void @nz.fail(ptr %m, i64 %n) noreturn {{"
    );
    let _ = writeln!(head, "  %w = call i64 @write(i32 2, ptr %m, i64 %n)");
    let _ = writeln!(head, "  call void @exit(i32 2)");
    let _ = writeln!(head, "  unreachable");
    let _ = writeln!(head, "}}");
    head.push('\n');

    head + &e.out
}

/// One function definition.
///
/// Parameters arrive as SSA values and are immediately stored into their slots, so the
/// body reads them with the same `load` it uses for any local. They are never stored to
/// again — the language has no mutable parameter — so one write at entry is the whole
/// story, and the emitter needs no notion of what a parameter is beyond "the first
/// `params` slots".
fn emit_function(e: &mut Emitter, f: &Function) {
    // Numbering restarts per function: LLVM requires a function's unnamed values to be
    // consecutive from zero, and `%p<n>`/`%slot<n>` are named so they do not consume the
    // numbering.
    e.next = 0;
    e.current = "entry".to_owned();
    // No loop target crosses a function boundary. Push and pop are balanced, so this is
    // already empty — clearing it says so rather than relying on it.
    e.loops.clear();

    let params = f
        .locals
        .iter()
        .take(f.params)
        .enumerate()
        .map(|(i, l)| format!("{} %p{i}", l.ty.llvm()))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        e.out,
        "define internal {} {}({params}) {{   ; {}",
        f.ret.llvm(),
        f.id.symbol(),
        f.name
    );
    let _ = writeln!(e.out, "entry:");
    for (i, local) in f.locals.iter().enumerate() {
        let _ = writeln!(
            e.out,
            "  %slot{i} = alloca {}            ; {} : {}",
            local.ty.llvm(),
            local.name,
            local.ty.name()
        );
    }
    for (i, local) in f.locals.iter().take(f.params).enumerate() {
        let _ = writeln!(e.out, "  store {} %p{i}, ptr %slot{i}", local.ty.llvm());
    }
    match e.block(&f.body) {
        Val::Of { value, .. } => {
            let _ = writeln!(e.out, "  ret {} %{value}", f.ret.llvm());
        }
        // The body transferred, so the end of the function is not reachable and there is
        // no value to return. `unreachable` is the terminator that says exactly that,
        // and it is the honest one — a `ret` here would need a value nothing produced.
        //
        // Lowering refuses this shape (a function body can only transfer through a
        // `break` outside a loop, which is `N0312`), so it is a terminator for a case
        // that should not arise rather than one that does.
        Val::Transferred => {
            let _ = writeln!(e.out, "  unreachable");
        }
    }
    let _ = writeln!(e.out, "}}");
    e.out.push('\n');
}

/// LLVM's constant-string escaping: `\XX` for anything not printable ASCII.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b == b'"' || b == b'\\' || !(0x20..0x7f).contains(&b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(b as char);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ir::{Block, CmpOp, Expr, FuncId, Function, Local, Module, Stmt};
    use nazm_span::Span;

    fn block(result: Expr) -> Block {
        Block {
            stmts: Vec::new(),
            result: Some(Box::new(result)),
        }
    }

    fn module(result: Expr, locals: Vec<Local>, stmts: Vec<Stmt>) -> Module {
        Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "main".to_owned(),
                locals,
                params: 0,
                ret: Ty::Int,
                body: Block {
                    stmts,
                    result: Some(Box::new(result)),
                },
            }],
            entry: FuncId(0),
        }
    }

    /// A two-function module: `main` calls `helper(args)`.
    fn calling(helper: Function, args: Vec<Expr>, ret: Ty) -> Module {
        Module {
            functions: vec![
                Function {
                    id: FuncId(0),
                    name: "main".to_owned(),
                    locals: Vec::new(),
                    params: 0,
                    ret: Ty::Int,
                    body: Block {
                        stmts: Vec::new(),
                        result: Some(Box::new(Expr::Call {
                            callee: FuncId(1),
                            args,
                            ret,
                        })),
                    },
                },
                helper,
            ],
            entry: FuncId(0),
        }
    }

    fn ir_of(m: &Module) -> String {
        emit_llvm(m, "t.nz", "a source line\nand another\n")
    }

    #[test]
    fn overflow_is_an_intrinsic_and_a_branch_never_nsw() {
        // The single most important property of this file. `nsw` does not check for
        // overflow — it promises overflow cannot happen, which licenses the optimiser to
        // delete the very branch that was supposed to catch it.
        let m = module(
            Expr::Arith {
                op: ArithOp::Add,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(2)),
                span: Span::new(0, 1),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(ir.contains("@llvm.sadd.with.overflow.i64"), "{ir}");
        assert!(ir.contains("br i1 %"), "{ir}");
        assert!(!ir.contains(" nsw "), "emitted nsw: {ir}");
        assert!(!ir.contains("llvm.trap"), "emitted llvm.trap: {ir}");
    }

    #[test]
    fn division_is_guarded_before_the_instruction_that_would_fault() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Div,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(0)),
                span: Span::new(2, 3),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        let guard = ir.find("icmp eq i64").expect("a zero check");
        let sdiv = ir.find("sdiv").expect("the division");
        assert!(guard < sdiv, "the division precedes its guard:\n{ir}");
        assert!(ir.contains("-9223372036854775808"), "no MIN/-1 check: {ir}");
        assert!(ir.contains("N0401"), "{ir}");
    }

    #[test]
    fn remainder_answers_min_by_minus_one_with_a_constant() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Rem,
                lhs: Box::new(Expr::Int(i64::MIN)),
                rhs: Box::new(Expr::Int(-1)),
                span: Span::new(0, 1),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(
            ir.contains("phi i64"),
            "no constant path for MIN % -1: {ir}"
        );
        assert!(ir.contains("srem"), "{ir}");
        assert!(ir.contains("N0401"), "{ir}");
        assert!(
            !ir.contains("remainder overflows"),
            "`%` was given an overflow case it does not have: {ir}"
        );
    }

    #[test]
    fn a_runtime_message_carries_its_code_and_position() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Div,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(0)),
                // The `/` on the third line: 18 bytes of preceding lines plus six into
                // this one, which is column 7.
                span: Span::new(24, 25),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = emit_llvm(&m, "prog.nz", "line one\nline two\n    1 / 0\n");
        assert!(ir.contains("prog.nz:3:7"), "{ir}");
        assert!(ir.contains("N0401"), "{ir}");
    }

    #[test]
    fn locals_are_slots_and_carry_their_type() {
        let m = module(
            Expr::Load {
                slot: 0,
                ty: Ty::Bool,
            },
            vec![Local {
                name: "flag".to_owned(),
                ty: Ty::Bool,
            }],
            vec![Stmt::Let {
                slot: 0,
                value: Expr::Bool(true),
            }],
        );
        let ir = ir_of(&m);
        assert!(ir.contains("%slot0 = alloca i1"), "{ir}");
        assert!(ir.contains("store i1 %"), "{ir}");
        assert!(ir.contains("load i1, ptr %slot0"), "{ir}");
    }

    #[test]
    fn a_comparison_is_signed_and_needs_no_guard() {
        // `slt`, not `ult`: `Int` is signed, so `-1 < 1` must hold. An unsigned predicate
        // would compare bit patterns and get it backwards.
        let m = module(
            Expr::Compare {
                op: CmpOp::Lt,
                operand_ty: Ty::Int,
                lhs: Box::new(Expr::Int(-1)),
                rhs: Box::new(Expr::Int(1)),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(ir.contains("icmp slt i64"), "{ir}");
        assert!(!ir.contains("icmp ult"), "unsigned comparison: {ir}");
    }

    #[test]
    fn a_phi_names_blocks_that_exist_and_reach_the_join() {
        // The bug this test exists for: naming the branch's *entry* label. A branch
        // containing checked arithmetic ends in the continuation block its guard opened,
        // not in the block it started in, so an entry-label `phi` would name a
        // predecessor that does not branch to the join — which LLVM rejects.
        //
        // Structural, not a snapshot: block numbers are an emission detail and pinning
        // them would fail on any unrelated change.
        let m = module(
            Expr::If {
                cond: Box::new(Expr::Bool(true)),
                // Arithmetic in the branch, so the branch opens blocks of its own.
                then_block: Box::new(block(Expr::Arith {
                    op: ArithOp::Add,
                    lhs: Box::new(Expr::Int(1)),
                    rhs: Box::new(Expr::Int(2)),
                    span: Span::new(0, 1),
                })),
                else_block: Box::new(block(Expr::Int(0))),
                ty: Some(Ty::Int),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);

        let phi = ir
            .lines()
            .find(|l| l.contains("phi i64") && l.contains("], ["))
            .expect("a phi for the if")
            .to_owned();

        let preds: Vec<String> = phi
            .split(", %")
            .skip(1)
            .filter_map(|s| s.split([' ', ']', ',']).next())
            .map(ToOwned::to_owned)
            .collect();
        assert_eq!(
            preds.len(),
            2,
            "a two-way phi needs two predecessors: {phi}"
        );

        for pred in preds {
            assert!(
                ir.contains(&format!("\n{pred}:")),
                "phi names `{pred}`, which is not a block:\n{ir}"
            );
            // And that block must actually branch to the join the phi is in.
            let after = ir.split(&format!("\n{pred}:")).nth(1).unwrap_or_default();
            let terminator = after
                .lines()
                .find(|l| l.trim_start().starts_with("br "))
                .unwrap_or_default();
            assert!(
                terminator.contains("br label %"),
                "`{pred}` does not jump to a join:\n{ir}"
            );
        }
    }

    #[test]
    fn a_symbol_is_generated_and_never_taken_from_the_source_name() {
        // A source name in an LLVM identifier needs escaping rules, and the escaping is
        // the bug. There are none here because no name reaches a symbol.
        let helper = Function {
            id: FuncId(1),
            // Deliberately hostile: spaces, quotes, an `@`, and the names of the C
            // functions this module declares.
            name: "printf\" @exit write".to_owned(),
            locals: Vec::new(),
            params: 0,
            ret: Ty::Int,
            body: Block {
                stmts: Vec::new(),
                result: Some(Box::new(Expr::Int(1))),
            },
        };
        let ir = ir_of(&calling(helper, Vec::new(), Ty::Int));

        assert!(ir.contains("define internal i64 @nz.f1("), "{ir}");
        // The name appears only after a comment marker, never as an identifier.
        for line in ir.lines().filter(|l| l.contains("printf\" @exit write")) {
            let before = line.split("printf").next().unwrap_or_default();
            assert!(
                before.contains(';'),
                "a source name escaped into code:\n{line}"
            );
        }
    }

    #[test]
    fn the_platform_entry_is_a_wrapper_and_only_it_prints() {
        // Nazm's `main` is an ordinary `@nz.fN`, so a program cannot collide with the C
        // entry point. And a helper returns a value rather than printing one.
        let helper = Function {
            id: FuncId(1),
            name: "helper".to_owned(),
            locals: Vec::new(),
            params: 0,
            ret: Ty::Int,
            body: Block {
                stmts: Vec::new(),
                result: Some(Box::new(Expr::Int(7))),
            },
        };
        let ir = ir_of(&calling(helper, Vec::new(), Ty::Int));

        assert!(ir.contains("define i32 @main() {"), "{ir}");
        assert!(
            ir.contains("define internal i64 @nz.f0("),
            "nazm main is not a wrapper: {ir}"
        );
        assert!(ir.contains("call i64 @nz.f0()"), "{ir}");
        assert_eq!(
            ir.matches("@printf").count(),
            2,
            "printf declared once, called once:\n{ir}"
        );
    }

    #[test]
    fn parameters_are_stored_into_their_slots_and_typed() {
        let helper = Function {
            id: FuncId(1),
            name: "helper".to_owned(),
            locals: vec![
                Local {
                    name: "n".to_owned(),
                    ty: Ty::Int,
                },
                Local {
                    name: "flag".to_owned(),
                    ty: Ty::Bool,
                },
            ],
            params: 2,
            ret: Ty::Int,
            body: Block {
                stmts: Vec::new(),
                result: Some(Box::new(Expr::Load {
                    slot: 0,
                    ty: Ty::Int,
                })),
            },
        };
        let ir = ir_of(&calling(
            helper,
            vec![Expr::Int(3), Expr::Bool(true)],
            Ty::Int,
        ));

        assert!(
            ir.contains("define internal i64 @nz.f1(i64 %p0, i1 %p1)"),
            "{ir}"
        );
        assert!(ir.contains("store i64 %p0, ptr %slot0"), "{ir}");
        assert!(ir.contains("store i1 %p1, ptr %slot1"), "{ir}");
        assert!(ir.contains("call i64 @nz.f1(i64 %"), "{ir}");
        assert!(
            ir.contains(", i1 %"),
            "the Bool argument lost its type: {ir}"
        );
    }

    #[test]
    fn the_result_is_printed_in_full_not_returned_as_a_status() {
        // An exit status is 8 bits. Returning a 64-bit `Int` through it would be wrong
        // for almost every value a program produces.
        let m = module(Expr::Int(i64::MIN), Vec::new(), Vec::new());
        let ir = ir_of(&m);
        assert!(ir.contains("@printf"), "{ir}");
        assert!(ir.contains("%lld"), "{ir}");
        assert!(ir.contains("ret i32 0"), "{ir}");
    }
}
