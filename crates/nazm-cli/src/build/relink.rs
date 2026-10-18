//! An executable reused instead of linked again (performance leadership 7.2, a warm build):
//! when the objects, the driver's description of the link and the output's file name are those
//! of the executable a build of this output last linked, that executable is the link's result.
//!
//! The Apple linker writes the same bytes for the same objects under the same file name
//! (`Backend::link_command` sets `ZERO_AR_DATE`; the UUID and the ad-hoc signature are of the
//! content and the name), and so do the others for the same command. So reusing it is the link
//! skipped, not a different executable:
//! `native_cache::an_unchanged_rebuild_reuses_its_executable_byte_for_byte` holds that.
//!
//! One record per output path, beside the project's objects (`.nazm/link`), replaced by each
//! link of that output: what is kept is bounded by the outputs a project builds.

use std::path::{Path, PathBuf};

use crate::backend::Backend;

/// Where a build of `output` keeps its last link, and what that link was of.
pub(super) struct Relink {
    exe: PathBuf,
    key_file: PathBuf,
    key: String,
}

impl Relink {
    /// The record for linking `objects` into `output` with `backend`, or `None` when the
    /// driver would not describe itself or an object cannot be read.
    pub(super) fn of(
        root: &Path,
        backend: &Backend,
        output: &Path,
        objects: &[PathBuf],
    ) -> Option<Self> {
        let description = backend.link_description()?;
        let name = output.file_name()?.to_string_lossy().into_owned();
        let mut h = blake3::Hasher::new();
        let mut field = |bytes: &[u8]| {
            h.update(bytes.len().to_string().as_bytes());
            h.update(b":");
            h.update(bytes);
        };
        field(b"nazm.link/1");
        field(description.as_bytes());
        field(name.as_bytes());
        for o in objects {
            field(&std::fs::read(o).ok()?);
        }
        let key = h.finalize().to_hex().to_string();
        let absolute = std::path::absolute(output).unwrap_or_else(|_| output.to_path_buf());
        let place = blake3::hash(absolute.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        let dir = root.join("link");
        Some(Self {
            exe: dir.join(format!("{place}.exe")),
            key_file: dir.join(format!("{place}.key")),
            key,
        })
    }

    /// Write the executable last linked from exactly these inputs to `tmp_exe`, if there is
    /// one: whether it was.
    pub(super) fn reuse(&self, tmp_exe: &Path) -> bool {
        let Ok(kept) = std::fs::read_to_string(&self.key_file) else {
            return false;
        };
        // The key, and the digest of the executable that link wrote: a record is a file, and
        // an executable that is not the one linked is linked again rather than believed.
        let Some((key, digest)) = kept.split_once('\n') else {
            return false;
        };
        if key != self.key {
            return false;
        }
        let Ok(bytes) = std::fs::read(&self.exe) else {
            return false;
        };
        if blake3::hash(&bytes).to_hex().as_str() != digest {
            return false;
        }
        if std::fs::write(tmp_exe, &bytes).is_err() {
            return false;
        }
        executable(tmp_exe)
    }

    /// Keep what was just linked to `tmp_exe` as this output's last link. A store that will
    /// not take it is not a reason to fail a build.
    pub(super) fn keep(&self, tmp_exe: &Path) {
        let Some(dir) = self.exe.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        // The old key first, so no reader pairs it with the new executable.
        let _ = std::fs::remove_file(&self.key_file);
        let Ok(bytes) = std::fs::read(tmp_exe) else {
            return;
        };
        let part = self
            .exe
            .with_extension(format!("{}.part", std::process::id()));
        if std::fs::write(&part, &bytes).is_err() || std::fs::rename(&part, &self.exe).is_err() {
            let _ = std::fs::remove_file(&part);
            return;
        }
        let key_part = self
            .key_file
            .with_extension(format!("{}.part", std::process::id()));
        let record = format!("{}\n{}", self.key, blake3::hash(&bytes).to_hex());
        if std::fs::write(&key_part, record).is_err()
            || std::fs::rename(&key_part, &self.key_file).is_err()
        {
            let _ = std::fs::remove_file(&key_part);
        }
    }
}

/// Mark `path` executable, as the linker leaves an executable: whether that held.
fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
}
