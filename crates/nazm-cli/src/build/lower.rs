//! Validation, lowering and code generation: one artefact per module and per generic instance,
//! and what a freestanding image may not reach.

use super::timing::Clock;
use super::*;

/// Validation, lowering and code generation: the program's artefacts, the foreign symbols it
/// calls, what it exports, and which runtime services it reached.
#[allow(clippy::too_many_arguments)]
pub(super) fn lowered(
    loaded: &nazm_service::load::Loaded,
    json: bool,
    choice: Choice<'_>,
    target: &nazm_lir::backend::Target,
    freestanding: bool,
    chosen: &(dyn nazm_lir::backend::Backend + Sync),
    clif_store: &Stores,
    clif_key: &(dyn Fn(&str) -> ObjectKey + Sync),
    clock: &Clock,
    heap_limit: Option<u64>,
    summary: bool,
    summarized: &Summarize<'_>,
) -> Result<Lowered, ExitCode> {
    let map = &loaded.map;
    // Ordinary validation, lowering and emission, all on the interpreter's own stack —
    // `nazm_core::on_checked` never lets the tree out. Every one of those passes recurses
    // with the shape of the syntax tree, and so does dropping it and dropping the IR, so
    // running any of them on the caller's stack would make a documented bound depend on
    // whoever called us. Only the emitted text comes back, which is flat.
    //
    // A program that does not type-check is not a question about the backend's coverage,
    // so those diagnostics are reported first and lowering never runs.
    // What a generated artefact calls each file: its durable identity where it has one,
    // so the same project built from two checkouts produces the same bytes.
    let names = nazm_lir::SourceNames::of(map, &loaded.graph);
    //
    // The backend is handed validated MIR, lowered from Core IR — never the tree (N39), and
    // never Core IR or the checker's tables either (N40).
    // The foreign symbols the program calls, so a link that cannot resolve them is reported
    // as the missing `--link` it is rather than as a compiler bug (N42).
    let mut foreign: Vec<String> = Vec::new();
    // Each export's symbol and C signature, for a library's header (N55).
    let mut exported: Vec<(String, Vec<nazm_lir::Ty>, nazm_lir::Ty)> = Vec::new();
    let mut unhosted: Vec<String> = Vec::new();
    let mut reached = nazm_lir::Needs::default();
    clock.mark("load");
    let artefacts = match nazm_core::on_lowered(
        map,
        &loaded.graph,
        if choice.library {
            nazm_core::Entry::Absent
        } else {
            nazm_core::Entry::Required
        },
        |core, resolution| {
            clock.mark("front_end");
            // The checker's warnings (R1b, L15): reported, or the build refused under
            // `--deny-warnings`.
            let warnings = nazm_core::warnings(resolution);
            // `nazm run` reports none, as the interpreter reports none (R1.5, D11).
            if !warnings.is_empty() && !choice.run {
                if choice.deny_warnings {
                    let w = warnings.len();
                    eprintln!(
                        "nazm: {w} warning{}, refused by --deny-warnings",
                        if w == 1 { "" } else { "s" }
                    );
                    return Err(warnings);
                }
                crate::emit(&warnings, map, json);
            }
            // A device register exists only on a board (N62): not on a hosted target, and not
            // in a WebAssembly module, whose only outside is its imports (N64).
            if !target.is_board() {
                if let Some((at, name)) = nazm_core::machine_only_call(resolution) {
                    return Err(vec![nazm_core::machine_only(
                        at,
                        &name,
                        &format!("a build for `{}`", target.triple),
                    )]);
                }
            }
            // Nor sections (Gate 3): a board's image is what a section is placed in.
            if !target.is_board() {
                if let Some((at, what)) = nazm_core::board_only(core) {
                    return Err(vec![nazm_core::board_only_refused(
                        at,
                        &what,
                        &format!("a build for `{}`", target.triple),
                    )]);
                }
            }
            // A C pointer crosses as a 64-bit word (N85), which is C's pointer only where pointers
            // are 64 bits: on wasm32 and a Cortex-M3 it is refused by name rather than passed as
            // the wrong type (Q1-A1-05).
            if matches!(
                target.arch,
                nazm_lir::backend::Arch::Wasm32 | nazm_lir::backend::Arch::Thumbv7m
            ) {
                if let Some(d) = narrow_pointer_refusal(resolution, &target.triple) {
                    return Err(vec![d]);
                }
            }
            // A Cortex-M3 (Gate 3) has no floating point, and a board no library to supply it; and
            // no access of 64 bits, which a 64-bit device register promises is one.
            if target.arch == nazm_lir::backend::Arch::Thumbv7m {
                if let Some(d) = cortex_m3_refusal(resolution, &target.triple) {
                    return Err(vec![d]);
                }
            }
            // The summation idiom, vectorised (N66, §7.68): a native build's rewrite of Core IR,
            // so the interpreter, reading the same program, still runs the loop.
            let mut core = core.clone();
            nazm_service::vectorize::rewrite(&mut core);
            let program = crate::mir::lower(&core, resolution, &loaded.graph)?;
            clock.mark("mir");
            let mut native =
                nazm_lir::lower_as(program, &names, choice.library).map_err(|e| e.diagnostics)?;
            // A runtime artifact (N82) was built with thread-local failure state: a program
            // linked to one keeps it, one thread or not.
            native.single_thread &= choice.runtime.is_none();
            clock.mark("lir");
            foreign = native
                .sigs
                .iter()
                .filter(|s| s.foreign)
                .map(|s| s.symbol.as_str().to_owned())
                .collect();
            exported = native
                .sigs
                .iter()
                .filter_map(|s| {
                    s.export
                        .as_ref()
                        .map(|e| (e.as_str().to_owned(), s.params.clone(), s.ret))
                })
                .collect();
            exported.sort_by(|a, b| a.0.cmp(&b.0));
            let cached = |k: &str, shared: bool| match clif_store
                .for_unit(shared)
                .map(|s| s.get(&clif_key(k)))
            {
                Some(ObjectFound::Hit(bytes)) => Some(bytes),
                _ => None,
            };
            // Which units every project shares (R1.5, D12): a standard or prelude module's own
            // unit. A generic instance is the program's, since its type arguments are.
            let shared: Vec<bool> = nazm_lir::backend::units(&native)
                .iter()
                .map(|(_, u, instance)| {
                    !instance
                        && loaded
                            .graph
                            .key(u.module)
                            .is_some_and(|k| k.is_std() || k.is_prelude())
                })
                .collect();
            let made = emit_artefacts(
                &native,
                &names,
                chosen,
                &cached,
                &shared,
                choice.library,
                freestanding.then_some(Freestanding {
                    unhosted: &mut unhosted,
                    watermark: choice.stack_watermark,
                    wasm: target.is_wasm(),
                    wasi: target.is_wasi(),
                    adapter: choice.adapter,
                    board: super::board_for(choice, target),
                }),
                choice.runtime.is_some(),
                &mut reached,
                choice.stack_size,
                heap_limit,
            );
            clock.mark("codegen");
            made
        },
    ) {
        Ok(_) if !unhosted.is_empty() => {
            if summary {
                return Err(summarized(
                    Status::Refused,
                    exit::CHECK_FAILED,
                    &[],
                    Some(map),
                    None,
                ));
            }
            if target.is_wasi() {
                eprintln!(
                    "nazm: `{}` is WASI preview 1, one thread and no sockets, and this program \
                     reaches what it has not got: {}; a WASI module has a heap, standard streams, \
                     arguments, clocks and files beneath its preopened directory — no tasks, \
                     channels, network or C",
                    target.triple,
                    unhosted.join(", ")
                );
                return Err(ExitCode::from(exit::CHECK_FAILED));
            }
            eprintln!(
                "nazm: `{}` has no operating system, and this program reaches what only a hosted \
                 runtime has: {}; \
                 a freestanding program computes with `Int` and `Bool`, functions, loops, static \
                 strings and, on the board, device registers — and with a `[heap]` in the \
                 board's manifest (`--board`), strings, sequences and closures too",
                target.triple,
                unhosted.join(", ")
            );
            return Err(ExitCode::from(exit::CHECK_FAILED));
        }
        Ok(a) => a,
        Err(diagnostics) => {
            // `N0101` is this backend's "not yet", and it reads differently from a
            // program being wrong. Distinguished for the summary line only; both are a
            // check failure and neither produces output.
            if summary {
                return Err(summarized(
                    Status::Rejected,
                    exit::CHECK_FAILED,
                    &diagnostics,
                    Some(map),
                    None,
                ));
            }
            let unsupported = diagnostics
                .iter()
                .any(|d| d.code == nazm_diag::codes::UNSUPPORTED_CONSTRUCT);
            // `nazm run` falls back to nothing (R1.5, D11): a refusal only a native build makes
            // says what runs the program instead.
            let diagnostics: Vec<_> = if choice.run {
                diagnostics.into_iter().map(for_run).collect()
            } else {
                diagnostics
            };
            emit(&diagnostics, map, json);
            let n = diagnostics.len();
            if choice.run && diagnostics.iter().any(|d| native_only(&d.code)) {
                eprintln!(
                    "nazm: {n} error{} in compiling this program. Nothing ran; \
                     `nazm run --interpret` runs it in the interpreter.",
                    if n == 1 { "" } else { "s" }
                );
            } else if unsupported {
                eprintln!(
                    "nazm: {n} construct{} outside `nazm build`'s subset. Nothing was \
                     written; `nazm run --interpret` still executes this program.",
                    if n == 1 { "" } else { "s" }
                );
            } else {
                eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            }
            return Err(ExitCode::from(exit::CHECK_FAILED));
        }
    };
    Ok(Lowered {
        artefacts,
        foreign,
        exported,
        reached,
    })
}

/// What [`lowered`] produced.
pub(super) struct Lowered {
    pub(super) artefacts: Vec<Artefact>,
    /// The foreign symbols the program calls (N42).
    pub(super) foreign: Vec<String>,
    /// Each export's symbol and C signature (N55).
    pub(super) exported: Vec<(String, Vec<nazm_lir::Ty>, nazm_lir::Ty)>,
    /// The runtime services the program reached (N82).
    pub(super) reached: nazm_lir::Needs,
}

/// A freestanding build's choices (N62, N64): where the parts it cannot have are named, whether
/// the board measures its stack, and whether the image is WebAssembly rather than the board.
struct Freestanding<'a> {
    unhosted: &'a mut Vec<String>,
    watermark: bool,
    wasm: bool,
    /// WebAssembly with WASI (Gate 3): its runtime and entry, with a heap.
    wasi: bool,
    /// A contract's WebAssembly adapter, appended to the unit that defines its `init` (N71).
    adapter: Option<&'a crate::contract_wasm::Shape>,
    /// The board the image is for (Gate 3): `--board`'s, or the target's built-in one.
    board: nazm_lir::runtime::board::Board,
}

/// One generated native unit, ready to be written down.
pub(super) struct Artefact {
    /// The file name to use, without an extension.
    ///
    /// Derived from the module's position in load order rather than from its source path:
    /// a path is not a filename — it has separators, it may not be relative, and it is
    /// **not this compiler's to trust**. `docs/architecture.md` records it as build
    /// plumbing, deliberately not as an identity.
    pub(super) name: String,
    /// LLVM text for clang, or an object a backend produced itself.
    pub(super) code: nazm_lir::backend::Code,
    /// Whether this is a generic function instance's unit.
    pub(super) instance: bool,
    /// For an object: the key its backend gave it before generating it.
    pub(super) key: Option<String>,
    /// Whether the object came from the store under that key rather than being generated.
    pub(super) reused: bool,
    /// Whether every project shares this unit (R1.5, D12): the runtime's, and a standard or
    /// prelude module's own. Its object is filed in the user cache.
    pub(super) shared: bool,
}

/// Whether `code` is a refusal only a native build makes, of a program the interpreter runs: the
/// backend's subset (`N0101`) and the instances a generic function would need (`N0357`, `N0358`).
fn native_only(code: &nazm_diag::Code) -> bool {
    use nazm_diag::codes::{
        GENERIC_INSTANCE_LIMIT, UNBOUNDED_SPECIALISATION, UNSUPPORTED_CONSTRUCT,
    };
    [
        UNSUPPORTED_CONSTRUCT,
        UNBOUNDED_SPECIALISATION,
        GENERIC_INSTANCE_LIMIT,
    ]
    .contains(code)
}

/// A refusal as `nazm run` reports it (R1.5, D11): one only a native build makes says that
/// `nazm run --interpret` runs the program, since `nazm run` itself will not.
fn for_run(d: nazm_diag::Diagnostic) -> nazm_diag::Diagnostic {
    if native_only(&d.code) {
        d.help(
            "`nazm run` compiles, with Cranelift, and falls back to nothing; \
             `nazm run --interpret` runs this program in the interpreter",
        )
    } else {
        d
    }
}

/// Lower a checked compilation into one artefact per module, plus the program's support.
///
/// The order is load order, then the runtime, then the entry. Nothing here reads a
/// directory, so the same program produces the same artefacts in the same order. A unit the
/// backend can key before generating it, and whose object is in the store, is not generated.
#[allow(clippy::too_many_arguments)]
fn emit_artefacts(
    native: &nazm_lir::Native,
    names: &nazm_lir::SourceNames<'_>,
    backend: &dyn nazm_lir::backend::Backend,
    cached: &dyn Fn(&str, bool) -> Option<Vec<u8>>,
    shared_units: &[bool],
    library: bool,
    freestanding: Option<Freestanding<'_>>,
    prebuilt: bool,
    reached: &mut nazm_lir::Needs,
    stack: Option<u64>,
    heap_limit: Option<u64>,
) -> Result<Vec<Artefact>, Vec<nazm_diag::Diagnostic>> {
    let mut out = Vec::new();
    let mut needs = backend.baseline();
    let mut errors = Vec::new();
    // One artefact per module and one per generic function instance, never folded into a
    // caller's: the object cache then reuses each by its own identity, one instance
    // requested by two modules is one object, and a module's object does not change when
    // another module starts instantiating something. `docs/architecture.md` §7.12.
    for (i, plan) in backend.plan(native, names).into_iter().enumerate() {
        let shared = shared_units.get(i).copied().unwrap_or(false);
        if let Some(bytes) = plan.key.as_deref().and_then(|k| cached(k, shared)) {
            out.push(Artefact {
                name: plan.name,
                code: nazm_lir::backend::Code::Object(bytes),
                instance: plan.instance,
                key: plan.key,
                reused: true,
                shared,
            });
            continue;
        }
        match backend.generate(native, names, i) {
            Ok(g) => {
                needs = needs.union(g.needs);
                out.push(Artefact {
                    name: plan.name,
                    code: g.code,
                    instance: plan.instance,
                    key: plan.key,
                    reused: false,
                    shared,
                });
            }
            Err(mut e) => errors.append(&mut e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // A freestanding image (N62): the board's runtime and entry in place of the hosted ones,
    // or the parts it cannot have, named. A library has no board entry; nor does it need one.
    if let Some(Freestanding {
        unhosted: refused,
        watermark,
        wasm,
        wasi,
        adapter,
        board,
    }) = freestanding
    {
        // The contract adapter (N71, §7.73): text appended to the contract's own unit, so it can
        // call what that unit defines; the backend's code is unchanged.
        if let Some(shape) = adapter {
            let init = native
                .program
                .functions
                .iter()
                .find(|f| f.name == "init")
                .and_then(|f| native.sigs.get(f.id.index()))
                .map(|s| format!("@{}(", s.symbol.as_str()));
            let unit = out.iter_mut().find(|a| {
                matches!(&a.code, nazm_lir::backend::Code::Llvm(ir)
                    // A Nazm function returns its failure bit, alone where it has no result in
                    // a register (R1, 5b.8).
                    if init.as_ref().is_some_and(|i| ir.contains(&format!("define hidden i1 {i}"))
                        || ir.contains(&format!("define internal i1 {i}"))))
            });
            match unit.map(|a| &mut a.code) {
                Some(nazm_lir::backend::Code::Llvm(ir)) => {
                    match crate::contract_wasm::adapter(ir, shape, native) {
                        Ok(text) => ir.push_str(&text),
                        Err(why) => refused.push(format!("contract adapter ({why})")),
                    }
                }
                _ => refused.push("contract adapter (no unit defines `init`)".to_owned()),
            }
        }
        // WebAssembly's runtime reaches its host through imports (N64, §7.66).
        let runtime = if wasi {
            nazm_lir::runtime::wasi::emit_runtime(&needs)
        } else if wasm {
            nazm_lir::runtime::wasm::emit_runtime(needs.stack)
        } else {
            nazm_lir::runtime::board::emit_runtime_for(&board, &needs)
        };
        let foreign: Vec<&str> = native
            .sigs
            .iter()
            .filter(|s| s.foreign)
            .map(|s| s.symbol.as_str())
            .collect();
        refused.extend(freestanding_refusals(&out, &runtime, &foreign));
        *reached = needs;
        if !refused.is_empty() {
            return Ok(out);
        }
        // The interrupt handlers (Gate 3), each matched to an interrupt the board has.
        let mut handlers = interrupt_handlers(native, &board)?;
        // The fixed-priority scheduler's tasks (Gate 3), which own the board's timer.
        let tasks = board_tasks(native, &board, &mut handlers)?;
        board_watchdog(&out, &board, &tasks)?;
        // The failure hook (Gate 3): the one function `@on_failure` marks, by its symbol.
        let on_failure = native
            .program
            .functions
            .iter()
            .find(|f| f.on_failure)
            .and_then(|f| {
                native
                    .sigs
                    .get(f.id.index())
                    .map(|s| (s.symbol.as_str().to_owned(), f.param_types().count() == 2))
            });
        // A runtime artifact (N82) is linked instead, once it is checked to fit.
        if !prebuilt {
            out.push(Artefact {
                name: "runtime".to_owned(),
                code: nazm_lir::backend::Code::Llvm(runtime),
                instance: false,
                key: None,
                reused: false,
                shared: true,
            });
        }
        if !library {
            out.push(Artefact {
                name: "entry".to_owned(),
                code: nazm_lir::backend::Code::Llvm(if wasi {
                    nazm_lir::runtime::wasi::emit_entry(
                        &native.entry.to_string(),
                        native.entry_roots,
                    )
                } else if wasm {
                    nazm_lir::runtime::wasm::emit_entry(
                        &native.entry.to_string(),
                        native.entry_roots,
                    )
                } else {
                    nazm_lir::runtime::board::emit_entry_with(
                        &board,
                        &native.entry.to_string(),
                        native.entry_roots,
                        watermark,
                        &handlers,
                        on_failure.as_ref().map(|(s, cap)| (s.as_str(), *cap)),
                        &tasks,
                    )
                }),
                instance: false,
                key: None,
                reused: false,
                shared: false,
            });
        }
        return Ok(out);
    }
    // After every unit, because what the runtime must define is the union of what the
    // modules reached — a whole-program fact, and the only one emission needs. A runtime
    // artifact (N82) is linked instead, once it is checked to provide all of it.
    let needs = nazm_lir::Needs {
        single: native.single_thread,
        ..needs
    };
    *reached = needs;
    if !prebuilt {
        out.push(Artefact {
            name: "runtime".to_owned(),
            code: nazm_lir::backend::Code::Llvm(nazm_lir::emit_runtime(needs)),
            instance: false,
            key: None,
            reused: false,
            shared: true,
        });
        // The task pool's context switch (N83, §7.84), for this target, wherever the runtime
        // has tasks or channels.
        let settled = needs.settled();
        if settled.tasks || settled.channels {
            if let Some(t) = switch_target(backend.target()) {
                out.push(Artefact {
                    name: "switch".to_owned(),
                    code: nazm_lir::backend::Code::Llvm(nazm_lir::runtime::switch::emit_switch(t)),
                    instance: false,
                    key: None,
                    reused: false,
                    shared: true,
                });
            }
        }
        // What the target does before `main` (Gate 3): on Windows, binary standard streams.
        if let Some(t) = switch_target(backend.target()) {
            if let Some(text) = nazm_lir::runtime::platform::emit_init(t) {
                out.push(Artefact {
                    name: "init".to_owned(),
                    code: nazm_lir::backend::Code::Llvm(text),
                    instance: false,
                    key: None,
                    reused: false,
                    shared: true,
                });
            }
        }
        // The task stacks and the stack budget, the platform's (Gate 3): wherever the runtime
        // checks the stack or runs the pool.
        if settled.stack || settled.tasks || settled.channels {
            if let Some(t) = switch_target(backend.target()) {
                out.push(Artefact {
                    name: "stack".to_owned(),
                    code: nazm_lir::backend::Code::Llvm(nazm_lir::runtime::platform::emit_stack(t)),
                    instance: false,
                    key: None,
                    reused: false,
                    shared: true,
                });
            }
        }
        // Where this target keeps `errno` (N85, §7.86), wherever C is called or a file is used.
        if settled.foreign || settled.files {
            if let Some(t) = switch_target(backend.target()) {
                out.push(Artefact {
                    name: "errno".to_owned(),
                    code: nazm_lir::backend::Code::Llvm(nazm_lir::runtime::switch::emit_errno(t)),
                    instance: false,
                    key: None,
                    reused: false,
                    shared: true,
                });
            }
        }
        // What this target's C library says about files (Gate 2): wherever a file is used.
        if settled.files {
            if let Some(t) = switch_target(backend.target()) {
                out.push(Artefact {
                    name: "os".to_owned(),
                    code: nazm_lir::backend::Code::Llvm(nazm_lir::runtime::platform::emit_os(t)),
                    instance: false,
                    key: None,
                    reused: false,
                    shared: true,
                });
            }
        }
    }
    // A heap limit (R1d, D5): every part of the runtime allocates through the counted heap as the
    // units already do — all but the stack unit, whose task stacks are not heap — and the counted
    // heap's own unit holds the limit.
    if let Some(limit) = heap_limit {
        for a in &mut out {
            if let (false, nazm_lir::backend::Code::Llvm(text)) = (a.name == "stack", &mut a.code) {
                *text = nazm_lir::runtime::heap::count_heap(text);
            }
        }
        out.push(Artefact {
            name: "heap".to_owned(),
            code: nazm_lir::backend::Code::Llvm(nazm_lir::runtime::heap::emit_heap(limit)),
            instance: false,
            key: None,
            reused: false,
            shared: true,
        });
    }
    // A library has no process entry: C's `main` is the caller's (N55).
    if library {
        return Ok(out);
    }
    // The scheduler's policy is the program's, not the linked runtime's (N106, §7.107): whether
    // it declares a C function — Cranelift links every part of the runtime, C's included.
    let calls_c = nazm_lir::backend::units(native)
        .iter()
        .any(|(_, u, _)| u.externals.iter().any(|e| e.foreign));
    let entry_needs = nazm_lir::Needs {
        foreign: calls_c,
        ..needs
    };
    let entry = nazm_lir::emit_entry_for(
        &native.entry,
        native.entry_roots,
        entry_needs,
        switch_target(backend.target()).and_then(nazm_lir::runtime::platform::args_hook),
        stack,
    );
    // The entry's allocator, under a heap limit, is the counted heap's as every unit's is (D5).
    let entry = if heap_limit.is_some() {
        nazm_lir::runtime::heap::count_heap(&entry)
    } else {
        entry
    };
    out.push(Artefact {
        name: "entry".to_owned(),
        code: nazm_lir::backend::Code::Llvm(entry),
        instance: false,
        key: None,
        reused: false,
        shared: false,
    });
    Ok(out)
}

/// What the units call that a board without an operating system does not have (N62), by the
/// runtime part each symbol belongs to, in a fixed order: every symbol a unit declares that no
/// unit and not the board's runtime defines, apart from LLVM's intrinsics and the program's own
/// foreign functions, which `--link` supplies. Read from the generated text, so it is exactly
/// what the link would leave unresolved — a static string printed is a C library call there.
/// Every `@task` of the program (Gate 3, `docs/spec.md`, *Boards*), and the board timer handed to
/// the scheduler's tick: a board whose manifest gives the timer no period has no ticks to schedule
/// by, and a program that handles the timer itself would take them from it — both `N0627`.
fn board_tasks(
    native: &nazm_lir::Native,
    board: &nazm_lir::runtime::board::Board,
    handlers: &mut Vec<nazm_lir::runtime::board::Handler>,
) -> Result<Vec<nazm_lir::runtime::board::Task>, Vec<nazm_diag::Diagnostic>> {
    use nazm_lir::runtime::board::{Handler, Task};
    let tasks: Vec<(&nazm_mir::Function, (u32, u64))> = native
        .program
        .functions
        .iter()
        .filter_map(|f| f.task.map(|t| (f, t)))
        .collect();
    let Some((first, _)) = tasks.first() else {
        return Ok(Vec::new());
    };
    let refuse = |at, why: String| {
        vec![
            nazm_diag::Diagnostic::error(nazm_diag::codes::INTERRUPT_HANDLER, at, why).help(
                "a task runs on the board timer's ticks: give the manifest's `[interrupts]` a \
                   `timer_period_us`, and handle no `timer` interrupt yourself",
            ),
        ]
    };
    let timer = match &board.interrupts {
        Some(i) if i.timer_period_us.is_some() => i.timer_number(),
        _ => {
            return Err(refuse(
                first.span,
                format!(
                    "`{}` is a task, and the board `{}` has no timer to schedule it by",
                    first.name, board.name
                ),
            ))
        }
    };
    if handlers.iter().any(|h| h.intid == timer) {
        return Err(refuse(
            first.span,
            "a program with tasks handles no `timer` interrupt: the scheduler's ticks are the \
             timer's"
                .to_owned(),
        ));
    }
    handlers.push(Handler {
        intid: timer,
        symbol: "nz_sched_tick".to_owned(),
        takes_cap: false,
        defined_here: true,
    });
    Ok(tasks
        .iter()
        .filter_map(|(f, (priority, period))| {
            native.sigs.get(f.id.index()).map(|s| Task {
                name: f.name.clone(),
                symbol: s.symbol.as_str().to_owned(),
                priority: *priority,
                period: *period,
                takes_cap: f.param_types().next().is_some(),
            })
        })
        .collect())
}

/// The board's watchdog and its feeds (Gate 3, `docs/spec.md`, *Boards*): a program that feeds a
/// watchdog needs a board with one, and a watchdog needs the scheduler, whose ticks it counts and
/// whose tasks feed it. `N0626`, naming the board.
fn board_watchdog(
    units: &[Artefact],
    board: &nazm_lir::runtime::board::Board,
    tasks: &[nazm_lir::runtime::board::Task],
) -> Result<(), Vec<nazm_diag::Diagnostic>> {
    let feeds = units.iter().any(|a| match &a.code {
        nazm_lir::backend::Code::Llvm(ir) => ir
            .lines()
            .any(|l| !l.starts_with("declare ") && l.contains("@nz.watchdog_feed(")),
        nazm_lir::backend::Code::Object(_) => false,
    });
    let why = match (board.watchdog, feeds, tasks.is_empty()) {
        (None, true, _) => format!(
            "this program feeds a watchdog, and the board `{}` has none",
            board.name
        ),
        (Some(_), _, true) => format!(
            "the board `{}` has a watchdog, and this program no `@task` to feed it",
            board.name
        ),
        _ => return Ok(()),
    };
    Err(vec![nazm_diag::Diagnostic::error(
        nazm_diag::codes::BOARD_MANIFEST,
        nazm_span::Span::synthetic(),
        why,
    )
    .help(
        "a watchdog is the manifest's `[watchdog] ticks = N`, counted on the scheduler's ticks: \
         give the board one, and feed it from a `@task` with `watchdog_feed()`",
    )])
}

/// What crosses the C boundary as a pointer, on a target whose pointers are 32 bits (Q1-A1-05):
/// a handle argument or result, a C string result, a callback, and a C-layout record holding a
/// handle. Each crosses as a 64-bit word (N85, §7.86), so a C function compiled for the target —
/// which takes and returns 32-bit pointers, and lays a pointer field out in four bytes — would be
/// called with the wrong signature and read the wrong fields. `N0381` at the first declaration,
/// naming the target; `Int`, `Bool` and a C-layout record of them still cross.
fn narrow_pointer_refusal(
    res: &nazm_core::sema::Resolution,
    triple: &str,
) -> Option<nazm_diag::Diagnostic> {
    use nazm_core::check::{foreign_param, foreign_result, CrossIn, CrossOut};
    use nazm_core::sema::{CRepr, Type};
    // Whether a C-layout record holds a handle, at any depth of nested C-layout records.
    let holds_handle = |t: Type| {
        let mut work = vec![t];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(t) = work.pop() {
            let Type::Record(id) = t else { continue };
            if !seen.insert(id.index()) {
                continue;
            }
            let Some(r) = res.records().get(id) else {
                continue;
            };
            match r.repr {
                CRepr::Opaque => return true,
                CRepr::Layout => work.extend(r.fields.iter().map(|f| f.ty)),
                CRepr::Nazm => {}
            }
        }
        false
    };
    for f in res.functions() {
        if f.foreign.is_none() {
            continue;
        }
        let param = f.params.iter().find_map(|t| match foreign_param(res, *t)? {
            CrossIn::Handle(_) => Some("take a C handle"),
            CrossIn::Callback => Some("take a C function pointer"),
            CrossIn::Struct if holds_handle(*t) => Some("take a C struct holding a handle"),
            _ => None,
        });
        let result = match foreign_result(res, checked_option_of(res, f.ret), f.ret) {
            Some(CrossOut::Handle { .. }) => Some("return a C handle"),
            Some(CrossOut::CString { .. }) => Some("return a C string"),
            _ => None,
        };
        if let Some(what) = param.or(result) {
            return Some(
                nazm_diag::Diagnostic::error(
                    nazm_diag::codes::FOREIGN_TYPE,
                    f.name_span,
                    format!(
                        "`{}` cannot {what} in a build for `{triple}`: a C pointer crosses as \
                         64 bits, and this target's pointers are 32",
                        f.name
                    ),
                )
                .help(
                    "on a 32-bit target only `Int`, `Bool` and C-layout records of them cross to \
                     C; build a program that uses C handles, C strings or callbacks for a 64-bit \
                     target",
                ),
            );
        }
    }
    None
}

/// The enum definition a foreign result's type applies, as Core IR lowering takes it: the checker
/// accepted the declaration only if it is the prelude's `Option`.
fn checked_option_of(
    res: &nazm_core::sema::Resolution,
    t: nazm_core::sema::Type,
) -> Option<nazm_core::sema::EnumId> {
    match res.types().definition_of(t) {
        Some(nazm_core::sema::TypeNode::Enum(e)) => Some(e),
        _ => None,
    }
}

/// What a Cortex-M3 cannot do, at the first place the program asks it (Gate 3, `docs/spec.md`,
/// *Boards*): a `Float32` or `Float64` anywhere — the M3 has no FPU, and a board no soft-float
/// library — or a 64-bit device register, which the core splits into two accesses. `N0392`.
fn cortex_m3_refusal(
    res: &nazm_core::sema::Resolution,
    triple: &str,
) -> Option<nazm_diag::Diagnostic> {
    use nazm_core::sema::{NumKind, Type};
    let float = |t: Type| matches!(t, Type::Num(NumKind::F32 | NumKind::F64));
    let refuse = |at, what: &str| {
        nazm_diag::Diagnostic::error(
            nazm_diag::codes::MACHINE_ONLY,
            at,
            format!("a build for `{triple}` cannot {what}"),
        )
        .help(
            "a Cortex-M3 has no floating-point unit and no 64-bit access; compute in integers, \
             and reach a device with the 8-, 16- or 32-bit registers",
        )
    };
    for f in res.functions() {
        if float(f.ret) || f.params.iter().any(|t| float(*t)) {
            return Some(refuse(
                f.name_span,
                "compute in floating point: it has no FPU",
            ));
        }
        if let Some(l) = f.locals.iter().find(|l| float(l.ty)) {
            return Some(refuse(l.span, "compute in floating point: it has no FPU"));
        }
        for site in &f.call_sites {
            if let nazm_core::sema::resolve::Callee::Builtin(
                b @ (nazm_core::sema::Intrinsic::MmioRead64
                | nazm_core::sema::Intrinsic::MmioWrite64),
            ) = site.callee
            {
                return Some(refuse(
                    site.span,
                    &format!("perform `{}`: its accesses are 32 bits wide", b.name()),
                ));
            }
        }
    }
    None
}

/// Every `@interrupt` handler of the program, matched to the interrupt the board's manifest
/// names (Gate 3, `docs/spec.md`, *Boards*): `timer` is the board timer's, which needs a period,
/// and every other name is one of `[interrupts.lines]`. A board without `[interrupts]`, or a name
/// it does not have, is `N0627` at the handler.
fn interrupt_handlers(
    native: &nazm_lir::Native,
    board: &nazm_lir::runtime::board::Board,
) -> Result<Vec<nazm_lir::runtime::board::Handler>, Vec<nazm_diag::Diagnostic>> {
    use nazm_lir::runtime::board::Handler;
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for f in &native.program.functions {
        let Some(name) = &f.interrupt else { continue };
        let refuse = |why: String| {
            nazm_diag::Diagnostic::error(nazm_diag::codes::INTERRUPT_HANDLER, f.span, why).help(
                "a board's interrupts are its manifest's `[interrupts]`: `timer` with \
                 `timer_period_us`, and each name of `[interrupts.lines]`",
            )
        };
        let intid = match &board.interrupts {
            None => {
                errors.push(refuse(format!(
                    "`{}` handles `{name}`, and the board `{}` has no interrupts",
                    f.name, board.name
                )));
                continue;
            }
            Some(i) if name == "timer" => {
                if i.timer_period_us.is_none() {
                    errors.push(refuse(format!(
                        "`{}` handles the board's timer, and its manifest gives no \
                         `timer_period_us`",
                        f.name
                    )));
                    continue;
                }
                i.timer_number()
            }
            Some(i) => match i.lines.iter().find(|(n, _)| n == name) {
                Some((_, id)) => i.line_number(*id),
                None => {
                    errors.push(refuse(format!(
                        "`{}` handles `{name}`, and the board has no interrupt of that name",
                        f.name
                    )));
                    continue;
                }
            },
        };
        let Some(sig) = native.sigs.get(f.id.index()) else {
            continue;
        };
        out.push(Handler {
            intid,
            symbol: sig.symbol.as_str().to_owned(),
            takes_cap: f.param_types().next().is_some(),
            defined_here: false,
        });
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

fn freestanding_refusals(units: &[Artefact], runtime: &str, foreign: &[&str]) -> Vec<String> {
    use nazm_lir::runtime::Part;
    let texts: Vec<&str> = units
        .iter()
        .filter_map(|a| match &a.code {
            nazm_lir::backend::Code::Llvm(ir) => Some(ir.as_str()),
            nazm_lir::backend::Code::Object(_) => None,
        })
        .chain(std::iter::once(runtime))
        .collect();
    let name = |rest: &str| -> String {
        rest.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$' | '-'))
            .collect()
    };
    // A unit declares the whole inventory; what it needs is what a line other than a
    // declaration names.
    let mut defined = std::collections::BTreeSet::new();
    let mut declared = std::collections::BTreeSet::new();
    let mut named = std::collections::BTreeSet::new();
    for line in texts.iter().flat_map(|t| t.lines()) {
        if line.starts_with("declare ") {
            if let Some((_, rest)) = line.split_once('@') {
                declared.insert(name(rest));
            }
            continue;
        }
        if line.starts_with("define ") {
            if let Some((_, rest)) = line.split_once('@') {
                defined.insert(name(rest));
            }
        }
        named.extend(line.split('@').skip(1).map(name));
    }
    let missing: Vec<&String> = declared
        .iter()
        .filter(|s| {
            named.contains(*s)
                && !defined.contains(*s)
                && !s.starts_with("llvm.")
                // The host's imports (N64): undefined in the module by design.
                && !s.starts_with("nz.host_")
                // WASI's imports (Gate 3): the host's, by name.
                && !s.starts_with("wasi.")
                // The failure hook's caller (Gate 3): the board entry's, written after this.
                && s.as_str() != "nz_board_on_failure"
                && !foreign.contains(&s.as_str())
        })
        .collect();
    let part_of = |s: &str| {
        nazm_lir::runtime::IMPORTS
            .iter()
            .find(|i| i.symbol == s)
            .map_or(Part::Base, |i| i.part)
    };
    [
        (Part::Strings, "strings"),
        (Part::Sequences, "sequences"),
        (Part::Io, "input and output"),
        (Part::Tasks, "tasks"),
        (Part::Channels, "channels"),
        (Part::Join, "task joins"),
        (Part::Closures, "closures"),
        (Part::Numbers, "numbers"),
        (Part::Files, "files and handles"),
        (Part::Net, "networking"),
        (Part::Wide, "128-bit integers"),
        (
            Part::Foreign,
            "`c_errno()` (N0613: no C library keeps an `errno`)",
        ),
        (Part::Libc, "C library"),
        (Part::Base, "hosted base"),
    ]
    .into_iter()
    .filter(|(part, _)| missing.iter().any(|s| part_of(s) == *part))
    .map(|(part, label)| {
        // Each part with the symbols that reach it, so the refusal says what to remove.
        let symbols: Vec<String> = missing
            .iter()
            .filter(|s| part_of(s) == part)
            .map(|s| format!("`{s}`"))
            .collect();
        format!("{label} ({})", symbols.join(", "))
    })
    .collect()
}
