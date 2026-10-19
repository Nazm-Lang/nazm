//! Architecture gates: one resolver, nothing below Core IR asking the checker, spans and
//! semantic ids that stay in their session, and the crates that may not serialise.

use crate::checks::{module_files, source_files, workspace_root};

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
pub(crate) fn check_single_resolver() -> Result<(), Vec<String>> {
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

/// Nothing below Core IR asks what a source position meant (N39, `docs/internal/architecture.md`
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
pub(crate) fn check_core_ir_boundary() -> Result<(), Vec<String>> {
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
    // The interpreter is a module: `eval.rs`, or every file of `eval/` once it is split.
    const INTERPRETER: &str = "crates/nazm-core/src/eval";
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
    let interpreter = module_files(&root, INTERPRETER);
    if interpreter.is_empty() {
        return Err(vec![format!(
            "{INTERPRETER}: neither a file nor a module directory"
        )]);
    }
    files.extend(interpreter.iter().cloned());
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
                     second lowering (docs/internal/architecture.md §7.41)",
                    at(i)
                ));
            }
        }
        if rel.starts_with(BACKEND) {
            for word in BACKEND_MAY_NOT_NAME {
                for (i, _) in flat.match_indices(word) {
                    problems.push(format!(
                        "{rel}:{} names `{word}`: the native backend reads validated MIR and \
                         nothing of the checker or of Core IR (docs/internal/architecture.md §7.42)",
                        at(i)
                    ));
                }
            }
        }
        if interpreter.contains(rel) {
            for word in INTERPRETER_MAY_NOT_NAME {
                for (i, _) in flat.match_indices(word) {
                    problems.push(format!(
                        "{rel}:{} names `{word}`: the interpreter runs Core IR and nothing \
                         else (docs/internal/architecture.md §7.41)",
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
    // A module: `check.rs`, or any file of `check/` once it is split.
    const CALLER: &str = "crates/nazm-core/src/check";

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
        if !module_files(root, CALLER).contains(file) {
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
pub(crate) fn check_span_identity() -> Result<(), Vec<String>> {
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
pub(crate) fn check_sema_purity() -> Result<(), Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_below_core_ir_asks_what_a_source_position_meant() {
        // The gate, over this tree, as a test: so a lookup coming back into the backend or
        // the interpreter fails the suite, not only `cargo xtask check` (N39).
        assert_eq!(check_core_ir_boundary(), Ok(()));
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
