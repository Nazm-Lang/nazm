//! `nazm fmt` — the canonical layout, written back.
//!
//! Per **file**, not per program: a `use` makes several files one program for checking,
//! and layout is not a property of a program. Formatting the merged buffer would be
//! formatting something that exists nowhere on disk.
//!
//! `--check` writes nothing and fails if any file would change, which is the form a CI
//! job and a pre-commit hook both want.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::exit;

pub(crate) fn run(files: &[PathBuf], check: bool) -> ExitCode {
    let mut changed = Vec::new();
    let mut failed = false;

    for path in files {
        let Ok(original) = std::fs::read_to_string(path) else {
            eprintln!("nazm: cannot read {}", path.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        };
        let formatted = match nazm_core::__syntax::format(&original) {
            Ok(text) => text,
            Err(diagnostics) => {
                // The file's own text, so the caret lands in the file the reader named
                // rather than in a buffer assembled from several.
                let file = nazm_span::SourceMap::single(path.display().to_string(), original);
                crate::emit(&diagnostics, &file, false);
                failed = true;
                continue;
            }
        };
        if formatted == original {
            continue;
        }
        changed.push(path.clone());
        if !check {
            if let Err(e) = std::fs::write(path, &formatted) {
                eprintln!("nazm: cannot write {}: {e}", path.display());
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
        }
    }

    if failed {
        eprintln!("nazm: some files could not be formatted because they do not lex");
        return ExitCode::from(exit::CHECK_FAILED);
    }

    report(&changed, check, files.len())
}

fn report(changed: &[PathBuf], check: bool, total: usize) -> ExitCode {
    if check {
        for p in changed {
            println!("{}", p.display());
        }
        if changed.is_empty() {
            println!("nazm: {total} file{} already formatted", plural(total));
            return ExitCode::from(exit::OK);
        }
        eprintln!(
            "nazm: {} file{} would change; run `nazm fmt` without --check",
            changed.len(),
            plural(changed.len())
        );
        return ExitCode::from(exit::CHECK_FAILED);
    }
    for p in changed {
        println!("{}", p.display());
    }
    println!(
        "nazm: {} of {total} file{} rewritten",
        changed.len(),
        plural(total)
    );
    ExitCode::from(exit::OK)
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
