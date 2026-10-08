//! What a command needs settled before it sees its file: library packages, package entries and
//! restriction profiles.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::Command;
use crate::{checking, package, profile};

/// What a command needs settled before it sees its file: a library package is checked as its
/// modules, a package directory stands for its `main` (N46), and the restriction profiles in
/// force are held (N47). `Some` when that alone decides the outcome.
pub(crate) fn prepare(command: &mut Command) -> Option<ExitCode> {
    // A library package — no `main` — is checked as its modules (N74, §7.76).
    if let Command::Check { file, json, .. } = command {
        if let Some(modules) = package::library_modules(file) {
            return Some(checking::library(&modules, *json));
        }
    }
    // What the user named, before a package directory becomes its entry file: profiles are
    // held to that package's manifests (Q1-C-04).
    let named_path = match command {
        Command::Run { file, .. } | Command::Check { file, .. } | Command::Build { file, .. } => {
            file.clone()
        }
        _ => PathBuf::new(),
    };
    match command {
        Command::Run { file, locked, .. }
        | Command::Check { file, locked, .. }
        | Command::Build { file, locked, .. } => match package::entry(file, *locked) {
            Ok(Some(main)) => *file = main,
            Ok(None) => {}
            Err(code) => return Some(code),
        },
        Command::Inspect { file, .. }
        | Command::ExplainCost { file, .. }
        | Command::ExplainFlow { file, .. } => match package::entry(file, false) {
            Ok(Some(main)) => *file = main,
            Ok(None) => {}
            Err(code) => return Some(code),
        },
        _ => {}
    }
    // Restriction profiles (N47): held before the command does its own work, from the facts a
    // full check settles.
    let wants_report = matches!(
        command,
        Command::Check {
            profile_report: true,
            ..
        }
    );
    match command {
        Command::Run {
            file,
            locked,
            profile,
            ..
        }
        | Command::Build {
            file,
            locked,
            profile,
            ..
        } => {
            if let Err(code) =
                profile::enforce(file, &named_path, profile.as_deref(), *locked, false)
            {
                return Some(code);
            }
        }
        Command::Check {
            file,
            locked,
            profile,
            ..
        } => {
            if let Err(code) =
                profile::enforce(file, &named_path, profile.as_deref(), *locked, wants_report)
            {
                return Some(code);
            }
        }
        _ => {}
    }
    None
}
