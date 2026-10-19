//! Reach gates: which crate may depend on, emit or call what — layering, the tokenizer, LLVM
//! text emission, the network, the code generators, and `unsafe`.

use crate::checks::collect_rs;
use crate::{metadata, rules};

/// Every workspace member must be placed in the layering, and may depend only on what
/// `rules.rs` allows.
pub(crate) fn check_layering() -> Result<(), Vec<String>> {
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
pub(crate) fn check_tokenizer_reach() -> Result<(), Vec<String>> {
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
pub(crate) fn check_emission_reach() -> Result<(), Vec<String>> {
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
///
/// One file is the exception, and only for the transport: the interpreter's network module, which
/// opens a socket only when the program it runs holds a `NetCap` and asks (Gate 2, `docs/user/spec.md`,
/// *Networking*). No provider's API is allowed even there, and nothing else of the compiler, the
/// checker or the tools may name the transport.
pub(crate) fn check_network_reach() -> Result<(), Vec<String>> {
    const PROGRAM_NETWORK: &str = "crates/nazm-core/src/eval/net.rs";
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
            let rel = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            for mark in MARKS {
                if *mark == "TcpStream" && rel == PROGRAM_NETWORK {
                    continue;
                }
                if text.contains(mark) {
                    problems.push(format!(
                        "{rel} mentions `{mark}`: only an evaluator may reach a network, and \
                         only {PROGRAM_NETWORK} a program's own sockets"
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

pub(crate) fn check_codegen_reach() -> Result<(), Vec<String>> {
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
pub(crate) fn check_unsafe_policy() -> Result<(), Vec<String>> {
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
