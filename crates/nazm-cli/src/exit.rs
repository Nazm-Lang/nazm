//! Exit codes, named so the mapping is visible in one place: the v1 contract
//! (`docs/user/stability.md`, *Diagnostics and exit statuses*; frozen in Q1-C1). Each status means one
//! thing, and a command chooses the one its failure is, never the one a neighbour used.

/// Success.
pub(crate) const OK: u8 = 0;
/// The source, the program, a package or what the user asked for is invalid: a diagnostic,
/// a refusal of the request, a command's own "no" (`fmt --check` with a file to change).
pub(crate) const CHECK_FAILED: u8 = 1;
/// The program ran and failed at run time.
pub(crate) const RUNTIME_FAILED: u8 = 2;
/// A file or resource the command needs could not be found or read.
pub(crate) const NO_SUCH_FILE: u8 = 3;
/// The compiler, a tool it runs, or the machine failed: no `clang`, a linker or assembler
/// that failed, an output the filesystem would not take, a compiler defect. Separate from
/// `CHECK_FAILED` on purpose: one means the request is wrong, the other that the machine
/// could not carry out one that is fine.
pub(crate) const TOOLCHAIN_FAILED: u8 = 4;
/// The command line itself is wrong: an unknown command or option, a missing or
/// unparseable argument, options that cannot go together.
pub(crate) const USAGE: u8 = 5;

/// Why a source root could not be decided: a directory or marker that could not be read is
/// 3; a module outside its root, a marker that is not empty, a manifest that is not valid,
/// is the layout's fault — 1. (`nazm_service::root::decide` reports both as text; this
/// reads its two "could not be read" spellings, and `exit_status_tests` holds them.)
pub(crate) fn root_status(why: &str) -> u8 {
    if why.starts_with("could not read") || why.ends_with("is not a directory") {
        NO_SUCH_FILE
    } else {
        CHECK_FAILED
    }
}

/// Why a program could not be loaded: a module that could not be found or read is 3;
/// anything else the loader refuses — a dependency the package does not have, an import
/// that leaves its package — is the program's, 1.
pub(crate) fn load_status(diagnostics: &[nazm_diag::Diagnostic]) -> u8 {
    if !diagnostics.is_empty()
        && diagnostics
            .iter()
            .all(|d| d.code == nazm_diag::codes::MODULE_NOT_FOUND)
    {
        NO_SUCH_FILE
    } else {
        CHECK_FAILED
    }
}

/// A target that could not be had: named on the command line and unknown, a usage error;
/// the host's own and unsupported, the machine's.
pub(crate) const fn target_refused(named: bool) -> u8 {
    if named {
        USAGE
    } else {
        TOOLCHAIN_FAILED
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::exit;

    /// `exit::root_status` reads `root::decide`'s text: its "could not be read" spellings must
    /// stay the ones it looks for, or an unreadable root would be reported as an invalid one.
    #[test]
    fn a_root_that_cannot_be_read_is_3_and_an_invalid_one_1() {
        let d = tempfile::tempdir().unwrap();
        let module = d.path().join("m.nz");
        std::fs::write(&module, "fn main() -> Int { 0 }\n").unwrap();
        // A `--root` that names no directory.
        let missing = d.path().join("nowhere");
        let e = nazm_service::root::decide(&module, Some(&missing)).unwrap_err();
        assert_eq!(exit::root_status(&e), exit::NO_SUCH_FILE, "{e}");
        // A module outside the root it was given.
        let other = tempfile::tempdir().unwrap();
        let e = nazm_service::root::decide(&module, Some(other.path())).unwrap_err();
        assert_eq!(exit::root_status(&e), exit::CHECK_FAILED, "{e}");
        // A marker that is not empty.
        std::fs::write(d.path().join("nazm.root"), "settings\n").unwrap();
        let e = nazm_service::root::decide(&module, None).unwrap_err();
        assert_eq!(exit::root_status(&e), exit::CHECK_FAILED, "{e}");
    }

    #[test]
    fn a_load_is_3_only_when_every_failure_is_a_module_not_found() {
        let nf = nazm_diag::Diagnostic::error(
            nazm_diag::codes::MODULE_NOT_FOUND,
            nazm_span::Span::synthetic(),
            "x",
        );
        let other = nazm_diag::Diagnostic::error(
            nazm_diag::codes::UNKNOWN_DEPENDENCY,
            nazm_span::Span::synthetic(),
            "y",
        );
        assert_eq!(
            exit::load_status(std::slice::from_ref(&nf)),
            exit::NO_SUCH_FILE
        );
        assert_eq!(exit::load_status(&[nf, other.clone()]), exit::CHECK_FAILED);
        assert_eq!(exit::load_status(&[other]), exit::CHECK_FAILED);
        assert_eq!(exit::load_status(&[]), exit::CHECK_FAILED);
    }

    #[test]
    fn statuses_are_the_contracts_six() {
        assert_eq!(
            [
                exit::OK,
                exit::CHECK_FAILED,
                exit::RUNTIME_FAILED,
                exit::NO_SUCH_FILE,
                exit::TOOLCHAIN_FAILED,
                exit::USAGE
            ],
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(exit::target_refused(true), exit::USAGE);
        assert_eq!(exit::target_refused(false), exit::TOOLCHAIN_FAILED);
    }
}
