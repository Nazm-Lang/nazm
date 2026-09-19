//! Repository automation.
//!
//! `cargo xtask check` runs the architectural gates. They are security and design
//! controls, not style checks: clippy and rustfmt cover style, and this covers the claims
//! the project makes about its own structure that no linter can see.

mod metadata;
mod rules;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => run_checks(),
        Some("check-layering") => report(&[("layering", check_layering())]),
        Some("check-codegen") => report(&[("codegen reach", check_codegen_reach())]),
        Some("check-unsafe") => report(&[("unsafe policy", check_unsafe_policy())]),
        Some("check-docs") => report(&[("doc claims", check_doc_claims())]),
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
           check-codegen   no front-end crate reaches a code generator\n  \
           check-unsafe    the unsafe allowlist and the workspace lint agree\n  \
           check-docs      documented claims that must stay true\n  \
           help            this message\n"
    );
}

fn run_checks() -> ExitCode {
    report(&[
        ("layering", check_layering()),
        ("codegen reach", check_codegen_reach()),
        ("unsafe policy", check_unsafe_policy()),
        ("doc claims", check_doc_claims()),
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

/// No crate may reach a code generator unless `rules.rs` says it can.
///
/// The rule exists before either backend does, because it is cheap to state now and
/// contentious once someone has a reason to break it. `docs/architecture.md` §1 puts
/// Cranelift and LLVM behind the LIR contract; this is what keeps that true.
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
