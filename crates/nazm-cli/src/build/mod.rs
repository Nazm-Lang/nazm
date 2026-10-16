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

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_cache::{LlvmInput, ObjectArtifact, ObjectFound, ObjectKey};

use crate::backend::{Backend, Outcome, DRIVER};
use crate::{emit, exit};

mod link;
mod lower;
mod objects;
mod preflight;
mod release;
mod stores;
mod timing;
mod tls;

use stores::Stores;

pub(crate) use release::link_args as release_link_args;

use self::preflight::{preflight, Settled};
pub(crate) use preflight::{board_for, board_of, read_board, switch_target};

use link::{link_name, linked, published, sibling};
use lower::{lowered, Lowered};
use nazm_service::summary::Status;
use objects::{compiled, objects_for_target, Compiled};
use preflight::admitted;
use timing::Clock;

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
    let summarized = |status: Status,
                      code: u8,
                      diagnostics: &[nazm_diag::Diagnostic],
                      map: Option<&nazm_span::SourceMap>,
                      artifact: Option<String>| {
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
    let Settled {
        source_root,
        target,
        soa,
        cross,
        heap_limit,
    } = match preflight(input, choice, summary, &summarized) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let llvm = nazm_lir::backend::LlvmText::new(target.clone())
        .with_debug(choice.debug)
        .with_soa(soa)
        .with_counted_heap(heap_limit.is_some());
    // Both backends write debug information (N48; Cranelift's since N91), and both allocate
    // through the counted heap under a heap limit (R1d, D5).
    let cranelift = nazm_codegen_clif::Cranelift::new(target.clone(), opt_level)
        .with_debug(choice.debug)
        .with_soa(soa)
        .with_counted_heap(heap_limit.is_some());
    // Nazm's own code generator (R9, D15): `nzcg`, found when first needed.
    let nzcg = nazm_codegen_nazm::NazmCodegen::new(target.clone())
        .with_debug(choice.debug)
        .with_soa(soa)
        .with_counted_heap(heap_limit.is_some());
    let chosen: &(dyn nazm_lir::backend::Backend + Sync) = match choice.backend {
        nazm_lir::backend::BackendId::Llvm => &llvm,
        nazm_lir::backend::BackendId::Cranelift => &cranelift,
        nazm_lir::backend::BackendId::Nazm => &nzcg,
    };
    // Objects a backend can key before generating them are looked up by that key, so a
    // reused unit is not generated at all. Keyed apart from clang's: a Cranelift object is a
    // function of its MIR and Cranelift's settings, not of any C compiler.
    let clif_store = Stores {
        project: cache.then(|| crate::derived::objects(source_root.path())),
        user: cache.then(crate::derived::user_objects).flatten(),
    };
    let clif_key = |k: &str| {
        ObjectKey::of(
            LlvmInput::of(k.as_bytes()),
            nazm_cache::BackendIdentity::of(chosen.id().name(), &chosen.version()),
            nazm_cache::CodegenConfig::of(&[chosen.target().triple.clone()]),
        )
    };
    let clock = Clock::start();
    let (loaded, output) =
        match admitted(input, out, json, choice, &source_root, summary, &summarized) {
            Ok(a) => a,
            Err(code) => return code,
        };
    let map = &loaded.map;
    // A freestanding target (N62) has no operating system under it: the runtime parts a
    // program reaches beyond the board's, named, refuse the build after code generation.
    // WASI (Gate 3) is WebAssembly's freestanding path with a heap and WASI's imports.
    let freestanding = matches!(
        target.os,
        nazm_lir::backend::Os::None | nazm_lir::backend::Os::Wasi
    );
    let Lowered {
        artefacts,
        foreign,
        exported,
        reached,
    } = match lowered(
        &loaded,
        json,
        choice,
        &target,
        freestanding,
        chosen,
        &clif_store,
        &clif_key,
        &clock,
        heap_limit,
        summary,
        &summarized,
    ) {
        Ok(l) => l,
        Err(code) => return code,
    };

    // A runtime artifact (N82, §7.83): checked against this compiler, the target and everything
    // the program reached before any file is written; refused by name, never replaced by a
    // generated runtime.
    let prebuilt = match choice.runtime {
        None => None,
        Some(dir) => match crate::runtime_artifact::fits(dir, &target, Some(reached)) {
            Ok(f) => Some(f.archive),
            Err((code, why)) => {
                if summary {
                    return summarized(Status::Refused, code, &[], Some(map), None);
                }
                eprintln!("nazm: {why}");
                return ExitCode::from(code);
            }
        },
    };

    // Temporaries beside the output, so the final move is a rename within one filesystem
    // rather than a copy that can half-succeed. One directory for all of them, because
    // there is one per module now and a program with forty modules should not put forty
    // pairs of files beside somebody's binary.
    // Made exclusively, mode 0700: a directory or symlink already at the name is never built
    // in (Q1-A1-03).
    let stem = format!(
        "{}.nazm-",
        output.file_name().unwrap_or_default().to_string_lossy()
    );
    let parent = output.parent().unwrap_or_else(|| Path::new(""));
    let made = crate::scratch::exclusive(parent, &stem);
    let work = made
        .as_ref()
        .map_or_else(|_| sibling(&output, ".nazm"), Clone::clone);
    // Linked inside the working directory under the output's own file name (N46): the
    // Mach-O linker writes that name into the executable's code signature and derives its
    // UUID from it, so a temporary name would make two builds of one program differ.
    let tmp_exe = link_name(&work, &output);
    if let Err(e) = made {
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

    let Compiled {
        backend,
        mut objects,
        tally,
        frames,
    } = match compiled(
        &artefacts,
        &work,
        opt_level,
        cache,
        cross.as_deref(),
        freestanding,
        &source_root,
        chosen,
        &clif_store,
        &clif_key,
        &cleanup,
        map,
        summary,
        &summarized,
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    // Deterministic: the units are in load order and the support artefacts last, so the
    // command line does not depend on anything the filesystem decided.
    // The runtime artifact after every object that calls it, and before the foreign inputs.
    objects.extend(prebuilt);
    objects.extend(choice.link.iter().cloned());
    // TLS (C1): the platform's library, for the link to add if the program's live code reaches
    // the engine (`link::linked`).
    let tls_libraries = (reached.tls && !freestanding).then(|| tls::link_args(&target));
    clock.mark("native_compile");
    if let Some(code) = objects_for_target(
        choice,
        &objects,
        &artefacts,
        &frames,
        &target,
        freestanding,
        &cleanup,
        with_report,
        &source_root,
        map,
        tally,
        summary,
    ) {
        return code;
    }
    if let Err(code) = linked(
        choice,
        &backend,
        &tmp_exe,
        &objects,
        &foreign,
        tls_libraries.as_ref(),
        &cleanup,
        &work,
        map,
        summary,
        &summarized,
    ) {
        return code;
    }
    published(
        choice,
        emit_ir,
        with_report,
        &artefacts,
        &output,
        &exported,
        &backend,
        &tmp_exe,
        &work,
        &cleanup,
        &clock,
        &source_root,
        map,
        tally,
        summary,
        &summarized,
    )
}

/// How a phase reports a failure as the summary (N31): the status, the exit code, the
/// diagnostics, the sources they point into, and the artefact when there is one.
type Summarize<'a> = dyn Fn(
        Status,
        u8,
        &[nazm_diag::Diagnostic],
        Option<&nazm_span::SourceMap>,
        Option<String>,
    ) -> ExitCode
    + 'a;

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
    /// With `library`, a shared library instead of a static one (N85).
    pub(crate) shared: bool,
    /// Objects for the target's own toolchain, not an executable (N57).
    pub(crate) objects: Option<&'a Path>,
    /// Measure the board's stack on the board (N63).
    pub(crate) stack_watermark: bool,
    /// `--stack-size` (R1b, L14): the bytes of stack `main` and every task run on.
    pub(crate) stack_size: Option<u64>,
    /// `--heap-limit` (R1d, D5): the most heap the program may hold live. The root package's
    /// `heap-limit` stands where this is not given ([`Settled`]'s is the one built with).
    pub(crate) heap_limit: Option<u64>,
    /// `--deny-warnings` (R1b, L15): refuse a program the checker warns of.
    pub(crate) deny_warnings: bool,
    /// `aos` or `soa`: how a `Vec` of records is laid out (N68).
    pub(crate) layout: &'a str,
    /// A contract's WebAssembly adapter (N71).
    pub(crate) adapter: Option<&'a crate::contract_wasm::Shape>,
    /// Print where the build's time went (N48).
    pub(crate) timings: bool,
    /// Carry debug information, for a debugger (N48).
    pub(crate) debug: bool,
    /// `--release` (R1.5, D15): stripped, position-independent, stack-protected and signed with
    /// the hardened runtime.
    pub(crate) release: bool,
    /// A runtime artifact to link instead of generating one (N82, §7.83).
    pub(crate) runtime: Option<&'a Path>,
    /// The profile `--profile` names, which `@cfg(profile = …)` sees (Gate 2, §17).
    pub(crate) profile: Option<&'a str>,
    /// A marked function to build through the generated entry (Gate 2, §18–§19).
    pub(crate) harness: Option<&'a crate::harness::Harness>,
    /// `--features`: asked of the root package beyond its `default` (Gate 2, §22).
    pub(crate) features: &'a [String],
    /// The board `--board` read (Gate 3); the target's built-in one when `None`.
    pub(crate) board: Option<&'a nazm_lir::runtime::board::Board>,
    /// Built for `nazm run` to execute (R1.5, D11): nothing is said about the build — no path,
    /// no authority line, no warnings — and a refusal only a native build makes names
    /// `nazm run --interpret`.
    pub(crate) run: bool,
}
