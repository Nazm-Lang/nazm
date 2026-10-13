//! The integration tests whose files begin with `e` to `l`, built as one executable.
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

mod ecosystem;
mod effect_params;
mod effects;
mod embedded;
mod equality;
mod evm;
mod exit_contract;
mod exit_status;
mod features;
mod ffi;
mod ffi_v2;
mod ffi_v3;
mod file_walk;
mod files;
mod flow_v3;
mod for_loops;
mod freestanding;
mod fuzz_diff;
mod fuzz_replay;
mod generic_channels;
mod generic_closures;
mod generic_impls;
mod generics;
mod hash_vectors;
mod heap;
mod incremental;
mod interpolation;
mod interrupts;
mod kernel;
mod layout;
mod line_breaks;
mod lir;
mod lir_oracle;
mod live;
mod lsp;
