//! The repository context map and the minimum task context planner. N33.
//!
//! # The contract
//!
//! **Not another source of truth: an index and a planner over the ones that exist.** Every
//! entity and every relationship names the authority it is read from, and nothing here
//! decides what a program means, what a document says or which test observes what:
//!
//! | Facts | Authority |
//! | --- | --- |
//! | packages, their targets, their dependencies, their documentation | the Cargo manifests (`[package.metadata.nazm] docs`), read as Cargo reads them |
//! | Nazm source roots and their compilation roots | [`manifest::SOURCES`], the one list of them |
//! | modules a compilation loads, its durable definitions, their links, callers, diagnostics | the compiler, through `nazm-service` (N17, N22, N25, N27, N28, N30) |
//! | documents, sections, their authority and links | `nazm-docs` (N32) |
//! | mutation profiles, which tests observe a path, which test kills which mutant | the mutation catalogue |
//! | published machine schemas | `schema/*.json`, by the `schema` constant each declares |
//!
//! [`map`] answers what exists and how it relates, as `nazm.repository-map/1`: identities,
//! kinds, parents, structural relationships and where to retrieve more — never a source
//! body or a section's text. [`task`] answers, for structured seeds and an intent, the
//! smallest context this repository can justify, as `nazm.task-context/1`: each item with
//! the reason it is there, its authority and how to retrieve it again, the seed's exact
//! source and the selected sections' exact text, within an explicit structural budget.
//!
//! # Identity
//!
//! `kind:authority-id`: `package:nazm-docs`, `target:nazm-docs:test:corpus`,
//! `sources:compiler`, `root:compiler:emit.nz`, `module:compiler:analyse.nz`,
//! `def:compiler:analyse.nz::fn check` (the compiler's durable definition key under a
//! source root), `doc:spec:generics` (N32's id), `schema:nazm.context/1`,
//! `profile:checker`, `mutation:n32-an-anchor-is-ignored`,
//! `test:nazm-docs:test:corpus::a_duplicate_id_is_refused` (a catalogue killer), and
//! `diagnostic:examples-bad:return_type_mismatch.nz:N0300@…` (N30's id within a root).
//! Never an absolute path, a line, an offset, a random value, a process, a time or a
//! commit; a path appears only as a location.
//!
//! # Freshness
//!
//! The repository `state` is BLAKE3 over a domain tag, the compiler, the manifest, the
//! documentation corpus's own state (N32), every Cargo manifest and the targets Cargo would
//! find, the mutation catalogue, every published schema, and every `.nz` file directly in a
//! source root — each by its repository-relative path and exact bytes. A task asked for
//! with another state is `stale_state`. A compilation that loads a file outside its source
//! root is not in that state, and is refused rather than answered from.

mod cargo;
mod catalogue;
pub mod manifest;
mod map;
mod repository;
mod sources;
mod task;

use std::path::Path;

use serde::Serialize;

pub use manifest::{Layout, SourceRoot, NAZM, SOURCES};
pub use map::{
    validate, Class, Delegated, Entity, MapResult, Omission, Relation, RepositoryMap, Retrieval,
};
pub use task::{
    Bound, Budget, Exclusion, Intent, Item, Refusal, TaskContext, TaskOmission, TaskRequest,
    TaskResult, Used, FAN_OUT, MAX_SEEDS,
};

/// The schema every repository map declares.
pub const MAP_SCHEMA: &str = "nazm.repository-map/1";
/// The schema every task context declares.
pub const TASK_SCHEMA: &str = "nazm.task-context/1";
/// The compiler whose facts the map and every task context are of.
pub const COMPILER: &str = concat!("nazm ", env!("CARGO_PKG_VERSION"));

/// Why a repository has no map, or a task no context: each problem by kind, with the
/// offending id, path or name where there is one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Problem {
    pub kind: &'static str,
    pub value: Option<String>,
}

impl Problem {
    pub(crate) fn new(kind: &'static str, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: Some(value.into()),
        }
    }
}

/// The map of the repository under `root`.
#[must_use]
pub fn map(root: &Path) -> MapResult {
    map_of(root, &NAZM)
}

/// The map of the repository under `root`, laid out as `layout` says.
#[must_use]
pub fn map_of(root: &Path, layout: &Layout) -> MapResult {
    on_parse_stack(|| map::build(root, layout)).unwrap_or_else(|p| map::unavailable(vec![p]))
}

/// The task context `request` asks of the repository under `root`.
#[must_use]
pub fn task(root: &Path, request: &TaskRequest) -> TaskResult {
    task_of(root, &NAZM, request)
}

/// The task context `request` asks of the repository under `root`, laid out as `layout`
/// says.
#[must_use]
pub fn task_of(root: &Path, layout: &Layout, request: &TaskRequest) -> TaskResult {
    on_parse_stack(|| task::plan(root, layout, request))
        .unwrap_or_else(|p| task::unavailable(vec![p]))
}

/// The repository state alone, or why the repository cannot be read.
///
/// # Errors
///
/// Every problem that makes the repository unreadable: a missing or malformed manifest, a
/// broken documentation corpus, a catalogue that does not parse.
pub fn state(root: &Path, layout: &Layout) -> Result<String, Vec<Problem>> {
    repository::Repository::load(root, layout).map(|r| r.state)
}

/// Run `f` on a thread with the parser's stack, as every command that analyses does.
fn on_parse_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, Problem> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .map_err(|e| Problem::new("analysis_failed", e.to_string()))?
            .join()
            .map_err(|_| Problem::new("analysis_failed", "the analysis stopped unexpectedly"))
    })
}

/// The one canonical serialisation of each answer.
macro_rules! json {
    ($t:ty) => {
        impl $t {
            /// Compact JSON, one line, fields in declaration order.
            #[must_use]
            pub fn to_json(&self) -> String {
                serde_json::to_string(self).unwrap_or_default()
            }
        }
    };
}
json!(MapResult);
json!(TaskResult);

#[cfg(test)]
mod tests {
    /// The structural half of "an index over existing authorities": nothing here keeps state
    /// between requests, iterates a hash map, stamps a time, a random value or a process id,
    /// runs a command, asks Git, reads the environment, or writes a file.
    #[test]
    fn the_planner_is_never_found_from_the_environment() {
        for (name, source) in [
            ("lib.rs", include_str!("lib.rs")),
            ("cargo.rs", include_str!("cargo.rs")),
            ("catalogue.rs", include_str!("catalogue.rs")),
            ("manifest.rs", include_str!("manifest.rs")),
            ("map.rs", include_str!("map.rs")),
            ("repository.rs", include_str!("repository.rs")),
            ("sources.rs", include_str!("sources.rs")),
            ("task.rs", include_str!("task.rs")),
        ] {
            let code: String = source
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with("//") && !t.starts_with("///")
                })
                .collect::<Vec<_>>()
                .join("\n");
            for forbidden in [
                "HashMap",
                "HashSet",
                "SystemTime",
                "Instant",
                "rand::",
                "process::id",
                "Command",
                "git",
                "env::var",
                "static mut",
                "OnceLock",
                "thread_local",
                concat!("fs::", "write"),
                concat!("File::", "create"),
                concat!("remove_", "file"),
                concat!("create_", "dir"),
                "f64",
                "f32",
            ] {
                assert!(
                    !code.contains(forbidden),
                    "`{forbidden}` in {name}: an answer would not be the repository's"
                );
            }
        }
    }
}
