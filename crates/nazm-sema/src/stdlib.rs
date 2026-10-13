//! The standard library (N45): toolchain-owned modules a program imports by name.
//!
//! # What it is
//!
//! Ordinary Nazm source — `library/std/*.nz` — checked by the same checker, published through
//! the same interface, and reached through the same import edge any `use` creates. What is
//! special is only where each comes from and what it is called: `use "@std/text";` names the
//! module keyed [`ModuleKey::std`]`("text")`, whose bytes this compiler carries, so a
//! compilation needs no installation directory. Unlike the prelude, none is imported unless a
//! module writes the `use`. `docs/spec.md`, *The standard library*, owns the rules;
//! `docs/architecture.md` §7.47 says why the boundary is here.
//!
//! # The bytes
//!
//! Each file is the definition. Nothing here describes a function in Rust.

use crate::key::ModuleKey;

/// How an import names a standard module: `@std/` and the module's name.
pub const PREFIX: &str = "@std/";

/// Every standard module, by name, with its source exactly as the repository holds it.
pub const MODULES: &[(&str, &str)] = &[
    ("audit", include_str!("../../../library/std/audit.nz")),
    ("binary", include_str!("../../../library/std/binary.nz")),
    ("blake3", include_str!("../../../library/std/blake3.nz")),
    ("chan", include_str!("../../../library/std/chan.nz")),
    ("deque", include_str!("../../../library/std/deque.nz")),
    ("dist", include_str!("../../../library/std/dist.nz")),
    ("env", include_str!("../../../library/std/env.nz")),
    ("error", include_str!("../../../library/std/error.nz")),
    ("file", include_str!("../../../library/std/file.nz")),
    ("fmt", include_str!("../../../library/std/fmt.nz")),
    ("fs", include_str!("../../../library/std/fs.nz")),
    ("hal", include_str!("../../../library/std/hal.nz")),
    ("hash", include_str!("../../../library/std/hash.nz")),
    ("hashmap", include_str!("../../../library/std/hashmap.nz")),
    ("hashset", include_str!("../../../library/std/hashset.nz")),
    ("io", include_str!("../../../library/std/io.nz")),
    ("ioerror", include_str!("../../../library/std/ioerror.nz")),
    ("json", include_str!("../../../library/std/json.nz")),
    ("linalg", include_str!("../../../library/std/linalg.nz")),
    ("log", include_str!("../../../library/std/log.nz")),
    ("map", include_str!("../../../library/std/map.nz")),
    ("net", include_str!("../../../library/std/net.nz")),
    ("num", include_str!("../../../library/std/num.nz")),
    ("option", include_str!("../../../library/std/option.nz")),
    ("path", include_str!("../../../library/std/path.nz")),
    ("process", include_str!("../../../library/std/process.nz")),
    ("random", include_str!("../../../library/std/random.nz")),
    ("result", include_str!("../../../library/std/result.nz")),
    ("seq", include_str!("../../../library/std/seq.nz")),
    ("sha256", include_str!("../../../library/std/sha256.nz")),
    ("show", include_str!("../../../library/std/show.nz")),
    ("sort", include_str!("../../../library/std/sort.nz")),
    ("test", include_str!("../../../library/std/test.nz")),
    ("text", include_str!("../../../library/std/text.nz")),
    ("time", include_str!("../../../library/std/time.nz")),
    ("toml", include_str!("../../../library/std/toml.nz")),
];

/// The standard module an import path names, if it names one: its name and its source.
#[must_use]
pub fn module(import: &str) -> Option<(&'static str, &'static str)> {
    let name = import.strip_prefix(PREFIX)?;
    MODULES.iter().find(|(n, _)| *n == name).copied()
}

/// Whether an import path is reserved for the standard library: it begins with `@std/`. Such
/// a path is never looked for on disk, so `use "@std/text";` cannot be answered by a project
/// file. Other paths beginning with `@` are ordinary relative paths, as they always were — a
/// project may keep files under a directory named `@core`.
#[must_use]
pub fn reserved(import: &str) -> bool {
    import.starts_with(PREFIX)
}

/// The name a standard module's file is shown under in a diagnostic.
#[must_use]
pub fn file_name(name: &str) -> String {
    format!("{PREFIX}{name}.nz")
}

/// A standard module's durable identity.
#[must_use]
pub fn key(name: &str) -> ModuleKey {
    ModuleKey::std(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_module_is_found_by_name_and_only_by_name() {
        assert_eq!(module("@std/text").map(|m| m.0), Some("text"));
        assert!(module("@std/nope").is_none());
        assert!(module("std/text").is_none());
        assert!(module("@std/text.nz").is_none());
        assert!(
            reserved("@std/anything") && !reserved("@core/prelude.nz") && !reserved("lib/a.nz")
        );
        let mut names: Vec<&str> = MODULES.iter().map(|m| m.0).collect();
        let sorted = {
            let mut v = names.clone();
            v.sort_unstable();
            v
        };
        assert_eq!(names, sorted, "listed by name");
        names.dedup();
        assert_eq!(names.len(), MODULES.len());
    }

    #[test]
    fn no_project_path_is_keyed_as_a_standard_module() {
        assert!(ModuleKey::relative("@std/text").is_none());
        assert!(ModuleKey::relative("@std/text.nz").is_none());
        assert!(ModuleKey::relative("@std").is_none());
        assert!(ModuleKey::std("text").is_std());
        assert!(!ModuleKey::prelude().is_std());
    }
}
