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

use nazm_sema::key::{DefKey, DefKind};
use nazm_sema::module::{ModuleGraph, ModuleId};

use crate::ir::{
    fits, ArithOp, Block, Builtin, CmpOp, Expr, External, Function, Local, Lowering as _, Native,
    RecordLayout, RecordTy, Stmt, Ty, Unit,
};
use crate::symbol::{Linkage, NativeSymbol, NativeTypeName};

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
///
/// Its symbol rather than a number: a call may cross a module boundary, and the thing on
/// the other side of that boundary is a linker symbol. See [`crate::symbol`].
#[derive(Debug, Clone)]
struct Sig {
    symbol: NativeSymbol,
    /// Which module declares it, so a unit knows which of these it owns and which it
    /// must declare.
    module: ModuleId,
    linkage: Linkage,
    params: Vec<Ty>,
    ret: Ty,
}

/// What the whole compilation settled before any module was lowered.
///
/// `docs/architecture.md` calls this **global backend admissibility**, and it is global
/// because two of its questions are: whether this backend can represent a signature, which
/// a caller in another module needs to know, and whether the call graph has a cycle, which
/// can run through any number of modules. Per-module lowering reads it and adds nothing to
/// it.
#[derive(Debug)]
pub struct Plan {
    /// By [`FnId`], the signature this backend accepted — or `None` where it refused one,
    /// which is what tells a refused user function apart from a built-in.
    sig: Vec<Option<Sig>>,
    /// By [`RecordTy`], how that record is laid out.
    ///
    /// Global, and computed once: two units that mention one record must lay it out
    /// identically or the objects they produce will not link, so the layout is settled
    /// here rather than by each unit for itself. Each unit then publishes the subset it
    /// mentions, which is what keeps one unit's text independent of another's records.
    records: Vec<RecordLayout>,
    entry: NativeSymbol,
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
    /// What the global pass settled. Read, never added to.
    plan: &'a Plan,
    /// The module being lowered. Everything a body names that this module does not own is
    /// somebody else's definition, and becomes an external reference.
    module: ModuleId,
    /// Definitions this module calls and does not own, collected as they are found.
    externals: Vec<External>,
    /// Record types this unit's code mentions, collected as they are found.
    ///
    /// A set rather than a list, because one record is mentioned many times and the unit
    /// must carry it once. Closed over field types at the end: a record this unit names is
    /// laid out in its LLVM text, and a field of record type makes that definition refer
    /// to another named type which must also be defined here.
    used_records: std::collections::BTreeSet<u32>,
    /// Locals of the function being lowered, in slot order, taken from the resolution.
    locals: Vec<Local>,
    /// Call edges, as **resolved definition identities**: the cycle check is a question
    /// about the program's functions, not about one unit's numbering, and a cycle may run
    /// through any number of modules.
    calls: Vec<(FnId, FnId, Span)>,
    /// The function whose body is being lowered, for recording edges.
    current: FnId,
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

/// How every record in this compilation is laid out.
///
/// One table, computed before any unit is lowered, for the reason `docs/spec.md` gives
/// under *Separate code generation*: two objects that pass a record between them must
/// agree about it, and the only way to be sure is that neither decided.
///
/// The order is **canonical by field name**, which is the layout rule. Nothing about the
/// author's declaration order reaches the object file, so moving two field declarations —
/// an edit the language says means nothing — changes no byte of it.
fn record_layouts(res: &Resolution, graph: &ModuleGraph) -> Vec<RecordLayout> {
    res.records()
        .all()
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = RecordTy(u32::try_from(i).unwrap_or(u32::MAX));
            // Derived from the durable identity where there is one, exactly as a function
            // symbol is, and from load order where there is not — for a module that has
            // no durable identity, which still compiles.
            let name = match graph.key(def.module) {
                Some(key) => NativeTypeName::of_definition(&DefKey::new(
                    key.clone(),
                    DefKind::Record,
                    def.name.clone(),
                )),
                None => NativeTypeName::of_unkeyed(def.module, &def.name),
            };
            let fields: Vec<Ty> = def.in_layout_order().map(|(_, f)| Ty::of(f.ty)).collect();
            let mut of_declaration = vec![0_u32; def.fields.len()];
            for (position, (declared, _)) in def.in_layout_order().enumerate() {
                if let Some(slot) = of_declaration.get_mut(declared as usize) {
                    *slot = u32::try_from(position).unwrap_or(u32::MAX);
                }
            }
            RecordLayout {
                id,
                name,
                source: def.name.clone(),
                fields,
                of_declaration,
            }
        })
        .collect()
}

impl Ctx<'_> {
    /// Note that this unit's code mentions a type, so its definition travels with it.
    ///
    /// Called wherever a type enters the unit — a local, a signature, an expression's
    /// result. A record whose name never appears in the text is not carried, which is what
    /// keeps this unit's object independent of records declared elsewhere.
    fn note(&mut self, ty: Ty) {
        if let Ty::Record(id) = ty {
            self.used_records.insert(id.0);
        }
    }

    /// The layout of a record this compilation declared.
    fn layout(&self, id: RecordTy) -> Option<&RecordLayout> {
        self.plan.records.get(id.index())
    }

    /// Where the field declared at `declared` sits in the LLVM struct.
    ///
    /// **The layout boundary.** Everything above this holds a declaration index, which is
    /// how a field's semantic identity is spelled in one session; everything below holds a
    /// position. This function is the only place the two meet.
    fn field_index(&self, id: RecordTy, declared: u32) -> Option<u32> {
        self.layout(id)?
            .of_declaration
            .get(declared as usize)
            .copied()
    }

    /// Add a hidden local, and return its slot.
    ///
    /// For a record being assembled: `docs/spec.md` requires a construction that fails
    /// part way through to release what it has already taken, and a value in a register is
    /// not reachable from the frame's cleanup. Named so that `--emit-ir` shows what it is.
    fn hidden(&mut self, ty: Ty, what: &str) -> usize {
        let slot = self.locals.len();
        self.locals.push(Local {
            name: format!("{what}#{slot}"),
            ty,
        });
        slot
    }

    /// `what` is a short noun phrase. The sentence supplies the rest, and the help
    /// supplies the subset — a phrase carrying its own clause reads as two sentences
    /// spliced together, which is how the first version of these messages read.
    fn unsupported(&mut self, span: Span, what: &str) {
        self.errors.push(unsupported_at(span, what));
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
        self.plan.sig.get(id.index()).and_then(Option::as_ref)
    }

    /// Record that this unit calls a definition, declaring it if another module owns it.
    ///
    /// The one place a cross-module reference is made, and what it makes is a **symbol**:
    /// there is no name here to look up and nothing downstream that could.
    fn refer_to(&mut self, sig: &Sig) {
        if sig.module == self.module {
            return;
        }
        let external = External {
            symbol: sig.symbol.clone(),
            params: sig.params.clone(),
            ret: sig.ret,
        };
        if !self.externals.contains(&external) {
            self.externals.push(external);
        }
    }
}

/// Settle what the backend can represent, for the whole compilation.
///
/// # Why this is global when lowering is not
///
/// Two questions cannot be answered inside one module. *Can this backend represent that
/// signature* has to be answered before a caller in another module can emit a call to it;
/// and *does the call graph contain a cycle* can run through any number of modules, so a
/// per-module pass would have to rediscover a fact it cannot see. `docs/architecture.md`
/// keeps both here rather than weakening the recursion policy to make lowering local.
///
/// # Errors
///
/// A `main` this backend refuses, or a program with none.
pub fn plan(program: &Program, res: &Resolution, graph: &ModuleGraph) -> Result<Plan, LowerError> {
    let mut errors = Vec::new();
    let mut sig: Vec<Option<Sig>> = vec![None; res.functions().len()];

    // Driven by the **resolution**, not by the syntax tree: each definition says which
    // declaration it came from, so pairing the two needs no name. A name stopped being
    // able to do the job when two modules became able to declare the same one.
    for (i, def) in res.functions().iter().enumerate() {
        let id = FnId::from_index(i);
        let Some(f) = program.functions.get(def.decl as usize) else {
            continue;
        };
        if let Some(accepted) = signature(res, graph, id, f, &mut errors) {
            if let Some(slot) = sig.get_mut(id.index()) {
                *slot = Some(accepted);
            }
        }
    }

    let records = record_layouts(res, graph);

    let entry = match res
        .entry()
        .and_then(|id| sig.get(id.index()))
        .and_then(Option::as_ref)
    {
        Some(s) => s.symbol.clone(),
        // `main` has no signature here for one of two reasons, and they are different
        // facts. Either the program genuinely has none — unreachable after a successful
        // check, which reports `N0204` itself — or this backend *refused* `main`'s
        // signature, in which case the refusal is already in `errors` and reporting a
        // missing `main` on top of it would be a second, false diagnostic about a
        // function the reader can see.
        None if !errors.is_empty() => {
            return Err(LowerError {
                diagnostics: errors,
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

    // The root module's `main` is reached from the entry wrapper, which lives in a unit of
    // its own — so it must be bindable across objects even though the language keeps it
    // private. The one place native linkage does not follow `pub`, and it is about where a
    // process starts rather than about what a module offers.
    if let Some(id) = res.entry() {
        if let Some(Some(s)) = sig.get_mut(id.index()) {
            s.linkage = Linkage::Hidden;
        }
    }

    if errors.is_empty() {
        Ok(Plan {
            sig,
            records,
            entry,
        })
    } else {
        Err(LowerError {
            diagnostics: errors,
        })
    }
}

/// What lowering one module produced.
#[derive(Debug)]
pub struct Lowered {
    pub unit: Unit,
    /// Call edges found in this module's bodies, as resolved definition identities. The
    /// cycle check is a question about the whole compilation and is answered once, after
    /// every module has contributed.
    pub calls: Vec<(FnId, FnId, Span)>,
    pub errors: Vec<Diagnostic>,
}

/// Lower one module's bodies, and nothing else.
///
/// # What it may see
///
/// Its own functions' bodies, and every function's *signature* through [`Plan`]. It never
/// reads another module's body, and there is nothing here through which it could: a call
/// into another module becomes an [`External`] — a symbol and a signature — which is
/// exactly what a linker needs and exactly what a dependency's implementation is not.
pub fn lower_module(
    program: &Program,
    res: &Resolution,
    plan: &Plan,
    module: ModuleId,
    source: &str,
) -> Lowered {
    let mut ctx = Ctx {
        res,
        plan,
        module,
        externals: Vec::new(),
        used_records: std::collections::BTreeSet::new(),
        locals: Vec::new(),
        calls: Vec::new(),
        current: FnId::from_index(0),
        ret_ty: Ty::Int,
        loop_depth: 0,
        errors: Vec::new(),
    };

    let mut functions = Vec::new();
    for (i, def) in res.functions().iter().enumerate() {
        if def.module != module {
            continue;
        }
        let id = FnId::from_index(i);
        let Some(f) = program.functions.get(def.decl as usize) else {
            continue;
        };
        if ctx.sig(id).is_none() {
            continue;
        }
        if let Some(lowered) = lower_function(&mut ctx, id, f) {
            functions.push(lowered);
        }
    }

    // Sorted, so the declarations a unit carries do not depend on the order its bodies
    // happened to mention them. Two builds of one module produce one text.
    ctx.externals.sort();

    // Closed over field types: a named LLVM type whose definition refers to another must
    // have that one defined beside it. Iterative, and bounded by the number of records.
    let mut work: Vec<u32> = ctx.used_records.iter().copied().collect();
    while let Some(id) = work.pop() {
        let Some(layout) = plan.records.get(id as usize) else {
            continue;
        };
        for f in &layout.fields {
            if let Ty::Record(inner) = f {
                if ctx.used_records.insert(inner.0) {
                    work.push(inner.0);
                }
            }
        }
    }
    let records: Vec<RecordLayout> = ctx
        .used_records
        .iter()
        .filter_map(|id| plan.records.get(*id as usize).cloned())
        .collect();

    Lowered {
        unit: Unit {
            module,
            source: source.to_owned(),
            functions,
            externals: ctx.externals,
            records,
        },
        calls: ctx.calls,
        errors: ctx.errors,
    }
}

/// Lower a checked program into one native unit per module, or explain what stopped it.
///
/// # The pipeline
///
/// ```text
/// resolved compilation → plan (global) → lower each module → cycle check (global)
/// ```
///
/// # Errors
///
/// *Every* construct outside the subset, not just the first: one run of `nazm build`
/// should say everything that would have to change, which is the same
/// one-mistake-one-error discipline the rest of the front end holds to.
pub fn lower(
    program: &Program,
    res: &Resolution,
    graph: &ModuleGraph,
    names: &crate::names::SourceNames<'_>,
) -> Result<Native, LowerError> {
    let settled = plan(program, res, graph)?;

    let mut units = Vec::new();
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    for module in graph.ids() {
        // The module's durable identity where it has one, which is what makes the
        // artefact the same bytes from any checkout. `crate::names` owns the rule.
        let source = graph.file(module).map_or("", |f| names.name(f));
        let mut lowered = lower_module(program, res, &settled, module, source);
        units.push(lowered.unit);
        calls.append(&mut lowered.calls);
        errors.append(&mut lowered.errors);
    }

    // The call graph is complete only now, so the cycle check comes last — and it is over
    // every module at once, because `A::f → B::g → A::f` is a cycle no one module can see.
    report_cycles(res, &calls, &mut errors);

    if errors.is_empty() {
        Ok(Native {
            units,
            entry: settled.entry,
        })
    } else {
        Err(LowerError {
            diagnostics: errors,
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
fn signature(
    res: &Resolution,
    graph: &ModuleGraph,
    id: FnId,
    f: &AstFn,
    errors: &mut Vec<Diagnostic>,
) -> Option<Sig> {
    let def = res.function(id)?;
    let params: Vec<Ty> = def.params.iter().map(|t| Ty::of(*t)).collect();
    let ret = Ty::of(def.ret);
    let mut ok = true;

    // Only the **root** module's `main` is an entry point, and only a compilation has a
    // root; an imported module's `main` is one of its definitions and nothing more. So the
    // shape rules below are asked of the entry and not of every function spelled `main`,
    // which is `docs/spec.md`'s rule expressed where the backend can act on it.
    if res.entry() == Some(id) {
        if !f.params.is_empty() {
            errors.push(unsupported_at(f.span, "a `main` with parameters"));
            ok = false;
        }
        if ret != Ty::Int {
            errors.push(unsupported_at(
                f.return_type.span,
                "a `main` that does not return `Int`",
            ));
            ok = false;
        }
    }

    // The symbol comes from the definition's durable identity, and from nothing about
    // this compilation. See [`crate::symbol`].
    let symbol = match graph.key(def.module) {
        Some(key) => NativeSymbol::of_definition(&DefKey::new(
            key.clone(),
            DefKind::Function,
            def.name.clone(),
        )),
        None => NativeSymbol::of_unkeyed(def.module, &def.name),
    };

    // Linkage follows the language's own visibility and nothing else: a private
    // definition cannot be named from another module, so nothing in another object can
    // refer to it, so it need not be in a symbol table at all. `plan` makes the one
    // exception, for the root entry the platform wrapper calls.
    let linkage = if def.visibility.is_exported() {
        Linkage::Hidden
    } else {
        Linkage::Internal
    };

    ok.then_some(Sig {
        symbol,
        module: def.module,
        linkage,
        params,
        ret,
    })
}

/// This backend's "not yet", wherever it is raised.
///
/// One function rather than two so that a refusal from the global pass and one from a
/// module's body are the same diagnostic — same code, same label, same help — instead of
/// two that drift.
fn unsupported_at(span: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::UNSUPPORTED_CONSTRUCT,
        span,
        format!("{what} is not in `nazm build`'s subset yet"),
    )
    .primary_label("not in the native subset")
    .help(SUBSET)
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
        symbol,
        linkage,
        ret,
        ..
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
    // Every local's type is mentioned by this unit's text: it is the type of an `alloca`.
    let local_types: Vec<Ty> = ctx.locals.iter().map(|l| l.ty).collect();
    for t in local_types {
        ctx.note(t);
    }
    ctx.note(ret);
    ctx.current = id;
    ctx.ret_ty = ret;
    // No loop target crosses a function boundary, so the depth starts at zero here and
    // not wherever the previous function left it.
    ctx.loop_depth = 0;

    let params = f.params.len();
    let body = f.body.as_ref()?;
    let block = lower_block(ctx, body)?;
    // No tail is legitimate now: a body every path of which returns has nothing to fall
    // off the end with, and `check_function` accepts exactly that by treating
    // `Completion::Diverges` as satisfying the signature. Emission writes no terminator
    // for such a body, because the `ret` is already there.
    let Some(result) = block.result.as_ref() else {
        return Some(Function {
            symbol,
            linkage,
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
        symbol,
        linkage,
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
/// Refuse a call graph that can reach itself, anywhere in the compilation.
///
/// # Why this is whole-program and stays whole-program
///
/// `A::f → B::g → A::f` is a cycle no module can see, and per-module code generation does
/// not make it stop being one. So this reads the edges every module contributed and
/// answers once, over **resolved definition identities** rather than any unit's numbering
/// — `docs/architecture.md` names it as the analysis that remains global, and weakening
/// the recursion policy to make lowering local would have been trading a language rule
/// for an implementation convenience.
fn report_cycles(res: &Resolution, calls: &[(FnId, FnId, Span)], errors: &mut Vec<Diagnostic>) {
    let n = res.functions().len();
    let mut edges: Vec<Vec<(usize, Span)>> = vec![Vec::new(); n];
    for (from, to, span) in calls {
        if let Some(e) = edges.get_mut(from.index()) {
            e.push((to.index(), *span));
        }
    }

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
            let edge = edges.get(node).and_then(|e| e.get(*next)).copied();
            match edge {
                Some((to, span)) => {
                    *next += 1;
                    match mark.get(to).copied() {
                        // Back edge: `node` can reach itself through `to`.
                        Some(Mark::Open) => {
                            if !reported.get(to).copied().unwrap_or(true) {
                                if let Some(r) = reported.get_mut(to) {
                                    *r = true;
                                }
                                let name = res
                                    .function(FnId::from_index(to))
                                    .map_or("this function", |f| f.name.as_str());
                                let what = if to == node {
                                    format!("a call to `{name}` from inside itself")
                                } else {
                                    format!("a call cycle through `{name}`")
                                };
                                errors.push(
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
    let place = &a.place;

    let Some((slot, ty)) = ctx.slot_at(place.name_span) else {
        // Unreachable: the checker resolved this name and recorded the slot. Reaching
        // here means the resolution is missing an entry it should have, which is a
        // compiler bug and worth reporting as one rather than as "unsupported".
        ctx.errors.push(
            Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                place.name_span,
                format!("`{}` is not a local the native backend can see", place.name),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    };

    // The projection path, one step at a time, each turned from a field identity into a
    // position. `target` is what the assignment actually writes.
    let mut path: Vec<(u32, Ty)> = Vec::with_capacity(place.path.len());
    let mut target = ty;
    for step in &place.path {
        let to = ctx.res.field(step.name_span)?;
        let Ty::Record(rid) = target else {
            ctx.unsupported(
                place.span,
                "a field written on something that is not a record",
            );
            return None;
        };
        let at = ctx.field_index(rid, to.field)?;
        let field_ty = ctx
            .layout(rid)
            .and_then(|l| l.fields.get(at as usize).copied())?;
        ctx.note(field_ty);
        path.push((at, field_ty));
        target = field_ty;
    }

    let value = value?;
    if !fits(&value, target) {
        // Unreachable after checking, which requires an assignment to match the place's
        // declared type (`N0300`). Reported rather than assumed away: a mismatched store
        // is a miscompile, and LLVM would reject the module for a reason that named none
        // of this.
        ctx.unsupported(
            a.span,
            &format!(
                "an assignment of `{}` to `{}`, which is `{}`",
                value.ty().map_or("nothing", Ty::name),
                place.name,
                target.name()
            ),
        );
        return None;
    }

    if path.is_empty() {
        Some(Stmt::Assign { slot, value })
    } else {
        Some(Stmt::SetField { slot, path, value })
    }
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
        AstExpr::Construct {
            ty_span, fields, ..
        } => lower_construct(ctx, *ty_span, fields),
        AstExpr::Field {
            base,
            name_span,
            span,
            ..
        } => lower_field(ctx, base, *name_span, *span),
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
    let Some(sig) = ctx.sig(fid).cloned() else {
        ctx.unsupported(
            sp.callee_span,
            &format!(
                "a task running `{}`, whose signature this backend refused",
                sp.callee
            ),
        );
        return None;
    };

    let Sig {
        symbol,
        params,
        ret,
        ..
    } = sig.clone();
    ctx.refer_to(&sig);

    // A task is an edge in the call graph like any other. `fn f() { scope { spawn f(); } }`
    // is recursion, and the fact that it goes through a thread does not make the stack
    // depth bounded.
    ctx.calls.push((ctx.current, fid, sp.callee_span));

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
        callee: symbol,
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
    let Some(sig) = resolved else {
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

    let Sig {
        symbol,
        ret,
        params,
        ..
    } = sig.clone();
    ctx.refer_to(&sig);

    // Recorded whether or not the arguments lowered: the cycle check is about the call
    // graph, and an edge exists as soon as the source names one.
    if let Some(Callee::Function(to)) = target {
        ctx.calls.push((ctx.current, to, callee_span));
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
        callee: symbol,
        args,
        ret,
    })
}

/// `Point(x: 1, y: 2)`.
///
/// Which record, and which field each initialiser fills, are **read from the resolution**:
/// the checker decided both, and deciding them again here would be the second resolver
/// N1 removed. What is decided here — and only here — is where each field sits.
fn lower_construct(
    ctx: &mut Ctx<'_>,
    ty_span: Span,
    fields: &[nazm_syntax::FieldInit],
) -> Option<Expr> {
    let id = ctx.res.construction(ty_span)?;
    let ty = Ty::of(nazm_sema::Type::Record(id));
    ctx.note(ty);
    let Ty::Record(rid) = ty else { return None };

    // In the order **written**, which is the order side effects happen in. The layout
    // index rides along, so emission never has to know the field's name.
    let mut lowered = Vec::with_capacity(fields.len());
    for f in fields {
        let declared = ctx
            .res
            .records()
            .get(id)
            .and_then(|d| d.field(&f.name).map(|(i, _)| i))?;
        let at = ctx.field_index(rid, declared)?;
        let value = lower_expr(ctx, &f.value)?;
        lowered.push((at, value));
    }

    // A slot only where a field owns something. A record of `Int`s cannot leak a partial
    // construction, so it does not pay for the machinery that stops one.
    let slot = ctx
        .res
        .records()
        .needs_cleanup(nazm_sema::Type::Record(id))
        .then(|| ctx.hidden(ty, "record"));

    Some(Expr::Record {
        ty,
        fields: lowered,
        slot,
    })
}

/// `base.name`.
fn lower_field(ctx: &mut Ctx<'_>, base: &AstExpr, name_span: Span, span: Span) -> Option<Expr> {
    let to = ctx.res.field(name_span)?;
    let base_lowered = lower_expr(ctx, base)?;
    let base_ty = base_lowered.ty()?;
    let Ty::Record(rid) = base_ty else {
        // Unreachable after checking, which refuses a projection on anything else
        // (`N0335`). Reported rather than assumed away.
        ctx.unsupported(span, "a field read from something that is not a record");
        return None;
    };
    let at = ctx.field_index(rid, to.field)?;
    let ty = ctx
        .layout(rid)
        .and_then(|l| l.fields.get(at as usize).copied())?;
    ctx.note(base_ty);
    ctx.note(ty);
    Some(Expr::Field {
        base: Box::new(base_lowered),
        base_ty,
        at,
        ty,
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
