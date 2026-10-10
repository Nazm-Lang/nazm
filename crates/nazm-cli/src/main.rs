//! `nazm` — run, check, or natively compile a Nazm program.
//!
//! Exit status is part of the interface, not an afterthought: `0` success, `1` an invalid
//! program or request, `2` a program that failed at run time, `3` a file or resource that
//! could not be found or read, `4` the compiler, its tools or the machine failed, `5` a wrong
//! command line (the v1 contract, `exit.rs`). A caller that has to parse stderr to find out
//! which happened is doing work the process could have told it.
//!
//! # The contract a compiled program keeps
//!
//! Decided before the wrapper was written, because these are the parts a user depends on
//! and they are awkward to change afterwards:
//!
//! | | |
//! | --- | --- |
//! | **Result** | The full signed 64-bit value, in decimal, on standard output, followed by a newline. **Not** the exit status, which is 8 bits and would silently truncate almost every `Int` |
//! | **Success** | Exit `0` |
//! | **Runtime failure** | Exit `2` — the same status `nazm run` uses — with one line on standard error: `nazm: error[CODE]: message at file:line:column` |
//! | **Diagnostic category** | The same stable code the interpreter reports: `N0400` for overflow, `N0401` for division by zero |
//! | **Source location** | Line and column of the operator that failed, resolved at compile time and baked into the binary as a constant |
//! | **`--json`** | Applies to **build-time** diagnostics only. A generated executable has no diagnostic framework linked into it and emits the plain line above; promising JSON there would mean shipping a serialiser in every binary |
//! | **Resource limits** | **None.** A compiled program has no iteration budget and no call-depth budget. `--max-iterations` and `--max-depth` belong to `nazm run` and are not read here |
//!
//! That last row is a real difference between the two implementations and is stated
//! rather than left to be discovered. `nazm run` stops a runaway loop with `N0402`; a
//! compiled one runs until something kills it. `docs/spec.md` already places both limits
//! in the interpreter rather than in the language, so the backend is not obliged to
//! reproduce them — it is obliged not to imply it has. Adopting a native budget is a
//! decision with its own diagnostic and its own increment, not a flag added in passing.
//!
//! `nazm run` and `nazm check` are unchanged by any of this.

use std::process::ExitCode;

mod accel;
mod apidoc;
mod attest;
mod backend;
mod bench;
mod bindgen;
mod board_manifest;
mod build;
mod capabilities;
mod checking;
mod cli;
mod context;
mod contract;
mod contract_wasm;
mod core_ir;
mod derived;
mod diagnostics;
mod docs;
mod exit;
mod explain;
mod fix;
mod fmt;
mod harness;
mod init;
mod inspect;
mod interface;
mod interpret;
mod lir;
mod live;
mod lsp;
mod mir;
mod obligations;
mod opened;
mod package;
mod patch;
mod prepare;
mod profile;
mod provenance;
mod repo;
mod resolved;
mod run_profile;
mod runtime_artifact;
mod scratch;
mod snapshot;
mod stack;
mod summary;
// Moved to `nazm-service` in N16, so `nazm lsp` loads exactly as every command does.
use nazm_service::{load, root};
mod test;

use clap::Parser;
use nazm_span::SourceMap;

use cli::{AttestOperation, Cli, Command, PatchOperation, RuntimeAction};
use interpret::run;
use prepare::prepare;

/// Every command on the stack `parse_source` asks its caller for, rather than the main
/// thread's: a check of the deepest accepted nesting needs more than the 8 MiB a main thread
/// gets, and a bound that depends on which command reached it is not a bound. A thread that
/// cannot start, or a panic — a compiler defect — is status 4 (Q1-C1; it was Rust's 101,
/// outside the contract).
fn main() -> ExitCode {
    std::thread::Builder::new()
        .name("nazm".to_owned())
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(command)
        .map_or(ExitCode::from(exit::TOOLCHAIN_FAILED), |t| {
            t.join().unwrap_or(ExitCode::from(exit::TOOLCHAIN_FAILED))
        })
}

/// The command line, parsed. Help and the version are answers (0); anything else clap rejects is
/// a usage error (5) — clap's own status is 2, the contract's runtime failure (Q1-C1).
fn parse_cli() -> Result<Cli, ExitCode> {
    Cli::try_parse().map_err(|e| {
        let _ = e.print();
        if e.use_stderr() {
            ExitCode::from(exit::USAGE)
        } else {
            ExitCode::from(exit::OK)
        }
    })
}

fn command() -> ExitCode {
    let mut cli = match parse_cli() {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(done) = prepare(&mut cli.command) {
        return done;
    }
    dispatch(cli.command)
}

/// Each command, to the module that carries it out.
fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::Lock { dir } => package::lock(&dir),
        Command::Attest { operation } => match operation {
            AttestOperation::Sign {
                key,
                out,
                provenance,
                artefacts,
            } => attest::sign(&key, out.as_deref(), &provenance, &artefacts),
            AttestOperation::Verify {
                allowed_signers,
                identity,
                statement,
                artefacts,
            } => attest::verify(&allowed_signers, &identity, &statement, &artefacts),
        },
        Command::Update { dir, names } => package::update(&dir, &names),
        Command::Publish {
            dir,
            registry,
            dry_run,
        } => {
            if dry_run {
                return package::publish_dry_run(&dir, &registry);
            }
            package::publish(&dir, &registry)
        }
        Command::Yank {
            package: which,
            registry,
            undo,
        } => package::yank(&which, &registry, undo),
        Command::Inspect { file, def } => inspect::run(&file, def.as_deref()),
        Command::Profile {
            file,
            backend,
            sample,
            interval_ms,
        } => run_profile::run(&file, &backend, sample.then_some(interval_ms)),
        Command::ExplainCost { file, json } => explain::cost(&file, json),
        Command::ExplainFlow { file, json } => explain::flow(&file, json),
        Command::Build {
            file,
            out,
            json,
            emit_ir,
            opt_level,
            no_cache,
            build_report,
            summary_json,
            backend,
            target,
            link,
            lib,
            shared,
            objects,
            stack_watermark,
            board,
            layout,
            timings,
            debug,
            runtime,
            selection,
            locked: _,
            profile,
            provenance,
            attest_with,
            sbom,
        } => {
            let harness = match harness::parsed(selection.harness.as_deref()) {
                Ok(h) => h,
                Err(code) => return code,
            };
            let features = selection.features;
            // A board's manifest (Gate 3): read before anything is compiled, for a board's target.
            let board = match board.as_deref() {
                None => None,
                Some(path) => match build::read_board(path, target.as_deref()) {
                    Ok(b) => Some(b),
                    Err(code) => return code,
                },
            };
            let built = build::run(
                &file,
                out.as_deref(),
                json,
                emit_ir,
                opt_level,
                !no_cache,
                build_report,
                summary_json,
                build::Choice {
                    backend: nazm_lir::backend::BackendId::by_name(&backend)
                        .unwrap_or(nazm_lir::backend::BackendId::Llvm),
                    target: target.as_deref(),
                    link: &link,
                    library: lib,
                    shared,
                    objects: objects.as_deref(),
                    stack_watermark,
                    layout: &layout,
                    adapter: None,
                    timings,
                    debug,
                    runtime: runtime.as_deref(),
                    profile: profile.as_deref(),
                    harness: harness.as_ref(),
                    features: &features,
                    board: board.as_ref(),
                },
            );
            if (provenance.is_none() && sbom.is_none()) || built != ExitCode::from(exit::OK) {
                return built;
            }
            provenance::write(
                &provenance::Request {
                    source: &file,
                    out: out.as_deref(),
                    opt_level,
                    backend: &backend,
                    target: target.as_deref(),
                    layout: &layout,
                    debug,
                    library: lib,
                    objects: objects.as_deref(),
                    stack_watermark,
                    link: &link,
                    profile: profile.as_deref(),
                    runtime: runtime.as_deref(),
                },
                provenance.as_deref(),
                sbom.as_deref(),
                attest_with.as_deref(),
            )
        }
        Command::Check {
            file,
            json,
            source_root,
            no_cache,
            cache_report,
            summary_json,
            locked: _,
            profile,
            features,
            profile_report: _,
        } => checking::run(
            &file,
            json,
            source_root.as_deref(),
            !no_cache,
            cache_report,
            summary_json,
            profile.as_deref(),
            &features,
        ),
        Command::Fix { file, apply, json } => fix::run(&file, apply, json),
        Command::Interface {
            file,
            hash,
            source_root,
        } => interface::run(&file, hash, source_root.as_deref()),
        Command::Resolve { file, source_root } => resolved::resolve(&file, source_root.as_deref()),
        Command::References {
            def,
            file,
            source_root,
        } => resolved::references(&def, &file, source_root.as_deref()),
        Command::Repl => live::repl(),
        Command::Init { dir, template } => init::run(&dir, &template),
        Command::Doc { path, out } => apidoc::run(&path, out.as_deref()),
        Command::Bindgen { header, out } => bindgen::run(&header, out.as_deref()),
        Command::Contract {
            file,
            interface,
            txs,
            evm,
            upgrade_check,
            wasm,
            accounts,
        } => {
            if let Some(old) = upgrade_check {
                return contract::upgrade_check(&file, &old);
            }
            contract::run(
                &file,
                interface,
                txs.as_deref(),
                evm.as_deref(),
                wasm.as_deref(),
                accounts.as_deref(),
            )
        }
        Command::Accel {
            file,
            kernel,
            input,
            reduce,
            no_check,
            print_kernel,
        } => accel::run(
            &file,
            &kernel,
            &input,
            reduce.as_deref().and_then(accel::Reduce::by_name),
            !no_check,
            print_kernel,
        ),
        Command::Comptime {
            file,
            function,
            max_iterations,
            max_depth,
        } => live::comptime(
            &file,
            &function,
            nazm_core::eval::Limits {
                max_iterations,
                max_depth,
            },
        ),
        Command::ReloadCheck { old, new } => live::reload_check(&old, &new),
        Command::Capabilities { json } => capabilities::run(json),
        Command::CoreIr {
            file,
            json,
            no_spans,
            source_root,
        } => core_ir::run(&file, json, !no_spans, source_root.as_deref()),
        Command::Mir {
            file,
            json,
            spans,
            source_root,
        } => mir::run(&file, json, spans, source_root.as_deref()),
        Command::Obligations {
            file,
            json,
            source_root,
        } => obligations::run(&file, json, source_root.as_deref()),
        Command::Lir {
            file,
            target,
            source_root,
            ops,
            soa,
            run,
            args,
        } => {
            if run {
                return lir::oracle(&file, source_root.as_deref(), &args);
            }
            lir::run(&file, target.as_deref(), source_root.as_deref(), ops, soa)
        }
        Command::Runtime { action } => match action {
            RuntimeAction::Build {
                target,
                profile,
                opt_level,
                out,
            } => runtime_artifact::build(target.as_deref(), profile.as_deref(), opt_level, &out),
            RuntimeAction::Verify { dir, target } => {
                runtime_artifact::verify(&dir, target.as_deref())
            }
        },
        Command::Lsp { source_root } => lsp::run(source_root.as_deref()),
        Command::Context {
            root,
            file,
            byte,
            json,
            source_root,
        } => context::run(&root, &file, byte, json, source_root.as_deref()),
        Command::Snapshot {
            root,
            json,
            source_root,
        } => snapshot::snapshot(&root, json, source_root.as_deref()),
        Command::Delta {
            root,
            baseline,
            json,
            source_root,
        } => snapshot::delta(&root, &baseline, json, source_root.as_deref()),
        Command::Diagnostics {
            root,
            detail,
            state,
            json,
            source_root,
        } => diagnostics::run(
            &root,
            detail.zip(state).as_ref(),
            json,
            source_root.as_deref(),
        ),
        Command::Patch { operation } => match operation {
            PatchOperation::Rename {
                root,
                file,
                byte,
                new_name,
                json,
                source_root,
            } => patch::run(
                &root,
                &file,
                byte,
                &patch::Ask::Rename(new_name),
                json,
                source_root.as_deref(),
            ),
            PatchOperation::Fix {
                root,
                file,
                byte,
                fix,
                json,
                source_root,
            } => patch::run(
                &root,
                &file,
                byte,
                &patch::Ask::Fix(fix),
                json,
                source_root.as_deref(),
            ),
            PatchOperation::Apply { plan, base } => {
                patch::apply(&plan, base.as_deref().unwrap_or(std::path::Path::new(".")))
            }
        },
        Command::Fmt { files, check } => fmt::run(&files, check),
        Command::Docs {
            index,
            document,
            section,
            state,
            json,
            root,
        } => {
            let root = root.unwrap_or_else(nazm_docs::default_root);
            match (index, section) {
                (true, _) => docs::index(&root, document.as_deref(), json),
                (false, Some(id)) => docs::section(&root, &id, state.as_deref(), json),
                (false, None) => unreachable!("clap requires one"),
            }
        }
        Command::Repo {
            map,
            task: _,
            intent,
            seed,
            state,
            max_entities,
            max_source_bytes,
            max_doc_bytes,
            json,
            root,
        } => {
            let root = root.unwrap_or_else(nazm_docs::default_root);
            if map {
                return repo::map(&root, json);
            }
            // Clap has already refused any other word.
            let intent = intent.as_deref().and_then(nazm_repo::Intent::parse);
            let request = nazm_repo::TaskRequest {
                intent,
                seeds: seed,
                state,
                budget: nazm_repo::Budget {
                    max_entities,
                    max_source_bytes,
                    max_doc_bytes,
                },
            };
            repo::task(&root, &request, json)
        }
        Command::Test {
            paths,
            json,
            interpret_only,
            summary_json,
            filter,
            fuzz,
        } => test::run(
            &paths,
            json,
            interpret_only,
            summary_json,
            filter.as_deref(),
            fuzz,
        ),
        Command::Bench {
            paths,
            iterations,
            json,
        } => bench::run(&paths, iterations, json),
        Command::Run {
            file,
            args,
            json,
            max_iterations,
            max_depth,
            locked: _,
            profile,
            harness,
            features,
        } => {
            let harness = match harness::parsed(harness.as_deref()) {
                Ok(h) => h,
                Err(code) => return code,
            };
            run(
                &file,
                json,
                nazm_core::eval::Limits {
                    max_iterations,
                    max_depth,
                },
                args,
                profile.as_deref(),
                harness.as_ref(),
                &features,
            )
        }
    }
}

pub(crate) fn report_ok(text: &str, _extra: Option<&str>) -> ExitCode {
    println!("{text}");
    ExitCode::from(exit::OK)
}

pub(crate) fn emit(diagnostics: &[nazm_diag::Diagnostic], map: &SourceMap, json: bool) {
    for d in diagnostics {
        if json {
            match d.to_json(map) {
                Ok(line) => eprintln!("{line}"),
                // A diagnostic that cannot be serialised must still be visible; falling
                // back to the rendering beats losing it.
                Err(_) => eprint!("{}", d.render(map)),
            }
        } else {
            eprint!("{}", d.render(map));
        }
    }
}
