//! Objects: each artefact compiled or reused from the object store, and `--objects DIR` for the
//! target's own toolchain.

use super::link::report_clang;
use super::lower::Artefact;
use super::timing::report;
use super::*;

/// Every artefact made an object: reused from the store when the exact input, configuration and
/// compiler were filed before, otherwise compiled and filed.
#[allow(clippy::too_many_arguments)]
pub(super) fn compiled(
    artefacts: &[Artefact],
    work: &Path,
    opt_level: u8,
    whole: bool,
    cache: bool,
    cross: Option<&str>,
    freestanding: bool,
    source_root: &nazm_service::root::SourceRoot,
    chosen: &(dyn nazm_lir::backend::Backend + Sync),
    clif_store: &Stores,
    clif_key: &(dyn Fn(&str) -> ObjectKey + Sync),
    cleanup: &dyn Fn(bool),
    map: &nazm_span::SourceMap,
    summary: bool,
    summarized: &Summarize<'_>,
) -> Result<Compiled, ExitCode> {
    // Asked once, before anything is compiled: what this backend is, and what it would
    // do. `crate::backend` says why the answer comes from the compiler rather than from
    // the three arguments this one passes.
    let backend = if cache {
        Backend::discover_whole(opt_level, work, cross, whole)
    } else {
        Backend::unidentified(opt_level, cross, whole)
    };
    // A backend that would not say what it is gets no cache. Guessing a configuration is
    // how one target's objects end up in another's build.
    //
    // Nor does a board build (N63): its stack bound reads the frame sizes clang reports while
    // compiling, which a reused object would not report.
    //
    // The runtime's and the standard library's units are filed in the user cache (R1.5, D12),
    // under the same keys; the project's own in its `.nazm/`.
    let usable = backend.settled().filter(|_| !freestanding).is_some();
    // A whole-program link's cache (R1.5 3.4) beside the project's objects, where they are kept.
    let backend = backend
        .with_lto_cache(usable.then(|| crate::derived::root(source_root.path()).join("lto")));
    let stores = Stores {
        project: usable.then(|| crate::derived::objects(source_root.path())),
        user: usable.then(crate::derived::user_objects).flatten(),
    };
    let mut frames = std::collections::BTreeMap::new();

    let mut tally = Native {
        units: artefacts.len(),
        instances: artefacts.iter().filter(|a| a.instance).count(),
        ..Native::default()
    };
    let mut objects: Vec<PathBuf> = Vec::new();
    for artefact in artefacts {
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
                        return Err(summarized(
                            Status::ToolchainFailed,
                            exit::TOOLCHAIN_FAILED,
                            &[],
                            Some(map),
                            None,
                        ));
                    }
                    return Err(ExitCode::from(exit::TOOLCHAIN_FAILED));
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
                    if let (Some(store), Some(k)) = (
                        clif_store.for_unit(artefact.shared),
                        artefact.key.as_deref(),
                    ) {
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
        if let (Some(store), Some(key)) = (stores.for_unit(artefact.shared), key) {
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
                return Err(summarized(
                    Status::ToolchainFailed,
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    Some(map),
                    None,
                ));
            }
            return Err(ExitCode::from(exit::TOOLCHAIN_FAILED));
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
                    return Err(summarized(
                        Status::ToolchainFailed,
                        exit::TOOLCHAIN_FAILED,
                        &[],
                        Some(map),
                        None,
                    ));
                }
                return Err(code);
            }
        }

        // Published after it exists and from the bytes that were produced, so what is
        // stored is what was linked. A store that will not take an entry is not a reason
        // to fail a build: the disk is full, or the tree is read-only, and the program is
        // fine either way.
        if let (Some(store), Some(key), Some(settled), Ok(bytes)) = (
            stores.for_unit(artefact.shared),
            key,
            backend.settled(),
            std::fs::read(&obj),
        ) {
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
    Ok(Compiled {
        backend,
        objects,
        tally,
        frames,
    })
}

/// What [`compiled`] made.
pub(super) struct Compiled {
    pub(super) backend: Backend,
    /// The objects, in link order.
    pub(super) objects: Vec<PathBuf>,
    pub(super) tally: Native,
    /// Each function's frame size, as the compiler reported it (N63).
    pub(super) frames: std::collections::BTreeMap<String, u64>,
}

/// `--objects DIR` (N57): every object in link order, and `link.txt`, for the target's own
/// toolchain; for a freestanding image, its stack bound too. `None` when the build links here.
#[allow(clippy::too_many_arguments)]
pub(super) fn objects_for_target(
    choice: Choice<'_>,
    objects: &[PathBuf],
    artefacts: &[Artefact],
    frames: &std::collections::BTreeMap<String, u64>,
    target: &nazm_lir::backend::Target,
    freestanding: bool,
    cleanup: &dyn Fn(bool),
    with_report: bool,
    source_root: &nazm_service::root::SourceRoot,
    map: &nazm_span::SourceMap,
    tally: Native,
    summary: bool,
) -> Option<ExitCode> {
    // Objects for the target's own toolchain (N57): every object in link order, renamed by its
    // position so the order survives, and `link.txt` saying what to link them with.
    if let Some(dir) = choice.objects {
        // A microcontroller's flash and RAM (M1): an image its objects already show will not fit
        // is refused before anything is written.
        if freestanding && target.is_board() {
            if let Err(why) = fits(&board_for(choice, target), objects) {
                cleanup(false);
                if !summary {
                    eprintln!("nazm: error[{}]: {why}", nazm_diag::codes::BOARD_MEMORY);
                }
                return Some(ExitCode::from(exit::CHECK_FAILED));
            }
        }
        // The sections `@section` names (Gate 3), read from what was compiled.
        let placed = nazm_lir::runtime::board::Placed::of(
            &artefacts
                .iter()
                .filter_map(|a| match &a.code {
                    nazm_lir::backend::Code::Llvm(ir) => Some(ir.as_str()),
                    nazm_lir::backend::Code::Object(_) => None,
                })
                .collect::<Vec<_>>(),
        );
        // TLS's engine (C1): the platform's library, named for the target's toolchain.
        let tls = artefacts
            .iter()
            .any(|a| a.name == "tls")
            .then(|| super::tls::link_args(target).ok())
            .flatten();
        let mut written = write_objects(
            dir,
            objects,
            target,
            &placed,
            &board_for(choice, target),
            tls.as_deref(),
        );
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
            let (root, reserved) = if target.is_wasi() {
                ("_start", nazm_lir::runtime::wasi::STACK)
            } else if target.is_wasm() {
                ("nazm_main", nazm_lir::runtime::wasm::STACK)
            } else {
                ("nz_board_main", board_for(choice, target).stack)
            };
            let bound = crate::stack::bound(root, frames, &calls);
            written = std::fs::write(
                dir.join("bounds.json"),
                crate::stack::report(&target.triple, &bound, frames, reserved),
            );
        }
        cleanup(false);
        return Some(match written {
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
        });
    }
    None
}

/// What a microcontroller board's image needs of its flash and RAM (M1, `docs/spec.md`,
/// *Boards*), read from its objects' section headers: code, read-only data and the
/// initial values of writable data in flash; writable and zeroed data and the stack in RAM. Each
/// section is aligned as it asks, in object order, so this is what the linker will need at
/// least — the linker script's own assertions hold the exact sum. A board loaded whole, or an
/// object that is not 32-bit ELF, is not measured.
pub(super) fn fits(
    board: &nazm_lir::runtime::board::Board,
    objects: &[PathBuf],
) -> Result<(), String> {
    let Some(m) = board.memory else {
        return Ok(());
    };
    let (mut flash, mut data, mut zeroed) = (0u64, 0u64, 0u64);
    for o in objects {
        let Ok(bytes) = std::fs::read(o) else {
            continue;
        };
        for (name, kind, size, align) in elf32_sections(&bytes) {
            if name.starts_with(".eh_frame") {
                continue;
            }
            let at = |n: u64| n.div_ceil(align.max(1)) * align.max(1) + size;
            match kind {
                SectionKind::Code => flash = at(flash),
                SectionKind::Data => data = at(data),
                SectionKind::Zeroed => zeroed = at(zeroed),
            }
        }
    }
    let need_flash = flash + data;
    if need_flash > m.flash.length {
        return Err(format!(
            "the image does not fit the board `{}`'s flash: it needs at least {need_flash:#x} \
             bytes and `memory.flash` has {:#x}, over by {:#x}",
            board.name,
            m.flash.length,
            need_flash - m.flash.length
        ));
    }
    let need_ram = data + zeroed + board.stack;
    if need_ram > m.ram.length {
        return Err(format!(
            "the image's data and stack do not fit the board `{}`'s RAM: they need at least \
             {need_ram:#x} bytes — the stack {:#x} of them — and `memory.ram` has {:#x}, over by \
             {:#x}",
            board.name,
            board.stack,
            m.ram.length,
            need_ram - m.ram.length
        ));
    }
    Ok(())
}

/// Where a section of an image goes (M1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    /// Code or read-only data: flash.
    Code,
    /// Writable data: RAM, its initial values in flash too.
    Data,
    /// Zeroed data: RAM.
    Zeroed,
}

/// Every allocated section of a 32-bit little-endian ELF object: its name, where it goes, its
/// size and its alignment. Nothing for anything else.
fn elf32_sections(b: &[u8]) -> Vec<(String, SectionKind, u64, u64)> {
    let u16_at = |at: usize| -> Option<u64> {
        let s: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
        Some(u64::from(u16::from_le_bytes(s)))
    };
    let u32_at = |at: usize| -> Option<u64> {
        let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(u64::from(u32::from_le_bytes(s)))
    };
    // `\x7fELF`, 32-bit, little-endian.
    if !b.starts_with(b"\x7fELF") || b.get(4) != Some(&1) || b.get(5) != Some(&1) {
        return Vec::new();
    }
    let header = || -> Option<Vec<(String, SectionKind, u64, u64)>> {
        let shoff = usize::try_from(u32_at(0x20)?).ok()?;
        let shentsize = usize::try_from(u16_at(0x2e)?).ok()?;
        let shnum = usize::try_from(u16_at(0x30)?).ok()?;
        let shstrndx = usize::try_from(u16_at(0x32)?).ok()?;
        let sh = |i: usize, field: usize| u32_at(shoff + i * shentsize + field);
        let names = usize::try_from(sh(shstrndx, 16)?).ok()?;
        let mut out = Vec::new();
        for i in 0..shnum {
            let (kind, flags) = (sh(i, 4)?, sh(i, 8)?);
            // SHF_ALLOC: in the image at all.
            if flags & 0x2 == 0 {
                continue;
            }
            let start = names + usize::try_from(sh(i, 0)?).ok()?;
            let end = b.get(start..)?.iter().position(|c| *c == 0)? + start;
            let name = String::from_utf8_lossy(b.get(start..end)?).into_owned();
            // SHT_NOBITS is zeroed; SHF_WRITE is data; anything else is flash's.
            let goes = if kind == 8 {
                SectionKind::Zeroed
            } else if flags & 0x1 != 0 {
                SectionKind::Data
            } else {
                SectionKind::Code
            };
            out.push((name, goes, sh(i, 20)?, sh(i, 32)?));
        }
        Some(out)
    };
    header().unwrap_or_default()
}

/// `DIR/NN-NAME.o` for each object in link order, and `DIR/link.txt` (N57): the target, the
/// objects in order, and the system libraries the runtime needs on it.
fn write_objects(
    dir: &Path,
    objects: &[PathBuf],
    target: &nazm_lir::backend::Target,
    placed: &nazm_lir::runtime::board::Placed,
    board: &nazm_lir::runtime::board::Board,
    tls: Option<&[String]>,
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
        nazm_lir::backend::Os::Linux => "-lpthread -lm".to_owned(),
        nazm_lir::backend::Os::Macos => String::new(),
        // Android (Gate 3): the NDK's `clang` for API level 30, with Bionic's mathematics.
        nazm_lir::backend::Os::Android => "-lm".to_owned(),
        // iOS (Gate 3): `xcrun --sdk iphoneos clang`; libSystem has everything.
        nazm_lir::backend::Os::Ios => String::new(),
        // MinGW-w64 (Gate 3): the libraries a Windows host's own link names too.
        nazm_lir::backend::Os::Windows => crate::backend::windows_libraries().join(" "),
        // A WASI module (Gate 3): itself, its imports WASI's.
        nazm_lir::backend::Os::Wasi => format!("none; {}", nazm_lir::runtime::wasi::LINK),
        // A WebAssembly module links nothing but itself, by `wasm-ld` (N64).
        nazm_lir::backend::Os::None if target.is_wasm() => {
            format!("none; {}", nazm_lir::runtime::wasm::LINK)
        }
        // A freestanding image links nothing but itself, by `link.ld` (N62).
        nazm_lir::backend::Os::None => "none; ld.lld -T link.ld".to_owned(),
    };
    let libs = match tls {
        Some(more) => format!("{libs} {}", more.join(" ")),
        None => libs,
    };
    let text = format!(
        "# Written by nazm build --objects. Link these, in this order, with the target's toolchain.\n\
         target {}\nobjects {}\nlibraries {libs}\n",
        target.triple,
        listed.join(" ")
    );
    std::fs::write(dir.join("link.txt"), text)?;
    // A module's reference host (N64): exactly the imports it may grant.
    if target.is_wasi() {
        std::fs::write(dir.join("wasi.mjs"), nazm_lir::runtime::wasi::HOST)?;
    } else if target.is_wasm() {
        std::fs::write(dir.join("host.mjs"), nazm_lir::runtime::wasm::HOST)?;
    }
    // The board's memory map, for `ld.lld -T link.ld` (N62).
    if target.is_board() {
        std::fs::write(
            dir.join("link.ld"),
            nazm_lir::runtime::board::linker_script_placing(board, placed),
        )?;
    }
    Ok(())
}
