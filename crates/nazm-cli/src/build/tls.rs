//! What a program that reaches TLS (v1 track C1, `docs/spec.md`, *TLS*) links beside the runtime:
//! the platform's TLS library, found on the host and never bundled.
//!
//! On macOS that is OpenSSL 3 as Homebrew installs it (`brew install openssl@3`), linked
//! dynamically by the absolute name its libraries carry, so Homebrew's security updates reach every
//! program built against it; and the Security framework, whose trust evaluation decides a client's
//! verdict. `NAZM_OPENSSL_DIR` names another installation (its `lib` directory is searched). On
//! Linux it is the distribution's `libssl.so.3`, linked dynamically by that name for the same
//! reason, so no development package is needed. A target with no binding is refused by name
//! (`N0830`) before anything is linked. Only an executable whose live code reaches the engine
//! records the library: `link::linked` links without it first.

use std::path::{Path, PathBuf};

use nazm_lir::backend::{Os, Target};

/// The variable naming an OpenSSL 3 installation to link against instead of the one found.
pub(crate) const OPENSSL_DIR: &str = "NAZM_OPENSSL_DIR";

/// Where Homebrew (Apple silicon, then Intel) and MacPorts install OpenSSL 3.
const MAC_PLACES: [&str; 3] = [
    "/opt/homebrew/opt/openssl@3",
    "/usr/local/opt/openssl@3",
    "/opt/local",
];

/// `N0830`: the program reaches `@std/tls`, and the target has no TLS binding.
pub(super) fn no_binding(triple: &str) -> nazm_diag::Diagnostic {
    nazm_diag::Diagnostic::error(
        nazm_diag::codes::TLS_TARGET,
        nazm_span::Span::synthetic(),
        format!("TLS has no binding for `{triple}`"),
    )
    .help(
        "TLS is a runtime part with a binding per target: OpenSSL 3 on macOS (with the Security \
         framework's trust) and on Linux. On WebAssembly the host terminates TLS; a board has \
         none",
    )
}

/// An OpenSSL 3 installation's directory, if it has the libraries.
fn installed(dir: &Path) -> bool {
    let lib = dir.join("lib");
    lib.join("libssl.3.dylib").is_file() || lib.join("libssl.dylib").is_file()
}

/// The OpenSSL 3 this Mac links against: `NAZM_OPENSSL_DIR`, or the first installation found.
fn mac_openssl() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os(OPENSSL_DIR) {
        let dir = PathBuf::from(dir);
        return if installed(&dir) {
            Ok(dir)
        } else {
            Err(format!(
                "{OPENSSL_DIR} is {}, which has no lib/libssl.3.dylib",
                dir.display()
            ))
        };
    }
    MAC_PLACES
        .iter()
        .map(PathBuf::from)
        .find(|d| installed(d))
        .ok_or_else(|| {
            "this program uses TLS, which links OpenSSL 3, and none was found: install it with \
             `brew install openssl@3`, or name an installation with NAZM_OPENSSL_DIR"
                .to_owned()
        })
}

/// The linker's arguments for a program that reaches TLS, after its objects.
///
/// # Errors
///
/// Why the platform's TLS library cannot be linked on this host.
pub(super) fn link_args(target: &Target) -> Result<Vec<String>, String> {
    let dir = std::env::var_os(OPENSSL_DIR).map(PathBuf::from);
    match target.os {
        Os::Macos => {
            let dir = mac_openssl()?;
            Ok(vec![
                format!("-L{}", dir.join("lib").display()),
                "-lssl".to_owned(),
                "-lcrypto".to_owned(),
                "-framework".to_owned(),
                "Security".to_owned(),
                "-framework".to_owned(),
                "CoreFoundation".to_owned(),
            ])
        }
        Os::Linux => {
            let mut args: Vec<String> = dir
                .map(|d| vec![format!("-L{}", d.join("lib").display())])
                .unwrap_or_default();
            // By the runtime library's own name: no development package is needed to link.
            args.extend(["-l:libssl.so.3".to_owned(), "-l:libcrypto.so.3".to_owned()]);
            Ok(args)
        }
        _ => Err(format!("TLS has no binding for `{}`", target.triple)),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn linux_links_the_distribution_s_openssl_and_nothing_else() {
        let t = Target::parse("x86_64-unknown-linux-gnu").unwrap_or_else(|e| panic!("{e}"));
        if std::env::var_os(OPENSSL_DIR).is_none() {
            assert_eq!(
                link_args(&t),
                Ok(vec![
                    "-l:libssl.so.3".to_owned(),
                    "-l:libcrypto.so.3".to_owned()
                ])
            );
        }
    }

    #[test]
    fn a_target_without_a_binding_is_refused_by_name() {
        let t = Target::parse("x86_64-pc-windows-gnu").unwrap_or_else(|e| panic!("{e}"));
        let why = link_args(&t).unwrap_err();
        assert!(why.contains("x86_64-pc-windows-gnu"), "{why}");
    }
}
