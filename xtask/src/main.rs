//! Repository automation.
//!
//! `cargo xtask check` runs the architectural gates. They are security and design
//! controls, not style checks: clippy and rustfmt cover style, and this covers the claims
//! the project makes about its own structure that no linter can see.

mod bench;
mod campaign;
mod contained;
mod gate;
mod guide;
mod metadata;
mod mutate;
mod plan;
mod procs;
mod rules;
mod suite;

use std::process::{Command, ExitCode};

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

fn run_checks() -> ExitCode {
    report(&[
        ("layering", check_layering()),
        ("tokenizer reach", check_tokenizer_reach()),
        ("network reach", check_network_reach()),
        ("codegen reach", check_codegen_reach()),
        ("emission reach", check_emission_reach()),
        ("unsafe policy", check_unsafe_policy()),
        ("doc claims", check_doc_claims()),
        ("capability matrix", check_capability_matrix()),
        ("one resolver", check_single_resolver()),
        ("core ir boundary", check_core_ir_boundary()),
        ("span identity", check_span_identity()),
        ("sema purity", check_sema_purity()),
        ("cache soundness", check_cache_soundness()),
        ("native identity", check_native_identity()),
        ("object cache", check_object_cache()),
        ("runtime parity", check_runtime_parity()),
        ("mutations apply", check_mutations_apply()),
        ("guide from grammar", guide::check()),
        ("docs corpus", check_docs_corpus()),
    ])
}

/// The documentation corpus `nazm docs` and `nazm-mcp` answer from sections cleanly: every
/// allowlisted document exists and is UTF-8, every section id is unique — so no anchor, and
/// no rule id an anchor carries, is defined twice — every in-corpus link and every generated
/// guide rule resolves, and the grammar parses into productions whose references are all
/// defined. Read with `nazm-docs` itself, so the gate and the tools cannot disagree about
/// what the corpus is. N32.
fn check_docs_corpus() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    nazm_docs::Corpus::load(&root, nazm_docs::CORPUS)
        .map(|_| ())
        .map_err(|problems| {
            problems
                .into_iter()
                .map(|p| {
                    format!(
                        "{}{}: {}{}",
                        p.document,
                        p.line.map(|l| format!(":{l}")).unwrap_or_default(),
                        p.kind,
                        p.value.map(|v| format!(" `{v}`")).unwrap_or_default()
                    )
                })
                .collect()
        })
}

fn report(results: &[(&str, Result<(), Vec<String>>)]) -> ExitCode {
    let mut failed = false;
    for (name, result) in results {
        match result {
            Ok(()) => println!("ok    {name}"),
            Err(problems) => {
                failed = true;
                println!("FAIL  {name}");
                for p in problems {
                    println!("        {p}");
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Every workspace member must be placed in the layering, and may depend only on what
/// `rules.rs` allows.
fn check_layering() -> Result<(), Vec<String>> {
    let meta = match metadata::workspace() {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("could not read cargo metadata: {e}")]),
    };

    let mut problems = Vec::new();
    for member in &meta.members {
        let Some(allowed) = rules::allowed_internal_deps(&member.name) else {
            problems.push(format!(
                "{} is not placed in the layering. Add it to xtask/src/rules.rs, deciding \
                 deliberately what it may depend on.",
                member.name
            ));
            continue;
        };
        for dep in &member.internal_deps {
            if !allowed.contains(&dep.as_str()) {
                problems.push(format!(
                    "{} depends on {dep}, which the layering does not allow",
                    member.name
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Only `nazm-tokens` depends on a tokenizer library; only the evaluators depend on
/// `nazm-tokens` (N34, N35); and nothing depends on an evaluator. The last two are also the
/// layering's — no other crate lists them — and are checked here by name so the reason is in
/// the message.
fn check_tokenizer_reach() -> Result<(), Vec<String>> {
    let meta = match metadata::workspace() {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("could not read cargo metadata: {e}")]),
    };
    let mut problems = Vec::new();
    for member in &meta.members {
        if member.name == rules::TOKENIZER_OWNER {
            continue;
        }
        for dep in &member.all_deps {
            if rules::TOKENIZER_CRATES.contains(&dep.as_str()) {
                problems.push(format!(
                    "{} depends on {dep}: a tokenizer may only be reached through {}, which \
                     nothing that ships depends on",
                    member.name,
                    rules::TOKENIZER_OWNER
                ));
            }
            if rules::TOKEN_EVALUATORS.contains(&dep.as_str()) {
                problems.push(format!(
                    "{} depends on {dep}: an evaluator reaches a model provider, and nothing \
                     may depend on it",
                    member.name
                ));
            }
            if dep == rules::TOKENIZER_OWNER
                && !rules::TOKEN_EVALUATORS.contains(&member.name.as_str())
            {
                problems.push(format!(
                    "{} depends on {}: token measurement must not reach the compiler, the \
                     service, the planner or a server",
                    member.name,
                    rules::TOKENIZER_OWNER
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Claims made in the docs that a reader would reasonably treat as current fact, and that
/// would quietly become false.
///
/// The README says the decision rule is not yet pre-registered. The moment it *is*, that
/// sentence has to change, and the failure mode is that nobody remembers. So: if the
/// pre-registration file exists, the README must no longer claim otherwise.
/// Only the crates that are supposed to may emit a backend artefact or run the toolchain.
///
/// # Why a source scan rather than a dependency scan
///
/// Text emission has no dependency to find. What it does have is a call site, and the
/// property worth protecting is exactly about call sites: **a check-only path must not
/// reach native emission or `clang`.** So this looks for the two capabilities by name in
/// the source of every crate and requires them to appear only where `rules.rs` allows.
///
/// Stated precisely, because it is easy to overclaim: this is a **source-level check on
/// where the capability is mentioned**, not a call-graph proof. It would not catch
/// emission reached through a trait object or a re-export under another name. What it
/// does catch is the realistic regression — someone calling `emit_llvm` or spawning
/// `clang` from the checker because it was convenient — and it fails loudly when a new
/// crate acquires either capability without being placed.
///
/// The behavioural half of the same claim is a test: `nazm check` run with an empty
/// `PATH` must still succeed, which no amount of source scanning can establish.
fn check_emission_reach() -> Result<(), Vec<String>> {
    // The capability, and the text that betrays it.
    const CAPABILITIES: &[(&str, &str)] = &[
        ("emits LLVM IR", "emit_llvm"),
        ("invokes the native toolchain", "\"clang\""),
    ];

    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let mut problems = Vec::new();
    for (crate_dir, allowed) in rules::emission_allowlist() {
        let src = root.join(crate_dir).join("src");
        let Ok(files) = collect_rs(&src) else {
            continue;
        };
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (what, needle) in CAPABILITIES {
                if text.contains(needle) && !allowed.contains(what) {
                    problems.push(format!(
                        "{} mentions `{needle}`, so it {what} — but rules.rs does not \
                         allow it to. A check-only path that can reach native emission or \
                         the toolchain is the separation this rule exists to keep.",
                        file.strip_prefix(root).unwrap_or(&file).display()
                    ));
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Only the agent benchmark reaches a network (N35): no other crate's source mentions the
/// transport it uses or a model provider's API. A source-level check, like emission reach: it
/// catches the realistic regression — a request added somewhere convenient — not a call graph.
fn check_network_reach() -> Result<(), Vec<String>> {
    const MARKS: &[&str] = &[
        "\"curl\"",
        "\"claude\"",
        "api.openai.com",
        "api.anthropic.com",
        "TcpStream",
    ];
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        return Err(vec!["could not read crates/".to_owned()]);
    };
    let mut problems = Vec::new();
    for c in crates.filter_map(Result::ok) {
        if rules::TOKEN_EVALUATORS
            .iter()
            .any(|e| c.file_name().to_str() == Some(e))
        {
            continue;
        }
        let Ok(files) = collect_rs(&c.path().join("src")) else {
            continue;
        };
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for mark in MARKS {
                if text.contains(mark) {
                    problems.push(format!(
                        "{} mentions `{mark}`: only an evaluator may reach a network",
                        file.strip_prefix(root).unwrap_or(&file).display()
                    ));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every `.rs` under a directory. Tests live outside `src/`, so assertions about IR
/// content in `tests/` are not mistaken for the capability itself.
fn collect_rs(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Documented claims that must stay true — **about the benchmark harness only**.
///
/// Named "doc claims", which over-promises: both checks below concern whether the
/// evaluation protocol's pre-registration status is stated consistently, and both are
/// no-ops while `docs/research/evaluation-preregistration.md` does not exist. A reader seeing
/// `ok doc claims` should not take it for more than that.
///
/// The language's documented claims are checked by `crates/nazm-cli/tests/docs.rs`,
/// which can run the binary and read the emitter's own constants — neither of which
/// `xtask` may do, since it depends on no compiler crate (its one internal dependency is
/// `nazm-docs`, for the corpus gate). It is a test rather than a gate for that reason, and
/// `cargo test --workspace` runs it.
/// The workspace root: this crate's manifest directory, one level up.
fn workspace_root() -> Option<std::path::PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf)
}

fn check_doc_claims() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let mut problems = Vec::new();
    let prereg = root.join("docs/research/evaluation-preregistration.md");
    let readme = match std::fs::read_to_string(root.join("README.md")) {
        Ok(s) => s,
        Err(e) => return Err(vec![format!("could not read README.md: {e}")]),
    };

    let readme_says_not_registered = readme.contains("decision rule is not pre-registered")
        || readme.contains("not binding before then");

    if prereg.exists() && readme_says_not_registered {
        problems.push(
            "docs/research/evaluation-preregistration.md exists, but README.md still says the \
             decision rule is not pre-registered. One of them is wrong."
                .to_owned(),
        );
    }

    // The reverse: the evaluation protocol must keep its own status honest.
    match std::fs::read_to_string(root.join("docs/research/evaluation.md")) {
        Ok(eval) => {
            if prereg.exists() && eval.contains("Not yet pre-registered") {
                problems.push(
                    "docs/research/evaluation.md still carries its draft banner although a \
                     pre-registration exists."
                        .to_owned(),
                );
            }
        }
        Err(e) => problems.push(format!("could not read docs/research/evaluation.md: {e}")),
    }

    // The claim that rotted in four places at once. `crates/nazm-lir` is an IR crate and
    // `nazm build` produces a native executable; a document saying otherwise is describing
    // the repository as it stood before 2026-09-20. Matched as a literal phrase because
    // that is what a reader sees — the intent is to make the sentence expensive to leave
    // lying around, not to parse English.
    if root.join("crates/nazm-lir").exists() {
        for doc in [
            "README.md",
            "Cargo.toml",
            "docs/architecture.md",
            "docs/roadmap.md",
            "docs/master-architecture.md",
            "docs/capability-matrix.md",
        ] {
            let Ok(text) = std::fs::read_to_string(root.join(doc)) else {
                problems.push(format!("could not read {doc}"));
                continue;
            };
            for n in absence_claims(&text) {
                problems.push(format!(
                    "{doc}:{n} says the workspace has no IR or no backend, but \
                     crates/nazm-lir exists and `nazm build` emits a native \
                     executable. One of them is wrong."
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The capability matrix must keep citing things that exist.
///
/// # Why this gate exists
///
/// Every status document in this repository rotted the same way: a capability landed, the
/// milestone record said so, and a table four hundred lines away went on saying "planned |
/// nothing exists". Prose cannot be trusted to track the tree, so the two claims that can
/// be checked mechanically are checked here.
///
/// # What it establishes, and what it does not
///
/// It reads files. It establishes that a cited path resolves and that a VERIFIED section
/// cites one — **not** that the evidence supports the claim. A row can point at a real
/// file and still be wrong about it. This catches the failure that actually happened:
/// evidence naming a file that was renamed or never written, as
/// `crates/nazm-cli/src/capabilities.rs` did for a test file that does not exist.
/// Directories a cited path may be rooted at.
///
/// Anchoring on these is what keeps ordinary prose in backticks — a type name, a command,
/// a diagnostic code — from being mistaken for a path.
const DOC_PATH_ROOTS: [&str; 12] = [
    "crates/",
    "compiler/",
    "docs/",
    "schema/",
    "xtask/",
    "scripts/",
    "bench/",
    "examples/",
    "docker/",
    "corpus/",
    ".github/",
    "review/",
];

/// The repository paths one line of a document cites, in backticks.
///
/// A trailing `:12` or `:12-20` is a line reference and is not part of the path.
fn cited_paths(line: &str) -> Vec<&str> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .filter(|piece| DOC_PATH_ROOTS.iter().any(|r| piece.starts_with(r)))
        .filter_map(|piece| piece.split(':').next())
        .filter(|path| !path.is_empty())
        .collect()
}

/// Headings marked VERIFIED whose section cites no repository path.
///
/// A section runs from its `###` heading to the next one.
fn verified_without_evidence(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut open: Option<(&str, bool)> = None;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            if let Some((h, false)) = open {
                problems.push(h.to_owned());
            }
            open = Some((heading.trim(), false));
        } else if let Some((_, has_path)) = open.as_mut() {
            if !cited_paths(line).is_empty() {
                *has_path = true;
            }
        }
    }
    if let Some((h, false)) = open {
        problems.push(h.to_owned());
    }
    problems
        .into_iter()
        .filter(|h| h.contains("VERIFIED"))
        .collect()
}

/// One-based line numbers **asserting** the workspace has no IR or no backend.
///
/// Two things keep this from firing on honest prose. The negation has to sit next to the
/// noun, or every sentence explaining that there is no *second* backend would trip. And a
/// phrase inside double quotes is a **quotation, not an assertion** — this repository
/// corrects a claim by quoting it and saying what replaced it, so a gate that could not
/// tell the two apart would punish exactly the practice it exists to encourage.
///
/// Quote state is tracked per paragraph rather than per line, because a correction block
/// quotes across a line break, and per file would let one unbalanced quotation mark
/// silence the rest of the document.
fn absence_claims(text: &str) -> Vec<usize> {
    // Matched over the paragraph rather than the line: the phrase wraps, and `Cargo.toml`
    // wrapped it exactly between "no IR and no" and "backend".
    let mut found = Vec::new();
    let mut quoted = false;
    // (text outside quotes, the line each character came from)
    let mut para = String::new();
    let mut origin: Vec<usize> = Vec::new();

    // `para` is accumulated already lower-cased and `origin` carries one entry per **byte**
    // of it, so a byte offset from `find` indexes straight into `origin`. Counting
    // characters here instead would drift on the first em dash.
    let flush = |para: &mut String, origin: &mut Vec<usize>, found: &mut Vec<usize>| {
        for needle in [
            "no ir and no backend",
            "no ir, no backend",
            "there is no compiler yet",
        ] {
            let mut from = 0;
            while let Some(at) = para.get(from..).and_then(|rest| rest.find(needle)) {
                let start = from + at;
                if let Some(line) = origin.get(start) {
                    found.push(*line);
                }
                from = start + needle.len();
            }
        }
        para.clear();
        origin.clear();
    };

    for (n, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() {
            flush(&mut para, &mut origin, &mut found);
            quoted = false;
            continue;
        }
        // A comment marker is punctuation, not content: `Cargo.toml` carries this prose in
        // `#` comments and a sentence wrapped across two of them is still one sentence.
        let line = raw.trim_start().trim_start_matches('#');
        for (i, run) in line.split('"').enumerate() {
            // Run 0 continues whatever state the line began in; each `"` flips it.
            let inside_quotes = quoted != (i % 2 == 1);
            if inside_quotes {
                continue; // a quotation, not an assertion
            }
            // Whitespace collapses to one space as it goes in, so a phrase that wrapped —
            // and picked up a comment marker and an indent on the way — still reads as the
            // one sentence it is.
            for c in run.chars() {
                if c.is_whitespace() {
                    if !para.ends_with(' ') {
                        origin.push(n + 1);
                        para.push(' ');
                    }
                    continue;
                }
                for lowered in c.to_lowercase() {
                    for _ in 0..lowered.len_utf8() {
                        origin.push(n + 1);
                    }
                    para.push(lowered);
                }
            }
        }
        if !para.ends_with(' ') {
            origin.push(n + 1);
            para.push(' ');
        }
        quoted = quoted != (line.matches('"').count() % 2 == 1);
    }
    flush(&mut para, &mut origin, &mut found);

    found.sort_unstable();
    found.dedup();
    found
}

/// The capability matrix must keep citing things that exist.
///
/// # Why this gate exists
///
/// Every status document here rotted the same way: a capability landed, the milestone
/// record said so, and a table four hundred lines away went on saying "planned | nothing
/// exists". Prose cannot be trusted to track the tree, so the two claims that *can* be
/// checked mechanically are checked.
///
/// # What it establishes, and what it does not
///
/// It reads files. It establishes that a cited path resolves and that a VERIFIED section
/// cites one — **not** that the evidence supports the claim. A row can point at a real
/// file and still be wrong about it. What this catches is the failure that actually
/// happened: evidence naming a file that was renamed or never written, as
/// `crates/nazm-cli/src/capabilities.rs` did for a test file that does not exist.
fn check_capability_matrix() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let text = match std::fs::read_to_string(root.join("docs/capability-matrix.md")) {
        Ok(s) => s,
        Err(e) => {
            return Err(vec![format!(
                "could not read docs/capability-matrix.md: {e}"
            )])
        }
    };

    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for path in cited_paths(line) {
            if !root.join(path).exists() {
                problems.push(format!(
                    "docs/capability-matrix.md:{} cites `{path}`, which does not exist",
                    n + 1
                ));
            }
        }
    }
    for heading in verified_without_evidence(&text) {
        problems.push(format!(
            "\"{heading}\" is VERIFIED but cites no repository path"
        ));
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The backend reads name resolution; it does not perform it.
///
/// # What this guards, and why a test could not
///
/// Until N1, `nazm-lir` resolved names itself — its own scope stack, its own slot
/// allocator, its own name-keyed signature table — because `rules.rs` forbids it
/// depending on `nazm-core` and the checker's answer was discarded before anyone could
/// hand it over. Two implementations of one rule, kept in agreement by tests.
///
/// The answer is now passed to `lower` as a `&Resolution`. But passing it does not force
/// reading it: someone could add a `HashMap<String, Sig>` back and rebuild the same tables
/// under another name, and every existing test would still pass, because both resolvers
/// would agree *on the day they were written*. That is exactly how the first duplication
/// survived as long as it did.
///
/// So the property checked is structural and blunt: **the backend declares no symbol table
/// keyed by a name.** Resolution is the only thing that turns a name into an identity, and
/// it does not live here. String interning in the emitter uses `Vec`, so it is unaffected.
///
/// This is a textual check over one crate's sources, and it establishes exactly that: no
/// declaration of that shape. It is not proof that no second resolver could ever be
/// written — it is a tripwire across the path the last one took.
fn check_single_resolver() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let dir = root.join("crates/nazm-lir/src");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => return Err(vec![format!("could not read {}: {e}", dir.display())]),
    };

    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            problems.push(format!("could not read {}", path.display()));
            continue;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        for (n, line) in text.lines().enumerate() {
            let flat = line.replace(' ', "");
            // A map or set keyed by a source name is the shape a symbol table takes.
            if flat.contains("HashMap<String,") || flat.contains("HashSet<String>") {
                problems.push(format!(
                    "crates/nazm-lir/src/{name}:{} declares a name-keyed table. The backend \
                     reads `nazm_sema::Resolution`; resolving names here is what N1 \
                     removed, and two resolvers agree only until they do not.",
                    n + 1
                ));
            }
        }
    }

    problems.extend(cross_module_readers(&root));

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Nothing below Core IR asks what a source position meant (N39, `docs/architecture.md`
/// §7.41).
///
/// The one lowering in `nazm-core` reads the checker's **position maps** — the span-keyed
/// answers to which slot a name is, which function a call is, which field, variant or payload
/// field is named, what a `?` means, which type arguments a call settled. Everything after it
/// reads Core IR, where those answers already are. The native backend used to ask each
/// question again, span by span, and the interpreter asked them as it ran; a lookup coming
/// back in either is a second lowering growing beside the first.
///
/// `nazm-lir` may still read the checker's **semantic tables** — definitions, signatures,
/// the call graph, the user-type table — which are about the program, not about where it was
/// written. The interpreter reads Core IR alone: it may not name the resolution or the
/// syntax tree at all.
fn check_core_ir_boundary() -> Result<(), Vec<String>> {
    const POSITION_MAPS: &[&str] = &[
        "res.callee(",
        "res.var(",
        "res.local(",
        "res.field(",
        "res.construction(",
        "res.variant(",
        "res.propagation(",
        "res.payload(",
        "res.type_args(",
        "res.checked_call(",
        "res.checked_calls(",
        "res.written_type(",
        "res.type_name(",
        "res.looked_up_on(",
        "res.scopes(",
    ];
    const INTERPRETER: &str = "crates/nazm-core/src/eval.rs";
    const INTERPRETER_MAY_NOT_NAME: &[&str] = &["Resolution", "nazm_syntax", "res."];
    // N40: the native backend reads validated MIR and nothing of the checker or of Core IR —
    // no `Resolution`, no Core IR, no question to the checker at all.
    const BACKEND: &str = "crates/nazm-lir/src/";
    const BACKEND_MAY_NOT_NAME: &[&str] = &["Resolution", "nazm_cir", "cir::", "res."];
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let mut files: Vec<String> = Vec::new();
    for dir in ["crates/nazm-lir/src", "crates/nazm-mir/src"] {
        match std::fs::read_dir(root.join(dir)) {
            Ok(entries) => {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".rs") {
                        files.push(format!("{dir}/{name}"));
                    }
                }
            }
            Err(e) => return Err(vec![format!("could not read {dir}: {e}")]),
        }
    }
    files.sort();
    files.push(INTERPRETER.to_owned());
    let mut problems = Vec::new();
    for rel in &files {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            problems.push(format!("could not read {rel}"));
            continue;
        };
        // The code with every comment and all whitespace removed, remembering the line each
        // byte came from: a lookup written across lines — `ctx`, `.res`, `.callee(` — is the
        // same lookup, and rustfmt writes long chains exactly that way.
        let mut flat = String::new();
        let mut line_of: Vec<usize> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for c in code.chars().filter(|c| !c.is_whitespace()) {
                flat.push(c);
                line_of.extend(std::iter::repeat_n(n + 1, c.len_utf8()));
            }
        }
        let at = |i: usize| line_of.get(i).copied().unwrap_or(0);
        for pat in POSITION_MAPS {
            for (i, _) in flat.match_indices(pat) {
                problems.push(format!(
                    "{rel}:{} asks the resolution `{pat}…`, a question about a source \
                     position. Below Core IR the answer is in Core IR; asking again is a \
                     second lowering (docs/architecture.md §7.41)",
                    at(i)
                ));
            }
        }
        if rel.starts_with(BACKEND) {
            for word in BACKEND_MAY_NOT_NAME {
                for (i, _) in flat.match_indices(word) {
                    problems.push(format!(
                        "{rel}:{} names `{word}`: the native backend reads validated MIR and \
                         nothing of the checker or of Core IR (docs/architecture.md §7.42)",
                        at(i)
                    ));
                }
            }
        }
        if rel == INTERPRETER {
            for word in INTERPRETER_MAY_NOT_NAME {
                for (i, _) in flat.match_indices(word) {
                    problems.push(format!(
                        "{rel}:{} names `{word}`: the interpreter runs Core IR and nothing \
                         else (docs/architecture.md §7.41)",
                        at(i)
                    ));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Everywhere that can see **every** module's definitions at once.
///
/// N2 made resolution depend on who is asking: a name means what the asking module's
/// environment says, and an environment holds that module's own definitions plus the
/// exports of what it directly imports. `Resolution::defined_anywhere` is the one method
/// that ignores that and looks at the whole compilation. It exists for a single
/// diagnostic — telling a reader that a name is private rather than absent — and it runs
/// only after resolution has already failed.
///
/// One caller. A second one is how a global namespace comes back: not by anyone
/// reintroducing the old map, but by a convenience that answers "where is `parse`
/// defined" without asking whose `parse`.
fn cross_module_readers(root: &std::path::Path) -> Vec<String> {
    const METHOD: &str = "defined_anywhere";
    // Where the method is declared, and the one place it may be called.
    const DECLARED_IN: &str = "crates/nazm-sema/src/resolve.rs";
    const CALLER: &str = "crates/nazm-core/src/check.rs";

    let mut found = Vec::new();
    let mut problems = Vec::new();
    for rel in source_files(root) {
        if rel == DECLARED_IN {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            if line.contains(METHOD) {
                found.push((rel.clone(), n + 1));
            }
        }
    }

    for (file, line) in &found {
        if file != CALLER {
            problems.push(format!(
                "{file}:{line} calls `{METHOD}`, which sees every module's definitions. \
                 Only {CALLER} may, and only to explain a resolution that already failed. \
                 Resolving a name from it would be the global namespace N2 removed."
            ));
        }
    }
    if found.len() > 1 {
        problems.push(format!(
            "`{METHOD}` has {} call sites. It is allowed one, in {CALLER}: {}",
            found.len(),
            found
                .iter()
                .map(|(f, l)| format!("{f}:{l}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    problems
}

/// Every `.rs` file under `crates/`, as a repository-relative path.
///
/// `xtask/` is not scanned, and does not need to be: `rules.rs` gives it no internal
/// dependencies at all, so nothing in it can reach the compiler's types. This file names
/// the method it is checking for, which would otherwise be its own first finding.
fn source_files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out.sort();
    out
}

/// A [`Span`](nazm_span::Span) may not be serialisable
/// A [`Span`](nazm_span::Span) may not be serialisable, because a serialisable one was
/// lossy and nothing about that was visible at the call site.
///
/// # What went wrong, so it is not reinvented
///
/// `nazm.diagnostic/1` publishes `{ start, end }` with no file, at offsets into the
/// concatenation of every source file. When spans gained a `FileId` on 2026-09-21, that
/// compatibility requirement was met by giving `Span` a hand-written `Serialize` that
/// dropped the file and a `Deserialize` that substituted `FileId::ROOT`. The published
/// bytes were right. The hazard was everything else: any future serializer — a cache, an
/// editor protocol, a module graph handed to an agent — would have silently discarded
/// source identity, and the value read back would have been a confident lie rather than
/// an error.
///
/// The conversion now lives in `nazm-diag`, which owns the wire format, and `nazm-span`
/// has no serde dependency at all. This check is what keeps that true: it fails if the
/// manifest gains serde or the source gains a derive, either of which would reopen the
/// door. Adding serde back is allowed — it just cannot happen without editing this.
fn check_span_identity() -> Result<(), Vec<String>> {
    check_unserialisable(
        "nazm-span",
        "A span is a file and a range *in that file*; serialising one without the file is \
         the lossy round trip removed on 2026-09-22. The wire conversion belongs to \
         nazm-diag, which owns nazm.diagnostic/1.",
    )
}

/// The same rule, one layer up: **`nazm-sema` may not be serialisable either.**
///
/// `FileId`, `ModuleId` and `FnId` are indices into one compilation. A file holding one of
/// them says `3` and means nothing to the run that reads it — the same failure as a span
/// that lost its file, in a form that would be harder to notice because `3` is a perfectly
/// good number. So the crate that holds them has no serde dependency, and `nazm-iface`
/// converts explicitly through durable keys instead.
///
/// `ModuleKey` and `DefKey` live in `nazm-sema` too and are perfectly persistable; they
/// travel as the strings they are, written by the crate that owns the format. Their being
/// there is not a reason to relax this.
fn check_sema_purity() -> Result<(), Vec<String>> {
    check_unserialisable(
        "nazm-sema",
        "Its ids are indices into one compilation, and a file holding one says a number \
         that means nothing to the run that reads it. `nazm-iface` owns nazm.interface/1 \
         and converts through durable keys; that is where serde belongs.",
    )
}

/// No serde in this crate's manifest, and no serde derive in its source.
fn check_unserialisable(crate_name: &str, why: &str) -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let mut problems = Vec::new();
    let manifest = root.join(format!("crates/{crate_name}/Cargo.toml"));
    match std::fs::read_to_string(&manifest) {
        Ok(text) => problems.extend(serde_in_manifest(crate_name, &text, why)),
        Err(e) => problems.push(format!("could not read {}: {e}", manifest.display())),
    }

    let dir = root.join(format!("crates/{crate_name}/src"));
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    problems.push(format!("could not read {}", path.display()));
                    continue;
                };
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                problems.extend(
                    serde_in_source(&text).into_iter().map(|(n, what)| {
                        format!("crates/{crate_name}/src/{name}:{n} {what}. {why}")
                    }),
                );
            }
        }
        Err(e) => problems.push(format!("could not read {}: {e}", dir.display())),
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The manifest lines that would put serde back into a crate that must not have it.
fn serde_in_manifest(crate_name: &str, text: &str, why: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .filter(|(_, l)| l.split('=').next().is_some_and(|k| k.trim() == "serde"))
        .map(|(n, _)| format!("crates/{crate_name}/Cargo.toml:{} adds serde. {why}", n + 1))
        .collect()
}

/// The source lines that would make something in one of those crates serialisable.
///
/// Doc comments are exempt: the crate documentation has to be able to *explain* why serde
/// is absent, and a check that forbids naming the thing it forbids is a check nobody can
/// document around.
fn serde_in_source(text: &str) -> Vec<(usize, &'static str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("/*") && !t.starts_with('*')
        })
        .filter_map(|(n, l)| {
            if l.contains("Serialize") || l.contains("Deserialize") {
                Some((n + 1, "derives a serde trait"))
            } else if l.contains("serde") {
                Some((n + 1, "refers to serde"))
            } else {
                None
            }
        })
        .collect()
}

/// **Native identity**: a symbol comes from what a definition *is*, not from where it sat.
///
/// # What this guards
///
/// N5 replaced `@nz.f<N>` — a position in one whole-program numbering — with a spelling of
/// the definition's `DefKey`. Four things could quietly undo that, and none of them would
/// fail a test on the day it was written:
///
/// 1. **a session id reaching a symbol.** `FnId` is an index into one `Resolution`. A
///    symbol derived from one is a symbol that moves when an unrelated module gains a
///    function, which is exactly what separate code generation cannot have;
/// 2. **`pub` choosing a spelling.** Visibility decides *linkage* — whether the linker
///    must be able to bind the name — and must not decide the name, because a native
///    symbol is compiler-private and `pub` is a statement about Nazm modules. A symbol
///    that changed when a definition was exported would be an ABI promise nobody made;
/// 3. **a positional symbol coming back.** The literal shape that was removed;
/// 4. **LLVM spelling reaching `nazm-sema`.** Semantic identity is backend-independent so
///    that a second backend can spell the same `DefKey` its own way. A `@nz.` in the
///    crate that owns identity is that boundary being crossed.
///
/// `ModuleId` is *permitted* in the symbol module and appears there deliberately: a module
/// with no durable identity gets a compilation-local symbol, which `docs/architecture.md`
/// records as an honest limitation rather than a durable-looking guess.
fn check_native_identity() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let mut problems = Vec::new();

    let symbol = root.join("crates/nazm-lir/src/symbol.rs");
    match std::fs::read_to_string(&symbol) {
        Ok(text) => {
            for (n, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') {
                    continue;
                }
                if line.contains("FnId") || line.contains("FuncId") {
                    problems.push(format!(
                        "crates/nazm-lir/src/symbol.rs:{} names a session-local function id. \
                         A symbol derived from one moves when an unrelated module gains a \
                         function, which is what N5 removed.",
                        n + 1
                    ));
                }
                for word in ["Visibility", "is_exported", "pub_span"] {
                    if line.contains(word) {
                        problems.push(format!(
                            "crates/nazm-lir/src/symbol.rs:{} names `{word}`. `pub` chooses \
                             linkage, never spelling: a symbol that changed when a \
                             definition was exported would be a binary promise the language \
                             does not make.",
                            n + 1
                        ));
                    }
                }
            }
        }
        Err(e) => problems.push(format!("could not read {}: {e}", symbol.display())),
    }

    // The positional form, anywhere in the backend.
    for rel in source_files(&root) {
        if !rel.starts_with("crates/nazm-lir/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('*') {
                continue;
            }
            if line.contains("@nz.f{") || line.contains("nz.f{") {
                problems.push(format!(
                    "{rel}:{} builds a symbol from a number. Whole-program positional \
                     naming is what stops a module being generated on its own.",
                    n + 1
                ));
            }
        }
    }

    // And the boundary the other way: identity does not know how a backend spells it.
    let dir = root.join("crates/nazm-sema/src");
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                for (n, line) in text.lines().enumerate() {
                    let t = line.trim_start();
                    if t.starts_with("//") || t.starts_with('*') {
                        continue;
                    }
                    if line.contains("@nz.") || line.contains("NativeSymbol") {
                        problems.push(format!(
                            "crates/nazm-sema/src/{name}:{} carries a backend's spelling. A \
                             `DefKey` says which definition this is; what a linker calls it \
                             is `nazm-lir`'s, so that a second backend can answer \
                             differently.",
                            n + 1
                        ));
                    }
                }
            }
        }
        Err(e) => problems.push(format!("could not read {}: {e}", dir.display())),
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// **Cache soundness**: a check key cannot quietly stop covering what checking reads.
///
/// # The failure this exists to catch
///
/// A cache entry says *this module checked cleanly under these inputs*. If an input stops
/// reaching the key, the entry is found for a compilation that should not have found it,
/// and the compiler accepts a program it would otherwise refuse — silently, reproducibly,
/// and with an explanation that sounds right. `docs/architecture.md` §3 names it as the
/// failure mode; this is the part of it a file read can see.
///
/// Three rules, each a thing no test can notice until the day it is wrong:
///
/// 1. **the checker's diagnostic codes are in its identity.** Almost every new checking
///    rule brings a code, so this makes adding one fail the build until
///    `nazm-cache`'s `CHECKER_CODES` is updated — and that list sits beside
///    `SEMANTIC_EPOCH`, which is the constant a person has to remember to bump;
/// 2. **every field of a check key is still computed into it.** Deleting one compiles,
///    passes a careless review, and is a wrong-answer bug;
/// 3. **no session-local id may reach `nazm-cache`.** The same rule as the span and sema
///    gates, one layer further out: a `FnId` written into a cache entry is a number that
///    means nothing to the run that reads it.
fn check_cache_soundness() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let mut problems = Vec::new();

    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();

    // 1. The codes the checker can raise, against the list the identity is built from.
    // The checker is `check.rs` and, since N36, the effect checker beside it — and since
    // N38 the provenance solver.
    let checker = match (
        read("crates/nazm-core/src/check.rs"),
        read("crates/nazm-core/src/effects.rs"),
        read("crates/nazm-core/src/provenance.rs"),
    ) {
        (Some(check), Some(effects), Some(provenance)) => {
            Some(format!("{check}\n{effects}\n{provenance}"))
        }
        _ => None,
    };
    match (checker, read("crates/nazm-cache/src/identity.rs")) {
        (Some(checker), Some(identity)) => {
            let mut raised: Vec<String> = checker
                .match_indices("codes::")
                .map(|(at, _)| {
                    checker
                        .get(at + "codes::".len()..)
                        .unwrap_or("")
                        .chars()
                        .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                        .collect::<String>()
                })
                .filter(|c| !c.is_empty())
                .collect();
            raised.sort();
            raised.dedup();

            let mut listed: Vec<String> = identity
                .lines()
                .skip_while(|l| !l.contains("pub const CHECKER_CODES"))
                .take_while(|l| !l.contains("];"))
                .filter_map(|l| l.trim().strip_prefix('"'))
                .filter_map(|l| l.split('"').next())
                .map(str::to_owned)
                .collect();
            listed.sort();
            listed.dedup();

            for code in raised.iter().filter(|c| !listed.contains(c)) {
                problems.push(format!(
                    "the checker can raise `{code}` and `nazm-cache`'s CHECKER_CODES does \
                     not list it. A new rule changes what a cached result means: add it \
                     there, and bump SEMANTIC_EPOCH in the same commit."
                ));
            }
            for code in listed.iter().filter(|c| !raised.contains(c)) {
                problems.push(format!(
                    "CHECKER_CODES lists `{code}` and the checker no longer raises it. The \
                     identity must describe the checker that exists."
                ));
            }
        }
        _ => problems.push(
            "could not read the checker or the cache identity; the cache-soundness gate \
             cannot be vacuous"
                .to_owned(),
        ),
    }

    // 2. Every input still reaches the key.
    match read("crates/nazm-cache/src/key.rs") {
        Some(text) => {
            let body = text
                .split("pub fn of(")
                .nth(1)
                .and_then(|t| t.split("\n    }").next())
                .unwrap_or("");
            for (what, needle) in [
                ("the checker's identity", "checker.to_string()"),
                ("the module's durable key", "module.as_str()"),
                ("the source fingerprint", "source.to_string()"),
                ("each dependency's key", "d.module.as_str()"),
                ("each dependency's interface", "d.interface.to_string()"),
            ] {
                if !body.contains(needle) {
                    problems.push(format!(
                        "a check key no longer covers {what}. An input left out of the key \
                         is a cache hit for a compilation whose answer may differ — which \
                         is a wrong program accepted, not a slow build."
                    ));
                }
            }
        }
        None => problems.push("could not read crates/nazm-cache/src/key.rs".to_owned()),
    }

    // 3. Nothing session-local may reach the crate that writes cache entries.
    let dir = root.join("crates/nazm-cache/src");
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                for (n, line) in text.lines().enumerate() {
                    let t = line.trim_start();
                    // Documentation has to be able to explain what is absent and why.
                    if t.starts_with("//") || t.starts_with('*') {
                        continue;
                    }
                    for id in ["FileId", "ModuleId", "FnId", "Span"] {
                        if line.contains(id) {
                            problems.push(format!(
                                "crates/nazm-cache/src/{name}:{} names `{id}`. It is an index \
                                 into one compilation, and an entry holding one says a number \
                                 that means nothing to the run that reads it.",
                                n + 1
                            ));
                        }
                    }
                }
            }
        }
        Err(e) => problems.push(format!("could not read {}: {e}", dir.display())),
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The native object cache: what a key must cover, and what a lookup must prove.
///
/// # Why a gate and not only tests
///
/// The tests establish behaviour — a body edit reuses a caller's object, a corrupted entry
/// is a miss. Each of them would keep passing if a key quietly *gained* a field, and each
/// would fail for a reason that reads like a flaky cache if one quietly lost one. These
/// rules are about the shape of the thing rather than about one run of it:
///
/// 1. an `ObjectKey` still covers all three of its inputs;
/// 2. reading an entry still recomputes the key from its recorded inputs **and** the digest
///    of the object beside them;
/// 3. nothing semantic reaches the key — no module identity, no interface, no source
///    fingerprint. Including any of them would be sound and would throw away exactly the
///    precision N5 measured;
/// 4. the stored header carries the fields it validates and no others, so a field cannot be
///    added to the wire without someone deciding whether it belongs in the key;
/// 5. the arguments that are hashed and the arguments that are executed come from one
///    function, so a key cannot describe an invocation that did not happen;
/// 6. the object compiler is invoked with an environment built from nothing, both when it
///    is asked what it will do and when it does it.
fn check_object_cache() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let mut problems = Vec::new();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();

    // 1 and 3. The key, and nothing but the key.
    match read("crates/nazm-cache/src/object.rs") {
        Some(text) => {
            let body = text
                .split("pub fn of(input: LlvmInput")
                .nth(1)
                .and_then(|t| t.split("\n    }").next())
                .unwrap_or("");
            for (what, needle) in [
                ("the emitted LLVM", "input.to_string()"),
                ("the backend's identity", "tool.to_string()"),
                ("the resolved configuration", "config.to_string()"),
            ] {
                if !body.contains(needle) {
                    problems.push(format!(
                        "an object key no longer covers {what}. An input left out of the \
                         key is an object handed to a build that did not produce it — a \
                         wrong program linked, not a slow build."
                    ));
                }
            }
            for line in text.lines().filter(|l| l.trim_start().starts_with("use ")) {
                for id in [
                    "ModuleKey",
                    "InterfaceHash",
                    "SourceFingerprint",
                    "CheckKey",
                ] {
                    if line.contains(id) {
                        problems.push(format!(
                            "crates/nazm-cache/src/object.rs imports `{id}`. An object is \
                             keyed on what the object compiler reads; a semantic fact in \
                             the key would invalidate a caller whose own input never moved."
                        ));
                    }
                }
            }
        }
        None => problems.push("could not read crates/nazm-cache/src/object.rs".to_owned()),
    }

    // 2 and 4. What a lookup proves, and what an entry may carry.
    match read("crates/nazm-cache/src/objects.rs") {
        Some(text) => {
            let body = text
                .split("pub fn read(")
                .nth(1)
                .and_then(|t| t.split("\n    }").next())
                .unwrap_or("");
            for (what, needle) in [
                (
                    "recompute the key from the inputs it records",
                    "ObjectKey::of(input, tool, config)",
                ),
                (
                    "recompute the digest of the object it carries",
                    "ObjectFingerprint::of(bytes)",
                ),
                ("check the length it claims", "header.length"),
            ] {
                if !body.contains(needle) {
                    problems.push(format!(
                        "reading a stored object no longer {what}. A file name is how an \
                         entry is found and never why it is believed."
                    ));
                }
            }

            const HEADER: &[&str] = &[
                "schema",
                "key",
                "llvm_input",
                "tool",
                "config",
                "object",
                "length",
            ];
            let fields: Vec<String> = text
                .split("pub struct ObjectHeader {")
                .nth(1)
                .and_then(|t| t.split("\n}").next())
                .unwrap_or("")
                .lines()
                .filter_map(|l| l.trim().strip_prefix("pub "))
                .filter_map(|l| l.split(':').next())
                .map(str::to_owned)
                .collect();
            for f in &fields {
                if !HEADER.contains(&f.as_str()) {
                    problems.push(format!(
                        "a stored object's header carries `{f}`, which this gate does not \
                         know. Every field in an entry is recomputed or compared — add it \
                         here once it is, so that a field nothing validates cannot appear."
                    ));
                }
            }
            for f in HEADER {
                if !fields.iter().any(|x| x == f) {
                    problems.push(format!(
                        "a stored object's header no longer carries `{f}`, so a lookup has \
                         one fewer thing it can prove."
                    ));
                }
            }
        }
        None => problems.push("could not read crates/nazm-cache/src/objects.rs".to_owned()),
    }

    // 5 and 6. One argument list, and an environment built from nothing.
    match read("crates/nazm-cli/src/backend.rs") {
        Some(text) => {
            let uses = text.matches("compile_args(").count();
            if uses < 3 {
                problems.push(
                    "the probe and the compilation no longer share one argument list. Two \
                     lists are two lists that can differ, and the difference is a key \
                     describing an invocation nobody ran."
                        .to_owned(),
                );
            }
            if text.matches("env_clear()").count() < 2 {
                problems.push(
                    "the object compiler is no longer invoked with an environment built \
                     from nothing. `CCC_OVERRIDE_OPTIONS` was measured to rewrite an \
                     invocation's own flags, which no key can see."
                        .to_owned(),
                );
            }
        }
        None => problems.push("could not read crates/nazm-cli/src/backend.rs".to_owned()),
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// No crate may **depend on** a code generator unless `rules.rs` says it can.
///
/// # What this establishes, and what it does not
///
/// It reads cargo metadata, so it is a statement about the dependency graph and nothing
/// else. **Emitting LLVM IR as text is code generation and this check cannot see it** —
/// `nazm-lir` generates a complete LLVM module with no codegen dependency at all, and
/// passes here trivially. Describing that as architecture validation would be describing
/// a vacuous result.
///
/// The capability question — *who may emit a backend artefact, and who may invoke the
/// toolchain* — is [`check_emission_reach`], which looks at call sites instead. The two
/// together are what `docs/architecture.md` §1 means by keeping Cranelift and LLVM behind
/// the LIR contract; neither is sufficient alone.
/// Every catalogued mutation still matches its file, exactly once.
///
/// A mutation whose pattern has drifted is reported `NOT INJECTED` — correctly, because a
/// defect that was never applied says nothing about the tests. But the runner only
/// discovers that after copying the tree, building the workspace and running the suite
/// once, which is minutes per entry inside the contained environment; and until it gets
/// there the catalogue's *size* is a claim about coverage that one of its entries is not
/// keeping.
///
/// This found a real one. `an-absolute-path-reaches-the-module-key` had matched nothing
/// since before N7: the line it names was rewritten to make both paths absolute, and the
/// pattern went on naming the old spelling. The catalogue said 85 and meant 84.
///
/// Pure file reads, no build, so it belongs in `check` where the cost is a few
/// milliseconds rather than in the run it protects.
fn check_mutations_apply() -> Result<(), Vec<String>> {
    const CATALOGUE: &str = "xtask/mutations/mutations.toml";
    let text = match std::fs::read_to_string(CATALOGUE) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{CATALOGUE}: {e}")]),
    };
    let catalogue: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("{CATALOGUE} is not valid TOML: {e}")]),
    };
    let Some(entries) = catalogue.get("mutation").and_then(toml::Value::as_array) else {
        return Err(vec![format!("{CATALOGUE} has no `[[mutation]]` entries")]);
    };

    let mut bad = Vec::new();
    // One read per file however many mutations name it: the catalogue is nearly a hundred
    // entries over a dozen files.
    let mut sources: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for entry in entries {
        let field = |name: &str| entry.get(name).and_then(toml::Value::as_str);
        let (Some(name), Some(file), Some(find)) = (field("name"), field("file"), field("find"))
        else {
            bad.push(format!(
                "{CATALOGUE} has an entry missing `name`, `file` or `find`"
            ));
            continue;
        };
        let source = match sources.get(file) {
            Some(s) => s,
            None => match std::fs::read_to_string(file) {
                Ok(s) => sources.entry(file.to_owned()).or_insert(s),
                Err(e) => {
                    bad.push(format!("{name}: cannot read {file}: {e}"));
                    continue;
                }
            },
        };
        match source.matches(find).count() {
            1 => {}
            0 => bad.push(format!(
                "{name}: its pattern is not in {file} any more, so the run would report it \
                 NOT INJECTED after building the workspace to find out. Repoint it at the \
                 line that carries the rule now, or retire it"
            )),
            n => bad.push(format!(
                "{name}: its pattern appears {n} times in {file}, so the runner cannot say \
                 which one it means. Widen it with enough context to be unique"
            )),
        }
    }

    // The typed schema, and everything about killers and profiles that can be checked
    // without building: unknown fields, duplicate names, a killer or profile target
    // that does not exist, an ignored killer, a profile named without a killer and
    // without a reason. A stale killer fails here in milliseconds rather than in a
    // campaign an hour in; the campaign still re-checks against the built binary.
    match plan::load(std::path::Path::new(CATALOGUE)) {
        Err(e) => bad.push(e),
        Ok(cat) => bad.extend(plan::validate(std::path::Path::new("."), &cat)),
    }

    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

/// The two compilers emit **the same runtime**, derived rather than transcribed.
///
/// `compiler/emit.nz` writes the same helpers `crates/nazm-lir/src/emit.rs` writes, because
/// the compiler written in Nazm has to give its programs the memory semantics the compiler
/// written in Rust gives its own. Until N8 the two were separate transcriptions kept in
/// step by hand, and they fell out of step in exactly the way that arrangement predicts:
/// N7 taught the Rust emitter to reclaim sequence storage and the Nazm one went on emitting
/// a header with no reference count at all, for a milestone, silently.
///
/// So the Nazm text is now **derived**: every `strs_push` line below is a line of the Rust
/// runtime constants with `hidden` changed to `internal`. This gate performs that
/// derivation again and refuses a difference. It is a pure file read; it builds nothing.
///
/// What it does *not* check is that the two emitters make the same *decisions* — where to
/// retain, where to release. Nothing textual can check that, and it is what the
/// differential memory tests in `crates/nazm-cli/tests/selfhost.rs` are for.
fn check_runtime_parity() -> Result<(), Vec<String>> {
    const RUST: &str = "crates/nazm-lir/src/runtime/core.rs";
    const NAZM: &str = "compiler/emit.nz";
    // Which Rust runtime constants make up each Nazm function, in order.
    const GROUPS: &[(&str, &[&str])] = &[
        (
            "runtime",
            &[
                "SEQUENCE_RUNTIME",
                "STRING_RUNTIME",
                "IO_RUNTIME",
                "JOIN_RUNTIME",
            ],
        ),
        ("task_runtime", &["TASK_RUNTIME"]),
        ("channel_runtime", &["CHANNEL_RUNTIME"]),
    ];

    let mut bad = Vec::new();
    let rust = match std::fs::read_to_string(RUST) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{RUST}: {e}")]),
    };
    let nazm = match std::fs::read_to_string(NAZM) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{NAZM}: {e}")]),
    };

    for (function, constants) in GROUPS {
        let mut lines: Vec<String> = Vec::new();
        let mut missing = false;
        for name in *constants {
            match raw_string_constant(&rust, name) {
                Some(body) => {
                    lines.extend(body);
                    lines.push(String::new());
                }
                None => {
                    bad.push(format!("{RUST} has no runtime constant `{name}`"));
                    missing = true;
                }
            }
        }
        if missing {
            continue;
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let mut want = String::from("    let r = strs_new();\n");
        for line in &lines {
            let line = line.replace("define hidden ", "define internal ");
            if line.contains('"') || line.contains('\\') {
                bad.push(format!(
                    "the runtime line `{line}` needs an escape Nazm string literals do not have"
                ));
            }
            want.push_str(&format!("    strs_push(r, \"{line}\");\n"));
        }
        want.push_str("    str_concat(str_join(r, nl()), nl())\n");
        match nazm_function_body(&nazm, function) {
            None => bad.push(format!("{NAZM} has no `fn {function}() -> Str`")),
            Some(have) if have == want => {}
            Some(have) => bad.push(format!(
                "{NAZM}'s `{function}` is not `{RUST}`'s runtime: {}. Regenerate it rather \
                 than editing it — the Rust runtime is the one that is written",
                first_difference(&want, &have)
            )),
        }
    }

    // The memory report's two strings, so a program built by either compiler prints the
    // same line and the differential tests can compare them.
    for (constant, function) in [
        ("MEMORY_VARIABLE", "memory_variable"),
        ("MEMORY_FORMAT", "memory_format"),
    ] {
        let Some(want) = rust_string_constant(&rust, constant) else {
            bad.push(format!("{RUST} has no `{constant}`"));
            continue;
        };
        match nazm_function_body(&nazm, function) {
            None => bad.push(format!("{NAZM} has no `fn {function}() -> Str`")),
            Some(have) if have.contains(&format!("\"{want}\"")) => {}
            Some(_) => bad.push(format!(
                "{NAZM}'s `{function}` does not return `{constant}`'s text, so the two \
                 compilers' programs would print different memory reports"
            )),
        }
    }

    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

/// The lines of `const NAME: &str = r"…";`, without its leading and trailing blank lines.
fn raw_string_constant(source: &str, name: &str) -> Option<Vec<String>> {
    let open = format!("const {name}: &str = r\"");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\";")?;
    let mut lines: Vec<String> = rest.get(..end)?.lines().map(str::to_owned).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    Some(lines)
}

/// The text of `pub const NAME: &str = "…";`, with Rust's line continuations resolved.
fn rust_string_constant(source: &str, name: &str) -> Option<String> {
    let open = format!("pub const {name}: &str = \"");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\";")?;
    let raw = rest.get(..end)?;
    // A `\` at the end of a line continues the literal and eats the following indentation.
    let mut out = String::new();
    let mut parts = raw.split('\\');
    out.push_str(parts.next().unwrap_or_default());
    for part in parts {
        out.push_str(part.trim_start());
    }
    Some(out)
}

/// The body of `fn NAME() -> Str { … }`, between its braces, as written.
fn nazm_function_body(source: &str, name: &str) -> Option<String> {
    let open = format!("\nfn {name}() -> Str {{\n");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\n}\n")?;
    Some(rest.get(..=end)?.to_owned())
}

/// The first line where two texts differ, for a message that names it rather than printing
/// two thousand lines and leaving the reader to diff them.
fn first_difference(want: &str, have: &str) -> String {
    for (n, (a, b)) in want.lines().zip(have.lines()).enumerate() {
        if a != b {
            return format!("line {} is `{}`, expected `{}`", n + 1, b.trim(), a.trim());
        }
    }
    format!(
        "it has {} lines where {} were derived",
        have.lines().count(),
        want.lines().count()
    )
}

fn check_codegen_reach() -> Result<(), Vec<String>> {
    const CODEGEN_CRATES: &[&str] = &[
        "cranelift",
        "cranelift-codegen",
        "cranelift-jit",
        "cranelift-module",
        "inkwell",
        "llvm-sys",
    ];

    let meta = match metadata::workspace() {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("could not read cargo metadata: {e}")]),
    };

    let mut problems = Vec::new();
    for member in &meta.members {
        if rules::may_depend_on_codegen(&member.name) {
            continue;
        }
        for dep in &member.all_deps {
            if CODEGEN_CRATES.contains(&dep.as_str()) {
                problems.push(format!(
                    "{} depends on {dep}. Only the codegen crates and the driver may reach \
                     a code generator — everything else goes through LIR.",
                    member.name
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The `unsafe` allowlist in `rules.rs` and the workspace lint must say the same thing.
///
/// They are two statements of one policy in two files, which is exactly the shape of
/// control that drifts. While the allowlist is empty the lint must be `forbid`; the moment
/// a crate is allowed `unsafe`, the lint must have been relaxed to `deny` in the same
/// change — `forbid` cannot be overridden per-crate, so a mismatch would not merely be
/// untidy, it would not build.
fn check_unsafe_policy() -> Result<(), Vec<String>> {
    let root = match std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        Some(r) => r.to_path_buf(),
        None => return Err(vec!["could not locate workspace root".to_owned()]),
    };

    let manifest = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(s) => s,
        Err(e) => return Err(vec![format!("could not read Cargo.toml: {e}")]),
    };

    let meta = match metadata::workspace() {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("could not read cargo metadata: {e}")]),
    };

    let any_allowed = meta
        .members
        .iter()
        .any(|m| rules::unsafe_allowed(&m.name).is_some());
    let lint_forbids = manifest.contains(r#"unsafe_code = "forbid""#);

    let mut problems = Vec::new();
    match (any_allowed, lint_forbids) {
        (false, false) => problems.push(
            "the unsafe allowlist in xtask/src/rules.rs is empty, but the workspace lint is \
             not `forbid`. Either tighten the lint or record why the exception exists."
                .to_owned(),
        ),
        (true, true) => problems.push(
            "xtask/src/rules.rs allows `unsafe` in a crate while the workspace lint is still \
             `forbid`, which cannot be overridden per-crate. Relax it to `deny` in the same \
             change that grants the exception."
                .to_owned(),
        ),
        _ => {}
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Inject every catalogued defect and require the suite to catch it.
///
/// Not part of `check`: it rebuilds and re-tests the workspace once per mutation, so it is
/// a scheduled job rather than a pre-commit gate.
/// `--name value` pairs and bare `--name` switches, in any order.
///
/// Deliberately minimal. A switch is a `--name` whose next token is another `--name` or
/// the end, and it maps to an empty string — the caller asks with `contains_key`. The
/// first version required a value unconditionally, so a trailing `--confirm-no-writers`
/// silently vanished and recovery refused a tree it had been told about. A flag a user
/// typed and the program did not see is worse than no flag.
fn flags(args: &mut impl Iterator<Item = String>) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        let Some(name) = a.strip_prefix("--") else {
            continue;
        };
        let value = match args.peek() {
            Some(next) if !next.starts_with("--") => args.next().unwrap_or_default(),
            _ => String::new(),
        };
        out.insert(name.to_owned(), value);
    }
    out
}

fn need(f: &std::collections::HashMap<String, String>, k: &str) -> Result<String, ExitCode> {
    match f.get(k) {
        Some(v) => Ok(v.clone()),
        None => {
            eprintln!("missing --{k}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// Run the catalogue against an arbitrary root. The lifecycle tests point this at a
/// disposable project so an interrupted run damages nothing that matters.
fn mutate_run(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let (Ok(root), Ok(catalogue)) = (need(&f, "root"), need(&f, "catalogue")) else {
        return ExitCode::FAILURE;
    };
    // The lifecycle tests want the whole of whatever catalogue they point at, so no
    // filter here. They do choose the strategy, and whether to resume.
    let mut o = mutate::Options::new(
        std::path::Path::new(&root),
        std::path::Path::new(&catalogue),
    );
    if let Some(s) = f.get("strategy") {
        match campaign::Strategy::parse(s) {
            Some(s) => o.strategy = s,
            None => {
                eprintln!("unknown strategy {s}");
                return ExitCode::FAILURE;
            }
        }
    }
    o.resume = f.contains_key("resume");
    if let Some(d) = f.get("campaign-dir") {
        o.campaign_dir = Some(std::path::PathBuf::from(d));
    }
    if let Some(n) = f.get("killer-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.killer_secs = n;
    }
    if let Some(n) = f.get("profile-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.profile_target_secs = n;
    }
    if let Some(n) = f.get("broad-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.broad_secs = n;
    }
    if let Some(n) = f.get("budget-secs").and_then(|n| n.parse().ok()) {
        o.budget = Some(std::time::Duration::from_secs(n));
    }
    if let Some(n) = f.get("cargo-jobs").and_then(|n| n.parse().ok()) {
        o.parallelism.cargo_jobs = n;
    }
    if let Some(n) = f.get("test-threads").and_then(|n| n.parse().ok()) {
        o.parallelism.test_threads = n;
    }
    o.verify_killers = f.contains_key("verify-killers");
    o.disposable_target = f.contains_key("disposable-target");
    finish_campaign(mutate::run_all(&o))
}

/// Exit status for a campaign: 0 clean, 1 problems or refused, 3 incomplete. Incomplete
/// is its own code so a supervisor can resume it without mistaking it for either.
fn finish_campaign(end: mutate::End) -> ExitCode {
    match end {
        mutate::End::Clean => {
            println!("ok    mutation coverage");
            ExitCode::SUCCESS
        }
        mutate::End::Problems(p) | mutate::End::Refused(p) => {
            println!("FAIL  mutation coverage");
            for x in p {
                println!("problem: {x}");
            }
            ExitCode::FAILURE
        }
        mutate::End::Incomplete(p) => {
            println!("INCOMPLETE mutation coverage");
            for x in p {
                println!("problem: {x}");
            }
            ExitCode::from(3)
        }
    }
}

/// `cargo xtask mutate …`. Every option is parsed strictly: a typo that silently ran the
/// whole catalogue, or none of it, would waste hours either way.
fn mutate_cli(rest: &[String]) -> ExitCode {
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        eprintln!("could not locate workspace root");
        return ExitCode::FAILURE;
    };
    let catalogue = root.join("xtask/mutations/mutations.toml");
    let mut o = mutate::Options::new(root, &catalogue);
    let mut plan_only = false;
    let mut verify: Vec<String> = Vec::new();
    let mut candidates = Vec::new();
    let mut i = 0;
    let value = |i: usize, what: &str| -> Result<String, ExitCode> {
        rest.get(i + 1).cloned().ok_or_else(|| {
            eprintln!("mutate: {what} needs a value");
            ExitCode::FAILURE
        })
    };
    while let Some(arg) = rest.get(i) {
        let step = match arg.as_str() {
            "--only" => match value(i, "--only") {
                Ok(v) => {
                    o.only.push(v);
                    2
                }
                Err(e) => return e,
            },
            "--strategy" => match value(i, "--strategy").map(|v| campaign::Strategy::parse(&v)) {
                Ok(Some(s)) => {
                    o.strategy = s;
                    2
                }
                Ok(None) => {
                    eprintln!("mutate: --strategy is `targeted` or `full`");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--plan" => {
                plan_only = true;
                1
            }
            "--resume" => {
                o.resume = true;
                1
            }
            "--reset-derived-caches" => {
                o.reset_derived = true;
                1
            }
            "--export-journal" => {
                o.export_journal = true;
                1
            }
            // N32-H: verify each decided mutant's killers in the same warm session.
            "--verify-killers" => {
                o.verify_killers = true;
                1
            }
            // N32-H2: the target is the harness's cache and may be cleaned.
            "--disposable-target" => {
                o.disposable_target = true;
                1
            }
            // N14.1: each step's internal parallelism. Never more mutants at once.
            "--cargo-jobs" | "--test-threads" => match value(i, arg).map(|v| v.parse::<u32>()) {
                Ok(Ok(n)) if (1..=16).contains(&n) => {
                    if arg == "--cargo-jobs" {
                        o.parallelism.cargo_jobs = n;
                    } else {
                        o.parallelism.test_threads = n;
                    }
                    2
                }
                Ok(_) => {
                    eprintln!("mutate: {arg} needs a whole number from 1 to 16");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--budget-secs" => match value(i, "--budget-secs").map(|v| v.parse::<u64>()) {
                Ok(Ok(n)) => {
                    o.budget = Some(std::time::Duration::from_secs(n));
                    2
                }
                Ok(Err(_)) => {
                    eprintln!("mutate: --budget-secs needs a whole number");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--campaign-dir" => match value(i, "--campaign-dir") {
                Ok(v) => {
                    o.campaign_dir = Some(v.into());
                    2
                }
                Err(e) => return e,
            },
            "--resume-from" => match value(i, "--resume-from") {
                Ok(v) => {
                    o.resume_from = Some(v.into());
                    2
                }
                Err(e) => return e,
            },
            "--source-commit" => match value(i, "--source-commit") {
                Ok(v) => {
                    o.source_commit = Some(v);
                    2
                }
                Err(e) => return e,
            },
            "--verify-killer" => match value(i, "--verify-killer") {
                Ok(v) => {
                    verify.push(v);
                    2
                }
                Err(e) => return e,
            },
            "--killer" => match value(i, "--killer").map(|v| mutate::parse_killer(&v)) {
                Ok(Ok(k)) => {
                    candidates.push(k);
                    2
                }
                Ok(Err(e)) => {
                    eprintln!("mutate: {e}");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            other => {
                eprintln!("mutate: unknown option {other}");
                return ExitCode::FAILURE;
            }
        };
        i += step;
    }
    if !candidates.is_empty() && verify.len() != 1 {
        eprintln!("mutate: --killer names candidates for exactly one --verify-killer");
        return ExitCode::FAILURE;
    }
    if plan_only {
        return match mutate::plan_only(root, &catalogue, &o.only) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(problems) => {
                for p in problems {
                    println!("problem: {p}");
                }
                ExitCode::FAILURE
            }
        };
    }
    if !verify.is_empty() {
        // One after another, each with its own pristine, mutated and restored builds,
        // so a session pays its cold build once for all of them.
        let mut failed = false;
        for name in &verify {
            println!("verifying killers for {name}");
            match mutate::verify_killer(&o, name, &candidates) {
                Ok(report) => print!("{report}"),
                Err(problems) => {
                    failed = true;
                    for p in problems {
                        println!("problem: {p}");
                    }
                }
            }
        }
        return if failed {
            println!("FAIL  killer verification");
            ExitCode::FAILURE
        } else {
            println!("ok    killer verification");
            ExitCode::SUCCESS
        };
    }
    finish_campaign(mutate::run_all(&o))
}

/// Run one script through the harness's subprocess runner and report how it ended.
///
/// Hidden, for the lifecycle tests: the watchdog can only be shown to work by killing
/// the process that armed it, which a test can do to this process and not to itself.
fn mutate_exec(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let Ok(script) = need(&f, "script") else {
        return ExitCode::FAILURE;
    };
    let secs = f
        .get("deadline-secs")
        .and_then(|n| n.parse().ok())
        .unwrap_or(60);
    let r = procs::run(&procs::Invocation {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), script],
        cwd: std::env::current_dir().unwrap_or_default(),
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(secs),
    });
    println!(
        "ended {:?} after {:.1}s, swept {}",
        r.ended,
        r.elapsed.as_secs_f64(),
        r.swept
    );
    ExitCode::SUCCESS
}

/// Take the lock, announce it, and hold it until told to stop.
///
/// The announcement is a file rather than a line on stdout because the test has to know
/// the lock is *held* before it starts competing, and a pipe read is one more thing that
/// can be buffered. Written after acquisition, so its existence is the fact it claims.
fn mutate_hold(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let (Ok(root), Ok(ready), Ok(release)) =
        (need(&f, "root"), need(&f, "ready"), need(&f, "release"))
    else {
        return ExitCode::FAILURE;
    };

    let Ok(lock) = mutate::acquire_lock(std::path::Path::new(&root)) else {
        eprintln!("hold: could not acquire");
        return ExitCode::FAILURE;
    };
    if std::fs::write(&ready, "held").is_err() {
        return ExitCode::FAILURE;
    }

    // Bounded, so a test that fails to signal leaves no process behind for the next one
    // to trip over.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !std::path::Path::new(&release).exists() {
        if std::time::Instant::now() > deadline {
            eprintln!("hold: released on the deadline, not on request");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(lock);
    ExitCode::SUCCESS
}

/// Startup recovery on its own, through the same function a real run calls.
fn mutate_recover(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    // An operator runs this from the checkout and should not have to say where it is.
    // The lifecycle tests pass `--root` to point it at a disposable project instead.
    let root = match f.get("root") {
        Some(r) => r.clone(),
        None => match std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            Some(p) => p.display().to_string(),
            None => {
                eprintln!("could not locate the workspace root");
                return ExitCode::FAILURE;
            }
        },
    };
    match mutate::recover(
        std::path::Path::new(&root),
        f.contains_key("confirm-no-writers"),
    ) {
        Ok(outcome) => {
            println!("{outcome}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `cargo xtask contained <workload>` — the only sanctioned way to run a generated
/// compiler stage, the selfhost suite, the bootstrap, fuzzing, the mutation catalogue or
/// a heavy benchmark. See `contained.rs` for why an environment variable is not enough.
fn run_contained(args: &[String]) -> ExitCode {
    use contained::{Bounds, Outcome};
    use std::time::Duration;

    let repo = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("contained: cannot resolve the repository root: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut bounds = Bounds::default();

    // `--profile NAME` before the workload chooses its internal parallelism (N14.1).
    // Without one, each workload takes the profile its calibration adopted.
    let (chosen, args) = match args.first().map(String::as_str) {
        Some("--profile") => {
            let Some(p) = args
                .get(1)
                .and_then(|n| contained::ExecutionProfile::by_name(n))
            else {
                let names: Vec<&str> = contained::ExecutionProfile::ALL
                    .iter()
                    .map(|p| p.name)
                    .collect();
                eprintln!("contained: --profile is one of {}", names.join(", "));
                return ExitCode::FAILURE;
            };
            (Some(p), args.get(2..).unwrap_or_default())
        }
        _ => (None, args),
    };
    // `--cache bench` (N32-H3): a benchmark workload's release builds kept in their own
    // harness-owned namespace. What a benchmark measures is the binary it runs, which is
    // the same bytes wherever it was compiled; only the compile is saved.
    let (explicit_cache, args) = match args.first().map(String::as_str) {
        Some("--cache") => {
            match args.get(1).map(String::as_str) {
                Some("bench") => (Some("bench"), args.get(2..).unwrap_or_default()),
                _ => {
                    eprintln!("contained: --cache takes `bench`; the other namespaces are the workloads' own");
                    return ExitCode::FAILURE;
                }
            }
        }
        _ => (None, args),
    };
    let workload = args.first().map(String::as_str).unwrap_or_default();
    let profile = chosen.unwrap_or_else(|| default_profile(workload));
    bounds.profile = profile;
    bounds.milli_cpus = profile.milli_cpus;
    let j = profile.cargo_jobs;

    let (name, argv): (&str, Vec<String>) = match args.first().map(String::as_str) {
        Some("probe") => return contained_probe(&repo),
        Some("cache") => return contained_cache(args.get(1).map(String::as_str)),
        Some("selfhost") => (
            "selfhost",
            shell(&cargo_test("-p nazm-cli --test selfhost", profile)),
        ),
        // The reference front end is **built here**, not taken from the image.
        //
        // `compiler/bootstrap.sh` requires `$nazm` to exist and does not build it, and
        // inside the container `CARGO_TARGET_DIR` is `/work/target` — which the image
        // warms with a binary from the day it was built. Until 2026-09-22 that binary was
        // what stage 1 used and what the reference check ran, so a bootstrap could report
        // on a front end months older than the source mounted at `/src` and say nothing
        // about it. The build is incremental against the image's warm cache and costs
        // seconds; the alternative is evidence that silently names the wrong compiler.
        Some("bootstrap") => {
            bounds.deadline = Duration::from_secs(1800);
            (
                "bootstrap",
                shell(&format!(
                    "cargo build --locked -p nazm-cli --jobs {j} \
                     && sh compiler/bootstrap.sh /work/bootstrap"
                )),
            )
        }
        // A scheduled profile runs the same binaries through `xtask workspace-tests`, built
        // here in the same target; any other runs `cargo test --workspace` as it always did.
        // Either way the container's memory peak is reported after the suite (N32-H3).
        Some("tests") => {
            let suite = if profile.test_workers > 1 || profile.name == "p4w1t4" {
                format!(
                    "cargo build --locked -q -p xtask --jobs {j} && /work/target/debug/xtask workspace-tests \
                     --jobs {j} --workers {} --test-threads {} --shape {}",
                    profile.test_workers, profile.test_threads, profile.name
                )
            } else {
                cargo_test("--workspace", profile)
            };
            (
                "tests",
                shell(&format!(
                    "{suite}; s=$?; echo \"contained: memory peak $(cat /sys/fs/cgroup/memory.peak 2>/dev/null) bytes\" >&2; exit $s"
                )),
            )
        }
        // The release candidate: every gate, then the candidate archived and handed out.
        //
        // It has to be exported, because `/work` is the container's writable layer and
        // goes with the container — a candidate assembled there and merely reported on is
        // a candidate nobody has. The archive is deterministic in the ways that are free
        // to make so (sorted entries, fixed owner and mtime), so two runs of the same
        // revision produce the same bytes.
        Some("release") => {
            bounds.deadline = Duration::from_secs(2400);
            let dest = repo.join("target").join("release-candidate");
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/rc/EXPORTS".to_owned(),
                dest: dest.clone(),
                allowed: vec![
                    "nazm-release-candidate.tar.gz".to_owned(),
                    "MANIFEST.txt".to_owned(),
                    "PROVENANCE.txt".to_owned(),
                ],
                // The archive carries a release binary and the whole documentation set.
                // 64 MiB is generous for that and still far below the writable layer's
                // own cap, so a runaway cannot reach the host through this path.
                max_bytes: 64 * 1024 * 1024,
            });
            if let Err(e) = std::fs::create_dir_all(&dest) {
                eprintln!("contained: cannot prepare {}: {e}", dest.display());
                return ExitCode::FAILURE;
            }
            // The revision, established here rather than inside. `git` is not in the
            // image and has no business being there, so the container cannot answer
            // "which commit is this?" — it would record `unknown` and a clean tree it had
            // no way to check. The supervisor mounted the tree and knows both.
            let commit = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default();
            let porcelain = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let Some(dirty) = porcelain else {
                eprintln!(
                    "contained: cannot read the tree's state with git, so the candidate \
                     could not be attributed to a revision"
                );
                return ExitCode::FAILURE;
            };
            if commit.is_empty() {
                eprintln!("contained: cannot establish the source commit");
                return ExitCode::FAILURE;
            }
            let clean = if dirty.is_empty() { "yes" } else { "no" };
            // Only the verdict crosses, not the porcelain text: a changed path may contain
            // a quote, and interpolating it into a shell command is how a filename becomes
            // an instruction.
            let identity = format!("NAZM_SOURCE_COMMIT={commit} NAZM_SOURCE_TREE_CLEAN={clean} ");
            (
                "release",
                shell(
                    &(identity
                        + "sh scripts/release-candidate.sh \
                     && mkdir -p /work/rc \
                     && cp /work/target/release-candidate/MANIFEST.txt /work/rc/ \
                     && cp /work/target/release-candidate/PROVENANCE.txt /work/rc/ \
                     && tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 \
                            -czf /work/rc/nazm-release-candidate.tar.gz \
                            -C /work/target/release-candidate . \
                     && cd /work/rc \
                     && for f in nazm-release-candidate.tar.gz MANIFEST.txt PROVENANCE.txt; do \
                            printf '%s %s\\n' \"$f\" \"$(wc -c < $f | tr -d ' ')\"; \
                        done > EXPORTS \
                     && cat EXPORTS"),
                ),
            )
        }
        Some("mutate") => {
            // The catalogue edits sources, so it needs a tree of its own. `/src` is
            // read-only and stays that way; the copy is disposable and lives on the
            // capped writable layer. `.git` is excluded because nothing here reads it.
            //
            // `.nazm` is excluded too, and the runner is told the copy is disposable so
            // it clears derived caches before every test step (`campaign::reset_derived`
            // says why: a check entry is keyed by the checker's declared identity, which
            // a mutant shares with the pristine checker).
            //
            // One container is one campaign session: one warm worker. One copy, one
            // pristine build, one baseline, then every mutant in turn against the same
            // target directory — at most one mutant injected at a time, each restored and
            // digest-checked before the next. A session that runs out of budget ends
            // *incomplete* and `--resume` continues it from the journal in a new one.
            //
            // The target is `/work/target`, which the image warms with the workspace's
            // registry dependencies: the container's own writable layer, discarded with
            // it, so nothing is shared with the host or with another session. Workspace
            // crates live at `/work/mut`, a path the image's artefacts were never built
            // for, so every one of them is compiled from the copy; only dependencies whose
            // package, version and features are unchanged are reused, which is what
            // Cargo's fingerprints key them on. N32-H: this halved the cold build.
            bounds.deadline = Duration::from_secs(3000);
            let passed = match mutate_passthrough(args.get(1..).unwrap_or_default()) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("contained: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let commit = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .filter(|c| c.chars().all(|ch| ch.is_ascii_hexdigit()))
                .unwrap_or_default();
            let commit = if commit.is_empty() {
                String::new()
            } else {
                format!(" --source-commit {commit}")
            };
            // Budget: stop starting mutants with enough of the deadline left for the one
            // in flight to finish in the ordinary case. Past it, the session is
            // incomplete and resumable, not failed.
            let budget = if passed.contains("--budget-secs")
                || passed.contains("--plan")
                || passed.contains("--verify-killer")
            {
                String::new()
            } else {
                " --budget-secs 2400".to_owned()
            };
            // `timeout` inside the container, so a supervisor that dies without killing
            // it still leaves a container that stops itself. TERM, not KILL: `timeout`
            // then exits 124, which says "the session ran out" in a way an exit of 137
            // cannot — the contained runner reads 137 as a memory kill, and on
            // 2026-09-24 it did, for a session whose only problem was a mutant still in
            // flight at the deadline. `exec` all the way down, so
            // a SIGINT the docker client proxies reaches the runner itself, and when it
            // dies the pid namespace takes every descendant with it.
            let deadline = bounds.deadline.as_secs().saturating_sub(20);
            let par = mutate_parallelism(profile);
            (
                "mutate",
                shell(&format!(
                    "rm -rf /work/mut && mkdir -p /work/mut /work/campaigns \
                     && tar -C /src -cf - --exclude=./target --exclude=./.git --exclude=.nazm . | tar -C /work/mut -xmf - \
                     && cd /work/mut && export CARGO_TARGET_DIR=/work/target \
                     && cargo build -q -p xtask --jobs {j} \
                     && exec timeout -s TERM -k 15 {deadline} /work/target/debug/xtask mutate \
                        --reset-derived-caches --disposable-target --export-journal --campaign-dir /work/campaigns \
                        --resume-from /src/target/mutation-campaigns {par}{commit}{budget}{passed}"
                )),
            )
        }
        Some("bench") => {
            // Results are produced in bounded container storage and exported by name.
            // The checkout stays read-only: a writable bind mount would put every write
            // on the host filesystem, which `--storage-opt` does not bound.
            bounds.deadline = Duration::from_secs(2400);
            let saving = args.iter().any(|a| a == "--save");
            let mode = if saving { "--save" } else { "--check" };
            // The supervisor decides the permitted names, from the mode it was asked for
            // and the platform it measures itself — never from the manifest, which is the
            // workload's own output. A check may export the record and nothing else, so
            // an ordinary check cannot write a baseline at all.
            let slug = match contained::image_platform_slug() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("contained: cannot establish the image's platform: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let mut allowed = vec!["record.json".to_owned()];
            if saving {
                allowed.push(format!("baseline-{slug}.json"));
            }
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/benchout/EXPORTS".to_owned(),
                dest: repo.join("bench"),
                allowed,
                // A record is one JSON object per measurement — a few kilobytes. A
                // megabyte is already far more than it can legitimately be, and far below
                // anything that could fill a disk.
                max_bytes: 1024 * 1024,
            });
            // The slug is computed from the machine that ran the benchmarks, not from
            // this one. A Linux run must name `baseline-linux-aarch64.json` even though
            // the host calling it is Darwin.
            //
            // On a check run the manifest lists only the record, so an ordinary check
            // cannot write a baseline at all. The seeded `baseline.json` is never listed:
            // it is another platform's file, copied in only so the comparison can read it.
            let manifest = if saving {
                r#"slug=$(uname -sm | tr 'A-Z ' 'a-z-' | tr -c 'a-z0-9-' '-' | sed 's/-*$//');                    : > /work/benchout/EXPORTS;                    for f in record.json baseline-$slug.json; do                      [ -f /work/benchout/$f ] && echo "$f $(wc -c < /work/benchout/$f)" >> /work/benchout/EXPORTS;                    done"#
            } else {
                r#": > /work/benchout/EXPORTS;                    [ -f /work/benchout/record.json ] && echo "record.json $(wc -c < /work/benchout/record.json)" >> /work/benchout/EXPORTS"#
            };
            (
                "bench",
                shell(&format!(
                    "mkdir -p /work/benchout \
                     && cp /src/bench/*.json /work/benchout/ 2>/dev/null; \
                     export NAZM_BENCH_DIR=/work/benchout; \
                     cargo build --release --locked -p nazm-cli --jobs 1 >/dev/null \
                     && cargo run -q -p xtask -- bench --runs 5 {mode} \
                     && if find /work/benchout -name '*.json' -size +1M | grep -q .; then \
                          echo 'bench: an artefact exceeded 1 MiB; refusing to export' >&2; exit 3; \
                        fi \
                     && {manifest}"
                )),
            )
        }
        Some("run") => {
            let cmd = args.get(1..).unwrap_or_default().join(" ");
            if cmd.trim().is_empty() {
                eprintln!("contained: `run` needs a command");
                return ExitCode::FAILURE;
            }
            ("run", shell(&cmd))
        }
        other => {
            eprintln!(
                "contained: unknown workload {:?}\n  probe | tests | selfhost | bootstrap | bench | mutate | run <command>",
                other.unwrap_or("<none>")
            );
            return ExitCode::FAILURE;
        }
    };

    // The persistent verification target, for the workloads that build the workspace and
    // are judged on it (N32-H2). A benchmark, a release and an arbitrary `run` never share
    // it: their numbers and artefacts must not depend on what an earlier session left.
    if let Some(namespace) = contained::cache_namespace(name).or(explicit_cache) {
        match contained::cache_ensure(&repo, namespace) {
            Ok(v) => bounds.cache = Some(v),
            Err(e) => {
                eprintln!("contained: cannot prepare the verification cache: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
    eprintln!(
        "contained: {name} — {} MiB memory (swap equal), {:.1} CPU, {} pids, {} GiB disk, {} s deadline",
        bounds.memory_mb,
        f64::from(bounds.milli_cpus) / 1000.0,
        bounds.pids,
        bounds.storage_gb,
        bounds.deadline.as_secs()
    );
    let r = contained::run(&repo, name, &borrowed, &bounds);
    if name == "mutate" {
        collect_journal(&repo, &r.stdout);
    }
    print!("{}", r.stdout);
    eprint!("{}", r.stderr);
    eprintln!(
        "\ncontained: {name} {} in {:.1}s — stdout {} B, stderr {} B, oom_killed={}, survivors={}",
        r.outcome.word(),
        r.elapsed.as_secs_f64(),
        r.stdout_bytes,
        r.stderr_bytes,
        r.oom_killed,
        contained::survivors()
    );
    match r.outcome {
        Outcome::Passed => ExitCode::SUCCESS,
        Outcome::Unavailable(why) => {
            // Explicit refusal. A silent skip would report green on no evidence.
            eprintln!("contained: REFUSED — containment is unavailable: {why}");
            ExitCode::FAILURE
        }
        Outcome::Failed(124) if name == "mutate" => {
            eprintln!(
                "contained: mutate ended INCOMPLETE — the session deadline passed with a \
                 mutant still in flight. That mutant was restored with the container and never \
                 journaled, so it has no verdict and runs again. Continue with \
                 `cargo xtask contained mutate --resume` and the same selection."
            );
            ExitCode::from(3)
        }
        Outcome::Failed(3) if name == "mutate" => {
            eprintln!(
                "contained: mutate ended INCOMPLETE — the session budget ran out before every \
                 selected mutant was finalized. Nothing about the rest is established. \
                 Continue with `cargo xtask contained mutate --resume` and the same selection."
            );
            ExitCode::from(3)
        }
        other => {
            eprintln!("contained: {name} did not pass ({})", other.word());
            ExitCode::FAILURE
        }
    }
}

/// The profile a workload runs under when `--profile` is not given. N14.1.
///
/// The workspace suite and the selfhost suite take P4T4, which their calibration qualified
/// (`docs/performance.md`). Everything else is P1. `mutate` is P1 because every parallel
/// profile tried crossed the frozen total-memory gate on it, not because its verdicts
/// differed. `bench` is P1 for good: it measures the programs and the compiler, and a
/// measurement shares the machine with nothing. `--profile p1` reproduces any older
/// result.
fn default_profile(workload: &str) -> contained::ExecutionProfile {
    match workload {
        "tests" | "selfhost" => contained::ExecutionProfile::P4T4,
        _ => contained::ExecutionProfile::P1,
    }
}

/// A contained `cargo test` over `scope`, with the profile's job and thread counts.
fn cargo_test(scope: &str, p: contained::ExecutionProfile) -> String {
    format!(
        "cargo test --locked {scope} --jobs {} -- --test-threads {}",
        p.cargo_jobs, p.test_threads
    )
}

/// The parallelism a contained campaign passes to its runner. It sizes each step; the
/// runner still takes one mutant at a time.
fn mutate_parallelism(p: contained::ExecutionProfile) -> String {
    format!(
        "--cargo-jobs {} --test-threads {}",
        p.cargo_jobs, p.test_threads
    )
}

/// The options `contained mutate` passes through, validated and quoted.
///
/// Rebuilt from a closed list rather than forwarded. Every value is interpolated into a
/// shell command, so each is restricted to the characters a mutation name, a test path
/// or a number uses, and single-quoted as well.
fn mutate_passthrough(args: &[String]) -> Result<String, String> {
    const WITH_VALUE: &[&str] = &[
        "--only",
        "--strategy",
        "--budget-secs",
        "--verify-killer",
        "--killer",
    ];
    const SWITCHES: &[&str] = &["--plan", "--resume", "--verify-killers"];
    let safe = |v: &str| {
        !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '=' | '.'))
    };
    let mut out = String::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if SWITCHES.contains(&a.as_str()) {
            out.push_str(&format!(" {a}"));
            i += 1;
        } else if WITH_VALUE.contains(&a.as_str()) {
            let Some(v) = args.get(i + 1) else {
                return Err(format!("{a} needs a value"));
            };
            if !safe(v) {
                return Err(format!(
                    "{a} {v:?}: only letters, digits and - _ : = . are accepted"
                ));
            }
            out.push_str(&format!(" {a} '{v}'"));
            i += 2;
        } else {
            return Err(format!("mutate: unknown option {a}"));
        }
    }
    Ok(out)
}

/// Append the journal lines a contained campaign printed to the host's copy, under
/// `target/mutation-campaigns/<key>/journal.jsonl`.
///
/// The container's own journal dies with it. What it printed is the only record that
/// crosses, so it is collected whatever the outcome, a timeout included: every line is
/// a mutant that was restored and verified before it was printed, and a later session
/// resumes from exactly these.
fn collect_journal(repo: &std::path::Path, stdout: &str) {
    use std::io::Write as _;
    let mut by_key: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix(campaign::EXPORT_PREFIX) {
            if let Some((key, json)) = rest.split_once(' ') {
                if key.len() == 16 && key.chars().all(|c| c.is_ascii_hexdigit()) {
                    by_key.entry(key).or_default().push(json);
                }
            }
        }
    }
    for (key, lines) in by_key {
        let dir = repo.join("target").join("mutation-campaigns").join(key);
        if std::fs::create_dir_all(&dir).is_err() {
            eprintln!("contained: cannot create {}", dir.display());
            continue;
        }
        let path = dir.join("journal.jsonl");
        let fresh = !path.exists();
        let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        else {
            eprintln!("contained: cannot open {}", path.display());
            continue;
        };
        let mut n = 0;
        for (i, l) in lines.iter().enumerate() {
            // The header opens a fresh file and is otherwise already there.
            if i == 0 && !fresh && l.contains("\"kind\":\"campaign\"") {
                continue;
            }
            if writeln!(f, "{l}").is_ok() {
                n += 1;
            }
        }
        let _ = f.sync_all();
        eprintln!(
            "contained: {n} journal line(s) for campaign {key} kept at {}",
            path.display()
        );
    }
}

/// `sh -c`, not `sh -lc`: a login shell re-reads `/etc/profile` and drops the image's
/// PATH, which is where the pinned toolchain lives.
fn shell(cmd: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("cd /src && {cmd}"),
    ]
}

/// `xtask workspace-tests --jobs J --workers W --test-threads T --shape NAME`: every test
/// binary `cargo test --workspace` would run, built by Cargo and named by its JSON messages,
/// each run once over W workers, longest first by the timing history for NAME; then the
/// doc-tests. The report is in canonical order and a failure cancels nothing. N32-H3.
fn workspace_tests(rest: &[String]) -> ExitCode {
    let value = |flag: &str, default: &str| -> String {
        rest.iter()
            .position(|a| a == flag)
            .and_then(|i| rest.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let num = |flag: &str, default: u32| {
        value(flag, &default.to_string())
            .parse::<u32>()
            .unwrap_or(default)
    };
    let (jobs, workers, threads) = (
        num("--jobs", 1),
        num("--workers", 1),
        num("--test-threads", 1),
    );
    let shape = value("--shape", "default");
    let Some(root) = workspace_root() else {
        eprintln!("workspace-tests: could not locate the workspace root");
        return ExitCode::FAILURE;
    };
    let started = std::time::Instant::now();
    let build = procs::run(&procs::Invocation {
        program: campaign::cargo(),
        args: [
            "test",
            "--locked",
            "--workspace",
            "--no-run",
            "--message-format=json",
            "--jobs",
            &jobs.to_string(),
        ]
        .map(str::to_owned)
        .to_vec(),
        cwd: root.clone(),
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(2400),
    });
    if !matches!(&build.ended, procs::Ended::Exited(s) if s.success()) {
        eprint!("{}", build.stderr);
        eprintln!(
            "workspace-tests: the suite did not build ({:?})",
            build.ended
        );
        return ExitCode::FAILURE;
    }
    let built = started.elapsed().as_secs_f64();
    let bins = suite::binaries(&build.stdout);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), std::path::PathBuf::from);
    let history_file = suite::history_path(&target);
    let history = suite::read_history(&history_file, &shape);
    let order = suite::schedule(&bins, &history);
    let t0 = std::time::Instant::now();
    let results = suite::execute(&bins, &order, workers as usize, |b| {
        suite::run_binary(b, threads, std::time::Duration::from_secs(1800))
    });
    let ran = t0.elapsed().as_secs_f64();

    let mut failed = Vec::new();
    let mut now = std::collections::BTreeMap::new();
    for (b, r) in bins.iter().zip(&results) {
        eprintln!("     Running {} ({})", b.id(), b.executable.display());
        match r {
            Some(r) => {
                print!("{}", r.stdout);
                eprint!("{}", r.stderr);
                now.insert(b.id(), r.secs);
                if !r.passed() {
                    failed.push(format!("{} ({:?})", b.id(), r.ended));
                }
            }
            None => failed.push(format!("{} (did not run)", b.id())),
        }
    }
    suite::write_history(&history_file, &shape, &now);
    let doc = procs::run(&procs::Invocation {
        program: campaign::cargo(),
        args: vec![
            "test".to_owned(),
            "--locked".to_owned(),
            "--workspace".to_owned(),
            "--doc".to_owned(),
            "--jobs".to_owned(),
            jobs.to_string(),
            "--".to_owned(),
            "--test-threads".to_owned(),
            threads.to_string(),
        ],
        cwd: root,
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(1200),
    });
    print!("{}", doc.stdout);
    eprint!("{}", doc.stderr);
    if !matches!(&doc.ended, procs::Ended::Exited(s) if s.success()) {
        failed.push(format!("doc-tests ({:?})", doc.ended));
    }
    let mut slowest: Vec<(&String, &f64)> = now.iter().collect();
    slowest.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
    eprintln!(
        "workspace-tests: {} binaries over {workers} worker(s) × {threads} thread(s): build {built:.1}s, run {ran:.1}s, doc-tests {:.1}s; history {}",
        bins.len(),
        doc.elapsed.as_secs_f64(),
        if history.is_empty() { "none (canonical order)" } else { "used" }
    );
    for (id, secs) in slowest.iter().take(10) {
        eprintln!("workspace-tests: slowest {secs:7.2}s {id}");
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        for f in &failed {
            eprintln!("workspace-tests: FAILED {f}");
        }
        ExitCode::FAILURE
    }
}

/// `contained cache` lists the harness-owned verification caches; `contained cache clear`
/// removes them. Nothing that is not labelled as the harness's is touched. N32-H2.
fn contained_cache(what: Option<&str>) -> ExitCode {
    match what {
        None | Some("list") => {
            let owned = contained::cache_list();
            if owned.is_empty() {
                println!("no verification cache");
            }
            for v in owned {
                println!(
                    "{}  namespace {}  key {}  created {}",
                    v.name, v.namespace, v.key, v.created
                );
            }
            ExitCode::SUCCESS
        }
        Some("clear") => {
            for n in contained::cache_clear() {
                println!("removed {n}");
            }
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("contained cache: `list` or `clear`, not `{other}`");
            ExitCode::FAILURE
        }
    }
}

/// Verifies that each limit is real on this daemon before anything depends on it. Every
/// probe's own maximum is small, so it stays safe even if enforcement silently fails.
fn contained_probe(repo: &std::path::Path) -> ExitCode {
    use contained::{Bounds, Outcome};
    use std::time::Duration;

    let tight = Bounds {
        memory_mb: 256,
        milli_cpus: 1000,
        profile: contained::ExecutionProfile::P1,
        pids: 64,
        storage_gb: 1,
        scratch_mb: 16,
        fsize_bytes: 4 * 1024 * 1024,
        deadline: Duration::from_secs(20),
        output_cap: 256 * 1024,
        export: None,
        cache: None,
    };

    // (name, argv, expected outcome, what it proves)
    let cases: Vec<(&str, Vec<String>, Outcome, &str)> = vec![
        (
            "limits-visible",
            shell("cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.swap.max /sys/fs/cgroup/pids.max"),
            Outcome::Passed,
            "the cgroup reports the caps that were asked for",
        ),
        (
            "memory",
            // Allocates in chunks and memsets every one. A shell pipeline would only
            // stream through small buffers and prove nothing; reserving address space
            // without touching it would prove nothing either. 400 MiB against a 256 MiB
            // cap is small enough to stay safe on the host even if enforcement fails.
            shell(
                "printf '%s' '#include <stdlib.h>\n#include <string.h>\n#include <stdio.h>\nint main(void){for(long i=0;i<50;i++){char*p=malloc(8L<<20);if(!p){printf(\"MALLOC_FAILED %ld\\n\",i);return 3;}memset(p,(int)(i&0xff),8L<<20);}printf(\"COMPLETED\\n\");return 0;}' > /scratch/m.c && clang -O0 -o /scratch/m /scratch/m.c && /scratch/m",
            ),
            Outcome::MemoryLimit,
            "allocation past the cap is refused, not merely reclaimed",
        ),
        (
            "deadline",
            shell("sleep 120"),
            Outcome::Timeout,
            "a workload outliving its deadline is killed at the container",
        ),
        (
            "output",
            shell("i=0; while [ $i -lt 400 ]; do head -c 65536 /dev/zero | tr '\\0' 'x'; i=$((i+1)); done"),
            Outcome::OutputOverflow,
            "a flood is drained and capped, not buffered",
        ),
        (
            "scratch",
            shell("dd if=/dev/zero of=/scratch/f bs=1M count=64 2>&1 | tail -1; test ! -s /scratch/f || test $(stat -c%s /scratch/f) -lt 67108864"),
            Outcome::Passed,
            "the tmpfs refuses to grow past its size",
        ),
        (
            "descendant",
            shell("(sleep 120 &) ; echo detached; sleep 120"),
            Outcome::Timeout,
            "a detached grandchild dies with the container",
        ),
    ];

    let mut failures = 0;
    println!("contained probe — verifying each bound is real before anything relies on it\n");
    for (name, argv, want, proves) in cases {
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let r = contained::run(repo, &format!("probe-{name}"), &borrowed, &tight);
        if let Outcome::Unavailable(why) = &r.outcome {
            eprintln!("REFUSED — containment is unavailable: {why}");
            return ExitCode::FAILURE;
        }
        let ok = r.outcome == want;
        if !ok {
            failures += 1;
        }
        println!(
            "  {:<11} {:<16} expected {:<16} {:.1}s  {}",
            if ok { "ok" } else { "MISMATCH" },
            name,
            want.word(),
            r.elapsed.as_secs_f64(),
            proves
        );
        if !ok {
            println!(
                "      got {} (stdout {} B, stderr {} B)",
                r.outcome.word(),
                r.stdout_bytes,
                r.stderr_bytes
            );
        }
    }
    let left = contained::survivors();
    println!("\n  survivors after all probes: {left}");
    if failures == 0 && left == 0 {
        println!("  containment verified on this daemon");
        ExitCode::SUCCESS
    } else {
        eprintln!("  containment NOT verified: {failures} mismatched, {left} survivors");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::{
        absence_claims, cargo_test, check_core_ir_boundary, cited_paths,
        contained::ExecutionProfile, default_profile, mutate_parallelism, serde_in_manifest,
        serde_in_source, verified_without_evidence,
    };

    #[test]
    fn nothing_below_core_ir_asks_what_a_source_position_meant() {
        // The gate, over this tree, as a test: so a lookup coming back into the backend or
        // the interpreter fails the suite, not only `cargo xtask check` (N39).
        assert_eq!(check_core_ir_boundary(), Ok(()));
    }

    #[test]
    fn only_the_calibrated_workloads_leave_p1() {
        assert_eq!(default_profile("tests"), ExecutionProfile::P4T4);
        assert_eq!(default_profile("selfhost"), ExecutionProfile::P4T4);
        for w in [
            "mutate",
            "bootstrap",
            "bench",
            "release",
            "probe",
            "run",
            "",
        ] {
            assert_eq!(default_profile(w), ExecutionProfile::P1, "{w}");
        }
    }

    #[test]
    fn p1_is_still_one_cpu_one_job_one_thread() {
        let p1 = ExecutionProfile::P1;
        assert_eq!(
            (p1.milli_cpus, p1.cargo_jobs, p1.test_threads),
            (1000, 1, 1)
        );
        assert_eq!(ExecutionProfile::by_name("p1"), Some(p1));
        assert_eq!(ExecutionProfile::by_name("p6"), None);
        assert_eq!(
            cargo_test("--workspace", p1),
            "cargo test --locked --workspace --jobs 1 -- --test-threads 1"
        );
        assert_eq!(mutate_parallelism(p1), "--cargo-jobs 1 --test-threads 1");
    }

    #[test]
    fn a_profile_reaches_every_step_it_sizes() {
        let p = ExecutionProfile::P4T4;
        assert_eq!(
            cargo_test("-p nazm-cli --test selfhost", p),
            "cargo test --locked -p nazm-cli --test selfhost --jobs 4 -- --test-threads 4"
        );
        assert_eq!(mutate_parallelism(p), "--cargo-jobs 4 --test-threads 4");
    }

    #[test]
    fn a_path_is_cited_only_when_it_is_rooted_in_the_repository() {
        assert_eq!(
            cited_paths("see `crates/nazm-lir/src/ir.rs` and `docs/spec.md`"),
            ["crates/nazm-lir/src/ir.rs", "docs/spec.md"]
        );
        // Prose in backticks is not a path, which is the whole reason for the anchor list.
        assert!(cited_paths("`N0101`, `nazm build`, `Ints`, `let mut`").is_empty());
        // A leading `./` or a bare filename is not anchored, and is deliberately ignored
        // rather than guessed at.
        assert!(cited_paths("`./build.rs` and `Cargo.toml`").is_empty());
    }

    #[test]
    fn a_line_reference_is_not_part_of_the_path() {
        assert_eq!(cited_paths("`docs/spec.md:761-822`"), ["docs/spec.md"]);
        assert_eq!(
            cited_paths("`xtask/src/main.rs:292`"),
            ["xtask/src/main.rs"]
        );
    }

    #[test]
    fn a_verified_section_must_cite_something() {
        let ok = "### 11. LLVM backend — VERIFIED\n\nSee `crates/nazm-lir/src/emit.rs`.\n";
        assert!(verified_without_evidence(ok).is_empty());

        let bare = "### 11. LLVM backend — VERIFIED\n\nIt works, trust me.\n";
        assert_eq!(
            verified_without_evidence(bare),
            ["11. LLVM backend — VERIFIED"]
        );
    }

    #[test]
    fn only_verified_sections_are_required_to_cite() {
        // MISSING is the case where there is nothing to point at, and demanding a path
        // would push the document toward citing files to prove an absence.
        let text = "### 5. Effects — MISSING\n\nNothing exists.\n";
        assert!(verified_without_evidence(text).is_empty());
    }

    #[test]
    fn a_sections_evidence_does_not_leak_into_the_next() {
        let text = "### A — VERIFIED\n\n`docs/spec.md`\n\n### B — VERIFIED\n\nnothing\n";
        assert_eq!(verified_without_evidence(text), ["B — VERIFIED"]);
    }

    #[test]
    fn the_absence_claim_is_matched_next_to_the_noun() {
        assert_eq!(
            absence_claims("it is an interpreter: no IR and no backend"),
            [1]
        );
        assert_eq!(absence_claims("x\nno IR, no backend, no runtime\ny"), [2]);
        assert_eq!(
            absence_claims("Phase 2 is bench only — there is no compiler yet"),
            [1]
        );
        // The sentence this must not break: there is no *second* backend, and saying so
        // is correct.
        assert!(absence_claims("Cranelift is planned; there is no second backend yet").is_empty());
        assert!(absence_claims("the LIR carries no effect rows").is_empty());
    }

    #[test]
    fn a_quoted_claim_is_a_quotation_not_an_assertion() {
        // How this repository corrects itself: quote the old sentence, then say what
        // replaced it. The gate must read that as history, not as a live claim.
        let corrected = "This section read \"Phase 2 is bench only — there is no compiler \
                         yet, deliberately\", and that is no longer the case.";
        assert!(absence_claims(corrected).is_empty());

        // Across a line break, which is how the real correction block is wrapped.
        let wrapped = "It read \"there is\nno compiler yet\", and it no longer does.\n";
        assert!(absence_claims(wrapped).is_empty());

        // A new paragraph resets the state, so one stray quotation mark cannot silence
        // the rest of a document.
        let stray = "a \"dangling quote\n\nno IR and no backend\n";
        assert_eq!(absence_claims(stray), [3]);
    }

    #[test]
    fn the_claim_is_caught_even_when_the_sentence_wraps() {
        // Exactly how `Cargo.toml` carried it: split mid-phrase, across two `#` comments.
        let wrapped = "# an interpreter: lexer, parser, checker, no IR and no\n\
                       # backend. Alongside it, the harness.\n";
        assert_eq!(absence_claims(wrapped), [1]);
    }

    #[test]
    fn a_serde_dependency_in_the_span_manifest_is_a_finding() {
        assert!(serde_in_manifest("nazm-span", "[dependencies]\n", "why").is_empty());
        // The line the correction of 2026-09-22 removed.
        assert_eq!(
            serde_in_manifest(
                "nazm-span",
                "[dependencies]\nserde = { workspace = true }\n",
                "why"
            )
            .len(),
            1
        );
        // Commented out is not a dependency, and the comment explaining the absence has
        // to be allowed to name it.
        assert!(
            serde_in_manifest("nazm-sema", "# serde = { workspace = true }\n", "why").is_empty()
        );
        // A different crate whose name merely starts the same way is not serde.
        assert!(
            serde_in_manifest("nazm-span", "serde_json = { workspace = true }\n", "why").is_empty()
        );
    }

    #[test]
    fn a_serde_derive_in_the_span_source_is_a_finding() {
        assert!(serde_in_source("pub struct Span { pub file: FileId }\n").is_empty());
        assert_eq!(
            serde_in_source("#[derive(Debug, Serialize)]\npub struct Span;\n"),
            [(1, "derives a serde trait")]
        );
        assert_eq!(
            serde_in_source("impl serde::Serializer for X {}\n").len(),
            1
        );
        // The crate documentation says why serde is absent, at length. Explaining a rule
        // must not break it.
        let doc = "//! Between 2026-09-21 and 2026-09-22 `Span` carried a `Serialize`\n                   //! that dropped the file. This crate no longer depends on serde.\n";
        assert!(serde_in_source(doc).is_empty());
    }
}
