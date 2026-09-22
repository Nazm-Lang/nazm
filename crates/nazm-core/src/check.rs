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
use nazm_syntax::{
    Assign, BinOp, Block, Expr, FieldInit, Function, Program, Record, Return, Spawn, Stmt, While,
};

use nazm_sema::interface::{reachable_records, Export, RecordExport, UnitInterface, Visibility};
use nazm_sema::intrinsic::Intrinsic;
use nazm_sema::module::{Dependency, ModuleGraph, ModuleId};
use nazm_sema::record::{FieldDef, RecordDef, RecordId, Records, MAX_RECORD_DEPTH};
use nazm_sema::resolve::{Callee, FieldRef, FnDef, FnId, LocalDef, Resolution};
use nazm_sema::types::Type;

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
    /// `nazm_sema::Resolution`.
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

/// A name a module may write, and what writing it means.
#[derive(Debug, Clone)]
struct Visible {
    sig: Signature,
    callee: Callee,
}

/// What one module may name in the type namespace.
///
/// Built-in type names are not in here: they are parsed by [`Type::from_name`] and cannot
/// be shadowed, which is the rule `docs/spec.md` states as *built-ins are not a module's
/// definitions*. This holds records only — this module's own, public and private alike,
/// and the ones its direct imports export.
#[derive(Debug, Default)]
struct Types {
    names: HashMap<String, RecordId>,
}

/// What one module may name.
///
/// Built-ins, this module's own definitions — public and private alike — and the exports
/// of the modules it **directly** imports. Nothing else: not a transitive dependency's
/// exports, not another module's private definitions, and not a definition from a module
/// that happens to be in the same compilation.
///
/// This replaced a `HashMap<String, Signature>` built once over every function in the
/// program. That map was the global namespace: it could not represent two modules each
/// defining `helper`, and it gave the same answer to "what does `parse` mean" regardless
/// of who asked. An environment is per module because the answer is.
#[derive(Debug, Default)]
struct Env {
    names: HashMap<String, Visible>,
}

impl Env {
    fn get(&self, name: &str) -> Option<&Visible> {
        self.names.get(name)
    }

    fn contains(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }
}

/// One module's declarations, as the checker receives them.
///
/// The functions written in one file, the module they belong to, and what that module
/// imports. Assembled from a `Program` because that is where the trees are; nothing here
/// needs the other modules' trees, which is the property the whole milestone is about.
#[derive(Debug)]
pub struct Unit<'a> {
    module: ModuleId,
    /// Each function of this module, with its index in `Program::functions`.
    functions: Vec<(u32, &'a Function)>,
    /// Each record of this module, with its index in `Program::records`.
    records: Vec<(u32, &'a Record)>,
    imports: &'a [Dependency],
}

impl<'a> Unit<'a> {
    /// The part of `program` that belongs to `module`.
    #[must_use]
    pub fn of(program: &'a Program, graph: &'a ModuleGraph, module: ModuleId) -> Self {
        let functions = program
            .functions
            .iter()
            .enumerate()
            .filter(|(_, f)| graph.module_of(f.name_span.file) == Some(module))
            .map(|(i, f)| (u32::try_from(i).unwrap_or(u32::MAX), f))
            .collect();
        let records = program
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| graph.module_of(r.name_span.file) == Some(module))
            .map(|(i, r)| (u32::try_from(i).unwrap_or(u32::MAX), r))
            .collect();
        Self {
            module,
            functions,
            records,
            imports: graph.imports(module),
        }
    }

    #[must_use]
    pub fn module(&self) -> ModuleId {
        self.module
    }
}

#[derive(Debug)]
pub struct Checker<'a> {
    /// What resolution decided, and the single answer every consumer reads.
    ///
    /// Borrowed, not owned: one resolution spans the whole compilation while each unit is
    /// checked separately into it. Two resolutions would be two answers.
    res: &'a mut Resolution,
    /// The module whose bodies are being checked.
    module: ModuleId,
    /// What that module may name, in the function namespace.
    env: Env,
    /// What that module may name, in the **type** namespace.
    ///
    /// A second map rather than a second kind of entry in the first, because
    /// `docs/spec.md` gives types and functions separate namespaces: a module may declare
    /// a record `Point` and a function `Point`, and one map could not hold both. Where
    /// the grammar puts a name decides which map is consulted, and the grammar always
    /// knows.
    types: Types,
    /// The modules it directly imports, for the diagnostic that tells a reader a
    /// definition exists and is private rather than that it does not exist.
    imports: Vec<ModuleId>,
    /// The function whose body is being checked, for slot allocation.
    current: Option<FnId>,
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

/// Which modules' bodies actually have to be checked.
///
/// # Why the decision is a parameter
///
/// A driver that holds a cache can know that a module's inputs are unchanged, and
/// therefore that checking its bodies would reach the conclusion it reached last time.
/// That is a decision about **reuse**, which needs a filesystem and a store; checking is a
/// decision about **meaning**, which must not. Splitting them here keeps one checking
/// authority — [`check_unit`] stays the only thing that decides what a body means — while
/// letting the driver say which bodies it needs decided.
///
/// The default, [`CheckEverything`], is what every caller but the cache uses, and what
/// `--no-cache` restores. Nothing downstream can tell the difference: a compilation that
/// skipped work and one that did not produce the same diagnostics, because a module is
/// only ever skippable when its answer is already known.
///
/// # The contract an implementor must keep
///
/// [`Reuse::must_check`] may return `false` for a module **only** if every input to
/// checking that module is identical to one whose check produced no diagnostics. What
/// those inputs are is `nazm_cache::CheckKey`'s subject; getting the set wrong is a wrong
/// answer, not a slow one.
pub trait Reuse {
    /// Every module's declared interface, before any body is checked.
    ///
    /// Called once. This is where an implementor computes whatever it needs to answer
    /// [`Reuse::must_check`] — notably each module's `InterfaceHash`, which is a function
    /// of its own declarations and therefore available for every module at once, cycles
    /// included.
    ///
    /// `records` is every record the compilation declared, which a driver needs to
    /// publish an interface: since N9 an exported signature may name one, and a record's
    /// shape is not in the interface value itself.
    fn plan(&mut self, graph: &ModuleGraph, interfaces: &[UnitInterface], records: &Records);

    /// Whether this module's bodies must be checked.
    fn must_check(&self, module: ModuleId) -> bool;

    /// This module's bodies were checked, and the module produced no diagnostic at all.
    ///
    /// Only then: an artefact means *this checked cleanly*, so a module with a diagnostic
    /// produces none and a refusal can never be cached.
    fn checked_clean(&mut self, module: ModuleId);
}

/// Check every module. The behaviour every caller but the cache has.
#[derive(Debug, Default, Clone, Copy)]
pub struct CheckEverything;

impl Reuse for CheckEverything {
    fn plan(&mut self, _graph: &ModuleGraph, _interfaces: &[UnitInterface], _records: &Records) {}

    fn must_check(&self, _module: ModuleId) -> bool {
        true
    }

    fn checked_clean(&mut self, _module: ModuleId) {}
}

/// Check a compilation. Returns what resolution decided and every diagnostic found.
///
/// # Two stages, and why they are two
///
/// 1. **Declarations**, for every module. A module's own declarations need nothing from
///    any other module: a signature is written out in full, so reading one requires no
///    inference and no ordering.
/// 2. **Bodies**, each module against its own definitions and the *interfaces* of the
///    modules it directly imports.
///
/// Neither stage needs modules in a particular order, which is exactly why an import cycle
/// remains legal — see `docs/spec.md`. It is also why this function is thin: the work is
/// in [`declare_unit`] and [`check_unit`], which are the same functions a single unit is
/// checked with. There is no second path that resolves names differently for the
/// whole-program case.
#[must_use]
pub fn check(program: &Program, graph: &ModuleGraph) -> (Resolution, Vec<Diagnostic>) {
    check_reusing(program, graph, &mut CheckEverything, &[])
}

/// As [`check`], letting a driver skip the bodies of modules whose answer it already has.
///
/// `unparsable` names the modules whose source did not parse cleanly. They are always
/// checked and never reported as clean: a tree built from a file with a syntax error is a
/// partial tree, and an interface derived from one is a partial interface that no later
/// run should inherit.
///
/// # What stage 1 still does for every module
///
/// Everything. Declarations are read, definitions are registered, duplicates and unknown
/// types are reported, and the root module is asked for `main` — all of it from source,
/// every time. Reuse applies to stage 2 only, which is where the bodies are and where the
/// work is. That is what makes a skipped module invisible: the facts every other module
/// and the compilation itself depend on were recomputed anyway.
#[must_use]
pub fn check_reusing(
    program: &Program,
    graph: &ModuleGraph,
    reuse: &mut dyn Reuse,
    unparsable: &[ModuleId],
) -> (Resolution, Vec<Diagnostic>) {
    let mut res = Resolution::default();
    let mut errors = Vec::new();

    let units: Vec<Unit<'_>> = graph.ids().map(|m| Unit::of(program, graph, m)).collect();

    // `declared_cleanly` is per module, because "this module checked" has to mean every
    // stage. A module whose declarations collide and whose bodies are fine is not a
    // module that checked.
    //
    // Two of the stages below are whole-graph — record names, and layout validation over
    // the containment graph — so an error in either makes **every** module unclean. That
    // is conservative and it is the only correct answer: an artefact means *this module
    // checked cleanly*, and a compilation with an unresolved type cycle has no module of
    // which that is true.
    let mut declared_cleanly: Vec<bool> = vec![true; units.len()];
    let mark = |cleanly: &mut Vec<bool>, n: usize, before: usize, now: usize| {
        if now != before {
            if let Some(slot) = cleanly.get_mut(n) {
                *slot = false;
            }
        }
    };

    // Stage 0a: every record's name, for the whole graph. Before any field type, because
    // a field may name a record declared later or in another module.
    let before = errors.len();
    let type_interfaces = declare_records(&units, &mut res, &mut errors);
    if errors.len() != before {
        declared_cleanly.iter_mut().for_each(|c| *c = false);
    }

    // Stage 0b: each module's field types, against the type half of its imports'
    // interfaces. Stage 0c: the layouts those fields describe.
    for (n, u) in units.iter().enumerate() {
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| type_interfaces.get(d.on.index()))
            .collect();
        let before = errors.len();
        resolve_record_fields(u, &deps, &mut res, &mut errors);
        mark(&mut declared_cleanly, n, before, errors.len());
    }
    let before = errors.len();
    validate_record_layouts(&mut res, &mut errors);
    if errors.len() != before {
        // A layout cycle may span modules, so every module is conservatively unclean:
        // an artefact means *this checked*, and none of them did.
        declared_cleanly.iter_mut().for_each(|c| *c = false);
    }

    // Stage 1, for the whole graph before any body is checked. A cycle needs this: A's
    // body may call B while B's calls A, and neither ordering of the two would work.
    let interfaces: Vec<UnitInterface> = units
        .iter()
        .enumerate()
        .map(|(n, u)| {
            let deps: Vec<&UnitInterface> = u
                .imports
                .iter()
                .filter_map(|d| type_interfaces.get(d.on.index()))
                .collect();
            let before = errors.len();
            let interface = declare_unit(u, &deps, &mut res, &mut errors);
            mark(&mut declared_cleanly, n, before, errors.len());
            interface
        })
        .collect();

    reuse.plan(graph, &interfaces, res.records());

    // Stage 2, module by module. `deps` is the *direct* imports and nothing else, so a
    // name only reachable transitively is not in scope — by construction rather than by a
    // rule someone has to remember.
    for (n, u) in units.iter().enumerate() {
        let unparsed = unparsable.contains(&u.module);
        if !unparsed && !reuse.must_check(u.module) {
            continue;
        }
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| interfaces.get(d.on.index()))
            .collect();
        let before = errors.len();
        check_unit(u, &deps, &mut res, &mut errors);
        if !unparsed && errors.len() == before && declared_cleanly.get(n) == Some(&true) {
            reuse.checked_clean(u.module);
        }
    }

    // The **root** module owns `main`. Another module's `main` is one of its definitions
    // and nothing more, which is why a helper file may have one.
    if res.definition(graph.root(), "main").is_none() {
        errors.push(
            Diagnostic::error(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "this program has no `main` function",
            )
            .help("`nazm run` evaluates `fn main() -> Int` in the file you named"),
        );
    }

    (res, errors)
}

/// Stage 0a for the whole compilation: declare every record's **name**.
///
/// Names before field types, for every module, because a field type may name a record
/// declared later in the file or in another module — `docs/spec.md` makes declaration
/// order irrelevant, and that is only true if every name exists before any type is
/// resolved. It is N2's declare-then-check, one stage earlier, and the returned
/// interfaces are the *type* half of what a dependent needs.
///
/// The interfaces returned carry records and imports and no exports: at this point no
/// record has fields, so there is nothing to close over and no signature to publish.
/// [`declare_unit`] returns the whole thing once both are known.
pub fn declare_records(
    units: &[Unit<'_>],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<UnitInterface> {
    let mut interfaces = Vec::with_capacity(units.len());
    for unit in units {
        let mut exported = Vec::new();
        for (decl, r) in &unit.records {
            // A built-in's name is not a module's to define. Unlike a duplicate, there is
            // no declaration to point at, so the reason is said rather than shown.
            if Type::from_name(&r.name).is_some() {
                errors.push(
                    Diagnostic::error(
                        codes::BUILTIN_TYPE_REDEFINED,
                        r.name_span,
                        format!("`{}` is a built-in type and cannot be redefined", r.name),
                    )
                    .help(
                        "built-in type names mean the same thing in every module of every \
                         program, so none of them may be shadowed; choose another name",
                    ),
                );
                continue;
            }
            if let Some(first) = res
                .records()
                .declared(unit.module, &r.name)
                .and_then(|id| res.records().get(id))
            {
                errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_TYPE,
                        r.name_span,
                        format!("`{}` is declared more than once in this module", r.name),
                    )
                    .secondary(first.name_span, "first declared here"),
                );
                continue;
            }
            let visibility = Visibility::from_keyword(r.pub_span);
            let id = res.records_mut().declare(RecordDef {
                name: r.name.clone(),
                module: unit.module,
                visibility,
                name_span: r.name_span,
                fields: Vec::new(),
                layout: Vec::new(),
                decl: *decl,
            });
            if visibility.is_exported() {
                exported.push(RecordExport {
                    def: id,
                    nameable: true,
                });
            }
        }
        interfaces.push(UnitInterface::new(
            unit.module,
            Vec::new(),
            exported,
            unit.imports.to_vec(),
        ));
    }
    interfaces
}

/// Stage 0b for one module: resolve its records' field types.
///
/// `deps` are the type-carrying interfaces of the modules this one directly imports —
/// what [`declare_records`] returned, selected by the caller. Their *bodies* are not a
/// parameter and never will be, which is the same claim [`check_unit`] makes one stage
/// later.
pub fn resolve_record_fields(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let types = type_environment(unit, deps, res, errors, Collisions::Report);
    // Only the first declaration of a name gets fields: a duplicate was already reported
    // and must not overwrite the shape the winner has.
    let mut completed: Vec<&str> = Vec::new();
    for (_, r) in &unit.records {
        let Some(id) = res.records().declared(unit.module, &r.name) else {
            continue;
        };
        if completed.contains(&r.name.as_str()) {
            continue;
        }
        completed.push(&r.name);
        let mut fields: Vec<FieldDef> = Vec::new();
        for f in &r.fields {
            if let Some(first) = fields.iter().find(|e| e.name == f.name) {
                errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("`{}` has two fields called `{}`", r.name, f.name),
                    )
                    .secondary(first.name_span, "first declared here")
                    .help("a field is identified by its name, so two cannot share one"),
                );
                continue;
            }
            let ty = resolve_type_in(&types, res, &f.ty.name, f.ty.span, errors);
            fields.push(FieldDef {
                name: f.name.clone(),
                ty,
                name_span: f.name_span,
                ty_span: f.ty.span,
            });
        }
        // A record with no fields. Refused rather than allowed, and the reason is the
        // grammar: `Marker()` is exactly what a call to a function named `Marker` looks
        // like, and types and functions are separate namespaces, so the two forms would
        // be genuinely ambiguous. `docs/spec.md` records the decision and what would make
        // it reversible.
        if fields.is_empty() && !r.fields.is_empty() {
            // Every field was a duplicate; those are already reported.
        } else if fields.is_empty() {
            errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    r.name_span,
                    format!("`{}` has no fields, which is not supported yet", r.name),
                )
                .help(
                    "a record with no fields would be built by writing `Name()`, which is \
                     exactly how a call to a function of that name is written — and types \
                     and functions are separate namespaces, so neither reading could be \
                     preferred. Give it a field, or use `Bool`",
                ),
            );
        }
        res.records_mut().complete(id, fields);
    }
}

/// Stage 0c for the whole compilation: refuse records that cannot be laid out.
///
/// A record contains its fields by value, so a chain of record-typed fields that returns
/// to where it started describes a value containing itself. `docs/spec.md` refuses it as
/// an **impossible layout** rather than as a safety problem, and states the rule over
/// *inline containment* so that a future indirection can break a cycle without the rule
/// changing.
///
/// Reported before anything lowers, so LLVM never sees a recursive type — a backend
/// complaining about one is a refusal that arrives after the compiler decided the program
/// was fine.
pub fn validate_record_layouts(res: &mut Resolution, errors: &mut Vec<Diagnostic>) {
    let mut reported = vec![false; res.records().len()];
    for i in 0..res.records().len() {
        let id = RecordId::from_index(i);
        if reported.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(cycle) = res.records().layout_cycle(id) else {
            continue;
        };
        // One diagnostic per cycle, not one per record in it.
        for r in &cycle.through {
            if let Some(slot) = reported.get_mut(r.index()) {
                *slot = true;
            }
        }
        let names: Vec<&str> = cycle
            .through
            .iter()
            .filter_map(|r| res.records().get(*r))
            .map(|d| d.name.as_str())
            .collect();
        let (closing, at) = cycle.via;
        let Some(field) = res
            .records()
            .get(closing)
            .and_then(|d| d.fields.get(at as usize))
        else {
            continue;
        };
        let Some(start) = res.records().get(id) else {
            continue;
        };
        let message = if names.len() == 2 {
            format!("`{}` contains itself", start.name)
        } else {
            format!(
                "`{}` contains itself through {}",
                start.name,
                names.join(" → ")
            )
        };
        errors.push(
            Diagnostic::error(codes::RECURSIVE_RECORD, start.name_span, message)
                .secondary(field.name_span, "this field closes the cycle")
                .help(
                    "a record holds its fields by value, so this one would have no finite \
                     size. This is an impossible layout rather than a safety problem: a \
                     later type that holds its contents somewhere else would make the same \
                     shape expressible",
                ),
        );
    }

    // Depth is only meaningful once there is no cycle to walk forever.
    for i in 0..res.records().len() {
        let id = RecordId::from_index(i);
        if reported.get(i).copied().unwrap_or(true) {
            continue;
        }
        if res.records().depth(Type::Record(id)).is_none() {
            let Some(def) = res.records().get(id) else {
                continue;
            };
            errors.push(
                Diagnostic::error(
                    codes::TYPE_NESTING_TOO_DEEP,
                    def.name_span,
                    format!("records may contain records {MAX_RECORD_DEPTH} deep"),
                )
                .help(
                    "each level multiplies the copy and cleanup code generated for every \
                     value of the outer record, so the bound is on the emitted program \
                     rather than on the compiler. It is stated rather than discovered",
                ),
            );
        }
    }
}

/// The type names one module may write.
///
/// Its own records — public and private, since a module sees all of its own — then the
/// records each direct import makes nameable. Both ways a name can collide are reported
/// here, with the same wording and the same philosophy as the function namespace's, and
/// for the same reason: an ambiguity is refused where it is created.
/// Whether a call to [`type_environment`] is the one that reports name collisions.
///
/// Three stages build the same environment — field resolution, signature declaration and
/// body checking — and a collision is one mistake, so exactly one of them says so.
/// `docs/spec.md` refuses an ambiguity where it is created, and the earliest stage that
/// can see it is where it is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Collisions {
    Report,
    Silent,
}

fn type_environment(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
    collisions: Collisions,
) -> Types {
    let mut names: HashMap<String, RecordId> = HashMap::new();
    for (_, r) in &unit.records {
        if let Some(id) = res.records().declared(unit.module, &r.name) {
            names.insert(r.name.clone(), id);
        }
    }

    let mut brought_in: HashMap<String, Span> = HashMap::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        for e in iface.records().iter().filter(|e| e.nameable) {
            let Some(def) = res.records().get(e.def) else {
                continue;
            };
            if let Some(own) = res
                .records()
                .declared(unit.module, &def.name)
                .and_then(|id| res.records().get(id))
            {
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::IMPORT_COLLIDES_WITH_DEFINITION,
                            d.at,
                            format!(
                                "this import brings in the type `{}`, which this module \
                                 declares",
                                def.name
                            ),
                        )
                        .secondary(own.name_span, "declared here")
                        .secondary(def.name_span, "and exported here")
                        .help(
                            "an imported type and a local one are not ranked: rename one \
                             of them, or stop exporting the other",
                        ),
                    );
                }
                continue;
            }
            if let Some(first) = brought_in.get(&def.name) {
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::AMBIGUOUS_IMPORT,
                            d.at,
                            format!(
                                "the type `{}` is exported by two imported modules",
                                def.name
                            ),
                        )
                        .secondary(*first, "also brought in here")
                        .secondary(def.name_span, "exported here")
                        .help(
                            "there is no way to write which one is meant, so the \
                             ambiguity is refused where it is created rather than where \
                             it is used",
                        ),
                    );
                }
                continue;
            }
            brought_in.insert(def.name.clone(), d.at);
            names.insert(def.name.clone(), e.def);
        }
    }
    Types { names }
}

/// A type name, against the built-ins and then this module's visible records.
///
/// Built-ins first and unshadowable, which is why they are matched before the map is
/// consulted rather than being put in it.
fn resolve_type_in(
    types: &Types,
    res: &Resolution,
    name: &str,
    span: Span,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    if let Some(t) = Type::from_name(name) {
        return t;
    }
    if let Some(id) = types.names.get(name) {
        return Type::Record(*id);
    }
    let mut d = Diagnostic::error(codes::UNKNOWN_TYPE, span, format!("`{name}` is not a type"))
        .help(format!(
            "the built-in types are {}, and a `struct` declares another",
            Type::all_names()
        ));
    // The same courtesy the function namespace extends: a type that exists somewhere is
    // more usefully reported as unreachable than as non-existent.
    if let Some(elsewhere) = res
        .records()
        .declared_anywhere(name)
        .next()
        .and_then(|id| res.records().get(id))
    {
        d = d.secondary(elsewhere.name_span, "a type of this name is declared here");
    }
    errors.push(d);
    // Continue as `Int` rather than abandoning: one unknown type should not suppress
    // every other error around it.
    Type::Int
}

/// Stage 1 for one module: declare what it defines, and say what it offers.
///
/// Reads this module's declarations and nothing else — no other module's source, no other
/// module's interface. That is what makes the stage orderable in any order, including one
/// that contains a cycle.
pub fn declare_unit(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> UnitInterface {
    let types = type_environment(unit, deps, res, errors, Collisions::Silent);
    let mut exports = Vec::new();

    for (decl, f) in &unit.functions {
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| resolve_type_in(&types, res, &p.ty.name, p.ty.span, errors))
            .collect();
        let ret = resolve_type_in(&types, res, &f.return_type.name, f.return_type.span, errors);

        // A built-in has no source span to point at, so "first defined here" would aim
        // at byte zero. Said plainly instead. Built-ins are not a module's definitions,
        // so no visibility rule makes this one legal.
        if Intrinsic::by_name(&f.name).is_some() {
            errors.push(
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
            continue;
        }

        // Within this module. Another module defining the same name is another module's
        // definition, and since 2026-09-22 that is not a collision.
        if let Some(existing) = res
            .definition(unit.module, &f.name)
            .and_then(|id| res.function(id))
        {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    format!("`{}` is defined more than once in this module", f.name),
                )
                .secondary(existing.name_span, "first defined here"),
            );
            continue;
        }

        let visibility = Visibility::from_keyword(f.pub_span);
        let id = res.declare(FnDef {
            name: f.name.clone(),
            module: unit.module,
            visibility,
            params: params.clone(),
            ret,
            name_span: f.name_span,
            decl: *decl,
            locals: Vec::new(),
        });

        if visibility.is_exported() {
            // A public signature naming a private record would publish a type the
            // importer cannot resolve. Checked at each public item on its *direct*
            // references, which is the recursion `docs/spec.md` asks for expressed
            // distributively: an intermediate record must itself be public to appear
            // here, and its own declaration is checked the same way — so one mistake
            // produces exactly one diagnostic, at the place the private name is written.
            for (t, at) in params
                .iter()
                .zip(f.params.iter().map(|p| p.ty.span))
                .chain(std::iter::once((&ret, f.return_type.span)))
            {
                report_private_type(*t, at, f.name_span, "function", &f.name, res, errors);
            }
            exports.push(Export {
                name: f.name.clone(),
                params,
                ret,
                name_span: f.name_span,
                def: id,
            });
        }
    }

    // The same check for an exported record's own fields, and the interface's record set.
    let mut records: Vec<RecordExport> = Vec::new();
    let mut roots: Vec<Type> = Vec::new();
    for (_, r) in &unit.records {
        let Some(id) = res.records().declared(unit.module, &r.name) else {
            continue;
        };
        let Some(def) = res.records().get(id) else {
            continue;
        };
        if !def.visibility.is_exported() {
            continue;
        }
        let name = def.name.clone();
        let name_span = def.name_span;
        let fields: Vec<(Type, Span)> = def.fields.iter().map(|f| (f.ty, f.ty_span)).collect();
        for (t, at) in fields {
            report_private_type(t, at, name_span, "record", &name, res, errors);
        }
        records.push(RecordExport {
            def: id,
            nameable: true,
        });
        roots.push(Type::Record(id));
    }
    // Closed over what the public surface mentions: an importer that projects through an
    // exported record needs every shape it can reach, and cannot go and read the module
    // that declared one. `RecordExport` says why this is not transitive visibility.
    roots.extend(exports.iter().flat_map(|e| {
        e.params
            .iter()
            .copied()
            .chain(std::iter::once(e.ret))
            .collect::<Vec<_>>()
    }));
    for id in reachable_records(res.records(), roots) {
        if !records.iter().any(|r| r.def == id) {
            records.push(RecordExport {
                def: id,
                nameable: false,
            });
        }
    }

    UnitInterface::new(unit.module, exports, records, unit.imports.to_vec())
}

/// Report a public item that names a private record.
///
/// `kind` and `name` describe the **public** item, so the message says what is leaking
/// and the labels say from where to where.
fn report_private_type(
    t: Type,
    at: Span,
    item_span: Span,
    kind: &str,
    name: &str,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let Type::Record(id) = t else { return };
    let Some(def) = res.records().get(id) else {
        return;
    };
    if def.visibility.is_exported() {
        return;
    }
    errors.push(
        Diagnostic::error(
            codes::PRIVATE_TYPE_EXPOSED,
            at,
            format!(
                "this public {kind} exposes `{}`, which is private to this module",
                def.name
            ),
        )
        .secondary(item_span, format!("`{name}` is public"))
        .secondary(def.name_span, "declared without `pub` here")
        .help(
            "an importer would have to name this type to use the item and cannot. Export \
             the type with `pub`, or stop exporting the item",
        ),
    );
}

/// Stage 2 for one module: check its bodies against its environment.
///
/// `deps` are the interfaces of the modules this one **directly** imports. Their bodies
/// are not a parameter, are not reachable from one, and are not read. That sentence is the
/// whole claim `docs/spec.md` makes about compilation units, and this signature is where
/// it is either true or not.
pub fn check_unit(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let env = environment(unit, deps, res, errors);
    // Built from `deps` rather than from a graph, so the one path that checks a module
    // against persisted interfaces resolves type names by exactly the rule the
    // whole-program path uses. Two builders would be two rules.
    let types = type_environment(unit, deps, res, errors, Collisions::Silent);
    let imports = unit.imports.iter().map(|d| d.on).collect();
    let mut c = Checker {
        res,
        module: unit.module,
        env,
        types,
        imports,
        current: None,
        scopes: Scopes::default(),
        errors: Vec::new(),
        return_type: None,
        scope_depth: 0,
        loop_depth: 0,
    };
    for (_, f) in &unit.functions {
        c.check_function(f);
    }
    errors.append(&mut c.errors);
}

/// The names one module may write.
///
/// Built-ins, then its own definitions, then the exports of each direct import — in that
/// order, and every conflict between the last two is reported rather than resolved.
fn environment(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Env {
    let mut names: HashMap<String, Visible> = HashMap::new();

    // Built-ins first, so a user definition of one collides with it rather than
    // shadowing it. They share the ordinary function namespace deliberately: a call is a
    // call, and `str_len(s)` should resolve by the same rule as any other. They belong to
    // no module, so every module has all of them.
    for i in Intrinsic::ALL {
        names.insert(
            i.name().to_owned(),
            Visible {
                sig: Signature {
                    params: i.params().to_vec(),
                    ret: i.ret(),
                    name_span: Span::synthetic(),
                },
                callee: Callee::Builtin(i),
            },
        );
    }

    // This module's own definitions, exported or not. A module sees all of its own.
    for (_, f) in &unit.functions {
        let Some(id) = res.definition(unit.module, &f.name) else {
            continue;
        };
        let Some(def) = res.function(id) else {
            continue;
        };
        names.insert(
            def.name.clone(),
            Visible {
                sig: Signature {
                    params: def.params.clone(),
                    ret: def.ret,
                    name_span: def.name_span,
                },
                callee: Callee::Function(id),
            },
        );
    }

    // The exports of each direct import, and both ways that can go wrong. `brought_in`
    // remembers which `use` introduced each name, so the second collision points at the
    // first rather than at "somewhere else".
    let mut brought_in: HashMap<&str, Span> = HashMap::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        for e in iface.exports() {
            if let Some(own) = res
                .definition(unit.module, &e.name)
                .and_then(|id| res.function(id))
            {
                errors.push(
                    Diagnostic::error(
                        codes::IMPORT_COLLIDES_WITH_DEFINITION,
                        d.at,
                        format!(
                            "this import brings in `{}`, which this module defines",
                            e.name
                        ),
                    )
                    .secondary(own.name_span, "defined here")
                    .secondary(e.name_span, "and exported here")
                    .help(
                        "an imported name and a local one are not ranked: rename one of \
                         them, or stop exporting the other",
                    ),
                );
                continue;
            }
            if let Some(first) = brought_in.get(e.name.as_str()) {
                errors.push(
                    Diagnostic::error(
                        codes::AMBIGUOUS_IMPORT,
                        d.at,
                        format!("`{}` is exported by two imported modules", e.name),
                    )
                    .secondary(*first, "also brought in here")
                    .secondary(e.name_span, "exported here")
                    .help(
                        "there is no way to write which one is meant, so the ambiguity \
                         is refused where it is created rather than where it is used",
                    ),
                );
                continue;
            }
            brought_in.insert(e.name.as_str(), d.at);
            names.insert(
                e.name.clone(),
                Visible {
                    sig: Signature {
                        params: e.params.clone(),
                        ret: e.ret,
                        name_span: e.name_span,
                    },
                    callee: Callee::Function(e.def),
                },
            );
        }
    }

    Env { names }
}

impl Checker<'_> {
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
    fn resolve_callee(&mut self, name: &str, at: Span) -> Option<Signature> {
        let visible = self.env.get(name)?.clone();
        self.res.record_call(at, visible.callee);
        Some(visible.sig)
    }

    /// Why this module cannot write `name`, when something in the compilation has it.
    ///
    /// Returns the diagnostic to raise instead of a plain "not defined". Both cases point
    /// at the definition, because in both the reader is looking at a file that has it.
    ///
    /// Reading every module's definitions is legitimate **here and nowhere else**: this
    /// runs only after resolution has already failed, and it decides what to say rather
    /// than what a name means. A resolver that consulted this would be the global
    /// namespace again, which is why it lives behind a name that says so.
    fn why_invisible(&self, name: &str, at: Span) -> Option<Diagnostic> {
        let mut elsewhere = None;
        for id in self.res.defined_anywhere(name) {
            let Some(def) = self.res.function(id) else {
                continue;
            };
            if self.imports.contains(&def.module) && !def.visibility.is_exported() {
                return Some(
                    Diagnostic::error(
                        codes::PRIVATE_DEFINITION,
                        at,
                        format!("`{name}` is private to the module that defines it"),
                    )
                    .secondary(def.name_span, "defined privately here")
                    .help(format!(
                        "write `pub fn {name}` there to export it, if it is meant to be \
                         part of that module's interface"
                    )),
                );
            }
            if !self.imports.contains(&def.module)
                && def.module != self.module
                && def.visibility.is_exported()
            {
                elsewhere = Some(def.name_span);
            }
        }
        // Exported by a module this one does not import. Almost always a missing `use`,
        // and almost always because the name used to be reachable through someone else's
        // import — which is exactly what stopped being true.
        elsewhere.map(|span| {
            Diagnostic::error(
                codes::UNDEFINED_FUNCTION,
                at,
                format!("`{name}` is not defined in this module"),
            )
            .secondary(span, "exported here, by a module this one does not import")
            .help("imports are not transitive: add a `use` for the module that exports it")
        })
    }

    /// How a type is spelled in a message.
    ///
    /// A `String` rather than a `Display` adapter because the table it reads is behind the
    /// same `&mut self` most of these sites already hold. A built-in spells itself; a
    /// record spells its declared name, which is not its identity — two modules may each
    /// declare `Thing`, and a message that says `Thing` is right about what was written.
    fn show(&self, t: Type) -> String {
        self.res.records().show(t).to_owned()
    }

    fn resolve_type(&mut self, name: &str, span: Span) -> Type {
        // Split borrow: the environment and the error list are both behind `self`.
        let mut errors = std::mem::take(&mut self.errors);
        let t = resolve_type_in(&self.types, self.res, name, span, &mut errors);
        self.errors = errors;
        t
    }

    /// Which record a type name written in an expression refers to, if it is one.
    ///
    /// Reports through [`Self::resolve_type`], so an unknown name is one diagnostic
    /// wherever it was written.
    fn resolve_record(&mut self, name: &str, span: Span) -> Option<RecordId> {
        match self.resolve_type(name, span) {
            Type::Record(id) => Some(id),
            other => {
                // A known type that is not a record: `Int(x: 1)`. Distinguished from an
                // unknown name, which `resolve_type` has already reported.
                if Type::from_name(name).is_some() {
                    let shown = self.show(other);
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNKNOWN_FIELD,
                            span,
                            format!("`{shown}` is not a record, so it has no fields to name"),
                        )
                        .help(
                            "this form builds a record. A built-in type is built by its \
                             own operations",
                        ),
                    );
                }
                None
            }
        }
    }

    /// Check a record construction and return the record it builds.
    ///
    /// Initialisers are evaluated **in the order written**, which `docs/spec.md` fixes
    /// and which this order of checking mirrors: a diagnostic about the second field
    /// comes after one about the first, and so does the side effect at runtime.
    fn check_construction(
        &mut self,
        ty: &str,
        ty_span: Span,
        fields: &[FieldInit],
        span: Span,
    ) -> Option<Type> {
        let Some(id) = self.resolve_record(ty, ty_span) else {
            // Still check the initialisers: one unknown type should not silence every
            // error inside it.
            for f in fields {
                self.check_expr(&f.value, None);
            }
            return None;
        };
        self.res.record_construction(ty_span, id);

        let mut given: Vec<(String, Span)> = Vec::new();
        for f in fields {
            let declared = self
                .res
                .records()
                .get(id)
                .and_then(|d| d.field(&f.name).map(|(_, fd)| (fd.ty, fd.name_span)));
            let Some((want, declared_at)) = declared else {
                let name = self.res.records().show(Type::Record(id)).to_owned();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{name}` has no field `{}`", f.name),
                    )
                    .secondary(
                        self.res.records().get(id).map_or(ty_span, |d| d.name_span),
                        "declared here",
                    ),
                );
                self.check_expr(&f.value, None);
                continue;
            };
            if let Some((_, first)) = given.iter().find(|(g, _)| *g == f.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("`{}` is given a value twice", f.name),
                    )
                    .secondary(*first, "first given here")
                    .help("every field is given exactly one value; there is no update form"),
                );
                self.check_expr(&f.value, None);
                continue;
            }
            given.push((f.name.clone(), f.name_span));
            let got = self.value_type(&f.value, Some(want));
            if got != want {
                let (swant, sgot) = (self.show(want), self.show(got));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        f.value.span(),
                        format!("`{}` is `{swant}`, but this is `{sgot}`", f.name),
                    )
                    .secondary(declared_at, format!("declared `{swant}` here")),
                );
            }
        }

        let missing: Vec<String> = self.res.records().get(id).map_or_else(Vec::new, |d| {
            d.fields
                .iter()
                .filter(|f| !given.iter().any(|(g, _)| *g == f.name))
                .map(|f| f.name.clone())
                .collect()
        });
        if !missing.is_empty() {
            let name = self.res.records().show(Type::Record(id)).to_owned();
            let list = missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    span,
                    format!("this `{name}` does not give a value for {list}"),
                )
                .secondary(
                    self.res.records().get(id).map_or(ty_span, |d| d.name_span),
                    "declared here",
                )
                .help(
                    "every field is given a value when a record is built. There are no \
                     default values and no partial records",
                ),
            );
        }
        Some(Type::Record(id))
    }

    /// Check `base.name` and return the field's type.
    ///
    /// Field lookup begins at the base's **resolved type** and goes no further: there is
    /// no global field namespace, so a name that is a field of some other record is not a
    /// field of this one. `docs/spec.md`, *Field resolution starts from the type*.
    fn check_projection(&mut self, base_ty: Type, name: &str, name_span: Span) -> Option<Type> {
        let Type::Record(id) = base_ty else {
            let shown = self.show(base_ty);
            self.errors.push(
                Diagnostic::error(
                    codes::INVALID_PROJECTION,
                    name_span,
                    format!("`{shown}` is not a record, so `.{name}` has nothing to read"),
                )
                .help("`.` reads a declared field out of a record value"),
            );
            return None;
        };
        let found = self
            .res
            .records()
            .get(id)
            .and_then(|d| d.field(name).map(|(i, f)| (i, f.ty)));
        let Some((index, ty)) = found else {
            let shown = self.show(base_ty);
            let at = self.res.records().get(id).map(|d| d.name_span);
            let mut d = Diagnostic::error(
                codes::UNKNOWN_FIELD,
                name_span,
                format!("`{shown}` has no field `{name}`"),
            );
            if let Some(at) = at {
                d = d.secondary(at, "declared here");
            }
            self.errors.push(d.help(
                "a field is looked for on the type of the value it is read from, and \
                 nowhere else",
            ));
            return None;
        };
        self.res.record_field(
            name_span,
            FieldRef {
                record: id,
                field: index,
            },
        );
        Some(ty)
    }

    fn check_function(&mut self, f: &Function) {
        // Slot allocation starts fresh per function: a caller's local and a callee's
        // local with the same spelling are different storage, not the same.
        //
        // Looked up in **this module's** table. Before N2 the name alone was enough,
        // which is the same thing as saying a name meant one function in the program.
        self.current = self.res.definition(self.module, &f.name);
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
                let (actual, declared) = (self.show(actual), self.show(declared));
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
                let declared = self.show(declared);
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
            if let Err(why) = self.res.records().task_safety(got) {
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
                        "`Ints` and `Strs` are handles to shared mutable storage with no \
                         synchronisation, and a record containing one at any depth inherits \
                         that. Pass `Int`, `Bool` or `Str` values, a `Chan`, or a record \
                         whose every field may cross",
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
        let Some(binding) = self.scopes.lookup(&place.name) else {
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

        self.res.record_var(place.name_span, binding.slot);

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
                    place
                        .path
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
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
                format!(
                    "{}.{}",
                    place.name,
                    place
                        .path
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                )
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
                    if self.env.contains(name) {
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
                        format!("`-` applies to `Int`, but this is `{}`", self.show(t)),
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
                let Some(sig) = self.resolve_callee(callee, *callee_span) else {
                    let d = self.why_invisible(callee, *callee_span).unwrap_or_else(|| {
                        Diagnostic::error(
                            codes::UNDEFINED_FUNCTION,
                            *callee_span,
                            format!("`{callee}` is not a function"),
                        )
                    });
                    self.errors.push(d);
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
                                    "argument {} of `{callee}` is `{}`, but this is `{}`",
                                    i + 1,
                                    self.show(want),
                                    self.show(got)
                                ),
                            ));
                        }
                    }
                }
                Completion::Value(sig.ret)
            }

            Expr::Construct {
                ty,
                ty_span,
                fields,
                span,
            } => match self.check_construction(ty, *ty_span, fields, *span) {
                Some(t) => Completion::Value(t),
                // The type did not resolve, and that is already reported. `Int` is what
                // every unresolved type continues as, so no second error is raised here.
                None => Completion::Value(Type::Int),
            },

            Expr::Field {
                base,
                name,
                name_span,
                ..
            } => {
                // The base is checked with no expectation: what a projection wants of its
                // base is "a record with this field", which is not a `Type` and so cannot
                // be passed as one.
                let base_ty = self.value_type(base, None);
                match self.check_projection(base_ty, name, *name_span) {
                    Some(t) => Completion::Value(t),
                    None => Completion::Value(Type::Int),
                }
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
                            format!(
                                "an `if` condition must be `Bool`, but this is `{}`",
                                self.show(ct)
                            ),
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
                        let (sa, sb) = (self.show(a), self.show(b));
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                else_block
                                    .tail
                                    .as_ref()
                                    .map_or(else_block.span, |t| t.span()),
                                format!("this branch is `{sb}`, but the other is `{sa}`"),
                            )
                            .secondary(
                                then_block
                                    .tail
                                    .as_ref()
                                    .map_or(then_block.span, |t| t.span()),
                                format!("this one is `{sa}`"),
                            )
                            .help("both branches of an `if` must have the same type"),
                        );
                        Completion::Value(a)
                    }
                    // One branch produces a value and the other does not.
                    (Completion::Value(a), Completion::Unit)
                    | (Completion::Unit, Completion::Value(a)) => {
                        let sa = self.show(a);
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                *e.span_ref(),
                                format!("one branch produces `{sa}` and the other produces nothing"),
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
            //
            // A record joins them rather than getting an exception, and for exactly that
            // reason: one with an `Ints` field would force the same unmade choice, and a
            // rule that held for some records and not others would be worse than no rule.
            BinOp::Eq | BinOp::NotEq => {
                lt == rt && !lt.is_sequence() && lt != Type::Chan && !lt.is_record()
            }
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
            let (slt, srt) = (self.show(lt), self.show(rt));
            let detail = if lt == rt {
                format!("`{}` does not apply to `{slt}`", op.symbol())
            } else {
                format!("`{}` cannot {verb} `{slt}` with `{srt}`", op.symbol())
            };
            self.errors.push(
                Diagnostic::error(codes::INVALID_OPERANDS, op_span, detail)
                    .secondary(lhs_span, format!("this is `{slt}`"))
                    .secondary(rhs_span, format!("this is `{srt}`")),
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
