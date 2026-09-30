//! Restriction profiles on the command line (N47, `docs/architecture.md` §7.49).
//!
//! The profiles in force are the one `--profile` names and every one a package manifest in the
//! build requires; their rules are evaluated over the facts a full check settles
//! (`nazm_service::profile`), before the command does its own work. `general`, alone and with
//! no report asked for, costs nothing: no second check is made.

use std::path::Path;
use std::process::ExitCode;

use nazm_diag::{codes, Diagnostic};
use nazm_service::profile::{self, Profile};
use nazm_span::Span;

use crate::exit;

fn unknown(name: &str, whose: &str) -> ExitCode {
    let known: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
    let d = Diagnostic::error(
        codes::UNKNOWN_PROFILE,
        Span::synthetic(),
        format!("{whose} names the profile `{name}`, which does not exist"),
    )
    .help(format!("the profiles are {}", known.join(", ")));
    eprintln!("error[{}]: {}", d.code, d.message);
    if let Some(h) = &d.help {
        eprintln!("  = help: {h}");
    }
    ExitCode::from(exit::CHECK_FAILED)
}

/// Hold the program rooted at `file` to the profiles in force.
///
/// # Errors
///
/// The exit code, once reported: an unknown profile (`N0511`), or a rule a profile in force
/// refuses (`N0510`, one per rule).
pub(crate) fn enforce(
    file: &Path,
    named: Option<&str>,
    locked_flag: bool,
    report: bool,
) -> Result<(), ExitCode> {
    let mut profiles = Vec::new();
    if let Some(n) = named {
        profiles.push(Profile::by_name(n).ok_or_else(|| unknown(n, "`--profile`"))?);
    }
    let graph = crate::package::graph_for(file).and_then(Result::ok);
    if let Some(g) = &graph {
        for p in &g.packages {
            if let Some(n) = &p.profile {
                let prof = Profile::by_name(n)
                    .ok_or_else(|| unknown(n, &format!("the package `{}`'s manifest", p.name)))?;
                profiles.push(prof);
            }
        }
    }
    let strict = profiles.iter().any(|p| *p != Profile::General);
    if !strict && !report {
        return Ok(());
    }
    // A package build held to its lockfile: `--locked` already verified it; a profile that
    // requires it verifies it here, so the flag is not needed to satisfy the rule.
    let locked = graph
        .as_ref()
        .is_some_and(|g| locked_flag || nazm_package::verify_locked(g).is_ok());
    let Ok(root) = nazm_service::root::decide(file, None) else {
        return Ok(());
    };
    let Ok(loaded) = nazm_service::load::load(file, root.path()) else {
        // The command reports what could not be loaded, in its own words.
        return Ok(());
    };
    let map = &loaded.map;
    let evaluated = nazm_core::on_checked_map(map, &loaded.graph, |_, res| {
        Ok(profile::evaluate(&profiles, &loaded.graph, res, locked))
    });
    let Ok(evaluation) = evaluated else {
        // A program that does not check is refused by the command, which says why.
        return Ok(());
    };
    if report {
        println!("{}", profile::report(&evaluation, map));
    }
    if evaluation.accepted() {
        return Ok(());
    }
    let diagnostics = profile::diagnostics(&evaluation);
    crate::emit(&diagnostics, map, false);
    let n = diagnostics.len();
    eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
    Err(ExitCode::from(exit::CHECK_FAILED))
}
