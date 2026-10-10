//! MIR to units: layout, symbols, linkage, and what the native subset refuses.
//!
//! # What this reads
//!
//! Validated MIR (`docs/architecture.md` §7.42) and nothing of the checker: not the syntax
//! tree, not a position map, not a `Resolution`. Until N40 this file lowered Core IR itself —
//! deciding temporaries, `?`, completion, and which slot a partly built value lived in — and
//! the emitter decided cleanup; MIR decides all of that now, once.
//!
//! What is decided here, and only here, is what MIR deliberately leaves out: layout — where a
//! field sits, what a variant's discriminant is — symbols and linkage, and which definitions
//! each unit must declare because another unit owns them.
//!
//! # Refusal, not partial compilation
//!
//! A program reaching here checked and its MIR validated, so it is well-typed. The refusals
//! below are about this backend's coverage and not about the program being wrong: a construct
//! outside the subset is a perfectly good Nazm construct this backend cannot compile, and saying
//! so is different from saying it is invalid. (Recursion was the first of them; since N49 it
//! compiles, with a stack check MIR places.) Every refusal points at `nazm run`, which still executes all of
//! it. Every unsupported construct produces a diagnostic and **no output**.

use std::collections::BTreeSet;

use nazm_diag::{codes, Diagnostic};
use nazm_mir as mir;
use nazm_sema::{ModuleId, Type};
use nazm_span::Span;

use crate::ir::{
    EnumLayout, EnumTy, External, RecordLayout, RecordTy, Ty, Unit, VariantLayout, VecLayout, VecTy,
};
use crate::names::SourceNames;
use crate::symbol::{encoded, of_identity, Linkage, NativeSymbol, NativeTypeName};

/// What the subset is, in one sentence, for every refusal to point at. Kept verbatim
/// across N40 so that no diagnostic a user sees changes with the representation beneath it.
const SUBSET: &str = "`nazm build` currently compiles functions over `Int`, `Bool` and \
                      `Str` — literals, `let` and `let mut` bindings, parameters, \
                      assignment, `while` with `break` and `continue`, `return`, unary \
                      `-`, the operators `+ - * / %`, the comparisons `== != < <= > >=`, \
                      parentheses, `if` in both statement and value position, the \
                      built-ins `str_len` `str_byte` `str_slice` `str_concat` \
                      `int_to_str`, direct calls, recursive ones included, and structured \
                      concurrency (`scope`, `spawn` and `Chan`) — with \
                      `fn main() -> Int` as the entry point. Everything else still runs \
                      under `nazm run`";

/// Constructs outside the native subset, with the spans that named them.
#[derive(Debug)]
pub struct LowerError {
    pub diagnostics: Vec<Diagnostic>,
}

/// What the backend knows about a MIR function before any body is emitted.
#[derive(Debug, Clone)]
pub struct Sig {
    /// What the linker calls it, derived from its durable identity.
    pub symbol: NativeSymbol,
    /// How much of the program may bind to it.
    pub linkage: Linkage,
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Which module declares it, so a unit knows which of these it owns.
    pub module: ModuleId,
    /// Whether this is a generic instance's, which is always defined in a unit of its own.
    pub instance: bool,
    /// Whether this is a foreign (C) function (N42): declared wherever it is called, defined
    /// nowhere in the program, and called with the C convention.
    pub foreign: bool,
    /// The C symbol an exported function's C entry is defined under (N55), in the unit that
    /// defines the function.
    pub export: Option<NativeSymbol>,
}

/// Everything one `nazm build` generates, before any of it is written down.
#[derive(Debug, Clone)]
pub struct Native {
    /// The validated MIR every unit's bodies are emitted from.
    pub program: mir::Program,
    /// By [`mir::FuncId`].
    pub sigs: Vec<Sig>,
    /// One per Nazm module that defines a concrete function, in module order.
    pub units: Vec<Unit>,
    /// One per concrete generic function instance, in MIR's canonical order.
    ///
    /// **Separate artefacts**, never appended to a caller's unit: an instance is a function of
    /// its generic definition and its type arguments, so one instance requested by five
    /// modules is one unit and one object, and a module's own object stays a function of that
    /// module's source and its dependencies' signatures. `docs/architecture.md` §7.12.
    pub instances: Vec<Unit>,
    /// The root module's `main`, which the platform entry wrapper calls.
    pub entry: NativeSymbol,
    /// How many capability parameters `main` takes (N37): the root authority the entry
    /// wrapper hands it, one erased word each.
    pub entry_roots: usize,
}

fn type_name(identity: &mir::TypeIdentity) -> NativeTypeName {
    let base = match &identity.owner {
        mir::Owner::Keyed(k) => NativeTypeName::of_definition(k),
        mir::Owner::Unkeyed { module, name } => NativeTypeName::of_unkeyed(*module, name),
    };
    if identity.args.is_empty() {
        base
    } else {
        NativeTypeName::of_instance(&base, &identity.args)
    }
}

/// Positions, by declaration index, of a canonical order.
fn of_declaration(canonical: &[u32], n: usize) -> Vec<u32> {
    let mut out = vec![0_u32; n];
    for (position, declared) in canonical.iter().enumerate() {
        if let Some(slot) = out.get_mut(*declared as usize) {
            *slot = u32::try_from(position).unwrap_or(u32::MAX);
        }
    }
    out
}

/// How every record, enum and `Vec` the program's MIR mentions is laid out.
///
/// One table, computed before any unit is emitted, for the reason `docs/spec.md` gives under
/// *Separate code generation*: two objects that pass a record between them must agree about
/// it, and the only way to be sure is that neither decided. The order is **canonical by name**,
/// which MIR carries: moving two field or variant declarations — an edit the language says
/// means nothing — changes no byte of any object.
fn layouts(types: &mir::Types) -> (Vec<RecordLayout>, Vec<EnumLayout>, Vec<VecLayout>) {
    let records = types
        .records
        .iter()
        .map(|(id, r)| RecordLayout {
            id: RecordTy(*id),
            name: type_name(&r.identity),
            source: r.source.clone(),
            fields: r
                .canonical
                .iter()
                .filter_map(|d| r.fields.get(*d as usize))
                .map(|f| Ty::of(f.ty))
                .collect(),
            of_declaration: of_declaration(&r.canonical, r.fields.len()),
        })
        .collect();
    let enums = types
        .enums
        .iter()
        .map(|(id, e)| {
            let name = type_name(&e.identity);
            let variants = e
                .canonical
                .iter()
                .filter_map(|d| e.variants.get(*d as usize))
                .map(|v| VariantLayout {
                    source: v.name.clone(),
                    name: name.member(&v.name),
                    fields: v
                        .canonical
                        .iter()
                        .filter_map(|d| v.fields.get(*d as usize))
                        .map(|f| Ty::of(f.ty))
                        .collect(),
                    of_declaration: of_declaration(&v.canonical, v.fields.len()),
                })
                .collect();
            EnumLayout {
                id: EnumTy(*id),
                name,
                source: e.source.clone(),
                variants,
                of_declaration: of_declaration(&e.canonical, e.variants.len()),
            }
        })
        .collect();
    let vecs = types
        .vecs
        .iter()
        .map(|(id, v)| VecLayout {
            id: VecTy(*id),
            name: encoded(&v.elem_key),
            elem: Ty::of(v.elem),
            source: v.source.clone(),
        })
        .collect();
    (records, enums, vecs)
}

fn unsupported_at(span: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::UNSUPPORTED_CONSTRUCT,
        span,
        format!("{what} is not in `nazm build`'s subset yet"),
    )
    .primary_label("not in the native subset")
    .help(SUBSET)
}

/// Every MIR function each function calls or spawns, with the span of the site.
fn callees(f: &mir::Function) -> Vec<(mir::FuncId, Span)> {
    let mut out = Vec::new();
    for b in &f.blocks {
        for s in &b.stmts {
            match &s.kind {
                mir::StatementKind::Assign {
                    value: mir::Rvalue::Call { callee, .. },
                    ..
                } => out.push((*callee, s.at)),
                // A closure names its code, in its own module (N50).
                mir::StatementKind::Assign {
                    value: mir::Rvalue::Closure { func, .. },
                    ..
                } => out.push((*func, s.at)),
                mir::StatementKind::Spawn {
                    callee, callee_at, ..
                } => out.push((*callee, *callee_at)),
                _ => {}
            }
        }
    }
    out
}

/// Lower validated MIR to the units `nazm build` emits.
///
/// # Errors
///
/// Every construct outside the native subset, each with the span that names it.
/// Two foreign declarations of one C symbol that disagree about its signature, anywhere in
/// the program (N42, `N0383`). Each module's unit declares the symbol it calls with its own
/// signature, and the linker binds them all to one definition without comparing them, so a
/// disagreement is a call with the wrong ABI that nothing downstream would notice.
fn report_foreign_conflicts(program: &mir::Program, errors: &mut Vec<Diagnostic>) {
    // An export's symbol is defined by Nazm (N55): once in the program, and never one the
    // program also imports from C.
    let mut defined: std::collections::BTreeMap<&str, &mir::Function> =
        std::collections::BTreeMap::new();
    for f in &program.functions {
        let Some(sym) = f.export.as_deref() else {
            continue;
        };
        let imported = program
            .functions
            .iter()
            .find(|g| g.foreign.as_deref() == Some(sym));
        if let Some(first) = defined.get(sym).copied().or(imported) {
            errors.push(
                Diagnostic::error(
                    codes::FOREIGN_CONFLICT,
                    f.span,
                    format!(
                        "the C symbol `{sym}` is exported here and defined or declared elsewhere"
                    ),
                )
                .secondary(first.span, "also here")
                .help("a C symbol has one definition in a program"),
            );
            continue;
        }
        defined.insert(sym, f);
    }
    let mut seen: std::collections::BTreeMap<&str, &mir::Function> =
        std::collections::BTreeMap::new();
    for f in &program.functions {
        let Some(sym) = f.foreign.as_deref() else {
            continue;
        };
        let Some(first) = seen.get(sym).copied() else {
            seen.insert(sym, f);
            continue;
        };
        let same = first.ret == f.ret && first.param_types().eq(f.param_types());
        if !same {
            errors.push(
                Diagnostic::error(
                    codes::FOREIGN_CONFLICT,
                    f.span,
                    format!(
                        "the C symbol `{sym}` is declared here with a signature another \
                         declaration of it does not have"
                    ),
                )
                .secondary(first.span, "declared differently here")
                .help(
                    "a C function has one signature; declare it once, or declare it the same \
                     way everywhere",
                ),
            );
        }
    }
}

pub fn lower(program: mir::Program, names: &SourceNames<'_>) -> Result<Native, LowerError> {
    lower_as(program, names, false)
}

/// [`lower`], for a static library when `library` (N55): no `main` is required, and the entry
/// symbol is never emitted.
///
/// # Errors
///
/// As [`lower`].
pub fn lower_as(
    program: mir::Program,
    names: &SourceNames<'_>,
    library: bool,
) -> Result<Native, LowerError> {
    let mut errors = Vec::new();
    let entry_id = program.entry;

    // Signatures, symbols and linkage.
    let sigs: Vec<Sig> = program
        .functions
        .iter()
        .map(|f| {
            let instance = !f.type_args.is_empty();
            // Bindable across objects when another artefact must reach it: exported, reached
            // from a generic body (whose instances are artefacts of their own), an instance,
            // or `main`, which the entry wrapper in a unit of its own calls.
            // A function `@section` places (Gate 3) is in the image whether or not it was
            // inlined: hidden, which the optimiser keeps, rather than internal.
            let linkage = if f.exported
                || f.reached_from_generic
                || instance
                || Some(f.id) == entry_id
                || f.section.is_some()
            {
                Linkage::Hidden
            } else {
                Linkage::Internal
            };
            Sig {
                symbol: f
                    .foreign
                    .as_deref()
                    .map_or_else(|| of_identity(&f.identity), NativeSymbol::foreign),
                linkage,
                params: f.param_types().map(Ty::of).collect(),
                ret: Ty::of(f.ret),
                module: f.module,
                instance,
                foreign: f.foreign.is_some(),
                export: f.export.as_deref().map(NativeSymbol::foreign),
            }
        })
        .collect();

    // `main`.
    let entry = match entry_id.and_then(|e| program.function(e)) {
        Some(f) => {
            if !f.param_types().all(|t| t.is_capability()) {
                errors.push(unsupported_at(f.span, "a `main` with parameters"));
            }
            if f.ret != Type::Int {
                errors.push(unsupported_at(
                    f.ret_span,
                    "a `main` that does not return `Int`",
                ));
            }
            Some((of_identity(&f.identity), f.capability_params()))
        }
        None => None,
    };

    // A task may not be handed a sequence: it is shared and mutable, and nothing synchronises
    // it.
    for f in &program.functions {
        for b in &f.blocks {
            for s in &b.stmts {
                if let mir::StatementKind::Spawn {
                    args, callee_at, ..
                } = &s.kind
                {
                    for a in args {
                        let t = match a {
                            mir::Operand::Local(l) => f.local(*l).and_then(|d| d.ty.value()),
                            mir::Operand::Const(c) => Some(c.ty()),
                        };
                        if let Some(bad @ (Type::Ints | Type::Strs)) = t {
                            errors.push(unsupported_at(
                                *callee_at,
                                &format!(
                                    "a task given a `{}`, which may not cross into one",
                                    bad.builtin_name().unwrap_or("sequence")
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    report_foreign_conflicts(&program, &mut errors);

    let entry = entry.or_else(|| library.then(|| (NativeSymbol::missing(), 0)));
    let Some((entry_symbol, entry_roots)) = entry else {
        if errors.is_empty() {
            errors.push(Diagnostic::error(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "this program has no `main` to compile",
            ));
        }
        return Err(LowerError {
            diagnostics: errors,
        });
    };
    if !errors.is_empty() {
        return Err(LowerError {
            diagnostics: errors,
        });
    }

    let (records, enums, vecs) = layouts(&program.types);
    let unit_of = |functions: Vec<mir::FuncId>, module: ModuleId, owns_module: bool| -> Unit {
        let source = program
            .modules
            .iter()
            .find(|(m, _)| *m == module)
            .map_or_else(String::new, |(_, file)| names.name(*file).to_owned());
        let mut externals: Vec<External> = Vec::new();
        let mut used: Vec<Type> = Vec::new();
        for id in &functions {
            let Some(f) = program.function(*id) else {
                continue;
            };
            used.push(f.ret);
            used.extend(f.locals.iter().filter_map(|l| l.ty.value()));
            for b in &f.blocks {
                for s in &b.stmts {
                    if let mir::StatementKind::Assign { value, .. } = &s.kind {
                        match value {
                            mir::Rvalue::Record { record, .. } => used.push(Type::Record(*record)),
                            mir::Rvalue::Variant { enumeration, .. }
                            | mir::Rvalue::Payload { enumeration, .. } => {
                                used.push(Type::Enum(*enumeration));
                            }
                            mir::Rvalue::Field { field, .. } => {
                                used.push(Type::Record(field.record));
                            }
                            mir::Rvalue::Primitive { elem: Some(e), .. } => {
                                used.push(*e);
                                used.extend(program.types.vec_of(*e));
                            }
                            mir::Rvalue::Compare { operand, .. } => used.push(*operand),
                            mir::Rvalue::Closure { func, .. } => used.extend(
                                program
                                    .function(*func)
                                    .and_then(|g| g.closure.clone())
                                    .unwrap_or_default(),
                            ),
                            _ => {}
                        }
                    }
                }
                if let mir::Terminator::Switch { enumeration, .. } = &b.term {
                    used.push(Type::Enum(*enumeration));
                }
            }
            for (callee, _) in callees(f) {
                let Some(sig) = sigs.get(callee.index()) else {
                    continue;
                };
                // An instance's unit owns no module's definitions, so everything it calls is
                // external to it; a module's unit owns its own non-instance functions. A
                // foreign function is defined by no unit, so every caller declares it.
                if owns_module && sig.module == module && !sig.instance && !sig.foreign {
                    continue;
                }
                // Nor is a function this unit defines, whatever it is: an instance that calls
                // itself, which recursion through a generic function does (N49), is defined
                // here and must not also be declared.
                if !sig.foreign && functions.contains(&callee) {
                    continue;
                }
                if let Some(g) = program.function(callee) {
                    used.push(g.ret);
                    used.extend(g.param_types());
                }
                let x = External {
                    symbol: sig.symbol.clone(),
                    params: sig.params.clone(),
                    ret: sig.ret,
                    foreign: sig.foreign,
                };
                if !externals.contains(&x) {
                    externals.push(x);
                }
            }
        }
        externals.sort();
        // The types this unit's text mentions, closed over what they contain: a record this
        // unit names is defined in its text, and so is every type its fields name.
        let mut rs: BTreeSet<u32> = BTreeSet::new();
        let mut es: BTreeSet<u32> = BTreeSet::new();
        let mut vs: BTreeSet<u32> = BTreeSet::new();
        while let Some(t) = used.pop() {
            match t {
                Type::Record(id) => {
                    let k = u32::try_from(id.index()).unwrap_or(u32::MAX);
                    if rs.insert(k) {
                        if let Some(r) = program.types.record(id) {
                            used.extend(r.fields.iter().map(|f| f.ty));
                        }
                    }
                }
                Type::Enum(id) => {
                    let k = u32::try_from(id.index()).unwrap_or(u32::MAX);
                    if es.insert(k) {
                        if let Some(e) = program.types.enumeration(id) {
                            for v in &e.variants {
                                used.extend(v.fields.iter().map(|f| f.ty));
                            }
                        }
                    }
                }
                Type::Vec(id) => {
                    let k = u32::try_from(id.index()).unwrap_or(u32::MAX);
                    if vs.insert(k) {
                        if let Some(v) = program.types.vec(id) {
                            used.push(v.elem);
                        }
                    }
                }
                // An array's element is laid out in this unit (Gate 2).
                Type::Array(id) => {
                    if let Some((elem, _)) = program.types.array(id) {
                        if !used.contains(&elem) {
                            used.push(elem);
                        }
                    }
                }
                // What a generic channel carries is laid out in this unit, as an element is.
                Type::ChanOf(id) => {
                    if let Some(c) = program.types.chan(id) {
                        let elem = c.elem;
                        if !used.contains(&elem) {
                            used.push(elem);
                        }
                    }
                }
                _ => {}
            }
        }
        Unit {
            module,
            source,
            functions,
            owns_module,
            externals,
            records: records
                .iter()
                .filter(|r| rs.contains(&r.id.0))
                .cloned()
                .collect(),
            enums: enums
                .iter()
                .filter(|e| es.contains(&e.id.0))
                .cloned()
                .collect(),
            vecs: vecs
                .iter()
                .filter(|v| vs.contains(&v.id.0))
                .cloned()
                .collect(),
            arrays: program
                .types
                .arrays
                .iter()
                .map(|(id, a)| crate::ir::ArrayLayout {
                    id: crate::ir::ArrayTy(*id),
                    elem: Ty::of(a.elem),
                    len: a.len,
                })
                .collect(),
        }
    };

    let units: Vec<Unit> = program
        .modules
        .iter()
        .map(|(m, _)| m)
        .map(|m| {
            let functions = program
                .functions
                .iter()
                .filter(|f| f.module == *m && f.type_args.is_empty() && f.foreign.is_none())
                .map(|f| f.id)
                .collect();
            unit_of(functions, *m, true)
        })
        .collect();
    let instances: Vec<Unit> = program
        .functions
        .iter()
        .filter(|f| !f.type_args.is_empty())
        .map(|f| unit_of(vec![f.id], f.module, false))
        .collect();

    Ok(Native {
        sigs,
        units,
        instances,
        entry: entry_symbol,
        entry_roots,
        program,
    })
}
