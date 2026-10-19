//! Cache gates: what a native object's identity covers, what a check key covers, and how the
//! object cache stores and refuses.

use crate::checks::{module_files, source_files, workspace_root};

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
/// with no durable identity gets a compilation-local symbol, which `docs/internal/architecture.md`
/// records as an honest limitation rather than a durable-looking guess.
pub(crate) fn check_native_identity() -> Result<(), Vec<String>> {
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
/// and with an explanation that sounds right. `docs/internal/architecture.md` §3 names it as the
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
pub(crate) fn check_cache_soundness() -> Result<(), Vec<String>> {
    let Some(root) = workspace_root() else {
        return Err(vec!["could not locate workspace root".to_owned()]);
    };
    let mut problems = Vec::new();

    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    // A module's text: `name.rs`, or every file of `name/` once it is split; `None` if
    // neither exists or a file of it cannot be read.
    let read_module = |base: &str| {
        let files = module_files(&root, base);
        if files.is_empty() {
            return None;
        }
        files
            .iter()
            .map(|f| read(f))
            .collect::<Option<Vec<String>>>()
            .map(|texts| texts.join("\n"))
    };

    // 1. The codes the checker can raise, against the list the identity is built from.
    // The checker is `check.rs` and, since N36, the effect checker beside it — and since
    // N38 the provenance solver, and since Gate 2 the constant evaluator.
    let checker = match (
        read_module("crates/nazm-core/src/check"),
        read_module("crates/nazm-core/src/effects"),
        read_module("crates/nazm-core/src/provenance"),
        read_module("crates/nazm-core/src/constants"),
    ) {
        (Some(check), Some(effects), Some(provenance), Some(constants)) => {
            Some(format!("{check}\n{effects}\n{provenance}\n{constants}"))
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
pub(crate) fn check_object_cache() -> Result<(), Vec<String>> {
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
