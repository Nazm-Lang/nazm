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
    cache: bool,
    cross: Option<&str>,
    freestanding: bool,
    source_root: &nazm_service::root::SourceRoot,
    chosen: &(dyn nazm_lir::backend::Backend + Sync),
    clif_store: &Option<ObjectStore>,
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
        Backend::discover(opt_level, work, cross)
    } else {
        Backend::unidentified(opt_level, cross)
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
        let mut written = write_objects(dir, objects, target);
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
        nazm_lir::backend::Os::Linux => "-lpthread -lm".to_owned(),
        nazm_lir::backend::Os::Macos => String::new(),
        // MinGW-w64 (Gate 3): Winsock and BCrypt, and the C runtime's own libraries linked in
        // statically, so the executable needs no MinGW DLL beside it.
        nazm_lir::backend::Os::Windows => "-static -pthread -lws2_32 -lbcrypt".to_owned(),
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
            nazm_lir::runtime::board::linker_script(board_of(target)),
        )?;
    }
    Ok(())
}
