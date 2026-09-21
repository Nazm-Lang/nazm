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

use nazm_span::Span;

use crate::ir::{ArithOp, Block, Builtin, Expr, FuncId, Function, Module, Stmt, Ty};

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
    next_label: u32,
    /// The block instructions are currently being appended to — the predecessor a `phi`
    /// must name. See the module documentation.
    current: String,
    /// Error strings, interned so a program with the same failure twice carries it once.
    strings: Vec<(String, String)>,
    /// `Str` literals, interned the same way and for the same reason. Kept apart from
    /// the error strings because these are the program's data: they carry arbitrary
    /// bytes and are never NUL-terminated.
    blobs: Vec<(String, Vec<u8>)>,
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
    /// Open `scope` blocks, innermost last, each as the SSA value of its list of tasks.
    ///
    /// A list rather than a counter because the join has to be emitted on **every** way
    /// out of a scope — falling off the end, and every `return`, `break` or `continue`
    /// that leaves it — and each of those needs the right list.
    ///
    /// The allocation dominates the whole body, including any loop inside it, so naming
    /// the SSA value directly is valid wherever a join has to be written.
    scopes: Vec<u32>,
    /// The task trampolines, accumulated while functions are emitted and appended after
    /// them. One per `spawn` site: it unpacks that site's argument block and calls the
    /// function with the types that site established.
    tasks: String,
    /// Whether `scope` and `spawn` are reached, and with them the thread helpers.
    needs_tasks: bool,
    /// How many trampolines have been generated, which is what names the next one.
    task_count: usize,
    /// Whether a channel is reached.
    needs_channels: bool,
    /// Whether the I/O helpers and the argument globals are reached.
    needs_io: bool,
    /// Whether `str_join`'s helper is reached.
    needs_join: bool,
    /// Whether the sequence helpers are reached. They are emitted only when they are —
    /// a program that never touches a sequence carries none of this.
    needs_sequences: bool,
}

/// Where `break` and `continue` go for one loop.
///
/// `continue` targets the **condition**, never the body: re-evaluating the condition is
/// what `continue` means, and jumping to the body would skip it — including any call or
/// checked arithmetic it contains.
struct Targets {
    head: String,
    exit: String,
    /// How many scopes were open when this loop started.
    ///
    /// A `break` joins the scopes opened **inside** the loop and no others: the ones
    /// around the loop are still running and are joined by their own closing braces.
    /// Recording the depth here is what tells the two apart — the same shape the Nazm
    /// parser's per-expression stack base has, for the same reason.
    scope_depth: usize,
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

    /// Block labels come from their own counter.
    ///
    /// They used to share `fresh()` with SSA temporaries, and because the labels are
    /// *named* (`b7:`, not `7:`) LLVM never counts them — so every label left a hole in
    /// the value sequence, and the function emitted `%5` where the implicit counter was
    /// still at 3. LLVM 19 and Apple clang 21 tolerate that; clang 17 rejects it outright
    /// with "instruction expected to be numbered '%3'". Sequential is the portable form,
    /// and the two counters cannot collide because one only ever prints after `%` and the
    /// other only ever after `b`.
    fn fresh_label(&mut self) -> u32 {
        let n = self.next_label;
        self.next_label += 1;
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

    /// Intern a `Str` literal, and return the symbol of a **constant `{ptr, i64}`** for
    /// it.
    ///
    /// A whole constant rather than a pointer the emitter then assembles: building the
    /// pair with `insertvalue` from `undef` would work, but a literal is known entirely
    /// at compile time and a load of a constant says so. Nothing is undefined even for
    /// an instant.
    ///
    /// An empty literal still gets a byte of storage, so its pointer is valid to pass to
    /// `memcmp`, which the specification's "equality compares bytes" rule needs for
    /// `"" == ""`.
    fn blob(&mut self, bytes: &[u8]) -> String {
        if let Some((sym, _)) = self.blobs.iter().find(|(_, b)| b == bytes) {
            return format!("{sym}.v");
        }
        let sym = format!("@.s{}", self.blobs.len());
        self.blobs.push((sym.clone(), bytes.to_vec()));
        format!("{sym}.v")
    }

    /// Build a `{ ptr, i64 }` from a pointer and a length already in registers.
    fn make_str(&mut self, ptr: u32, len: u32) -> u32 {
        // `undef` here is the standard way to build an aggregate, and **both** fields are
        // written before the value is read — this is not the poison-producing shortcut
        // the architecture rules forbid, which is about arithmetic whose overflow an
        // optimiser may then assume away.
        let half = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{half} = insertvalue {{ ptr, i64 }} undef, ptr %{ptr}, 0"
        );
        let whole = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{whole} = insertvalue {{ ptr, i64 }} %{half}, i64 %{len}, 1"
        );
        whole
    }

    fn str_parts(&mut self, s: u32) -> (u32, u32) {
        let ptr = self.fresh();
        let _ = writeln!(self.out, "  %{ptr} = extractvalue {{ ptr, i64 }} %{s}, 0");
        let len = self.fresh();
        let _ = writeln!(self.out, "  %{len} = extractvalue {{ ptr, i64 }} %{s}, 1");
        (ptr, len)
    }

    /// Fail unless `ok`, with the out-of-bounds diagnostic.
    fn require(&mut self, bad: u32, message: &str, span: Span) {
        self.guard(
            bad,
            &Failure {
                code: "N0405",
                message: message.to_owned(),
            },
            span.start,
        );
    }

    /// A pointer to element `i` of a sequence, bounds-checked.
    ///
    /// The check is here rather than inside a helper because the diagnostic has to name
    /// *this* call's source position, and a shared helper has only one.
    fn sequence_slot(&mut self, handle: u32, index: u32, elem: Ty, span: Span) -> u32 {
        let len = self.fresh();
        let _ = writeln!(self.out, "  %{len} = load i64, ptr %{handle}");
        let neg = self.fresh();
        let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{index}, 0");
        let past = self.fresh();
        let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{index}, %{len}");
        let bad = self.fresh();
        let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
        self.require(bad, "this index is outside the sequence", span);
        let data_field = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{data_field} = getelementptr i8, ptr %{handle}, i64 16"
        );
        let data = self.fresh();
        let _ = writeln!(self.out, "  %{data} = load ptr, ptr %{data_field}");
        let slot = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{slot} = getelementptr {}, ptr %{data}, i64 %{index}",
            elem.llvm()
        );
        slot
    }

    /// `malloc`, with the failure the specification names rather than a null pointer
    /// loose in the program.
    fn allocate(&mut self, bytes: u32, span: Span) -> u32 {
        let buf = self.fresh();
        let _ = writeln!(self.out, "  %{buf} = call ptr @malloc(i64 %{bytes})");
        let null = self.fresh();
        let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{buf}, null");
        self.guard(
            null,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span.start,
        );
        buf
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
    /// The exact bytes a failure prints, interned, as `(symbol, length)`.
    ///
    /// Split out of [`Self::guard`] because a runtime helper that can fail — the one
    /// behind `chan_recv` — has to be *given* its message: it is one function serving
    /// every call site, and the position in the text is the call site's, not its own.
    fn failure_text(&mut self, failure: &Failure, span_offset: u32) -> (String, usize) {
        let (line, col) = self.position(span_offset);
        let text = format!(
            "nazm: error[{}]: {} at {}:{line}:{col}\n",
            failure.code, failure.message, self.file
        );
        self.intern(&text)
    }

    fn guard(&mut self, cond: u32, failure: &Failure, span_offset: u32) {
        let (sym, len) = self.failure_text(failure, span_offset);
        let fail = self.fresh_label();
        let cont = self.fresh_label();
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
                // Its own `ret`, not a branch to a shared exit. An operand that
                // transfers instead wins outright — the `break` in
                // `return if c { break; } else { 1 };` leaves the loop and no `ret` is
                // written, matching `take!`.
                //
                // The value is produced **before** the joins, and the joins before the
                // `ret`: a `return` inside a `scope` still waits for that scope's tasks,
                // which is the whole of what a scope promises. The order is the
                // interpreter's — evaluate, join, then leave.
                Stmt::Return(e) => {
                    if let Val::Of { value, ty } = self.expr(e) {
                        self.join_through(0);
                        let _ = writeln!(self.out, "  ret {} %{value}", ty.llvm());
                    }
                    Reach::Transferred
                }
                Stmt::Break => {
                    self.transfer(|t| &t.exit);
                    Reach::Transferred
                }
                Stmt::Continue => {
                    self.transfer(|t| &t.head);
                    Reach::Transferred
                }
                Stmt::Scope { body, span } => self.scope(body, *span),
                Stmt::Spawn {
                    callee,
                    args,
                    ret,
                    span,
                } => self.spawn(*callee, args, *ret, *span),
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
            let depth = target.scope_depth;
            // Only the scopes opened inside this loop. The ones around it are still
            // running, and joining them here would wait for tasks the loop never started.
            self.join_through(depth);
            let _ = writeln!(self.out, "  br label %{label}");
        }
    }

    /// Join every open scope from the innermost down to `depth`, innermost first.
    ///
    /// Innermost first because that is the nesting: an inner scope's tasks are contained
    /// by the outer one, and waiting for the outer first would be waiting for a task that
    /// has not been allowed to finish.
    fn join_through(&mut self, depth: usize) {
        for tids in self.scopes.clone().into_iter().skip(depth).rev() {
            let _ = writeln!(self.out, "  call void @nz.scope_join(ptr %{tids})");
        }
    }

    /// `scope { body }` — a list of tasks, the body, and the wait.
    ///
    /// The list is an ordinary sequence header, reused rather than reinvented: a task
    /// handle is pointer-sized and a growable array of them is exactly what
    /// `@nz.arr_new` already provides.
    ///
    /// A scope inside a loop allocates a fresh list each time round, because the
    /// statement is reached each time round. Nothing is ever freed, here as everywhere
    /// else (`docs/spec.md`).
    fn scope(&mut self, body: &[Stmt], span: Span) -> Reach {
        self.needs_sequences = true;
        self.needs_tasks = true;
        let tids = self.fresh();
        let _ = writeln!(self.out, "  %{tids} = call ptr @nz.arr_new()");
        let null = self.fresh();
        let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{tids}, null");
        self.guard(
            null,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span.start,
        );

        self.scopes.push(tids);
        let reach = self.stmts(body);
        self.scopes.pop();
        // A body that transferred has already emitted its joins on the way out, at the
        // point it left from — writing one here as well would append to a block that
        // already has a terminator.
        if reach == Reach::FallsThrough {
            let _ = writeln!(self.out, "  call void @nz.scope_join(ptr %{tids})");
        }
        reach
    }

    /// `spawn f(args);` — pack the arguments, start the thread, record it for the join.
    ///
    /// # Why the arguments are copied to the heap
    ///
    /// The task outlives this stack frame by construction, so it cannot be handed a
    /// pointer into it. One allocation per spawn carries the whole argument list, and its
    /// first eight bytes are the thread handle — so the list the scope joins holds one
    /// pointer per task and needs no second allocation.
    fn spawn(&mut self, callee: FuncId, args: &[Expr], ret: Ty, span: Span) -> Reach {
        self.needs_sequences = true;
        self.needs_tasks = true;

        let mut vals = Vec::new();
        for a in args {
            match self.expr(a) {
                Val::Of { value, ty } => vals.push((value, ty)),
                Val::Transferred => return Reach::Transferred,
            }
        }

        // The handle first, then each argument at its own offset. `Str` is sixteen
        // bytes; everything else is eight, which keeps every offset a multiple of eight
        // and so correctly aligned for whatever is stored there.
        let mut offsets = Vec::new();
        let mut size = 8u64;
        for (_, ty) in &vals {
            offsets.push(size);
            size += if *ty == Ty::Str { 16 } else { 8 };
        }

        let blk = self.fresh();
        let _ = writeln!(self.out, "  %{blk} = call ptr @malloc(i64 {size})");
        let null = self.fresh();
        let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{blk}, null");
        self.guard(
            null,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span.start,
        );

        for ((value, ty), offset) in vals.iter().zip(&offsets) {
            let slot = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{slot} = getelementptr i8, ptr %{blk}, i64 {offset}"
            );
            let _ = writeln!(self.out, "  store {} %{value}, ptr %{slot}", ty.llvm());
        }

        let types: Vec<Ty> = vals.iter().map(|(_, t)| *t).collect();
        let trampoline = self.trampoline(callee, ret, &types, &offsets);
        let tids = self.scopes.last().copied().unwrap_or_default();
        let rc = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{rc} = call i32 @nz.spawn(ptr %{tids}, ptr @{trampoline}, ptr %{blk})"
        );
        let oom = self.fresh();
        let _ = writeln!(self.out, "  %{oom} = icmp eq i32 %{rc}, -1");
        self.guard(
            oom,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span.start,
        );
        let bad = self.fresh();
        let _ = writeln!(self.out, "  %{bad} = icmp sgt i32 %{rc}, 0");
        self.guard(
            bad,
            &Failure {
                code: "N0404",
                // The interpreter's wording for the same failure: each task is an
                // operating-system thread, and running out of them is a limit of the
                // machine rather than anything about the program.
                message: "this task could not be started".to_owned(),
            },
            span.start,
        );
        Reach::FallsThrough
    }

    /// The `void *(void *)` function a thread actually starts in.
    ///
    /// `pthread_create` takes one pointer, so every spawn site needs a small function
    /// that unpacks its own argument block and calls the Nazm function with the types
    /// that site established. One per site rather than one per callee: two spawns of the
    /// same function are the same trampoline only if their argument types match, and
    /// generating per site makes that true without having to check it.
    fn trampoline(&mut self, callee: FuncId, ret: Ty, types: &[Ty], offsets: &[u64]) -> String {
        let name = format!("nz.task{}", self.taskno());
        let mut body = String::new();
        let _ = writeln!(&mut body, "\ndefine internal ptr @{name}(ptr %a) {{");
        let _ = writeln!(&mut body, "entry:");
        let mut passed = Vec::new();
        for (i, (ty, offset)) in types.iter().zip(offsets).enumerate() {
            let _ = writeln!(
                &mut body,
                "  %p{i} = getelementptr i8, ptr %a, i64 {offset}"
            );
            let _ = writeln!(&mut body, "  %v{i} = load {}, ptr %p{i}", ty.llvm());
            passed.push(format!("{} %v{i}", ty.llvm()));
        }
        // The task's return value is discarded, which `docs/spec.md` says outright:
        // results travel through channels, so there is one mechanism rather than two.
        let ret = ret.llvm();
        let _ = writeln!(
            &mut body,
            "  %r = call {ret} {}({})",
            callee.symbol(),
            passed.join(", ")
        );
        let _ = writeln!(&mut body, "  ret ptr null");
        let _ = writeln!(&mut body, "}}");
        self.tasks.push_str(&body);
        name
    }

    fn taskno(&mut self) -> usize {
        let n = self.task_count;
        self.task_count += 1;
        n
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
        let head = format!("b{}", self.fresh_label());
        let body_label = format!("b{}", self.fresh_label());
        let exit = format!("b{}", self.fresh_label());

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
            scope_depth: self.scopes.len(),
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
        let then_label = format!("b{}", self.fresh_label());
        let cont = format!("b{}", self.fresh_label());

        let Some(else_body) = else_body else {
            let _ = writeln!(self.out, "  br i1 %{c}, label %{then_label}, label %{cont}");
            self.open(&then_label);
            if self.stmts(then_body) == Reach::FallsThrough {
                let _ = writeln!(self.out, "  br label %{cont}");
            }
            self.open(&cont);
            return Reach::FallsThrough;
        };

        let else_label = format!("b{}", self.fresh_label());
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
                // `Str` is a two-field aggregate, so `icmp` does not apply to it: two
                // strings are equal when they have the same bytes, not when they share
                // a buffer. Ordering `Str` is refused by the checker, so only `==` and
                // `!=` reach here.
                if *operand_ty == Ty::Str {
                    let out = self.string_equality(a, b, *op == crate::ir::CmpOp::NotEq);
                    return Val::Of {
                        value: out,
                        ty: Ty::Bool,
                    };
                }
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
            Expr::Str(bytes) => {
                let sym = self.blob(bytes);
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = load {{ ptr, i64 }}, ptr {sym}");
                Val::Of {
                    value: n,
                    ty: Ty::Str,
                }
            }
            Expr::Builtin { op, args, span } => self.builtin(*op, args, *span),
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => self.conditional(cond, then_block, else_block),
        }
    }

    /// The built-ins, each as the instructions it means.
    ///
    /// Arguments are evaluated left to right and **all of them before any work**, the
    /// same rule calls follow, so an argument that fails or transfers stops the built-in
    /// before it starts.
    fn builtin(&mut self, op: Builtin, args: &[Expr], span: Span) -> Val {
        let mut vals = Vec::new();
        for a in args {
            match self.expr(a) {
                Val::Of { value, .. } => vals.push(value),
                Val::Transferred => return Val::Transferred,
            }
        }
        let arg = |i: usize| vals.get(i).copied().unwrap_or_default();

        let value = match op {
            Builtin::StrLen => {
                let (_, len) = self.str_parts(arg(0));
                return Val::Of {
                    value: len,
                    ty: Ty::Int,
                };
            }
            Builtin::StrByte => {
                let (ptr, len) = self.str_parts(arg(0));
                let i = arg(1);
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{i}, 0");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{i}, %{len}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
                self.require(bad, "this byte index is outside the `Str`", span);
                let at = self.fresh();
                let _ = writeln!(self.out, "  %{at} = getelementptr i8, ptr %{ptr}, i64 %{i}");
                let byte = self.fresh();
                let _ = writeln!(self.out, "  %{byte} = load i8, ptr %{at}");
                let wide = self.fresh();
                // Zero-extended: a byte is `0..=255`, never negative. `sext` here would
                // make every byte above 127 a negative `Int`.
                let _ = writeln!(self.out, "  %{wide} = zext i8 %{byte} to i64");
                return Val::Of {
                    value: wide,
                    ty: Ty::Int,
                };
            }
            Builtin::StrSlice => {
                let (ptr, len) = self.str_parts(arg(0));
                let (from, to) = (arg(1), arg(2));
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{from}, 0");
                let inverted = self.fresh();
                let _ = writeln!(self.out, "  %{inverted} = icmp slt i64 %{to}, %{from}");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sgt i64 %{to}, %{len}");
                let a = self.fresh();
                let _ = writeln!(self.out, "  %{a} = or i1 %{neg}, %{inverted}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{a}, %{past}");
                self.require(bad, "this slice is not inside the `Str`", span);
                // No copy: a slice of an immutable, never-freed buffer is a pointer and
                // a length. See `docs/bootstrap-design.md`, *Memory model*.
                let start = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{start} = getelementptr i8, ptr %{ptr}, i64 %{from}"
                );
                let n = self.fresh();
                // Cannot overflow: the guard above establishes `0 <= from <= to`.
                let _ = writeln!(self.out, "  %{n} = sub i64 %{to}, %{from}");
                self.make_str(start, n)
            }
            Builtin::StrConcat => {
                let (pa, la) = self.str_parts(arg(0));
                let (pb, lb) = self.str_parts(arg(1));
                let n = self.fresh();
                // Two lengths that each fit in memory cannot sum past `Int`, so this is
                // a plain add rather than a checked one.
                let _ = writeln!(self.out, "  %{n} = add i64 %{la}, %{lb}");
                // One byte more than needed, so an empty result still owns storage and
                // its pointer stays valid to compare.
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 %{n}, 1");
                let buf = self.allocate(room, span);
                let _ = writeln!(
                    self.out,
                    "  call void @llvm.memcpy.p0.p0.i64(ptr %{buf}, ptr %{pa}, i64 %{la}, i1 false)"
                );
                let rest = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{rest} = getelementptr i8, ptr %{buf}, i64 %{la}"
                );
                let _ = writeln!(
                    self.out,
                    "  call void @llvm.memcpy.p0.p0.i64(ptr %{rest}, ptr %{pb}, i64 %{lb}, i1 false)"
                );
                self.make_str(buf, n)
            }
            // --- the outside world ------------------------------------------------
            //
            // Ambient: nothing in the language restricts which functions may reach the
            // file system. `docs/spec.md` says so plainly rather than implying a
            // capability system that is not here.
            Builtin::ArgsCount => {
                self.needs_io = true;
                let argc = self.fresh();
                let _ = writeln!(self.out, "  %{argc} = load i64, ptr @nz.argc");
                return Val::Of {
                    value: argc,
                    ty: Ty::Int,
                };
            }
            Builtin::Arg => {
                self.needs_io = true;
                let i = arg(0);
                let argc = self.fresh();
                let _ = writeln!(self.out, "  %{argc} = load i64, ptr @nz.argc");
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{i}, 0");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{i}, %{argc}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
                self.require(bad, "this index is outside the arguments", span);
                // `argv[0]` is the program's own name and is deliberately not visible:
                // it is a path nobody should depend on, and excluding it is what makes
                // `arg(i)` mean the same thing interpreted and compiled.
                let shifted = self.fresh();
                let _ = writeln!(self.out, "  %{shifted} = add i64 %{i}, 1");
                let base = self.fresh();
                let _ = writeln!(self.out, "  %{base} = load ptr, ptr @nz.argv");
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr ptr, ptr %{base}, i64 %{shifted}"
                );
                let text = self.fresh();
                let _ = writeln!(self.out, "  %{text} = load ptr, ptr %{slot}");
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = call i64 @strlen(ptr %{text})");
                self.make_str(text, len)
            }
            Builtin::Print | Builtin::EPrint => {
                let (ptr, len) = self.str_parts(arg(0));
                let fd = if op == Builtin::Print { 1 } else { 2 };
                let written = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{written} = call i64 @write(i32 {fd}, ptr %{ptr}, i64 %{len})"
                );
                return Val::Of {
                    value: written,
                    ty: Ty::Int,
                };
            }
            Builtin::ExitWith => {
                let narrow = self.fresh();
                let _ = writeln!(self.out, "  %{narrow} = trunc i64 %{} to i32", arg(0));
                let _ = writeln!(self.out, "  call void @exit(i32 %{narrow})");
                let _ = writeln!(self.out, "  unreachable");
                // The process is over, so there is no value and none is invented. This
                // is the one built-in that transfers, and the emitter already has the
                // vocabulary for that.
                return Val::Transferred;
            }
            Builtin::ReadFile => {
                self.needs_io = true;
                let (ptr, len) = self.str_parts(arg(0));
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {{ ptr, i64 }} @nz.read_file(ptr %{ptr}, i64 %{len})"
                );
                let got = self.fresh();
                let _ = writeln!(self.out, "  %{got} = extractvalue {{ ptr, i64 }} %{out}, 0");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp eq ptr %{got}, null");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0407",
                        message: "this file could not be read".to_owned(),
                    },
                    span.start,
                );
                return Val::Of {
                    value: out,
                    ty: Ty::Str,
                };
            }
            Builtin::WriteFile => {
                self.needs_io = true;
                let (pp, pn) = self.str_parts(arg(0));
                let (dp, dn) = self.str_parts(arg(1));
                let n = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{n} = call i64 @nz.write_file(ptr %{pp}, i64 %{pn}, ptr %{dp}, i64 %{dn})"
                );
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp slt i64 %{n}, 0");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0407",
                        message: "this file could not be written".to_owned(),
                    },
                    span.start,
                );
                return Val::Of {
                    value: n,
                    ty: Ty::Int,
                };
            }
            Builtin::FileExists => {
                self.needs_io = true;
                let (pp, pn) = self.str_parts(arg(0));
                let yes = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{yes} = call i1 @nz.file_exists(ptr %{pp}, i64 %{pn})"
                );
                return Val::Of {
                    value: yes,
                    ty: Ty::Bool,
                };
            }

            // --- sequences -------------------------------------------------------
            //
            // The handle is a pointer to a `{ i64 len, i64 cap, ptr data }` header, so
            // two bindings of one sequence are the same header and a push through
            // either is visible through both. Growth rewrites the header in place;
            // the handle never moves.
            Builtin::IntsNew | Builtin::StrsNew => {
                self.needs_sequences = true;
                let h = self.fresh();
                let _ = writeln!(self.out, "  %{h} = call ptr @nz.arr_new()");
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{h}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span.start,
                );
                return Val::Of {
                    value: h,
                    ty: op.ret(),
                };
            }
            Builtin::IntsLen | Builtin::StrsLen => {
                let n = self.fresh();
                // `len` is the header's first field, so the handle points straight at it.
                let _ = writeln!(self.out, "  %{n} = load i64, ptr %{}", arg(0));
                return Val::Of {
                    value: n,
                    ty: Ty::Int,
                };
            }
            Builtin::IntsGet | Builtin::StrsGet => {
                let elem = op.sequence().and_then(Ty::element).unwrap_or(Ty::Int);
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                let v = self.fresh();
                let _ = writeln!(self.out, "  %{v} = load {}, ptr %{slot}", elem.llvm());
                return Val::Of { value: v, ty: elem };
            }
            Builtin::IntsSet | Builtin::StrsSet => {
                let elem = op.sequence().and_then(Ty::element).unwrap_or(Ty::Int);
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                let _ = writeln!(self.out, "  store {} %{}, ptr %{slot}", elem.llvm(), arg(2));
                // The index written. The language has no unit type, and the index is
                // the one answer that is always meaningful.
                return Val::Of {
                    value: arg(1),
                    ty: Ty::Int,
                };
            }
            Builtin::StrJoin => {
                self.needs_sequences = true;
                self.needs_join = true;
                let (sp, sn) = self.str_parts(arg(1));
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {{ ptr, i64 }} @nz.str_join(ptr %{}, ptr %{sp}, i64 %{sn})",
                    arg(0)
                );
                let got = self.fresh();
                let _ = writeln!(self.out, "  %{got} = extractvalue {{ ptr, i64 }} %{out}, 0");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp eq ptr %{got}, null");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span.start,
                );
                return Val::Of {
                    value: out,
                    ty: Ty::Str,
                };
            }
            // A channel is a pointer to a runtime header holding a bounded queue, a
            // mutex and a condition variable. Every one of these four is a
            // synchronisation point, which is why a `Chan` may cross into a task and a
            // sequence may not (`docs/spec.md`, *Structured concurrency*).
            Builtin::ChanNew => {
                self.needs_channels = true;
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp slt i64 %{}, 1", arg(0));
                self.require(bad, "a channel needs a capacity of at least 1", span);
                let c = self.fresh();
                let _ = writeln!(self.out, "  %{c} = call ptr @nz.chan_new(i64 %{})", arg(0));
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{c}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span.start,
                );
                return Val::Of {
                    value: c,
                    ty: Ty::Chan,
                };
            }
            Builtin::ChanSend => {
                self.needs_channels = true;
                let ok = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{ok} = call i1 @nz.chan_send(ptr %{}, i64 %{})",
                    arg(0),
                    arg(1)
                );
                return Val::Of {
                    value: ok,
                    ty: Ty::Bool,
                };
            }
            // The received value is pushed onto the sequence by the helper rather than
            // returned and pushed here, because the push happens only when there **was**
            // a value and a call site cannot branch without a block of its own. The
            // failure text is passed in: the helper is shared and the position is not.
            Builtin::ChanRecv => {
                self.needs_channels = true;
                self.needs_sequences = true;
                let (sym, len) = self.failure_text(
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span.start,
                );
                let ok = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{ok} = call i1 @nz.chan_recv(ptr %{}, ptr %{}, ptr {sym}, i64 {len})",
                    arg(0),
                    arg(1)
                );
                return Val::Of {
                    value: ok,
                    ty: Ty::Bool,
                };
            }
            Builtin::ChanClose => {
                self.needs_channels = true;
                let _ = writeln!(self.out, "  call void @nz.chan_close(ptr %{})", arg(0));
                // The language has no unit type and `chan_close` is specified to produce
                // `Int`. Zero, materialised rather than invented: a constant cannot be
                // returned as an SSA value without an instruction to hold it.
                let z = self.fresh();
                let _ = writeln!(self.out, "  %{z} = add i64 0, 0");
                return Val::Of {
                    value: z,
                    ty: Ty::Int,
                };
            }
            Builtin::IntsPop | Builtin::StrsPop => {
                let seq = op.sequence().unwrap_or(Ty::Ints);
                let elem = seq.element().unwrap_or(Ty::Int);
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let empty = self.fresh();
                let _ = writeln!(self.out, "  %{empty} = icmp sle i64 %{len}, 0");
                self.require(empty, "this sequence is empty", span);
                let last = self.fresh();
                let _ = writeln!(self.out, "  %{last} = sub i64 %{len}, 1");
                let data_field = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data_field} = getelementptr i8, ptr %{}, i64 16",
                    arg(0)
                );
                let data = self.fresh();
                let _ = writeln!(self.out, "  %{data} = load ptr, ptr %{data_field}");
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {}, ptr %{data}, i64 %{last}",
                    elem.llvm()
                );
                let v = self.fresh();
                let _ = writeln!(self.out, "  %{v} = load {}, ptr %{slot}", elem.llvm());
                // The length is written back *after* the value is read, so the item is
                // still inside the sequence while it is being fetched. Capacity is left
                // alone: a stack that shrinks and grows should not reallocate.
                let _ = writeln!(self.out, "  store i64 %{last}, ptr %{}", arg(0));
                return Val::Of { value: v, ty: elem };
            }
            Builtin::IntsPush | Builtin::StrsPush => {
                self.needs_sequences = true;
                let seq = op.sequence().unwrap_or(Ty::Ints);
                let elem = seq.element().unwrap_or(Ty::Int);
                let size = seq.element_size().unwrap_or(8);
                let data = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data} = call ptr @nz.arr_grow(ptr %{}, i64 {size})",
                    arg(0)
                );
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{data}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span.start,
                );
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {}, ptr %{data}, i64 %{len}",
                    elem.llvm()
                );
                let _ = writeln!(self.out, "  store {} %{}, ptr %{slot}", elem.llvm(), arg(1));
                let grown = self.fresh();
                let _ = writeln!(self.out, "  %{grown} = add i64 %{len}, 1");
                let _ = writeln!(self.out, "  store i64 %{grown}, ptr %{}", arg(0));
                return Val::Of {
                    value: grown,
                    ty: Ty::Int,
                };
            }
            Builtin::StrFromByte => {
                let b = arg(0);
                let low = self.fresh();
                let _ = writeln!(self.out, "  %{low} = icmp slt i64 %{b}, 0");
                let high = self.fresh();
                let _ = writeln!(self.out, "  %{high} = icmp sgt i64 %{b}, 255");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{low}, %{high}");
                self.require(bad, "this is not a byte value", span);
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 1, 0");
                let buf = self.allocate(room, span);
                let narrow = self.fresh();
                let _ = writeln!(self.out, "  %{narrow} = trunc i64 %{b} to i8");
                let _ = writeln!(self.out, "  store i8 %{narrow}, ptr %{buf}");
                let one = self.fresh();
                let _ = writeln!(self.out, "  %{one} = add i64 1, 0");
                self.make_str(buf, one)
            }
            Builtin::IntToStr => {
                // 20 digits is the widest `Int` (`-9223372036854775808`), plus the NUL
                // `snprintf` insists on writing.
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 21, 0");
                let buf = self.allocate(room, span);
                let written = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{written} = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %{buf}, \
                     i64 21, ptr @.dec, i64 %{})",
                    arg(0)
                );
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = sext i32 %{written} to i64");
                self.make_str(buf, n)
            }
        };
        Val::Of { value, ty: Ty::Str }
    }

    /// `==` and `!=` on two `Str`s: equal lengths, then equal bytes.
    ///
    /// The length test guards the comparison rather than merely preceding it — `memcmp`
    /// is only reached when both strings really are that long, so no path reads past
    /// either buffer. Emitted as a branch and a `phi` for exactly that reason; an `and`
    /// of two computed values would have evaluated the `memcmp` first.
    fn string_equality(&mut self, a: u32, b: u32, negate: bool) -> u32 {
        let (pa, la) = self.str_parts(a);
        let (pb, lb) = self.str_parts(b);
        let same_len = self.fresh();
        let _ = writeln!(self.out, "  %{same_len} = icmp eq i64 %{la}, %{lb}");

        let compare = format!("b{}", self.fresh_label());
        let join = format!("b{}", self.fresh_label());
        let short = self.current.clone();
        let _ = writeln!(
            self.out,
            "  br i1 %{same_len}, label %{compare}, label %{join}"
        );

        self.open(&compare);
        let diff = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{diff} = call i32 @memcmp(ptr %{pa}, ptr %{pb}, i64 %{la})"
        );
        let equal = self.fresh();
        let _ = writeln!(self.out, "  %{equal} = icmp eq i32 %{diff}, 0");
        let from_compare = self.current.clone();
        let _ = writeln!(self.out, "  br label %{join}");

        self.open(&join);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi i1 [ false, %{short} ], [ %{equal}, %{from_compare} ]"
        );
        if !negate {
            return out;
        }
        let flipped = self.fresh();
        let _ = writeln!(self.out, "  %{flipped} = xor i1 %{out}, true");
        flipped
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
        let then_label = format!("b{}", self.fresh_label());
        let else_label = format!("b{}", self.fresh_label());
        let join_label = format!("b{}", self.fresh_label());
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
        let is_zero = format!("b{}", self.fresh_label());
        let compute = format!("b{}", self.fresh_label());
        let join = format!("b{}", self.fresh_label());
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
        next_label: 0,
        current: "entry".to_owned(),
        strings: Vec::new(),
        blobs: Vec::new(),
        file: file.to_owned(),
        lines: line_starts,
        source_len: source.len(),
        loops: Vec::new(),
        scopes: Vec::new(),
        tasks: String::new(),
        needs_tasks: false,
        task_count: 0,
        needs_channels: false,
        needs_sequences: false,
        needs_io: false,
        needs_join: false,
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
    let _ = writeln!(e.out, "define i32 @main(i32 %argc, ptr %argv) {{");
    let _ = writeln!(e.out, "entry:");
    if e.needs_io {
        // Stashed in globals so `args_count` and `arg` need no parameter threaded
        // through every Nazm function. One fewer than `argc`, because `argv[0]` is the
        // program's own name and is deliberately not visible.
        let _ = writeln!(e.out, "  %argc64 = sext i32 %argc to i64");
        let _ = writeln!(e.out, "  %nargs = sub i64 %argc64, 1");
        let _ = writeln!(e.out, "  store i64 %nargs, ptr @nz.argc");
        let _ = writeln!(e.out, "  store ptr %argv, ptr @nz.argv");
    }
    let _ = writeln!(e.out, "  %result = call i64 {}()", module.entry.symbol());
    let _ = writeln!(
        e.out,
        "  %print = call i32 (ptr, ...) @printf(ptr @.fmt, i64 %result)"
    );
    let _ = writeln!(e.out, "  ret i32 0");
    let _ = writeln!(e.out, "}}");

    // Both helper sets grow a sequence, so either one reaching means the sequence
    // runtime is reached too. Deriving it here rather than at each use keeps the two
    // facts from drifting apart.
    if e.needs_tasks || e.needs_channels {
        e.needs_sequences = true;
    }

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
    let _ = writeln!(
        head,
        "declare i32 @snprintf(ptr noundef, i64, ptr noundef, ...)"
    );
    let _ = writeln!(head, "declare ptr @malloc(i64)");
    let _ = writeln!(head, "declare i32 @memcmp(ptr, ptr, i64)");
    if e.needs_sequences {
        let _ = writeln!(head, "declare ptr @realloc(ptr, i64)");
    }
    if e.needs_io {
        for d in [
            "declare i64 @strlen(ptr)",
            "declare ptr @fopen(ptr, ptr)",
            "declare i32 @fseek(ptr, i64, i32)",
            "declare i64 @ftell(ptr)",
            "declare i64 @fread(ptr, i64, i64, ptr)",
            "declare i64 @fwrite(ptr, i64, i64, ptr)",
            "declare i32 @fclose(ptr)",
        ] {
            let _ = writeln!(head, "{d}");
        }
        let _ = writeln!(head, "@nz.argc = internal global i64 0");
        let _ = writeln!(head, "@nz.argv = internal global ptr null");
        let _ = writeln!(
            head,
            "@.rb = private unnamed_addr constant [3 x i8] c\"rb\\00\""
        );
        let _ = writeln!(
            head,
            "@.wb = private unnamed_addr constant [3 x i8] c\"wb\\00\""
        );
    }
    let _ = writeln!(
        head,
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)"
    );
    let _ = writeln!(head, "declare i64 @write(i32, ptr, i64)");
    let _ = writeln!(head, "declare void @exit(i32) noreturn");
    if e.needs_tasks {
        for d in [
            "declare i32 @pthread_create(ptr, ptr, ptr, ptr)",
            "declare i32 @pthread_join(ptr, ptr)",
        ] {
            let _ = writeln!(head, "{d}");
        }
    }
    if e.needs_channels {
        for d in [
            "declare i32 @pthread_mutex_init(ptr, ptr)",
            "declare i32 @pthread_mutex_lock(ptr)",
            "declare i32 @pthread_mutex_unlock(ptr)",
            "declare i32 @pthread_cond_init(ptr, ptr)",
            "declare i32 @pthread_cond_wait(ptr, ptr)",
            "declare i32 @pthread_cond_broadcast(ptr)",
        ] {
            let _ = writeln!(head, "{d}");
        }
    }
    head.push('\n');
    // `%lld\n`: the full signed 64-bit value in decimal. Encoding the result in the exit
    // status instead would truncate it to 8 bits, which for a language whose `Int` is
    // 64-bit would be a lie in the common case.
    let _ = writeln!(
        head,
        "@.fmt = private unnamed_addr constant [6 x i8] c\"%lld\\0A\\00\""
    );
    // `int_to_str`'s format. Separate from `@.fmt` because it has no newline: a `Str` is
    // the digits and nothing else.
    let _ = writeln!(
        head,
        "@.dec = private unnamed_addr constant [5 x i8] c\"%lld\\00\""
    );
    // `Str` literals: the bytes, and a constant pair naming them. An empty literal still
    // gets a byte of storage so that its pointer is valid to hand to `memcmp`.
    for (sym, bytes) in &e.blobs {
        let stored = if bytes.is_empty() { &[0u8][..] } else { bytes };
        let _ = writeln!(
            head,
            "{sym} = private unnamed_addr constant [{} x i8] c\"{}\"",
            stored.len(),
            escape_bytes(stored)
        );
        let _ = writeln!(
            head,
            "{sym}.v = private unnamed_addr constant {{ ptr, i64 }} {{ ptr {sym}, i64 {} }}",
            bytes.len()
        );
    }
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
    if e.needs_sequences {
        head.push_str(SEQUENCE_RUNTIME);
    }
    if e.needs_io {
        head.push_str(IO_RUNTIME);
    }
    if e.needs_join {
        head.push_str(JOIN_RUNTIME);
    }
    if e.needs_tasks {
        head.push_str(TASK_RUNTIME);
    }
    if e.needs_channels {
        head.push_str(CHANNEL_RUNTIME);
    }
    head.push('\n');

    // The trampolines last: they call the functions in `out`, and putting them after
    // reads in the order a person would want them — the program, then the scaffolding
    // that lets a thread enter it.
    head + &e.out + &e.tasks
}

/// `str_join`, as generated LLVM.
///
/// A loop, so it cannot be inlined at the call site the way the other built-ins are: it
/// measures every part, allocates **once**, and copies. That is the whole point — the
/// same result built with `str_concat` in a loop is quadratic in time and memory, and
/// with nothing ever freed the compiler written in Nazm exhausted memory assembling its
/// own output before this existed.
///
/// Signals allocation failure by returning a null pointer, which the call site turns
/// into `N0406` at the right span.
const JOIN_RUNTIME: &str = r"
define internal { ptr, i64 } @nz.str_join(ptr %h, ptr %sp, i64 %sn) {
entry:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %sum
sum:
  %i = phi i64 [ 0, %entry ], [ %i2, %sumbody ]
  %acc = phi i64 [ 0, %entry ], [ %acc2, %sumbody ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %sumbody, label %alloc
sumbody:
  %e = getelementptr { ptr, i64 }, ptr %data, i64 %i
  %lp = getelementptr i8, ptr %e, i64 8
  %li = load i64, ptr %lp
  %acc2 = add i64 %acc, %li
  %i2 = add i64 %i, 1
  br label %sum
alloc:
  ; One separator between each pair, so n-1 of them, and none for an empty sequence.
  %less = sub i64 %n, 1
  %neg = icmp slt i64 %less, 0
  %gaps = select i1 %neg, i64 0, i64 %less
  %sepbytes = mul i64 %gaps, %sn
  %total = add i64 %acc, %sepbytes
  ; One byte more, so an empty result still owns storage and its pointer stays valid.
  %room = add i64 %total, 1
  %buf = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %buf, null
  br i1 %bad, label %fail, label %copy
copy:
  %j = phi i64 [ 0, %alloc ], [ %j2, %tail ]
  %off = phi i64 [ 0, %alloc ], [ %off3, %tail ]
  %more2 = icmp slt i64 %j, %n
  br i1 %more2, label %body, label %done
body:
  %first = icmp eq i64 %j, 0
  br i1 %first, label %tail, label %gap
gap:
  %d1 = getelementptr i8, ptr %buf, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %d1, ptr %sp, i64 %sn, i1 false)
  %off1 = add i64 %off, %sn
  br label %tail
tail:
  %off2 = phi i64 [ %off, %body ], [ %off1, %gap ]
  %e2 = getelementptr { ptr, i64 }, ptr %data, i64 %j
  %p2 = load ptr, ptr %e2
  %l2p = getelementptr i8, ptr %e2, i64 8
  %l2 = load i64, ptr %l2p
  %d2 = getelementptr i8, ptr %buf, i64 %off2
  call void @llvm.memcpy.p0.p0.i64(ptr %d2, ptr %p2, i64 %l2, i1 false)
  %off3 = add i64 %off2, %l2
  %j2 = add i64 %j, 1
  br label %copy
done:
  %a = insertvalue { ptr, i64 } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64 } %a, i64 %total, 1
  ret { ptr, i64 } %b
fail:
  %z = insertvalue { ptr, i64 } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64 } %z, i64 0, 1
  ret { ptr, i64 } %z2
}
";

/// The file-system helpers, as generated LLVM.
///
/// Here rather than at the call sites because each is a dozen basic blocks and none of
/// them needs a source position: every failure is signalled by a null pointer or a
/// negative count, and the *call site* turns that into `N0407` at the right span. That
/// split is the same one the sequence helpers use, for the same reason.
///
/// A Nazm `Str` is not NUL-terminated — the specification makes an embedded zero an
/// ordinary byte — so `@nz.cstr` copies the path into a terminated buffer before any
/// libc call sees it. Passing the bytes directly would truncate a path at the first
/// zero, or read past the end of one that has none.
const IO_RUNTIME: &str = r"
define internal ptr @nz.cstr(ptr %p, i64 %n) {
entry:
  %room = add i64 %n, 1
  %buf = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %buf, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %p, i64 %n, i1 false)
  %end = getelementptr i8, ptr %buf, i64 %n
  store i8 0, ptr %end
  ret ptr %buf
}

define internal i1 @nz.file_exists(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %no, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %no, label %yes
yes:
  %ignored = call i32 @fclose(ptr %f)
  ret i1 true
no:
  ret i1 false
}

define internal { ptr, i64 } @nz.read_file(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %measure
measure:
  ; SEEK_END is 2 and SEEK_SET is 0 on every target this is verified against.
  %e = call i32 @fseek(ptr %f, i64 0, i32 2)
  %len = call i64 @ftell(ptr %f)
  %unmeasurable = icmp slt i64 %len, 0
  br i1 %unmeasurable, label %closefail, label %reserve
reserve:
  %s = call i32 @fseek(ptr %f, i64 0, i32 0)
  ; One byte more than the file, so an empty file still owns storage and its pointer
  ; stays valid to compare.
  %room = add i64 %len, 1
  %buf = call ptr @malloc(i64 %room)
  %nobuf = icmp eq ptr %buf, null
  br i1 %nobuf, label %closefail, label %read
read:
  %got = call i64 @fread(ptr %buf, i64 1, i64 %len, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %len
  br i1 %short, label %fail, label %done
done:
  %a = insertvalue { ptr, i64 } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64 } %a, i64 %len, 1
  ret { ptr, i64 } %b
closefail:
  %c2 = call i32 @fclose(ptr %f)
  br label %fail
fail:
  %z = insertvalue { ptr, i64 } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64 } %z, i64 0, 1
  ret { ptr, i64 } %z2
}

define internal i64 @nz.write_file(ptr %pp, i64 %pn, ptr %dp, i64 %dn) {
entry:
  %path = call ptr @nz.cstr(ptr %pp, i64 %pn)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.wb)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %write
write:
  %got = call i64 @fwrite(ptr %dp, i64 1, i64 %dn, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %dn
  br i1 %short, label %fail, label %done
done:
  ret i64 %got
fail:
  ret i64 -1
}
";

/// The two sequence helpers, as generated LLVM rather than a linked runtime crate.
///
/// Generated because that is the smallest thing that works: `docs/architecture.md` keeps
/// `nazm-rt` for when a runtime genuinely needs stack switching and a custom allocator,
/// and neither of these does. They are emitted only into programs that use a sequence.
///
/// **Bounds checks are not here.** They live at each call site, because the diagnostic
/// has to name that call's source position and a shared helper has only one. What is
/// here is allocation and growth, which have no position to report — both signal failure
/// by returning null, and the call site turns that into `N0406`.
///
/// The header is `{ i64 len, i64 cap, ptr data }`, 24 bytes. The handle is its address
/// and never moves, which is what makes a sequence a reference: growth rewrites `data`
/// and `cap` in place and every holder sees it.
const SEQUENCE_RUNTIME: &str = r"
define internal ptr @nz.arr_new() {
entry:
  %h = call ptr @malloc(i64 24)
  %bad = icmp eq ptr %h, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  store i64 0, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  store i64 0, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  store ptr null, ptr %datap
  ret ptr %h
}

define internal ptr @nz.arr_grow(ptr %h, i64 %elem) {
entry:
  %len = load i64, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  %cap = load i64, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %grow, label %done
grow:
  ; Doubling, with a floor of four. `realloc(null, n)` is `malloc(n)`, so the first
  ; growth needs no special case.
  %twice = mul i64 %cap, 2
  %tiny = icmp slt i64 %twice, 4
  %want = select i1 %tiny, i64 4, i64 %twice
  %bytes = mul i64 %want, %elem
  %old = load ptr, ptr %datap
  %new = call ptr @realloc(ptr %old, i64 %bytes)
  %failed = icmp eq ptr %new, null
  br i1 %failed, label %oom, label %kept
kept:
  ; The header is updated only after the allocation succeeded, so a failed growth
  ; leaves the sequence exactly as it was rather than half-resized.
  store ptr %new, ptr %datap
  store i64 %want, ptr %capp
  br label %done
oom:
  ret ptr null
done:
  %data = load ptr, ptr %datap
  ret ptr %data
}
";

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
        // The body transferred, which since M1f is ordinary: every path returned, so the
        // current block already holds its `ret` and **nothing more may be written**.
        // Appending `unreachable` here — as this arm did while the case could not arise —
        // would give the block two terminators and produce a module clang rejects.
        //
        // If a block somehow reached here without a terminator it would have none at
        // all, and clang would say so loudly. That is the right failure; the alternative,
        // `unreachable` on a path control really takes, is undefined behaviour in
        // silence.
        Val::Transferred => {}
    }
    let _ = writeln!(e.out, "}}");
    e.out.push('\n');
}

/// The same escaping for arbitrary bytes, which a `Str` literal may contain.
fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes {
        if *b == b'"' || *b == b'\\' || !(0x20..0x7f).contains(b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(*b as char);
        }
    }
    out
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

/// Starting a task, and waiting for one, as generated LLVM.
///
/// # The slot is reserved before the thread starts
///
/// `@nz.spawn` records the task **first** and undoes the record if `pthread_create`
/// fails. A thread that started and could not be recorded would never be joined, and a
/// scope that cannot join a task it started is not a scope — so the failure that must not
/// happen is "started but untracked", and this is the order that makes it impossible.
///
/// # What is stored
///
/// One pointer per task: the argument block, whose first eight bytes `pthread_create`
/// writes the thread handle into. The block is allocated by the spawn site anyway, so
/// using its head as the handle's storage costs nothing and keeps the list to one
/// allocation.
///
/// Returns `0` on success, `-1` when the list could not grow, and `1` when the thread
/// could not be started. The call site turns those into `N0406` and `N0404`.
const TASK_RUNTIME: &str = r"
define internal i32 @nz.spawn(ptr %h, ptr %fn, ptr %blk) {
entry:
  %data = call ptr @nz.arr_grow(ptr %h, i64 8)
  %oom = icmp eq ptr %data, null
  br i1 %oom, label %nomem, label %record
nomem:
  ret i32 -1
record:
  %len = load i64, ptr %h
  %slot = getelementptr i64, ptr %data, i64 %len
  %raw = ptrtoint ptr %blk to i64
  store i64 %raw, ptr %slot
  %next = add i64 %len, 1
  store i64 %next, ptr %h
  %rc = call i32 @pthread_create(ptr %blk, ptr null, ptr %fn, ptr %blk)
  %bad = icmp ne i32 %rc, 0
  br i1 %bad, label %undo, label %ok
undo:
  store i64 %len, ptr %h
  ret i32 1
ok:
  ret i32 0
}

define internal void @nz.scope_join(ptr %h) {
entry:
  ; The count is read once. Nothing can be added while this runs: the scope's body has
  ; finished by every path that reaches here, and a task cannot spawn into its parent.
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %ep = getelementptr i64, ptr %data, i64 %i
  %raw = load i64, ptr %ep
  %blk = inttoptr i64 %raw to ptr
  %tid = load ptr, ptr %blk
  %rc = call i32 @pthread_join(ptr %tid, ptr null)
  %next = add i64 %i, 1
  br label %test
done:
  ret void
}
";

/// A bounded channel, as generated LLVM: one mutex, one condition variable, one ring.
///
/// The header is `{ i64 cap, i64 len, i64 head, i64 closed, ptr data, ptr mutex, ptr cond }`.
///
/// # One condition variable, broadcast every time
///
/// Senders and receivers wait on the same variable and every change wakes all of them.
/// Not the fastest arrangement and the obviously correct one — `docs/spec.md` promises no
/// fairness, and a signal-one scheme has to be right about *which* one, which is where
/// the lost-wakeup bugs live. The interpreter's `Channel` is built the same way, so the
/// two agree by construction rather than by testing.
///
/// # The waits are loops
///
/// `pthread_cond_wait` may return without the condition holding. Every wait here is
/// inside a loop that re-reads the state, which is the only correct way to use one.
const CHANNEL_RUNTIME: &str = r"
define internal ptr @nz.chan_new(i64 %cap) {
entry:
  %c = call ptr @malloc(i64 56)
  %bad = icmp eq ptr %c, null
  br i1 %bad, label %fail, label %ring
fail:
  ret ptr null
ring:
  %bytes = mul i64 %cap, 8
  %d = call ptr @malloc(i64 %bytes)
  %badd = icmp eq ptr %d, null
  br i1 %badd, label %fail, label %lock
lock:
  ; Sized generously on purpose: pthread_mutex_t and pthread_cond_t are opaque and
  ; differently sized on every platform, and there is no header to ask from here.
  ; Over-allocating is what makes this portable without a size to get wrong.
  %m = call ptr @malloc(i64 128)
  %badm = icmp eq ptr %m, null
  br i1 %badm, label %fail, label %cond
cond:
  %v = call ptr @malloc(i64 128)
  %badv = icmp eq ptr %v, null
  br i1 %badv, label %fail, label %init
init:
  %mi = call i32 @pthread_mutex_init(ptr %m, ptr null)
  %vi = call i32 @pthread_cond_init(ptr %v, ptr null)
  store i64 %cap, ptr %c
  %lenp = getelementptr i8, ptr %c, i64 8
  store i64 0, ptr %lenp
  %headp = getelementptr i8, ptr %c, i64 16
  store i64 0, ptr %headp
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 0, ptr %closedp
  %datap = getelementptr i8, ptr %c, i64 32
  store ptr %d, ptr %datap
  %mp = getelementptr i8, ptr %c, i64 40
  store ptr %m, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  store ptr %v, ptr %vp
  ret ptr %c
}

define internal i1 @nz.chan_send(ptr %c, i64 %v) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; Closed is tested first and on every pass, so a channel closed while this sender
  ; waited refuses rather than queueing into something nobody will drain.
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %refuse, label %space
space:
  %len = load i64, ptr %lenp
  %cap = load i64, ptr %c
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %wait, label %store
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
refuse:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
store:
  %head = load i64, ptr %headp
  %sum = add i64 %head, %len
  %idx = urem i64 %sum, %cap
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %idx
  store i64 %v, ptr %slot
  %n1 = add i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 true
}

define internal i1 @nz.chan_recv(ptr %c, ptr %seq, ptr %msg, i64 %mn) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; A queued value wins over the close flag, which is what makes closing end a channel
  ; rather than discard what is in it.
  %len = load i64, ptr %lenp
  %empty = icmp sle i64 %len, 0
  br i1 %empty, label %maybe, label %take
maybe:
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %drained, label %wait
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
drained:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
take:
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %head
  %val = load i64, ptr %slot
  %h1 = add i64 %head, 1
  %nh = urem i64 %h1, %cap
  store i64 %nh, ptr %headp
  %n1 = sub i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ; The push happens after the unlock. Growing a sequence may call `realloc`, and holding
  ; a channel's mutex across an allocation would make every other task on it wait for the
  ; allocator.
  %grown = call ptr @nz.arr_grow(ptr %seq, i64 8)
  %oom = icmp eq ptr %grown, null
  br i1 %oom, label %nomem, label %push
nomem:
  call void @nz.fail(ptr %msg, i64 %mn)
  unreachable
push:
  %sn = load i64, ptr %seq
  %sslot = getelementptr i64, ptr %grown, i64 %sn
  store i64 %val, ptr %sslot
  %sn1 = add i64 %sn, 1
  store i64 %sn1, ptr %seq
  ret i1 true
}

define internal void @nz.chan_close(ptr %c) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 1, ptr %closedp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u = call i32 @pthread_mutex_unlock(ptr %m)
  ret void
}
";

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

        // The C entry point takes `argc`/`argv` since B3; what matters here is that it
        // is a separate, external wrapper around Nazm's `main`, not its signature.
        assert!(
            ir.contains("define i32 @main(i32 %argc, ptr %argv) {"),
            "{ir}"
        );
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
