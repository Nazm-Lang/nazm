//! Every diagnostic code is either asserted by a test, or named as unprovokable.
//!
//! A code is part of the machine interface: `docs/user/diagnostics.md` promises that one
//! never changes meaning and is never reused, so each is a commitment. A commitment no
//! test exercises is a claim about behaviour nobody has seen.
//!
//! Some genuinely cannot be provoked from a test — they need the machine to run out of
//! memory or of threads. Those are listed below **with the reason**, because a code
//! nobody tested and a code nobody can test look identical from the outside and only one
//! of them is acceptable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// Codes no test asserts, and why each one cannot be reached from a test. (`N0404` left the
/// list in N106: a `NAZM_SCHEDULER` that names no scheduler provokes it on purpose.)
///
/// Adding to this list is how a code escapes the requirement, so it takes a sentence
/// that has to be true. Removing something from it is what happens when someone finds a
/// way to provoke it.
///
/// Empty since Gate 3: `N0406`, an allocation failed, was here — exhausting a host's memory
/// measures the machine, not the compiler — until a board's arena made it reachable on purpose:
/// `heap.rs` runs a program out of a 256-byte heap under QEMU and asserts it.
const UNPROVOKABLE: &[(&str, &str)] = &[];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read(path: &str) -> String {
    let p = repo_root().join(path);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every `.rs` file under the workspace's test directories, concatenated.
fn all_tests() -> String {
    let mut out = String::new();
    for dir in [
        "crates/nazm-core/tests",
        "crates/nazm-cli/tests",
        "crates/nazm-syntax/tests",
    ] {
        let Ok(entries) = std::fs::read_dir(repo_root().join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            // Not this file. It names every excused code in `UNPROVOKABLE`, so reading
            // itself would make every excused code look asserted — and the check that
            // a code is not both would fire on its own text.
            if p.file_name().is_some_and(|n| n == "codes.rs") {
                continue;
            }
            if p.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push_str(&uncommented(&text));
                }
            }
        }
    }
    // Unit tests live beside the code they test and assert codes just as usefully — but
    // only the test module counts. The first version of this concatenated whole source
    // files, which contain the `Code::new("N0404")` declarations and every `codes::X`
    // emission site, so every code looked asserted and the check proved nothing. Take
    // the text from `mod tests {` onward and no more.
    // A module split into a directory is read file by file: a `tests.rs` whole, any other
    // file from its own `mod tests {`.
    for module in [
        "crates/nazm-syntax/src/lexer",
        "crates/nazm-syntax/src/parser",
        "crates/nazm-diag/src/lib",
        "crates/nazm-core/src/check",
        "crates/nazm-core/src/lower",
    ] {
        for file in module_files(module) {
            let text = read(&file);
            if file.ends_with("/tests.rs") {
                out.push_str(&uncommented(&text));
            } else if let Some(i) = text.find("\nmod tests {") {
                out.push_str(&uncommented(text.get(i..).unwrap_or_default()));
            }
        }
    }
    out
}

/// `text` without its comment lines (v1, Q1-D-24): a code named only in a comment — "this used
/// to be `N0101`" — is not a test asserting it.
fn uncommented(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .fold(String::new(), |mut out, l| {
            out.push_str(l);
            out.push('\n');
            out
        })
}

/// Every declared code: `(constant, code)`, from `crates/nazm-diag/src/lib.rs`.
fn declared() -> Vec<(String, String)> {
    let mut codes = Vec::new();
    for line in read("crates/nazm-diag/src/lib.rs").lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(": Code = Code::new(\"") else {
            continue;
        };
        let Some(number) = rest.split('"').next() else {
            continue;
        };
        codes.push((name.to_owned(), number.to_owned()));
    }
    codes
}

/// Q1-D-11, -13, -14: `docs/user/diagnostics.md`'s registry is every declared code, each once, with
/// its constant, and nothing else but codes marked reserved, which no constant declares.
#[test]
fn the_registry_is_every_declared_code_and_only_them() {
    let docs = read("docs/user/diagnostics.md");
    let begin = docs
        .find("<!-- registry:begin -->")
        .expect("registry start");
    let end = docs.find("<!-- registry:end -->").expect("registry end");
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in docs[begin..end].lines() {
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(code), Some(name)) = (cols.get(1), cols.get(2)) else {
            continue;
        };
        let code = code.trim_matches('`');
        if !code.starts_with('N') || code.len() != 5 {
            continue;
        }
        rows.push((code.to_owned(), name.trim_matches('`').to_owned()));
    }
    let declared = declared();
    assert!(declared.len() > 140, "only {} declared", declared.len());
    let mut seen = std::collections::BTreeSet::new();
    for (code, _) in &rows {
        assert!(seen.insert(code.clone()), "{code} has two rows");
    }
    for (name, code) in &declared {
        assert!(
            rows.iter().any(|(c, n)| c == code && n == name),
            "{code} ({name}) is declared and not in docs/user/diagnostics.md's registry: add the row\n| `{code}` | `{name}` | … |"
        );
    }
    for (code, name) in &rows {
        if name == "—" {
            assert!(
                !declared.iter().any(|(_, c)| c == code),
                "{code} is marked reserved and is declared"
            );
        } else {
            assert!(
                declared.iter().any(|(n, c)| c == code && n == name),
                "the registry names {code} ({name}), which is not declared"
            );
        }
    }
    // The umbrella code says it is one.
    assert!(rows.iter().any(|(c, _)| c == "N0101"));
    assert!(docs.contains("**the umbrella code**"));
}

/// The files of the module at `base`: `base.rs`, or every `.rs` file under `base/`.
fn module_files(base: &str) -> Vec<String> {
    let file = format!("{base}.rs");
    if repo_root().join(&file).is_file() {
        return vec![file];
    }
    let mut out = Vec::new();
    let mut stack = vec![repo_root().join(base)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let rel = p.strip_prefix(repo_root()).unwrap_or(&p);
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    assert!(
        !out.is_empty(),
        "{base}: neither a file nor a module directory"
    );
    out.sort();
    out
}

#[test]
fn every_code_is_asserted_by_a_test_or_named_as_unprovokable() {
    let declared = read("crates/nazm-diag/src/lib.rs");
    let tests = all_tests();

    let mut codes = Vec::new();
    for line in declared.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(": Code = Code::new(\"") else {
            continue;
        };
        let Some(number) = rest.split('"').next() else {
            continue;
        };
        codes.push((name.to_owned(), number.to_owned()));
    }
    assert!(codes.len() > 25, "only found {} codes", codes.len());

    let mut missing = Vec::new();
    for (name, number) in &codes {
        // Either the constant is named in a test, or the number appears in one — a
        // JSON assertion quotes the number rather than the constant.
        let asserted = tests.contains(&format!("codes::{name}")) || tests.contains(number);
        let excused = UNPROVOKABLE.iter().any(|(n, _)| n == number);
        if !asserted && !excused {
            missing.push(format!("{number} ({name})"));
        }
        assert!(
            !(asserted && excused),
            "{number} is listed as unprovokable and is also asserted by a test — remove \
             it from the list"
        );
    }
    assert!(
        missing.is_empty(),
        "these codes are promised and never asserted: {missing:?}. Either assert one, \
         or add it to UNPROVOKABLE with the reason it cannot be reached."
    );
}

#[test]
fn every_unprovokable_code_is_a_real_code_with_a_reason() {
    let declared = read("crates/nazm-diag/src/lib.rs");
    for (number, why) in UNPROVOKABLE {
        assert!(
            declared.contains(&format!("Code::new(\"{number}\")")),
            "{number} is excused and is not a code"
        );
        assert!(
            why.len() > 40,
            "{number}'s reason is too short to be one: {why:?}"
        );
    }
}
