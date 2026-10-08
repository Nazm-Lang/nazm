//! What every `nazm-service` integration test shares: the stack the service runs on, a scratch
//! project on disk, and the repository's own `.nz` files.

// Each test binary includes this whole module and uses only part of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;

/// Run `f` on a thread with the stack the service gives its own parser.
pub(crate) fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

/// A temporary directory holding the named files.
pub(crate) struct Project {
    pub(crate) dir: tempfile::TempDir,
}

impl Project {
    pub(crate) fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, text) in files {
            let p = dir.path().join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, text).unwrap();
        }
        Self { dir }
    }
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    pub(crate) fn root(&self) -> PathBuf {
        self.path("main.nz")
    }
    pub(crate) fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }
    pub(crate) fn write(&self, name: &str, text: &str) {
        std::fs::write(self.path(name), text).unwrap();
    }
}

/// The repository root.
pub(crate) fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

/// Every `.nz` file under `dir`, in path order.
pub(crate) fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for e in entries {
        if e.is_dir() {
            out.extend(walk(&e));
        } else if e.extension().is_some_and(|x| x == "nz") {
            out.push(e);
        }
    }
    out
}
