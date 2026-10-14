//! The integration tests whose files begin with `m` to `r`, built as one executable.
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

mod main_ends;
mod manifests;
mod match_guards;
mod memory;
mod mir;
mod mobile;
mod named_methods;
mod native_cache;
mod native_units;
mod net;
mod numbers;
mod order_hash;
mod ownership_cycles;
mod packages;
mod patterns;
mod platform_files;
mod platform_matrix;
mod prelude;
mod printing;
mod process;
mod process_control;
mod profiles;
mod profiles_v2;
mod promotion;
mod propagation;
mod provenance;
mod realtime;
mod realtime_tasks;
mod reexports;
mod regex;
mod registry;
mod release_examples;
mod repository;
mod resolution;
mod resolved_units;
mod resource_regression;
mod run_compiled;
mod runtime;
mod runtime_artifact;
