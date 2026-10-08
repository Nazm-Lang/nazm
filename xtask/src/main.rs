//! Repository automation.
//!
//! `cargo xtask check` runs the architectural gates. They are security and design
//! controls, not style checks: clippy and rustfmt cover style, and this covers the claims
//! the project makes about its own structure that no linter can see.

mod bench;
mod campaign;
mod checks;
mod claims;
mod commands;
mod contained;
mod evidence;
mod gate;
mod governance;
mod guide;
mod metadata;
mod mutate;
mod plan;
mod procs;
mod rules;
mod suite;
mod supply;

use std::process::ExitCode;

use crate::checks::architecture::{
    check_core_ir_boundary, check_sema_purity, check_single_resolver, check_span_identity,
};
use crate::checks::cache::{check_cache_soundness, check_native_identity, check_object_cache};
use crate::checks::docs::{check_capability_matrix, check_doc_claims, check_docs_corpus};
use crate::checks::mutations::check_mutations_apply;
use crate::checks::reach::{
    check_codegen_reach, check_emission_reach, check_layering, check_unsafe_policy,
};
use crate::checks::runtime_parity::check_runtime_parity;
use crate::checks::{report, run_checks};
use crate::commands::contained::run_contained;
use crate::commands::mutate::{mutate_cli, mutate_exec, mutate_hold, mutate_recover, mutate_run};
use crate::commands::workspace_tests::workspace_tests;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => run_checks(),
        Some("check-layering") => report(&[("layering", check_layering())]),
        Some("check-codegen") => report(&[("codegen reach", check_codegen_reach())]),
        Some("check-emission") => report(&[("emission reach", check_emission_reach())]),
        Some("check-unsafe") => report(&[("unsafe policy", check_unsafe_policy())]),
        Some("check-docs") => report(&[("doc claims", check_doc_claims())]),
        // Listed in the help since N1 and never dispatched, so `cargo xtask check-matrix`
        // printed "unknown task" and exited **zero**. `cargo xtask check` ran them, which
        // is why nobody noticed; a gate that cannot be run on its own is a gate nobody
        // debugs.
        Some("check-matrix") => report(&[("capability matrix", check_capability_matrix())]),
        Some("check-resolver") => report(&[("one resolver", check_single_resolver())]),
        Some("check-core-ir") => report(&[("core ir boundary", check_core_ir_boundary())]),
        Some("check-span") => report(&[("span identity", check_span_identity())]),
        Some("check-sema") => report(&[("sema purity", check_sema_purity())]),
        Some("check-cache") => report(&[("cache soundness", check_cache_soundness())]),
        Some("check-symbols") => report(&[("native identity", check_native_identity())]),
        Some("check-objects") => report(&[("object cache", check_object_cache())]),
        Some("check-runtime") => report(&[("runtime parity", check_runtime_parity())]),
        Some("check-mutations") => report(&[("mutations apply", check_mutations_apply())]),
        Some("check-guide") => report(&[("guide from grammar", guide::check())]),
        Some("check-corpus") => report(&[("docs corpus", check_docs_corpus())]),
        Some("check-claims") => report(&[("performance claims", claims::check(&guide::root()))]),
        // N32-H3: the workspace suite, built by Cargo and scheduled longest first.
        Some("workspace-tests") => {
            let rest: Vec<String> = args.collect();
            workspace_tests(&rest)
        }
        // N32-H2: which MILESTONE evidence the named changed paths call for.
        Some("gate-plan") => {
            let paths: Vec<String> = args.collect();
            let p = gate::plan(&paths);
            for r in &p.reasons {
                println!("  {r}");
            }
            println!(
                "lifecycle {}",
                match p.lifecycle {
                    gate::Lifecycle::Full => "full",
                    gate::Lifecycle::Invariant => "invariant",
                }
            );
            println!("mcp shared mutations {}", if p.mcp { "yes" } else { "no" });
            println!(
                "benchmarks {}",
                if p.benches.is_empty() {
                    "none".to_owned()
                } else {
                    p.benches.iter().copied().collect::<Vec<_>>().join(" ")
                }
            );
            println!("lifecycle command: cargo {}", p.lifecycle_args().join(" "));
            println!(
                "stages {}",
                p.stages().into_iter().collect::<Vec<_>>().join(" ")
            );
            // A gate over these paths plus the harness bounds this one (N32-H3).
            let mut widened = paths.clone();
            widened.push("xtask/src/mutate.rs".to_owned());
            println!(
                "covered by the harness-changing plan over the same paths {}",
                if gate::plan(&widened).covers(&p) {
                    "yes"
                } else {
                    "no"
                }
            );
            ExitCode::SUCCESS
        }
        Some("bench") => {
            if let Some(why) = contained::refuse_uncontained("cargo xtask bench") {
                eprintln!("{why}");
                return ExitCode::FAILURE;
            }
            let rest: Vec<String> = args.collect();
            let mode = if rest.iter().any(|a| a == "--save") {
                bench::Mode::Save
            } else if rest.iter().any(|a| a == "--check") {
                bench::Mode::Check
            } else {
                bench::Mode::Record
            };
            let runs = rest
                .iter()
                .position(|a| a == "--runs")
                .and_then(|i| rest.get(i + 1))
                .and_then(|n| n.parse().ok())
                .unwrap_or(5);
            bench::run(mode, runs)
        }
        Some("guide") => match guide::write() {
            Ok(()) => {
                println!("docs/guide.md rewritten from docs/grammar.ebnf");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        Some("mutate") => {
            let rest: Vec<String> = args.collect();
            // Planning reads the catalogue and runs nothing; everything else is the heavy work
            // the contained runner exists for.
            if !rest.iter().any(|a| a == "--plan") {
                if let Some(why) = contained::refuse_uncontained("cargo xtask mutate") {
                    eprintln!("{why}");
                    return ExitCode::FAILURE;
                }
            }
            mutate_cli(&rest)
        }
        // Documented, because it is now the *only* way to recover an interrupted run:
        // `mutate` refuses rather than repairing, since it cannot establish that the
        // dead run's descendants have stopped.
        Some("mutate-recover") => mutate_recover(&mut args),
        // Hidden, and reachable only from the lifecycle tests. They exist because the
        // properties they check cannot be observed from inside one process: mutual
        // exclusion needs two independent processes to contend, and an interruption
        // needs a process that did not create the state left behind. Kept out of `help`
        // so they are not mistaken for an interface.
        Some("mutate-run") => mutate_run(&mut args),
        Some("mutate-hold") => mutate_hold(&mut args),
        Some("mutate-exec") => mutate_exec(&mut args),
        Some("contained") => {
            let rest: Vec<String> = args.collect();
            run_contained(&rest)
        }
        Some("evidence") => {
            let rest: Vec<String> = args.collect();
            evidence::run(&rest)
        }
        Some("help" | "--help" | "-h") | None => {
            help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown task: {other}\n");
            help();
            ExitCode::FAILURE
        }
    }
}

fn help() {
    println!(
        "cargo xtask <task>\n\n\
         Tasks:\n  \
           check           run every gate\n  \
           check-layering  crate dependency direction (rules.rs)\n  \
           check-codegen   no front-end crate *depends on* a code generator\n  \
           check-emission  only the backend crate emits IR or runs clang\n  \
           check-unsafe    the unsafe allowlist and the workspace lint agree\n  \
           check-docs      documented claims that must stay true\n  \
           check-matrix    every VERIFIED row in the capability matrix cites a real path\n  \
           check-resolver  the backend reads name resolution, it does not redo it\n  \
           check-span      a span cannot be serialised without its file\n  \
           check-sema      no session-local id can reach a persistent interface\n  \
           check-cache     a check key still covers everything checking reads\n  \
           check-symbols   a native symbol comes from identity, not from position\n  \
           check-guide     docs/guide.md is what docs/grammar.ebnf produces\n  \
           gate-plan PATHS which MILESTONE lifecycle evidence the changed paths call for;\n  \
                           an unknown path, or none, calls for all of it\n  \
           check-corpus    the documentation corpus sections: every document there, every\n  \
                           id unique, every link resolved, the grammar parsed\n  \
           guide           rewrite docs/guide.md from docs/grammar.ebnf\n  \
           evidence DIR    write DIR/sbom.json and DIR/index.json (every file's BLAKE3); --verify checks them\n  \
           workspace-tests every test binary of the workspace, scheduled; --host-safe leaves\n  \
                           out, by name, each that builds compiler stages (CI's subset)\n  \
           bench           measure; --save records a baseline, --check compares to one,\n  \
                           --runs N sets the repetitions (default 5)\n  \
           mutate          inject catalogued defects; each must be caught. Run it as\n  \
                           `cargo xtask contained mutate`, never directly on a checkout.\n  \
                           --only NAME        select by substring, repeatably\n  \
                           --strategy S       targeted (default: killer, profile, suite)\n  \
                                              or full (the suite for every mutant)\n  \
                           --plan             print what each mutant would run; runs nothing\n  \
                           --resume           reuse verdicts journaled by this exact campaign\n  \
                           --budget-secs N    stop starting mutants after N seconds\n  \
                           --verify-killer M  prove candidate killers for mutation M:\n  \
                                              pass pristine, fail mutated, pass restored\n  \
                           --killer P:T:NAME  a candidate, T = lib | test=X | bin=X\n  \
                           --verify-killers   also verify every decided mutant's catalogued\n  \
                                              killers in the same session, around the\n  \
                                              one injection its verdict is taken from\n  \
           mutate-recover  repair a tree an interrupted mutation run left mutated;\n  \
                           needs --confirm-no-writers, and read what it prints first\n  \
           help            this message\n"
    );
}
