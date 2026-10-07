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
    Assign, BinOp, Block, Enum, Expr, FieldInit, Function, ImplDecl, MatchArm, Program, Record,
    Return, Spawn, Stmt, Trait, TypeName, TypeParam as WrittenParam, While,
};

use nazm_sema::enums::{EnumDef, EnumId, VariantDef};
use nazm_sema::interface::{
    reachable_types, EnumExport, Export, RecordExport, UnitInterface, Visibility,
};
use nazm_sema::intrinsic::{Intrinsic, Shape};
use nazm_sema::module::{Dependency, ModuleGraph, ModuleId};
use nazm_sema::numeric::NumVal;
use nazm_sema::record::{CRepr, FieldDef, RecordDef, RecordId, OPAQUE_FIELD};
use nazm_sema::resolve::{
    CallSite, Callee, CheckedCall, FieldRef, FnDef, FnId, Instantiation, LocalDef, LocalRef,
    PayloadRef, Propagation, Resolution, VariantRef, WrittenType,
};
use nazm_sema::traits::{ImplDef, TraitCall, TraitDef, TraitId, TraitMethodDef};
use nazm_sema::types::{ParamOwner, Type, TypeParam};
use nazm_sema::udt::{TypeNode, UserTypes, MAX_TYPE_DEPTH};
use nazm_sema::ModuleFacts;
use nazm_sema::{ConvertMode, NumKind, NumericFact};

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<Type>,
    pub ret: Type,
    pub name_span: Span,
    /// How many type parameters the callee declares; zero for one that is not generic.
    pub generics: u32,
    /// Whose parameters `params` and `ret` mention, when `generics` is not zero or the callee
    /// has an effect parameter.
    pub owner: Option<ParamOwner>,
    /// Whether the callee declares an effect parameter (N51), which each call binds.
    pub effect_param: bool,
    /// How many `requires` clauses the callee states (N87).
    pub preconditions: u32,
    /// Whether a contract clause may call it (N87): a function declaring `! {}` that is not
    /// foreign, or a built-in that reads and neither allocates nor performs an effect.
    pub pure: bool,
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

/// An attempt to build a capability, which nothing but the runtime may (N37).
fn forged(k: nazm_sema::CapabilityKind, at: Span) -> Diagnostic {
    Diagnostic::error(
        codes::CAPABILITY_CONSTRUCTED,
        at,
        format!("`{k}` is a capability, and a capability cannot be constructed"),
    )
    .help(format!(
        "authority comes only from `main`: give it {} `{k}` parameter, which the runtime \
         supplies, and pass that to what needs it",
        k.article()
    ))
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

    /// The binding a name reaches, and the index of the frame it is in (N50: a closure's
    /// boundary is a frame index, and a binding before it is captured).
    fn lookup_at(&self, name: &str) -> Option<(usize, Binding)> {
        self.frames
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, f)| f.get(name).copied().map(|b| (i, b)))
    }

    fn depth(&self) -> usize {
        self.frames.len()
    }

    /// The authority the bindings visible here hold (N37): the kind of every binding a
    /// name reaches whose type is a capability. A shadowed binding is not reachable, so it
    /// holds nothing; a binding of a record that contains one holds nothing either — the
    /// authority is the capability value, and a reader sees it only once it is bound.
    ///
    /// Over the frames from `start` in: inside a closure, its own (N50) — a closure cannot
    /// capture a capability, so the authority it holds is its parameters'.
    fn held_from(&self, start: usize) -> nazm_sema::CapabilitySet {
        let mut held = nazm_sema::CapabilitySet::EMPTY;
        for (i, frame) in self.frames.iter().enumerate().skip(start) {
            for (name, b) in frame {
                let Type::Cap(k) = b.ty else { continue };
                let inner = self.frames.get(i + 1..).unwrap_or_default();
                if !inner.iter().any(|f| f.contains_key(name)) {
                    held = held.with(k);
                }
            }
        }
        held
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
/// definitions*. This holds the user-defined types — this module's own, public and private
/// alike, and the ones its direct imports export.
///
/// **One map, both kinds.** `docs/spec.md` puts records and enums in the same namespace, so
/// `struct Item` and `enum Item` in one module is a collision; two maps would have made it
/// legal by accident, which is the kind of decision nobody would have written down.
#[derive(Debug, Default)]
struct Types {
    names: HashMap<String, Type>,
    /// Names refused as an ambiguity or a collision, which mean nothing unambiguously
    /// here. Recorded for the scope trace (N19), which leaves them out.
    refused: Vec<String>,
    /// Every alias this module imports a module under (N49); its types are in `names` as
    /// `alias::Name`, which no identifier can collide with.
    aliases: Vec<String>,
    /// The module's constants, computed (Gate 2): the values its bodies name and the lengths
    /// its array types are written with.
    consts: crate::constants::Constants,
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
    /// Names refused as an ambiguity or a collision. See [`Types::refused`].
    refused: Vec<String>,
    /// Every alias this module imports a module under, and the module (N49); the module's
    /// exports are in `names` as `alias::name`.
    aliases: HashMap<String, ModuleId>,
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
    /// Each enum of this module, with its index in `Program::enums`.
    enums: Vec<(u32, &'a Enum)>,
    /// Its traits and impl headers (N78). An impl's methods are in `functions`.
    traits: Vec<&'a Trait>,
    impls: Vec<&'a ImplDecl>,
    imports: &'a [Dependency],
    /// Its constants (Gate 2).
    consts: Vec<&'a nazm_syntax::Const>,
    /// The core prelude's module in this compilation, found by its durable key. `None`
    /// for a compilation assembled without one, which then has no `Result` for `?` to
    /// recognise — the answer, not an error.
    prelude: Option<ModuleId>,
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
        let enums = program
            .enums
            .iter()
            .enumerate()
            .filter(|(_, e)| graph.module_of(e.name_span.file) == Some(module))
            .map(|(i, e)| (u32::try_from(i).unwrap_or(u32::MAX), e))
            .collect();
        let traits = program
            .traits
            .iter()
            .filter(|t| graph.module_of(t.name_span.file) == Some(module))
            .collect();
        let impls = program
            .impls
            .iter()
            .filter(|i| graph.module_of(i.impl_span.file) == Some(module))
            .collect();
        Self {
            module,
            functions,
            records,
            enums,
            traits,
            impls,
            imports: graph.imports(module),
            consts: program
                .consts
                .iter()
                .filter(|c| graph.module_of(c.name_span.file) == Some(module))
                .collect(),
            prelude: graph.prelude(),
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
    /// Where that return type is written, for a `?` whose error type disagrees with it.
    return_span: Option<Span>,
    /// The core prelude's `Result`, by definition — the one enum `?` takes apart.
    ///
    /// Found once per unit from the prelude module's declarations, which are keyed by the
    /// prelude's durable identity. A project enum shaped like it, or named like it, is a
    /// different definition and never this.
    result: Option<EnumId>,
    /// The core prelude's `Option`, by definition — what `try_convert` returns (Gate 2).
    option: Option<EnumId>,
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
    /// The type parameters in scope — the current function's — by name, for a type
    /// written inside its body. Empty outside a generic function.
    type_scope: Vec<(String, Type)>,
    /// The closures whose bodies enclose the expression being checked, innermost last (N50).
    closures: Vec<ClosureFrame>,
    /// How many closures the top-level function being checked has, for their names.
    closure_count: u32,
    /// The effect parameter of the function being checked, by name, with its owner (N51).
    effect_param: Option<(String, ParamOwner)>,
    /// The traits this module may name (N78), and the modules whose impls it may use.
    trait_names: HashMap<String, Option<TraitId>>,
    impl_modules: Vec<ModuleId>,
    /// This module's functions' preconditions (N87): each one's parameter names and its
    /// `requires` clauses, for proving a call of it whose arguments are all literals.
    preconditions: HashMap<FnId, (Vec<String>, Vec<Expr>)>,
}

/// A closure whose body is being checked (N50).
#[derive(Debug)]
struct ClosureFrame {
    id: FnId,
    /// The scope depth at its body's start: a binding in a frame below it is captured.
    boundary: usize,
    /// The enclosing function's slot of each binding already captured, and the closure's.
    captured: HashMap<u32, u32>,
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
    fn plan(&mut self, graph: &ModuleGraph, interfaces: &[UnitInterface], types: &UserTypes);

    /// Whether this module's bodies must be checked.
    fn must_check(&self, module: ModuleId) -> bool;

    /// This module's bodies were checked, and the module produced no diagnostic at all.
    ///
    /// Only then: an artefact means *this checked cleanly*, so a module with a diagnostic
    /// produces none and a refusal can never be cached. `facts` are its bodies reduced to
    /// provenance flows (N38), which an artefact keeps so a later run that reuses the module
    /// still has them: they are a function of exactly what the module's key covers.
    fn checked_clean(&mut self, module: ModuleId, facts: &ModuleFacts, res: &Resolution);

    /// The provenance facts of a module whose bodies were **not** checked this run, from
    /// what [`Reuse::checked_clean`] kept. `None` when there are none, and the module's
    /// functions are then taken as the worst case rather than as local.
    fn facts(&self, _module: ModuleId, _res: &Resolution) -> Option<ModuleFacts> {
        None
    }
}

/// Check every module. The behaviour every caller but the cache has.
#[derive(Debug, Default, Clone, Copy)]
pub struct CheckEverything;

impl Reuse for CheckEverything {
    fn plan(&mut self, _graph: &ModuleGraph, _interfaces: &[UnitInterface], _types: &UserTypes) {}

    fn must_check(&self, _module: ModuleId) -> bool {
        true
    }

    fn checked_clean(&mut self, _module: ModuleId, _facts: &ModuleFacts, _res: &Resolution) {}
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
    // A completion probe (N26): never an ordinary program, whose `hole` is `None`.
    if let Some(hole) = program.hole {
        res.set_completion_probe(match hole {
            nazm_syntax::HoleSite::Call { callee } => Some(callee),
            _ => None,
        });
    }
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

    // Stage 0a: every user-defined type's name, for the whole graph. Before any field
    // type, because a field may name a type declared later or in another module.
    // Applied types met while field types are still being resolved are interned now and
    // completed once every definition has its fields. `nazm_sema::generic` says why.
    res.types_mut().defer();
    let before = errors.len();
    let mut type_interfaces = declare_types(&units, &mut res, &mut errors);
    if errors.len() != before {
        declared_cleanly.iter_mut().for_each(|c| *c = false);
    }
    // What `pub use` re-exports of types and traits (N103), before anything resolves a type
    // name an importer writes. Refusals wait for the full interfaces below.
    apply_reexports(
        &units,
        &mut type_interfaces,
        &mut res,
        &mut Vec::new(),
        false,
    );

    // Stage 0b: each module's field types, against the type half of its imports'
    // interfaces. Stage 0c: the layouts those fields describe.
    for (n, u) in units.iter().enumerate() {
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| type_interfaces.get(d.on.index()))
            .collect();
        let before = errors.len();
        resolve_type_fields(u, &deps, &mut res, &mut errors);
        mark(&mut declared_cleanly, n, before, errors.len());
    }
    // Every definition has its fields now, so every application met so far can have its.
    res.types_mut().settle();
    let before = errors.len();
    validate_type_layouts(&mut res, &mut errors);
    if errors.len() != before {
        // A layout cycle may span modules, so every module is conservatively unclean:
        // an artefact means *this checked*, and none of them did.
        declared_cleanly.iter_mut().for_each(|c| *c = false);
    }

    // Stage 1, for the whole graph before any body is checked. A cycle needs this: A's
    // body may call B while B's calls A, and neither ordering of the two would work.
    // Every trait's method signatures (N78), before any module declares an impl of one.
    let before = errors.len();
    declare_trait_methods(&units, &type_interfaces, &mut res, &mut errors);
    if errors.len() != before {
        declared_cleanly.iter_mut().for_each(|c| *c = false);
    }

    let mut interfaces: Vec<UnitInterface> = units
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
    // And everything `pub use` re-exports (N103), now that every module offers its own.
    let mut refused = Vec::new();
    let unclean = apply_reexports(&units, &mut interfaces, &mut res, &mut refused, true);
    for m in unclean {
        if let Some(n) = units.iter().position(|u| u.module == m) {
            if let Some(c) = declared_cleanly.get_mut(n) {
                *c = false;
            }
        }
    }
    errors.extend(refused);
    // The arrays the signatures wrote (Gate 2).
    validate_new_arrays(&mut res, &mut errors);

    reuse.plan(graph, &interfaces, res.types());

    // Stage 2, module by module. `deps` is the *direct* imports and nothing else, so a
    // name only reachable transitively is not in scope — by construction rather than by a
    // rule someone has to remember.
    let mut all_facts: Vec<(ModuleId, Option<ModuleFacts>)> = Vec::with_capacity(units.len());
    for (n, u) in units.iter().enumerate() {
        let unparsed = unparsable.contains(&u.module);
        if !unparsed && !reuse.must_check(u.module) {
            all_facts.push((u.module, reuse.facts(u.module, &res)));
            continue;
        }
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| interfaces.get(d.on.index()))
            .collect();
        let before = errors.len();
        check_unit(u, &deps, &mut res, &mut errors);
        let facts = crate::provenance::facts(&u.functions, u.module, &res);
        if !unparsed && errors.len() == before && declared_cleanly.get(n) == Some(&true) {
            reuse.checked_clean(u.module, &facts, &res);
        }
        all_facts.push((u.module, Some(facts)));
    }

    // Provenance (N38): every module's facts, fresh or kept, solved as one program — every
    // run, so a callee's body can never be stale in what its callers are told.
    crate::provenance::solve(&all_facts, &mut res, &mut errors);

    // `main` cannot be generic: there is no caller to choose its type arguments.
    if let Some(def) = res
        .definition(graph.root(), "main")
        .and_then(|id| res.function(id))
        .filter(|d| !d.type_params.is_empty())
    {
        errors.push(
            Diagnostic::error(
                codes::GENERIC_MAIN,
                def.name_span,
                "`main` cannot be generic",
            )
            .help(
                "the process entry is called by nothing that could choose its type \
                 arguments. Make `main` concrete and call a generic function from it",
            ),
        );
    }

    // Nor polymorphic in its effects (N51): nothing calls it to bind the parameter.
    if let Some(def) = res
        .definition(graph.root(), "main")
        .and_then(|id| res.function(id))
        .filter(|d| polymorphic(d.declared_effects))
    {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                def.name_span,
                "`main` cannot have an effect parameter",
            )
            .help(
                "the process entry is called by nothing that could bind it: declare what \
                 `main` does, and call an effect-polymorphic function from it",
            ),
        );
    }

    // `main`'s parameters are the root of authority (N37): the runtime hands it one of each
    // capability it declares, and it can supply nothing else.
    if let Some(def) = res
        .definition(graph.root(), "main")
        .and_then(|id| res.function(id))
    {
        let root = units.iter().find(|u| u.module == graph.root());
        let written = root.and_then(|u| {
            u.functions
                .iter()
                .find(|(decl, _)| *decl == def.decl)
                .map(|(_, f)| *f)
        });
        for (i, t) in def.params.iter().enumerate() {
            if t.is_capability() {
                continue;
            }
            let at = written
                .and_then(|f| f.params.get(i))
                .map_or(def.name_span, |p| p.span);
            errors.push(
                Diagnostic::error(
                    codes::MAIN_PARAMETER,
                    at,
                    "a parameter of `main` must be a capability",
                )
                .help(
                    "the runtime calls `main`, and what it can hand over is authority: \
                     `IoCap` and `SpawnCap`. Read the command line with `arg` instead",
                ),
            );
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

/// Stage 0a for the whole compilation: declare every user-defined type's **name**.
///
/// Names before field types, for every module, because a field type may name a type
/// declared later in the file or in another module — `docs/spec.md` makes declaration
/// order irrelevant, and that is only true if every name exists before any type is
/// resolved. It is N2's declare-then-check, one stage earlier, and the returned interfaces
/// are the *type* half of what a dependent needs.
///
/// Records and enums are declared in one pass over one namespace. A module that declares
/// `struct Item` and `enum Item` is refused here rather than producing two types nothing
/// could tell apart.
///
/// The interfaces returned carry types and imports and no exports: at this point no type
/// has fields, so there is nothing to close over and no signature to publish.
/// [`declare_unit`] returns the whole thing once both are known.
pub fn declare_types(
    units: &[Unit<'_>],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<UnitInterface> {
    let mut interfaces = Vec::with_capacity(units.len());
    for unit in units {
        let mut records = Vec::new();
        let mut enums = Vec::new();
        for (decl, r) in &unit.records {
            if refuse_type_name(unit.module, &r.name, r.name_span, res, errors) {
                continue;
            }
            let visibility = Visibility::from_keyword(r.pub_span);
            let params = declared_type_params(&r.type_params, errors);
            let id = res.records_mut().declare(RecordDef {
                name: r.name.clone(),
                module: unit.module,
                visibility,
                name_span: r.name_span,
                fields: Vec::new(),
                layout: Vec::new(),
                decl: *decl,
                params: params.clone(),
                instance: None,
                repr: match &r.foreign {
                    None => CRepr::Nazm,
                    Some(f) if f.opaque => CRepr::Opaque,
                    Some(_) => CRepr::Layout,
                },
            });
            res.types_mut().name_params(ParamOwner::Record(id), params);
            declared_requirements(
                &r.type_params,
                ParamOwner::Record(id),
                &HashMap::new(),
                res,
                errors,
            );
            if visibility.is_exported() {
                records.push(RecordExport {
                    def: id,
                    nameable: true,
                });
            }
        }
        for (decl, e) in &unit.enums {
            if refuse_type_name(unit.module, &e.name, e.name_span, res, errors) {
                continue;
            }
            let visibility = Visibility::from_keyword(e.pub_span);
            let params = declared_type_params(&e.type_params, errors);
            let id = res.enums_mut().declare(EnumDef {
                name: e.name.clone(),
                module: unit.module,
                visibility,
                name_span: e.name_span,
                variants: Vec::new(),
                order: Vec::new(),
                decl: *decl,
                params: params.clone(),
                instance: None,
            });
            res.types_mut().name_params(ParamOwner::Enum(id), params);
            declared_requirements(
                &e.type_params,
                ParamOwner::Enum(id),
                &HashMap::new(),
                res,
                errors,
            );
            if visibility.is_exported() {
                enums.push(EnumExport {
                    def: id,
                    nameable: true,
                });
            }
        }
        // Traits share the type namespace (N78); their methods' signatures are resolved
        // once every type is, by `declare_trait_methods`.
        for t in &unit.traits {
            if refuse_type_name(unit.module, &t.name, t.name_span, res, errors) {
                continue;
            }
            res.traits_mut().declare(TraitDef {
                name: t.name.clone(),
                module: unit.module,
                visibility: Visibility::from_keyword(t.pub_span),
                name_span: t.name_span,
                methods: Vec::new(),
            });
        }
        interfaces.push(UnitInterface::new(
            unit.module,
            Vec::new(),
            records,
            enums,
            unit.imports.to_vec(),
        ));
    }
    interfaces
}

/// Every trait the module `unit` may name, by the name it writes (N78): its own, and the
/// exported traits of every module it imports — unqualified under a plain `use`, as
/// `alias::Name` under an alias. `None` for a name two imports give two traits, which a
/// use must then qualify. The module's own trait wins over an import's, as a module's own
/// record does.
fn trait_scope(unit: &Unit<'_>, res: &Resolution) -> HashMap<String, Option<TraitId>> {
    let mut own: HashMap<String, Option<TraitId>> = HashMap::new();
    for (id, t) in res.traits().iter() {
        if t.module == unit.module {
            own.insert(t.name.clone(), Some(id));
        }
    }
    let mut out: HashMap<String, Option<TraitId>> = HashMap::new();
    for d in unit.imports {
        let reexported: Vec<TraitId> = res.traits().reexported_by(d.on).collect();
        for (id, t) in res.traits().iter() {
            // Its own exported traits, and those it re-exports (N103).
            let offered =
                (t.module == d.on && t.visibility.is_exported()) || reexported.contains(&id);
            if !offered {
                continue;
            }
            if d.plain {
                out.entry(t.name.clone())
                    .and_modify(|e| {
                        if *e != Some(id) {
                            *e = None;
                        }
                    })
                    .or_insert(Some(id));
            }
            for (alias, _) in &d.aliases {
                out.insert(qualified_key(alias, &t.name), Some(id));
            }
        }
    }
    out.extend(own);
    out
}

/// Every type an interface lets an importer name, by the name it writes: each nameable record
/// and enum by its own, and each type re-exported under another (N103).
fn offered_types(iface: &UnitInterface, res: &Resolution) -> Vec<(String, Type)> {
    let mut out: Vec<(String, Type)> = iface
        .records()
        .iter()
        .filter(|e| e.nameable)
        .map(|e| Type::Record(e.def))
        .chain(
            iface
                .enums()
                .iter()
                .filter(|e| e.nameable)
                .map(|e| Type::Enum(e.def)),
        )
        .filter_map(|t| type_name_of(res, t).map(|n| (n, t)))
        .collect();
    out.extend(iface.renamed().iter().cloned());
    out
}

/// The modules whose impls `unit` may use (N78): itself and every module it imports
/// directly — and every module those re-export from (N103), whose impls their interfaces
/// carry. Coherence is over the whole compilation; *use* is limited to what the module's own
/// check key covers, so a cached check never depends on an impl it did not see.
fn impl_scope(unit: &Unit<'_>, res: &Resolution) -> Vec<ModuleId> {
    let mut out = vec![unit.module];
    let mut work: Vec<ModuleId> = unit.imports.iter().map(|d| d.on).collect();
    while let Some(m) = work.pop() {
        if out.contains(&m) {
            continue;
        }
        out.push(m);
        work.extend(res.traits().reexport_sources(m));
    }
    out
}

/// Apply every `pub use` (N103): each module's interface gains the definitions it
/// re-exports, under their own names or the names it gives them, each the **same**
/// definition — its id, and so its identity, is the defining module's. With `full`, the
/// functions and impls too, and every refusal reported; without, types and traits only,
/// silently, for the type interfaces the importers' types resolve against.
///
/// A module re-exporting from one that re-exports is applied after it; a cycle of re-exports
/// has no order and is refused (`N0213`). Returns the modules a refusal was reported for.
fn apply_reexports(
    units: &[Unit<'_>],
    ifaces: &mut [UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
    full: bool,
) -> Vec<ModuleId> {
    let mut unclean = Vec::new();
    let mut done: Vec<bool> = units
        .iter()
        .map(|u| u.imports.iter().all(|d| d.reexport.is_none()))
        .collect();
    let finished = |done: &[bool], m: ModuleId| {
        units
            .iter()
            .position(|u| u.module == m)
            .is_none_or(|i| done.get(i).copied().unwrap_or(true))
    };
    let mut progressed = true;
    while progressed {
        progressed = false;
        for (n, u) in units.iter().enumerate() {
            if done.get(n).copied().unwrap_or(true) {
                continue;
            }
            if !u
                .imports
                .iter()
                .filter(|d| d.reexport.is_some())
                .all(|d| finished(&done, d.on))
            {
                continue;
            }
            for d in u.imports.iter() {
                let Some(spec) = &d.reexport else { continue };
                let before = errors.len();
                reexport_one(u.module, d.on, spec, ifaces, res, errors, full);
                if errors.len() != before {
                    unclean.push(u.module);
                }
            }
            if let Some(slot) = done.get_mut(n) {
                *slot = true;
            }
            progressed = true;
        }
    }
    if full {
        for (n, u) in units.iter().enumerate() {
            if done.get(n).copied().unwrap_or(true) {
                continue;
            }
            for d in u.imports.iter() {
                if let Some(spec) = &d.reexport {
                    errors.push(
                        Diagnostic::error(
                            codes::REEXPORT_CYCLE,
                            spec.at,
                            "this re-export is part of a cycle of re-exports",
                        )
                        .help(
                            "a re-export offers what another module offers, so a cycle of \
                             them offers nothing in particular: re-export from the module \
                             that defines the name",
                        ),
                    );
                    unclean.push(u.module);
                }
            }
        }
    }
    unclean
}

/// One `pub use`: what module `from` offers, added to `module`'s interface.
fn reexport_one(
    module: ModuleId,
    from: ModuleId,
    spec: &nazm_sema::ReexportSpec,
    ifaces: &mut [UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
    full: bool,
) {
    let Some(src) = ifaces.get(from.index()).cloned() else {
        return;
    };
    res.traits_mut().record_reexport_source(module, from);
    // What `from` offers, by the name an importer writes.
    let mut types: Vec<(String, Type)> = Vec::new();
    for r in src.records().iter().filter(|r| r.nameable) {
        if let Some(d) = res.records().get(r.def) {
            types.push((d.name.clone(), Type::Record(r.def)));
        }
    }
    for e in src.enums().iter().filter(|e| e.nameable) {
        if let Some(d) = res.enums().get(e.def) {
            types.push((d.name.clone(), Type::Enum(e.def)));
        }
    }
    types.extend(src.renamed().iter().cloned());
    let traits: Vec<(String, TraitId)> = res
        .traits()
        .iter()
        .filter(|(_, t)| t.module == from && t.visibility.is_exported())
        .map(|(id, t)| (t.name.clone(), id))
        .chain(
            res.traits()
                .reexported_by(from)
                .filter_map(|id| res.traits().get(id).map(|t| (t.name.clone(), id)))
                .collect::<Vec<_>>(),
        )
        .collect();
    let functions: Vec<Export> = src.exports().to_vec();

    // The chosen names, or every one, with the name each is offered under.
    struct Chosen {
        name: String,
        offered: String,
        at: Span,
        renamed: bool,
    }
    let chosen: Vec<Chosen> = match &spec.names {
        Some(list) => list
            .iter()
            .map(|i| Chosen {
                name: i.name.clone(),
                offered: i.offered().to_owned(),
                at: i.span,
                renamed: i.alias.is_some(),
            })
            .collect(),
        None => functions
            .iter()
            .map(|e| e.name.clone())
            .chain(types.iter().map(|(n, _)| n.clone()))
            .chain(traits.iter().map(|(n, _)| n.clone()))
            .map(|name| Chosen {
                offered: name.clone(),
                name,
                at: spec.at,
                renamed: false,
            })
            .collect(),
    };

    for c in chosen {
        let function = functions.iter().find(|e| e.name == c.name);
        let ty = types.iter().find(|(n, _)| *n == c.name).map(|(_, t)| *t);
        let tr = traits.iter().find(|(n, _)| *n == c.name).map(|(_, t)| *t);
        let Some(here) = ifaces.get(module.index()) else {
            return;
        };
        // A name this module already offers, its own or another re-export's.
        let taken_fn = here.exports().iter().any(|e| e.name == c.offered);
        let taken_ty = here.user_type(res.types(), &c.offered).is_some()
            || res
                .traits()
                .iter()
                .any(|(_, t)| t.module == module && t.name == c.offered)
            || res
                .traits()
                .reexported_by(module)
                .any(|id| res.traits().get(id).is_some_and(|t| t.name == c.offered));
        if function.is_none() && ty.is_none() && tr.is_none() {
            if full {
                errors.push(
                    Diagnostic::error(
                        codes::REEXPORT_UNKNOWN,
                        c.at,
                        format!(
                            "`{}` is not exported by the module this re-exports from",
                            c.name
                        ),
                    )
                    .help("a module re-exports only what the module it names exports"),
                );
            }
            continue;
        }
        if let Some(e) = function {
            if full {
                if taken_fn {
                    errors.push(taken(&c.offered, c.at));
                    continue;
                }
                let mut offered = e.clone();
                offered.name.clone_from(&c.offered);
                // Its identity is where it is defined, however many modules pass it on.
                if offered.origin.is_none() {
                    offered.origin = Some((from, e.name.clone()));
                }
                if let Some(i) = ifaces.get_mut(module.index()) {
                    i.add_reexport(offered);
                }
                if c.at != spec.at {
                    res.record_call(c.at, Callee::Function(e.def));
                }
            }
        }
        if let Some(t) = ty {
            if taken_ty && full && function.is_none() {
                errors.push(taken(&c.offered, c.at));
                continue;
            }
            if !taken_ty {
                if let Some(i) = ifaces.get_mut(module.index()) {
                    i.add_reexported_type(t, c.renamed.then(|| c.offered.clone()));
                }
                if c.at != spec.at {
                    match t {
                        Type::Record(id) => res.record_type_name(c.at, TypeNode::Record(id)),
                        Type::Enum(id) => res.record_type_name(c.at, TypeNode::Enum(id)),
                        _ => {}
                    }
                }
            }
        }
        if let Some(t) = tr {
            if c.renamed {
                if full {
                    errors.push(
                        Diagnostic::error(
                            codes::REEXPORT_UNKNOWN,
                            c.at,
                            format!("`{}` is a trait, which is re-exported under its own name", c.name),
                        )
                        .help("a trait's name is how an importer's bounds and calls find it: drop `as`"),
                    );
                }
                continue;
            }
            // Recorded at the type stage already: the full stage only reports.
            let again = res.traits().reexported_by(module).any(|x| x == t);
            if taken_ty && !again && full && function.is_none() && ty.is_none() {
                errors.push(taken(&c.offered, c.at));
                continue;
            }
            res.traits_mut().record_reexport(module, t);
            if c.at != spec.at {
                res.traits_mut().record_mention(c.at, t);
            }
        }
    }
    if full {
        if let Some(i) = ifaces.get_mut(module.index()) {
            i.add_reexported_traits(src.traits(), src.impls());
        }
    }
}

/// `N0212`: a name this module offers twice.
fn taken(name: &str, at: Span) -> Diagnostic {
    Diagnostic::error(
        codes::REEXPORT_COLLIDES,
        at,
        format!("`{name}` is already offered by this module"),
    )
    .help(
        "a module offers each name once: re-export it under another name with `as`, or \
         rename the definition",
    )
}

/// Stage 0c (N78): every trait's method signatures, with `Self` the trait's own type
/// parameter. After every type is declared and settled, before any module's functions:
/// an impl may be in a module declared before its trait's.
pub fn declare_trait_methods(
    units: &[Unit<'_>],
    type_interfaces: &[UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    for unit in units {
        if unit.traits.is_empty() {
            continue;
        }
        let deps: Vec<&UnitInterface> = unit
            .imports
            .iter()
            .filter_map(|d| type_interfaces.get(d.on.index()))
            .collect();
        let types = type_environment(unit, &deps, res, errors, Collisions::Silent);
        for t in &unit.traits {
            let Some(id) = res
                .traits()
                .iter()
                .find(|(_, d)| d.module == unit.module && d.name_span == t.name_span)
                .map(|(id, _)| id)
            else {
                continue;
            };
            let me = Type::Param(id.self_param());
            res.types_mut()
                .name_params(ParamOwner::Trait(id), vec!["Self".to_owned()]);
            let scope = vec![("Self".to_owned(), me)];
            let mut methods: Vec<TraitMethodDef> = Vec::new();
            for m in &t.methods {
                if let Some(first) = methods.iter().find(|x| x.name == m.name) {
                    errors.push(
                        Diagnostic::error(
                            codes::DUPLICATE_FUNCTION,
                            m.name_span,
                            format!(
                                "`{}` is declared more than once in trait `{}`",
                                m.name, t.name
                            ),
                        )
                        .secondary(first.name_span, "first declared here"),
                    );
                    continue;
                }
                let resolved: Vec<Type> = m
                    .params
                    .iter()
                    .map(|p| resolve_written(&types, res, &p.ty, &scope, None, errors))
                    .collect();
                let self_ok = m.params.first().is_some_and(|p| p.name == "self")
                    && resolved.first() == Some(&me);
                if !self_ok {
                    errors.push(
                        Diagnostic::error(
                            codes::TRAIT_METHOD_SELF,
                            m.params.first().map_or(m.name_span, |p| p.span),
                            format!("`{}`'s first parameter must be `self: Self`", m.name),
                        )
                        .help(
                            "a trait method is called on a value of the implementing type, \
                             which it receives first: `fn show(self: Self) -> Str;`",
                        ),
                    );
                    continue;
                }
                let ret = resolve_written(&types, res, &m.return_type, &scope, None, errors);
                let effects = m
                    .effects
                    .as_ref()
                    .and_then(|d| crate::effects::declared(d, None, errors))
                    .unwrap_or_default();
                methods.push(TraitMethodDef {
                    name: m.name.clone(),
                    name_span: m.name_span,
                    params: resolved.into_iter().skip(1).collect(),
                    ret,
                    effects,
                });
            }
            if let Some(def) = res.traits_mut().get_mut(id) {
                def.methods = methods;
            }
        }
    }
}

/// The trait a written name in `unit` refers to (N78), or the diagnostic saying why none.
// One diagnostic, built only when the name does not resolve: not worth a box.
#[allow(clippy::result_large_err)]
fn named_trait(
    written: &TypeName,
    scope: &HashMap<String, Option<TraitId>>,
    res: &Resolution,
    unit: &Unit<'_>,
) -> Result<TraitId, Diagnostic> {
    let key = written_key(written.qualifier.as_ref(), &written.name);
    match scope.get(&key) {
        Some(Some(id)) => Ok(*id),
        Some(None) => Err(Diagnostic::error(
            codes::AMBIGUOUS_IMPORT,
            written.name_span,
            format!(
                "`{}` is a trait in more than one imported module",
                written.name
            ),
        )
        .help("import one of them under a name and write `alias::Trait`")),
        None => {
            let is_type = res.records().declared(unit.module, &written.name).is_some()
                || res.enums().declared(unit.module, &written.name).is_some()
                || Type::from_name(&written.name).is_some();
            Err(if is_type {
                Diagnostic::error(
                    codes::NOT_A_TRAIT,
                    written.name_span,
                    format!("`{}` is a type, not a trait", written.name),
                )
                .help("a bound or an impl names a trait: `impl Show for Point`")
            } else {
                Diagnostic::error(
                    codes::UNKNOWN_TYPE,
                    written.name_span,
                    format!("no trait named `{}` is in scope", written.name),
                )
                .help("declare it, or import the module that exports it")
            })
        }
    }
}

/// Whether `ty` may have an impl (N78, `architecture.md` §7.79), and the module that
/// declares it — `None` for a built-in.
fn implementable(res: &Resolution, ty: Type) -> Result<Option<ModuleId>, &'static str> {
    match ty {
        Type::Int | Type::Bool | Type::Str => Ok(None),
        Type::Record(id) => {
            let def = res.records().get(id).ok_or("an unknown record")?;
            if !def.params.is_empty() || def.instance.is_some() {
                return Err("a generic record");
            }
            Ok(Some(def.module))
        }
        Type::Enum(id) => {
            let def = res.enums().get(id).ok_or("an unknown enum")?;
            if !def.params.is_empty() || def.instance.is_some() {
                return Err("a generic enum");
            }
            Ok(Some(def.module))
        }
        Type::Param(_) => Err("a type parameter"),
        _ => Err("a built-in type other than `Int`, `Bool` or `Str`"),
    }
}

/// The function name an impl method is declared under: `Trait/Type/m`, which no written
/// name can be (N78).
/// The definition a written function declared: by its name, or — for an impl's method,
/// which is declared under `Trait/Type/m` — by where its name is (N78).
pub(crate) fn defined(res: &Resolution, module: ModuleId, f: &Function) -> Option<FnId> {
    if f.method_of.is_none() {
        return res.definition(module, &f.name);
    }
    res.functions()
        .iter()
        .position(|d| d.module == module && d.name_span == f.name_span && d.closure.is_none())
        .map(FnId::from_index)
}

fn impl_method_name(trait_name: &str, ty: &str, method: &str) -> String {
    format!("{trait_name}/{ty}/{method}")
}

/// File one impl whose header resolved (N78): its methods against its trait's, the orphan
/// rule, and the coherence table. Every rule is reported; the impl is filed only if its
/// module may hold it, and it is the table's answer only if it is the first of its kind.
fn register_impl(
    unit: &Unit<'_>,
    imp: &ImplDecl,
    trait_id: TraitId,
    ty: Type,
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let Some(tr) = res.traits().get(trait_id).cloned() else {
        return;
    };
    let ty_name = res.types().show(ty);
    // The orphan rule: the trait's module, or the type's.
    let home = implementable(res, ty).ok().flatten();
    if tr.module != unit.module && home != Some(unit.module) {
        errors.push(
            Diagnostic::error(
                codes::IMPL_ORPHAN,
                imp.impl_span,
                format!(
                    "an impl of `{}` for `{ty_name}` must be in the module declaring `{}` or \
                     the one declaring `{ty_name}`",
                    tr.name, tr.name
                ),
            )
            .secondary(tr.name_span, "the trait is declared here")
            .help(
                "one impl per trait and type, where everyone who can name both can find it: \
                 move it to the trait's module or the type's",
            ),
        );
        return;
    }
    let mut methods: Vec<Option<FnId>> = vec![None; tr.methods.len()];
    for (_, f) in unit.functions.iter().filter(|(_, f)| {
        f.method_of
            .as_ref()
            .is_some_and(|m| m.impl_span == imp.impl_span)
    }) {
        let Some(id) = res.definition(unit.module, &impl_method_name(&tr.name, &ty_name, &f.name))
        else {
            continue;
        };
        let Some(index) = tr.method(&f.name) else {
            errors.push(
                Diagnostic::error(
                    codes::IMPL_METHOD_MISMATCH,
                    f.name_span,
                    format!("`{}` is not a method of `{}`", f.name, tr.name),
                )
                .secondary(tr.name_span, "the trait is declared here")
                .help("an impl defines its trait's methods and nothing else"),
            );
            continue;
        };
        let Some(want) = tr.methods.get(index as usize) else {
            continue;
        };
        let Some(def) = res.function(id).cloned() else {
            continue;
        };
        let types = res.types_mut();
        let owner = ParamOwner::Trait(trait_id);
        let params: Vec<Type> = std::iter::once(ty)
            .chain(
                want.params
                    .iter()
                    .map(|t| types.substitute(*t, owner, &[ty])),
            )
            .collect();
        let ret = types.substitute(want.ret, owner, &[ty]);
        let self_named = f.params.first().is_some_and(|p| p.name == "self");
        let effects_match = def.declared_effects.unwrap_or_default() == want.effects;
        if def.params != params
            || def.ret != ret
            || !self_named
            || !effects_match
            || !def.type_params.is_empty()
        {
            let wanted: Vec<String> = params.iter().map(|t| types.show(*t)).collect();
            errors.push(
                Diagnostic::error(
                    codes::IMPL_METHOD_MISMATCH,
                    f.name_span,
                    format!(
                        "`{}` must be `fn {}(self: {}) -> {}` with the trait's effects, as `{}` \
                         declares it with `Self` = `{ty_name}`",
                        f.name,
                        f.name,
                        wanted.join(", "),
                        types.show(ret),
                        tr.name
                    ),
                )
                .secondary(want.name_span, "the trait's method"),
            );
            continue;
        }
        if let Some(slot) = methods.get_mut(index as usize) {
            *slot = Some(id);
        }
    }
    let missing: Vec<&str> = tr
        .methods
        .iter()
        .zip(&methods)
        .filter(|(_, m)| m.is_none())
        .map(|(m, _)| m.name.as_str())
        .collect();
    if !missing.is_empty() {
        errors.push(
            Diagnostic::error(
                codes::IMPL_MISSING_METHOD,
                imp.impl_span,
                format!(
                    "this impl of `{}` for `{ty_name}` does not define {}",
                    tr.name,
                    quoted(&missing.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>())
                ),
            )
            .secondary(tr.name_span, "the trait is declared here")
            .help("an impl defines every method of its trait; there are no default methods"),
        );
    }
    let filed = res.traits_mut().implement(ImplDef {
        trait_id,
        self_ty: ty,
        module: unit.module,
        impl_span: imp.impl_span,
        methods,
    });
    if let Err(first) = filed {
        let at = res
            .traits()
            .impl_def(first)
            .map_or(imp.impl_span, |d| d.impl_span);
        errors.push(
            Diagnostic::error(
                codes::IMPL_CONFLICT,
                imp.impl_span,
                format!("`{ty_name}` already implements `{}`", tr.name),
            )
            .secondary(at, "the first impl")
            .help("a type implements a trait at most once in a program, so every call means one function"),
        );
    }
}

/// Whether this module may not declare a type of this name, having said why.
///
/// One function for both kinds, because the namespace is one: a built-in's name is nobody's
/// to take, and a second declaration under a name this module already has is a duplicate
/// whether or not the two are the same kind.
fn refuse_type_name(
    module: ModuleId,
    name: &str,
    at: Span,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) -> bool {
    // A built-in's name is not a module's to define. Unlike a duplicate, there is no
    // declaration to point at, so the reason is said rather than shown. `Vec` is one: a
    // type written `Vec` is always the built-in, so a record declared under that name
    // could be built and never named.
    if Type::from_name(name).is_some() || name == VEC {
        errors.push(
            Diagnostic::error(
                codes::BUILTIN_TYPE_REDEFINED,
                at,
                format!("`{name}` is a built-in type and cannot be redefined"),
            )
            .help(
                "built-in type names mean the same thing in every module of every \
                 program, so none of them may be shadowed; choose another name",
            ),
        );
        return true;
    }
    let first = res
        .records()
        .declared(module, name)
        .and_then(|id| res.records().get(id))
        .map(|d| d.name_span)
        .or_else(|| {
            res.enums()
                .declared(module, name)
                .and_then(|id| res.enums().get(id))
                .map(|d| d.name_span)
        })
        .or_else(|| {
            res.traits()
                .iter()
                .find(|(_, t)| t.module == module && t.name == name)
                .map(|(_, t)| t.name_span)
        });
    if let Some(first) = first {
        errors.push(
            Diagnostic::error(
                codes::DUPLICATE_TYPE,
                at,
                format!("`{name}` is declared more than once in this module"),
            )
            .secondary(first, "first declared here")
            .help(
                "records and enums share one type namespace, so a `struct` and an `enum` \
                 of the same name collide exactly as two of either would",
            ),
        );
        return true;
    }
    false
}

/// A declaration's type parameters, checked once: unique within the list (`N0350`), and
/// never a built-in type's name or `Vec` (`N0331`). Returns the names in order, for
/// messages — a parameter's identity is its position, not what it is called.
fn declared_type_params(params: &[WrittenParam], errors: &mut Vec<Diagnostic>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        if Type::from_name(&p.name).is_some() || p.name == VEC {
            errors.push(
                Diagnostic::error(
                    codes::BUILTIN_TYPE_REDEFINED,
                    p.span,
                    format!(
                        "`{}` is a built-in type and cannot be a type parameter",
                        p.name
                    ),
                )
                .help(
                    "a built-in type's name means the same thing everywhere, including \
                     inside a generic definition; choose another name, such as `T`",
                ),
            );
        } else if let Some(first) = params.iter().take(i).find(|q| q.name == p.name) {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_TYPE_PARAMETER,
                    p.span,
                    format!("`{}` is declared twice in this parameter list", p.name),
                )
                .secondary(first.span, "first declared here")
                .help("a type parameter is one position; two cannot share a name"),
            );
        }
        names.push(p.name.clone());
    }
    names
}

/// The one capability a type parameter may require. N14.
///
/// Not a type and not a declaration: a name that means something only after the `:` in a
/// type parameter list, so no record, enum or import can shadow it and it shadows nothing.
pub const EQUALITY: &str = "Equality";

/// What a declaration's type parameters require, recorded against each parameter's
/// identity — its owner and position. N14.
///
/// Only a function's parameters may carry a requirement. One on a record's or an enum's is
/// refused (`N0101`) rather than parsed and ignored: every instance of such a type would
/// have to be checked wherever it is written, and nothing needs that yet. A name that is
/// not a capability is `N0365`.
fn declared_requirements(
    params: &[WrittenParam],
    owner: ParamOwner,
    traits: &HashMap<String, Option<TraitId>>,
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    for (i, p) in params.iter().enumerate() {
        let Some(req) = &p.requires else {
            continue;
        };
        if !matches!(owner, ParamOwner::Function(_)) {
            errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    req.span,
                    "a requirement on a record's or an enum's type parameter is not supported yet",
                )
                .help(
                    "require the capability where it is used: a function can say \
                     `fn same[T: Equality](a: Box[T], b: Box[T]) -> Bool`",
                ),
            );
            continue;
        }
        // A trait (N78): the parameter's methods are the trait's.
        if req.name != EQUALITY {
            if let Some(Some(t)) = traits.get(&req.name) {
                res.traits_mut().record_mention(req.span, *t);
                res.traits_mut().bound(
                    TypeParam {
                        owner,
                        index: u32::try_from(i).unwrap_or(u32::MAX),
                    },
                    *t,
                );
                continue;
            }
            errors.push(
                Diagnostic::error(
                    codes::UNKNOWN_REQUIREMENT,
                    req.span,
                    format!(
                        "`{}` is not a capability a type parameter can require",
                        req.name
                    ),
                )
                .help(
                    "a type parameter requires `Equality` or a trait in scope: \
                     `fn same[T: Equality](a: T, b: T) -> Bool`, `fn f[T: Show](x: T) -> Str`",
                ),
            );
            continue;
        }
        res.types_mut().require_equality(TypeParam {
            owner,
            index: u32::try_from(i).unwrap_or(u32::MAX),
        });
    }
}

/// The written name of the one generic built-in type.
///
/// Not in [`Type::from_name`], which answers for a complete type, and `Vec` alone is not
/// one: it is always applied to its element.
const VEC: &str = "Vec";

/// A declaration's type parameters as the types they are inside it, by name.
/// Record each of `params` as the declaration of its parameter of `owner` (N23), at the
/// name its `[…]` list writes — the identity [`param_scope`] gives the same position.
fn declare_type_params(res: &mut Resolution, params: &[WrittenParam], owner: ParamOwner) {
    for (i, p) in params.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        res.record_written_type(
            p.span,
            WrittenType::ParamDeclaration(TypeParam { owner, index }),
        );
    }
}

fn param_scope(params: &[WrittenParam], owner: ParamOwner) -> Vec<(String, Type)> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                p.name.clone(),
                Type::Param(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                }),
            )
        })
        .collect()
}

/// Stage 0b for one module: resolve its types' field types.
///
/// `deps` are the type-carrying interfaces of the modules this one directly imports —
/// what [`declare_types`] returned, selected by the caller. Their *bodies* are not a
/// parameter and never will be, which is the same claim [`check_unit`] makes one stage
/// later.
pub fn resolve_type_fields(
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
        declare_type_params(res, &r.type_params, ParamOwner::Record(id));
        let scope = param_scope(&r.type_params, ParamOwner::Record(id));
        record_type_scope(res, r.span, &scope, &types);
        let mut fields = resolve_fields(&types, res, &r.name, &r.fields, &scope, errors);
        // A record C sees (N85, §7.86): the C ABI, not generic, and — with fields — only
        // fields C has a layout for. An opaque handle's one field is a pointer no program
        // can name.
        if let Some(f) = &r.foreign {
            foreign_struct(r, f, &fields, res, errors);
            if f.opaque {
                fields = vec![FieldDef {
                    name: OPAQUE_FIELD.to_owned(),
                    ty: Type::Int,
                    name_span: r.name_span,
                    ty_span: r.name_span,
                }];
            }
        }
        // A record with no fields. Refused rather than allowed, and the reason is the
        // grammar: `Marker()` is exactly what a call to a function named `Marker` looks
        // like, and types and functions are separate namespaces, so the two forms would
        // be genuinely ambiguous. `docs/spec.md` records the decision and what would make
        // it reversible.
        if fields.is_empty() && r.fields.is_empty() {
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

    let mut done: Vec<&str> = Vec::new();
    for (_, e) in &unit.enums {
        let Some(id) = res.enums().declared(unit.module, &e.name) else {
            continue;
        };
        if done.contains(&e.name.as_str()) {
            continue;
        }
        done.push(&e.name);
        // An enum with no variants is a type with no values, which raises `Never`
        // semantics the language has not settled. Refused deliberately rather than
        // allowed as a side effect. `docs/spec.md` says what a future milestone would
        // have to decide.
        if e.variants.is_empty() {
            errors.push(
                Diagnostic::error(
                    codes::EMPTY_ENUM,
                    e.name_span,
                    format!("`{}` has no variants, which is not supported yet", e.name),
                )
                .help(
                    "a type with no values decides what a function returning one means \
                     and whether a `match` on one is exhaustive, and neither has been \
                     settled. Give it a variant",
                ),
            );
        }
        let mut variants: Vec<VariantDef> = Vec::new();
        declare_type_params(res, &e.type_params, ParamOwner::Enum(id));
        let scope = param_scope(&e.type_params, ParamOwner::Enum(id));
        record_type_scope(res, e.span, &scope, &types);
        for v in &e.variants {
            if let Some(first) = variants.iter().find(|p| p.name == v.name) {
                errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_VARIANT,
                        v.name_span,
                        format!("`{}` has two variants called `{}`", e.name, v.name),
                    )
                    .secondary(first.name_span, "first declared here")
                    .help("a variant is identified by its name, so two cannot share one"),
                );
                continue;
            }
            let fields = resolve_fields(&types, res, &v.name, &v.fields, &scope, errors);
            variants.push(VariantDef {
                name: v.name.clone(),
                name_span: v.name_span,
                fields,
                layout: Vec::new(),
            });
        }
        res.enums_mut().complete(id, variants);
    }
}

/// A literal value, as the checker's precondition proof evaluates one (N87).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lit {
    Int(i64),
    Bool(bool),
}

impl Lit {
    fn show(&self) -> String {
        match self {
            Self::Int(i) => i.to_string(),
            Self::Bool(b) => b.to_string(),
        }
    }
}

/// A place's path as a reader writes it: `b.c`, `items[…].x` (Gate 2 added indexes).
fn shown_path(path: &[nazm_syntax::PlaceStep]) -> String {
    let mut out = String::new();
    for step in path {
        match step {
            nazm_syntax::PlaceStep::Field(f) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(&f.name);
            }
            nazm_syntax::PlaceStep::Index { .. } => out.push_str("[…]"),
        }
    }
    out
}

/// The conversion functions that are not type names (Gate 2), reserved as built-ins are.
const CONVERSIONS: [&str; 3] = ["try_convert", "wrap_convert", "saturate_convert"];

/// Whether `e` is a number literal, possibly negated — the operand that takes its type from the
/// other side of a binary operator (Gate 2).
fn is_number_literal(e: &Expr) -> bool {
    match e {
        Expr::Int { .. } | Expr::WideInt { .. } | Expr::Float { .. } => true,
        Expr::Neg { operand, .. } => is_number_literal(operand),
        _ => false,
    }
}

/// `e`'s value from literals and the names in `env`, or `None` for anything else — including
/// an operation that would fail, which is then not proved. `&&` and `||` short-circuit, as at
/// run time, so a clause guarded by one is evaluated as the program would evaluate it.
fn literal(e: &Expr, env: &HashMap<&str, Lit>) -> Option<Lit> {
    match e {
        Expr::Int { value, .. } => Some(Lit::Int(*value)),
        Expr::Bool { value, .. } => Some(Lit::Bool(*value)),
        Expr::Var { name, .. } => env.get(name.as_str()).copied(),
        Expr::Neg { operand, .. } => match literal(operand, env)? {
            Lit::Int(i) => i.checked_neg().map(Lit::Int),
            Lit::Bool(_) => None,
        },
        Expr::Not { operand, .. } => match literal(operand, env)? {
            Lit::Bool(b) => Some(Lit::Bool(!b)),
            Lit::Int(_) => None,
        },
        Expr::Binary { op, lhs, rhs, .. } => {
            let l = literal(lhs, env)?;
            match (op, l) {
                (BinOp::And, Lit::Bool(false)) => return Some(Lit::Bool(false)),
                (BinOp::Or, Lit::Bool(true)) => return Some(Lit::Bool(true)),
                _ => {}
            }
            let r = literal(rhs, env)?;
            match (l, r) {
                (Lit::Int(a), Lit::Int(b)) => match op {
                    BinOp::Add => a.checked_add(b).map(Lit::Int),
                    BinOp::Sub => a.checked_sub(b).map(Lit::Int),
                    BinOp::Mul => a.checked_mul(b).map(Lit::Int),
                    BinOp::Div => a.checked_div(b).map(Lit::Int),
                    BinOp::Rem => a.checked_rem(b).map(Lit::Int),
                    BinOp::Eq => Some(Lit::Bool(a == b)),
                    BinOp::NotEq => Some(Lit::Bool(a != b)),
                    BinOp::Lt => Some(Lit::Bool(a < b)),
                    BinOp::LtEq => Some(Lit::Bool(a <= b)),
                    BinOp::Gt => Some(Lit::Bool(a > b)),
                    BinOp::GtEq => Some(Lit::Bool(a >= b)),
                    BinOp::BitAnd => Some(Lit::Int(a & b)),
                    BinOp::BitOr => Some(Lit::Int(a | b)),
                    BinOp::BitXor => Some(Lit::Int(a ^ b)),
                    BinOp::Shl => u32::try_from(b)
                        .ok()
                        .filter(|n| *n < 64)
                        .map(|n| Lit::Int(a << n)),
                    BinOp::Shr => u32::try_from(b)
                        .ok()
                        .filter(|n| *n < 64)
                        .map(|n| Lit::Int(a >> n)),
                    BinOp::And | BinOp::Or => None,
                },
                (Lit::Bool(a), Lit::Bool(b)) => match op {
                    BinOp::And => Some(Lit::Bool(a && b)),
                    BinOp::Or => Some(Lit::Bool(a || b)),
                    BinOp::Eq => Some(Lit::Bool(a == b)),
                    BinOp::NotEq => Some(Lit::Bool(a != b)),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// A clause, for a witness: its shape, not its source text, which the checker does not hold.
fn describe_clause(e: &Expr) -> String {
    match e {
        Expr::Int { value, .. } => value.to_string(),
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Var { name, .. } => name.clone(),
        Expr::Neg { operand, .. } => format!("-{}", describe_clause(operand)),
        Expr::Not { operand, .. } => format!("!{}", describe_clause(operand)),
        Expr::Binary { op, lhs, rhs, .. } => {
            let o = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Eq => "==",
                BinOp::NotEq => "!=",
                BinOp::Lt => "<",
                BinOp::LtEq => "<=",
                BinOp::Gt => ">",
                BinOp::GtEq => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::BitAnd => "&",
                BinOp::BitOr => "|",
                BinOp::BitXor => "^",
                BinOp::Shl => "<<",
                BinOp::Shr => ">>",
            };
            format!("{} {o} {}", describe_clause(lhs), describe_clause(rhs))
        }
        _ => "…".to_owned(),
    }
}

/// How many `requires` clauses a function states (N87).
fn preconditions_of(f: &Function) -> u32 {
    u32::try_from(
        f.contracts
            .iter()
            .filter(|c| c.kind == nazm_syntax::ContractKind::Requires)
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// The function a callee is, or a definition no function has.
fn callee_fn(c: Callee) -> FnId {
    match c {
        Callee::Function(f) => f,
        Callee::Builtin(_) | Callee::Indirect(_) => FnId::from_index(usize::MAX),
    }
}

/// The rules an `extern "C" struct` meets (N85, §7.86): the C ABI; no type parameters, since
/// C has one layout per struct; and for a C-layout record, fields of `Int`, `Bool`, an opaque
/// handle or another C-layout record — each of which has the layout C gives `int64_t`, `bool`,
/// a pointer and the nested struct.
fn foreign_struct(
    r: &nazm_syntax::Record,
    f: &nazm_syntax::ForeignStruct,
    fields: &[FieldDef],
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    if f.abi != "C" {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_ABI,
                f.abi_span,
                format!("`\"{}\"` is not an ABI this compiler lays out", f.abi),
            )
            .help("the one foreign ABI is `\"C\"`"),
        );
    }
    if let (Some(first), Some(last)) = (r.type_params.first(), r.type_params.last()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                first.span.join(last.span),
                "a C struct cannot be generic",
            )
            .help("C has one layout per struct; declare one per type"),
        );
    }
    for field in fields {
        if !c_layout_type(res, field.ty) {
            errors.push(
                Diagnostic::error(
                    codes::FOREIGN_TYPE,
                    field.ty_span,
                    format!(
                        "a C struct cannot have a field of this type (`{}`)",
                        field.name
                    ),
                )
                .primary_label("no C layout")
                .help(
                    "a C struct's fields are `Int` (`int64_t`), `Bool` (`bool`), an opaque \
                     handle (a pointer) or another C struct; a `Str`, a sequence, an enum or \
                     an ordinary record has no layout C shares",
                ),
            );
        }
    }
}

/// `N0611`: an opaque handle built or taken apart (N85).
fn opaque_refused(at: Span, ty: &str, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::OPAQUE_HANDLE,
        at,
        format!("`{ty}` is an opaque C handle, which cannot be {what}"),
    )
    .help(
        "a handle declared `extern \"C\" struct Name;` is a C pointer Nazm never reads: only a \
         foreign function returns one, and only C reads what it points to",
    )
}

/// Whether a value of `t` has a C layout (N85): `Int`, `Bool`, or a record declared
/// `extern "C"` — a C-layout record or an opaque handle.
fn c_layout_type(res: &Resolution, t: Type) -> bool {
    match t {
        Type::Int | Type::Bool => true,
        Type::Record(id) => res.records().get(id).is_some_and(|d| d.repr != CRepr::Nazm),
        _ => false,
    }
}

/// One declared field list, resolved. Shared by a record's fields and a variant's payload,
/// because they are the same thing: `docs/spec.md` makes a payload record-shaped so that a
/// future tuple variant is a new decision rather than a drift.
fn resolve_fields(
    types: &Types,
    res: &mut Resolution,
    owner: &str,
    written: &[nazm_syntax::Field],
    scope: &[(String, Type)],
    errors: &mut Vec<Diagnostic>,
) -> Vec<FieldDef> {
    let mut fields: Vec<FieldDef> = Vec::new();
    for f in written {
        if let Some(first) = fields.iter().find(|e| e.name == f.name) {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FIELD,
                    f.name_span,
                    format!("`{owner}` has two fields called `{}`", f.name),
                )
                .secondary(first.name_span, "first declared here")
                .help("a field is identified by its name, so two cannot share one"),
            );
            continue;
        }
        let ty = resolve_written(types, res, &f.ty, scope, None, errors);
        fields.push(FieldDef {
            name: f.name.clone(),
            ty,
            name_span: f.name_span,
            ty_span: f.ty.span,
        });
    }
    fields
}

/// Stage 0c for the whole compilation: refuse types that cannot be laid out.
///
/// A record contains its fields by value and an enum contains its active payload by value,
/// so a chain of either that returns to where it started describes a value containing
/// itself. `docs/spec.md` refuses it as an **impossible layout** rather than as a safety
/// problem, and states the rule over *inline containment* so that a future indirection can
/// break a cycle without the rule changing.
///
/// One graph over both kinds since N10: a cycle that runs through an enum is the same
/// impossibility as one that does not, and a walk that knew only about records would have
/// run forever on `struct A { b: B } enum B { V(a: A) }`.
///
/// Reported before anything lowers, so LLVM never sees a recursive type — a backend
/// complaining about one is a refusal that arrives after the compiler decided the program
/// was fine.
pub fn validate_type_layouts(res: &mut Resolution, errors: &mut Vec<Diagnostic>) {
    // Every array written in a field (Gate 2), once every definition it might contain is known.
    validate_new_arrays(res, errors);
    // Over **definitions**, since N11. An applied type is not a declaration and has no
    // name span of its own to point at, and whether it can contain itself is decided by
    // the definitions it applies: `nazm_sema::UserTypes::ownership_cycles` says how.
    let cycles = res.types().ownership_cycles();
    let mut reported: Vec<TypeNode> = Vec::new();
    for cycle in &cycles {
        reported.extend(cycle.through.iter().copied());
        let Some(first) = cycle.through.first().copied() else {
            continue;
        };
        let Some(start_span) = node_span(res, first) else {
            continue;
        };
        let names: Vec<String> = cycle
            .through
            .iter()
            .map(|n| res.types().show(n.ty()))
            .collect();
        let start = res.types().show(first.ty());
        let closing = cycle.hops.last().and_then(|via| closing_field(res, via));
        let d = match cycle.reach {
            nazm_sema::Reach::Inline => {
                let message = if names.len() == 2 {
                    format!("`{start}` contains itself")
                } else {
                    format!("`{start}` contains itself through {}", names.join(" → "))
                };
                Diagnostic::error(codes::RECURSIVE_RECORD, start_span, message).help(
                    "a record holds its fields by value and an enum holds its active \
                     payload by value, so this one would have no finite size. This is an \
                     impossible layout rather than a safety problem: a later type that \
                     holds its contents somewhere else would make the same shape \
                     expressible",
                )
            }
            nazm_sema::Reach::Counted => {
                let path = cycle
                    .hops
                    .iter()
                    .map(|via| hop_name(res, via))
                    .chain(std::iter::once(start.clone()))
                    .collect::<Vec<_>>()
                    .join(" → ");
                Diagnostic::error(
                    codes::OWNERSHIP_CYCLE,
                    start_span,
                    format!("`{start}` can own itself through a `Vec`: {path}"),
                )
                .help(
                    "a `Vec` holds its elements through a reference-counted handle, so this \
                     layout is finite — but a value of it could hold a `Vec` containing \
                     itself, and no count would ever reach zero. Nazm refuses such an \
                     ownership cycle rather than leak it. Store the children somewhere that \
                     does not own them: an index into a `Vec` owned outside the type",
                )
            }
        };
        errors.push(match closing {
            Some(at) => d.secondary(at, "this field closes the cycle"),
            None => d,
        });
    }

    // Depth is only meaningful once there is no cycle to walk forever, and it is asked of
    // declarations: an applied type's depth is its definition's with its arguments filled.
    let nodes: Vec<TypeNode> = (0..res.records().len())
        .map(|i| TypeNode::Record(RecordId::from_index(i)))
        .filter(|n| is_declaration(res, *n))
        .chain(
            (0..res.enums().len())
                .map(|i| TypeNode::Enum(EnumId::from_index(i)))
                .filter(|n| is_declaration(res, *n)),
        )
        .collect();
    for node in &nodes {
        if reported.contains(node) {
            continue;
        }
        if res.types().depth(node.ty()).is_none() {
            let Some(at) = node_span(res, *node) else {
                continue;
            };
            errors.push(
                Diagnostic::error(
                    codes::TYPE_NESTING_TOO_DEEP,
                    at,
                    format!("user-defined types may nest {MAX_TYPE_DEPTH} deep"),
                )
                .help(
                    "each level multiplies the copy and cleanup code generated for every \
                     value of the outer type, so the bound is on the emitted program \
                     rather than on the compiler. It is stated rather than discovered",
                ),
            );
        }
    }
}

/// Whether a table entry is a definition as written, rather than an application of one.
fn is_declaration(res: &Resolution, node: TypeNode) -> bool {
    match node {
        TypeNode::Record(id) => res.records().get(id).is_some_and(|d| d.instance.is_none()),
        TypeNode::Enum(id) => res.enums().get(id).is_some_and(|d| d.instance.is_none()),
    }
}

/// One hop of an ownership path, as a reader would find it: `Node.children`, or
/// `Tree.Branch.kids` for a payload field.
fn hop_name(res: &Resolution, via: &nazm_sema::CycleEdge) -> String {
    match (via.from, via.variant) {
        (TypeNode::Record(id), _) => res
            .records()
            .get(id)
            .and_then(|d| {
                d.fields
                    .get(via.field as usize)
                    .map(|f| format!("{}.{}", d.name, f.name))
            })
            .unwrap_or_default(),
        (TypeNode::Enum(id), Some(v)) => res
            .enums()
            .get(id)
            .and_then(|d| {
                let vd = d.variants.get(v as usize)?;
                vd.fields
                    .get(via.field as usize)
                    .map(|f| format!("{}.{}.{}", d.name, vd.name, f.name))
            })
            .unwrap_or_default(),
        (TypeNode::Enum(_), None) => String::new(),
    }
}

/// Where the field that closes a cycle is written.
fn closing_field(res: &Resolution, via: &nazm_sema::CycleEdge) -> Option<Span> {
    match (via.from, via.variant) {
        (TypeNode::Record(id), _) => res
            .records()
            .get(id)?
            .fields
            .get(via.field as usize)
            .map(|f| f.name_span),
        (TypeNode::Enum(id), Some(v)) => res
            .enums()
            .get(id)?
            .variants
            .get(v as usize)?
            .fields
            .get(via.field as usize)
            .map(|f| f.name_span),
        (TypeNode::Enum(_), None) => None,
    }
}

/// Where a definition's name is written.
fn node_span(res: &Resolution, node: TypeNode) -> Option<Span> {
    match node {
        TypeNode::Record(id) => res.records().get(id).map(|d| d.name_span),
        TypeNode::Enum(id) => res.enums().get(id).map(|d| d.name_span),
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
    let mut names: HashMap<String, Type> = HashMap::new();
    for (_, r) in &unit.records {
        if let Some(id) = res.records().declared(unit.module, &r.name) {
            names.insert(r.name.clone(), Type::Record(id));
        }
    }
    for (_, e) in &unit.enums {
        if let Some(id) = res.enums().declared(unit.module, &e.name) {
            names.insert(e.name.clone(), Type::Enum(id));
        }
    }

    let mut brought_in: HashMap<String, Span> = HashMap::new();
    let mut refused: Vec<String> = Vec::new();
    let mut aliases: Vec<String> = Vec::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        // Under a name (N49): every nameable type as `alias::Name`, colliding with nothing.
        for (alias, _) in &d.aliases {
            aliases.push(alias.clone());
            for (name, offer) in offered_types(iface, res) {
                names.insert(qualified_key(alias, &name), offer);
            }
        }
        if !d.plain {
            continue;
        }
        // Records then enums, each the ones this interface makes nameable. One loop body
        // for both: which kind a name refers to changes the `Type` that goes in the map
        // and nothing else about the rule.
        for (name, offer) in offered_types(iface, res) {
            let Some(declared_at) = user_type_span(res, offer) else {
                continue;
            };
            if let Some(own) = names
                .get(&name)
                .copied()
                .and_then(|t| user_type_span(res, t))
                .filter(|_| {
                    res.records().declared(unit.module, &name).is_some()
                        || res.enums().declared(unit.module, &name).is_some()
                })
            {
                // The prelude's names are reserved rather than colliding at an import: no
                // `use` was written, so there is no import line to point at, and the rule
                // is about the declaration. A project `Result` would otherwise be the one
                // ordinary lookup finds while `?` still meant the core one.
                if Some(d.on) == unit.prelude {
                    if collisions == Collisions::Report {
                        errors.push(
                            Diagnostic::error(
                                codes::PRELUDE_TYPE_REDECLARED,
                                own,
                                format!("`{name}` is a core prelude type"),
                            )
                            .secondary(declared_at, "declared by the core prelude here")
                            .help(
                                "every module sees the prelude's `Result` and `Option` \
                                 without importing them, and there is no way to shadow \
                                 one. Give this type another name",
                            ),
                        );
                    }
                    refused.push(name);
                    continue;
                }
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::IMPORT_COLLIDES_WITH_DEFINITION,
                            d.at,
                            format!(
                                "this import brings in the type `{name}`, which this \
                                 module declares"
                            ),
                        )
                        .secondary(own, "declared here")
                        .secondary(declared_at, "and exported here")
                        .help(
                            "an imported type and a local one are not ranked: rename one \
                             of them, or stop exporting the other",
                        ),
                    );
                }
                refused.push(name);
                continue;
            }
            if let Some(first) = brought_in.get(&name) {
                // Only reachable when another import exports a prelude name, which that
                // module's own declaration was already refused for. One mistake, one
                // error: the prelude's meaning stands and nothing more is said here.
                if Some(d.on) == unit.prelude {
                    names.insert(name, offer);
                    continue;
                }
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::AMBIGUOUS_IMPORT,
                            d.at,
                            format!("the type `{name}` is exported by two imported modules"),
                        )
                        .secondary(*first, "also brought in here")
                        .secondary(declared_at, "exported here")
                        .help(
                            "there is no way to write which one is meant, so the \
                             ambiguity is refused where it is created rather than where \
                             it is used",
                        ),
                    );
                }
                refused.push(name);
                continue;
            }
            brought_in.insert(name.clone(), d.at);
            names.insert(name, offer);
        }
    }
    // Constants are computed here, where every type position can read them; their refusals
    // are reported by the pass that reports collisions, once.
    let mut scratch = Vec::new();
    let sink = if matches!(collisions, Collisions::Report) {
        errors
    } else {
        &mut scratch
    };
    let consts = crate::constants::evaluate(&unit.consts, sink);
    Types {
        names,
        refused,
        aliases,
        consts,
    }
}

/// Hold every array type written since the last stage to its element and size rules (Gate 2).
pub fn validate_new_arrays(res: &mut Resolution, errors: &mut Vec<Diagnostic>) {
    for (t, at) in res.take_arrays_to_validate() {
        if let Some(d) = array_refusal(res, t, at) {
            errors.push(d);
        }
    }
}

/// An array's length, written as an integer literal or a constant's name (Gate 2).
fn array_length(types: &Types, e: &Expr, errors: &mut Vec<Diagnostic>) -> Option<u32> {
    let n = match e {
        Expr::Int { value, .. } => Some(i128::from(*value)),
        Expr::Var { name, .. } => types.consts.values.get(name).and_then(|(_, v)| v.as_int()),
        _ => None,
    };
    let Some(n) = n else {
        errors.push(
            Diagnostic::error(
                codes::NOT_A_CONSTANT,
                e.span(),
                "an array's length is an integer literal or an `Int` constant's name",
            )
            .help("declare one at the top of the module: `const SIZE: Int = 64;`"),
        );
        return None;
    };
    match u32::try_from(n) {
        Ok(n) if n > 0 => Some(n),
        _ => {
            errors.push(Diagnostic::error(
                codes::ARRAY_TOO_LARGE,
                e.span(),
                format!("an array has 1 to {} elements, and this is {n}", u32::MAX),
            ));
            None
        }
    }
}

/// Why an array type is refused, as a diagnostic at `at` (Gate 2); `None` when it is one the
/// language has.
fn array_refusal(res: &Resolution, t: Type, at: Span) -> Option<Diagnostic> {
    let Type::Array(a) = t else {
        return None;
    };
    let (elem, len) = res.types().array_info(a)?;
    match res.types().array_problem(elem, len)? {
        nazm_sema::ArrayProblem::Empty => Some(Diagnostic::error(
            codes::ARRAY_TOO_LARGE,
            at,
            "an array has at least one element",
        )),
        nazm_sema::ArrayProblem::NotPlain(owner) => Some(
            Diagnostic::error(
                codes::ARRAY_ELEMENT,
                at,
                format!(
                    "an array holds plain data, and `{}` holds a `{}`",
                    res.types().show(elem),
                    res.types().show(owner)
                ),
            )
            .help(
                "an array's elements are numbers, `Bool`s, and records, enums and arrays of \
                 them — values that own nothing and are copied by their bytes; a collection \
                 of strings or handles is a `Vec`",
            ),
        ),
        nazm_sema::ArrayProblem::TooLarge(bytes) => Some(
            Diagnostic::error(
                codes::ARRAY_TOO_LARGE,
                at,
                format!(
                    "`{}` holds {bytes} bytes, and an array holds at most {}",
                    res.types().show(t),
                    nazm_sema::MAX_ARRAY_BYTES
                ),
            )
            .help("an array lives inline, on a task's stack; a larger buffer is a `Vec`"),
        ),
    }
}

/// Record what a module may name, as its checker is about to use it (N19): every function
/// and built-in in its environment, every type in its type namespace and every built-in
/// type — less the names the environments refused, which mean nothing unambiguously.
fn record_module_names(res: &mut Resolution, module: ModuleId, env: &Env, types: &Types) {
    use nazm_sema::scope::{defined, Candidate, Named};
    let callables = env
        .names
        .iter()
        .filter(|(n, _)| !env.refused.contains(n))
        .map(|(name, v)| Named {
            name: name.clone(),
            candidate: match v.callee {
                Callee::Builtin(i) => Candidate::Intrinsic(i),
                Callee::Function(id) => Candidate::Entity(nazm_sema::ReferenceTarget::Function(id)),
                // An environment holds named functions only; an indirect callee is a value.
                Callee::Indirect(_) => Candidate::Intrinsic(nazm_sema::Intrinsic::StrLen),
            },
        })
        .collect();
    let mut named: Vec<Named> = types
        .names
        .iter()
        .filter(|(n, _)| !types.refused.contains(n))
        .filter_map(|(name, t)| {
            TypeNode::of(*t).map(|node| Named {
                name: name.clone(),
                candidate: defined(node),
            })
        })
        .collect();
    for t in Type::ALL {
        if let Some(name) = t.builtin_name() {
            named.push(Named {
                name: name.to_owned(),
                candidate: Candidate::BuiltinType(*t),
            });
        }
    }
    named.push(Named {
        name: VEC.to_owned(),
        candidate: Candidate::BuiltinVec,
    });
    res.scopes_mut().module(module, callables, named);
}

/// Record a definition's type parameters over its declaration (N19), each with the type
/// name it hides — found here by the same lookup `resolve_written_inner` makes after it.
fn record_type_scope(res: &mut Resolution, span: Span, scope: &[(String, Type)], types: &Types) {
    use nazm_sema::scope::{defined, Candidate};
    let params = scope
        .iter()
        .filter_map(|(name, t)| {
            let Type::Param(p) = t else { return None };
            let hides = if name == VEC {
                Some(Candidate::BuiltinVec)
            } else if let Some(b) = Type::from_name(name) {
                Some(Candidate::BuiltinType(b))
            } else {
                types
                    .names
                    .get(name)
                    .and_then(|t| TypeNode::of(*t))
                    .map(defined)
            };
            Some((name.clone(), *p, hides))
        })
        .collect();
    res.scopes_mut().type_scope(span, params);
}

/// A user-defined type's declared name, or `None` for a built-in.
fn type_name_of(res: &Resolution, t: Type) -> Option<String> {
    match t {
        Type::Record(id) => res.records().get(id).map(|d| d.name.clone()),
        Type::Enum(id) => res.enums().get(id).map(|d| d.name.clone()),
        _ => None,
    }
}

/// Where a user-defined type's name is written.
fn user_type_span(res: &Resolution, t: Type) -> Option<Span> {
    TypeNode::of(t).and_then(|n| node_span(res, n))
}

/// A type name, against the built-ins and then this module's visible types.
///
/// Built-ins first and unshadowable, which is why they are matched before the map is
/// consulted rather than being put in it. A name alone: whether a generic definition may be
/// named without its arguments is the caller's question, because a construction's head and a
/// pattern may and a field's type may not.
fn resolve_name(
    types: &Types,
    res: &Resolution,
    name: &str,
    span: Span,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    if let Some(t) = Type::from_name(name) {
        return t;
    }
    if let Some(t) = types.names.get(name) {
        return *t;
    }
    if name.contains("::") {
        let aliases: Vec<&str> = types.aliases.iter().map(String::as_str).collect();
        errors.push(unknown_qualified(name, &aliases, span, "type"));
        return Type::Int;
    }
    // A trait where a type is expected (N78): there are no trait objects.
    let bare = name.rsplit("::").next().unwrap_or(name);
    if let Some((_, t)) = res.traits().iter().find(|(_, t)| t.name == bare) {
        errors.push(
            Diagnostic::error(
                codes::TRAIT_NOT_A_TYPE,
                span,
                format!("`{name}` is a trait, not a type"),
            )
            .secondary(t.name_span, "the trait")
            .help(format!(
                "a value has one concrete type; to accept any type with `{bare}`, take a type \
                 parameter that requires it: `fn f[T: {bare}](x: T)`"
            )),
        );
        return Type::Int;
    }
    let mut d = Diagnostic::error(codes::UNKNOWN_TYPE, span, format!("`{name}` is not a type"))
        .help(format!(
            "the built-in types are {}, and `struct` and `enum` declare others",
            Type::all_names()
        ));
    // The same courtesy the function namespace extends: a type that exists somewhere is
    // more usefully reported as unreachable than as non-existent.
    let elsewhere = res
        .records()
        .declared_anywhere(name)
        .next()
        .and_then(|id| res.records().get(id))
        .map(|d| d.name_span)
        .or_else(|| {
            res.enums()
                .declared_anywhere(name)
                .next()
                .and_then(|id| res.enums().get(id))
                .map(|d| d.name_span)
        });
    if let Some(at) = elsewhere {
        d = d.secondary(at, "a type of this name is declared here");
    }
    errors.push(d);
    // Continue as `Int` rather than abandoning: one unknown type should not suppress
    // every other error around it.
    Type::Int
}

/// A written type, resolved: its name, its arguments, and the application they make.
///
/// Where a type is written in full — a field, a parameter, a return, a type argument. A
/// type parameter in `scope` shadows every other name; `Vec` is the one generic built-in;
/// and a generic definition must be written with exactly as many arguments as it declares.
/// The bound on how deeply a written type may nest is checked once, at the top.
fn resolve_written(
    types: &Types,
    res: &mut Resolution,
    written: &TypeName,
    scope: &[(String, Type)],
    effect: Option<(&str, ParamOwner)>,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    let t = resolve_written_inner(types, res, written, scope, effect, errors);
    if res.types().nesting(t) > MAX_TYPE_DEPTH {
        errors.push(
            Diagnostic::error(
                codes::TYPE_NESTING_TOO_DEEP,
                written.span,
                format!("types may nest {MAX_TYPE_DEPTH} deep"),
            )
            .help(
                "each level multiplies the copy and cleanup code generated for every value \
                 of the outer type, so the bound is on the emitted program rather than on \
                 the compiler. It is stated rather than discovered",
            ),
        );
        return Type::Int;
    }
    t
}

fn resolve_written_inner(
    types: &Types,
    res: &mut Resolution,
    written: &TypeName,
    scope: &[(String, Type)],
    effect: Option<(&str, ParamOwner)>,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    let args: Option<Vec<Type>> = written.args.as_ref().map(|a| {
        a.iter()
            .map(|w| resolve_written_inner(types, res, w, scope, effect, errors))
            .collect()
    });
    // An effect parameter is named only in function types (N51): inside a type's
    // arguments it would make two instances of one type that differ only in effects.
    if let (Some((name, owner)), Some(args)) = (effect, &args) {
        if args
            .iter()
            .any(|a| res.types().names_effect_param(*a, owner))
        {
            errors.push(
                Diagnostic::error(
                    codes::EFFECT_PARAMETER,
                    written.span,
                    format!("the effect parameter `{name}` is named inside this type's arguments"),
                )
                .help(
                    "an effect parameter may be named in a function type that is a \
                     parameter's or the result's, or one nested in those: write the function \
                     type there",
                ),
            );
        }
    }
    // `[T; N]` (Gate 2): an array of the element written, its length a constant.
    if let (Some(len), true) = (&written.length, written.name == TypeName::ARRAY) {
        let elem = args
            .as_ref()
            .and_then(|a| a.first().copied())
            .unwrap_or(Type::Int);
        let n = array_length(types, len, errors).unwrap_or(1);
        let t = res.types_mut().array_of(elem, n);
        res.record_array_written(t, written.span);
        return t;
    }
    // `fn(T…) -> R ! {e}` (N50): its parts resolved as any type is, interned.
    if let Some(f) = &written.function {
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| resolve_written_inner(types, res, p, scope, effect, errors))
            .collect();
        let ret = resolve_written_inner(types, res, &f.ret, scope, effect, errors);
        let effects = f
            .effects
            .as_ref()
            .and_then(|d| crate::effects::declared(d, effect.map(|e| e.0), errors))
            .unwrap_or(nazm_sema::EffectSet::EMPTY);
        return res.types_mut().fn_of(nazm_sema::FnSignature {
            params,
            ret,
            effects,
            row: effect.map(|e| e.1),
        });
    }
    // A qualified type names a module's definition (N49): never a type parameter, never `Vec`
    // or a built-in, which belong to no module — so those branches are for plain names only.
    let qualified = written.qualifier.is_some();
    if let Some((_, t)) = scope.iter().find(|(n, _)| !qualified && *n == written.name) {
        if args.is_some() {
            errors.push(not_generic(&written.name, "a type parameter", written.span));
        }
        // Which parameter this name is, recorded where it was decided (N23).
        if let Type::Param(p) = *t {
            res.record_written_type(written.name_span, WrittenType::Param(p));
        }
        return *t;
    }
    // `Chan[T]` (N53): `Chan` with an argument is a generic channel, for a `T` that may cross
    // into a task; `Chan` alone is the `Int` channel, below.
    if !qualified && written.name == "Chan" {
        if let Some(args) = args {
            if args.len() != 1 {
                errors.push(wrong_arity("Chan", 1, args.len(), written.span));
            }
            let elem = args.first().copied().unwrap_or(Type::Int);
            if let Some(d) = channel_element(res.types(), elem, written.span) {
                errors.push(d);
            }
            let chan = res.types_mut().chan_of(elem);
            res.record_written_type(written.name_span, WrittenType::Builtin(chan));
            return chan;
        }
    }
    if !qualified && written.name == VEC {
        let Some(args) = args else {
            errors.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_ARGUMENTS,
                    written.name_span,
                    "`Vec` is written with its element type",
                )
                .help("a vector of `Int` is `Vec[Int]`; `Vec` alone is not a type"),
            );
            return Type::Int;
        };
        if args.len() != 1 {
            errors.push(wrong_arity("Vec", 1, args.len(), written.span));
        }
        let elem = args.first().copied().unwrap_or(Type::Int);
        let vec = res.types_mut().vec_of(elem);
        // The one generic built-in, as the vector type it was resolved to (N23).
        res.record_written_type(written.name_span, WrittenType::Builtin(vec));
        return vec;
    }
    let key = written_key(written.qualifier.as_ref(), &written.name);
    let t = resolve_name(types, res, &key, written.name_span, errors);
    // Which definition this name is, recorded where it was decided (N18). A built-in is
    // not one, and neither is a type parameter, which returned above.
    if let Some(node) = TypeNode::of(t) {
        res.record_type_name(written.name_span, node);
    }
    if !qualified && Type::from_name(&written.name).is_some() {
        if args.is_some() {
            errors.push(not_generic(&written.name, "a built-in type", written.span));
        }
        // A built-in, as the type resolution decided it is (N23).
        res.record_written_type(written.name_span, WrittenType::Builtin(t));
        return t;
    }
    let Some(node) = TypeNode::of(t) else {
        return t;
    };
    let arity = res.types().declared_params(node);
    match (args, arity) {
        (None, 0) => t,
        (None, n) => {
            let shown = res.types().show(t);
            errors.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_ARGUMENTS,
                    written.name_span,
                    format!(
                        "`{shown}` is generic, so it is written with its {n} type argument{}",
                        if n == 1 { "" } else { "s" }
                    ),
                )
                .secondary(
                    user_type_span(res, t).unwrap_or(written.name_span),
                    "declared here",
                )
                .help(format!(
                    "write `{shown}[…]` with {n} type{}",
                    if n == 1 { "" } else { "s" }
                )),
            );
            Type::Int
        }
        (Some(_), 0) => {
            let shown = res.types().show(t);
            errors.push(not_generic(&shown, "not generic", written.span));
            t
        }
        (Some(mut args), n) => {
            if args.len() != n {
                errors.push(wrong_arity(
                    &res.types().show(t),
                    n,
                    args.len(),
                    written.span,
                ));
                args.resize(n, Type::Int);
            }
            apply(res, node, args)
        }
    }
}

/// A generic definition applied to its arguments, interned.
fn apply(res: &mut Resolution, node: TypeNode, args: Vec<Type>) -> Type {
    match node {
        TypeNode::Record(id) => res.types_mut().apply_record(id, args),
        TypeNode::Enum(id) => res.types_mut().apply_enum(id, args),
    }
}

/// `N0351`: type arguments written on something that takes none.
fn not_generic(name: &str, what: &str, at: Span) -> Diagnostic {
    let why = if what == "not generic" {
        format!("`{name}` is not generic, so it takes no type arguments")
    } else {
        format!("`{name}` is {what}, so it takes no type arguments")
    };
    Diagnostic::error(codes::TYPE_ARGUMENTS_ON_NON_GENERIC, at, why)
        .help("only a definition that declares type parameters is applied to type arguments")
}

/// `N0353`: the wrong number of type arguments.
fn wrong_arity(name: &str, want: usize, got: usize, at: Span) -> Diagnostic {
    Diagnostic::error(
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        at,
        format!(
            "`{name}` takes {want} type argument{}, but {got} {} given",
            if want == 1 { "" } else { "s" },
            if got == 1 { "was" } else { "were" }
        ),
    )
    .help("a generic definition is applied to exactly as many types as it declares")
}

/// The C symbols the native runtime and the platform entry declare themselves (N42): a
/// foreign declaration of one could disagree with the runtime's, so each is refused (`N0383`).
/// `a_reserved_c_symbol_is_every_one_the_runtime_calls` keeps this list the registry's.
pub const RESERVED_C_SYMBOLS: &[&str] = &[
    "exit",
    "fclose",
    "fopen",
    "fread",
    "free",
    "fseek",
    "ftell",
    "fwrite",
    "getenv",
    "main",
    "malloc",
    "memcmp",
    "memcpy",
    "printf",
    "pthread_cond_broadcast",
    "pthread_cond_destroy",
    // The stack guard and the task threads' stack size (N49).
    "getrlimit",
    "pthread_attr_init",
    "pthread_attr_setstacksize",
    "pthread_attr_destroy",
    "pthread_cond_init",
    "pthread_cond_wait",
    // Select's deadline and the clock (N54).
    "pthread_cond_timedwait",
    "clock_gettime",
    // The task pool: its settings, its tasks' guarded stacks, and their zeroing (N83).
    "atoll",
    "getpagesize",
    "posix_memalign",
    "mprotect",
    "memset",
    "nz_ctx_switch",
    "nz_ctx_prepare",
    "nz_task_main",
    // Where `errno` is (N85): the runtime's own question, and each target's answer to it.
    "nz_errno_location",
    "__error",
    "__errno_location",
    "pthread_create",
    "pthread_join",
    "pthread_mutex_destroy",
    "pthread_mutex_init",
    "pthread_mutex_lock",
    "pthread_mutex_unlock",
    "realloc",
    "snprintf",
    "strlen",
    "write",
];

/// The rules a foreign declaration must meet (N42, `docs/architecture.md` §7.44).
/// The ABI and the symbol of a foreign declaration or an export (N42, N55): `"C"`, a C
/// identifier, and none the runtime declares itself.
fn c_abi_and_symbol(d: &nazm_syntax::ForeignDecl, errors: &mut Vec<Diagnostic>) {
    if d.abi != "C" {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_ABI,
                d.abi_span,
                format!("`\"{}\"` is not an ABI this compiler calls", d.abi),
            )
            .help("the one foreign ABI is `\"C\"`: the C calling convention of the target"),
        );
    }
    let first_ok = d
        .symbol
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    let rest_ok = d
        .symbol
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !first_ok || !rest_ok || d.symbol.len() > 255 {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SYMBOL,
                d.symbol_span,
                format!("`\"{}\"` is not a C symbol", d.symbol.escape_debug()),
            )
            .help(
                "a foreign symbol is a C identifier: ASCII letters, digits and `_`, not \
                 starting with a digit, at most 255 bytes",
            ),
        );
    }
    if RESERVED_C_SYMBOLS.contains(&d.symbol.as_str()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_CONFLICT,
                d.symbol_span,
                format!(
                    "`{}` is a symbol the native runtime already declares",
                    d.symbol
                ),
            )
            .help(
                "the runtime and the platform entry call these themselves with their own \
                 signatures; a foreign declaration of one could disagree with it",
            ),
        );
    }
}

fn check_foreign(
    f: &Function,
    d: &nazm_syntax::ForeignDecl,
    params: &[Type],
    ret: Type,
    res: &Resolution,
    option: Option<EnumId>,
    errors: &mut Vec<Diagnostic>,
) {
    c_abi_and_symbol(d, errors);
    if let (Some(first), Some(last)) = (f.type_params.first(), f.type_params.last()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                first.span.join(last.span),
                "a foreign function cannot be generic",
            )
            .help("a C function has one signature; declare one per type"),
        );
    }
    if let Some(e) = &f.effects {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                e.span,
                "a foreign function's effects are not written",
            )
            .help("calling one is always the effect `foreign`, which needs a `ForeignCap`"),
        );
    }
    for (t, p) in params.iter().zip(&f.params) {
        if foreign_param(res, *t).is_none() {
            errors.push(foreign_type(p.ty.span, "take"));
        }
    }
    if foreign_result(res, option, ret).is_none() {
        errors.push(foreign_type(f.return_type.span, "return"));
    }
}

/// How a foreign function's parameter of type `t` crosses (N42, N55, N85), or `None` when it
/// cannot: the one table every consumer — the checker, Core IR lowering — reads.
#[must_use]
pub fn foreign_param(res: &Resolution, t: Type) -> Option<CrossIn> {
    match t {
        Type::Int | Type::Bool => Some(CrossIn::Scalar),
        Type::Str => Some(CrossIn::CString),
        Type::Record(id) => match res.records().get(id)?.repr {
            CRepr::Opaque => Some(CrossIn::Handle(id)),
            CRepr::Layout => Some(CrossIn::Struct),
            CRepr::Nazm => None,
        },
        // A C function pointer (N85): an export's C entry, its parameters and result scalars.
        Type::Fn(id) => {
            let sig = res.types().fn_signature(id)?;
            let scalar = |t: &Type| matches!(t, Type::Int | Type::Bool);
            (sig.params.iter().all(scalar) && scalar(&sig.ret)).then_some(CrossIn::Callback)
        }
        _ => None,
    }
}

/// How a foreign function's result of type `t` crosses back (N85), or `None` when it cannot.
#[must_use]
pub fn foreign_result(res: &Resolution, option: Option<EnumId>, t: Type) -> Option<CrossOut> {
    let handle = |t: Type| match t {
        Type::Record(id) if res.records().get(id)?.repr == CRepr::Opaque => Some(id),
        _ => None,
    };
    match t {
        Type::Int | Type::Bool => Some(CrossOut::Scalar),
        Type::Str => Some(CrossOut::CString { nullable: false }),
        Type::Record(_) => handle(t).map(|id| CrossOut::Handle {
            record: id,
            nullable: false,
        }),
        Type::Enum(_) => {
            let def = option?;
            if res.types().definition_of(t) != Some(TypeNode::Enum(def)) {
                return None;
            }
            match res.types().type_args(t).as_slice() {
                [Type::Str] => Some(CrossOut::CString { nullable: true }),
                [inner] => handle(*inner).map(|id| CrossOut::Handle {
                    record: id,
                    nullable: true,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A foreign parameter's crossing (N85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossIn {
    /// `Int` or `Bool`, by value.
    Scalar,
    /// A `Str`, as a borrowed NUL-terminated copy (N55).
    CString,
    /// An opaque handle: its pointer.
    Handle(RecordId),
    /// A C-layout record, as a borrowed pointer to a copy.
    Struct,
    /// An export's C entry, as a C function pointer.
    Callback,
}

/// A foreign result's crossing (N85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOut {
    /// `Int` or `Bool`, by value.
    Scalar,
    /// A `const char *` C keeps, copied; null is `None` when `nullable`, a failure otherwise.
    CString { nullable: bool },
    /// A pointer, as a handle; null is `None` when `nullable`, a failure otherwise.
    Handle { record: RecordId, nullable: bool },
}

/// The effect parameter a function declares, if any (N51): the first, with every further one
/// and one a foreign function writes refused (`N0388`), and one on a function that declares no
/// effect set too — what such a function's body requires would name a parameter its callers
/// never bind.
fn declared_effect_param<'f>(f: &'f Function, errors: &mut Vec<Diagnostic>) -> Option<&'f str> {
    let first = f.effect_params.first()?;
    for extra in f.effect_params.iter().skip(1) {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                extra.span,
                format!("`{}` declares a second effect parameter", f.name),
            )
            .secondary(first.span, "the first")
            .help("a function declares at most one: every set it writes may name that one"),
        );
    }
    if f.foreign.is_some() {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                first.span,
                "a foreign function cannot have an effect parameter",
            )
            .help("calling C is always the effect `foreign`"),
        );
        return None;
    }
    if f.effects.is_none() {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                first.span,
                format!(
                    "`{}` has an effect parameter, so it declares its effect set",
                    f.name
                ),
            )
            .help(format!(
                "write `! {{ {} }}` after the result type: a function with an effect parameter \
                 states what it does in terms of it",
                first.name
            )),
        );
    }
    Some(first.name.as_str())
}

/// The rules an export meets (N55, `docs/architecture.md` §7.57): the foreign declaration's ABI
/// and symbol rules, and — since C can hand Nazm no capability and has no cleanup Nazm knows of —
/// public, `! {}`, not generic, and only `Int` and `Bool` across.
fn check_export(
    f: &Function,
    d: &nazm_syntax::ForeignDecl,
    params: &[Type],
    ret: Type,
    errors: &mut Vec<Diagnostic>,
) {
    c_abi_and_symbol(d, errors);
    let shape = |at: Span, what: String| {
        Diagnostic::error(codes::EXPORT_SHAPE, at, what).help(
            "an export is called by C, which can hand Nazm no capability and has no cleanup \
             Nazm knows of: it is `pub`, declares `! {}`, is not generic, and takes and \
             returns only `Int` and `Bool`",
        )
    };
    if f.pub_span.is_none() {
        errors.push(shape(
            f.name_span,
            format!("`{}` is exported to C, so it is `pub`", f.name),
        ));
    }
    match &f.effects {
        Some(e) if e.names.is_empty() => {}
        Some(e) => errors.push(shape(
            e.span,
            format!("`{}` is exported to C, so it performs no effect", f.name),
        )),
        None => errors.push(shape(
            f.return_type.span,
            format!("`{}` is exported to C, so it declares `! {{}}`", f.name),
        )),
    }
    if let (Some(first), Some(last)) = (f.type_params.first(), f.type_params.last()) {
        errors.push(shape(
            first.span.join(last.span),
            format!("`{}` is exported to C, so it cannot be generic", f.name),
        ));
    }
    let scalar = |t: Type| matches!(t, Type::Int | Type::Bool);
    for (t, p) in params.iter().zip(&f.params) {
        if !scalar(*t) {
            errors.push(shape(
                p.ty.span,
                format!("`{}` is exported to C, so it cannot take this type", f.name),
            ));
        }
    }
    if !scalar(ret) {
        errors.push(shape(
            f.return_type.span,
            format!(
                "`{}` is exported to C, so it cannot return this type",
                f.name
            ),
        ));
    }
}

fn foreign_type(at: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::FOREIGN_TYPE,
        at,
        format!("a foreign function cannot {what} this type"),
    )
    .primary_label("not FFI-safe")
    .help(
        "an argument is `Int`, `Bool`, a `Str` (borrowed), an opaque handle, an `extern \"C\" \
         struct` (by borrowed pointer) or an export's function type; a result is `Int`, `Bool`, \
         a handle or a `Str` (copied), or an `Option` of either when C may return null. An \
         ordinary record or enum has no C layout, and a capability is never a word C can forge",
    )
}

/// Every module's interface, as a compilation declares them: types, their fields and layouts,
/// traits' methods, each module's definitions, then what `pub use` re-exports (N103). For a
/// tool that publishes an interface without checking bodies — the same stages, in the same
/// order, as [`check`] runs before any body.
pub fn declared_interfaces(
    units: &[Unit<'_>],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<UnitInterface> {
    res.types_mut().defer();
    let mut types = declare_types(units, res, errors);
    apply_reexports(units, &mut types, res, &mut Vec::new(), false);
    for u in units {
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| types.get(d.on.index()))
            .collect();
        resolve_type_fields(u, &deps, res, errors);
    }
    res.types_mut().settle();
    validate_type_layouts(res, errors);
    declare_trait_methods(units, &types, res, errors);
    let mut interfaces: Vec<UnitInterface> = units
        .iter()
        .map(|u| {
            let deps: Vec<&UnitInterface> = u
                .imports
                .iter()
                .filter_map(|d| types.get(d.on.index()))
                .collect();
            declare_unit(u, &deps, res, errors)
        })
        .collect();
    apply_reexports(units, &mut interfaces, res, errors, true);
    validate_new_arrays(res, errors);
    interfaces
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

    // Each impl's header, resolved once (N78): its trait and its type, or `None` when either
    // did not resolve — reported here, and its methods then declared as nothing at all.
    let trait_names = trait_scope(unit, res);
    let mut headers: Vec<(Span, Option<(TraitId, Type)>)> = Vec::new();
    for imp in &unit.impls {
        let trait_id = match named_trait(&imp.trait_name, &trait_names, res, unit) {
            Ok(t) => {
                res.traits_mut().record_mention(imp.trait_name.name_span, t);
                Some(t)
            }
            Err(d) => {
                errors.push(d);
                None
            }
        };
        let before = errors.len();
        let ty = resolve_written(&types, res, &imp.self_ty, &[], None, errors);
        let ty =
            (errors.len() == before)
                .then_some(ty)
                .and_then(|ty| match implementable(res, ty) {
                    Ok(_) => Some(ty),
                    Err(why) => {
                        errors.push(
                            Diagnostic::error(
                                codes::IMPL_TYPE_REFUSED,
                                imp.self_ty.span,
                                format!("`{}` cannot have an impl: it is {why}", imp.self_ty.name),
                            )
                            .help(
                                "an impl is for a non-generic record or enum, or for `Int`, \
                             `Bool` or `Str`",
                            ),
                        );
                        None
                    }
                });
        headers.push((imp.impl_span, trait_id.zip(ty)));
    }

    // Each foreign symbol's first declaration in this module, for `N0383`.
    let mut foreign_seen: std::collections::HashMap<String, (Vec<Type>, Type, Span)> =
        std::collections::HashMap::new();
    for (decl, f) in &unit.functions {
        // An impl's method (N78): declared under `Trait/Type/m`, with `Self` its impl's type.
        let method = match &f.method_of {
            Some(m) => match headers.iter().find(|(at, _)| *at == m.impl_span) {
                Some((_, Some(h))) => Some(*h),
                // Its impl's header did not resolve, which is reported: nothing to declare.
                _ => continue,
            },
            None => None,
        };
        let name = method.map_or_else(
            || f.name.clone(),
            |(t, ty)| {
                let trait_name = res.traits().get(t).map_or("?", |d| d.name.as_str());
                impl_method_name(trait_name, &res.types().show(ty), &f.name)
            },
        );
        // A generic function's parameter types mention its own parameters, and a parameter
        // names its owner — so the id comes first. Nothing else declares a function between
        // here and the `declare` below, so the id this one would get is known now.
        let id = res.next_function();
        let generics = declared_type_params(&f.type_params, errors);
        let mut scope = param_scope(&f.type_params, ParamOwner::Function(id));
        if let Some((_, ty)) = method {
            scope.push(("Self".to_owned(), ty));
        }
        let effect = declared_effect_param(f, errors).map(|n| (n, ParamOwner::Function(id)));
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| resolve_written(&types, res, &p.ty, &scope, effect, errors))
            .collect();
        let ret = resolve_written(&types, res, &f.return_type, &scope, effect, errors);
        let mut declared_effects = f
            .effects
            .as_ref()
            .and_then(|d| crate::effects::declared(d, effect.map(|e| e.0), errors));
        // An impl method that writes no effect set has its trait method's (N78): a trait's
        // signature is a promise, and its impls keep it.
        if let (None, Some((t, _))) = (declared_effects, method) {
            declared_effects = res
                .traits()
                .get(t)
                .and_then(|d| d.method(&f.name).and_then(|i| d.methods.get(i as usize)))
                .map(|m| m.effects);
        }
        if let Some((name, owner)) = effect {
            res.types_mut().name_effect_param(owner, name.to_owned());
        }

        // A built-in has no source span to point at, so "first defined here" would aim
        // at byte zero. Said plainly instead. Built-ins are not a module's definitions,
        // so no visibility rule makes this one legal.
        if method.is_none()
            && (Intrinsic::by_name(&f.name).is_some() || CONVERSIONS.contains(&f.name.as_str()))
        {
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
            // Nothing was declared, so the id this signature's parameters were resolved
            // under goes to the next function: what was recorded under it is not theirs.
            res.forget_type_params_of(ParamOwner::Function(id));
            continue;
        }

        // Within this module. Another module defining the same name is another module's
        // definition, and since 2026-09-22 that is not a collision.
        if let Some(existing) = res
            .definition(unit.module, &name)
            .and_then(|id| res.function(id))
        {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    if method.is_some() {
                        format!("`{}` is defined more than once in this impl", f.name)
                    } else {
                        format!("`{}` is defined more than once in this module", f.name)
                    },
                )
                .secondary(existing.name_span, "first defined here"),
            );
            // Its signature is resolved again with its body, under the first definition,
            // which replaces what was recorded here under `id`.
            continue;
        }

        // A foreign declaration (N42): its shape, its ABI, its symbol and its types, each
        // refused by name. What it declares is a contract with no body — the effect
        // `foreign` and nothing else.
        let option = unit
            .prelude
            .and_then(|m| res.enums().declared(m, nazm_sema::prelude::OPTION));
        let foreign = f.foreign.as_ref().map(|d| {
            check_foreign(f, d, &params, ret, res, option, errors);
            match foreign_seen.get(d.symbol.as_str()) {
                Some((p, r, at)) if (p.as_slice(), *r) != (params.as_slice(), ret) => {
                    errors.push(
                        Diagnostic::error(
                            codes::FOREIGN_CONFLICT,
                            d.symbol_span,
                            format!(
                                "the C symbol `{}` is declared again with another signature",
                                d.symbol
                            ),
                        )
                        .secondary(*at, "declared differently here")
                        .help("a C function has one signature; declare it the same way"),
                    );
                }
                Some(_) => {}
                None => {
                    foreign_seen.insert(d.symbol.clone(), (params.clone(), ret, d.symbol_span));
                }
            }
            d.symbol.clone()
        });
        // An export (N55): a Nazm function C may call, under the rules §7.57 states.
        let export = f.export.as_ref().map(|d| {
            check_export(f, d, &params, ret, errors);
            d.symbol.clone()
        });
        let declared_effects = if foreign.is_some() {
            Some(nazm_sema::EffectSet::of(nazm_sema::Effect::Foreign))
        } else {
            declared_effects
        };

        // A method is never in the function namespace: it is reached through its trait.
        let visibility = if method.is_some() {
            Visibility::Private
        } else {
            Visibility::from_keyword(f.pub_span)
        };
        let declared = res.declare(FnDef {
            name: name.clone(),
            module: unit.module,
            visibility,
            params: params.clone(),
            ret,
            name_span: f.name_span,
            decl: *decl,
            locals: Vec::new(),
            type_params: generics.clone(),
            call_sites: Vec::new(),
            declared_effects,
            inferred_effects: None,
            effect_reasons: Vec::new(),
            provenance: None,
            mentions: Vec::new(),
            foreign: foreign.clone(),
            export,
            preconditions: preconditions_of(f),
            closure: None,
        });
        debug_assert_eq!(declared, id);
        declare_type_params(res, &f.type_params, ParamOwner::Function(id));
        res.types_mut()
            .name_params(ParamOwner::Function(id), generics.clone());
        declared_requirements(
            &f.type_params,
            ParamOwner::Function(id),
            &trait_names,
            res,
            errors,
        );

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
            let bounds = (0..u32::try_from(generics.len()).unwrap_or(u32::MAX))
                .filter_map(|index| {
                    let t = res.traits().bound_of(TypeParam {
                        owner: ParamOwner::Function(id),
                        index,
                    })?;
                    let d = res.traits().get(t)?;
                    Some((index, d.name.clone(), d.module))
                })
                .collect();
            exports.push(Export {
                bounds,
                name: f.name.clone(),
                params,
                ret,
                name_span: f.name_span,
                def: id,
                type_params: generics,
                effects: declared_effects,
                foreign: foreign.clone(),
                preconditions: preconditions_of(f),
                origin: None,
            });
        }
    }

    // The same check for an exported type's own fields, and the interface's type set.
    for (imp, (_, header)) in unit.impls.iter().zip(&headers) {
        let Some((trait_id, ty)) = *header else {
            continue;
        };
        register_impl(unit, imp, trait_id, ty, res, errors);
    }

    let mut records: Vec<RecordExport> = Vec::new();
    let mut enums: Vec<EnumExport> = Vec::new();
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
    for (_, e) in &unit.enums {
        let Some(id) = res.enums().declared(unit.module, &e.name) else {
            continue;
        };
        let Some(def) = res.enums().get(id) else {
            continue;
        };
        if !def.visibility.is_exported() {
            continue;
        }
        let name = def.name.clone();
        let name_span = def.name_span;
        // Every payload field of every variant: a public enum's variants and their
        // payloads are all part of the public type, because `docs/spec.md` gives it no
        // per-variant visibility and an importer must be able to match every arm.
        let payloads: Vec<(Type, Span)> = def
            .variants
            .iter()
            .flat_map(|v| v.fields.iter().map(|f| (f.ty, f.ty_span)))
            .collect();
        for (t, at) in payloads {
            report_private_type(t, at, name_span, "enum", &name, res, errors);
        }
        enums.push(EnumExport {
            def: id,
            nameable: true,
        });
        roots.push(Type::Enum(id));
    }
    // Closed over what the public surface mentions: an importer that projects through an
    // exported record or matches an exported enum needs every shape it can reach, and
    // cannot go and read the module that declared one. `RecordExport` says why this is not
    // transitive visibility.
    roots.extend(exports.iter().flat_map(|e| {
        e.params
            .iter()
            .copied()
            .chain(std::iter::once(e.ret))
            .collect::<Vec<_>>()
    }));
    for node in reachable_types(res.types(), roots) {
        match node {
            TypeNode::Record(id) => {
                if !records.iter().any(|r| r.def == id) {
                    records.push(RecordExport {
                        def: id,
                        nameable: false,
                    });
                }
            }
            TypeNode::Enum(id) => {
                if !enums.iter().any(|e| e.def == id) {
                    enums.push(EnumExport {
                        def: id,
                        nameable: false,
                    });
                }
            }
        }
    }

    // What the module offers of traits (N78): its exported traits, and every impl it declares
    // — impls are global to a compilation, so all of them are part of what it offers.
    let traits = res
        .traits()
        .iter()
        .filter(|(_, t)| t.module == unit.module && t.visibility.is_exported())
        .map(|(_, t)| nazm_sema::interface::TraitExport {
            name: t.name.clone(),
            module: t.module,
            methods: t
                .methods
                .iter()
                .map(|m| (m.name.clone(), m.params.clone(), m.ret, m.effects))
                .collect(),
        })
        .collect();
    let impls = res
        .traits()
        .impls()
        .filter(|(_, d)| d.module == unit.module)
        .filter_map(|(_, d)| {
            let t = res.traits().get(d.trait_id)?;
            Some(nazm_sema::interface::ImplExport {
                module: d.module,
                trait_name: t.name.clone(),
                trait_module: t.module,
                self_ty: d.self_ty,
                methods: t.methods.iter().map(|m| m.name.clone()).collect(),
            })
        })
        .collect();
    UnitInterface::new(unit.module, exports, records, enums, unit.imports.to_vec())
        .with_traits(traits, impls)
}

/// Report a public item that names a private user-defined type.
///
/// `kind` and `name` describe the **public** item, so the message says what is leaking
/// and the labels say from where to where. Records and enums both, because the closure
/// `docs/spec.md` requires runs through both and a check that knew one kind would let a
/// private enum out through a public record's field.
fn report_private_type(
    t: Type,
    at: Span,
    item_span: Span,
    kind: &str,
    name: &str,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    // Anywhere in the written type, since N11: `Box[Secret]` exposes `Secret` exactly as a
    // field of type `Secret` would. The first private definition found is reported, once —
    // one written type is one mistake.
    let mut work = vec![t];
    let mut found: Option<(String, Span)> = None;
    while let Some(t) = work.pop() {
        work.extend(res.types().type_args(t));
        let private = match res.types().definition_of(t) {
            Some(TypeNode::Record(id)) => res
                .records()
                .get(id)
                .filter(|d| !d.visibility.is_exported())
                .map(|d| (d.name.clone(), d.name_span)),
            Some(TypeNode::Enum(id)) => res
                .enums()
                .get(id)
                .filter(|d| !d.visibility.is_exported())
                .map(|d| (d.name.clone(), d.name_span)),
            None => None,
        };
        if private.is_some() {
            found = private;
            break;
        }
    }
    let Some((private_name, declared_at)) = found else {
        return;
    };
    errors.push(
        Diagnostic::error(
            codes::PRIVATE_TYPE_EXPOSED,
            at,
            format!("this public {kind} exposes `{private_name}`, which is private to this module"),
        )
        .secondary(item_span, format!("`{name}` is public"))
        .secondary(declared_at, "declared without `pub` here")
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
    let types = type_environment(unit, deps, res, errors, Collisions::Silent);
    record_module_names(res, unit.module, &env, &types);
    // Built from `deps` rather than from a graph, so the one path that checks a module
    // against persisted interfaces resolves type names by exactly the rule the
    // whole-program path uses. Two builders would be two rules.
    let imports = unit.imports.iter().map(|d| d.on).collect();
    let result = unit
        .prelude
        .and_then(|m| res.enums().declared(m, nazm_sema::prelude::RESULT));
    let option = unit
        .prelude
        .and_then(|m| res.enums().declared(m, nazm_sema::prelude::OPTION));
    let impl_modules = impl_scope(unit, res);
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
        return_span: None,
        result,
        option,
        scope_depth: 0,
        loop_depth: 0,
        type_scope: Vec::new(),
        closures: Vec::new(),
        closure_count: 0,
        effect_param: None,
        trait_names: HashMap::new(),
        impl_modules,
        preconditions: HashMap::new(),
    };
    for (_, f) in &unit.functions {
        let requires: Vec<Expr> = f
            .contracts
            .iter()
            .filter(|k| k.kind == nazm_syntax::ContractKind::Requires)
            .map(|k| k.expr.clone())
            .collect();
        if let (false, Some(id)) = (requires.is_empty(), defined(c.res, unit.module, f)) {
            let names = f.params.iter().map(|p| p.name.clone()).collect();
            c.preconditions.insert(id, (names, requires));
        }
    }
    c.trait_names = trait_scope(unit, c.res);
    for (_, f) in &unit.functions {
        c.check_function(f);
    }
    // Effects are a fact about the module's resolved calls, so they are settled once every
    // body has been checked and every call resolved (N36).
    crate::effects::analyse(&unit.functions, unit.module, deps, c.res, &mut c.errors);
    errors.append(&mut c.errors);
}

/// The names one module may write.
///
/// Built-ins, then its own definitions, then the exports of each direct import — in that
/// order, and every conflict between the last two is reported rather than resolved.
/// The key a qualified name is held under in a module's environment (N49): `alias::name`.
/// `::` is in no identifier, so no unqualified name can be the same string.
fn qualified_key(alias: &str, name: &str) -> String {
    format!("{alias}::{name}")
}

/// The key a name written with an optional qualifier is looked up by.
fn written_key(qualifier: Option<&nazm_syntax::Qualifier>, name: &str) -> String {
    match qualifier {
        Some(q) => qualified_key(&q.name, name),
        None => name.to_owned(),
    }
}

/// `NAME::item` that resolves to nothing (N49): `NAME` is no alias, or its module does not
/// export `item`.
fn unknown_qualified(key: &str, aliases: &[&str], at: Span, what: &str) -> Diagnostic {
    let (alias, item) = key.split_once("::").unwrap_or(("", key));
    if aliases.contains(&alias) {
        Diagnostic::error(
            codes::UNKNOWN_QUALIFIED_NAME,
            at,
            format!("the module imported as `{alias}` exports no {what} `{item}`"),
        )
        .help(format!(
            "`{alias}::` names what that module marks `pub`; check the name, or export it there"
        ))
    } else {
        Diagnostic::error(
            codes::UNKNOWN_QUALIFIED_NAME,
            at,
            format!("`{alias}` is not the name of an imported module"),
        )
        .help(format!(
            "import one under that name: `use \"path.nz\" as {alias};`"
        ))
    }
}

fn environment(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Env {
    let mut names: HashMap<String, Visible> = HashMap::new();

    // Built-ins first, so a user definition of one collides with it rather than
    // shadowing it. They share the ordinary function namespace deliberately: a call is a
    // call, and `str_len(s)` should resolve by the same rule as any other. They belong to
    // no module, so every module has all of them.
    for i in Intrinsic::ALL {
        // The table's shapes become types here, once: `Vec[T]` is interned, so it cannot
        // be written in a `const`. `T` is the built-in's own parameter.
        let owner = ParamOwner::Builtin(i);
        let t = Type::Param(TypeParam { owner, index: 0 });
        let mut shape = |s: Shape| match s.monomorphic() {
            Some(m) => m,
            None if s == Shape::T => t,
            None if s == Shape::ChanT => res.types_mut().chan_of(t),
            None if s == Shape::ChansT => {
                let c = res.types_mut().chan_of(t);
                res.types_mut().vec_of(c)
            }
            None => res.types_mut().vec_of(t),
        };
        let params: Vec<Type> = i.params().iter().map(|s| shape(*s)).collect();
        let ret = shape(i.ret());
        if i.generics() > 0 {
            res.types_mut().name_params(owner, vec!["T".to_owned()]);
        }
        names.insert(
            i.name().to_owned(),
            Visible {
                sig: Signature {
                    params,
                    ret,
                    name_span: Span::synthetic(),
                    generics: i.generics(),
                    owner: (i.generics() > 0).then_some(owner),
                    effect_param: false,
                    preconditions: 0,
                    pure: matches!(
                        i,
                        Intrinsic::StrLen
                            | Intrinsic::StrByte
                            | Intrinsic::IntsLen
                            | Intrinsic::IntsGet
                            | Intrinsic::StrsLen
                            | Intrinsic::VecLen
                            | Intrinsic::VecGet
                    ),
                },
                callee: Callee::Builtin(i),
            },
        );
    }

    // This module's own definitions, exported or not. A module sees all of its own. An
    // impl's methods are not among them: a method is reached through its trait (N78).
    for (_, f) in &unit.functions {
        if f.method_of.is_some() {
            continue;
        }
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
                    generics: u32::try_from(def.type_params.len()).unwrap_or(u32::MAX),
                    owner: (!def.type_params.is_empty() || polymorphic(def.declared_effects))
                        .then_some(ParamOwner::Function(id)),
                    effect_param: polymorphic(def.declared_effects),
                    preconditions: def.preconditions,
                    pure: def.foreign.is_none()
                        && def.declared_effects == Some(nazm_sema::EffectSet::EMPTY),
                },
                callee: Callee::Function(id),
            },
        );
    }

    // The exports of each direct import, and both ways that can go wrong. `brought_in`
    // remembers which `use` introduced each name, so the second collision points at the
    // first rather than at "somewhere else".
    let mut brought_in: HashMap<&str, Span> = HashMap::new();
    let mut refused: Vec<String> = Vec::new();
    let mut aliases: HashMap<String, ModuleId> = HashMap::new();
    let mut alias_at: HashMap<String, Span> = HashMap::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        // Under a name (N49): every export as `alias::name`, colliding with nothing — but the
        // alias itself may not be a name this module already gives something else.
        for (alias, at) in &d.aliases {
            let clash = if let Some(first) = alias_at.get(alias) {
                Some((*first, "another import already has this name"))
            } else if names
                .get(alias)
                .is_some_and(|v| matches!(v.callee, Callee::Function(_)))
            {
                names
                    .get(alias)
                    .map(|v| (v.sig.name_span, "a function of this module"))
            } else if res.records().declared(unit.module, alias).is_some()
                || res.enums().declared(unit.module, alias).is_some()
            {
                Some((*at, "a type of this module"))
            } else {
                None
            };
            if let Some((other, what)) = clash {
                errors.push(
                    Diagnostic::error(
                        codes::ALIAS_COLLIDES,
                        *at,
                        format!("`{alias}` already names {what}"),
                    )
                    .secondary(other, "the other meaning")
                    .help(
                        "an import's name is written before `::` and must mean only that \
                         module here: choose another",
                    ),
                );
                continue;
            }
            alias_at.insert(alias.clone(), *at);
            aliases.insert(alias.clone(), d.on);
            for e in iface.exports() {
                names.insert(
                    qualified_key(alias, &e.name),
                    Visible {
                        sig: Signature {
                            params: e.params.clone(),
                            ret: e.ret,
                            name_span: e.name_span,
                            generics: u32::try_from(e.type_params.len()).unwrap_or(u32::MAX),
                            owner: (!e.type_params.is_empty() || polymorphic(e.effects))
                                .then_some(ParamOwner::Function(e.def)),
                            effect_param: polymorphic(e.effects),
                            preconditions: e.preconditions,
                            pure: e.foreign.is_none()
                                && e.effects == Some(nazm_sema::EffectSet::EMPTY),
                        },
                        callee: Callee::Function(e.def),
                    },
                );
            }
        }
        if !d.plain {
            continue;
        }
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
                refused.push(e.name.clone());
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
                refused.push(e.name.clone());
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
                        generics: u32::try_from(e.type_params.len()).unwrap_or(u32::MAX),
                        owner: (!e.type_params.is_empty() || polymorphic(e.effects))
                            .then_some(ParamOwner::Function(e.def)),
                        effect_param: polymorphic(e.effects),
                        preconditions: e.preconditions,
                        pure: e.foreign.is_none() && e.effects == Some(nazm_sema::EffectSet::EMPTY),
                    },
                    callee: Callee::Function(e.def),
                },
            );
        }
    }

    Env {
        names,
        refused,
        aliases,
    }
}

/// Where a type argument came from, when it was written rather than inferred.
const WRITTEN: usize = usize::MAX;

/// Why an argument's type could not be matched against its declared type.
/// Whether a declared effect set names its function's effect parameter (N51).
fn polymorphic(effects: Option<nazm_sema::EffectSet>) -> bool {
    effects.is_some_and(nazm_sema::EffectSet::has_param)
}

#[derive(Debug, Clone, Copy)]
enum Unify {
    /// The callee's effect parameter already bound to another set (N51).
    EffectConflict {
        earlier: nazm_sema::EffectSet,
        from: usize,
    },
    /// A parameter already settled to another type.
    Conflict {
        param: TypeParam,
        earlier: Type,
        from: usize,
    },
    /// The shapes differ: a `Vec` where the signature wants a record, say.
    Mismatch,
}

/// The arguments settled so far, with each unsettled parameter left as itself — so that
/// substituting them into a declared type says what is known and nothing more.
fn settled_args(solved: &[Option<(Type, usize)>], owner: ParamOwner) -> Vec<Type> {
    solved
        .iter()
        .enumerate()
        .map(|(i, s)| {
            s.map_or(
                Type::Param(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                }),
                |(t, _)| t,
            )
        })
        .collect()
}

/// Mark every parameter of `owner` that `formal` mentions as already accounted for.
fn excuse(types: &nazm_sema::UserTypes, formal: Type, owner: ParamOwner, excused: &mut [bool]) {
    let mut work = vec![formal];
    while let Some(t) = work.pop() {
        if let Type::Param(p) = t {
            if p.owner == owner {
                if let Some(slot) = excused.get_mut(p.index as usize) {
                    *slot = true;
                }
            }
        }
        work.extend(types.type_args(t));
    }
}

/// What `actual` says `param` is, by matching it against `formal` — for the message that
/// reports a conflict, which should name the type the argument implies rather than the
/// argument's whole type.
fn inferred_from(
    types: &nazm_sema::UserTypes,
    formal: Type,
    actual: Type,
    param: TypeParam,
) -> Option<Type> {
    match formal {
        Type::Param(p) if p == param => Some(actual),
        Type::Vec(_) | Type::ChanOf(_) | Type::Record(_) | Type::Enum(_) => types
            .type_args(formal)
            .into_iter()
            .zip(types.type_args(actual))
            .find_map(|(f, a)| inferred_from(types, f, a, param)),
        _ => None,
    }
}

/// `N0390` for a `Chan[elem]` whose element may not cross into a task, or `None` (N53): the
/// rule a `spawn` argument follows, so a channel carries exactly what a task may be given.
fn channel_element(types: &nazm_sema::UserTypes, elem: Type, at: Span) -> Option<Diagnostic> {
    let why = types.task_safety(elem).err()?;
    let shown = types.show(elem);
    let message = if why.path.is_empty() {
        format!("a channel cannot carry `{shown}`: it may not cross into a task")
    } else {
        format!(
            "a channel cannot carry `{shown}`: its `{}` is a `{}`, which may not cross into a task",
            why.path.join("."),
            types.show(why.ty)
        )
    };
    Some(Diagnostic::error(codes::CHANNEL_ELEMENT, at, message).help(
        "a channel's values reach another task, so they follow the rule a `spawn` argument \
         does: `Int`, `Bool`, `Str`, channels, and records and enums of those",
    ))
}

/// A list of names, quoted, for a diagnostic that has to name several.
fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Checker<'_> {
    /// Give a binding its slot in the current function's storage.
    ///
    /// **Parameters first, in order, then body bindings as they are declared, and never
    /// reused** — including for a name that shadows another. That ordering is a contract
    /// the backend relies on to store incoming arguments without a separate notion of what
    /// a parameter is, and it lives here now because this is where bindings are made.
    fn allocate(&mut self, name: &str, span: Span, ty: Type, mutable: bool) -> u32 {
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
    fn open_scope(&mut self, span: Span) {
        self.scopes.push();
        self.res.scopes_mut().open(span, self.current);
    }

    /// Pop the innermost lexical scope, and record that it ended.
    fn close_scope(&mut self) {
        self.scopes.pop();
        self.res.scopes_mut().close();
    }

    /// Bind a name in the innermost scope, visible from byte `visible_from` — and record,
    /// from this checker's own lookup, which binding it hides and whether it collides
    /// with one in the same scope, so that what is visible where can be read back without
    /// comparing a single name.
    fn define(
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
    fn record_var(&mut self, span: Span, slot: u32) {
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
    fn held(&self) -> nazm_sema::CapabilitySet {
        self.scopes.held_from(0)
    }

    /// The binding `name` reaches here, captured into every closure between it and here
    /// (N50): a binding of a frame below a closure's boundary is copied into that closure
    /// when the closure is made, so inside it the name means the closure's copy. A
    /// `let mut` binding or a capability may not be captured (`N0386`).
    fn lookup_var(&mut self, name: &str, at: Span) -> Option<Binding> {
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
        // what it does. A mutable binding still may not be: its copy would stop tracking it.
        if b.mutable {
            self.errors.push(
                Diagnostic::error(
                    codes::CAPTURE_REFUSED,
                    at,
                    format!("a closure cannot capture `{name}`, a `let mut` binding"),
                )
                .help(
                    "a closure copies what it captures, so a mutable binding would silently \
                     stop being tracked; bind a copy with `let`, or share a sequence or a \
                     channel",
                ),
            );
            return Some(Binding {
                mutable: false,
                declared_at: None,
                ..b
            });
        }
        // Gate 1-C1: a closure's environment owns what it captures, so capturing a container
        // that can hold a function value is the one way it could come to own itself
        // (`docs/spec.md`, *Cycles*). Decided by the captured binding's type, once per capture.
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
    fn function_value(&mut self, name: &str, at: Span, expected: Option<Type>) -> Type {
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
        // Its thunk is the using module's; an instance of a generic body is an artefact of its
        // own, which could not reach it (N50).
        if !self.type_scope.is_empty() {
            return refuse(self, "named inside a generic function");
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

    /// A call through a binding of function type (N50).
    /// Whether `name.m()` names a receiver rather than an enum (N78): a value in scope, or —
    /// when no enum of that name is visible — a trait. An enum keeps every meaning it had.
    fn names_a_receiver(&self, qualifier: Option<&nazm_syntax::Qualifier>, name: &str) -> bool {
        let key = written_key(qualifier, name);
        if qualifier.is_none() && self.scopes.lookup(name).is_some() {
            return true;
        }
        let enum_visible = matches!(self.types.names.get(&key), Some(Type::Enum(_)));
        !enum_visible && matches!(self.trait_names.get(&key), Some(Some(_)))
    }

    /// A method call (N78, `architecture.md` §7.79): `recv.m(args)`, or `Trait.m(recv, args)`
    /// when the receiver as written is a trait's name. Resolved here, completely: on a
    /// concrete type to the impl's function, recorded as an ordinary call; on a type
    /// parameter to its bound's method, recorded as a trait call and — for effects, authority
    /// and provenance — as an indirect call of the method's signature.
    #[allow(clippy::too_many_arguments)]
    fn check_method_call(
        &mut self,
        qualifier: Option<&nazm_syntax::Qualifier>,
        receiver: &Expr,
        method: &str,
        at: Span,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        // What an unresolved call continues as: what was wanted, so one mistake is one
        // diagnostic.
        let fallback = expected.unwrap_or(Type::Int);
        // `Show.show(x)`: the trait is written, and the receiver is the first argument.
        let ufcs = match receiver {
            Expr::Var {
                name,
                span: name_at,
            } if qualifier.is_some() || self.scopes.lookup(name).is_none() => {
                let key = written_key(qualifier, name);
                match self.trait_names.get(&key) {
                    Some(Some(t)) => Some((*t, *name_at)),
                    _ if qualifier.is_some() => {
                        self.errors.push(Diagnostic::error(
                            codes::NOT_A_TRAIT,
                            *name_at,
                            format!("`{key}` is not a trait in scope"),
                        ));
                        for a in args {
                            self.check_expr(a, None);
                        }
                        return fallback;
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((t, name_at)) = ufcs {
            self.res.traits_mut().record_mention(name_at, t);
        }
        let (recv, rest): (&Expr, &[Expr]) = match ufcs {
            Some((t, name_at)) => {
                let Some((first, rest)) = args.split_first() else {
                    let tname = self.res.traits().get(t).map_or("?", |d| d.name.as_str());
                    self.errors.push(
                        Diagnostic::error(
                            codes::WRONG_ARGUMENT_COUNT,
                            span,
                            format!("`{tname}.{method}` needs its receiver as its first argument"),
                        )
                        .secondary(name_at, "the trait"),
                    );
                    return fallback;
                };
                (first, rest)
            }
            None => (receiver, args),
        };
        let recv_ty = self.value_type(recv, None);

        // Which trait's method this is.
        let chosen: Result<(TraitId, u32), Diagnostic> = if let Some((t, _)) = ufcs {
            let def = self.res.traits().get(t);
            match def.and_then(|d| d.method(method)) {
                Some(i) => Ok((t, i)),
                None => Err(Diagnostic::error(
                    codes::NO_METHOD,
                    at,
                    format!(
                        "`{}` has no method `{method}`",
                        def.map_or("?", |d| d.name.as_str())
                    ),
                )),
            }
        } else {
            self.lookup_method(recv_ty, method, at)
        };
        let (trait_id, index) = match chosen {
            Ok(c) => c,
            Err(d) => {
                self.errors.push(d);
                for a in rest {
                    self.check_expr(a, None);
                }
                return fallback;
            }
        };
        let Some(want) = self
            .res
            .traits()
            .get(trait_id)
            .and_then(|d| d.methods.get(index as usize))
            .cloned()
        else {
            return fallback;
        };
        let tname = self
            .res
            .traits()
            .get(trait_id)
            .map_or_else(|| "?".to_owned(), |d| d.name.clone());
        let owner = ParamOwner::Trait(trait_id);
        let params: Vec<Type> = want
            .params
            .iter()
            .map(|t| self.res.types_mut().substitute(*t, owner, &[recv_ty]))
            .collect();
        let ret = self.res.types_mut().substitute(want.ret, owner, &[recv_ty]);

        let through_bound = matches!(recv_ty, Type::Param(_));
        if through_bound {
            let fty = self.res.types_mut().fn_of(nazm_sema::FnSignature {
                params: std::iter::once(recv_ty)
                    .chain(params.iter().copied())
                    .collect(),
                ret,
                effects: want.effects,
                row: None,
            });
            self.res.traits_mut().record_call(
                at,
                TraitCall {
                    trait_id,
                    method: index,
                    self_ty: recv_ty,
                },
            );
            self.res.record_call(at, Callee::Indirect(fty));
        } else {
            let imp = self
                .res
                .traits()
                .method_fn(trait_id, recv_ty, index)
                .filter(|_| {
                    self.res
                        .traits()
                        .find(trait_id, recv_ty)
                        .and_then(|i| self.res.traits().impl_def(i))
                        .is_some_and(|d| self.impl_modules.contains(&d.module))
                });
            let Some(f) = imp else {
                self.errors.push(
                    Diagnostic::error(
                        codes::NO_METHOD,
                        at,
                        format!(
                            "`{}` does not implement `{tname}` in this module's scope",
                            self.show(recv_ty)
                        ),
                    )
                    .help(
                        "an impl is used where its module is this one or one this module imports",
                    ),
                );
                for a in rest {
                    self.check_expr(a, None);
                }
                return ret;
            };
            self.res.record_call(at, Callee::Function(f));
        }
        self.record_site(at, Vec::new(), false);

        if rest.len() != params.len() {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{tname}.{method}` takes {} argument{} after its receiver, but {} {} given",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    rest.len(),
                    if rest.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        for (i, a) in rest.iter().enumerate() {
            let want = params.get(i).copied();
            let got = self.value_type(a, want);
            if let Some(want) = want {
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{tname}.{method}` is `{}`, but this is `{}`",
                            i + 1,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
            }
        }
        if let Some(callee) = self.res.callee(at) {
            self.res.record_checked_call(
                at,
                CheckedCall {
                    callee,
                    span,
                    args: std::iter::once(recv.span())
                        .chain(rest.iter().map(Expr::span))
                        .collect(),
                    params: std::iter::once(recv_ty).chain(params).collect(),
                    ret,
                    instance: None,
                    spawned: false,
                    preconditions: self
                        .res
                        .function(callee_fn(callee))
                        .map_or(0, |d| d.preconditions),
                },
            );
        }
        ret
    }

    /// The one trait method `method` on a value of `ty` (N78): through a type parameter's
    /// bound, or among the traits in scope with an impl for `ty` this module may use.
    // One diagnostic, built only when the lookup fails: not worth a box.
    #[allow(clippy::result_large_err)]
    fn lookup_method(
        &self,
        ty: Type,
        method: &str,
        at: Span,
    ) -> Result<(TraitId, u32), Diagnostic> {
        let shown = self.show(ty);
        if let Type::Param(p) = ty {
            let bound = self.res.traits().bound_of(p);
            return bound
                .and_then(|t| {
                    self.res
                        .traits()
                        .get(t)
                        .and_then(|d| d.method(method))
                        .map(|i| (t, i))
                })
                .ok_or_else(|| {
                    Diagnostic::error(
                        codes::NO_METHOD,
                        at,
                        format!("`{shown}` has no method `{method}`"),
                    )
                    .help(
                        "a type parameter has the methods of the trait it requires: \
                         `fn f[T: Show](x: T) -> Str { x.show() }`",
                    )
                });
        }
        let mut found: Vec<(TraitId, u32)> = Vec::new();
        let mut elsewhere: Vec<String> = Vec::new();
        for (t, def) in self.res.traits().iter() {
            let Some(i) = def.method(method) else {
                continue;
            };
            let Some(imp) = self.res.traits().find(t, ty) else {
                continue;
            };
            let in_scope = self.trait_names.values().any(|v| *v == Some(t));
            let usable = self
                .res
                .traits()
                .impl_def(imp)
                .is_some_and(|d| self.impl_modules.contains(&d.module));
            if in_scope && usable {
                found.push((t, i));
            } else {
                elsewhere.push(format!("`{}`", def.name));
            }
        }
        match found.as_slice() {
            [one] => Ok(*one),
            [] => {
                let d = Diagnostic::error(
                    codes::NO_METHOD,
                    at,
                    format!("no trait in scope gives `{shown}` a method `{method}`"),
                );
                Err(if elsewhere.is_empty() {
                    d
                } else {
                    d.help(format!(
                        "{} would: import the module that declares it",
                        elsewhere.join(", ")
                    ))
                })
            }
            many => {
                let names: Vec<String> = many
                    .iter()
                    .filter_map(|(t, _)| self.res.traits().get(*t).map(|d| format!("`{}`", d.name)))
                    .collect();
                Err(Diagnostic::error(
                    codes::AMBIGUOUS_METHOD,
                    at,
                    format!(
                        "`{method}` on `{shown}` is a method of {}",
                        names.join(" and ")
                    ),
                )
                .help("name the trait: `Show.show(x)`"))
            }
        }
    }

    /// Whether a value of `got` may be passed or returned where `want` is expected (N79): the
    /// same type; an `IoCap` where an `OutCap` is, which hands on less authority than it
    /// holds; or a function type with the same parameters and result whose effects are within
    /// the expected one's. An effect parameter is matched by binding, never by inclusion.
    fn assignable(&self, got: Type, want: Type) -> bool {
        if got == want {
            return true;
        }
        match (got, want) {
            (Type::Cap(g), Type::Cap(w)) => nazm_sema::CapabilitySet::of(g).implied().contains(w),
            (Type::Fn(g), Type::Fn(w)) => {
                let types = self.res.types();
                let (Some(gs), Some(ws)) = (types.fn_signature(g), types.fn_signature(w)) else {
                    return false;
                };
                gs.params == ws.params
                    && gs.ret == ws.ret
                    && gs.row.is_none()
                    && ws.row.is_none()
                    && !gs.effects.has_param()
                    && !ws.effects.has_param()
                    && gs.effects.without(ws.effects).is_empty()
            }
            _ => false,
        }
    }

    fn check_indirect_call(
        &mut self,
        name: &str,
        at: Span,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let Some(b) = self.lookup_var(name, at) else {
            return expected.unwrap_or(Type::Int);
        };
        self.record_var(at, b.slot);
        let Type::Fn(f) = b.ty else {
            return expected.unwrap_or(Type::Int);
        };
        let Some(fsig) = self.res.types().fn_signature(f).cloned() else {
            return expected.unwrap_or(Type::Int);
        };
        if args.len() != fsig.params.len() {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{name}` takes {} argument{}, but {} {} given",
                    fsig.params.len(),
                    if fsig.params.len() == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        self.res.record_call(at, Callee::Indirect(b.ty));
        self.record_site(at, Vec::new(), false);
        for (i, a) in args.iter().enumerate() {
            let want = fsig.params.get(i).copied();
            let got = self.value_type(a, want);
            if let Some(want) = want {
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{name}` is `{}`, but this is `{}`",
                            i + 1,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
            }
        }
        let sig = Signature {
            params: fsig.params.clone(),
            ret: fsig.ret,
            name_span: at,
            generics: 0,
            owner: None,
            effect_param: false,
            preconditions: 0,
            pure: false,
        };
        self.record_checked_call(at, span, args, &sig, None, false);
        fsig.ret
    }

    /// A closure expression (N50): a definition of its own, checked as a function is, with
    /// what its body names from around it captured.
    #[allow(clippy::too_many_arguments)]
    fn check_closure(
        &mut self,
        params: &[nazm_syntax::Param],
        return_type: &TypeName,
        effects: Option<&nazm_syntax::EffectDecl>,
        body: &Block,
        fn_span: Span,
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let Some(enclosing) = self.current else {
            return expected.unwrap_or(Type::Int);
        };
        if !self.type_scope.is_empty() {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_A_FUNCTION_VALUE,
                    fn_span,
                    "a closure inside a generic function is not supported yet",
                )
                .help(
                    "a closure there would need an instance per type argument; write it in a \
                     function that is not generic, or pass a function value in",
                ),
            );
            return expected.unwrap_or(Type::Int);
        }
        let param_types: Vec<Type> = params
            .iter()
            .map(|p| self.resolve_written_type(&p.ty))
            .collect();
        let ret = self.resolve_written_type(return_type);
        let effect = self.effect_param.clone();
        let declared = match effects {
            Some(d) => {
                crate::effects::declared(d, effect.as_ref().map(|e| e.0.as_str()), &mut self.errors)
            }
            None => Some(nazm_sema::EffectSet::EMPTY),
        }
        .unwrap_or(nazm_sema::EffectSet::EMPTY);
        let (enclosing_name, decl) = self
            .res
            .function(enclosing)
            .map(|d| (d.name.clone(), d.decl))
            .unwrap_or_default();
        let base = enclosing_name.split('#').next().unwrap_or("").to_owned();
        let name = format!("{base}#{}", self.closure_count);
        self.closure_count += 1;
        let ty = self.res.types_mut().fn_of(nazm_sema::FnSignature {
            params: param_types.clone(),
            ret,
            effects: declared,
            row: effect.map(|e| e.1),
        });
        let id = self.res.declare_closure(FnDef {
            name,
            module: self.module,
            visibility: Visibility::Private,
            params: param_types.clone(),
            ret,
            name_span: fn_span,
            decl,
            locals: Vec::new(),
            type_params: Vec::new(),
            call_sites: Vec::new(),
            declared_effects: Some(declared),
            inferred_effects: None,
            effect_reasons: Vec::new(),
            provenance: None,
            mentions: Vec::new(),
            foreign: None,
            export: None,
            preconditions: 0,
            closure: Some(nazm_sema::ClosureInfo {
                enclosing,
                at: span,
                written_effects: effects.map(|d| d.span),
                ty,
                captures: Vec::new(),
            }),
        });

        // The closure's body is a function of its own: its own slots, returns, loops and
        // scopes, and none of the enclosing body's.
        let saved_current = self.current.replace(id);
        let saved_return = self.return_type.replace(ret);
        let saved_span = self.return_span.replace(return_type.span);
        let saved_scope_depth = std::mem::replace(&mut self.scope_depth, 0);
        let saved_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        self.closures.push(ClosureFrame {
            id,
            boundary: self.scopes.depth(),
            captured: HashMap::new(),
        });
        self.open_scope(span);
        for (p, ty) in params.iter().zip(&param_types) {
            if self.scopes.defined_here(&p.name) {
                self.errors.push(Diagnostic::error(
                    codes::ALREADY_DEFINED_IN_SCOPE,
                    p.span,
                    format!("`{}` is listed twice in this parameter list", p.name),
                ));
            }
            let slot = self.allocate(&p.name, p.name_span, *ty, false);
            self.define(&p.name, *ty, false, None, slot, p.name_span.end);
        }
        let completion = self.check_block(body, Some(ret));
        match completion {
            Completion::Diverges => {}
            Completion::Value(actual) if actual == ret => {}
            Completion::Value(actual) => {
                let (actual, declared) = (self.show(actual), self.show(ret));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.tail.as_ref().map_or(body.span, |t| t.span()),
                        format!("this closure returns `{actual}`, but its type says `{declared}`"),
                    )
                    .secondary(return_type.span, format!("declared `{declared}` here")),
                );
            }
            Completion::Unit => {
                let declared = self.show(ret);
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.span,
                        format!("this closure must produce a `{declared}`, but its body does not"),
                    )
                    .secondary(return_type.span, format!("declared `{declared}` here")),
                );
            }
        }
        self.close_scope();
        self.closures.pop();
        self.current = saved_current;
        self.return_type = saved_return;
        self.return_span = saved_span;
        self.scope_depth = saved_scope_depth;
        self.loop_depth = saved_loop_depth;

        self.res.record_mention(enclosing, ty);
        ty
    }

    /// The aliases this module imports modules under (N49), for a diagnostic.
    fn alias_names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.env.aliases.keys().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    fn resolve_callee(&mut self, name: &str, at: Span) -> Option<Signature> {
        let visible = self.env.get(name)?.clone();
        self.res.record_call(at, visible.callee);
        Some(visible.sig)
    }

    /// The record a completion probe's hole call names, recorded as a construction's head
    /// would be (N26).
    fn called_record(&mut self, name: &str, at: Span, written: Option<&[TypeName]>) {
        if self.refused(name) {
            return;
        }
        let Some(def) = self.resolve_record(name, at) else {
            return;
        };
        if let Some(Type::Record(id)) = self.applied(Type::Record(def), written, at) {
            self.res.record_construction(at, id);
        }
    }

    /// Whether a completion probe must not anchor through the type name `name` (N26): the
    /// module's types refused it as an ambiguity or a collision, so it means nothing
    /// unambiguously here, whatever the table goes on to answer. Always `false` outside a
    /// probe, so nothing an ordinary compilation records depends on it.
    fn refused(&self, name: &str) -> bool {
        self.res.probing() && self.types.refused.iter().any(|n| n == name)
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
        self.res.types().show(t)
    }

    /// An effect set as source writes it, the effect parameter by its name (N51).
    fn show_effects(&self, set: nazm_sema::EffectSet) -> String {
        let mut names: Vec<String> = set.named().names().into_iter().map(str::to_owned).collect();
        if set.has_param() {
            names.push(
                self.res
                    .types()
                    .effect_param_name(self.effect_param.as_ref().map(|e| e.1)),
            );
        }
        format!("{{ {} }}", names.join(", "))
    }

    /// The definition a name written as a construction's head or a pattern's enum refers
    /// to — the **definition**, not a type: `Maybe` in `Maybe.Some(value: x)` names the
    /// generic enum whose instance the scrutinee already is, and `Pair` in
    /// `Pair[Int, Str](…)` is applied by its arguments afterwards.
    fn resolve_type(&mut self, name: &str, span: Span) -> Type {
        // Split borrow: the environment and the error list are both behind `self`.
        let mut errors = std::mem::take(&mut self.errors);
        let t = if let Some((_, p)) = self.type_scope.iter().find(|(n, _)| n == name) {
            *p
        } else {
            let t = resolve_name(&self.types, self.res, name, span, &mut errors);
            // The head of a construction and the enum before a variant's `.`: which
            // definition the name is, recorded where it was decided (N18).
            if let Some(node) = TypeNode::of(t) {
                self.res.record_type_name(span, node);
            }
            t
        };
        self.errors = errors;
        t
    }

    /// A type in the current function's signature, resolved again to the same answer
    /// stage 1 reached — **silently**, because stage 1 already reported whatever was wrong
    /// with it, and one mistake is one error. Before N11 an unknown parameter type was
    /// reported twice, once here and once there.
    fn signature_type(&mut self, written: &TypeName) -> Type {
        let mut discarded = Vec::new();
        let scope = std::mem::take(&mut self.type_scope);
        let effect = self.effect_param.clone();
        let t = resolve_written(
            &self.types,
            self.res,
            written,
            &scope,
            effect.as_ref().map(|(n, o)| (n.as_str(), *o)),
            &mut discarded,
        );
        self.type_scope = scope;
        t
    }

    /// A type written in full inside a body — a type argument — in the current function's
    /// parameter scope.
    fn resolve_written_type(&mut self, written: &TypeName) -> Type {
        let mut errors = std::mem::take(&mut self.errors);
        let scope = std::mem::take(&mut self.type_scope);
        let effect = self.effect_param.clone();
        let t = resolve_written(
            &self.types,
            self.res,
            written,
            &scope,
            effect.as_ref().map(|(n, o)| (n.as_str(), *o)),
            &mut errors,
        );
        self.type_scope = scope;
        self.errors = errors;
        t
    }

    /// Resolve explicit type arguments written at a use of a generic definition, and apply
    /// the definition to them — `Pair[Int, Str]` from `Pair` and `[Int, Str]`.
    ///
    /// The rules every type position has: a generic definition is written with its
    /// arguments (`N0352`), a non-generic one without (`N0351`), and exactly as many as it
    /// declares (`N0353`). `at` is the name the arguments follow.
    ///
    /// `None` where the arguments were missing, so the caller checks what it can without a
    /// type to check it against rather than against one invented here.
    fn applied(&mut self, def: Type, written: Option<&[TypeName]>, at: Span) -> Option<Type> {
        let args: Option<Vec<Type>> =
            written.map(|w| w.iter().map(|t| self.resolve_written_type(t)).collect());
        let Some(node) = TypeNode::of(def) else {
            return Some(def);
        };
        let arity = self.res.types().declared_params(node);
        match (args, arity) {
            (None, 0) => Some(def),
            (Some(_), 0) => {
                let shown = self.show(def);
                self.errors.push(not_generic(&shown, "not generic", at));
                Some(def)
            }
            (None, n) => {
                let shown = self.show(def);
                self.errors.push(
                    Diagnostic::error(
                        codes::MISSING_TYPE_ARGUMENTS,
                        at,
                        format!(
                            "`{shown}` is generic, so it is built with its {n} type argument{} \
                             written",
                            if n == 1 { "" } else { "s" }
                        ),
                    )
                    .help(format!(
                        "write them after the name: `{shown}[…]`. A construction infers \
                         nothing"
                    )),
                );
                None
            }
            (Some(mut args), n) => {
                if args.len() != n {
                    let shown = self.show(def);
                    self.errors.push(wrong_arity(&shown, n, args.len(), at));
                    args.resize(n, Type::Int);
                }
                let t = apply(self.res, node, args);
                self.mention(t);
                Some(t)
            }
        }
    }

    /// Note that the body being checked mentions a type, for the native instance planner.
    /// Only a generic body records anything: a monomorphic one needs no substitution.
    fn mention(&mut self, t: Type) {
        if let Some(id) = self.current {
            self.res.record_mention(id, t);
        }
    }

    /// Which record a type name written in an expression refers to, if it is one.
    ///
    /// Reports through [`Self::resolve_type`], so an unknown name is one diagnostic
    /// wherever it was written.
    fn resolve_record(&mut self, name: &str, span: Span) -> Option<RecordId> {
        match self.resolve_type(name, span) {
            Type::Record(id) => Some(id),
            Type::Enum(id) => {
                // `E(x: 1)` where `E` is an enum. A real mistake with an exact fix, so it
                // says the fix rather than that `E` is not a record.
                let name = self.res.types().show(Type::Enum(id)).to_owned();
                let first = self
                    .res
                    .enums()
                    .get(id)
                    .and_then(|d| d.variants.first().map(|v| v.name.clone()));
                let mut d = Diagnostic::error(
                    codes::UNKNOWN_VARIANT,
                    span,
                    format!("`{name}` is an enum, so a value of it names one of its variants"),
                );
                if let Some(v) = first {
                    d = d.help(format!("write `{name}.{v}(…)`"));
                }
                self.errors.push(d);
                None
            }
            Type::Param(_) => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        span,
                        format!("`{name}` is a type parameter, so it has no fields to name"),
                    )
                    .help(
                        "nothing is known about a type parameter inside its definition, so \
                         there is no record to build",
                    ),
                );
                None
            }
            Type::Cap(k) => {
                // Authority is not constructible (N37): the only capabilities are the ones
                // the runtime hands `main`. Its own code, not "not a record", because the
                // question a reader has is where one comes from, not what fields it lacks.
                self.errors.push(forged(k, span));
                None
            }
            other => {
                // A known type that is not a user-defined one: `Int(x: 1)`. Distinguished
                // from an unknown name, which `resolve_type` has already reported.
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

    /// Which enum a type name written in an expression or a pattern refers to.
    fn resolve_enum(&mut self, name: &str, span: Span) -> Option<EnumId> {
        match self.resolve_type(name, span) {
            Type::Enum(id) => Some(id),
            Type::Record(id) => {
                let shown = self.res.types().show(Type::Record(id)).to_owned();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_VARIANT,
                        span,
                        format!("`{shown}` is a record, so it has no variants"),
                    )
                    .help("a record is built by naming its fields: `Point(x: 1, y: 2)`"),
                );
                None
            }
            Type::Param(_) => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_VARIANT,
                        span,
                        format!("`{name}` is a type parameter, so it has no variants"),
                    )
                    .help("only an `enum` has variants, and nothing is known about a parameter"),
                );
                None
            }
            other => {
                if Type::from_name(name).is_some() {
                    let shown = self.show(other);
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNKNOWN_VARIANT,
                            span,
                            format!("`{shown}` is a built-in type, so it has no variants"),
                        )
                        .help("only an `enum` has variants"),
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
        type_args: Option<&[TypeName]>,
        fields: &[FieldInit],
        span: Span,
    ) -> Option<Type> {
        let applied = self
            .resolve_record(ty, ty_span)
            .and_then(|def| self.applied(Type::Record(def), type_args, ty_span));
        let Some(Type::Record(id)) = applied else {
            // Still check the initialisers: one unknown type should not silence every
            // error inside it.
            for f in fields {
                self.check_expr(&f.value, None);
            }
            return None;
        };
        self.res.record_construction(ty_span, id);
        // Only C makes a handle (N85): it has no field a program could give a value.
        if self
            .res
            .records()
            .get(id)
            .is_some_and(|d| d.repr == CRepr::Opaque)
        {
            for f in fields {
                self.check_expr(&f.value, None);
            }
            self.errors.push(opaque_refused(ty_span, ty, "built"));
            return Some(Type::Record(id));
        }

        let mut given: Vec<(String, Span)> = Vec::new();
        for f in fields {
            let declared = self
                .res
                .records()
                .get(id)
                .and_then(|d| d.field(&f.name).map(|(i, fd)| (i, fd.ty, fd.name_span)));
            let Some((index, want, declared_at)) = declared else {
                let name = self.res.types().show(Type::Record(id)).to_owned();
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
            // Which field this initialiser gives a value to — decided just above, and
            // recorded (N17) so that a field's uses include the constructions that set it,
            // not only the projections that read it.
            self.res.record_field(
                f.name_span,
                FieldRef {
                    record: id,
                    field: index,
                },
            );
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
            let name = self.res.types().show(Type::Record(id)).to_owned();
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

    /// Check `Enum.Variant(field: value, …)` and return the enum it builds.
    ///
    /// The same law as a record construction, applied to the fields of one variant: every
    /// payload field exactly once, by name, with no unknown field and no duplicate, and
    /// initialisers checked **in the order written** because that is the order they run in.
    #[allow(clippy::too_many_arguments)]
    fn check_variant(
        &mut self,
        ty: &str,
        ty_span: Span,
        type_args: Option<&[TypeName]>,
        variant: &str,
        variant_span: Span,
        fields: &[FieldInit],
        span: Span,
    ) -> Option<Type> {
        let applied = if self.refused(ty) {
            None
        } else {
            self.resolve_enum(ty, ty_span)
                .and_then(|def| self.applied(Type::Enum(def), type_args, ty_span))
        };
        let Some(Type::Enum(id)) = applied else {
            // Still check the initialisers: one unknown type should not silence every
            // error inside it.
            for f in fields {
                self.check_expr(&f.value, None);
            }
            return None;
        };
        let Some(at) = self.find_variant(id, variant, variant_span) else {
            for f in fields {
                self.check_expr(&f.value, None);
            }
            return Some(Type::Enum(id));
        };
        self.res.record_variant(
            variant_span,
            VariantRef {
                enumeration: id,
                variant: at,
            },
        );

        let mut given: Vec<(String, Span)> = Vec::new();
        for f in fields {
            let declared = self
                .variant_def(id, at)
                .and_then(|v| v.field(&f.name).map(|(i, fd)| (i, fd.ty, fd.name_span)));
            let Some((index, want, declared_at)) = declared else {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{owner}` has no payload field `{}`", f.name),
                    )
                    .secondary(
                        self.variant_span(id, at).unwrap_or(ty_span),
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
                    .help("every payload field is given exactly one value"),
                );
                self.check_expr(&f.value, None);
                continue;
            }
            given.push((f.name.clone(), f.name_span));
            self.res.record_payload(
                f.name_span,
                PayloadRef {
                    enumeration: id,
                    variant: at,
                    field: index,
                },
            );
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

        let missing = self.missing_payload_fields(id, at, &given);
        if !missing.is_empty() {
            let owner = self.variant_path(id, at);
            let list = quoted(&missing);
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    span,
                    format!("this `{owner}` does not give a value for {list}"),
                )
                .secondary(
                    self.variant_span(id, at).unwrap_or(ty_span),
                    "declared here",
                )
                .help(
                    "every payload field is given a value when a variant is built. There \
                     are no default values",
                ),
            );
        }
        Some(Type::Enum(id))
    }

    /// Check `match scrutinee { … }`.
    ///
    /// The scrutinee is checked once, the arms are checked against the enum it named, and
    /// exhaustiveness is decided here rather than deferred: `docs/spec.md` gives the
    /// checker the language's meaning, and a `match` whose coverage were settled at
    /// lowering would have two answers to one question.
    fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
        expected: Option<Type>,
    ) -> Completion {
        let sty = self.value_type(scrutinee, None);
        let Type::Enum(id) = sty else {
            let shown = self.show(sty);
            self.errors.push(
                Diagnostic::error(
                    codes::MATCH_ON_NON_ENUM,
                    scrutinee.span(),
                    format!("`match` needs an enum, but this is `{shown}`"),
                )
                .help(
                    "`match` selects one variant of an enum. There is no literal, record \
                     or range pattern",
                ),
            );
            return Completion::Value(expected.unwrap_or(Type::Int));
        };

        // One slot per variant, in declaration order: where the arm that covers it was
        // written. That is both the duplicate check and the exhaustiveness check, and
        // keeping them as one structure is what stops the two disagreeing.
        let count = self.res.enums().get(id).map_or(0, |d| d.variants.len());
        let mut covered: Vec<Option<Span>> = vec![None; count];
        // Whether some arm named a variant this enum does not have, or another enum's.
        // Coverage is not reported when one did: the checker does not know which variant
        // the author meant, so "does not cover `B`" would be a second diagnostic about the
        // one mistake already reported. `docs/spec.md`, *One mistake, one error*.
        let mut unresolved = false;
        let mut result: Option<Completion> = None;
        let mut first_value: Option<(Type, Span)> = None;

        for arm in arms {
            let Some(named) =
                self.resolve_enum(&written_key(arm.qualifier.as_ref(), &arm.ty), arm.ty_span)
            else {
                unresolved = true;
                continue;
            };
            // The enum a pattern names is a **definition**: `Maybe.Some(…)` against a
            // `Maybe[Int]` names the enum it is an instance of, and the scrutinee supplies
            // the arguments. A different definition is still another enum's variant.
            if self.res.types().definition_of(sty) != Some(TypeNode::Enum(named)) {
                let (want, got) = (self.show(sty), self.show(Type::Enum(named)));
                self.errors.push(
                    Diagnostic::error(
                        codes::WRONG_ENUM_IN_PATTERN,
                        arm.ty_span,
                        format!("this pattern is a `{got}`, but the value is a `{want}`"),
                    )
                    .secondary(scrutinee.span(), format!("this is a `{want}`"))
                    .help(
                        "a variant belongs to its enum, so a pattern names the scrutinee's \
                         own enum. There is no structural matching between two enums that \
                         happen to have a variant of the same name",
                    ),
                );
                unresolved = true;
                continue;
            }
            let Some(at) = self.find_variant(id, &arm.variant, arm.variant_span) else {
                unresolved = true;
                continue;
            };
            if let Some(Some(first)) = covered.get(at as usize).copied() {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_MATCH_ARM,
                        arm.variant_span,
                        format!("`{owner}` already has an arm"),
                    )
                    .secondary(first, "first covered here")
                    .help(
                        "the discriminant selects exactly one arm, so a second one for the \
                         same variant can never run",
                    ),
                );
                continue;
            }
            if let Some(slot) = covered.get_mut(at as usize) {
                *slot = Some(arm.variant_span);
            }
            self.res.record_variant(
                arm.variant_span,
                VariantRef {
                    enumeration: id,
                    variant: at,
                },
            );

            let completion = self.check_arm(id, at, arm, expected.or(first_value.map(|(t, _)| t)));
            if let Completion::Value(t) = completion {
                match first_value {
                    None => first_value = Some((t, arm.body.span())),
                    Some((want, at_span)) if want != t => {
                        let (swant, sgot) = (self.show(want), self.show(t));
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                arm.body.span(),
                                format!("this arm is `{sgot}`, but an earlier one is `{swant}`"),
                            )
                            .secondary(at_span, format!("this one is `{swant}`"))
                            .help("every arm of a `match` must have the same type"),
                        );
                    }
                    Some(_) => {}
                }
            }
            result = Some(match (result, completion) {
                (None, c) => c,
                // One arm leaving means the others decide, exactly as one branch of an
                // `if` does.
                (Some(Completion::Diverges), c) | (Some(c), Completion::Diverges) => c,
                (Some(Completion::Unit), _) | (Some(_), Completion::Unit) => Completion::Unit,
                (Some(a), _) => a,
            });
        }

        let missing: Vec<String> = covered
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_none())
            .filter_map(|(i, _)| {
                self.res
                    .enums()
                    .get(id)
                    .and_then(|d| d.variants.get(i))
                    .map(|v| v.name.clone())
            })
            .collect();
        if !missing.is_empty() && !unresolved {
            let name = self.res.types().show(sty).to_owned();
            let list = quoted(&missing);
            let declared = self.res.enums().get(id).map(|d| d.name_span);
            let mut d = Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                span,
                format!("this `match` does not cover {list}"),
            )
            .help(format!(
                "an enum is closed, so every variant of `{name}` gets an arm. There is no \
                 `_` arm: adding a variant is meant to break the matches that believed \
                 they had handled all of them"
            ));
            if let Some(at) = declared {
                d = d.secondary(at, "declared here");
            }
            self.errors.push(d);
        }

        result.unwrap_or(Completion::Value(expected.unwrap_or(Type::Int)))
    }

    /// Whether `t` is an instance of the core prelude's `Result`, and if so its success
    /// and error types.
    ///
    /// By **definition identity**: the application's definition must be the prelude's
    /// `Result`. Nothing here reads a name or looks at variants, so a project enum called
    /// `Result`, or one shaped `Ok`/`Err`, is simply not one.
    fn result_parts(&self, t: Type) -> Option<(EnumId, Type, Type)> {
        let Type::Enum(id) = t else { return None };
        let def = self.result?;
        if self.res.types().definition_of(t) != Some(TypeNode::Enum(def)) {
            return None;
        }
        match self.res.types().type_args(t).as_slice() {
            [ok, err] => Some((id, *ok, *err)),
            _ => None,
        }
    }

    /// `operand?`.
    ///
    /// # The rule
    ///
    /// The operand is a `Result[T, E]` and the enclosing function returns `Result[U, E]` —
    /// the **same** `E`, compared as types, so two error types with identical fields are
    /// still two error types. The expression is a `T`. `U` is free: `?` yields the
    /// operand's success value where it is written, and what leaves is only the error.
    ///
    /// # Completion
    ///
    /// Exactly the operand's. The `Err` path leaves the function, which is a `return`, and
    /// a `return` inside an expression is already how a value position can transfer; the
    /// path that stays is `Value(T)`. So `?` needs no completion of its own, and
    /// `docs/spec.md` says why that is the point.
    fn check_propagate(&mut self, operand: &Expr, at: Span, expected: Option<Type>) -> Completion {
        let completion = self.check_expr(operand, None);
        let ot = match completion {
            Completion::Value(t) => t,
            // An operand that never produces a value: nothing is taken apart, and the
            // expression transfers with it.
            Completion::Diverges => return Completion::Diverges,
            Completion::Unit => {
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        operand.span(),
                        "this produces no value",
                    )
                    .help("`?` takes apart a `Result`, and this is not a value at all"),
                );
                return Completion::Value(expected.unwrap_or(Type::Int));
            }
        };

        let Some((operand_id, ok, err)) = self.result_parts(ot) else {
            let shown = self.show(ot);
            self.errors.push(
                Diagnostic::error(
                    codes::PROPAGATE_NON_RESULT,
                    at,
                    format!("`?` needs a `Result`, but this is `{shown}`"),
                )
                .secondary(operand.span(), format!("this is `{shown}`"))
                .help(
                    "`?` propagates the core prelude's `Result` and nothing else — not an \
                     `Option`, and not an enum with the same variants. Take the value \
                     apart with `match` instead",
                ),
            );
            return Completion::Value(expected.unwrap_or(Type::Int));
        };

        let declared = self.return_type;
        let Some((into, _, into_err)) = declared.and_then(|t| self.result_parts(t)) else {
            let shown = declared.map_or_else(|| "nothing".to_owned(), |t| self.show(t));
            let mut d = Diagnostic::error(
                codes::PROPAGATE_OUTSIDE_RESULT,
                at,
                format!("`?` can only be used in a function that returns a `Result`, and this one returns `{shown}`"),
            )
            .help(
                "`?` leaves the function with the error, so the function has to return a \
                 `Result` with the same error type. Here, `match` on the value and decide \
                 what the error means instead",
            );
            if let Some(span) = self.return_span {
                d = d.secondary(span, format!("declared `{shown}` here"));
            }
            self.errors.push(d);
            return Completion::Value(ok);
        };

        if err != into_err {
            let (have, want) = (self.show(err), self.show(into_err));
            let mut d = Diagnostic::error(
                codes::PROPAGATE_ERROR_MISMATCH,
                at,
                format!(
                    "this `?` propagates a `{have}`, but the function's error type is `{want}`"
                ),
            )
            .secondary(operand.span(), format!("its error type is `{have}`"))
            .help(
                "there is no conversion between error types: `?` returns the error value \
                 as it is. Match on the value and build the error this function returns",
            );
            if let Some(span) = self.return_span {
                d = d.secondary(span, format!("the function returns `{want}` errors"));
            }
            self.errors.push(d);
            return Completion::Value(ok);
        }

        let variant = |c: &Self, id: EnumId, name: &str| {
            c.res
                .enums()
                .get(id)
                .and_then(|d| d.variant(name))
                .map(|(i, _)| i)
        };
        if let (Some(ok_at), Some(err_at), Some(into_at)) = (
            variant(self, operand_id, "Ok"),
            variant(self, operand_id, "Err"),
            variant(self, into, "Err"),
        ) {
            self.res.record_propagation(
                at,
                Propagation {
                    operand: operand_id,
                    ok: ok_at,
                    err: err_at,
                    into,
                    into_err: into_at,
                },
            );
        }
        Completion::Value(ok)
    }

    /// One arm: its payload bindings, then its body, in a scope of their own.
    fn check_arm(
        &mut self,
        id: EnumId,
        at: u32,
        arm: &MatchArm,
        expected: Option<Type>,
    ) -> Completion {
        self.open_scope(arm.span);
        let mut named: Vec<(String, Span)> = Vec::new();
        for f in &arm.fields {
            let declared = self
                .variant_def(id, at)
                .and_then(|v| v.field(&f.name).map(|(i, fd)| (i, fd.ty)));
            let Some((index, ty)) = declared else {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{owner}` has no payload field `{}`", f.name),
                    )
                    .secondary(
                        self.variant_span(id, at).unwrap_or(arm.variant_span),
                        "declared here",
                    ),
                );
                continue;
            };
            if let Some((_, first)) = named.iter().find(|(g, _)| *g == f.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("this pattern names `{}` twice", f.name),
                    )
                    .secondary(*first, "first named here"),
                );
                continue;
            }
            named.push((f.name.clone(), f.name_span));
            self.res.record_payload(
                f.name_span,
                PayloadRef {
                    enumeration: id,
                    variant: at,
                    field: index,
                },
            );
            // `_` names nothing. The field is still resolved above, because it still
            // exists and is still released with the value — `docs/spec.md` is explicit
            // that a discard declines to name a payload rather than forgetting it.
            let Some(binding) = &f.binding else { continue };
            if self.scopes.defined_here(binding) {
                self.errors.push(
                    Diagnostic::error(
                        codes::ALREADY_DEFINED_IN_SCOPE,
                        f.binding_span,
                        format!("`{binding}` is bound twice by this pattern"),
                    )
                    .help("each payload field binds a name of its own, or is discarded with `_`"),
                );
                continue;
            }
            // Not mutable: a pattern binding is a borrowed, immutable view of the payload
            // the match's scrutinee holds. That is a statement about the *name* — a
            // binding that names a sequence still reaches shared mutable storage, which
            // `docs/spec.md` says in as many words.
            let slot = self.allocate(binding, f.binding_span, ty, false);
            self.record_var(f.binding_span, slot);
            self.define(
                binding,
                ty,
                false,
                Some(f.binding_span),
                slot,
                f.binding_span.end,
            );
        }

        let missing = self.missing_payload_fields(id, at, &named);
        if !missing.is_empty() {
            let owner = self.variant_path(id, at);
            let list = quoted(&missing);
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    arm.variant_span,
                    format!("this pattern does not name {list} of `{owner}`"),
                )
                .help(
                    "a pattern names every payload field, binding it or discarding it with \
                     `_`. There is no rest pattern, so adding a payload field breaks the \
                     patterns that did not know about it rather than ignoring it",
                ),
            );
        }

        let completion = self.check_expr(&arm.body, expected);
        self.close_scope();
        completion
    }

    /// The variant of this name, reported if the enum does not have one.
    fn find_variant(&mut self, id: EnumId, name: &str, at: Span) -> Option<u32> {
        if let Some((i, _)) = self.res.enums().get(id).and_then(|d| d.variant(name)) {
            return Some(i);
        }
        // Which enum the name was looked for in, for a tool completing it (N21).
        self.res.record_unresolved_member(at, Type::Enum(id));
        let shown = self.res.types().show(Type::Enum(id)).to_owned();
        let known: Vec<String> = self
            .res
            .enums()
            .get(id)
            .map(|d| d.variants.iter().map(|v| v.name.clone()).collect())
            .unwrap_or_default();
        let declared = self.res.enums().get(id).map(|d| d.name_span);
        let mut d = Diagnostic::error(
            codes::UNKNOWN_VARIANT,
            at,
            format!("`{shown}` has no variant `{name}`"),
        );
        if let Some(span) = declared {
            d = d.secondary(span, "declared here");
        }
        if !known.is_empty() {
            d = d.help(format!("its variants are {}", quoted(&known)));
        }
        self.errors.push(d);
        None
    }

    fn variant_def(&self, id: EnumId, at: u32) -> Option<&VariantDef> {
        self.res.enums().get(id)?.variants.get(at as usize)
    }

    fn variant_span(&self, id: EnumId, at: u32) -> Option<Span> {
        self.variant_def(id, at).map(|v| v.name_span)
    }

    /// `Enum.Variant`, for a message. A spelling, not an identity.
    fn variant_path(&self, id: EnumId, at: u32) -> String {
        let e = self.res.types().show(Type::Enum(id));
        match self.variant_def(id, at) {
            Some(v) => format!("{e}.{}", v.name),
            None => e.to_owned(),
        }
    }

    /// The payload fields of this variant that `given` does not mention.
    fn missing_payload_fields(&self, id: EnumId, at: u32, given: &[(String, Span)]) -> Vec<String> {
        self.variant_def(id, at).map_or_else(Vec::new, |v| {
            v.fields
                .iter()
                .filter(|f| !given.iter().any(|(g, _)| *g == f.name))
                .map(|f| f.name.clone())
                .collect()
        })
    }

    /// Check `base.name` and return the field's type.
    ///
    /// Field lookup begins at the base's **resolved type** and goes no further: there is
    /// no global field namespace, so a name that is a field of some other record is not a
    /// field of this one. `docs/spec.md`, *Field resolution starts from the type*.
    fn check_projection(&mut self, base_ty: Type, name: &str, name_span: Span) -> Option<Type> {
        // `x.show` with no call and no field of that name, where a trait in scope has the
        // method (N78): methods are not values.
        let is_field = matches!(base_ty, Type::Record(id)
            if self.res.records().get(id).is_some_and(|d| d.field(name).is_some()));
        let a_method = self.trait_names.values().flatten().any(|t| {
            self.res
                .traits()
                .get(*t)
                .is_some_and(|d| d.method(name).is_some())
        });
        if !is_field && a_method {
            self.errors.push(
                Diagnostic::error(
                    codes::METHOD_NOT_A_VALUE,
                    name_span,
                    format!("`.{name}` is a method, and a method is called, not taken as a value"),
                )
                .help(format!(
                    "call it, `x.{name}()`, or make a value of the call with a closure: \
                     `fn() -> … {{ x.{name}() }}`"
                )),
            );
            return None;
        }
        let Type::Record(id) = base_ty else {
            let shown = self.show(base_ty);
            let help = if base_ty.is_enum() {
                // An enum's payload depends on its active variant, so there is nothing a
                // projection could mean. `docs/spec.md` gives `match` as the one
                // elimination form, and saying so is more use than "not a record".
                "which fields an enum value has depends on its active variant, so a \
                 payload is read by `match` and not by `.`"
            } else {
                "`.` reads a declared field out of a record value"
            };
            self.errors.push(
                Diagnostic::error(
                    codes::INVALID_PROJECTION,
                    name_span,
                    format!("`{shown}` is not a record, so `.{name}` has nothing to read"),
                )
                .help(help),
            );
            return None;
        };
        let found = self
            .res
            .records()
            .get(id)
            .and_then(|d| d.field(name).map(|(i, f)| (i, f.ty)));
        let Some((index, ty)) = found else {
            // What the name was looked for on, for a tool completing it (N21).
            self.res.record_unresolved_member(name_span, base_ty);
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
        self.current = defined(self.res, self.module, f);
        self.closure_count = 0;
        self.effect_param = self.current.and_then(|id| {
            f.effect_params
                .first()
                .map(|p| (p.name.clone(), ParamOwner::Function(id)))
        });
        // The function's own type parameters are in scope for every type written inside
        // it, and in no other function.
        self.type_scope = self
            .current
            .map(|id| param_scope(&f.type_params, ParamOwner::Function(id)))
            .unwrap_or_default();
        // An impl's method may write its impl's type as `Self` (N78): its first parameter's.
        if f.method_of.is_some() {
            if let Some(ty) = self
                .current
                .and_then(|id| self.res.function(id))
                .and_then(|d| d.params.first().copied())
            {
                self.type_scope.push(("Self".to_owned(), ty));
            }
        }
        record_type_scope(self.res, f.span, &self.type_scope, &self.types);
        self.open_scope(f.span);
        for p in &f.params {
            let ty = self.signature_type(&p.ty);
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
            let slot = self.allocate(&p.name, p.name_span, ty, false);
            self.define(&p.name, ty, false, None, slot, p.name_span.end);
        }

        let declared = self.signature_type(&f.return_type);
        // Contracts (N87, §7.88): `Bool`, over the parameters and, after `ensures`, `result`.
        self.check_contracts(f, declared);
        if f.foreign.is_some() {
            // A foreign declaration has no body, and that is not a parse failure (N42).
            self.close_scope();
            return;
        }
        let Some(body) = &f.body else {
            // The body failed to parse; the parse error is already reported. Anything
            // said about statements that were never built would be about the parser's
            // guess, not the program.
            self.res.scopes_mut().incomplete();
            self.close_scope();
            return;
        };
        let previous = self.return_type.replace(declared);
        let previous_span = self.return_span.replace(f.return_type.span);
        let completion = self.check_block(body, Some(declared));
        self.return_type = previous;
        self.return_span = previous_span;

        match completion {
            // Every path returned. No tail is needed, and demanding one would force a
            // dummy expression after code that cannot reach it.
            Completion::Diverges => {}
            Completion::Value(actual) if self.assignable(actual, declared) => {}
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
        self.close_scope();
    }

    /// A function's contract clauses (N87, §7.88): each a `Bool`; a `requires` over the
    /// parameters, an `ensures` over them and `result`, the value being returned, in a slot of
    /// the function's own; and each made only of what cannot fail for want of authority or
    /// memory — literals, names, operators, projections and calls of pure functions (`N0614`).
    fn check_contracts(&mut self, f: &Function, declared: Type) {
        for c in &f.contracts {
            let ensures = c.kind == nazm_syntax::ContractKind::Ensures;
            if ensures {
                self.open_scope(c.span);
                let slot = match self.current.and_then(|id| self.res.contract_result(id)) {
                    Some(slot) => slot,
                    None => {
                        let slot = self.allocate("result", c.keyword_span, declared, false);
                        if let Some(id) = self.current {
                            self.res.record_contract_result(id, slot);
                        }
                        slot
                    }
                };
                self.define("result", declared, false, None, slot, c.keyword_span.end);
            }
            let got = self.value_type(&c.expr, Some(Type::Bool));
            if got != Type::Bool {
                let shown = self.show(got);
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        c.expr.span(),
                        format!("a contract clause is a `Bool`, but this is `{shown}`"),
                    )
                    .help("a clause states a condition: compare, or combine with `&&`, `||`, `!`"),
                );
            }
            if let Some((at, what)) = self.impure_clause(&c.expr) {
                self.errors.push(
                    Diagnostic::error(
                        codes::CONTRACT_SHAPE,
                        at,
                        format!("a contract clause cannot {what}"),
                    )
                    .primary_label("not a contract's")
                    .help(
                        "a clause is evaluated every time the function is entered or returns, \
                             and must not fail for want of authority or memory: it is literals, \
                             names, operators, field reads and calls of functions declaring `! {}`",
                    ),
                );
            }
            if ensures {
                self.close_scope();
            }
        }
    }

    /// The first part of a clause a contract may not contain (N87), with what it is.
    fn impure_clause(&self, e: &Expr) -> Option<(Span, &'static str)> {
        match e {
            Expr::Int { .. }
            | Expr::WideInt { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. }
            | Expr::Var { .. } => None,
            Expr::Neg { operand, .. }
            | Expr::Not { operand, .. }
            | Expr::BitNot { operand, .. } => self.impure_clause(operand),
            Expr::Array { span, .. } | Expr::ArrayRepeat { span, .. } => {
                Some((*span, "build a value"))
            }
            Expr::Index { base, index, .. } => self
                .impure_clause(base)
                .or_else(|| self.impure_clause(index)),
            Expr::Binary { lhs, rhs, .. } => {
                self.impure_clause(lhs).or_else(|| self.impure_clause(rhs))
            }
            Expr::Field { base, .. } => self.impure_clause(base),
            Expr::Call {
                qualifier,
                callee,
                callee_span,
                args,
                ..
            } => {
                let pure = self
                    .env
                    .get(&written_key(qualifier.as_ref(), callee))
                    .is_some_and(|v| v.sig.pure);
                if pure {
                    args.iter().find_map(|a| self.impure_clause(a))
                } else {
                    Some((*callee_span, "call a function that is not pure"))
                }
            }
            Expr::Closure { span, .. } => Some((*span, "make a closure")),
            Expr::Construct { span, .. } | Expr::Variant { span, .. } => {
                Some((*span, "build a value"))
            }
            Expr::Match { span, .. } | Expr::If { span, .. } => {
                Some((*span, "branch; write the condition with `&&` and `||`"))
            }
            Expr::Block(b) => Some((b.span, "contain a block")),
            Expr::Propagate { op_span, .. } => Some((*op_span, "propagate a failure")),
            Expr::MethodCall { span, .. } => Some((*span, "call a method")),
        }
    }

    fn check_block(&mut self, b: &Block, expected: Option<Type>) -> Completion {
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
        let Some(binding) = self.lookup_var(&place.name, place.span) else {
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
            Expr::Int { value, span } => {
                Completion::Value(self.integer_literal(i128::from(*value), *span, expected))
            }
            Expr::WideInt { value, span } => {
                Completion::Value(self.integer_literal(i128::from(*value), *span, expected))
            }
            // `Float32` where one is expected, `Float64` everywhere else (Gate 2).
            Expr::Float { span, .. } => Completion::Value(self.float_literal(*span, expected)),
            Expr::Bool { .. } => Completion::Value(Type::Bool),
            Expr::Str { .. } => Completion::Value(Type::Str),

            Expr::Var { name, span } => match self.lookup_var(name, *span) {
                Some(b) => {
                    // Which binding this mention refers to, recorded where the question
                    // was answered. The backend reads this instead of walking scopes again.
                    self.record_var(*span, b.slot);
                    Completion::Value(b.ty)
                }
                // A constant (Gate 2): its type, and its value for Core IR to write.
                None if self.types.consts.values.contains_key(name) => {
                    Completion::Value(self.const_value(name, *span))
                }
                None if self.env.contains(name) => {
                    Completion::Value(self.function_value(name, *span, expected))
                }
                None => {
                    self.errors.push(Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    ));
                    Completion::Value(expected.unwrap_or(Type::Int))
                }
            },

            Expr::Closure {
                params,
                return_type,
                effects,
                body,
                fn_span,
                span,
            } => Completion::Value(self.check_closure(
                params,
                return_type,
                effects.as_ref(),
                body,
                *fn_span,
                *span,
                expected,
            )),

            Expr::Neg {
                operand, op_span, ..
            } => Completion::Value(self.check_negation(operand, *op_span, expected)),

            // `~x` (Gate 2): the complement of an integer of any width.
            Expr::BitNot {
                operand, op_span, ..
            } => Completion::Value(self.check_complement(operand, *op_span, expected)),

            // Arrays (Gate 2).
            Expr::Array { elems, span } => {
                Completion::Value(self.check_array_literal(elems, *span, expected))
            }
            Expr::ArrayRepeat { value, count, span } => {
                Completion::Value(self.check_array_repeat(value, count, *span, expected))
            }
            Expr::Index {
                base,
                index,
                bracket_span,
                ..
            } => Completion::Value(self.check_index(base, index, *bracket_span, expected)),

            Expr::Not {
                operand, op_span, ..
            } => {
                let t = self.value_type(operand, Some(Type::Bool));
                if t != Type::Bool {
                    self.errors.push(Diagnostic::error(
                        codes::INVALID_OPERANDS,
                        *op_span,
                        format!("`!` applies to `Bool`, but this is `{}`", self.show(t)),
                    ));
                }
                Completion::Value(Type::Bool)
            }

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => Completion::Value(self.check_binary_expr(*op, lhs, rhs, *op_span, expected)),

            Expr::Call {
                qualifier,
                callee,
                callee_span,
                type_args,
                args,
                span,
            } => {
                // A binding of function type of that name hides a function of it (N50):
                // the call is through the value.
                if qualifier.is_none()
                    && type_args.is_none()
                    && matches!(self.scopes.lookup(callee).map(|b| b.ty), Some(Type::Fn(_)))
                {
                    return Completion::Value(self.check_indirect_call(
                        callee,
                        *callee_span,
                        args,
                        *span,
                        expected,
                    ));
                }
                // `UInt8(x)`, `try_convert[T](x)` and its kin (Gate 2): a numeric conversion,
                // where no function of the name is in scope.
                if qualifier.is_none() {
                    if let Some(t) = self.maybe_conversion(
                        callee,
                        *callee_span,
                        type_args.as_deref(),
                        args,
                        *span,
                    ) {
                        return Completion::Value(t);
                    }
                }
                // `m::f` is looked up as one key (N49); everything below names it as written.
                let key = written_key(qualifier.as_ref(), callee);
                let callee = &key;
                let Some(sig) = self.resolve_callee(callee, *callee_span) else {
                    let qualified = qualifier.is_some().then(|| {
                        unknown_qualified(callee, &self.alias_names(), *callee_span, "function")
                    });
                    let d = qualified
                        .or_else(|| self.why_invisible(callee, *callee_span))
                        .unwrap_or_else(|| {
                            // `IoCap()`: the question is where authority comes from (N37).
                            if let Some(Type::Cap(k)) = Type::from_name(callee) {
                                return forged(k, *callee_span);
                            }
                            Diagnostic::error(
                                codes::UNDEFINED_FUNCTION,
                                *callee_span,
                                format!("`{callee}` is not a function"),
                            )
                        });
                    self.errors.push(d);
                    // A completion probe's `Name(` with no function `Name` (N26): which
                    // record the module's types name by it, read and applied exactly as a
                    // construction's head is — so a generic record with no type arguments
                    // written names no instance, and nothing is recorded. Never in an
                    // ordinary program, whose `completion_call` is `None`.
                    if self.res.completion_call() == Some(*callee_span) {
                        self.called_record(callee, *callee_span, type_args.as_deref());
                    }
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

                if sig.generics > 0 || sig.effect_param {
                    let ret = self.check_generic_call(
                        callee,
                        *callee_span,
                        type_args.as_deref(),
                        args,
                        *span,
                        &sig,
                    );
                    return Completion::Value(ret);
                }
                if type_args.is_some() {
                    self.errors
                        .push(not_generic(callee, "not generic", *callee_span));
                }
                self.record_site(*callee_span, Vec::new(), false);

                // Left to right, matching the evaluation order the spec fixes.
                for (i, a) in args.iter().enumerate() {
                    let want = sig.params.get(i).copied();
                    let got = self.value_type(a, want);
                    if let Some(want) = want {
                        if !self.assignable(got, want) {
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
                self.record_checked_call(*callee_span, *span, args, &sig, None, false);
                self.check_callbacks(*callee_span, args, &sig);
                self.prove_preconditions(*callee_span, args);
                Completion::Value(sig.ret)
            }

            Expr::Construct {
                qualifier,
                ty,
                ty_span,
                type_args,
                fields,
                span,
            } => match self.check_construction(
                &written_key(qualifier.as_ref(), ty),
                *ty_span,
                type_args.as_deref(),
                fields,
                *span,
            ) {
                Some(t) => Completion::Value(t),
                // The type did not resolve, and that is already reported. `Int` is what
                // every unresolved type continues as, so no second error is raised here.
                None => Completion::Value(Type::Int),
            },

            // `x.m()` and `Show.m()` with nothing to tell them from a variant by (N78): a
            // call when the name before the `.` is a value or a trait rather than an enum.
            Expr::Variant {
                qualifier,
                ty,
                ty_span,
                type_args: None,
                variant,
                variant_span,
                fields,
                span,
            } if fields.is_empty() && self.names_a_receiver(qualifier.as_ref(), ty) => {
                let receiver = Expr::Var {
                    name: ty.clone(),
                    span: *ty_span,
                };
                Completion::Value(self.check_method_call(
                    qualifier.as_ref(),
                    &receiver,
                    variant,
                    *variant_span,
                    &[],
                    *span,
                    expected,
                ))
            }

            Expr::MethodCall {
                qualifier,
                receiver,
                method,
                method_span,
                args,
                span,
            } => Completion::Value(self.check_method_call(
                qualifier.as_ref(),
                receiver,
                method,
                *method_span,
                args,
                *span,
                expected,
            )),

            Expr::Variant {
                qualifier,
                ty,
                ty_span,
                type_args,
                variant,
                variant_span,
                fields,
                span,
            } => match self.check_variant(
                &written_key(qualifier.as_ref(), ty),
                *ty_span,
                type_args.as_deref(),
                variant,
                *variant_span,
                fields,
                *span,
            ) {
                Some(t) => Completion::Value(t),
                None => Completion::Value(expected.unwrap_or(Type::Int)),
            },

            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.check_match(scrutinee, arms, *span, expected),

            Expr::Field {
                base,
                name,
                name_span,
                ..
            } => {
                // `State.Rea` while `State.Ready(…)` is being written: `State` is no value,
                // so this is a projection that fails — but the module's types name `State`
                // as an enum, and a tool completing the name after `.` needs which (N21).
                // Recorded, not acted on: the diagnostics below are unchanged. A generic
                // enum is not recorded, since which instance is meant is not written.
                if let Expr::Var {
                    name: qualifier, ..
                } = &**base
                {
                    let is_value = self.scopes.lookup(qualifier).is_some();
                    let is_param = self.type_scope.iter().any(|(n, _)| n == qualifier);
                    if let (false, false, false, Some(Type::Enum(id))) = (
                        is_value,
                        is_param,
                        self.refused(qualifier),
                        self.types.names.get(qualifier).copied(),
                    ) {
                        if self.res.types().arity(ParamOwner::Enum(id)) == 0 {
                            self.res
                                .record_unresolved_member(*name_span, Type::Enum(id));
                        }
                    }
                }
                // The base is checked with no expectation: what a projection wants of its
                // base is "a record with this field", which is not a `Type` and so cannot
                // be passed as one.
                let base_ty = self.value_type(base, None);
                if let Type::Record(id) = base_ty {
                    if self
                        .res
                        .records()
                        .get(id)
                        .is_some_and(|d| d.repr == CRepr::Opaque)
                    {
                        let shown = self.show(base_ty);
                        self.errors
                            .push(opaque_refused(*name_span, &shown, "taken apart"));
                        return Completion::Value(Type::Int);
                    }
                }
                match self.check_projection(base_ty, name, *name_span) {
                    Some(t) => Completion::Value(t),
                    None => Completion::Value(Type::Int),
                }
            }

            Expr::Propagate {
                operand, op_span, ..
            } => self.check_propagate(operand, *op_span, expected),

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

    /// Record a call as it was checked (N20): its resolved callee, its arguments and the
    /// signature they were checked against, for a tool to read rather than work out.
    /// A call whose arguments are all literals, of a function of this module stating a
    /// precondition (N87, §7.88): each clause evaluated with them. False is `N0615` — the call
    /// would fail every time — and every clause true makes the call's obligation proved, with
    /// the evaluated clauses as its witness. Any other call is not reasoned about.
    fn prove_preconditions(&mut self, at: Span, args: &[Expr]) {
        let Some(Callee::Function(callee)) = self.res.callee(at) else {
            return;
        };
        let Some((names, clauses)) = self.preconditions.get(&callee) else {
            return;
        };
        let values: Option<Vec<Lit>> = args.iter().map(|a| literal(a, &HashMap::new())).collect();
        let Some(values) = values else {
            return;
        };
        if values.len() != names.len() {
            return;
        }
        let env: HashMap<&str, Lit> = names.iter().map(String::as_str).zip(values).collect();
        let mut witness: Vec<String> = Vec::new();
        for clause in clauses {
            match literal(clause, &env) {
                Some(Lit::Bool(true)) => witness.push(format!("{} holds", describe_clause(clause))),
                Some(Lit::Bool(false)) => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::PRECONDITION_REFUTED,
                            at,
                            "this call breaks its callee's precondition",
                        )
                        .secondary(clause.span(), "false for these arguments")
                        .help("with these literal arguments the call fails every time it runs (`N0410`)"),
                    );
                    return;
                }
                _ => return,
            }
        }
        let shown: Vec<String> = names
            .iter()
            .map(|n| {
                format!(
                    "{n} = {}",
                    env.get(n.as_str()).map_or_else(String::new, Lit::show)
                )
            })
            .collect();
        self.res.record_proved_call(
            at,
            format!("{} with {}", witness.join(", "), shown.join(", ")),
        );
    }

    /// A call of a foreign function passes each callback as an export's name (N85, §7.86):
    /// only an export has a C entry that needs no environment and lives as long as the
    /// process. A closure, a function value held in a binding, or a function that is not an
    /// export is `N0612`.
    fn check_callbacks(&mut self, at: Span, args: &[Expr], sig: &Signature) {
        let Some(Callee::Function(callee)) = self.res.callee(at) else {
            return;
        };
        if self
            .res
            .function(callee)
            .is_none_or(|d| d.foreign.is_none())
        {
            return;
        }
        for (a, want) in args.iter().zip(&sig.params) {
            if !matches!(want, Type::Fn(_)) {
                continue;
            }
            let export = self
                .res
                .fn_value(a.span())
                .and_then(|v| self.res.function(v.function))
                .is_some_and(|d| d.export.is_some());
            if !export {
                self.errors.push(
                    Diagnostic::error(
                        codes::CALLBACK_NOT_EXPORT,
                        a.span(),
                        "a callback C calls is the name of an export",
                    )
                    .primary_label("not an export")
                    .help(
                        "write the name of a `pub extern \"C\" fn … ! {} = \"symbol\" { … }`: \
                         its C entry needs no environment and lives as long as the process, \
                         which a closure or another function value does not",
                    ),
                );
            }
        }
    }

    fn record_checked_call(
        &mut self,
        at: Span,
        span: Span,
        args: &[Expr],
        sig: &Signature,
        instance: Option<Instantiation>,
        spawned: bool,
    ) {
        let Some(callee) = self.res.callee(at) else {
            return;
        };
        self.res.record_checked_call(
            at,
            CheckedCall {
                callee,
                span,
                args: args.iter().map(Expr::span).collect(),
                params: sig.params.clone(),
                ret: sig.ret,
                instance,
                spawned,
                preconditions: sig.preconditions,
            },
        );
    }

    /// Record one call of the body being checked, for the native backend's planner.
    fn record_site(&mut self, at: Span, type_args: Vec<Type>, spawned: bool) {
        let (Some(id), Some(callee)) = (self.current, self.res.callee(at)) else {
            return;
        };
        self.res.record_call_site(
            id,
            CallSite {
                span: at,
                callee,
                type_args,
                spawned,
                held: self.held(),
            },
        );
    }

    /// A call to a generic function: settle its type arguments, check its arguments
    /// against them, and return what it produces.
    ///
    /// The arguments are **written** or **inferred from the arguments**, and nothing
    /// else — the expected result type never takes part, which is what keeps a call's
    /// meaning visible in the call (`docs/spec.md`, *Calling a generic function*). Left to
    /// right: an argument whose declared type is already fully known is checked against
    /// it, exactly as a monomorphic call's is, and one whose declared type still mentions
    /// an unsettled parameter is unified with it.
    fn check_generic_call(
        &mut self,
        name: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
        sig: &Signature,
    ) -> Type {
        let Some(owner) = sig.owner else {
            return sig.ret;
        };
        let n = sig.generics as usize;
        // Each parameter's argument, and where it came from: an argument's index, or
        // `WRITTEN` for an explicit one.
        let mut solved: Vec<Option<(Type, usize)>> = vec![None; n];
        // The callee's effect parameter, once an argument binds it (N51).
        let mut bound: Option<(nazm_sema::EffectSet, usize)> = None;
        let caller = self.effect_param.as_ref().map(|e| e.1);
        let mut explicit = false;
        // Parameters an argument already failed on. One mistake is one error: an argument
        // of the wrong shape is reported as a mismatch, and the parameters its declared
        // type mentions are not reported again as uninferable.
        let mut excused: Vec<bool> = vec![false; n];
        if let Some(written) = written {
            let resolved: Vec<Type> = written
                .iter()
                .map(|w| self.resolve_written_type(w))
                .collect();
            if resolved.len() == n {
                for (slot, t) in solved.iter_mut().zip(resolved) {
                    *slot = Some((t, WRITTEN));
                }
                explicit = true;
            } else {
                self.errors.push(wrong_arity(name, n, resolved.len(), at));
            }
        }

        for (i, a) in args.iter().enumerate() {
            let Some(formal) = sig.params.get(i).copied() else {
                self.check_expr(a, None);
                continue;
            };
            // Checked against its declared type — as a monomorphic call's argument is — when
            // that type is fully known: it mentions no parameter, or every parameter was
            // **written**. A parameter settled by an earlier argument is not "written": the
            // two arguments are unified, so that `same(1, "x")` is a conflict between them
            // rather than the second one read against what the first one happened to say.
            let open = self.res.types().mentions(formal, owner)
                || (sig.effect_param && self.res.types().names_effect_param(formal, owner));
            if (explicit && !sig.effect_param) || !open {
                let now = settled_args(&solved, owner);
                let want = self.res.types_mut().substitute(formal, owner, &now);
                let got = self.value_type(a, Some(want));
                // As a monomorphic call's argument is (N79, N104): an attenuation is accepted.
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{name}` is `{}`, but this is `{}`",
                            i + 1,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
                continue;
            }
            // What is already known is the expectation — for recovery only: it is what an
            // argument that never produces a value stands in for, and it never settles a
            // parameter.
            let expected = solved.iter().all(Option::is_some).then(|| {
                let now = settled_args(&solved, owner);
                self.res.types_mut().substitute(formal, owner, &now)
            });
            match self.check_expr(a, expected) {
                Completion::Value(got) => {
                    match self.unify(formal, got, owner, &mut solved, &mut bound, caller, i) {
                        Ok(()) => {}
                        Err(Unify::EffectConflict { earlier, from }) => {
                            let pname = self.res.types().effect_param_name(Some(owner));
                            let shown = |set: nazm_sema::EffectSet, me: &Self| {
                                if set.is_empty() {
                                    "none".to_owned()
                                } else {
                                    format!("`{}`", me.show_effects(set))
                                }
                            };
                            let earlier = shown(earlier, self);
                            self.errors.push(
                                Diagnostic::error(
                                    codes::CONFLICTING_EFFECT_ARGUMENT,
                                    a.span(),
                                    format!(
                                        "`{pname}` is {earlier} from argument {}, and argument {} \
                                     binds it to something else",
                                        from + 1,
                                        i + 1
                                    ),
                                )
                                .secondary(sig.name_span, "declared here")
                                .help(format!(
                                    "every use of `{pname}` in `{name}`'s signature is one set of \
                                 effects; give the function values the same effects"
                                )),
                            );
                        }
                        Err(Unify::Conflict {
                            param,
                            earlier,
                            from,
                        }) => {
                            let p = self.res.types().param_name(param);
                            let earlier = self.show(earlier);
                            let source = if from == WRITTEN {
                                "the written type arguments".to_owned()
                            } else {
                                format!("argument {}", from + 1)
                            };
                            let implied =
                                inferred_from(self.res.types(), formal, got, param).unwrap_or(got);
                            let implied = self.show(implied);
                            self.errors.push(
                                Diagnostic::error(
                                    codes::CONFLICTING_TYPE_ARGUMENTS,
                                    a.span(),
                                    format!(
                                    "`{p}` is `{earlier}` from {source}, but argument {} makes \
                                     it `{implied}`",
                                    i + 1
                                ),
                                )
                                .help(format!(
                                    "every use of `{p}` in `{name}`'s signature is one type. \
                                 Inference does not choose the first and convert the rest"
                                )),
                            );
                        }
                        Err(Unify::Mismatch) => {
                            excuse(self.res.types(), formal, owner, &mut excused);
                            let partial = settled_args(&solved, owner);
                            let want = self.res.types_mut().substitute(formal, owner, &partial);
                            self.errors.push(Diagnostic::error(
                                codes::TYPE_MISMATCH,
                                a.span(),
                                format!(
                                    "argument {} of `{name}` is `{}`, but this is `{}`",
                                    i + 1,
                                    self.show(want),
                                    self.show(got)
                                ),
                            ));
                        }
                    }
                }
                Completion::Unit => {
                    excuse(self.res.types(), formal, owner, &mut excused);
                    self.errors.push(
                        Diagnostic::error(codes::TYPE_MISMATCH, a.span(), "this produces no value")
                            .help(
                                "an `if` used as a value needs an `else`, and a block used as \
                                 one needs a final expression with no `;` after it",
                            ),
                    );
                }
                // An argument that never produces a value says nothing about a type.
                Completion::Diverges => {}
            }
        }

        let missing: Vec<String> = solved
            .iter()
            .enumerate()
            .filter(|(i, s)| s.is_none() && !excused.get(*i).copied().unwrap_or(false))
            .map(|(i, _)| {
                self.res.types().param_name(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                })
            })
            .collect();
        if !missing.is_empty() {
            self.errors.push(
                Diagnostic::error(
                    codes::CANNOT_INFER_TYPE_ARGUMENT,
                    at,
                    format!(
                        "nothing in this call to `{name}` says what {} {}",
                        quoted(&missing),
                        if missing.len() == 1 { "is" } else { "are" }
                    ),
                )
                .secondary(sig.name_span, "declared here")
                .help(format!(
                    "a type argument is inferred from the arguments and never from what the \
                     result is used for. Write it: `{name}[…](…)`"
                )),
            );
        }

        // What each parameter's declaration requires, of the argument it was given (N14).
        // Asked of the one derivation `==` asks, so a concrete argument answers by N13's rule
        // and a caller's own parameter answers by its declaration: forwarding a `T` to a
        // callee that requires equality works exactly when the caller's `T` requires it too.
        // Written or inferred makes no difference to the rule, only to where it is reported.
        for (i, s) in solved.iter().enumerate() {
            let Some((t, from)) = *s else {
                continue;
            };
            let param = TypeParam {
                owner,
                index: u32::try_from(i).unwrap_or(u32::MAX),
            };
            // A trait bound (N78): the argument has a usable impl, or is a caller's own
            // parameter bound by the same trait.
            if let Some(tr) = self.res.traits().bound_of(param) {
                let satisfied = match t {
                    Type::Param(q) => self.res.traits().bound_of(q) == Some(tr),
                    _ => self
                        .res
                        .traits()
                        .find(tr, t)
                        .and_then(|imp| self.res.traits().impl_def(imp))
                        .is_some_and(|d| self.impl_modules.contains(&d.module)),
                };
                if !satisfied {
                    let p = self.res.types().param_name(param);
                    let shown = self.show(t);
                    let tname = self
                        .res
                        .traits()
                        .get(tr)
                        .map_or_else(|| "?".to_owned(), |d| d.name.clone());
                    let place = if from == WRITTEN {
                        at
                    } else {
                        args.get(from).map_or(at, Expr::span)
                    };
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNSATISFIED_REQUIREMENT,
                            place,
                            format!(
                                "`{name}` requires `{p}: {tname}`, and `{p}` is `{shown}` here, \
                                 which has no impl of `{tname}` in this module's scope"
                            ),
                        )
                        .secondary(sig.name_span, "declared here")
                        .help(if let Type::Param(_) = t {
                            format!("declare `{shown}: {tname}` on this definition")
                        } else {
                            format!(
                                "implement it, `impl {tname} for {shown}`, in the module of \
                                 `{tname}` or of `{shown}`, and import that module here"
                            )
                        }),
                    );
                }
                continue;
            }
            if !self.res.types().requires_equality(param) {
                continue;
            }
            let Err(why) = self.res.types().equality(t) else {
                continue;
            };
            let p = self.res.types().param_name(param);
            let shown = self.show(t);
            let detail = if let Type::Param(_) = t {
                format!("this `{shown}` is a type parameter whose declaration does not require it")
            } else if why.path.is_empty() {
                format!("`{shown}` has no equality")
            } else {
                format!(
                    "its `{}` is a `{}`, which has no equality",
                    why.path.join("."),
                    self.show(why.ty)
                )
            };
            // A written argument is reported at the callee, where `N0353` reports the list;
            // an inferred one at the argument it was inferred from.
            let place = if from == WRITTEN {
                at
            } else {
                args.get(from).map_or(at, Expr::span)
            };
            self.errors.push(
                Diagnostic::error(
                    codes::UNSATISFIED_REQUIREMENT,
                    place,
                    if let Type::Param(_) = t {
                        format!("`{name}` requires `{p}: {EQUALITY}`, and {detail}")
                    } else {
                        format!(
                            "`{name}` requires `{p}: {EQUALITY}`, and `{p}` is `{shown}` here: \
                             {detail}"
                        )
                    },
                )
                .secondary(sig.name_span, "declared here")
                .help(if let Type::Param(_) = t {
                    format!(
                        "a generic definition may pass on only what it requires itself: \
                         declare `{shown}: {EQUALITY}`"
                    )
                } else {
                    "equality is derived from what a type contains; a sequence, a `Vec` or a \
                     `Chan` anywhere inside a type gives it none"
                        .to_owned()
                }),
            );
        }

        let settled: Vec<Type> = solved
            .iter()
            .map(|s| s.map_or(Type::Int, |(t, _)| t))
            .collect();
        for t in &settled {
            self.mention(*t);
        }
        // A generic numeric built-in's `T` is in its class (Gate 2): `count_ones` takes an
        // integer, `float_sqrt` a float.
        if let ParamOwner::Builtin(i) = owner {
            if let (Some(class), Some(t)) = (i.requires(), settled.first().copied()) {
                if !class.admits(t) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        at,
                        format!(
                            "`{name}` applies to {}, and this is `{}`",
                            class.describe(),
                            self.show(t)
                        ),
                    ));
                }
            }
        }
        // The one generic built-in that makes a `Chan[T]` from a written `T` (N53).
        if owner == ParamOwner::Builtin(Intrinsic::ChanNewOf) {
            if let Some(d) = settled
                .first()
                .and_then(|t| channel_element(self.res.types(), *t, at))
            {
                self.errors.push(d);
            }
        }
        self.res.record_type_args(at, settled.clone());
        self.record_site(at, settled.clone(), false);
        // What this call settled, for tooling: unsettled parameters stay themselves, and
        // the substitution only reads what already exists, so recording it interns nothing.
        let partial = settled_args(&solved, owner);
        let instance = Instantiation {
            owner,
            written: explicit,
            args: solved.iter().map(|s| s.map(|(t, _)| t)).collect(),
            params: sig
                .params
                .iter()
                .map(|p| self.res.types().substituted(*p, owner, &partial))
                .collect(),
            ret: self.res.types().substituted(sig.ret, owner, &partial),
        };
        let mut instance = instance;
        let mut ret = self.res.types_mut().substitute(sig.ret, owner, &settled);
        // The effect parameter, as this call bound it (N51): what the call performs, and what
        // a function type in its result says — which is the result the call has.
        if sig.effect_param {
            let binding = bound.map_or(nazm_sema::EffectSet::EMPTY, |(b, _)| b);
            self.res.record_effect_binding(at, binding);
            ret = self
                .res
                .types_mut()
                .bind_effects(ret, owner, binding, caller);
            instance.ret = Some(ret);
        }
        self.record_checked_call(at, span, args, sig, Some(instance), false);
        self.mention(ret);
        if self.res.types().nesting(ret) > MAX_TYPE_DEPTH {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_NESTING_TOO_DEEP,
                    span,
                    format!("this call produces a type nested more than {MAX_TYPE_DEPTH} deep"),
                )
                .help("types may nest 32 deep, and the result of a generic call is a type too"),
            );
            return Type::Int;
        }
        ret
    }

    /// Match a declared parameter type against an argument's type, settling the callee's
    /// parameters it mentions. Structural through applications, `Vec` and function types;
    /// nothing else is.
    #[allow(clippy::too_many_arguments)]
    fn unify(
        &self,
        formal: Type,
        actual: Type,
        owner: ParamOwner,
        solved: &mut [Option<(Type, usize)>],
        bound: &mut Option<(nazm_sema::EffectSet, usize)>,
        caller: Option<ParamOwner>,
        arg: usize,
    ) -> Result<(), Unify> {
        let types = self.res.types();
        if !types.mentions(formal, owner) && !types.names_effect_param(formal, owner) {
            return if formal == actual {
                Ok(())
            } else {
                Err(Unify::Mismatch)
            };
        }
        match formal {
            Type::Param(p) if p.owner == owner => {
                let Some(slot) = solved.get_mut(p.index as usize) else {
                    return Err(Unify::Mismatch);
                };
                match *slot {
                    None => {
                        *slot = Some((actual, arg));
                        Ok(())
                    }
                    Some((t, _)) if t == actual => Ok(()),
                    Some((t, from)) => Err(Unify::Conflict {
                        param: p,
                        earlier: t,
                        from,
                    }),
                }
            }
            Type::Vec(_) => {
                let (Type::Vec(_), Some(fe), Some(ae)) = (
                    actual,
                    types.type_args(formal).first().copied(),
                    types.type_args(actual).first().copied(),
                ) else {
                    return Err(Unify::Mismatch);
                };
                self.unify(fe, ae, owner, solved, bound, caller, arg)
            }
            // A generic channel, as a `Vec` is: through its element (N53).
            Type::ChanOf(_) => {
                let (Type::ChanOf(_), Some(fe), Some(ae)) = (
                    actual,
                    types.type_args(formal).first().copied(),
                    types.type_args(actual).first().copied(),
                ) else {
                    return Err(Unify::Mismatch);
                };
                self.unify(fe, ae, owner, solved, bound, caller, arg)
            }
            Type::Record(_) | Type::Enum(_) => {
                if types.definition_of(formal) != types.definition_of(actual)
                    || std::mem::discriminant(&formal) != std::mem::discriminant(&actual)
                {
                    return Err(Unify::Mismatch);
                }
                let (fa, aa) = (types.type_args(formal), types.type_args(actual));
                if fa.len() != aa.len() {
                    return Err(Unify::Mismatch);
                }
                for (f, a) in fa.into_iter().zip(aa) {
                    self.unify(f, a, owner, solved, bound, caller, arg)?;
                }
                Ok(())
            }
            // Structural through a function type's parameters and result (N50). Its effects
            // are the same set — or, where it names the callee's effect parameter, bind it to
            // what the argument has beyond the named ones (N51).
            Type::Fn(ff) => {
                let Type::Fn(af) = actual else {
                    return Err(Unify::Mismatch);
                };
                let (Some(fs), Some(asig)) = (types.fn_signature(ff), types.fn_signature(af))
                else {
                    return Err(Unify::Mismatch);
                };
                if fs.params.len() != asig.params.len() {
                    return Err(Unify::Mismatch);
                }
                if fs.row == Some(owner) {
                    let named = fs.effects.named();
                    // A parameter in the argument's effects can only be the caller's own.
                    if !named.is_within(asig.effects)
                        || (asig.effects.has_param() && asig.row != caller)
                    {
                        return Err(Unify::Mismatch);
                    }
                    let binding = asig.effects.without(named);
                    match *bound {
                        None => *bound = Some((binding, arg)),
                        Some((b, _)) if b == binding => {}
                        Some((earlier, from)) => {
                            return Err(Unify::EffectConflict { earlier, from });
                        }
                    }
                } else if fs.effects != asig.effects || fs.row != asig.row {
                    return Err(Unify::Mismatch);
                }
                let pairs: Vec<(Type, Type)> = fs
                    .params
                    .iter()
                    .copied()
                    .zip(asig.params.iter().copied())
                    .chain(std::iter::once((fs.ret, asig.ret)))
                    .collect();
                for (f, a) in pairs {
                    self.unify(f, a, owner, solved, bound, caller, arg)?;
                }
                Ok(())
            }
            _ => Err(Unify::Mismatch),
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
            // Arithmetic is on two numbers of **one** type (Gate 2; `Int` only before it). No
            // string concatenation, and `+` on two `Bool`s is a mistake rather than something
            // to define. Nothing converts: `Int + UInt8` is refused.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                lt == rt && lt.num().is_some()
            }
            // Ordering is numeric only: comparing strings needs a collation decision, and
            // ordering booleans is almost always a mistake.
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => lt == rt && lt.num().is_some(),
            // The bit operators (Gate 2): two integers of one type, or for a shift an integer
            // and an `Int` amount.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => lt == rt && lt.is_integer(),
            BinOp::Shl | BinOp::Shr => lt.is_integer() && rt == Type::Int,
            // Equality needs **one** type — two records with identical fields are two
            // types, and nothing converts — and that type must have equality, which is
            // derived rather than listed (`docs/spec.md`, *Equality is derived*, N13).
            // `Int`, `Bool` and `Str` have it. A sequence, a `Vec` and a `Chan` do not:
            // equality on a handle would have to mean either "the same storage" or "the
            // same contents", the language has chosen neither, and offering one silently
            // is how a program comes to depend on the wrong answer. A record or an enum has
            // it exactly when everything it contains does, over every variant. A type
            // parameter does not: `==` is not valid for every type, and an unconstrained
            // `T` supports only what every type supports.
            BinOp::Eq | BinOp::NotEq => lt == rt && self.res.types().equality(lt).is_ok(),
            // `&&` and `||` are `Bool` only (N49): no truthiness, so `1 && 2` is a mistake.
            BinOp::And | BinOp::Or => lt == Type::Bool && rt == Type::Bool,
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
            // For a composite without equality, the component responsible — the first a
            // reader would find, `Outer.inner.values` — so the refusal says why rather than
            // only that.
            let blocked = match op {
                BinOp::Eq | BinOp::NotEq if lt == rt => self.res.types().equality(lt).err(),
                _ => None,
            };
            // A type parameter without a requirement is the one blocker the author can lift
            // where they stand, by declaring it (N14); say so, naming the parameter.
            let unrequired = blocked.as_ref().and_then(|w| match w.ty {
                Type::Param(_) => Some(self.show(w.ty)),
                _ => None,
            });
            let detail = if let Some(why) = blocked.filter(|w| !w.path.is_empty()) {
                format!(
                    "`{}` does not apply to `{slt}`: its `{}` is a `{}`, which has no equality",
                    op.symbol(),
                    why.path.join("."),
                    self.show(why.ty)
                )
            } else if lt == rt {
                format!("`{}` does not apply to `{slt}`", op.symbol())
            } else {
                format!("`{}` cannot {verb} `{slt}` with `{srt}`", op.symbol())
            };
            let mut refusal = Diagnostic::error(codes::INVALID_OPERANDS, op_span, detail)
                .secondary(lhs_span, format!("this is `{slt}`"))
                .secondary(rhs_span, format!("this is `{srt}`"));
            if let Some(p) = unrequired {
                refusal = refusal.help(format!(
                    "`{p}` is not known to have equality, because its declaration does not \
                     require it; a definition that compares one declares `{p}: {EQUALITY}`"
                ));
            }
            self.errors.push(refusal);

            // The operands were wrong, so this expression has no honest type. Reporting
            // the operator's usual one produces a second error about a mismatch the
            // author never wrote — two diagnostics for one mistake, and the second is
            // fiction. Adopt what the context wanted instead.
            return expected.unwrap_or(if op.is_comparison() || op.is_logical() {
                Type::Bool
            } else if lt.num().is_some() {
                lt
            } else {
                Type::Int
            });
        }

        if op.is_comparison() || op.is_logical() {
            Type::Bool
        } else {
            lt
        }
    }

    /// A constant named as a value (Gate 2): its declared type, its value recorded for lowering.
    #[inline(never)]
    fn const_value(&mut self, name: &str, at: Span) -> Type {
        let Some((ty, value)) = self.types.consts.values.get(name).cloned() else {
            return Type::Int;
        };
        self.res.record_const_use(at, value);
        ty
    }

    /// The element type an expected array type gives, if one is expected.
    fn expected_element(&self, expected: Option<Type>) -> Option<Type> {
        match expected {
            Some(Type::Array(a)) => self.res.types().array_info(a).map(|(e, _)| e),
            _ => None,
        }
    }

    /// Refuse an array type written or made in a body, where every definition is known.
    fn validate_array(&mut self, t: Type, at: Span) {
        if let Some(d) = array_refusal(self.res, t, at) {
            self.errors.push(d);
        }
    }

    /// `[a, b, c]` (Gate 2): every element of one type, the first's or the one expected.
    #[inline(never)]
    fn check_array_literal(&mut self, elems: &[Expr], span: Span, expected: Option<Type>) -> Type {
        let want = self.expected_element(expected);
        let Some(first) = elems.first() else {
            self.errors.push(
                Diagnostic::error(
                    codes::ARRAY_TOO_LARGE,
                    span,
                    "an array has at least one element",
                )
                .help("an empty collection is a `Vec`: `vec_new[Int]()`"),
            );
            return expected.unwrap_or(Type::Int);
        };
        let elem = self.value_type(first, want);
        for e in elems.iter().skip(1) {
            let t = self.value_type(e, Some(elem));
            if t != elem {
                self.errors.push(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    e.span(),
                    format!(
                        "this array's elements are `{}`, and this is `{}`",
                        self.show(elem),
                        self.show(t)
                    ),
                ));
            }
        }
        let n = u32::try_from(elems.len()).unwrap_or(u32::MAX);
        let t = self.res.types_mut().array_of(elem, n);
        self.validate_array(t, span);
        t
    }

    /// `[value; count]` (Gate 2): `count` copies, `count` a literal or a constant.
    #[inline(never)]
    fn check_array_repeat(
        &mut self,
        value: &Expr,
        count: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let want = self.expected_element(expected);
        let elem = self.value_type(value, want);
        let n = match count {
            Expr::Var { name, span } if self.types.consts.values.contains_key(name) => {
                self.const_value(name, *span);
                array_length(&self.types, count, &mut self.errors)
            }
            _ => array_length(&self.types, count, &mut self.errors),
        };
        let t = self.res.types_mut().array_of(elem, n.unwrap_or(1));
        if n.is_some() {
            self.validate_array(t, span);
        }
        t
    }

    /// An index's type, which is an `Int`, and a constant one's range (Gate 2).
    fn check_index_value(&mut self, index: &Expr, len: Option<u32>) {
        let it = self.value_type(index, Some(Type::Int));
        if it != Type::Int {
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                index.span(),
                format!("an index is an `Int`, and this is `{}`", self.show(it)),
            ));
            return;
        }
        let constant = match index {
            Expr::Int { value, .. } => Some(i128::from(*value)),
            Expr::Neg { operand, .. } => match operand.as_ref() {
                Expr::Int { value, .. } => Some(-i128::from(*value)),
                _ => None,
            },
            _ => None,
        };
        if let (Some(i), Some(n)) = (constant, len) {
            if i < 0 || i >= i128::from(n) {
                self.errors.push(Diagnostic::error(
                    codes::ARRAY_INDEX,
                    index.span(),
                    format!("index {i} is outside an array of {n} elements"),
                ));
            }
        }
    }

    /// `base[index]` (Gate 2): an element of an array.
    #[inline(never)]
    fn check_index(
        &mut self,
        base: &Expr,
        index: &Expr,
        bracket: Span,
        expected: Option<Type>,
    ) -> Type {
        let bt = self.value_type(base, None);
        let info = match bt {
            Type::Array(a) => self.res.types().array_info(a),
            _ => None,
        };
        self.check_index_value(index, info.map(|(_, n)| n));
        match info {
            Some((elem, _)) => elem,
            None => {
                let mut d = Diagnostic::error(
                    codes::ARRAY_INDEX,
                    bracket,
                    format!("`[…]` indexes an array, and this is `{}`", self.show(bt)),
                );
                if bt.is_sequence() {
                    d = d.help("a `Vec`'s element is read with `vec_get(xs, i)`");
                }
                self.errors.push(d);
                expected.unwrap_or(Type::Int)
            }
        }
    }

    /// A float literal's type (Gate 2): `Float32` where one is expected, `Float64` everywhere else.
    #[inline(never)]
    fn float_literal(&mut self, span: Span, expected: Option<Type>) -> Type {
        if expected == Some(Type::Num(NumKind::F32)) {
            self.res
                .record_numeric(span, NumericFact::Literal(Type::Num(NumKind::F32)));
            Type::Num(NumKind::F32)
        } else {
            Type::Num(NumKind::F64)
        }
    }

    /// `-operand` (Gate 2 widened it to every numeric type). Its own function, out of
    /// [`Self::check_expr`]: that one recurses once per nesting level, and its frame is what
    /// bounds how deep a program may nest before the checker's stack runs out.
    #[inline(never)]
    fn check_negation(&mut self, operand: &Expr, op_span: Span, expected: Option<Type>) -> Type {
        // `-128` written where an `Int8` is expected is one literal, judged whole.
        if let Expr::Int { value, span } = operand {
            if let Some(k) = expected.and_then(Type::num).filter(|k| k.is_integer()) {
                if k != NumKind::I64 {
                    return self.integer_literal(-i128::from(*value), *span, expected);
                }
            }
        }
        let hint = expected.filter(|t| t.num().is_some()).or(Some(Type::Int));
        let t = self.value_type(operand, hint);
        if t.num().is_none() {
            self.errors.push(Diagnostic::error(
                codes::INVALID_OPERANDS,
                op_span,
                format!("`-` applies to a number, but this is `{}`", self.show(t)),
            ));
            return Type::Int;
        }
        t
    }

    /// `~operand` (Gate 2), out of [`Self::check_expr`] for the reason [`Self::check_negation`] is.
    #[inline(never)]
    fn check_complement(&mut self, operand: &Expr, op_span: Span, expected: Option<Type>) -> Type {
        let hint = expected.filter(|t| t.is_integer()).or(Some(Type::Int));
        let t = self.value_type(operand, hint);
        if !t.is_integer() {
            self.errors.push(Diagnostic::error(
                codes::INVALID_OPERANDS,
                op_span,
                format!("`~` applies to an integer, but this is `{}`", self.show(t)),
            ));
            return Type::Int;
        }
        t
    }

    /// A binary operator's operands and result, out of [`Self::check_expr`] for the reason
    /// [`Self::check_negation`] is. A literal takes the type the other operand has (Gate 2): `x + 1`
    /// and `1 + x` with `x: UInt8` are both `UInt8` arithmetic. A shift's amount is an `Int`.
    #[inline(never)]
    fn check_binary_expr(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        op_span: Span,
        expected: Option<Type>,
    ) -> Type {
        let shift = matches!(op, BinOp::Shl | BinOp::Shr);
        let hint = if op.is_comparison() || op.is_logical() {
            None
        } else {
            expected.filter(|t| t.num().is_some())
        };
        let (lt, rt) = if shift {
            let lt = self.value_type(lhs, hint.filter(|t| t.is_integer()));
            let rt = self.value_type(rhs, Some(Type::Int));
            (lt, rt)
        } else if is_number_literal(lhs) && !is_number_literal(rhs) {
            let rt = self.value_type(rhs, hint);
            let lt = self.value_type(lhs, Some(rt).filter(|t| t.num().is_some()).or(hint));
            (lt, rt)
        } else {
            let lt = self.value_type(lhs, hint);
            let rt = self.value_type(rhs, Some(lt).filter(|t| t.num().is_some()).or(hint));
            (lt, rt)
        };
        if shift {
            self.refuse_constant_shift(lt, rhs);
        }
        self.check_binary(
            &BinaryCheck {
                op,
                lt,
                rt,
                op_span,
                lhs_span: lhs.span(),
                rhs_span: rhs.span(),
            },
            expected,
        )
    }

    /// A numeric conversion's type, where `callee` names one and no function of that name is
    /// in scope; `None` for every other call (Gate 2).
    #[inline(never)]
    fn maybe_conversion(
        &mut self,
        callee: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
    ) -> Option<Type> {
        if self.env.contains(callee) {
            return None;
        }
        let mode = match callee {
            "try_convert" => ConvertMode::Try,
            "wrap_convert" => ConvertMode::Wrap,
            "saturate_convert" => ConvertMode::Saturate,
            name if NumKind::by_name(name).is_some() => ConvertMode::Exact,
            _ => return None,
        };
        Some(self.check_conversion(callee, at, written, args, span, mode))
    }

    /// An integer literal's type (Gate 2): the integer type expected, where one is, and `Int`
    /// everywhere else — refused, by name, when its value does not fit that type.
    #[inline(never)]
    fn integer_literal(&mut self, v: i128, span: Span, expected: Option<Type>) -> Type {
        let ty = match expected.and_then(Type::num) {
            Some(k) if k.is_integer() => Type::of_num(k),
            _ => Type::Int,
        };
        let k = ty.num().unwrap_or(NumKind::I64);
        if NumVal::int(k, v).is_none() {
            let (lo, hi) = k.int_range().unwrap_or((0, 0));
            self.errors.push(
                Diagnostic::error(
                    codes::INTEGER_TOO_LARGE,
                    span,
                    format!("`{v}` does not fit in `{}`", k.name()),
                )
                .help(format!("`{}` holds {lo} to {hi}", k.name())),
            );
        }
        if ty != Type::Int {
            self.res.record_numeric(span, NumericFact::Literal(ty));
        }
        ty
    }

    /// A shift by a constant amount outside `0..width` (Gate 2): it would stop the program
    /// every time it ran, so it is refused where it is written (`N0617`).
    fn refuse_constant_shift(&mut self, lt: Type, amount: &Expr) {
        let Some(k) = lt.num().filter(|k| k.is_integer()) else {
            return;
        };
        let n = match amount {
            Expr::Int { value, .. } => i128::from(*value),
            Expr::Neg { operand, .. } => match operand.as_ref() {
                Expr::Int { value, .. } => -i128::from(*value),
                _ => return,
            },
            _ => return,
        };
        if n < 0 || n >= i128::from(k.bits()) {
            self.errors.push(
                Diagnostic::error(
                    codes::SHIFT_AMOUNT_REFUSED,
                    amount.span(),
                    format!("`{}` cannot be shifted by {n}", k.name()),
                )
                .help(format!(
                    "a shift's amount is 0 to {} for a `{}`; a larger or negative one stops \
                     the program (`N0412`) rather than being masked",
                    k.bits() - 1,
                    k.name()
                )),
            );
        }
    }

    /// A numeric conversion (Gate 2): `T(x)` exactly, or `try_convert[T](x)`,
    /// `wrap_convert[T](x)`, `saturate_convert[T](x)`. `docs/general-purpose.md` §1–§2.
    fn check_conversion(
        &mut self,
        name: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
        mode: ConvertMode,
    ) -> Type {
        let to = if mode == ConvertMode::Exact {
            if written.is_some() {
                self.errors
                    .push(not_generic(name, "a conversion, not generic", at));
            }
            NumKind::by_name(name).map_or(Type::Int, Type::of_num)
        } else {
            match written {
                Some([one]) => {
                    let t = self.resolve_written_type(one);
                    if t.num().is_none() {
                        self.errors.push(Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            at,
                            format!(
                                "`{name}` converts to a numeric type, and `{}` is not one",
                                self.show(t)
                            ),
                        ));
                        Type::Int
                    } else {
                        t
                    }
                }
                _ => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CANNOT_INFER_TYPE_ARGUMENT,
                            at,
                            format!("`{name}` needs the type it converts to, written"),
                        )
                        .help(format!("write it as `{name}[UInt8](x)`")),
                    );
                    Type::Int
                }
            }
        };
        if args.len() != 1 {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{name}` takes 1 argument, but {} {} given",
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        // A literal converted exactly is that literal, written at the target type.
        let hint = (mode == ConvertMode::Exact).then_some(to);
        let mut from = Type::Int;
        for (i, a) in args.iter().enumerate() {
            let t = self.value_type(a, hint);
            if i == 0 {
                from = t;
            }
        }
        if from.num().is_none() {
            if let Some(a) = args.first() {
                self.errors.push(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    a.span(),
                    format!(
                        "`{name}` converts a number, but this is `{}`",
                        self.show(from)
                    ),
                ));
            }
        } else if mode == ConvertMode::Wrap && !(from.is_integer() && to.is_integer()) {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    at,
                    format!(
                        "`wrap_convert` is between integer types, and this converts `{}` to `{}`",
                        self.show(from),
                        self.show(to)
                    ),
                )
                .help("a float converts by `T(x)`, `try_convert` or `saturate_convert`"),
            );
        }
        let result = if mode == ConvertMode::Try {
            match self.option {
                Some(o) => self.res.types_mut().apply_enum(o, vec![to]),
                None => to,
            }
        } else {
            to
        };
        self.res.record_numeric(
            at,
            NumericFact::Convert {
                to,
                from,
                mode,
                result,
            },
        );
        result
    }
}
