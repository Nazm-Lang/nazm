//! Documentation gates: the benchmark harness's claims, the capability matrix's evidence, and
//! the documentation corpus.

use crate::checks::workspace_root;

/// The documentation corpus `nazm docs` and `nazm-mcp` answer from sections cleanly: every
/// allowlisted document exists and is UTF-8, every section id is unique — so no anchor, and
/// no rule id an anchor carries, is defined twice — every in-corpus link and every generated
/// guide rule resolves, and the grammar parses into productions whose references are all
/// defined. Read with `nazm-docs` itself, so the gate and the tools cannot disagree about
/// what the corpus is. N32.
pub(crate) fn check_docs_corpus() -> Result<(), Vec<String>> {
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

/// Documented claims that must stay true — **about the benchmark harness only**.
///
/// Named "doc claims", which over-promises: both checks below concern whether the
/// evaluation protocol's pre-registration status is stated consistently, and both are
/// no-ops while `docs/internal/research/evaluation-preregistration.md` does not exist. A reader seeing
/// `ok doc claims` should not take it for more than that.
///
/// The language's documented claims are checked by `crates/nazm-cli/tests/docs.rs`,
/// which can run the binary and read the emitter's own constants — neither of which
/// `xtask` may do, since it depends on no compiler crate (its one internal dependency is
/// `nazm-docs`, for the corpus gate). It is a test rather than a gate for that reason, and
/// `cargo test --workspace` runs it.
pub(crate) fn check_doc_claims() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let mut problems = Vec::new();
    let prereg = root.join("docs/internal/research/evaluation-preregistration.md");
    let readme = match std::fs::read_to_string(root.join("README.md")) {
        Ok(s) => s,
        Err(e) => return Err(vec![format!("could not read README.md: {e}")]),
    };

    let readme_says_not_registered = readme.contains("decision rule is not pre-registered")
        || readme.contains("not binding before then");

    if prereg.exists() && readme_says_not_registered {
        problems.push(
            "docs/internal/research/evaluation-preregistration.md exists, but README.md still says the \
             decision rule is not pre-registered. One of them is wrong."
                .to_owned(),
        );
    }

    // The reverse: the evaluation protocol must keep its own status honest.
    match std::fs::read_to_string(root.join("docs/internal/research/evaluation.md")) {
        Ok(eval) => {
            if prereg.exists() && eval.contains("Not yet pre-registered") {
                problems.push(
                    "docs/internal/research/evaluation.md still carries its draft banner although a \
                     pre-registration exists."
                        .to_owned(),
                );
            }
        }
        Err(e) => problems.push(format!(
            "could not read docs/internal/research/evaluation.md: {e}"
        )),
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
            "docs/internal/architecture.md",
            "docs/user/roadmap.md",
            "docs/internal/master-architecture.md",
            "docs/internal/capability-matrix.md",
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
pub(crate) fn check_capability_matrix() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };

    let text = match std::fs::read_to_string(root.join("docs/internal/capability-matrix.md")) {
        Ok(s) => s,
        Err(e) => {
            return Err(vec![format!(
                "could not read docs/internal/capability-matrix.md: {e}"
            )])
        }
    };

    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for path in cited_paths(line) {
            if !root.join(path).exists() {
                problems.push(format!(
                    "docs/internal/capability-matrix.md:{} cites `{path}`, which does not exist",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_cited_only_when_it_is_rooted_in_the_repository() {
        assert_eq!(
            cited_paths("see `crates/nazm-lir/src/ir.rs` and `docs/user/spec.md`"),
            ["crates/nazm-lir/src/ir.rs", "docs/user/spec.md"]
        );
        // Prose in backticks is not a path, which is the whole reason for the anchor list.
        assert!(cited_paths("`N0101`, `nazm build`, `Ints`, `let mut`").is_empty());
        // A leading `./` or a bare filename is not anchored, and is deliberately ignored
        // rather than guessed at.
        assert!(cited_paths("`./build.rs` and `Cargo.toml`").is_empty());
    }

    #[test]
    fn a_line_reference_is_not_part_of_the_path() {
        assert_eq!(
            cited_paths("`docs/user/spec.md:761-822`"),
            ["docs/user/spec.md"]
        );
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
        let text = "### A — VERIFIED\n\n`docs/user/spec.md`\n\n### B — VERIFIED\n\nnothing\n";
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
