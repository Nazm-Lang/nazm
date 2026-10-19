//! `nazm run`: load, check and evaluate a program, then report its value and the memory and
//! scheduler counters a compiled program would report.

use std::process::ExitCode;

use nazm_core::Outcome;
use nazm_service::root;

use crate::{emit, exit};

/// `nazm run`: check the program and, when it passes, evaluate it.
pub(crate) fn run(
    path: &std::path::Path,
    json: bool,
    limits: nazm_core::eval::Limits,
    args: Vec<String>,
    profile: Option<&str>,
    harness: Option<&crate::harness::Harness>,
    features: &[String],
) -> ExitCode {
    // `nazm run` evaluates; it persists nothing and reuses nothing, so where the project
    // begins is not its question. The root is discovered rather than given because the
    // loader computes a durable key for every module whether or not this command has a
    // use for one.
    let source_root = match root::decide(path, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(crate::exit::root_status(&e));
        }
    };

    // The root file and everything it imports, and how they relate. A program with no
    // `use` takes the same path and produces a graph of one module.
    let mut loaded = match crate::harness::loaded(path, source_root.path(), harness) {
        Ok(f) => f,
        // What was loaded comes back with the errors, so the caret lands on the `use`
        // that could not be resolved rather than on nothing.
        Err((partial, diagnostics)) => {
            emit(&diagnostics, &partial.map, json);
            return ExitCode::from(crate::exit::load_status(&diagnostics));
        }
    };
    // The interpreter runs the program on this host (Gate 2, §17).
    let config = match crate::profile::configuration(path, profile, None, features) {
        Ok(c) => c,
        Err(code) => return code,
    };
    loaded.graph = std::mem::take(&mut loaded.graph).with_config(config);
    let map = &loaded.map;

    // Checking always runs; evaluation only when it passed. Running a program known to be
    // ill-typed produces a second, less useful error about the same mistake.
    let outcome = nazm_core::run_map(map, &loaded.graph, limits, args);

    // After evaluation, so every environment frame and every value it held has been
    // dropped. The same line a compiled program writes, from the same environment
    // variable, so the two implementations can be compared on reclamation and not only on
    // the value they produce. `docs/user/spec.md` says when storage is reclaimed; this is how a
    // test checks that both of them did it.
    if std::env::var_os(nazm_lir::MEMORY_VARIABLE).is_some() {
        let c = nazm_core::eval::memory_counts();
        let mut line = String::from("nazm-memory:");
        for (name, (allocated, reclaimed)) in [
            ("sequences", c.sequences),
            ("strings", c.strings),
            ("channels", c.channels),
            ("closures", c.closures),
        ] {
            use std::fmt::Write as _;
            let _ = write!(
                line,
                " {name} allocated={allocated} reclaimed={reclaimed} live={}",
                allocated - reclaimed
            );
        }
        eprintln!("{line}");
    }
    // The scheduler's counters (N54), on their own variable, as a compiled program has them.
    if std::env::var_os(nazm_lir::runtime::SCHED_VARIABLE).is_some() {
        let c = nazm_core::eval::sched_counts();
        eprintln!(
            "nazm-sched: spawned={} joined={} selects={} timeouts={}",
            c.spawned, c.joined, c.selects, c.timeouts
        );
    }

    match outcome {
        Outcome::Value(v) => end_with_result(&v),
        Outcome::CheckFailed(ds) => {
            emit(&ds, map, json);
            eprintln!(
                "nazm: {} error{}",
                ds.len(),
                if ds.len() == 1 { "" } else { "s" }
            );
            ExitCode::from(exit::CHECK_FAILED)
        }
        Outcome::RuntimeFailed(d) => {
            emit(std::slice::from_ref(&d), map, json);
            ExitCode::from(exit::RUNTIME_FAILED)
        }
    }
}

/// How a program ends with `main`'s result (R1, 5b.3): with it as the exit status — written to
/// no stream — unless the result report is asked for (`NAZM_RESULT_REPORT`), which prints it and
/// ends with `0` as every program did before. A result outside `0..=255` is `N0416`'s failure,
/// as `exit_with` of one is, and status 2: what a compiled program does.
pub(crate) fn end_with_result(v: &nazm_core::Value) -> ExitCode {
    if std::env::var_os(nazm_lir::RESULT_VARIABLE).is_some() {
        return crate::report_ok(&v.to_string(), None);
    }
    match v {
        nazm_core::Value::Int(n) => match u8::try_from(*n) {
            Ok(code) => ExitCode::from(code),
            Err(_) => {
                eprintln!("{}", nazm_lir::RESULT_STATUS);
                ExitCode::from(2)
            }
        },
        other => crate::report_ok(&other.to_string(), None),
    }
}
