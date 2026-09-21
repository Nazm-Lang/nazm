//! `nazm build` — source to a native executable.
//!
//! The pipeline is the existing front end plus two new steps: `nazm_lir::lower` turns a
//! *checked* tree into the low-level representation, `nazm_lir::emit_llvm` turns that into
//! LLVM IR text, and `clang` turns that into a binary. No LLVM is linked into `nazm` —
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

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::{emit, exit};

/// Compile, or explain what stopped it.
pub(crate) fn run(
    input: &Path,
    out: Option<&Path>,
    json: bool,
    emit_ir: bool,
    opt_level: u8,
) -> ExitCode {
    let map = match crate::load::load(input) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            emit(&diagnostics, &partial, json);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };

    let output = match out {
        Some(p) => p.to_path_buf(),
        // `prog.nz` → `prog`. A stem with no extension would collide with its source, and
        // the check below catches that rather than this guessing around it.
        None => PathBuf::from(input.file_stem().unwrap_or_default()),
    };

    if same_file(input, &output) {
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
    let ir = match nazm_core::on_checked_map(&map, |program, resolution| {
        nazm_lir::lower(program, resolution)
            .map(|module| nazm_lir::emit_llvm(&module, &input.display().to_string(), &map))
            .map_err(|e| e.diagnostics)
    }) {
        Ok(ir) => ir,
        Err(diagnostics) => {
            // `N0101` is this backend's "not yet", and it reads differently from a
            // program being wrong. Distinguished for the summary line only; both are a
            // check failure and neither produces output.
            let unsupported = diagnostics
                .iter()
                .any(|d| d.code == nazm_diag::codes::UNSUPPORTED_CONSTRUCT);
            emit(&diagnostics, &map, json);
            let n = diagnostics.len();
            if unsupported {
                eprintln!(
                    "nazm: {n} construct{} outside the native subset; nothing was compiled",
                    if n == 1 { "" } else { "s" }
                );
            } else {
                eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            }
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };

    // Temporaries beside the output, so the final move is a rename within one filesystem
    // rather than a copy that can half-succeed.
    let stamp = std::process::id();
    let tmp_ll = sibling(&output, &format!(".nazm-{stamp}.ll"));
    let tmp_exe = sibling(&output, &format!(".nazm-{stamp}.out"));

    if let Err(e) = std::fs::write(&tmp_ll, &ir) {
        eprintln!("nazm: cannot write {}: {e}", tmp_ll.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }

    let status = Command::new("clang")
        .arg(format!("-O{opt_level}"))
        // The module carries no target triple, so clang supplies the host's and would
        // otherwise warn about overriding nothing.
        .arg("-Wno-override-module")
        .arg("-o")
        .arg(&tmp_exe)
        .arg(&tmp_ll)
        .output();

    let cleanup = |keep_ir: bool| {
        if !keep_ir {
            let _ = std::fs::remove_file(&tmp_ll);
        }
        let _ = std::fs::remove_file(&tmp_exe);
    };

    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            cleanup(false);
            eprintln!(
                "nazm: `clang` was not found on PATH, and `nazm build` needs it to \
                 assemble the LLVM IR it generates.\n\
                 nazm: install the Xcode command line tools (`xcode-select --install`) \
                 or your platform's clang package. `nazm run` needs none of this."
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Err(e) => {
            cleanup(false);
            eprintln!("nazm: could not run `clang`: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Ok(o) if !o.status.success() => {
            // Keep the IR: clang's complaint is about a file the user cannot see
            // otherwise, and a compiler bug is much easier to report with it in hand.
            cleanup(true);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!(
                "nazm: `clang` rejected the generated IR (exit {:?}). This is a bug in \
                 `nazm build`, not in your program — the IR has been kept at {} to \
                 report it with.",
                o.status.code(),
                tmp_ll.display()
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Ok(_) => {
            if emit_ir {
                let beside = with_suffix(&output, "ll");
                if let Err(e) = std::fs::rename(&tmp_ll, &beside) {
                    eprintln!("nazm: cannot write {}: {e}", beside.display());
                }
            } else {
                let _ = std::fs::remove_file(&tmp_ll);
            }
            // Published last, so an existing executable survives every failure above.
            if let Err(e) = std::fs::rename(&tmp_exe, &output) {
                let _ = std::fs::remove_file(&tmp_exe);
                eprintln!("nazm: cannot write {}: {e}", output.display());
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
            println!("{}", output.display());
            ExitCode::from(exit::OK)
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

fn with_suffix(output: &Path, ext: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    output.with_file_name(name)
}
