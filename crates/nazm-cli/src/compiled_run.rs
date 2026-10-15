//! `nazm run` (R1.5, D11): the program built with Cranelift, in process, into a private
//! temporary directory, then run with the standard streams it was given, ending with its exit
//! status. `nazm run --interpret` is the interpreter ([`crate::interpret`]); neither falls back to
//! the other.

use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};

use crate::build::{self, Choice};
use crate::exit;

/// The optimisation a compiled run is built with: Cranelift's `speed`, as `--opt-level 2` asks
/// of `nazm build`. On `bench/compare/nazm/fib.nz` a warm build costs the same at 0 and at 2
/// (`docs/performance.md`), so the faster code is not paid for in the build.
const OPT_LEVEL: u8 = 2;

/// Build `path` and run it with `args`; the build's own refusal, or the program's status.
pub(crate) fn run(
    path: &Path,
    json: bool,
    args: &[String],
    profile: Option<&str>,
    harness: Option<&crate::harness::Harness>,
    features: &[String],
) -> ExitCode {
    // Exclusive and private (Q1-A1-02), and removed on every path out.
    let Ok(work) = crate::scratch::temporary("nazm-run-") else {
        eprintln!("nazm: cannot make a working directory to build the program in");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let stem = path
        .file_stem()
        .map_or_else(|| "program".into(), |s| s.to_string_lossy().into_owned());
    let exe = work.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
    let code = built_and_ran(path, &exe, json, args, profile, harness, features);
    let _ = std::fs::remove_dir_all(&work);
    code
}

fn built_and_ran(
    path: &Path,
    exe: &Path,
    json: bool,
    args: &[String],
    profile: Option<&str>,
    harness: Option<&crate::harness::Harness>,
    features: &[String],
) -> ExitCode {
    let built = build::run(
        path,
        Some(exe),
        json,
        false,
        OPT_LEVEL,
        // The project's object store, as `nazm build` keeps it: a second run reuses its units.
        true,
        false,
        false,
        Choice {
            backend: nazm_lir::backend::BackendId::Cranelift,
            target: None,
            link: &[],
            library: false,
            shared: false,
            objects: None,
            stack_watermark: false,
            stack_size: None,
            heap_limit: None,
            deny_warnings: false,
            layout: "aos",
            adapter: None,
            timings: false,
            debug: false,
            release: false,
            runtime: None,
            profile,
            harness,
            features,
            board: None,
            run: true,
        },
    );
    if built != ExitCode::from(exit::OK) {
        return built;
    }
    // Standard input, output and error are inherited: what the program writes reaches the
    // caller as it writes it, and the environment — the result and memory reports among it — is
    // the caller's.
    match Command::new(exe).args(args).status() {
        Ok(status) => passed_through(status),
        Err(e) => {
            eprintln!("nazm: could not run the built program: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}

/// The program's exit status as `nazm run`'s: its own code, or `128 +` the signal that ended it,
/// as a shell reports one.
fn passed_through(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return ExitCode::from(u8::try_from(128 + signal).unwrap_or(u8::MAX));
        }
    }
    ExitCode::from(u8::MAX)
}
