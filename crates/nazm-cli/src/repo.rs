//! `nazm repo` — the repository context map, `nazm.repository-map/1`, or the minimum
//! context for a task, `nazm.task-context/1`. N33.
//!
//! Every answer is read from the repository as it is on disk at this call and dropped. With
//! `--json`, standard output is exactly one object, whatever the answer, and standard error
//! is empty; the exit status is 0 with a map or a context — complete or partial — and 1 with
//! a refusal. Without it, the same object indented. `docs/internal/architecture.md` §7.35.

use std::path::Path;
use std::process::ExitCode;

use nazm_repo::{MapResult, TaskRequest, TaskResult};

pub(crate) fn map(root: &Path, json: bool) -> ExitCode {
    let r = nazm_repo::map(root);
    print(&r, json, &r.to_json());
    exit(matches!(r, MapResult::Available(_)))
}

pub(crate) fn task(root: &Path, request: &TaskRequest, json: bool) -> ExitCode {
    let r = nazm_repo::task(root, request);
    print(&r, json, &r.to_json());
    exit(matches!(r, TaskResult::Available(_)))
}

fn print(value: &impl serde::Serialize, json: bool, compact: &str) {
    let text = if json {
        compact.to_owned()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| compact.to_owned())
    };
    println!("{text}");
}

fn exit(answered: bool) -> ExitCode {
    if answered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
