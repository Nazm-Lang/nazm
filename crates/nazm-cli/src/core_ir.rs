//! `nazm core-ir` — print what a checked program became, `nazm.core-ir/3` (N39; `/2` since
//! N78, `/3` since R3).
//!
//! A compiler developer's question — *what did this source become, which definition does
//! this call reach, which loop does this `break` leave* — answered from the representation
//! both backends start from, rather than from LLVM IR or the interpreter's behaviour. The
//! same one lowering `nazm run` and `nazm build` use, reached the same way: only for a
//! program that checks. Not a user-facing syntax and not an interchange format;
//! `docs/architecture.md` §7.41 says what the version promises.

use std::path::Path;
use std::process::ExitCode;

use crate::exit;

pub(crate) fn run(
    path: &Path,
    compact: bool,
    spans: bool,
    sources: bool,
    declared: Option<&Path>,
) -> ExitCode {
    let loaded = match crate::opened::opened(path, declared, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    // As a module: a library with no `main` has Core IR too, and prints with no entry.
    let printed = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            let options = nazm_core::cir::Options { spans, compact };
            Ok(if sources {
                nazm_core::cir::print_with_sources(core, res, &loaded.graph, options, &loaded.map)
            } else {
                nazm_core::cir::print(core, res, &loaded.graph, options)
            })
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
