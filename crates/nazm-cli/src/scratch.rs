//! Scratch directories the CLI builds into and runs from (Q1-A1-02, Q1-A1-03).
//!
//! Each is created **exclusively** — `mkdir`, never "create unless it exists" — and, on Unix, with
//! mode 0700. A name someone else already holds, as a directory or as a symlink to one, is not
//! adopted: the next name is tried. So nothing this process compiles, links or executes lands in a
//! directory another user prepared, on a shared `/tmp` or beside an output in a shared directory.

use std::path::{Path, PathBuf};

/// A fresh directory in `parent` named `{stem}{process id}`, or `{stem}{process id}-{n}` when that
/// is taken. `parent` is created first if it does not exist.
pub(crate) fn exclusive(parent: &Path, stem: &str) -> std::io::Result<PathBuf> {
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    let mut last = None;
    for n in 0..64u32 {
        let name = if n == 0 {
            format!("{stem}{pid}")
        } else {
            format!("{stem}{pid}-{n}")
        };
        let dir = parent.join(name);
        match make(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free scratch directory name")))
}

/// A fresh directory under the system's temporary directory.
pub(crate) fn temporary(stem: &str) -> std::io::Result<PathBuf> {
    exclusive(&std::env::temp_dir(), stem)
}

#[cfg(unix)]
fn make(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn make(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().create(dir)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_name_already_taken_is_never_adopted() {
        let base = std::env::temp_dir().join(format!("nazm-scratch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let pid = std::process::id();
        // A directory, and a symlink to one, already at the first two names.
        std::fs::create_dir(base.join(format!("w-{pid}"))).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&base, base.join(format!("w-{pid}-1"))).unwrap();
        // Windows: a link needs a privilege an ordinary account lacks; a directory takes it.
        #[cfg(not(unix))]
        std::fs::create_dir(base.join(format!("w-{pid}-1"))).unwrap();
        let got = exclusive(&base, "w-").unwrap();
        assert_eq!(
            got.file_name().unwrap().to_string_lossy(),
            format!("w-{pid}-2")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&got).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        std::fs::remove_dir_all(&base).unwrap();
    }
}
