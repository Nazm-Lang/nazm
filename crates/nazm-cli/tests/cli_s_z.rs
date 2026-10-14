//! The integration tests whose files begin with `s` to `z`, built as one executable.
//!
//! Each file beside this one is a module of one of the `cli_*` executables, by its first
//! letter, so the suite links five executables rather than one per file; `selfhost.rs`
//! stays its own, as only a contained run may start it. A new test file is declared in its
//! executable below — `every_test_file_belongs_to_its_executable` (in `cli_a_b.rs`) fails
//! until it is.

// The helpers below are shared by the modules, which each allow these for themselves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod differential;
mod memory_report;

mod scheduler;
mod scheduler_default;
mod scheduler_pool;
mod schemas;
mod sections;
mod secure_service;
mod select;
mod short_variants;
mod shorter_forms;
mod simd;
mod single_thread;
mod source_identity;
mod source_root;
mod speed_text;
mod stack_floor;
mod stack_size;
mod statics;
mod stdin_lines;
mod stdlib;
mod stdlib_1_0;
mod storage;
mod string_append;
mod string_literals;
mod summary;
mod tiers;
mod time;
mod toml;
mod trace;
mod traits;
mod tuples;
mod usability;
mod user_tests;
mod value_blocks;
mod visibility;
mod warnings;
mod wasi;
mod wasm;
mod wasm_contract;
mod wide;
mod windows;
mod workflow;
mod workspace_edit;
