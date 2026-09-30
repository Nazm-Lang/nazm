//! `nazm mir` — print what a checked program's native functions became, `nazm.mir/1` (N40).
//!
//! The compiler developer's view one level below `nazm core-ir`: each concrete function a
//! native build contains — every generic instance included — as basic blocks with its copies,
//! moves, drops, scope joins and failure edges written out, and the validator's verdict on it.
//! Only for a program that checks, reached the way `nazm build` reaches it. Debug output, not an
//! interchange format; `docs/architecture.md` §7.42 says what the version promises.

use std::path::Path;
use std::process::ExitCode;

use crate::{exit, load};

pub(crate) fn run(path: &Path, compact: bool, spans: bool, declared: Option<&Path>) -> ExitCode {
    let source_root = match crate::root::decide(path, declared) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let loaded = match load::load(path, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let map = &loaded.map;
    let printed =
        nazm_core::on_lowered(map, &loaded.graph, nazm_core::Entry::Absent, |core, res| {
            let program = lower(core, res, &loaded.graph)?;
            let name = |s: nazm_span::Span| {
                let at = map.line_col(s);
                format!("{}:{}:{}", map.name(s.file), at.line, at.column)
            };
            let options = nazm_mir::Options {
                span: if spans { Some(&name) } else { None },
                compact,
            };
            Ok(nazm_mir::print(&program, &options))
        });
    match printed {
        Ok(text) => {
            if compact {
                println!("{}", text.trim_end());
            } else {
                print!("{text}");
            }
            ExitCode::from(exit::OK)
        }
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            ExitCode::from(exit::CHECK_FAILED)
        }
    }
}

/// Core IR to verified MIR: the lowering `nazm build` and `nazm mir` share. A MIR the validator
/// rejects after a clean check is a compiler defect, and nothing is produced from it.
pub(crate) fn lower(
    core: &nazm_core::cir::Program,
    res: &nazm_core::Resolution,
    graph: &nazm_core::sema::ModuleGraph,
) -> Result<nazm_mir::Program, Vec<nazm_diag::Diagnostic>> {
    let program = nazm_mir::lower(core, res, graph)?;
    let broken = nazm_mir::verify(&program);
    if broken.is_empty() {
        return Ok(program);
    }
    Err(broken
        .into_iter()
        .map(|e| {
            nazm_diag::Diagnostic::error(
                nazm_diag::codes::COMPILER_DEFECT,
                nazm_span::Span::synthetic(),
                format!("the compiler produced MIR its validator rejects: {e}"),
            )
            .help("this is a bug in the compiler, not in the program: it checked, and its MIR is malformed")
        })
        .collect())
}
