//! Repository automation.
//!
//! `cargo xtask check` runs the architectural gates. They are security and design
//! controls, not style checks: clippy and rustfmt cover style, and this covers the claims
//! the project makes about its own structure that no linter can see.

mod bench;
mod contained;
mod guide;
mod metadata;
mod mutate;
mod rules;

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
        Some("check-guide") => report(&[("guide from grammar", guide::check())]),
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
            // `--only NAME` selects by substring, repeatably. Nothing else is accepted:
            // a typo that silently ran the whole catalogue, or none of it, would waste
            // hours either way.
            let rest: Vec<String> = args.collect();
            let mut only = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                match rest.get(i).map(String::as_str) {
                    Some("--only") => match rest.get(i + 1) {
                        Some(name) => {
                            only.push(name.clone());
                            i += 2;
                        }
                        None => {
                            eprintln!("mutate: --only needs a mutation name");
                            return ExitCode::FAILURE;
                        }
                    },
                    Some(other) => {
                        eprintln!("mutate: unknown option {other}");
                        return ExitCode::FAILURE;
                    }
                    None => break,
                }
            }
            report(&[("mutation coverage", run_mutations(&only))])
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
           check-guide     docs/guide.md is what docs/grammar.ebnf produces\n  \
           guide           rewrite docs/guide.md from docs/grammar.ebnf\n  \
           bench           measure; --save records a baseline, --check compares to one,\n  \
                           --runs N sets the repetitions (default 5)\n  \
           mutate          inject catalogued defects; each must be caught.\n  \
                           --only NAME selects by substring, repeatably, because each\n  \
                           entry costs a full build and test run\n  \
           mutate-recover  repair a tree an interrupted mutation run left mutated;\n  \
                           needs --confirm-no-writers, and read what it prints first\n  \
           help            this message\n"
    );
}

fn run_checks() -> ExitCode {
    report(&[
        ("layering", check_layering()),
        ("codegen reach", check_codegen_reach()),
        ("emission reach", check_emission_reach()),
        ("unsafe policy", check_unsafe_policy()),
        ("doc claims", check_doc_claims()),
        ("capability matrix", check_capability_matrix()),
        ("guide from grammar", guide::check()),
    ])
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
/// no-ops while `docs/evaluation-preregistration.md` does not exist. A reader seeing
/// `ok doc claims` should not take it for more than that.
///
/// The language's documented claims are checked by `crates/nazm-cli/tests/docs.rs`,
/// which can run the binary and read the emitter's own constants — neither of which
/// `xtask` may do, since it depends on no internal crate. It is a test rather than a
/// gate for that reason, and `cargo test --workspace` runs it.
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
    let prereg = root.join("docs/evaluation-preregistration.md");
    let readme = match std::fs::read_to_string(root.join("README.md")) {
        Ok(s) => s,
        Err(e) => return Err(vec![format!("could not read README.md: {e}")]),
    };

    let readme_says_not_registered = readme.contains("decision rule is not pre-registered")
        || readme.contains("not binding before then");

    if prereg.exists() && readme_says_not_registered {
        problems.push(
            "docs/evaluation-preregistration.md exists, but README.md still says the \
             decision rule is not pre-registered. One of them is wrong."
                .to_owned(),
        );
    }

    // The reverse: the evaluation protocol must keep its own status honest.
    match std::fs::read_to_string(root.join("docs/evaluation.md")) {
        Ok(eval) => {
            if prereg.exists() && eval.contains("Not yet pre-registered") {
                problems.push(
                    "docs/evaluation.md still carries its draft banner although a \
                     pre-registration exists."
                        .to_owned(),
                );
            }
        }
        Err(e) => problems.push(format!("could not read docs/evaluation.md: {e}")),
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
    // filter here.
    match mutate::run_all(
        std::path::Path::new(&root),
        std::path::Path::new(&catalogue),
        &[],
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problems) => {
            for p in problems {
                println!("problem: {p}");
            }
            ExitCode::FAILURE
        }
    }
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

fn run_mutations(only: &[String]) -> Result<(), Vec<String>> {
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let catalogue = root.join("xtask/mutations/mutations.toml");
    mutate::run_all(root, &catalogue, only)
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

    let (name, argv): (&str, Vec<String>) = match args.first().map(String::as_str) {
        Some("probe") => return contained_probe(&repo),
        Some("selfhost") => (
            "selfhost",
            shell("cargo test --locked -p nazm-cli --test selfhost --jobs 1 -- --test-threads 1"),
        ),
        Some("bootstrap") => {
            bounds.deadline = Duration::from_secs(1800);
            (
                "bootstrap",
                shell("sh compiler/bootstrap.sh /work/bootstrap"),
            )
        }
        Some("tests") => (
            "tests",
            shell("cargo test --locked --workspace --jobs 1 -- --test-threads 1"),
        ),
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
            bounds.deadline = Duration::from_secs(3000);
            let only: String = args.iter().skip(1).map(|a| format!(" {a}")).collect();
            (
                "mutate",
                shell(&format!(
                    "rm -rf /work/mut && mkdir -p /work/mut \
                     && tar -C /src -cf - --exclude=./target --exclude=./.git . | tar -C /work/mut -xf - \
                     && cd /work/mut && export CARGO_TARGET_DIR=/work/mut/target \
                     && cargo run -q -p xtask -- mutate{only}"
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
        other => {
            eprintln!("contained: {name} did not pass ({})", other.word());
            ExitCode::FAILURE
        }
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

/// Verifies that each limit is real on this daemon before anything depends on it. Every
/// probe's own maximum is small, so it stays safe even if enforcement silently fails.
fn contained_probe(repo: &std::path::Path) -> ExitCode {
    use contained::{Bounds, Outcome};
    use std::time::Duration;

    let tight = Bounds {
        memory_mb: 256,
        milli_cpus: 1000,
        pids: 64,
        storage_gb: 1,
        scratch_mb: 16,
        fsize_bytes: 4 * 1024 * 1024,
        deadline: Duration::from_secs(20),
        output_cap: 256 * 1024,
        export: None,
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
    use super::{absence_claims, cited_paths, verified_without_evidence};

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
}
