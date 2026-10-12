//! Type and scope checking.
//!
//! Runs before evaluation and reports **every** error it can, not just the first: an
//! agent that fixes one error and recompiles to find the next spends a turn per mistake,
//! and the whole argument for this language rests on not doing that.
//!
//! Where an error makes a subexpression's type unknowable, checking continues with the
//! *expected* type rather than stopping. That produces one honest error instead of a
//! cascade of invented ones.

use nazm_diag::{codes, Diagnostic};
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
use nazm_sema::record::{CRepr, FieldDef, RecordDef, RecordId, OPAQUE_FIELD};
use nazm_sema::resolve::{
    CallSite, Callee, CheckedCall, FieldRef, FnDef, FnId, Instantiation, LocalDef, LocalRef,
    PayloadRef, Propagation, Resolution, VariantRef, WrittenType,
};
use nazm_sema::traits::{ImplDef, TraitCall, TraitDef, TraitId, TraitMethodDef};
use nazm_sema::types::{ParamOwner, Type, TypeParam};
use nazm_sema::udt::{TypeNode, UserTypes, MAX_TYPE_DEPTH};
use nazm_sema::{ConvertMode, NumKind, NumericFact};

pub mod attributes;
mod call;
mod clauses;
mod construct;
mod declare;
mod expr;
mod foreign;
mod function;
mod generic;
mod layout;
mod operators;
mod pattern;
mod reexport;
mod scope;
mod stmt;
mod traits;
mod unit;
mod usefulness;
mod written;

pub use declare::{declare_types, resolve_type_fields, EQUALITY};
pub use foreign::{foreign_param, foreign_result, CrossIn, CrossOut, RESERVED_C_SYMBOLS};
pub use layout::{validate_new_arrays, validate_type_layouts};
pub use traits::declare_trait_methods;
pub(crate) use traits::defined;
pub use unit::{check_unit, declare_unit, declared_interfaces};

use std::collections::HashMap;

use nazm_sema::numeric::NumVal;
use nazm_sema::ModuleFacts;
use nazm_span::Span;

use crate::check::generic::polymorphic;
use crate::check::reexport::apply_reexports;

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
    /// The loop depth where the `defer` body being checked begins (R1b, L12), or `None` outside
    /// one: a `break` or `continue` at that depth, a `return` or a `?` would leave it.
    defer_floor: Option<u32>,
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
///
/// (A module's position among `units` is its id's index.)
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
    // The trait a record, enum or tuple derives (L6): `@std/hash`'s `Hash`, if it is here.
    let hash_key = nazm_sema::stdlib::key("hash");
    let derived = res
        .traits()
        .iter()
        .find(|(_, d)| d.name == "Hash" && graph.key(d.module) == Some(&hash_key))
        .map(|(t, _)| t);
    res.traits_mut().set_derived_hash(derived);
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
    // Stage 1b (L8): every module's constants, in dependency order.
    for n in unit::declare_all_consts(&units, &mut interfaces, &mut res, &mut errors) {
        if let Some(c) = declared_cleanly.get_mut(n) {
            *c = false;
        }
    }
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
