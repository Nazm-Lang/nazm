//! The prologue most commands open with: decide the program's source root, load its modules, and
//! on failure say why and return the status the exit contract gives it.

use std::path::Path;

use nazm_service::load::Loaded;
use nazm_service::{load, root};

/// `file`'s modules, loaded from the source root `declared` or the one found for it; or, having
/// printed why not (diagnostics as JSON when `json`), the exit status for the failure.
pub(crate) fn opened(file: &Path, declared: Option<&Path>, json: bool) -> Result<Loaded, u8> {
    let source_root = root::decide(file, declared).map_err(|e| {
        eprintln!("nazm: {e}");
        crate::exit::root_status(&e)
    })?;
    load::load(file, source_root.path()).map_err(|(partial, diagnostics)| {
        crate::emit(&diagnostics, &partial.map, json);
        crate::exit::load_status(&diagnostics)
    })
}
