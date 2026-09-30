//! Core IR to [`crate::ir`], with everything outside the native subset refused by name.
//!
//! # What this reads
//!
//! Core IR (`docs/architecture.md` §7.41) and the checker's semantic tables — signatures,
//! visibility, the call graph, the user-type table. **Not** the syntax tree, which this crate
//! cannot see, and not the resolution's position maps: which slot a name is, which function a
//! call is, which field a projection reads, what a `?` means and which loop a `break` leaves
//! all arrive in Core IR, decided once by the one lowering in `nazm-core`. Until N39 this file
//! walked the tree and asked the resolution each of those questions again, span by span —
//! the same questions the interpreter asked as it ran. `cargo xtask check` now refuses a
//! position lookup here.
//!
//! What is decided here, and only here, is what Core IR deliberately leaves out: layout —
//! where a field sits, what a variant's discriminant is — linkage and symbols, which
//! temporaries need a slot so a failure part way through a construction can release what it
//! built, and which instance of a generic function a call reaches.
//!
//! # Refusal, not partial compilation
//!
//! Lowering runs *after* checking has passed and Core IR has been verified, so a program
//! reaching here is well-typed. That is exactly why the refusals below are about the
//! backend's coverage and not about the program being wrong: recursion is a perfectly good
//! Nazm construct that this backend cannot compile, and saying so is different from saying
//! it is invalid. Every refusal points at `nazm run`, which still executes all of it.
//!
//! Every unsupported construct produces a diagnostic and **no output**. A compiler that
//! emits something for a program it did not fully understand is worse than one that
//! stops, because the something runs.
//!
//! # The one `if`, in two positions
//!
//! Core IR has already said which `if` is a statement and which is a value, and which region
//! wants a value: a function body, a value `if` branch or `match` arm, and a block used as a
//! value do; a loop body, a statement branch or arm, a `scope` body and a block written as a
//! statement do not. [`lower_block`] and [`lower_body`] are those two kinds, over a shared
//! [`lower_stmts`] that has no opinion about either. What differs between the positions is
//! completion, and that is represented rather than implied: [`Ends`] records it for
//! statement sequences, and `Option` on [`Block::result`] and on `Expr::If`'s type for values.
//!
//! # Where `break` and `continue` may appear
//!
//! Core IR names the loop each one targets; LIR does not — the emitter acts on the innermost
//! loop whose **body** encloses the transfer. The two agree because a `break` in a loop's
//! *condition* names the enclosing loop, which is the innermost body around it; a target
//! that is not the innermost is refused as a compiler bug rather than emitted against the
//! wrong loop ([`loop_target`]).

use nazm_cir as cir;
use nazm_cir::Ends;
use nazm_diag::{codes, Diagnostic};
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_span::Span;

use nazm_sema::key::{DefKey, DefKind};
use nazm_sema::module::{ModuleGraph, ModuleId};
use nazm_sema::types::{ParamOwner, Type};
use nazm_sema::udt::TypeNode;

use crate::instance::{plan_instances, InstanceKey, Instances};
use crate::ir::{
    fits, ArithOp, Block, Builtin, CmpOp, EnumLayout, EnumTy, Expr, External, Function, Local,
    Lowering as _, MatchArm, Native, RecordLayout, RecordTy, StatementArm, Stmt, Ty, Unit,
    VariantLayout, VecLayout, VecTy,
};
use crate::symbol::{encoded, type_key, Linkage, NativeSymbol, NativeTypeName};

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
    /// Whether this is a generic function instance's, which is always defined in a unit
    /// of its own and so is external to every unit that calls it — including the unit of
    /// the module its generic definition belongs to.
    instance: bool,
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
    /// By [`EnumTy`], how that enum is laid out and which discriminant each variant has.
    /// Global and computed once, for the reason the records are.
    enums: Vec<EnumLayout>,
    /// By [`VecTy`], each concrete `Vec`'s element type and helper name. N11.
    vecs: Vec<VecLayout>,
    /// Every generic function instance the program needs, and, in the same order, the
    /// signature this backend gives each.
    instances: Instances,
    instance_sigs: Vec<Sig>,
    /// `main`'s symbol, and how many root capabilities it takes.
    entry: (NativeSymbol, usize),
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
    /// The enum types this unit's text mentions, by the same rule.
    used_enums: std::collections::BTreeSet<u32>,
    /// The `Vec` types this unit's text mentions, by the same rule.
    used_vecs: std::collections::BTreeSet<u32>,
    /// The substitution a generic instance's body is lowered under: its definition's
    /// parameters, and the concrete arguments that replace them. `None` for an ordinary
    /// function, whose types are concrete already.
    subst: Option<(ParamOwner, Vec<Type>)>,
    /// Whether this unit is a generic instance's. Such a unit defines exactly one function
    /// and **owns no module's definitions**, so everything it calls is external to it.
    instance_unit: bool,
    /// Locals of the function being lowered, in slot order: Core IR's resolved locals, then
    /// the hidden slots made here.
    locals: Vec<Local>,
    /// How many of [`Self::locals`] are Core IR's resolved locals — the checker's slots.
    resolved: usize,
    /// For each Core IR temporary, the hidden slot it became, once the body mentions it.
    temps: Vec<Option<usize>>,
    /// The Core IR function whose body is being lowered: its constants and locals.
    function: Option<&'a cir::Function>,
    /// The declared return type of the function being lowered, for checking `return`.
    /// Reset per function, like everything else about a function's body.
    ret_ty: Ty,
    /// The loops whose **bodies** enclose the statement being lowered, innermost last: the
    /// emitter's target stack, which a Core IR transfer's named target must agree with.
    /// Reset per function, because a loop target cannot cross a function boundary.
    loops: Vec<cir::LoopId>,
    errors: Vec<Diagnostic>,
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
/// The native name of a record or enum table entry.
///
/// Derived from the durable identity where there is one, exactly as a function symbol is,
/// and from load order where there is not — for a module that has no durable identity,
/// which still compiles. An applied entry adds its arguments' canonical keys, so two units
/// that name `Box[Int]` name it identically and `Box[Int]` and `Box[Str]` never share a
/// name, whatever their layouts happen to be.
fn native_type_name(res: &Resolution, graph: &ModuleGraph, node: TypeNode) -> NativeTypeName {
    let definition = res.types().definition_of(node.ty()).unwrap_or(node);
    let (kind, module, name) = match definition {
        TypeNode::Record(id) => (
            DefKind::Record,
            res.records().get(id).map(|d| d.module),
            res.records().get(id).map(|d| d.name.clone()),
        ),
        TypeNode::Enum(id) => (
            DefKind::Enum,
            res.enums().get(id).map(|d| d.module),
            res.enums().get(id).map(|d| d.name.clone()),
        ),
    };
    let (Some(module), Some(name)) = (module, name) else {
        return NativeTypeName::of_unkeyed(ModuleId::ROOT, "?");
    };
    let base = match graph.key(module) {
        Some(key) => NativeTypeName::of_definition(&DefKey::new(key.clone(), kind, name)),
        None => NativeTypeName::of_unkeyed(module, &name),
    };
    let args = res.types().type_args(node.ty());
    if args.is_empty() {
        base
    } else {
        let keys: Vec<String> = args
            .iter()
            .map(|a| type_key(*a, res.types(), graph))
            .collect();
        NativeTypeName::of_instance(&base, &keys)
    }
}

fn record_layouts(res: &Resolution, graph: &ModuleGraph) -> Vec<RecordLayout> {
    res.records()
        .all()
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = RecordTy(u32::try_from(i).unwrap_or(u32::MAX));
            let node = TypeNode::Record(nazm_sema::RecordId::from_index(i));
            let name = native_type_name(res, graph, node);
            // A generic definition as written, or an application still mentioning a
            // parameter, has no layout: no native code can hold a value of one. Its entry
            // stays in the table so that ids index it, and nothing names it.
            let concrete = def.params.is_empty() && res.types().is_concrete(node.ty());
            let fields: Vec<Ty> = if concrete {
                def.in_layout_order().map(|(_, f)| Ty::of(f.ty)).collect()
            } else {
                Vec::new()
            };
            let mut of_declaration = vec![0_u32; def.fields.len()];
            for (position, (declared, _)) in def.in_layout_order().enumerate() {
                if let Some(slot) = of_declaration.get_mut(declared as usize) {
                    *slot = u32::try_from(position).unwrap_or(u32::MAX);
                }
            }
            RecordLayout {
                id,
                name,
                source: res.types().show(node.ty()),
                fields,
                of_declaration,
            }
        })
        .collect()
}

/// How every enum in this compilation is laid out, and what discriminant each variant has.
///
/// One table, computed before any unit is lowered, for the reason the record table is:
/// two objects that pass an enum between them must agree about its tags and its payload
/// positions, and the only way to be sure is that neither decided.
///
/// The variant order is **canonical by variant name** and the payload order canonical by
/// field name. Nothing about the author's declaration order reaches the object file, so
/// moving two variant declarations — an edit the language says means nothing — changes no
/// byte of it.
fn enum_layouts(res: &Resolution, graph: &ModuleGraph) -> Vec<EnumLayout> {
    res.enums()
        .all()
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = EnumTy(u32::try_from(i).unwrap_or(u32::MAX));
            let node = TypeNode::Enum(nazm_sema::EnumId::from_index(i));
            let name = native_type_name(res, graph, node);
            let concrete = def.params.is_empty() && res.types().is_concrete(node.ty());
            let mut variants: Vec<VariantLayout> = Vec::new();
            let mut of_declaration = vec![0_u32; def.variants.len()];
            for (tag, (declared, v)) in def.in_tag_order().enumerate() {
                if let Some(slot) = of_declaration.get_mut(declared as usize) {
                    *slot = u32::try_from(tag).unwrap_or(u32::MAX);
                }
                let fields: Vec<Ty> = if concrete {
                    v.in_layout_order().map(|(_, f)| Ty::of(f.ty)).collect()
                } else {
                    Vec::new()
                };
                let mut fields_of_declaration = vec![0_u32; v.fields.len()];
                for (position, (field_declared, _)) in v.in_layout_order().enumerate() {
                    if let Some(slot) = fields_of_declaration.get_mut(field_declared as usize) {
                        *slot = u32::try_from(position).unwrap_or(u32::MAX);
                    }
                }
                variants.push(VariantLayout {
                    source: v.name.clone(),
                    name: name.member(&v.name),
                    fields,
                    of_declaration: fields_of_declaration,
                });
            }
            EnumLayout {
                id,
                name,
                source: res.types().show(node.ty()),
                variants,
                of_declaration,
            }
        })
        .collect()
}

/// Every concrete `Vec` of this compilation: its element, and the name its release helper
/// is derived from — the element's canonical key, so the name is a function of the type.
fn vec_layouts(res: &Resolution, graph: &ModuleGraph) -> Vec<VecLayout> {
    (0..res.types().vec_count())
        .map(|i| {
            let id = VecTy(u32::try_from(i).unwrap_or(u32::MAX));
            let v = nazm_sema::VecId::from_index(i);
            let elem = res.types().vec_element(v).unwrap_or(Type::Int);
            VecLayout {
                id,
                name: encoded(&type_key(elem, res.types(), graph)),
                elem: Ty::concrete(elem).unwrap_or(Ty::Int),
                source: res.types().show(Type::Vec(v)),
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
        match ty {
            Ty::Record(id) => {
                self.used_records.insert(id.0);
            }
            Ty::Enum(id) => {
                self.used_enums.insert(id.0);
            }
            Ty::Vec(id) => {
                self.used_vecs.insert(id.0);
            }
            _ => {}
        }
    }

    /// A front-end type as this backend represents it, **under the substitution of the
    /// instance being lowered**.
    ///
    /// The one place a generic body's types become concrete. By lookup, never by creation:
    /// the instance planner interned every type this body mentions under this substitution
    /// before any code was written, so a type missing here is a type the planner missed —
    /// reported as the compiler bug it is rather than guessed at.
    fn ty(&mut self, t: Type, at: Span) -> Option<Ty> {
        let concrete = match &self.subst {
            Some((owner, args)) => self.res.types().substituted(t, *owner, args),
            None => Some(t),
        };
        match concrete.filter(|c| self.res.types().is_concrete(*c)) {
            Some(c) => Ty::concrete(c),
            None => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNSUPPORTED_CONSTRUCT,
                        at,
                        format!(
                            "`nazm build` reached a generic type, `{}`, with no concrete \
                             instance for it",
                            self.res.types().show(t)
                        ),
                    )
                    .help(
                        "this is a bug in the compiler's instance planning; `nazm run` still works",
                    ),
                );
                None
            }
        }
    }

    /// A front-end type under the current substitution, as a front-end type.
    fn concrete(&self, t: Type) -> Option<Type> {
        match &self.subst {
            Some((owner, args)) => self.res.types().substituted(t, *owner, args),
            None => Some(t),
        }
    }

    /// The layout of a record this compilation declared.
    fn layout(&self, id: RecordTy) -> Option<&RecordLayout> {
        self.plan.records.get(id.index())
    }

    /// The layout of an enum this compilation declared.
    fn enum_layout(&self, id: EnumTy) -> Option<&EnumLayout> {
        self.plan.enums.get(id.index())
    }

    /// The discriminant of the variant declared at `declared`.
    ///
    /// **The layout boundary**, the enum's half. Everything above holds a declaration
    /// index — which is how a variant's semantic identity is spelled in one session — and
    /// everything below holds a number. This function is the only place the two meet.
    fn tag(&self, id: EnumTy, declared: u32) -> Option<u32> {
        self.enum_layout(id)?
            .of_declaration
            .get(declared as usize)
            .copied()
    }

    /// Where the payload field declared at `declared` sits in the variant's struct.
    fn payload_index(&self, id: EnumTy, tag: u32, declared: u32) -> Option<u32> {
        self.enum_layout(id)?
            .variants
            .get(tag as usize)?
            .of_declaration
            .get(declared as usize)
            .copied()
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

    /// Which slot a Core IR local is, and its type.
    ///
    /// A resolved local is its own slot: the checker numbered them, parameters first, and
    /// Core IR kept the numbering. A temporary becomes a hidden slot the first time the body
    /// mentions it — named for what it holds, so `--emit-ir` shows `ok#7` where a `?`'s
    /// success value lives.
    fn slot_of(&mut self, l: cir::LocalId) -> Option<(usize, Ty)> {
        if l.index() < self.resolved {
            return self.locals.get(l.index()).map(|x| (l.index(), x.ty));
        }
        let at = l.index() - self.resolved;
        if let Some(Some(slot)) = self.temps.get(at) {
            let slot = *slot;
            return self.locals.get(slot).map(|x| (slot, x.ty));
        }
        let local = self.function.and_then(|f| f.local(l))?;
        let (name, t) = (local.name.clone(), local.ty);
        let ty = self.ty(t, local.at)?;
        let slot = self.hidden(ty, &name);
        if let Some(entry) = self.temps.get_mut(at) {
            *entry = Some(slot);
        }
        Some((slot, ty))
    }

    /// A Core IR local's display name, for a diagnostic.
    fn local_name(&self, l: cir::LocalId) -> String {
        self.function
            .and_then(|f| f.local(l))
            .map_or_else(|| format!("local {}", l.0), |x| x.name.clone())
    }

    /// A function's display name, for a diagnostic. Never resolved against.
    fn function_name(&self, id: FnId) -> String {
        self.res
            .function(id)
            .map_or_else(|| "?".to_owned(), |d| d.name.clone())
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
        // An instance's unit defines only the instance, so even a function of the module
        // the generic definition belongs to is somebody else's definition here.
        if sig.module == self.module && !self.instance_unit && !sig.instance {
            return;
        }
        // A declaration names its parameter and result types, so their definitions travel
        // with this unit exactly as a local's would. Until the N11 conformance corpus a
        // call could only mention a type the caller's own code had also mentioned; an
        // instance taking `Maybe[Str]`, called with a value the caller never bound, was
        // declared with a type this unit did not carry and clang read `<enum>`.
        for t in sig.params.iter().chain(std::iter::once(&sig.ret)) {
            self.note(*t);
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
pub fn plan(
    core: &cir::Program,
    res: &Resolution,
    graph: &ModuleGraph,
    instances: Instances,
) -> Result<Plan, LowerError> {
    let mut errors = Vec::new();
    let mut sig: Vec<Option<Sig>> = vec![None; res.functions().len()];

    // A private function a generic body calls is reached from that body's **instances**,
    // which are artefacts of their own — so it must be bindable across objects. Decided by
    // its own module's generic bodies, never by who instantiates them: a module's object
    // must not depend on its importers.
    let mut from_generic: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for f in res.functions() {
        if f.type_params.is_empty() {
            continue;
        }
        for site in &f.call_sites {
            if let Callee::Function(g) = site.callee {
                from_generic.insert(g.index());
            }
        }
    }

    // Driven by the **resolution**, not by the syntax tree: each definition says which
    // declaration it came from, so pairing the two needs no name. A name stopped being
    // able to do the job when two modules became able to declare the same one.
    for (i, def) in res.functions().iter().enumerate() {
        let id = FnId::from_index(i);
        // A generic definition has no one signature; its instances each have theirs.
        if !def.type_params.is_empty() {
            continue;
        }
        let Some(f) = core.function(id) else {
            continue;
        };
        if let Some(mut accepted) = signature(res, graph, id, f, &mut errors) {
            if from_generic.contains(&i) {
                accepted.linkage = Linkage::Hidden;
            }
            if let Some(slot) = sig.get_mut(id.index()) {
                *slot = Some(accepted);
            }
        }
    }

    let records = record_layouts(res, graph);
    let enums = enum_layouts(res, graph);
    let vecs = vec_layouts(res, graph);
    let instance_sigs: Vec<Sig> = instances
        .planned
        .iter()
        .map(|p| Sig {
            symbol: p.symbol.clone(),
            instance: true,
            module: res.function(p.key.def).map_or(ModuleId::ROOT, |d| d.module),
            // One definition, in one artefact of its own, called from any unit that
            // needs it — so bindable across objects, and exported from none.
            linkage: Linkage::Hidden,
            params: p.params.iter().map(|t| Ty::of(*t)).collect(),
            ret: Ty::of(p.ret),
        })
        .collect();

    let entry = match res
        .entry()
        .and_then(|id| sig.get(id.index()))
        .and_then(Option::as_ref)
    {
        Some(s) => (s.symbol.clone(), s.params.len()),
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
            enums,
            vecs,
            instances,
            instance_sigs,
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
    pub errors: Vec<Diagnostic>,
}

/// A fresh context for one unit.
fn context<'a>(res: &'a Resolution, plan: &'a Plan, module: ModuleId) -> Ctx<'a> {
    Ctx {
        res,
        plan,
        module,
        externals: Vec::new(),
        used_records: std::collections::BTreeSet::new(),
        used_enums: std::collections::BTreeSet::new(),
        used_vecs: std::collections::BTreeSet::new(),
        subst: None,
        instance_unit: false,
        locals: Vec::new(),
        resolved: 0,
        temps: Vec::new(),
        function: None,
        ret_ty: Ty::Int,
        loops: Vec::new(),
        errors: Vec::new(),
    }
}

/// The unit a context has collected, closed over the types its definitions mention.
fn finish(ctx: Ctx<'_>, module: ModuleId, source: &str, functions: Vec<Function>) -> Lowered {
    let mut ctx = ctx;
    // Sorted, so the declarations a unit carries do not depend on the order its bodies
    // happened to mention them. Two builds of one module produce one text.
    ctx.externals.sort();

    // Closed over field, payload and element types: a named LLVM type whose definition
    // refers to another must have that one defined beside it, and a `Vec`'s release helper
    // releases its elements by their own law. One walk over every kind, because each may
    // contain each. Iterative, and bounded by the number of definitions.
    let plan = ctx.plan;
    let mut work: Vec<Ty> = ctx
        .used_records
        .iter()
        .map(|id| Ty::Record(RecordTy(*id)))
        .chain(ctx.used_enums.iter().map(|id| Ty::Enum(EnumTy(*id))))
        .chain(ctx.used_vecs.iter().map(|id| Ty::Vec(VecTy(*id))))
        .collect();
    while let Some(ty) = work.pop() {
        let inner: Vec<Ty> = match ty {
            Ty::Record(id) => plan
                .records
                .get(id.index())
                .map(|l| l.fields.clone())
                .unwrap_or_default(),
            Ty::Enum(id) => plan
                .enums
                .get(id.index())
                .map(|l| l.variants.iter().flat_map(|v| v.fields.clone()).collect())
                .unwrap_or_default(),
            Ty::Vec(id) => plan
                .vecs
                .get(id.index())
                .map(|l| vec![l.elem])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for t in inner {
            match t {
                Ty::Record(r) if ctx.used_records.insert(r.0) => work.push(t),
                Ty::Enum(e) if ctx.used_enums.insert(e.0) => work.push(t),
                Ty::Vec(v) if ctx.used_vecs.insert(v.0) => work.push(t),
                _ => {}
            }
        }
    }
    let records: Vec<RecordLayout> = ctx
        .used_records
        .iter()
        .filter_map(|id| plan.records.get(*id as usize).cloned())
        .collect();
    let enums: Vec<EnumLayout> = ctx
        .used_enums
        .iter()
        .filter_map(|id| plan.enums.get(*id as usize).cloned())
        .collect();
    let vecs: Vec<VecLayout> = ctx
        .used_vecs
        .iter()
        .filter_map(|id| plan.vecs.get(*id as usize).cloned())
        .collect();

    Lowered {
        unit: Unit {
            module,
            source: source.to_owned(),
            functions,
            externals: ctx.externals,
            records,
            enums,
            vecs,
        },
        errors: ctx.errors,
    }
}

/// Lower one module's bodies, and nothing else.
///
/// # What it may see
///
/// Its own functions' bodies, and every function's *signature* through [`Plan`]. It never
/// reads another module's body, and there is nothing here through which it could: a call
/// into another module becomes an [`External`] — a symbol and a signature — which is
/// exactly what a linker needs and exactly what a dependency's implementation is not.
///
/// Its generic definitions are **not** here. They have no one signature and no one body
/// as native code; each instance is lowered into a unit of its own by [`lower_instance`].
pub fn lower_module(
    core: &cir::Program,
    res: &Resolution,
    plan: &Plan,
    module: ModuleId,
    source: &str,
) -> Lowered {
    let mut ctx = context(res, plan, module);
    let mut functions = Vec::new();
    for f in core.of_module(module) {
        if f.type_params > 0 || ctx.sig(f.def).is_none() {
            continue;
        }
        if let Some(lowered) = lower_function(&mut ctx, f) {
            functions.push(lowered);
        }
    }
    finish(ctx, module, source, functions)
}

/// Lower one generic function instance into a unit of its own.
///
/// The generic definition's **Core IR body**, lowered under the instance's substitution:
/// nothing is re-parsed, re-resolved or re-checked, and no source is copied. Every name in
/// the body means what the checker said it meant; only the types it was checked with are
/// replaced, and they are replaced by lookup ([`Ctx::ty`]). Spans are the definition's own,
/// so a failure inside an instance names the line of the generic function that failed.
pub fn lower_instance(
    core: &cir::Program,
    res: &Resolution,
    plan: &Plan,
    at: usize,
    source: &str,
) -> Option<Lowered> {
    let planned = plan.instances.planned.get(at)?;
    let f = core.function(planned.key.def)?;
    let mut ctx = context(res, plan, f.module);
    ctx.subst = Some((
        ParamOwner::Function(planned.key.def),
        planned.key.args.clone(),
    ));
    ctx.instance_unit = true;
    let sig = plan.instance_sigs.get(at)?.clone();
    let lowered = lower_body_as(&mut ctx, f, &sig);
    let functions = lowered.into_iter().collect();
    Some(finish(ctx, f.module, source, functions))
}

/// Lower a checked program into one native unit per module, and one per generic function
/// instance, or explain what stopped it.
///
/// # The pipeline
///
/// ```text
/// Core IR + resolution → instance plan → plan (global) → lower each module
///                                                      → lower each instance
///                      → recursion check (global)
/// ```
///
/// The resolution is cloned: instance planning interns the concrete types each instance
/// needs, and that is a native build's business, not the checker's answer.
///
/// # Errors
///
/// *Every* construct outside the subset, not just the first: one run of `nazm build`
/// should say everything that would have to change, which is the same
/// one-mistake-one-error discipline the rest of the front end holds to.
pub fn lower(
    core: &cir::Program,
    res: &Resolution,
    graph: &ModuleGraph,
    names: &crate::names::SourceNames<'_>,
) -> Result<Native, LowerError> {
    let mut res = res.clone();
    let instances =
        plan_instances(&mut res, graph).map_err(|diagnostics| LowerError { diagnostics })?;
    let res = res;
    let settled = plan(core, &res, graph, instances)?;

    let mut units = Vec::new();
    let mut errors = Vec::new();
    for module in graph.ids() {
        // A unit holds a module's native code, and a module that declares no function has
        // none: its types are laid out, and their helpers emitted, in the units that use
        // them. So it has no unit and no object — the core prelude, which declares two
        // enums, is the case that made this a rule rather than an empty file per build.
        if !res.functions().iter().any(|d| d.module == module) {
            continue;
        }
        // The module's durable identity where it has one, which is what makes the
        // artefact the same bytes from any checkout. `crate::names` owns the rule.
        let source = graph.file(module).map_or("", |f| names.name(f));
        let mut lowered = lower_module(core, &res, &settled, module, source);
        units.push(lowered.unit);
        errors.append(&mut lowered.errors);
    }
    let mut instances = Vec::new();
    for at in 0..settled.instances.planned.len() {
        let module = settled
            .instances
            .planned
            .get(at)
            .and_then(|p| res.function(p.key.def))
            .map(|d| d.module);
        let source = module
            .and_then(|m| graph.file(m))
            .map_or("", |f| names.name(f));
        if let Some(mut lowered) = lower_instance(core, &res, &settled, at, source) {
            instances.push(lowered.unit);
            errors.append(&mut lowered.errors);
        }
    }

    // The call graph is whole-program, so the cycle check is too — and it is over
    // **definitions**, from the calls the checker recorded. An instance-level cycle is a
    // definition-level one, and a definition-level cycle through a generic function makes
    // every instance of it reach another, so the two refuse the same programs; the
    // definition graph also covers a generic function nothing instantiates.
    report_cycles(&res, &mut errors);

    if errors.is_empty() {
        Ok(Native {
            units,
            instances,
            entry: settled.entry.0,
            entry_roots: settled.entry.1,
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
    f: &cir::Function,
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
        // A capability parameter is the root of authority, which the entry wrapper hands
        // over (N37); any other is a value nothing could supply, and the checker has
        // already refused it.
        if !def.params.iter().all(|t| t.is_capability()) {
            errors.push(unsupported_at(f.span, "a `main` with parameters"));
            ok = false;
        }
        if ret != Ty::Int {
            errors.push(unsupported_at(
                f.ret_span,
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
        instance: false,
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
/// The slots, their order and their types are Core IR's locals — the checker's slots,
/// **parameters first** — and nothing here can disagree with the checker about which binding a
/// name means. Core IR's own temporaries (a `?`'s two bindings) become hidden slots where the
/// body first mentions them, which is where the lowering that preceded Core IR made them.
fn lower_function<'a>(ctx: &mut Ctx<'a>, f: &'a cir::Function) -> Option<Function> {
    let sig = ctx.sig(f.def)?.clone();
    lower_body_as(ctx, f, &sig)
}

/// Lower a body under a signature: an ordinary function's own, or an instance's.
fn lower_body_as<'a>(ctx: &mut Ctx<'a>, f: &'a cir::Function, sig: &Sig) -> Option<Function> {
    let Sig {
        symbol,
        linkage,
        ret,
        ..
    } = sig.clone();
    let resolved: Vec<(String, Type)> = f
        .locals
        .iter()
        .take_while(|l| !matches!(l.origin, cir::LocalOrigin::Temp(_)))
        .map(|l| (l.name.clone(), l.ty))
        .collect();
    let mut locals = Vec::with_capacity(f.locals.len());
    for (name, t) in resolved {
        let ty = ctx.ty(t, f.name_span)?;
        locals.push(Local { name, ty });
    }
    ctx.resolved = locals.len();
    ctx.temps = vec![None; f.locals.len().saturating_sub(ctx.resolved)];
    ctx.locals = locals;
    // Every local's type is mentioned by this unit's text: it is the type of an `alloca`.
    let local_types: Vec<Ty> = ctx.locals.iter().map(|l| l.ty).collect();
    for t in local_types {
        ctx.note(t);
    }
    ctx.note(ret);
    ctx.ret_ty = ret;
    // No loop target crosses a function boundary.
    ctx.loops.clear();

    let previous = ctx.function.replace(f);
    let block = lower_block(ctx, &f.body);
    ctx.function = previous;
    let block = block?;
    // No tail is legitimate: a body every path of which returns has nothing to fall off the
    // end with, and checking accepts exactly that. Emission writes no terminator for such a
    // body, because the `ret` is already there.
    let Some(result) = block.result.as_ref() else {
        return Some(Function {
            symbol,
            linkage,
            name: f.name.clone(),
            locals: std::mem::take(&mut ctx.locals),
            params: f.params as usize,
            ret,
            body: block,
        });
    };
    if !fits(result, ret) {
        // Unreachable after checking, which requires the body to produce the declared
        // type, and after Core IR's own verification. Reported rather than assumed away.
        ctx.unsupported(
            f.body.at,
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
        params: f.params as usize,
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
fn report_cycles(res: &Resolution, errors: &mut Vec<Diagnostic>) {
    let n = res.functions().len();
    let mut edges: Vec<Vec<(usize, Span)>> = vec![Vec::new(); n];
    for (from, f) in res.functions().iter().enumerate() {
        for site in &f.call_sites {
            if let Callee::Function(to) = site.callee {
                if let Some(e) = edges.get_mut(from) {
                    e.push((to.index(), site.span));
                }
            }
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

/// A value region: statements, then a tail — none where the statements transfer.
fn lower_block(ctx: &mut Ctx<'_>, b: &cir::Block) -> Option<Block> {
    let (stmts, ends) = lower_stmts(ctx, &b.stmts);

    if ends == Ends::Transfers {
        // Every path through the statements leaves, so the block has no value, and Core IR
        // lowered no tail for it.
        return Some(Block {
            stmts,
            result: None,
        });
    }

    let Some(tail) = &b.tail else {
        ctx.unsupported(b.at, "a block with no final result expression");
        return None;
    };

    let result = lower_expr(ctx, tail)?;
    Some(Block {
        stmts,
        result: Some(Box::new(result)),
    })
}

/// A statement region: a loop body, a statement `if` branch or `match` arm, a `scope` body, a
/// block written as a statement. Its statements, in place.
fn lower_body(ctx: &mut Ctx<'_>, b: &cir::Block) -> (Vec<Stmt>, Ends) {
    lower_stmts(ctx, &b.stmts)
}

/// The statements every kind of region shares.
///
/// A statement that cannot be lowered records a diagnostic and contributes nothing, which
/// is safe because [`lower`] returns `Err` whenever any diagnostic was recorded — no
/// module is ever built out of a partially lowered body.
fn lower_stmts(ctx: &mut Ctx<'_>, statements: &[cir::Stmt]) -> (Vec<Stmt>, Ends) {
    let mut stmts = Vec::new();
    for s in statements {
        match s {
            // `let` and `let mut` lower identically. Mutability is a *checking* property:
            // it decides whether an assignment to this binding is legal, and the checker
            // has already decided that (`N0310`).
            cir::Stmt::Let { local, value } => {
                let value = lower_expr(ctx, value);
                let Some((slot, _)) = ctx.slot_of(*local) else {
                    ctx.errors.push(
                        Diagnostic::error(
                            codes::UNDEFINED_VARIABLE,
                            Span::synthetic(),
                            format!("local {} has no slot", local.0),
                        )
                        .help("this is a compiler bug: Core IR names a local it does not have"),
                    );
                    continue;
                };
                if let Some(value) = value {
                    stmts.push(Stmt::Let { slot, value });
                }
            }
            cir::Stmt::Assign {
                local,
                path,
                value,
                at,
            } => stmts.extend(lower_assign(ctx, *local, path, value, *at)),
            cir::Stmt::Loop { id, cond, body, .. } => {
                stmts.extend(lower_while(ctx, *id, cond, body));
            }
            cir::Stmt::Scope { body, at } => {
                // Its own scope in the language too: a binding made inside `scope { … }`
                // does not outlive it, exactly as in any other block.
                let (body, ends) = lower_body(ctx, body);
                stmts.push(Stmt::Scope { body, span: *at });
                // A `scope` whose body always transfers transfers too — after the join,
                // which emission is what arranges. Nothing after it can run.
                if ends == Ends::Transfers {
                    return (stmts, Ends::Transfers);
                }
            }
            cir::Stmt::Spawn {
                callee,
                args,
                at,
                callee_at,
            } => {
                if let Some(stmt) = lower_spawn(ctx, *callee, args, *at, *callee_at) {
                    stmts.push(stmt);
                }
            }
            cir::Stmt::Return { value, at } => {
                if let Some(stmt) = lower_return(ctx, value, *at) {
                    stmts.push(stmt);
                }
                // A `return` leaves the function whether or not its operand lowered, so
                // the rest of the block is unreachable either way.
                return (stmts, Ends::Transfers);
            }
            cir::Stmt::Break { target, at } => {
                if loop_target(ctx, "break", *target, *at) {
                    stmts.push(Stmt::Break);
                    return (stmts, Ends::Transfers);
                }
            }
            cir::Stmt::Continue { target, at } => {
                if loop_target(ctx, "continue", *target, *at) {
                    stmts.push(Stmt::Continue);
                    return (stmts, Ends::Transfers);
                }
            }
            // Evaluated and its value dropped — what the language means by `e;`.
            cir::Stmt::Eval(e) => stmts.extend(lower_expr(ctx, e).map(Stmt::Discard)),
            cir::Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                let (stmt, ends) = lower_statement_if(ctx, cond, then, otherwise.as_ref());
                stmts.extend(stmt);
                if ends == Ends::Transfers {
                    return (stmts, ends);
                }
            }
            cir::Stmt::Match(m) => {
                let (stmt, ends) = lower_statement_match(ctx, m);
                stmts.extend(stmt);
                if ends == Ends::Transfers {
                    return (stmts, ends);
                }
            }
            // A block written as a statement is its statements, in place: each binding in
            // it already has a slot of its own, so no scope has to be marked.
            cir::Stmt::Block(b) => {
                let (inner, ends) = lower_body(ctx, b);
                stmts.extend(inner);
                if ends == Ends::Transfers {
                    return (stmts, ends);
                }
            }
        }
    }
    (stmts, Ends::FallsThrough)
}

/// `return e;` — leave the function with `e`. No loop target is consulted: a `return`
/// inside a loop leaves the *function*.
fn lower_return(ctx: &mut Ctx<'_>, value: &cir::Expr, at: Span) -> Option<Stmt> {
    let value = lower_expr(ctx, value)?;
    if !fits(&value, ctx.ret_ty) {
        // Unreachable after checking, which requires a `return` to match the signature
        // (`N0300`). Reported rather than assumed away: a `ret` of the wrong type is a
        // module clang rejects for a reason that names none of this.
        ctx.unsupported(
            at,
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

/// Whether a `break` or `continue` names the loop this backend would leave or restart.
///
/// LIR spells a transfer without its target: the emitter's target stack is the loops whose
/// bodies enclose it, innermost last, and a transfer acts on the innermost. Core IR names the
/// target, so the two must agree — a `break` in a loop's *condition* names the enclosing loop,
/// which is exactly the innermost body around it. A target that is not the innermost would be
/// emitted against the wrong loop, so it is refused as the compiler bug it is.
fn loop_target(ctx: &mut Ctx<'_>, word: &str, target: cir::LoopId, span: Span) -> bool {
    match ctx.loops.last() {
        Some(innermost) if *innermost == target => true,
        Some(_) => {
            ctx.errors.push(
                Diagnostic::error(
                    codes::NOT_IN_A_LOOP,
                    span,
                    format!("`{word}` names a loop other than the innermost one around it"),
                )
                .help("this is a bug in the compiler; `nazm run` still works"),
            );
            false
        }
        None => {
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
    }
}

/// Statement `if`, with or without `else`. Without an `else` it always falls through; with
/// one it transfers only when both branches do.
fn lower_statement_if(
    ctx: &mut Ctx<'_>,
    cond: &cir::Expr,
    then_block: &cir::Block,
    else_block: Option<&cir::Block>,
) -> (Option<Stmt>, Ends) {
    // The condition is lowered outside both branch scopes: it is evaluated before either
    // and cannot see a binding made inside one.
    let span = cond.at;
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

/// `local.path = value;` — a store into the slot Core IR names, through the field path it
/// names, each step turned from a field identity into a position.
///
/// The value is lowered first and stored second, and the emitter keeps that order: a
/// right-hand side that fails at run time, or that transfers control, takes its own path
/// and the slot is never written.
fn lower_assign(
    ctx: &mut Ctx<'_>,
    local: cir::LocalId,
    path: &[cir::Step],
    value: &cir::Expr,
    at: Span,
) -> Option<Stmt> {
    let value = lower_expr(ctx, value);
    let name = ctx.local_name(local);

    let Some((slot, ty)) = ctx.slot_of(local) else {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                at,
                format!("`{name}` is not a local the native backend can see"),
            )
            .help("this is a bug in the compiler; `nazm run` still works"),
        );
        return None;
    };

    let mut steps: Vec<(u32, Ty)> = Vec::with_capacity(path.len());
    let mut target = ty;
    for step in path {
        let Ty::Record(rid) = target else {
            ctx.unsupported(at, "a field written on something that is not a record");
            return None;
        };
        let position = ctx.field_index(rid, step.field)?;
        let field_ty = ctx
            .layout(rid)
            .and_then(|l| l.fields.get(position as usize).copied())?;
        ctx.note(field_ty);
        steps.push((position, field_ty));
        target = field_ty;
    }

    let value = value?;
    if !fits(&value, target) {
        // Unreachable after checking, which requires an assignment to match the place's
        // declared type (`N0300`). A mismatched store is a miscompile.
        ctx.unsupported(
            at,
            &format!(
                "an assignment of `{}` to `{name}`, which is `{}`",
                value.ty().map_or("nothing", Ty::name),
                target.name()
            ),
        );
        return None;
    }

    if steps.is_empty() {
        Some(Stmt::Assign { slot, value })
    } else {
        Some(Stmt::SetField {
            slot,
            path: steps,
            value,
        })
    }
}

/// `while cond { body }`.
///
/// The condition is lowered **outside** the loop's own target: a `break` written in a
/// condition belongs to the *enclosing* loop, which Core IR has already said by naming it.
fn lower_while(
    ctx: &mut Ctx<'_>,
    id: cir::LoopId,
    cond: &cir::Expr,
    body: &cir::Block,
) -> Option<Stmt> {
    let span = cond.at;
    let cond = lower_expr(ctx, cond);

    ctx.loops.push(id);
    // Lowered whether or not the condition did, so a program with problems in both
    // reports both.
    let (body, _) = lower_body(ctx, body);
    ctx.loops.pop();

    let cond = cond?;
    if !fits(&cond, Ty::Bool) {
        // Unreachable after checking, which requires a `Bool` condition (`N0301`).
        ctx.unsupported(span, "a `while` whose condition is not a `Bool`");
        return None;
    }

    Some(Stmt::While { cond, body })
}

fn lower_expr(ctx: &mut Ctx<'_>, e: &cir::Expr) -> Option<Expr> {
    match &e.kind {
        cir::ExprKind::Const(c) => match ctx.function.and_then(|f| f.constant(*c)) {
            Some(cir::Const::Int(v)) => Some(Expr::Int(*v)),
            Some(cir::Const::Bool(v)) => Some(Expr::Bool(*v)),
            Some(cir::Const::Str(v)) => Some(Expr::Str(v.clone())),
            None => {
                ctx.unsupported(e.at, "a literal missing from its function's constants");
                None
            }
        },
        cir::ExprKind::Local(l) => match ctx.slot_of(*l) {
            Some((slot, ty)) => Some(Expr::Load { slot, ty }),
            None => {
                // Unreachable: Core IR's verifier holds every local in range.
                let name = ctx.local_name(*l);
                ctx.errors.push(
                    Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        e.at,
                        format!("`{name}` is not a local the native backend can see"),
                    )
                    .help("this is a bug in the compiler; `nazm run` still works"),
                );
                None
            }
        },
        cir::ExprKind::Neg(operand) => {
            let operand = lower_expr(ctx, operand)?;
            if !fits(&operand, Ty::Int) {
                ctx.unsupported(e.at, "negation of a non-`Int` value");
                return None;
            }
            Some(Expr::Neg {
                operand: Box::new(operand),
                span: e.at,
            })
        }
        cir::ExprKind::Arith { op, lhs, rhs } => lower_arith(ctx, *op, lhs, rhs, e.at),
        cir::ExprKind::Compare { op, lhs, rhs, .. } => lower_compare(ctx, *op, lhs, rhs, e.at),
        cir::ExprKind::Call {
            callee,
            type_args,
            args,
            callee_at,
        } => lower_call(ctx, *callee, type_args, args, *callee_at),
        cir::ExprKind::Builtin {
            op,
            type_args,
            args,
            callee_at,
        } => {
            // Arguments first, left to right, and all of them.
            let lowered: Vec<Option<Expr>> = args.iter().map(|a| lower_expr(ctx, a)).collect();
            // **Built-in or user function is read, not decided again**: Core IR says which,
            // and an instruction sequence is what a built-in becomes.
            if op.generics() > 0 {
                lower_vec(ctx, *op, lowered, type_args, *callee_at)
            } else {
                lower_builtin(ctx, *op, lowered, *callee_at)
            }
        }
        cir::ExprKind::Record { record, fields, .. } => lower_construct(ctx, *record, fields, e.at),
        cir::ExprKind::Variant {
            enumeration,
            variant,
            fields,
            ..
        } => lower_variant(ctx, *enumeration, *variant, fields, e.at),
        cir::ExprKind::Field { base, field, .. } => lower_field(ctx, base, *field, e.at),
        cir::ExprKind::Match(m) => lower_match(ctx, m),
        cir::ExprKind::If {
            cond,
            then,
            otherwise,
        } => lower_if(ctx, cond, then, otherwise, e.at),
        // The block every other block is, in a position that uses its value. Its bindings
        // already have slots of their own.
        cir::ExprKind::Block(b) => lower_block(ctx, b).map(|b| Expr::Block(Box::new(b))),
    }
}

/// `spawn f(args);` — a task running a function of this compilation, by its identity.
fn lower_spawn(
    ctx: &mut Ctx<'_>,
    fid: FnId,
    args: &[cir::Expr],
    span: Span,
    callee_span: Span,
) -> Option<Stmt> {
    // Every argument, even when the callee turns out to be unusable, so a spawn with two
    // bad arguments reports both.
    let lowered: Vec<Option<Expr>> = args.iter().map(|a| lower_expr(ctx, a)).collect();
    let name = ctx.function_name(fid);

    let Some(sig) = ctx.sig(fid).cloned() else {
        ctx.unsupported(
            callee_span,
            &format!("a task running `{name}`, whose signature this backend refused"),
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

    // A task is an edge in the call graph like any other — `fn f() { scope { spawn f(); } }`
    // is recursion — and the checker recorded it as one, which is what the cycle check
    // reads.

    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;
    if args.len() != params.len() || !args.iter().zip(&params).all(|(a, want)| fits(a, *want)) {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                callee_span,
                format!("`nazm build` lowered a task running `{name}` with the wrong arguments"),
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
            callee_span,
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
        span,
    })
}

fn lower_arith(
    ctx: &mut Ctx<'_>,
    op: cir::ArithOp,
    lhs: &cir::Expr,
    rhs: &cir::Expr,
    op_span: Span,
) -> Option<Expr> {
    // Left, then right, always, so a program with problems on both sides reports both.
    let lhs = lower_expr(ctx, lhs);
    let rhs = lower_expr(ctx, rhs);
    let (lhs, rhs) = (lhs?, rhs?);
    if !fits(&lhs, Ty::Int) || !fits(&rhs, Ty::Int) {
        // Unreachable after checking: arithmetic is `Int` only.
        ctx.unsupported(op_span, "arithmetic on a non-`Int` value");
        return None;
    }
    Some(Expr::Arith {
        op: match op {
            cir::ArithOp::Add => ArithOp::Add,
            cir::ArithOp::Sub => ArithOp::Sub,
            cir::ArithOp::Mul => ArithOp::Mul,
            cir::ArithOp::Div => ArithOp::Div,
            cir::ArithOp::Rem => ArithOp::Rem,
        },
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: op_span,
    })
}

fn lower_compare(
    ctx: &mut Ctx<'_>,
    op: cir::CmpOp,
    lhs: &cir::Expr,
    rhs: &cir::Expr,
    op_span: Span,
) -> Option<Expr> {
    let lhs = lower_expr(ctx, lhs);
    let rhs = lower_expr(ctx, rhs);
    let (lhs, rhs) = (lhs?, rhs?);
    let cmp = match op {
        cir::CmpOp::Eq => CmpOp::Eq,
        cir::CmpOp::NotEq => CmpOp::NotEq,
        cir::CmpOp::Lt => CmpOp::Lt,
        cir::CmpOp::LtEq => CmpOp::LtEq,
        cir::CmpOp::Gt => CmpOp::Gt,
        cir::CmpOp::GtEq => CmpOp::GtEq,
    };
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
    // left operand, so the recorded type is never read.
    let operand_ty = lhs.ty().or_else(|| rhs.ty()).unwrap_or(Ty::Int);
    if cmp.is_ordering() && !fits(&lhs, Ty::Int) {
        ctx.unsupported(op_span, &format!("ordering `{}` values", operand_ty.name()));
        return None;
    }
    // Unreachable after checking, which gives a handle no equality (`N0304`) — and Core IR
    // settled the law it compares by. Refused rather than emitted, because the only
    // instruction that fits a pointer is an `icmp` on it, which would answer "the same
    // storage".
    if matches!(operand_ty, Ty::Ints | Ty::Strs | Ty::Chan | Ty::Vec(_)) {
        ctx.unsupported(
            op_span,
            &format!("comparing `{}` values", operand_ty.name()),
        );
        return None;
    }
    Some(Expr::Compare {
        op: cmp,
        operand_ty,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

/// A call to a function of this compilation, by identity. A generic callee is one of its
/// **instances**: the type arguments Core IR recorded for this call, under the substitution
/// of the body being lowered, are the instance's key, and the plan has its symbol.
fn lower_call(
    ctx: &mut Ctx<'_>,
    fid: FnId,
    type_args: &[Type],
    args: &[cir::Expr],
    callee_span: Span,
) -> Option<Expr> {
    // Arguments first, left to right, and **all of them** even when the callee turns out
    // to be unusable — a program with problems in two arguments should report two.
    let lowered: Vec<Option<Expr>> = args.iter().map(|a| lower_expr(ctx, a)).collect();
    let callee = ctx.function_name(fid);

    let resolved = if is_generic(ctx.res, fid) {
        let concrete: Option<Vec<Type>> = type_args.iter().map(|t| ctx.concrete(*t)).collect();
        concrete.and_then(|args| {
            let key = InstanceKey { def: fid, args };
            ctx.plan
                .instances
                .planned
                .iter()
                .position(|p| p.key == key)
                .and_then(|i| ctx.plan.instance_sigs.get(i))
                .cloned()
        })
    } else {
        ctx.sig(fid).cloned()
    };
    let Some(sig) = resolved else {
        // Reaching here means the callee is a function whose signature *this backend*
        // refused — a different fact from any the checker reports.
        ctx.unsupported(
            callee_span,
            &format!("a call to `{callee}`, whose signature this backend refused"),
        );
        return None;
    };

    let Sig {
        symbol,
        ret,
        params,
        ..
    } = sig.clone();
    ctx.refer_to(&sig);

    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;

    // The checker has already established arity and argument types (`N0303`, `N0300`),
    // so this agreeing is the normal case. It is checked anyway because the one thing it
    // would catch is a *lowering* bug — an argument that acquired the wrong type on the
    // way here — and that is a miscompile rather than a diagnostic.
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

/// `Point(x: 1, y: 2)`. Which record and which field each initialiser fills are Core IR's;
/// what is decided here — and only here — is where each field sits.
fn lower_construct(
    ctx: &mut Ctx<'_>,
    written: nazm_sema::RecordId,
    fields: &[cir::Init],
    at: Span,
) -> Option<Expr> {
    // The record this construction builds, **as this instance sees it**: `Box[T]` in a
    // generic body is `Box[Int]` in the instance for `Int`. The field an initialiser names
    // is the same declaration index in every application.
    let concrete = ctx.concrete(Type::Record(written))?;
    let Type::Record(id) = concrete else {
        return None;
    };
    let ty = ctx.ty(concrete, at)?;
    ctx.note(ty);
    let Ty::Record(rid) = ty else { return None };

    // In the order **written**, which is the order side effects happen in. The layout
    // index rides along, so emission never has to know the field's name.
    let mut lowered = Vec::with_capacity(fields.len());
    for f in fields {
        let position = ctx.field_index(rid, f.field)?;
        let value = lower_expr(ctx, &f.value)?;
        lowered.push((position, value));
    }

    // A slot only where a field owns something. A record of `Int`s cannot leak a partial
    // construction, so it does not pay for the machinery that stops one.
    let slot = ctx
        .res
        .types()
        .needs_cleanup(nazm_sema::Type::Record(id))
        .then(|| ctx.hidden(ty, "record"));

    Some(Expr::Record {
        ty,
        fields: lowered,
        slot,
    })
}

/// `Enum.Variant(field: value, …)`. Which enum, which variant and which payload field each
/// initialiser fills are Core IR's; what is decided here is the discriminant and where each
/// field sits.
fn lower_variant(
    ctx: &mut Ctx<'_>,
    enumeration: nazm_sema::EnumId,
    variant: u32,
    fields: &[cir::Init],
    at: Span,
) -> Option<Expr> {
    let ty = ctx.ty(Type::Enum(enumeration), at)?;
    ctx.note(ty);
    let Ty::Enum(eid) = ty else { return None };
    let tag = ctx.tag(eid, variant)?;

    // In the order **written**, which is the order side effects happen in.
    let mut lowered = Vec::with_capacity(fields.len());
    for f in fields {
        let position = ctx.payload_index(eid, tag, f.field)?;
        let value = lower_expr(ctx, &f.value)?;
        lowered.push((position, value));
    }

    // A slot only where a payload field of **this variant** owns something: what a
    // construction can leak is what it has already built.
    let owns = ctx
        .enum_layout(eid)
        .and_then(|l| l.variants.get(tag as usize))
        .is_some_and(|v| v.fields.iter().any(|t| t.owns().is_some()));
    let slot = owns.then(|| ctx.hidden(ty, "variant"));

    Some(Expr::Variant {
        ty,
        tag,
        fields: lowered,
        slot,
    })
}

/// A value `match` — a written one, or the one a `?` is.
///
/// The scrutinee is lowered once into a hidden local this expression owns — which is what
/// `docs/spec.md`'s *the scrutinee is a value the match owns* means in the backend — and
/// each arm becomes a block whose head binds the payload fields it names, each an ordinary
/// [`Stmt::Let`] over an [`Expr::Payload`]: a binding *is* a local that takes a reference to
/// what it names, and `Stmt::Let` is the one path that already gets take-before-release
/// right when the same slot is reached again on the next turn of a loop.
///
/// A `?`'s error arm is an ordinary `return`, so every scope it leaves is joined and every
/// local and temporary released on the way out by the one path that does that for `return`.
/// There is no landing pad and no unwinding, because there is nothing to unwind.
fn lower_match(ctx: &mut Ctx<'_>, m: &cir::Match) -> Option<Expr> {
    let lowered = lower_expr(ctx, &m.scrutinee)?;
    let scrutinee_ty = lowered.ty()?;
    let Ty::Enum(eid) = scrutinee_ty else {
        // Unreachable after checking, which refuses a `match` on anything else (`N0342`).
        ctx.unsupported(m.at, "a `match` on something that is not an enum");
        return None;
    };
    ctx.note(scrutinee_ty);
    let slot = ctx.hidden(scrutinee_ty, scrutinee_name(m.origin));

    let mut lowered_arms: Vec<MatchArm> = Vec::with_capacity(m.arms.len());
    let mut result: Option<Ty> = None;
    for arm in &m.arms {
        let (tag, mut stmts) = lower_arm_head(ctx, arm, eid, scrutinee_ty, slot)?;
        // An arm's body is a value region: its statements run after the bindings, in the
        // arm's own branch, and its tail is the arm's value — lowered by the one function
        // every value region goes through, so a transferring arm has no result and stays
        // out of the join.
        let inner = lower_block(ctx, &arm.body)?;
        stmts.extend(inner.stmts);
        let block = Block {
            stmts,
            result: inner.result,
        };
        if let Some(t) = block.result.as_ref().and_then(|e| e.ty()) {
            result = Some(t);
        }
        lowered_arms.push(MatchArm { tag, block });
    }
    if let Some(t) = result {
        ctx.note(t);
    }

    Some(Expr::Match {
        scrutinee: Box::new(lowered),
        scrutinee_ty,
        slot,
        arms: lowered_arms,
        ty: result,
    })
}

/// What `--emit-ir` calls the hidden local a `match` owns its scrutinee in: `propagate` for
/// the one a `?` is, `match` for one that was written. The text a reader recognises, and the
/// only thing about a `match` that remembers where it came from.
fn scrutinee_name(origin: cir::MatchOrigin) -> &'static str {
    match origin {
        cir::MatchOrigin::Written => "match",
        cir::MatchOrigin::Propagate => "propagate",
    }
}

/// Statement `match`: the arms are statements, and one may complete with nothing. It
/// transfers only when **every** arm does; one arm falling through is a path past it.
fn lower_statement_match(ctx: &mut Ctx<'_>, m: &cir::Match) -> (Option<Stmt>, Ends) {
    let lowered = lower_expr(ctx, &m.scrutinee);
    let Some(scrutinee_ty) = lowered.as_ref().and_then(Expr::ty) else {
        // A scrutinee that failed to lower has reported; one that always transfers leaves
        // before any arm could run, so nothing after it is reachable either.
        return match lowered {
            Some(e) => (Some(Stmt::Discard(e)), Ends::Transfers),
            None => (None, Ends::FallsThrough),
        };
    };
    let Ty::Enum(eid) = scrutinee_ty else {
        ctx.unsupported(m.at, "a `match` on something that is not an enum");
        return (None, Ends::FallsThrough);
    };
    ctx.note(scrutinee_ty);
    let slot = ctx.hidden(scrutinee_ty, scrutinee_name(m.origin));

    let mut lowered_arms: Vec<StatementArm> = Vec::with_capacity(m.arms.len());
    let mut every_arm_leaves = !m.arms.is_empty();
    for arm in &m.arms {
        let Some((tag, mut body)) = lower_arm_head(ctx, arm, eid, scrutinee_ty, slot) else {
            return (None, Ends::FallsThrough);
        };
        let (effect, ends) = lower_body(ctx, &arm.body);
        body.extend(effect);
        if ends == Ends::FallsThrough {
            every_arm_leaves = false;
        }
        lowered_arms.push(StatementArm { tag, body });
    }

    let ends = if every_arm_leaves {
        Ends::Transfers
    } else {
        Ends::FallsThrough
    };
    let Some(scrutinee) = lowered else {
        return (None, ends);
    };
    (
        Some(Stmt::Match {
            scrutinee,
            scrutinee_ty,
            slot,
            arms: lowered_arms,
        }),
        ends,
    )
}

/// One arm's discriminant, and its bindings as the first statements of its body.
///
/// Shared by both forms of `match`, so a binding means the same thing whether or not the
/// `match` produces a value: an ordinary [`Stmt::Let`] of an [`Expr::Payload`], which takes
/// its own reference through the one path that already gets take-before-release right.
fn lower_arm_head(
    ctx: &mut Ctx<'_>,
    arm: &cir::Arm,
    eid: EnumTy,
    scrutinee_ty: Ty,
    slot: usize,
) -> Option<(u32, Vec<Stmt>)> {
    let tag = ctx.tag(eid, arm.variant)?;
    let mut stmts: Vec<Stmt> = Vec::new();
    for b in &arm.bindings {
        let position = ctx.payload_index(eid, tag, b.field)?;
        let (binding_slot, ty) = ctx.slot_of(b.local)?;
        ctx.note(ty);
        stmts.push(Stmt::Let {
            slot: binding_slot,
            value: Expr::Payload {
                base: Box::new(Expr::Load {
                    slot,
                    ty: scrutinee_ty,
                }),
                base_ty: scrutinee_ty,
                tag,
                at: position,
                ty,
            },
        });
    }
    Some((tag, stmts))
}

/// `base.field`.
fn lower_field(ctx: &mut Ctx<'_>, base: &cir::Expr, field: u32, span: Span) -> Option<Expr> {
    let base_lowered = lower_expr(ctx, base)?;
    let base_ty = base_lowered.ty()?;
    let Ty::Record(rid) = base_ty else {
        // Unreachable after checking, which refuses a projection on anything else
        // (`N0335`). Reported rather than assumed away.
        ctx.unsupported(span, "a field read from something that is not a record");
        return None;
    };
    let position = ctx.field_index(rid, field)?;
    let ty = ctx
        .layout(rid)
        .and_then(|l| l.fields.get(position as usize).copied())?;
    ctx.note(base_ty);
    ctx.note(ty);
    Some(Expr::Field {
        base: Box::new(base_lowered),
        base_ty,
        at: position,
        ty,
    })
}

fn is_generic(res: &Resolution, f: FnId) -> bool {
    res.function(f).is_some_and(|d| !d.type_params.is_empty())
}

/// One of the `Vec[T]` built-ins, with its concrete vector.
///
/// Which vector is **this site's**: the element type Core IR recorded for the call, as the
/// body being lowered substitutes it. Every ownership rule the emitter applies follows from
/// it — nothing here depends on how `T` was spelled.
fn lower_vec(
    ctx: &mut Ctx<'_>,
    op: Builtin,
    lowered: Vec<Option<Expr>>,
    type_args: &[Type],
    span: Span,
) -> Option<Expr> {
    let args: Vec<Expr> = lowered.into_iter().collect::<Option<_>>()?;
    let elem = type_args.first().copied()?;
    let elem = ctx.concrete(elem)?;
    let Some(vec) = ctx.res.types().find_vec(elem) else {
        ctx.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                span,
                format!(
                    "`nazm build` reached `{}` with no `Vec` of `{}` planned",
                    op.name(),
                    ctx.res.types().show(elem)
                ),
            )
            .help("this is a bug in the compiler's instance planning; `nazm run` still works"),
        );
        return None;
    };
    let vec_ty = ctx.ty(vec, span)?;
    let elem_ty = ctx.ty(elem, span)?;
    ctx.note(vec_ty);
    ctx.note(elem_ty);
    let Ty::Vec(vid) = vec_ty else { return None };
    // The front end established arity and types; this is the lowering's own check that it
    // did not lose them on the way.
    let want: Vec<Ty> = nazm_sema::Intrinsic::params(op)
        .iter()
        .map(|s| match s {
            nazm_sema::Shape::VecT => vec_ty,
            nazm_sema::Shape::T => elem_ty,
            other => other.monomorphic().map_or(Ty::Int, Ty::of),
        })
        .collect();
    if args.len() != want.len() || !args.iter().zip(&want).all(|(a, w)| fits(a, *w)) {
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
    Some(Expr::Vec {
        op,
        vec: vid,
        elem: elem_ty,
        args,
        span,
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

/// Value-position `if`: both branches are value regions, each its own scope.
fn lower_if(
    ctx: &mut Ctx<'_>,
    cond: &cir::Expr,
    then_block: &cir::Block,
    else_block: &cir::Block,
    span: Span,
) -> Option<Expr> {
    // The condition is lowered first, and outside both branch scopes.
    let cond = lower_expr(ctx, cond);

    // Both are lowered even when the condition failed, so all the problems in the program
    // are reported at once.
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
