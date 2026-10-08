//! `nazm core-ir` — print what a checked program became, `nazm.core-ir/2` (N39; `/2` since N78).
//!
//! A compiler developer's question — *what did this source become, which definition does
//! this call reach, which loop does this `break` leave* — answered from the representation
//! both backends start from, rather than from LLVM IR or the interpreter's behaviour. The
//! same one lowering `nazm run` and `nazm build` use, reached the same way: only for a
//! program that checks. Not a user-facing syntax and not an interchange format;
//! `docs/architecture.md` §7.41 says what the version promises.

use std::path::Path;
use std::process::ExitCode;

use crate::{exit, load};

pub(crate) fn run(path: &Path, compact: bool, spans: bool, declared: Option<&Path>) -> ExitCode {
    let source_root = match crate::root::decide(path, declared) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(crate::exit::root_status(&e));
        }
    };
    let loaded = match load::load(path, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(crate::exit::load_status(&diagnostics));
        }
    };
    // As a module: a library with no `main` has Core IR too, and prints with no entry.
    let printed = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            Ok(nazm_core::cir::print(
                core,
                res,
                &loaded.graph,
                nazm_core::cir::Options { spans, compact },
            ))
        },
    );
    match printed {
        Ok(text) => {
            if compact {
                println!("{text}");
            } else {
                print!("{text}");
            }
            ExitCode::from(exit::OK)
        }
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, &loaded.map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            ExitCode::from(exit::CHECK_FAILED)
        }
    }
}
