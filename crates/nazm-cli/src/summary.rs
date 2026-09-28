//! `--summary-json` for `nazm check` and `nazm build`: one `nazm.command-summary/1` object,
//! built from the outcome the command reached and the diagnostics it produced. N31.
//!
//! A view added beside the existing output, not a replacement: `--json` and the human
//! rendering are unchanged, and so is every exit status.

use std::path::Path;
use std::process::ExitCode;

use nazm_diag::Diagnostic;
use nazm_service::summary::{summarize, Command, Outcome, Status};
use nazm_span::SourceMap;

/// Print the summary of what the command concluded, alone on stdout, and return the exit
/// status the command returns without it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish(
    command: Command,
    status: Status,
    exit_code: u8,
    diagnostics: &[Diagnostic],
    map: Option<&SourceMap>,
    artifact: Option<String>,
    root: &Path,
    source_root: Option<&Path>,
) -> ExitCode {
    let outcome = Outcome {
        command,
        status,
        exit_code,
        diagnostics,
        map,
        artifact,
    };
    // Linking references analyses the root, on the parser's stack like every analysis.
    let text = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, || summarize(&outcome, root, source_root).to_json())
            .ok()
            .and_then(|h| h.join().ok())
    });
    match text {
        Some(t) => println!("{t}"),
        None => eprintln!("nazm: the summary could not be derived"),
    }
    ExitCode::from(exit_code)
}
