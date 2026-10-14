//! The integration tests whose files begin with `c` to `d`, built as one executable.
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

mod c_foundation;
mod capabilities;
mod cell;
mod checked;
mod cli;
mod closures;
mod codes;
mod collections_r1e;
mod compat_v1;
mod compound_assign;
mod concurrent;
mod const_tables;
mod constraints;
mod contract;
mod contract_property;
mod contracts;
mod conveniences;
mod core_ir;
mod cortex_m;
mod cost_and_flow;
mod counts;
mod cranelift;
mod cross;
mod debugger;
mod debugger_v3;
mod debugging;
mod default_methods;
mod defer;
mod derive_json;
mod derive_toml;
mod distributed;
mod docs;
mod documentation;
mod domain_profiles;
mod domains;
mod durable_identity;
