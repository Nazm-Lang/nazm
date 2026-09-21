//! Repository automation.
//!
//! `cargo xtask check` runs the architectural gates. They are security and design
//! controls, not style checks: clippy and rustfmt cover style, and this covers the claims
//! the project makes about its own structure that no linter can see.

mod bench;
mod guide;
mod metadata;
mod mutate;
mod rules;

use std::process::ExitCode;

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

fn check_doc_claims() -> Result<(), Vec<String>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf);
    let Some(root) = root else {
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
