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
use std::process::ExitCode;

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
}

/// Compile, or explain what stopped it.
pub(crate) fn run(
    input: &Path,
    out: Option<&Path>,
    json: bool,
    emit_ir: bool,
    opt_level: u8,
    cache: bool,
    with_report: bool,
) -> ExitCode {
    // Discovered, never given: `nazm build` takes no `--source-root`, and the marker is
    // how a project declares one. It decides every module's durable identity, which is
    // the name the generated code carries and therefore part of what an object is keyed
    // on.
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
    // What a generated artefact calls each file: its durable identity where it has one,
    // so the same project built from two checkouts produces the same bytes.
    let names = nazm_lir::SourceNames::of(map, &loaded.graph);
    let artefacts = match nazm_core::on_checked_map(map, &loaded.graph, |program, resolution| {
        nazm_lir::lower(program, resolution, &loaded.graph, &names)
            .map(|native| emit_artefacts(&native, &names))
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

    // Asked once, before anything is compiled: what this backend is, and what it would
    // do. `crate::backend` says why the answer comes from the compiler rather than from
    // the three arguments this one passes.
    let backend = if cache {
        Backend::discover(opt_level, &work)
    } else {
        Backend::unidentified(opt_level)
    };
    // A backend that would not say what it is gets no cache. Guessing a configuration is
    // how one target's objects end up in another's build.
    let store: Option<ObjectStore> = backend
        .settled()
        .map(|_| crate::derived::objects(source_root.path()));

    let mut tally = Native {
        units: artefacts.len(),
        ..Native::default()
    };
    let mut objects: Vec<PathBuf> = Vec::new();
    for artefact in &artefacts {
        let ll = work.join(format!("{}.ll", artefact.name));
        let obj = work.join(format!("{}.o", artefact.name));

        // The key is over the bytes that are about to be handed over, so it cannot
        // describe anything but what is compiled.
        let key = backend
            .settled()
            .map(|s| ObjectKey::of(LlvmInput::of(artefact.ir.as_bytes()), s.identity, s.config));
        if let (Some(store), Some(key)) = (store.as_ref(), key) {
            match store.get(&key) {
                ObjectFound::Hit(bytes) => {
                    // Materialised into this build's own directory. A cached entry is an
                    // envelope, not an object, and it is never written to or linked in
                    // place — it is immutable derived data.
                    if std::fs::write(&obj, &bytes).is_ok() {
                        tally.reused += 1;
                        objects.push(obj);
                        continue;
                    }
                }
                ObjectFound::Miss => {}
                ObjectFound::Rejected(_) => tally.rejected += 1,
            }
        }

        if let Err(e) = std::fs::write(&ll, &artefact.ir) {
            cleanup(false);
            eprintln!("nazm: cannot write {}: {e}", ll.display());
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        match backend.compile(&ll, &obj) {
            Outcome::Ran(o) if o.status.success() => tally.compiled += 1,
            other => return report_clang(other, &ll, cleanup),
        }

        // Published after it exists and from the bytes that were produced, so what is
        // stored is what was linked. A store that will not take an entry is not a reason
        // to fail a build: the disk is full, or the tree is read-only, and the program is
        // fine either way.
        if let (Some(store), Some(key), Some(settled), Ok(bytes)) =
            (store.as_ref(), key, backend.settled(), std::fs::read(&obj))
        {
            let entry = ObjectArtifact::of(
                LlvmInput::of(artefact.ir.as_bytes()),
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
    match backend.link(&tmp_exe, &objects) {
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
    if with_report {
        println!("{}", report(source_root.how(), map.len(), tally));
    }
    println!("{}", output.display());
    ExitCode::from(exit::OK)
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
        r#"{{"schema":"nazm.build-report/1","root":"{}","semantic":{{"checked":{modules},"reused":0}},"native":{{"units":{},"compiled":{},"reused":{},"rejected":{},"stored":{}}},"link":{{"performed":true}}}}"#,
        root.as_str(),
        t.units,
        t.compiled,
        t.reused,
        t.rejected,
        t.stored,
    )
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
fn emit_artefacts(native: &nazm_lir::Native, names: &nazm_lir::SourceNames<'_>) -> Vec<Artefact> {
    let mut out = Vec::new();
    let mut needs = nazm_lir::Needs::default();
    for (i, unit) in native.units.iter().enumerate() {
        let (ir, reached) = nazm_lir::emit_unit(unit, names);
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

fn with_suffix(output: &Path, ext: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    output.with_file_name(name)
}
