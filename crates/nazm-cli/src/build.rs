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
    // Discovered, never given. Native compilation reuses nothing and persists nothing
    // keyed on a module, so the root is asked for only because the loader computes keys.
    let source_root = match crate::root::decide(input, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let loaded = match crate::load::load(input, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
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
    let artefacts = match nazm_core::on_checked_map(map, &loaded.graph, |program, resolution| {
        nazm_lir::lower(program, resolution, &loaded.graph, map)
            .map(|native| emit_artefacts(&native, map))
            .map_err(|e| e.diagnostics)
    }) {
        Ok(a) => a,
        Err(diagnostics) => {
            // `N0101` is this backend's "not yet", and it reads differently from a
            // program being wrong. Distinguished for the summary line only; both are a
            // check failure and neither produces output.
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
    let tmp_exe = sibling(&output, &format!(".nazm-{stamp}.out"));
    if let Err(e) = std::fs::create_dir_all(&work) {
        eprintln!("nazm: cannot create {}: {e}", work.display());
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

    let mut objects: Vec<PathBuf> = Vec::new();
    for artefact in &artefacts {
        let ll = work.join(format!("{}.ll", artefact.name));
        let obj = work.join(format!("{}.o", artefact.name));
        if let Err(e) = std::fs::write(&ll, &artefact.ir) {
            cleanup(false);
            eprintln!("nazm: cannot write {}: {e}", ll.display());
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        // **Independently generated.** Each unit becomes an object on its own, with no
        // other unit's IR in scope, which is what makes this separate code generation
        // rather than concatenation with extra steps.
        match clang(&["-c"], opt_level, &obj, std::slice::from_ref(&ll)) {
            Outcome::Ran(o) if o.status.success() => objects.push(obj),
            other => return report_clang(other, &ll, cleanup),
        }
    }

    // Deterministic: the units are in load order and the support artefacts last, so the
    // command line does not depend on anything the filesystem decided.
    match clang(&[], opt_level, &tmp_exe, &objects) {
        Outcome::Ran(o) if o.status.success() => {}
        other => return report_clang(other, &work, cleanup),
    }

    if emit_ir {
        // Beside the output, one file per unit, named as the artefacts are.
        for artefact in &artefacts {
            let beside = with_suffix(&output, &format!("{}.ll", artefact.name));
            if let Err(e) = std::fs::write(&beside, &artefact.ir) {
                eprintln!("nazm: cannot write {}: {e}", beside.display());
            }
        }
    }
    // The intermediates, and only them: the executable is still on its way into place.
    let _ = std::fs::remove_dir_all(&work);

    // Published last, so an existing executable survives every failure above.
    if let Err(e) = std::fs::rename(&tmp_exe, &output) {
        let _ = std::fs::remove_file(&tmp_exe);
        eprintln!("nazm: cannot write {}: {e}", output.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    println!("{}", output.display());
    ExitCode::from(exit::OK)
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
    ir: String,
}

/// Lower a checked compilation into one artefact per module, plus the program's support.
///
/// The order is load order, then the runtime, then the entry. Nothing here reads a
/// directory, so the same program produces the same artefacts in the same order.
fn emit_artefacts(native: &nazm_lir::Native, map: &nazm_span::SourceMap) -> Vec<Artefact> {
    let mut out = Vec::new();
    let mut needs = nazm_lir::Needs::default();
    for (i, unit) in native.units.iter().enumerate() {
        let (ir, reached) = nazm_lir::emit_unit(unit, map);
        needs = needs.union(reached);
        out.push(Artefact {
            name: format!("m{i}"),
            ir,
        });
    }
    // After every unit, because what the runtime must define is the union of what the
    // modules reached — a whole-program fact, and the only one emission needs.
    out.push(Artefact {
        name: "runtime".to_owned(),
        ir: nazm_lir::emit_runtime(needs),
    });
    out.push(Artefact {
        name: "entry".to_owned(),
        ir: nazm_lir::emit_entry(&native.entry, needs),
    });
    out
}

/// What running `clang` produced.
enum Outcome {
    Ran(std::process::Output),
    Missing,
    Failed(std::io::Error),
}

fn clang(extra: &[&str], opt_level: u8, output: &Path, inputs: &[PathBuf]) -> Outcome {
    let mut c = Command::new("clang");
    c.arg(format!("-O{opt_level}"))
        // The modules carry no target triple, so clang supplies the host's and would
        // otherwise warn about overriding nothing.
        .arg("-Wno-override-module")
        .args(extra)
        .arg("-o")
        .arg(output)
        .args(inputs);
    match c.output() {
        Ok(o) => Outcome::Ran(o),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Outcome::Missing,
        Err(e) => Outcome::Failed(e),
    }
}

/// Explain a `clang` failure, keeping the IR when its complaint is about it.
fn report_clang(outcome: Outcome, kept: &Path, cleanup: impl Fn(bool)) -> ExitCode {
    match outcome {
        Outcome::Missing => {
            cleanup(false);
            eprintln!(
                "nazm: `clang` was not found on PATH, and `nazm build` needs it to \
                 assemble the LLVM IR it generates.\n\
                 nazm: install the Xcode command line tools (`xcode-select --install`) \
                 or your platform's clang package. `nazm run` needs none of this."
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Outcome::Failed(e) => {
            cleanup(false);
            eprintln!("nazm: could not run `clang`: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Outcome::Ran(o) => {
            // Keep the IR: clang's complaint is about a file the user cannot see
            // otherwise, and a compiler bug is much easier to report with it in hand.
            cleanup(true);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!(
                "nazm: `clang` rejected the generated IR (exit {:?}). This is a bug in \
                 `nazm build`, not in your program — the IR has been kept at {} to \
                 report it with.",
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

fn with_suffix(output: &Path, ext: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    output.with_file_name(name)
}
