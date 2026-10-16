//! The link and what follows it: an executable, archive or shared library, the debug
//! information, a library's header, and publishing the output by rename.

use super::lower::Artefact;
use super::timing::{report, Clock};
use super::*;

/// The link: an executable, a static archive or a shared library, from the objects in order.
#[allow(clippy::too_many_arguments)]
pub(super) fn linked(
    choice: Choice<'_>,
    backend: &Backend,
    tmp_exe: &Path,
    objects: &[PathBuf],
    foreign: &[String],
    tls: Option<&Result<Vec<String>, String>>,
    cleanup: &dyn Fn(bool),
    work: &Path,
    map: &nazm_span::SourceMap,
    summary: bool,
    summarized: &Summarize<'_>,
) -> Result<(), ExitCode> {
    let attempt = |objects: &[PathBuf]| {
        if choice.library && choice.shared {
            backend.link_shared(tmp_exe, objects)
        } else if choice.library {
            archive(tmp_exe, objects)
        } else if choice.release {
            backend.link_release(tmp_exe, objects)
        } else {
            backend.link(tmp_exe, objects)
        }
    };
    let mut linked = attempt(objects);
    // TLS (C1): a module that reaches the engine — `@std/http` does — makes no program need the
    // platform's TLS library unless its live code calls it. An executable is dead-stripped, so it
    // links without the library unless a call the stripping kept needs it; only then is the link
    // run again with it (`build::tls`). A static archive's C host links it.
    let failed = !matches!(&linked, Outcome::Ran(o) if o.status.success());
    if let (true, false, Some(tls)) = (failed, choice.library && !choice.shared, tls) {
        match tls {
            Ok(libraries) => {
                let mut with = objects.to_vec();
                with.extend(libraries.iter().map(PathBuf::from));
                linked = attempt(&with);
            }
            Err(why) => eprintln!("nazm: {why}"),
        }
    }
    match linked {
        // A release build (R1.5, D15) is stripped and signed once it is linked.
        Outcome::Ran(o) if o.status.success() && choice.release => {
            if let Err(e) = super::release::finish(tmp_exe) {
                cleanup(false);
                eprintln!("nazm: the release build was not finished: {e}");
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
        }
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
        other => {
            let code = report_clang(other, work, cleanup);
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
    Ok(())
}

/// `--emit-ir`: beside the output, one file per unit, named as the artefacts are — for a linked
/// build and for one whose objects a target's own toolchain links (`--objects`).
pub(super) fn write_ir(artefacts: &[Artefact], output: &Path) {
    for artefact in artefacts {
        let nazm_lir::backend::Code::Llvm(ir) = &artefact.code else {
            continue;
        };
        let beside = with_suffix(output, &format!("{}.ll", artefact.name));
        if let Err(e) = std::fs::write(&beside, ir) {
            eprintln!("nazm: cannot write {}: {e}", beside.display());
        }
    }
}

/// The finished build: IR beside the output when asked, the debug information, a library's
/// header, then the output published by rename, last, so an existing one survives every
/// failure before it.
#[allow(clippy::too_many_arguments)]
pub(super) fn published(
    choice: Choice<'_>,
    emit_ir: bool,
    with_report: bool,
    artefacts: &[Artefact],
    output: &Path,
    exported: &[(String, Vec<nazm_lir::Ty>, nazm_lir::Ty)],
    backend: &Backend,
    tmp_exe: &Path,
    work: &Path,
    cleanup: &dyn Fn(bool),
    clock: &Clock,
    source_root: &nazm_service::root::SourceRoot,
    map: &nazm_span::SourceMap,
    tally: Native,
    summary: bool,
    summarized: &Summarize<'_>,
) -> ExitCode {
    if emit_ir {
        write_ir(artefacts, output);
    }
    // On macOS the linker leaves debug information in the objects it read, which are about
    // to go, so a debug build gathers it into `OUTPUT.dSYM` first (N48). Elsewhere it is in
    // the executable already.
    if gathers_debug_information(choice.debug, std::env::consts::OS) {
        let dsym = with_suffix(output, "dSYM");
        if let Err(e) = backend.gather_debug_info(tmp_exe, &dsym) {
            eprintln!("nazm: the debug information was not gathered: {e}");
        }
    }
    // A library's header, beside it (N55): written before the archive is published, so a
    // published archive always has its header.
    if choice.library {
        let header = output.with_extension("h");
        if let Err(e) = std::fs::write(&header, c_header(output, exported)) {
            cleanup(false);
            eprintln!("nazm: cannot write {}: {e}", header.display());
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    // Published last, so an existing executable survives every failure above — and then the
    // intermediates go, the working directory the executable was linked in included.
    let published = std::fs::rename(tmp_exe, output);
    let _ = std::fs::remove_dir_all(work);
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
    // A build for `nazm run` (R1.5, D11) says nothing: what follows is the program's output.
    if choice.run {
        return ExitCode::from(exit::OK);
    }
    // What the program may touch (R1, option C), on standard error with the build's other words.
    if let Some(held) = crate::authority::of_main(map.text(nazm_span::FileId::ROOT)) {
        eprintln!("{}", crate::authority::line(&held));
    }
    println!("{}", output.display());
    ExitCode::from(exit::OK)
}

/// Explain a `clang` failure, keeping the IR when its complaint is about it.
pub(super) fn report_clang(outcome: Outcome, kept: &Path, cleanup: impl Fn(bool)) -> ExitCode {
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
            let said = String::from_utf8_lossy(&o.stderr);
            eprint!("{said}");
            // Gate 3: a failure in the link step or in the toolchain's own setup (a missing
            // SDK library, `xcrun`) is the environment's, and blaming the IR for it sends the
            // reader to the wrong place.
            if toolchain_failure(&said) {
                eprintln!(
                    "nazm: `{}` failed while linking or setting up (exit {:?}) — its message \
                     above is about the toolchain or its environment, not about the generated \
                     IR. The IR has been kept at {} in case it is needed.",
                    DRIVER,
                    o.status.code(),
                    kept.display()
                );
            } else {
                eprintln!(
                    "nazm: `{}` rejected the generated IR (exit {:?}). This is a bug in \
                     `nazm build`, not in your program — the IR has been kept at {} to \
                     report it with.",
                    DRIVER,
                    o.status.code(),
                    kept.display()
                );
            }
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}

/// Whether clang's own message says the link step or the toolchain's setup failed, rather than
/// its reading of the IR.
fn toolchain_failure(said: &str) -> bool {
    [
        "linker command failed",
        "xcrun:",
        "ld: library",
        "ld: framework",
        "unable to find sdk",
        "cannot find -l",
        // With Xcode.app selected rather than the command line tools, the shim reports a
        // missing SDK through `xcode-select` and `xcodebuild` instead (0.9.0 preview).
        "xcode-select:",
        "Failed to determine realpath",
        "xcodebuild -sdk",
    ]
    .iter()
    .any(|m| said.contains(m))
}

/// Whether two paths name the same file, falling back to a textual comparison.
///
/// `canonicalize` fails on a path that does not exist yet, which is the normal case for
/// an output — so a plain comparison is the fallback rather than an error.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub(super) fn sibling(output: &Path, suffix: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    output.with_file_name(name)
}

/// Where the executable is linked: inside the working directory, under the output's own file
/// name, which the Mach-O linker writes into the executable (N46).
pub(super) fn link_name(work: &Path, output: &Path) -> PathBuf {
    work.join(output.file_name().unwrap_or_default())
}

/// Whether a build gathers its debug information beside the executable before the objects
/// holding it are removed: a debug build on macOS, whose linker leaves it in the objects (N48).
fn gathers_debug_information(debug: bool, os: &str) -> bool {
    debug && os == "macos"
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
    let c = |t: &nazm_lir::Ty| match t {
        nazm_lir::Ty::Bool => "bool",
        // Every number's C type (Gate 2, §23).
        nazm_lir::Ty::Num(k) => match k {
            nazm_core::sema::NumKind::I8 => "int8_t",
            nazm_core::sema::NumKind::I16 => "int16_t",
            nazm_core::sema::NumKind::I32 => "int32_t",
            nazm_core::sema::NumKind::U8 => "uint8_t",
            nazm_core::sema::NumKind::U16 => "uint16_t",
            nazm_core::sema::NumKind::U32 => "uint32_t",
            nazm_core::sema::NumKind::U64 => "uint64_t",
            nazm_core::sema::NumKind::F32 => "float",
            nazm_core::sema::NumKind::F64 => "double",
            nazm_core::sema::NumKind::I64 => "int64_t",
            // Never exported: the C boundary refuses them (R1b, L13b).
            nazm_core::sema::NumKind::I128 => "__int128",
            nazm_core::sema::NumKind::U128 => "unsigned __int128",
        },
        _ => "int64_t",
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
