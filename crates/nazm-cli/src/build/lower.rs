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
    clif_store: &Option<ObjectStore>,
    clif_key: &(dyn Fn(&str) -> ObjectKey + Sync),
    clock: &Clock,
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
            // The summation idiom, vectorised (N66, §7.68): a native build's rewrite of Core IR,
            // so the interpreter, reading the same program, still runs the loop.
            let mut core = core.clone();
            nazm_service::vectorize::rewrite(&mut core);
            let program = crate::mir::lower(&core, resolution, &loaded.graph)?;
            clock.mark("mir");
            let native =
                nazm_lir::lower_as(program, &names, choice.library).map_err(|e| e.diagnostics)?;
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
            let cached = |k: &str| match clif_store.as_ref().map(|s| s.get(&clif_key(k))) {
                Some(ObjectFound::Hit(bytes)) => Some(bytes),
                _ => None,
            };
            let made = emit_artefacts(
                &native,
                &names,
                chosen,
                &cached,
                choice.library,
                freestanding.then_some(Freestanding {
                    unhosted: &mut unhosted,
                    watermark: choice.stack_watermark,
                    wasm: target.is_wasm(),
                    adapter: choice.adapter,
                }),
                choice.runtime.is_some(),
                &mut reached,
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
            eprintln!(
                "nazm: `{}` has no operating system, and this program reaches what only a hosted \
                 runtime has: {}; \
                 a freestanding program computes with `Int` and `Bool`, functions, loops, static \
                 strings and, on the board, device registers",
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
            emit(&diagnostics, map, json);
            let n = diagnostics.len();
            if unsupported {
                eprintln!(
                    "nazm: {n} construct{} outside `nazm build`'s subset. Nothing was \
                     written; `nazm run` still executes this program.",
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
    /// A contract's WebAssembly adapter, appended to the unit that defines its `init` (N71).
    adapter: Option<&'a crate::contract_wasm::Shape>,
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
    cached: &dyn Fn(&str) -> Option<Vec<u8>>,
    library: bool,
    freestanding: Option<Freestanding<'_>>,
    prebuilt: bool,
    reached: &mut nazm_lir::Needs,
) -> Result<Vec<Artefact>, Vec<nazm_diag::Diagnostic>> {
    let mut out = Vec::new();
    let mut needs = backend.baseline();
    let mut errors = Vec::new();
    // One artefact per module and one per generic function instance, never folded into a
    // caller's: the object cache then reuses each by its own identity, one instance
    // requested by two modules is one object, and a module's object does not change when
    // another module starts instantiating something. `docs/architecture.md` §7.12.
    for (i, plan) in backend.plan(native, names).into_iter().enumerate() {
        if let Some(bytes) = plan.key.as_deref().and_then(cached) {
            out.push(Artefact {
                name: plan.name,
                code: nazm_lir::backend::Code::Object(bytes),
                instance: plan.instance,
                key: plan.key,
                reused: true,
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
        adapter,
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
                    if init.as_ref().is_some_and(|i| ir.contains(&format!("define hidden void {i}"))
                        || ir.contains(&format!("define internal void {i}"))))
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
        let runtime = if wasm {
            nazm_lir::runtime::wasm::emit_runtime(needs.stack)
        } else {
            nazm_lir::runtime::board::emit_runtime(board_of(backend.target()), needs.stack)
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
        // A runtime artifact (N82) is linked instead, once it is checked to fit.
        if !prebuilt {
            out.push(Artefact {
                name: "runtime".to_owned(),
                code: nazm_lir::backend::Code::Llvm(runtime),
                instance: false,
                key: None,
                reused: false,
            });
        }
        if !library {
            out.push(Artefact {
                name: "entry".to_owned(),
                code: nazm_lir::backend::Code::Llvm(if wasm {
                    nazm_lir::runtime::wasm::emit_entry(
                        &native.entry.to_string(),
                        native.entry_roots,
                    )
                } else {
                    nazm_lir::runtime::board::emit_entry(
                        board_of(backend.target()),
                        &native.entry.to_string(),
                        native.entry_roots,
                        watermark,
                    )
                }),
                instance: false,
                key: None,
                reused: false,
            });
        }
        return Ok(out);
    }
    // After every unit, because what the runtime must define is the union of what the
    // modules reached — a whole-program fact, and the only one emission needs. A runtime
    // artifact (N82) is linked instead, once it is checked to provide all of it.
    *reached = needs;
    if !prebuilt {
        out.push(Artefact {
            name: "runtime".to_owned(),
            code: nazm_lir::backend::Code::Llvm(nazm_lir::emit_runtime(needs)),
            instance: false,
            key: None,
            reused: false,
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
                });
            }
        }
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
    out.push(Artefact {
        name: "entry".to_owned(),
        code: nazm_lir::backend::Code::Llvm(nazm_lir::emit_entry_for(
            &native.entry,
            native.entry_roots,
            entry_needs,
            switch_target(backend.target()).and_then(nazm_lir::runtime::platform::args_hook),
        )),
        instance: false,
        key: None,
        reused: false,
    });
    Ok(out)
}

/// What the units call that a board without an operating system does not have (N62), by the
/// runtime part each symbol belongs to, in a fixed order: every symbol a unit declares that no
/// unit and not the board's runtime defines, apart from LLVM's intrinsics and the program's own
/// foreign functions, which `--link` supplies. Read from the generated text, so it is exactly
/// what the link would leave unresolved — a static string printed is a C library call there.
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
