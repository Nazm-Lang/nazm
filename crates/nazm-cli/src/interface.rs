//! `nazm interface` — write a module's published interface, durably.
//!
//! # What it is for
//!
//! A module is checked against its dependencies' *interfaces* rather than their bodies
//! (`docs/spec.md`). This is the command that makes that a fact about files rather than a
//! fact about one process: it takes a module, and writes what that module offers in a
//! form another run can read.
//!
//! It is **not** a cache. It writes where it is told, reads nothing back, and decides
//! nothing about reuse. `capability-matrix.md` says incremental compilation is absent, and
//! producing an artefact that a cache would need is not having one.
//!
//! # The module it publishes
//!
//! The one named on the command line, treated as the root — so the source root is its
//! directory and its own key is its file name. A module that imports others still needs
//! them to be readable, because its interface names its direct dependencies by their
//! durable keys and it cannot check without them.

use std::path::Path;
use std::process::ExitCode;

use nazm_core::sema::ModuleId;
use nazm_iface::{InterfaceHash, PersistentUnitInterface};

use crate::{exit, load};

pub(crate) fn run(path: &Path, with_hash: bool) -> ExitCode {
    let loaded = match load::load(path) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };

    // Checked first, always. An interface published from source that does not check would
    // be a claim about signatures nothing verified, and the first consumer of it would be
    // checking against a guess.
    //
    // As a **module**, not as a program: a library has no `main` and is exactly the thing
    // whose interface is worth publishing. `nazm_core::Entry` names the distinction.
    let published = nazm_core::on_module_map(&loaded.map, &loaded.graph, |program, _| {
        let unit = nazm_core::Unit::of(program, &loaded.graph, ModuleId::ROOT);
        let mut res = nazm_core::Resolution::default();
        let mut errors = Vec::new();
        let interface = nazm_core::declare_unit(&unit, &mut res, &mut errors);
        Ok::<_, Vec<nazm_diag::Diagnostic>>(PersistentUnitInterface::published(
            &interface,
            &loaded.graph,
        ))
    });

    let published = match published {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::CHECK_FAILED);
        }
        Err(diagnostics) => {
            let n = diagnostics.len();
            crate::emit(&diagnostics, &loaded.map, false);
            eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };

    match published.to_json() {
        Ok(json) => {
            if with_hash {
                match InterfaceHash::of(&published) {
                    Ok(h) => println!("{} {h}", InterfaceHash::ALGORITHM),
                    Err(e) => {
                        eprintln!("nazm: could not fingerprint the interface: {e}");
                        return ExitCode::from(exit::TOOLCHAIN_FAILED);
                    }
                }
            } else {
                println!("{json}");
            }
            ExitCode::from(exit::OK)
        }
        Err(e) => {
            eprintln!("nazm: could not write the interface: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}
