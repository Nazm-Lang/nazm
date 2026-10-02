//! `nazm build` — source to a native executable.
//!
//! The pipeline is the existing front end, then Core IR lowered to MIR and validated
//! (`nazm_mir`, N40), then `nazm_lir::lower` laying MIR out into one unit per module and per
//! generic instance, `nazm_lir::emit_unit` writing each as LLVM IR text, and `clang` turning
//! that into a binary. No LLVM is linked into `nazm` —
//! `docs/architecture.md` §1 says not to in year one, and `clang` reads IR from a file.
//!
//! # Publishing the result
//!
//! Everything is built under temporary names in the output's own directory and the
//! executable is moved into place only after `clang` succeeds. So a failed build leaves
//! an existing binary exactly as it was, rather than deleting it and then failing — which
//! is the behaviour that costs someone a working artefact at the worst moment.
//!
//! The output path is refused if it is the input, because a build that overwrote its own
//! source would be unrecoverable.
//!
//! # What is reused, and what is not
//!
//! Every stage above `clang -c` runs on every build: the files are read, parsed, checked,
//! admitted, lowered and emitted, every time. What is reused is the one step that costs
//! more than all of them together — turning one LLVM unit into one object — and it is
//! reused only when the exact bytes handed to the object compiler, the configuration that
//! compiler resolved, and the compiler itself are all the ones a previous build recorded.
//! `crate::backend` owns the first three and `nazm_cache::object` owns the key.
//!
//! The link runs on every build too. There is no executable cache, no link-result cache
//! and no incremental linker, and `docs/capability-matrix.md` says so in those words.
//!
//! # Why the reuse cannot skip anything it must not
//!
//! A key is a digest of an artefact, so the artefact has to exist before the key does —
//! which means everything required to produce it has already run, including the
//! whole-program admissibility that refuses cross-module recursion. A fully warm store
//! still checks, still plans, still lowers and still refuses. That is not a rule this file
//! follows; it is the shape of the pipeline.

use nazm_service::summary::Status;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_cache::{LlvmInput, ObjectArtifact, ObjectFound, ObjectKey, ObjectStore};

use crate::backend::{Backend, Outcome, DRIVER};
use crate::{emit, exit};

/// What one build did with the native cache, if anyone asked.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Native {
    /// LLVM units emitted. Always every unit: emission is not cached.
    units: usize,
    /// Units handed to `clang -c`.
    compiled: usize,
    /// Units whose object came from the store.
    reused: usize,
    /// Units compiled because what was filed could not be used.
    rejected: usize,
    /// Objects written to the store this run.
    stored: usize,
    /// Generic function instances: how many units there were, and of those how many were
    /// compiled and how many reused. Already counted in the totals above; broken out
    /// because the question *what did one more instantiation cost* is about these alone.
    instances: usize,
    instances_compiled: usize,
    instances_reused: usize,
}

/// Compile, or explain what stopped it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    input: &Path,
    out: Option<&Path>,
    json: bool,
    emit_ir: bool,
    opt_level: u8,
    cache: bool,
    with_report: bool,
    summary: bool,
    choice: Choice<'_>,
) -> ExitCode {
    // `--summary-json` (N31): at every exit below, the outcome reported as one
    // `nazm.command-summary/1` on stdout in place of the rendering and the path. The exit
    // status is the same, and so is everything the build writes.
    let summarized = |status, code, diagnostics: &[nazm_diag::Diagnostic], map, artifact| {
        crate::summary::finish(
            nazm_service::summary::Command::Build,
            status,
            code,
            diagnostics,
            map,
            artifact,
            input,
            None,
        )
    };
    // Discovered, never given: `nazm build` takes no `--source-root`, and the marker is
    // how a project declares one. It decides every module's durable identity, which is
    // the name the generated code carries and therefore part of what an object is keyed
    // on.
    let source_root = match crate::root::decide(input, None) {
        Ok(r) => r,
        Err(e) => {
            if summary {
                return summarized(Status::Unreadable, exit::NO_SUCH_FILE, &[], None, None);
            }
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    // The target and the backend, settled before anything is compiled (N41). A target that
    // is not this host is refused: an object for it would need a linker and a runtime v1
    // does not have, and building for the host instead would be a silent lie.
    let target = match nazm_lir::backend::Target::requested(choice.target) {
        Ok(t) => t,
        Err(e) => {
            if summary {
                return summarized(Status::Refused, exit::TOOLCHAIN_FAILED, &[], None, None);
            }
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    // A foreign object that is not there is refused before anything is compiled, by name,
    // rather than reported by the linker as an undefined symbol after the whole build (N42).
    if let Some(missing) = choice.link.iter().find(|p| !p.is_file()) {
        if summary {
            return summarized(Status::Refused, exit::TOOLCHAIN_FAILED, &[], None, None);
        }
        eprintln!(
            "nazm: `--link {}`: no such file; `--link` names an object or library that \
             defines foreign functions",
            missing.display()
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // Debug information is the LLVM backend's (N48): refused, rather than silently dropped,
    // for Cranelift.
    if choice.debug && choice.backend == nazm_lir::backend::BackendId::Cranelift {
        eprintln!(
            "nazm: `--debug` carries debug information through the LLVM backend only; \
             `--backend cranelift` writes none"
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // A freestanding image is the LLVM backend's (N62): Cranelift's object writer knows no
    // binary format for a target without an operating system, so it is refused by name here
    // rather than by Cranelift after the program is lowered.
    if choice.backend == nazm_lir::backend::BackendId::Cranelift
        && target.os == nazm_lir::backend::Os::None
    {
        if summary {
            return summarized(Status::Refused, exit::TOOLCHAIN_FAILED, &[], None, None);
        }
        eprintln!(
            "nazm: `--backend cranelift` builds no freestanding target; `{}` is the LLVM \
             backend's",
            target.triple
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // The watermark is the board's (N63): a hosted program has an operating system's stack.
    if choice.stack_watermark && !target.is_board() {
        if summary {
            return summarized(Status::Refused, exit::TOOLCHAIN_FAILED, &[], None, None);
        }
        eprintln!(
            "nazm: `--stack-watermark` measures the board's stack; it needs \
             `--target aarch64-unknown-none`"
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // A target other than the host (N57): clang is told it, and where the host cannot link it
    // the build writes objects instead, or is refused by name before compiling anything.
    let host = nazm_lir::backend::Target::host().ok();
    let cross = (host.as_ref() != Some(&target)).then(|| target.triple.clone());
    let links_here = host.as_ref().is_some_and(|h| target.links_on(h));
    if !links_here && choice.objects.is_none() && !choice.library {
        if summary {
            return summarized(Status::Refused, exit::TOOLCHAIN_FAILED, &[], None, None);
        }
        eprintln!(
            "nazm: this host has no linker for `{}`; build it with `--objects DIR`, which writes \
             every object and `DIR/link.txt` for that target's own toolchain to link",
            target.triple
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let llvm = nazm_lir::backend::LlvmText::new(target.clone()).with_debug(choice.debug);
    let cranelift = nazm_codegen_clif::Cranelift::new(target.clone(), opt_level);
    let chosen: &(dyn nazm_lir::backend::Backend + Sync) = match choice.backend {
        nazm_lir::backend::BackendId::Llvm => &llvm,
        nazm_lir::backend::BackendId::Cranelift => &cranelift,
    };
    // Objects a backend can key before generating them are looked up by that key, so a
    // reused unit is not generated at all. Keyed apart from clang's: a Cranelift object is a
    // function of its MIR and Cranelift's settings, not of any C compiler.
    let clif_store: Option<ObjectStore> =
        cache.then(|| crate::derived::objects(source_root.path()));
    let clif_key = |k: &str| {
        ObjectKey::of(
            LlvmInput::of(k.as_bytes()),
            nazm_cache::BackendIdentity::of(chosen.id().name(), &chosen.version()),
            nazm_cache::CodegenConfig::of(&[chosen.target().triple.clone()]),
        )
    };
    let clock = Clock::start();
    let loaded = match crate::load::load(input, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            if summary {
                return summarized(
                    Status::Unreadable,
                    exit::NO_SUCH_FILE,
                    &diagnostics,
                    Some(&partial.map),
                    None,
                );
            }
            emit(&diagnostics, &partial.map, json);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let map = &loaded.map;

    let output = match out {
        Some(p) => p.to_path_buf(),
        // `prog.nz` → `prog`. A stem with no extension would collide with its source, and
        // the check below catches that rather than this guessing around it.
        None => PathBuf::from(input.file_stem().unwrap_or_default()),
    };

    if same_file(input, &output) {
        if summary {
            return summarized(
                Status::Refused,
                exit::TOOLCHAIN_FAILED,
                &[],
                Some(map),
                None,
            );
        }
        eprintln!(
            "nazm: refusing to write the executable over its own source ({}). \
             Pass -o with a different path.",
            output.display()
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }

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
    // A freestanding target (N62) has no operating system under it: the runtime parts a
    // program reaches beyond the board's, named, refuse the build after code generation.
    let freestanding = target.os == nazm_lir::backend::Os::None;
    let mut unhosted: Vec<String> = Vec::new();
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
            let program = crate::mir::lower(core, resolution, &loaded.graph)?;
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
                }),
            );
            clock.mark("codegen");
            made
        },
    ) {
        Ok(_) if !unhosted.is_empty() => {
            if summary {
                return summarized(
                    Status::Refused,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    Some(map),
                    None,
                );
            }
            eprintln!(
                "nazm: `{}` has no operating system, and this program reaches what only a hosted \
                 runtime has: {}; \
                 a freestanding program computes with `Int` and `Bool`, functions, loops, static \
                 strings and, on the board, device registers",
                target.triple,
                unhosted.join(", ")
            );
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        Ok(a) => a,
        Err(diagnostics) => {
            // `N0101` is this backend's "not yet", and it reads differently from a
            // program being wrong. Distinguished for the summary line only; both are a
            // check failure and neither produces output.
            if summary {
                return summarized(
                    Status::Rejected,
                    exit::CHECK_FAILED,
                    &diagnostics,
                    Some(map),
                    None,
                );
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
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };

    // Temporaries beside the output, so the final move is a rename within one filesystem
    // rather than a copy that can half-succeed. One directory for all of them, because
    // there is one per module now and a program with forty modules should not put forty
    // pairs of files beside somebody's binary.
    let stamp = std::process::id();
    let work = sibling(&output, &format!(".nazm-{stamp}"));
    // Linked inside the working directory under the output's own file name (N46): the
    // Mach-O linker writes that name into the executable's code signature and derives its
    // UUID from it, so a temporary name would make two builds of one program differ.
    let tmp_exe = link_name(&work, &output);
    if let Err(e) = std::fs::create_dir_all(&work) {
        eprintln!("nazm: cannot create {}: {e}", work.display());
        if summary {
            return summarized(
                Status::ToolchainFailed,
                exit::TOOLCHAIN_FAILED,
                &[],
                Some(map),
                None,
            );
        }
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }

    // Removes what this build made, and only that: the working directory it created and
    // the executable it was assembling. Nothing else is scanned and nothing else is
    // touched.
    let cleanup = |keep: bool| {
        if !keep {
            let _ = std::fs::remove_dir_all(&work);
        }
        let _ = std::fs::remove_file(&tmp_exe);
    };

    // Asked once, before anything is compiled: what this backend is, and what it would
    // do. `crate::backend` says why the answer comes from the compiler rather than from
    // the three arguments this one passes.
    let backend = if cache {
        Backend::discover(opt_level, &work, cross.as_deref())
    } else {
        Backend::unidentified(opt_level, cross.as_deref())
    };
    // A backend that would not say what it is gets no cache. Guessing a configuration is
    // how one target's objects end up in another's build.
    //
    // Nor does a board build (N63): its stack bound reads the frame sizes clang reports while
    // compiling, which a reused object would not report.
    let store: Option<ObjectStore> = backend
        .settled()
        .filter(|_| !freestanding)
        .map(|_| crate::derived::objects(source_root.path()));
    let mut frames = std::collections::BTreeMap::new();

    let mut tally = Native {
        units: artefacts.len(),
        instances: artefacts.iter().filter(|a| a.instance).count(),
        ..Native::default()
    };
    let mut objects: Vec<PathBuf> = Vec::new();
    for artefact in &artefacts {
        let ll = work.join(format!("{}.ll", artefact.name));
        let obj = work.join(format!("{}.o", artefact.name));

        // An object a backend produced itself: written down, and stored under the key its
        // backend gave it, unless it came from the store.
        let ir = match &artefact.code {
            nazm_lir::backend::Code::Llvm(ir) => ir,
            nazm_lir::backend::Code::Object(bytes) => {
                if let Err(e) = std::fs::write(&obj, bytes) {
                    cleanup(false);
                    eprintln!("nazm: cannot write {}: {e}", obj.display());
                    if summary {
                        return summarized(
                            Status::ToolchainFailed,
                            exit::TOOLCHAIN_FAILED,
                            &[],
                            Some(map),
                            None,
                        );
                    }
                    return ExitCode::from(exit::TOOLCHAIN_FAILED);
                }
                if artefact.reused {
                    tally.reused += 1;
                    if artefact.instance {
                        tally.instances_reused += 1;
                    }
                } else {
                    tally.compiled += 1;
                    if artefact.instance {
                        tally.instances_compiled += 1;
                    }
                    if let (Some(store), Some(k)) = (clif_store.as_ref(), artefact.key.as_deref()) {
                        let entry = ObjectArtifact::of(
                            LlvmInput::of(k.as_bytes()),
                            nazm_cache::BackendIdentity::of(chosen.id().name(), &chosen.version()),
                            nazm_cache::CodegenConfig::of(&[chosen.target().triple.clone()]),
                            bytes.clone(),
                        );
                        if entry.key() == Some(clif_key(k)) && store.put(&entry).is_ok() {
                            tally.stored += 1;
                        }
                    }
                }
                objects.push(obj);
                continue;
            }
        };

        // The key is over the bytes that are about to be handed over, so it cannot
        // describe anything but what is compiled.
        let key = backend
            .settled()
            .map(|s| ObjectKey::of(LlvmInput::of(ir.as_bytes()), s.identity, s.config));
        if let (Some(store), Some(key)) = (store.as_ref(), key) {
            match store.get(&key) {
                ObjectFound::Hit(bytes) => {
                    // Materialised into this build's own directory. A cached entry is an
                    // envelope, not an object, and it is never written to or linked in
                    // place — it is immutable derived data.
                    if std::fs::write(&obj, &bytes).is_ok() {
                        tally.reused += 1;
                        if artefact.instance {
                            tally.instances_reused += 1;
                        }
                        objects.push(obj);
                        continue;
                    }
                }
                ObjectFound::Miss => {}
                ObjectFound::Rejected(_) => tally.rejected += 1,
            }
        }

        if let Err(e) = std::fs::write(&ll, ir) {
            cleanup(false);
            eprintln!("nazm: cannot write {}: {e}", ll.display());
            if summary {
                return summarized(
                    Status::ToolchainFailed,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    Some(map),
                    None,
                );
            }
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        let compiled = if freestanding {
            backend.compile_reporting_frames(&ll, &obj)
        } else {
            backend.compile(&ll, &obj)
        };
        match compiled {
            Outcome::Ran(o) if o.status.success() => {
                crate::stack::frames(&String::from_utf8_lossy(&o.stderr), &mut frames);
                tally.compiled += 1;
                if artefact.instance {
                    tally.instances_compiled += 1;
                }
            }
            other => {
                let code = report_clang(other, &ll, cleanup);
                if summary {
                    return summarized(
                        Status::ToolchainFailed,
                        exit::TOOLCHAIN_FAILED,
                        &[],
                        Some(map),
                        None,
                    );
                }
                return code;
            }
        }

        // Published after it exists and from the bytes that were produced, so what is
        // stored is what was linked. A store that will not take an entry is not a reason
        // to fail a build: the disk is full, or the tree is read-only, and the program is
        // fine either way.
        if let (Some(store), Some(key), Some(settled), Ok(bytes)) =
            (store.as_ref(), key, backend.settled(), std::fs::read(&obj))
        {
            let entry = ObjectArtifact::of(
                LlvmInput::of(ir.as_bytes()),
                settled.identity,
                settled.config,
                bytes,
            );
            if entry.key() == Some(key) && store.put(&entry).is_ok() {
                tally.stored += 1;
            }
        }
        objects.push(obj);
    }

    // Deterministic: the units are in load order and the support artefacts last, so the
    // command line does not depend on anything the filesystem decided.
    objects.extend(choice.link.iter().cloned());
    clock.mark("native_compile");
    // Objects for the target's own toolchain (N57): every object in link order, renamed by its
    // position so the order survives, and `link.txt` saying what to link them with.
    if let Some(dir) = choice.objects {
        let mut written = write_objects(dir, &objects, &target);
        if freestanding && written.is_ok() {
            let texts: Vec<&str> = artefacts
                .iter()
                .filter_map(|a| match &a.code {
                    nazm_lir::backend::Code::Llvm(ir) => Some(ir.as_str()),
                    nazm_lir::backend::Code::Object(_) => None,
                })
                .collect();
            let calls = crate::stack::calls(&texts);
            // The image's root: the board's entry, or the module's one export (N64).
            let (root, reserved) = if target.is_wasm() {
                ("nazm_main", nazm_lir::runtime::wasm::STACK)
            } else {
                ("nz_board_main", nazm_lir::runtime::board::STACK)
            };
            let bound = crate::stack::bound(root, &frames, &calls);
            written = std::fs::write(
                dir.join("bounds.json"),
                crate::stack::report(&target.triple, &bound, &frames, reserved),
            );
        }
        cleanup(false);
        return match written {
            Ok(()) => {
                if with_report {
                    println!(
                        "{}",
                        report(source_root.how(), map.len(), tally).replace(
                            r#""link":{"performed":true}"#,
                            r#""link":{"performed":false}"#
                        )
                    );
                }
                if !summary {
                    eprintln!(
                        "nazm: wrote {} objects for {} to {}; link them as {}/link.txt says",
                        objects.len(),
                        target.triple,
                        dir.display(),
                        dir.display()
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("nazm: cannot write {}: {e}", dir.display());
                ExitCode::from(exit::TOOLCHAIN_FAILED)
            }
        };
    }
    let linked = if choice.library {
        archive(&tmp_exe, &objects)
    } else {
        backend.link(&tmp_exe, &objects)
    };
    match linked {
        Outcome::Ran(o) if o.status.success() => {}
        Outcome::Ran(o) if !foreign.is_empty() => {
            // A program that calls C links against what `--link` named, so an unresolved
            // symbol is most likely the user's, not the compiler's: say which ones it may be.
            cleanup(false);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!(
                "nazm: linking failed. This program calls the foreign function{} {}; each \
                 must be defined by an object or library passed with `--link FILE`",
                if foreign.len() == 1 { "" } else { "s" },
                foreign
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if summary {
                return summarized(
                    Status::ToolchainFailed,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    Some(map),
                    None,
                );
            }
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        other => {
            let code = report_clang(other, &work, cleanup);
            if summary {
                return summarized(
                    Status::ToolchainFailed,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    Some(map),
                    None,
                );
            }
            return code;
        }
    }

    if emit_ir {
        // Beside the output, one file per unit, named as the artefacts are.
        for artefact in &artefacts {
            let nazm_lir::backend::Code::Llvm(ir) = &artefact.code else {
                continue;
            };
            let beside = with_suffix(&output, &format!("{}.ll", artefact.name));
            if let Err(e) = std::fs::write(&beside, ir) {
                eprintln!("nazm: cannot write {}: {e}", beside.display());
            }
        }
    }
    // On macOS the linker leaves debug information in the objects it read, which are about
    // to go, so a debug build gathers it into `OUTPUT.dSYM` first (N48). Elsewhere it is in
    // the executable already.
    if gathers_debug_information(choice.debug, std::env::consts::OS) {
        let dsym = with_suffix(&output, "dSYM");
        if let Err(e) = backend.gather_debug_info(&tmp_exe, &dsym) {
            eprintln!("nazm: the debug information was not gathered: {e}");
        }
    }
    // A library's header, beside it (N55): written before the archive is published, so a
    // published archive always has its header.
    if choice.library {
        let header = output.with_extension("h");
        if let Err(e) = std::fs::write(&header, c_header(&output, &exported)) {
            cleanup(false);
            eprintln!("nazm: cannot write {}: {e}", header.display());
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    // Published last, so an existing executable survives every failure above — and then the
    // intermediates go, the working directory the executable was linked in included.
    let published = std::fs::rename(&tmp_exe, &output);
    let _ = std::fs::remove_dir_all(&work);
    if let Err(e) = published {
        eprintln!("nazm: cannot write {}: {e}", output.display());
        if summary {
            return summarized(
                Status::ToolchainFailed,
                exit::TOOLCHAIN_FAILED,
                &[],
                Some(map),
                None,
            );
        }
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    if summary {
        return summarized(
            Status::Success,
            exit::OK,
            &[],
            Some(map),
            Some(output.display().to_string()),
        );
    }
    clock.mark("link");
    if with_report {
        println!("{}", report(source_root.how(), map.len(), tally));
    }
    if choice.timings {
        eprintln!("{}", clock.report());
    }
    println!("{}", output.display());
    ExitCode::from(exit::OK)
}

/// Where a build's time went (N48): each phase's wall time since the one before, in the order
/// the build ran them. `--timings` prints them as `nazm.timings/1`, one line on stderr.
struct Clock {
    start: std::time::Instant,
    last: std::sync::Mutex<(std::time::Instant, Vec<(&'static str, f64)>)>,
}

impl Clock {
    fn start() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            last: std::sync::Mutex::new((now, Vec::new())),
        }
    }

    fn mark(&self, phase: &'static str) {
        if let Ok(mut g) = self.last.lock() {
            let now = std::time::Instant::now();
            let ms = now.duration_since(g.0).as_secs_f64() * 1000.0;
            g.1.push((phase, ms));
            g.0 = now;
        }
    }

    fn report(&self) -> String {
        let total = self.start.elapsed().as_secs_f64() * 1000.0;
        let phases = self
            .last
            .lock()
            .map(|g| {
                g.1.iter()
                    .map(|(p, ms)| format!("{{\"phase\":\"{p}\",\"ms\":{ms:.3}}}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        format!("{{\"schema\":\"nazm.timings/1\",\"phases\":[{phases}],\"total_ms\":{total:.3}}}")
    }
}

/// One `nazm.build-report/1` object.
///
/// Structured because the reader is as likely to be a script or an agent as a person, and
/// versioned because anything a script reads is an interface. Off unless asked for: a
/// compiler that reports on itself on every run is a compiler whose output has to be
/// filtered before it can be read.
///
/// # Why `semantic.reused` is always zero here
///
/// It is not an oversight and it is not a placeholder. `nazm check` may skip a module's
/// bodies because all it needs afterwards is the interface; `nazm build` needs the
/// *resolution* those bodies produce, because that is what lowering reads, and nothing
/// persists a resolution. So a build checks everything, every time, and says so rather
/// than leaving a reader to infer it from a missing field. `docs/roadmap.md` records what
/// reusing it would require.
///
/// # Why `link.performed` is always true
///
/// Same reason, stated the same way. The report is printed after a successful link, and
/// the link runs on every build: there is no executable cache and no incremental linker.
fn report(root: crate::root::How, modules: usize, t: Native) -> String {
    format!(
        r#"{{"schema":"nazm.build-report/1","root":"{}","semantic":{{"checked":{modules},"reused":0}},"native":{{"units":{},"compiled":{},"reused":{},"rejected":{},"stored":{},"instances":{{"units":{},"compiled":{},"reused":{}}}}},"link":{{"performed":true}}}}"#,
        root.as_str(),
        t.units,
        t.compiled,
        t.reused,
        t.rejected,
        t.stored,
        t.instances,
        t.instances_compiled,
        t.instances_reused,
    )
}

/// What `nazm build` was asked to generate code with (N41).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Choice<'a> {
    pub(crate) backend: nazm_lir::backend::BackendId,
    /// An explicit target triple, which must be the host's.
    pub(crate) target: Option<&'a str>,
    /// Foreign objects and libraries for the linker (N42), in the order given.
    pub(crate) link: &'a [std::path::PathBuf],
    /// A static library and its header, not an executable (N55).
    pub(crate) library: bool,
    /// Objects for the target's own toolchain, not an executable (N57).
    pub(crate) objects: Option<&'a Path>,
    /// Measure the board's stack on the board (N63).
    pub(crate) stack_watermark: bool,
    /// Print where the build's time went (N48).
    pub(crate) timings: bool,
    /// Carry debug information, for a debugger (N48).
    pub(crate) debug: bool,
}

/// A freestanding build's choices (N62, N64): where the parts it cannot have are named, whether
/// the board measures its stack, and whether the image is WebAssembly rather than the board.
struct Freestanding<'a> {
    unhosted: &'a mut Vec<String>,
    watermark: bool,
    wasm: bool,
}

/// One generated native unit, ready to be written down.
struct Artefact {
    /// The file name to use, without an extension.
    ///
    /// Derived from the module's position in load order rather than from its source path:
    /// a path is not a filename — it has separators, it may not be relative, and it is
    /// **not this compiler's to trust**. `docs/architecture.md` records it as build
    /// plumbing, deliberately not as an identity.
    name: String,
    /// LLVM text for clang, or an object a backend produced itself.
    code: nazm_lir::backend::Code,
    /// Whether this is a generic function instance's unit.
    instance: bool,
    /// For an object: the key its backend gave it before generating it.
    key: Option<String>,
    /// Whether the object came from the store under that key rather than being generated.
    reused: bool,
}

/// Lower a checked compilation into one artefact per module, plus the program's support.
///
/// The order is load order, then the runtime, then the entry. Nothing here reads a
/// directory, so the same program produces the same artefacts in the same order. A unit the
/// backend can key before generating it, and whose object is in the store, is not generated.
fn emit_artefacts(
    native: &nazm_lir::Native,
    names: &nazm_lir::SourceNames<'_>,
    backend: &dyn nazm_lir::backend::Backend,
    cached: &dyn Fn(&str) -> Option<Vec<u8>>,
    library: bool,
    freestanding: Option<Freestanding<'_>>,
) -> Result<Vec<Artefact>, Vec<nazm_diag::Diagnostic>> {
    let mut out = Vec::new();
    let mut needs = backend.baseline();
    let mut errors = Vec::new();
    // One artefact per module and one per generic function instance, never folded into a
    // caller's: the object cache then reuses each by its own identity, one instance
    // requested by two modules is one object, and a module's object does not change when
    // another module starts instantiating something. `docs/architecture.md` §7.12.
    for (i, plan) in backend.plan(native).into_iter().enumerate() {
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
    }) = freestanding
    {
        // WebAssembly's runtime reaches its host through imports (N64, §7.66).
        let runtime = if wasm {
            nazm_lir::runtime::wasm::emit_runtime(needs.stack)
        } else {
            nazm_lir::runtime::board::emit_runtime(needs.stack)
        };
        let foreign: Vec<&str> = native
            .sigs
            .iter()
            .filter(|s| s.foreign)
            .map(|s| s.symbol.as_str())
            .collect();
        refused.extend(freestanding_refusals(&out, &runtime, &foreign));
        if !refused.is_empty() {
            return Ok(out);
        }
        out.push(Artefact {
            name: "runtime".to_owned(),
            code: nazm_lir::backend::Code::Llvm(runtime),
            instance: false,
            key: None,
            reused: false,
        });
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
    // modules reached — a whole-program fact, and the only one emission needs.
    out.push(Artefact {
        name: "runtime".to_owned(),
        code: nazm_lir::backend::Code::Llvm(nazm_lir::emit_runtime(needs)),
        instance: false,
        key: None,
        reused: false,
    });
    // A library has no process entry: C's `main` is the caller's (N55).
    if library {
        return Ok(out);
    }
    out.push(Artefact {
        name: "entry".to_owned(),
        code: nazm_lir::backend::Code::Llvm(nazm_lir::emit_entry(
            &native.entry,
            native.entry_roots,
            needs,
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

/// Explain a `clang` failure, keeping the IR when its complaint is about it.
fn report_clang(outcome: Outcome, kept: &Path, cleanup: impl Fn(bool)) -> ExitCode {
    match outcome {
        Outcome::Missing => {
            cleanup(false);
            eprintln!(
                "nazm: `{DRIVER}` was not found on PATH, and `nazm build` needs it to \
                 assemble the LLVM IR it generates.\n\
                 nazm: install the Xcode command line tools (`xcode-select --install`) \
                 or your platform's clang package. `nazm run` needs none of this."
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Outcome::Failed(e) => {
            cleanup(false);
            eprintln!("nazm: could not run `{DRIVER}`: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Outcome::Ran(o) => {
            // Keep the IR: clang's complaint is about a file the user cannot see
            // otherwise, and a compiler bug is much easier to report with it in hand.
            cleanup(true);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!(
                "nazm: `{}` rejected the generated IR (exit {:?}). This is a bug in \
                 `nazm build`, not in your program — the IR has been kept at {} to \
                 report it with.",
                DRIVER,
                o.status.code(),
                kept.display()
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}

/// Whether two paths name the same file, falling back to a textual comparison.
///
/// `canonicalize` fails on a path that does not exist yet, which is the normal case for
/// an output — so a plain comparison is the fallback rather than an error.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn sibling(output: &Path, suffix: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    output.with_file_name(name)
}

/// Where the executable is linked: inside the working directory, under the output's own file
/// name, which the Mach-O linker writes into the executable (N46).
fn link_name(work: &Path, output: &Path) -> PathBuf {
    work.join(output.file_name().unwrap_or_default())
}

/// Whether a build gathers its debug information beside the executable before the objects
/// holding it are removed: a debug build on macOS, whose linker leaves it in the objects (N48).
fn gathers_debug_information(debug: bool, os: &str) -> bool {
    debug && os == "macos"
}

/// `DIR/NN-NAME.o` for each object in link order, and `DIR/link.txt` (N57): the target, the
/// objects in order, and the system libraries the runtime needs on it.
fn write_objects(
    dir: &Path,
    objects: &[PathBuf],
    target: &nazm_lir::backend::Target,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut listed = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        let name = o
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let to = format!("{i:02}-{name}");
        std::fs::copy(o, dir.join(&to))?;
        listed.push(to);
    }
    let libs = match target.os {
        nazm_lir::backend::Os::Linux => "-lpthread".to_owned(),
        nazm_lir::backend::Os::Macos => String::new(),
        // A WebAssembly module links nothing but itself, by `wasm-ld` (N64).
        nazm_lir::backend::Os::None if target.is_wasm() => {
            format!("none; {}", nazm_lir::runtime::wasm::LINK)
        }
        // A freestanding image links nothing but itself, by `link.ld` (N62).
        nazm_lir::backend::Os::None => "none; ld.lld -T link.ld".to_owned(),
    };
    let text = format!(
        "# Written by nazm build --objects. Link these, in this order, with the target's toolchain.\n\
         target {}\nobjects {}\nlibraries {libs}\n",
        target.triple,
        listed.join(" ")
    );
    std::fs::write(dir.join("link.txt"), text)?;
    // A module's reference host (N64): exactly the imports it may grant.
    if target.is_wasm() {
        std::fs::write(dir.join("host.mjs"), nazm_lir::runtime::wasm::HOST)?;
    }
    // The board's memory map, for `ld.lld -T link.ld` (N62).
    if target.is_board() {
        std::fs::write(
            dir.join("link.ld"),
            nazm_lir::runtime::board::linker_script(),
        )?;
    }
    Ok(())
}

/// A static archive of `objects` (N55), with the system `ar` and the timestamps zeroed so two
/// builds of one library are the same bytes.
fn archive(output: &Path, objects: &[PathBuf]) -> Outcome {
    let mut c = Command::new("ar");
    c.env("ZERO_AR_DATE", "1")
        .arg("rcs")
        .arg(output)
        .args(objects);
    crate::backend::run_command(&mut c)
}

/// A library's C header (N55): every export, by symbol, in `<stdint.h>` and `<stdbool.h>` types.
fn c_header(output: &Path, exported: &[(String, Vec<nazm_lir::Ty>, nazm_lir::Ty)]) -> String {
    let c = |t: &nazm_lir::Ty| {
        if *t == nazm_lir::Ty::Bool {
            "bool"
        } else {
            "int64_t"
        }
    };
    let stem: String = output
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let guard = format!("NAZM_{stem}_H");
    let mut s = String::new();
    s.push_str("/* Generated by nazm build --lib. Do not edit. */\n");
    s.push_str(&format!("#ifndef {guard}\n#define {guard}\n\n"));
    s.push_str("#include <stdbool.h>\n#include <stdint.h>\n\n");
    s.push_str("#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");
    s.push_str(
        "/* A Nazm failure inside any of these is reported on standard error and ends the\n   \
         process with status 2: nothing unwinds into C. */\n",
    );
    for (sym, params, ret) in exported {
        let ps = if params.is_empty() {
            "void".to_owned()
        } else {
            params
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{} a{i}", c(t)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        s.push_str(&format!("{} {sym}({ps});\n", c(ret)));
    }
    s.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    s
}

fn with_suffix(output: &Path, ext: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    output.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_executable_is_linked_under_its_own_name_inside_the_working_directory() {
        let work = Path::new("/tmp/build/.nazm-7");
        let at = link_name(work, Path::new("/tmp/build/prog"));
        assert_eq!(at, Path::new("/tmp/build/.nazm-7/prog"));
    }

    #[test]
    fn only_a_debug_build_on_macos_gathers_its_debug_information() {
        assert!(gathers_debug_information(true, "macos"));
        assert!(!gathers_debug_information(false, "macos"));
        assert!(!gathers_debug_information(true, "linux"));
    }
}
