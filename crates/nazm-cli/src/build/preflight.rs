//! Before anything is compiled: the source root, the target and its backend, the refusals that
//! need nothing more, the program loaded, and the output path it may be written to.

use super::link::same_file;
use super::*;

/// What is settled before anything is compiled, and every refusal that needs nothing more: the
/// source root, the target and its backend, the foreign objects, the layout, the watermark, and
/// whether this host can link the target.
pub(super) fn preflight(
    input: &Path,
    choice: Choice<'_>,
    summary: bool,
    summarized: &Summarize<'_>,
) -> Result<Settled, ExitCode> {
    // Discovered, never given: `nazm build` takes no `--source-root`, and the marker is
    // how a project declares one. It decides every module's durable identity, which is
    // the name the generated code carries and therefore part of what an object is keyed
    // on.
    let source_root = match crate::root::decide(input, None) {
        Ok(r) => r,
        Err(e) => {
            let code = exit::root_status(&e);
            if summary {
                let status = if code == exit::NO_SUCH_FILE {
                    Status::Unreadable
                } else {
                    Status::Refused
                };
                return Err(summarized(status, code, &[], None, None));
            }
            eprintln!("nazm: {e}");
            return Err(ExitCode::from(code));
        }
    };
    // The target and the backend, settled before anything is compiled (N41). A target that
    // is not this host is refused: an object for it would need a linker and a runtime v1
    // does not have, and building for the host instead would be a silent lie.
    let target = match nazm_lir::backend::Target::requested(choice.target) {
        Ok(t) => t,
        Err(e) => {
            // A `--target` that names nothing this compiler builds is the command line's fault
            // (5); a host it cannot build for is the machine's (4) (Q1-C1).
            let code = crate::exit::target_refused(choice.target.is_some());
            if summary {
                return Err(summarized(Status::Refused, code, &[], None, None));
            }
            eprintln!("nazm: {e}");
            return Err(ExitCode::from(code));
        }
    };
    // A foreign object that is not there is refused before anything is compiled, by name,
    // rather than reported by the linker as an undefined symbol after the whole build (N42).
    if let Some(missing) = choice.link.iter().find(|p| !p.is_file()) {
        if summary {
            return Err(summarized(
                Status::Refused,
                exit::NO_SUCH_FILE,
                &[],
                None,
                None,
            ));
        }
        eprintln!(
            "nazm: `--link {}`: no such file; `--link` names an object or library that \
             defines foreign functions",
            missing.display()
        );
        return Err(ExitCode::from(exit::NO_SUCH_FILE));
    }
    // A freestanding image is the LLVM backend's (N62): Cranelift's object writer knows no
    // binary format for a target without an operating system, so it is refused by name here
    // rather than by Cranelift after the program is lowered.
    if choice.backend == nazm_lir::backend::BackendId::Cranelift
        && matches!(
            target.os,
            nazm_lir::backend::Os::None | nazm_lir::backend::Os::Wasi
        )
    {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--backend cranelift` builds no freestanding target; `{}` is the LLVM \
             backend's",
            target.triple
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // The mobile targets (Gate 3) are the LLVM backend's.
    if choice.backend == nazm_lir::backend::BackendId::Cranelift
        && matches!(
            target.os,
            nazm_lir::backend::Os::Android | nazm_lir::backend::Os::Ios
        )
    {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--backend cranelift` builds no mobile target; `{}` is the LLVM backend's",
            target.triple
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // Nazm's own code generator (R9, D15) builds for the two macOS targets before 1.0 (its D13),
    // and must be there to run: both asked before anything is compiled, so neither is reported
    // later as the compiler's own fault, and nothing falls back to another backend.
    if choice.backend == nazm_lir::backend::BackendId::Nazm {
        if !nazm_codegen_nazm::carries(&target) {
            if summary {
                return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
            }
            eprintln!(
                "nazm: `--backend nazm` builds for aarch64-apple-darwin and x86_64-apple-darwin \
                 before 1.0; `{}` is the LLVM backend's",
                target.triple
            );
            return Err(ExitCode::from(exit::USAGE));
        }
        if let Err(e) = nazm_codegen_nazm::NazmCodegen::new(target.clone()).program() {
            if summary {
                return Err(summarized(
                    Status::Refused,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    None,
                    None,
                ));
            }
            eprintln!("nazm: `--backend nazm` runs Nazm's code generator: {e}");
            return Err(ExitCode::from(exit::TOOLCHAIN_FAILED));
        }
    }
    // RISC-V (N86) is code generation the host's `clang` may not have — Apple's has none. Asked
    // before anything is compiled, so a missing code generator is named rather than reported as
    // the compiler's own fault.
    if target.arch == nazm_lir::backend::Arch::Riscv64 && !crate::backend::generates("riscv64") {
        if summary {
            return Err(summarized(
                Status::Refused,
                exit::TOOLCHAIN_FAILED,
                &[],
                None,
                None,
            ));
        }
        eprintln!(
            "nazm: this host's `{}` has no RISC-V code generator, so it cannot build `{}`; build \
             where one does — the `nazm-qemu:n86` image's",
            crate::backend::DRIVER,
            target.triple
        );
        return Err(ExitCode::from(exit::TOOLCHAIN_FAILED));
    }
    // The field-by-field layout (N68, §7.70), asked for by name. LIR's since N105, so both
    // backends take it.
    let soa = match choice.layout {
        "aos" => false,
        "soa" => true,
        other => {
            if summary {
                return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
            }
            eprintln!("nazm: `--layout {other}` is not a layout; the layouts are `aos` and `soa`");
            return Err(ExitCode::from(exit::USAGE));
        }
    };
    // A DLL is not built for Windows (Gate 3): its exports would need marking, and nothing of
    // one has run. Refused by name rather than written as objects that link to something else.
    if choice.shared && target.os == nazm_lir::backend::Os::Windows {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--shared` builds no DLL for `{}`: a static library (`--lib`) is what Windows \
             has from this compiler",
            target.triple
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // A runtime artifact is generated for a target's built-in board (N82); one built for another
    // board's stack would check against the wrong bound (Gate 3).
    if choice.board.is_some() && choice.runtime.is_some() {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--runtime` is a built-in board's runtime, and `--board` names another; build \
             with one or the other"
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // The watermark is the board's (N63): a hosted program has an operating system's stack.
    if choice.stack_watermark && !target.is_board() {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--stack-watermark` measures a board's stack; it needs \
             `--target aarch64-unknown-none` or `--target riscv64gc-unknown-none-elf`"
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // A stack size is a hosted program's (R1b, L14): a board's stack is its manifest's, and a
    // WebAssembly module's its host's.
    if choice.stack_size.is_some() && (target.is_board() || target.is_wasm()) {
        if summary {
            return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
        }
        eprintln!(
            "nazm: `--stack-size` sizes a hosted program's threads; a board's stack is its \
             manifest's, and a WebAssembly module's is its host's"
        );
        return Err(ExitCode::from(exit::USAGE));
    }
    // A heap limit (R1d, D5): `--heap-limit`, or the root package's `heap-limit`. A hosted
    // program's: a board's heap is its manifest's arena, a WebAssembly module's its host's, a
    // library's its C host's; and a runtime artifact (N82) was built without the count.
    let heap_limit = choice.heap_limit.or_else(|| {
        crate::package::profile_graph(input)
            .and_then(|g| g.packages.get(g.root).and_then(|p| p.heap_limit))
    });
    if heap_limit.is_some() {
        let why = if target.is_board() || target.is_wasm() {
            Some("a board's heap is its manifest's, and a WebAssembly module's is its host's")
        } else if choice.library {
            Some("a library's heap is its C host's")
        } else if choice.runtime.is_some() {
            Some("a runtime artifact (`--runtime`) was built without the count")
        } else {
            None
        };
        if let Some(why) = why {
            if summary {
                return Err(summarized(Status::Refused, exit::USAGE, &[], None, None));
            }
            eprintln!("nazm: a heap limit bounds a hosted program's heap; {why}");
            return Err(ExitCode::from(exit::USAGE));
        }
    }
    // A target other than the host (N57): clang is told it, and where the host cannot link it
    // the build writes objects instead, or is refused by name before compiling anything.
    let host = nazm_lir::backend::Target::host().ok();
    let cross = (host.as_ref() != Some(&target)).then(|| target.triple.clone());
    let links_here = host.as_ref().is_some_and(|h| target.links_on(h));
    if !links_here && choice.objects.is_none() && !choice.library {
        if summary {
            return Err(summarized(
                Status::Refused,
                exit::TOOLCHAIN_FAILED,
                &[],
                None,
                None,
            ));
        }
        eprintln!(
            "nazm: this host has no linker for `{}`; build it with `--objects DIR`, which writes \
             every object and `DIR/link.txt` for that target's own toolchain to link",
            target.triple
        );
        return Err(ExitCode::from(exit::TOOLCHAIN_FAILED));
    }
    Ok(Settled {
        source_root,
        target,
        soa,
        cross,
        heap_limit,
    })
}

/// What [`preflight`] settled.
pub(super) struct Settled {
    pub(super) source_root: nazm_service::root::SourceRoot,
    pub(super) target: nazm_lir::backend::Target,
    /// The field-by-field layout (N68).
    pub(super) soa: bool,
    /// The target's triple, when it is not the host's.
    pub(super) cross: Option<String>,
    /// The heap limit the program is built with (R1d, D5): `--heap-limit`'s, or the root
    /// package's `heap-limit`.
    pub(super) heap_limit: Option<u64>,
}

/// The program loaded, and the output path it may be written to: never over its own source or
/// any module it loads, and a shared library's name without a comma.
pub(super) fn admitted(
    input: &Path,
    out: Option<&Path>,
    json: bool,
    choice: Choice<'_>,
    source_root: &nazm_service::root::SourceRoot,
    summary: bool,
    summarized: &Summarize<'_>,
) -> Result<(nazm_service::load::Loaded, PathBuf), ExitCode> {
    let mut loaded = match crate::harness::loaded(input, source_root.path(), choice.harness) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            let code = exit::load_status(&diagnostics);
            if summary {
                let status = if code == exit::NO_SUCH_FILE {
                    Status::Unreadable
                } else {
                    Status::Rejected
                };
                return Err(summarized(
                    status,
                    code,
                    &diagnostics,
                    Some(&partial.map),
                    None,
                ));
            }
            emit(&diagnostics, &partial.map, json);
            return Err(ExitCode::from(code));
        }
    };
    // `@cfg` is decided for the target built for, under the profiles held (Gate 2, §17), with
    // each package's features (§22).
    let config =
        crate::profile::configuration(input, choice.profile, choice.target, choice.features)?;
    loaded.graph = std::mem::take(&mut loaded.graph).with_config(config);
    let map = &loaded.map;
    let output = match out {
        Some(p) => p.to_path_buf(),
        // `prog.nz` → `prog`. A stem with no extension would collide with its source, and
        // the check below catches that rather than this guessing around it.
        None => PathBuf::from(input.file_stem().unwrap_or_default()),
    };

    // The entry's source, or any module's the program loads (Q1-A1-07).
    let clobbers = same_file(input, &output)
        || map
            .file_names()
            .iter()
            .any(|n| same_file(Path::new(n), &output));
    if clobbers {
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
            "nazm: refusing to write the executable over its own source ({}). \
             Pass -o with a different path.",
            output.display()
        );
        return Err(ExitCode::from(exit::CHECK_FAILED));
    }
    // A shared library names itself to the linker as `-Wl,…,NAME`, and the linker splits that
    // argument at every comma: a comma in the name would be an argument of its own (Q1-A1-06).
    if choice.library
        && choice.shared
        && output
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(','))
    {
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
            "nazm: a shared library's file name may not contain a comma ({}): the linker would \
             read what follows it as another argument",
            output.display()
        );
        return Err(ExitCode::from(exit::CHECK_FAILED));
    }
    Ok((loaded, output))
}

/// The task pool's switch for a hosted target (N83); `None` for a freestanding one, which has no
/// tasks.
/// The board a freestanding target names (N86): every target that is not WebAssembly and has no
/// operating system is one of the table's, and the first is N62's.
pub(crate) fn board_of(t: &nazm_lir::backend::Target) -> &'static nazm_lir::runtime::board::Board {
    nazm_lir::runtime::board::board(&t.triple).unwrap_or(nazm_lir::runtime::board::AARCH64)
}

/// The board a build is for (Gate 3): the one `--board` read, or the target's built-in one.
pub(crate) fn board_for(
    choice: Choice<'_>,
    t: &nazm_lir::backend::Target,
) -> nazm_lir::runtime::board::Board {
    choice.board.cloned().unwrap_or_else(|| board_of(t).clone())
}

/// `--board FILE` (Gate 3): the manifest read for the `--target` named, which must be a board's.
/// Refused before anything is compiled: a target that is not a board's is the command line's
/// fault (5), a file that is not there 3, and a manifest it cannot honour `N0626` (1).
pub(crate) fn read_board(
    path: &Path,
    target: Option<&str>,
) -> Result<nazm_lir::runtime::board::Board, ExitCode> {
    use nazm_lir::backend::Arch;
    use nazm_lir::runtime::board::Isa;
    let t = nazm_lir::backend::Target::requested(target).ok();
    let Some(t) = t.filter(nazm_lir::backend::Target::is_board) else {
        eprintln!(
            "nazm: `--board` describes a board: it needs `--target aarch64-unknown-none` or \
             `--target riscv64gc-unknown-none-elf`"
        );
        return Err(ExitCode::from(exit::USAGE));
    };
    let isa = match t.arch {
        Arch::Riscv64 => Isa::Riscv64,
        Arch::Thumbv7m => Isa::Thumbv7m,
        _ => Isa::Aarch64,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: `--board {}`: {e}", path.display());
            return Err(ExitCode::from(exit::NO_SUCH_FILE));
        }
    };
    crate::board_manifest::read(&text, &t.triple, isa).map_err(|r| {
        eprintln!("{}: {r}", path.display());
        ExitCode::from(exit::CHECK_FAILED)
    })
}

pub(crate) fn switch_target(
    t: &nazm_lir::backend::Target,
) -> Option<nazm_lir::runtime::switch::SwitchTarget> {
    use nazm_lir::backend::{Arch, Os};
    use nazm_lir::runtime::switch::SwitchTarget;
    match (t.arch, t.os) {
        (Arch::Aarch64, Os::Macos) => Some(SwitchTarget::Aarch64Darwin),
        (Arch::Aarch64, Os::Linux) => Some(SwitchTarget::Aarch64Linux),
        (Arch::X86_64, Os::Macos) => Some(SwitchTarget::X86_64Darwin),
        (Arch::X86_64, Os::Linux) => Some(SwitchTarget::X86_64Linux),
        (Arch::X86_64, Os::Windows) => Some(SwitchTarget::X86_64Windows),
        // The mobile targets (Gate 3): Android is Linux's kernel with Bionic; iOS is Darwin.
        (Arch::Aarch64, Os::Android) => Some(SwitchTarget::Aarch64Android),
        (Arch::Aarch64, Os::Ios) => Some(SwitchTarget::Aarch64Darwin),
        _ => None,
    }
}
