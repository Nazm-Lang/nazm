//! The agent task benchmark, N35: does the smaller N33 context let a real model solve the same
//! repository tasks at lower cost, without losing correctness?
//!
//! Each task of the [`suite`] is put to a model twice per trial: once with the naive context an
//! agent without the planner reads (the whole files the facts are in, as N33 and N34 defined
//! them), and once with `nazm repo --task`'s own output, byte for byte. Nothing else differs —
//! the system prompt, the task's wording, the answer contract, the model and its parameters are
//! one value for both arms ([`prompt`]). Each answer is scored fact by fact by a mechanical
//! [`oracle`] whose truths are read from the authorities, never from either context. Usage is
//! the provider's own report, kept raw and normalised ([`provider`]); money is a separate,
//! dated price list applied to it ([`price`]). The [`run`] refuses to exceed a spend cap,
//! retries a transport failure a bounded number of times and never an answer, and resumes a
//! journal without repeating an identical request. The [`report`] is deterministic and puts
//! correctness before cost.
//!
//! This crate is the only one that reaches a network, and it does so only in `run`, through
//! `curl`, with the credential passed on standard input. Every test drives it with a fake
//! provider or recorded responses; nothing that ships depends on it.

pub mod oracle;
pub mod price;
pub mod prompt;
pub mod provider;
pub mod report;
pub mod run;
pub mod suite;

// The seven N33 tasks and their baselines, from the one module the N33 and N34 benchmarks
// read: N35 cannot measure a baseline they did not. Its checks assert against the authorities,
// so a drifted fact stops `suite::build` rather than producing a quietly different suite.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    unreachable_pub
)]
#[path = "../../nazm-repo/tests/scenarios/mod.rs"]
mod scenarios;

/// Every record and report.
pub const SCHEMA: &str = "nazm.agent-benchmark/1";

/// The prompt template. A change to [`prompt::SYSTEM`], the layout or any task's wording is a
/// new version, and results under different versions are not compared.
pub const PROMPT_VERSION: &str = "nazm.agent-bench-prompt/1";

/// The task suite: its tasks, their wording, contracts and oracles.
pub const SUITE_VERSION: &str = "nazm.agent-bench-suite/1";

/// BLAKE3 of `bytes`, as lowercase hex.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
