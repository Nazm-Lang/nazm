//! Claims the documentation makes that the build can check.
//!
//! `cargo xtask check`'s "doc claims" gate exists for this, and today it checks two
//! things about the benchmark harness's pre-registration status — both of which are
//! no-ops, because the file they are about does not exist. So the gate passes and
//! establishes nothing about the language, which is where this session's claims are.
//!
//! These are the ones a machine can settle. A README that lists a command the binary
//! does not have, or omits one it does, is the drift this project exists to refuse; so
//! is a schema file nothing documents, or a documented target the emitter disagrees
//! with.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

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

/// The text with every run of whitespace collapsed to one space.
///
/// Prose is wrapped, so a sentence a document genuinely contains is split across two
/// lines and an exact substring search misses it. The first version of the target check
/// failed for exactly that reason, on a README that said the right thing.
fn flowed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_readme_lists_exactly_the_commands_the_binary_has() {
    let help = Command::new(NAZM).arg("--help").output().expect("run");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();

    // clap's `Commands:` block, one per line, name first.
    let mut real: BTreeSet<String> = BTreeSet::new();
    let mut inside = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            inside = true;
            continue;
        }
        if inside {
            if line.trim().is_empty() {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                // `help` is clap's, not this compiler's, and documenting it would be
                // documenting the argument parser.
                if name != "help" {
                    real.insert(name.to_owned());
                }
            }
        }
    }
    assert!(real.len() >= 5, "only found {real:?} in --help");

    // The README's command table: rows beginning `| \`nazm NAME`.
    let readme = read("README.md");
    let mut documented: BTreeSet<String> = BTreeSet::new();
    for line in readme.lines() {
        let Some(rest) = line.trim().strip_prefix("| `nazm ") else {
            continue;
        };
        if let Some(name) = rest.split_whitespace().next() {
            documented.insert(name.trim_end_matches('`').to_owned());
        }
    }

    assert_eq!(
        documented, real,
        "README.md's command table and `nazm --help` disagree.\n  documented: \
         {documented:?}\n  real:       {real:?}"
    );
}

#[test]
fn every_schema_file_is_documented_and_every_documented_schema_exists() {
    let dir = repo_root().join("schema");
    let mut files: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("schema/").flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "json") {
            files.insert(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert!(files.len() >= 4, "only {files:?}");

    let doc = read("docs/diagnostics.md");
    for f in &files {
        // `nazm.fix-1.json` is documented under its identifier, `nazm.fix/1`.
        let identifier = f
            .trim_end_matches(".json")
            .rsplit_once('-')
            .map(|(name, major)| format!("{name}/{major}"))
            .unwrap_or_default();
        assert!(
            doc.contains(&identifier),
            "schema/{f} exists and `{identifier}` is not mentioned in \
             docs/diagnostics.md"
        );
    }

    // And each schema agrees with itself: the title is the identifier it stamps on
    // every object.
    for f in &files {
        let text = std::fs::read_to_string(dir.join(f)).expect("read");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let title = value.get("title").and_then(serde_json::Value::as_str);
        let stamped = value
            .pointer("/properties/schema/const")
            .and_then(serde_json::Value::as_str);
        assert_eq!(
            title, stamped,
            "schema/{f}: the title and the `schema` constant differ"
        );
    }
}

#[test]
fn every_example_is_checked_by_a_test() {
    // `examples/` is what a reader is pointed at first, so an example that nothing runs
    // is the worst place for a program that does not work. The list in `tests/build.rs`
    // pairs each with its expected value and runs it; this requires the list to be
    // complete.
    let root = repo_root();
    let mut present: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("examples"))
        .expect("examples/")
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "nz") {
            present.insert(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert!(present.len() >= 4, "only {present:?}");

    let build = read("crates/nazm-cli/tests/build.rs");
    let missing: Vec<&String> = present
        .iter()
        .filter(|f| !build.contains(&format!("examples/{f}")))
        .collect();
    assert!(
        missing.is_empty(),
        "these examples are run by nothing: {missing:?}. Add each to the list in          crates/nazm-cli/tests/build.rs with the value it should produce."
    );
}

#[test]
fn the_documented_target_is_the_one_the_emitter_names() {
    // One sentence, in one place, quoted rather than paraphrased wherever it appears.
    let note = nazm_lir::TARGET_NOTE;
    let target = "aarch64-apple-darwin";
    assert!(note.contains(target), "the emitter's note changed: {note}");
    for file in ["README.md", "docs/releases/4fb1554.md"] {
        let text = flowed(&read(file));
        assert!(
            text.contains(target),
            "{file} does not name the only verified target"
        );
        assert!(
            text.contains("unverified rather than unsupported"),
            "{file} names the target without the qualification the emitter carries"
        );
    }
}

#[test]
#[allow(clippy::indexing_slicing)] // the row's length is asserted first
fn the_support_page_states_the_compilers_own_matrix() {
    // `docs/support.md` is the public answer to "does it run on X?", and the compiler carries
    // the same answer in `SUPPORT` (N95). A page edited by hand drifts from a table edited in
    // code, so every cell is compared: the status under each backend — with a QEMU run
    // written `emulator-verified`, a kind of run-verified — and the debugger, word for word.
    use nazm_lir::backend::{Support, NON_GOALS, SUPPORT};
    let page = read("docs/support.md");
    let label = |s: Support, row: &str| -> Vec<&'static str> {
        match s {
            Support::Run(_) if row.contains("QEMU") => vec!["emulator-verified"],
            Support::Run(_) => vec!["run-verified"],
            Support::Wine(_) => vec!["run-verified under Wine"],
            Support::WindowsHost(_) => vec!["run-verified on a Windows host"],
            Support::Bionic(_) => vec!["run-verified on a Linux kernel"],
            Support::Compile(_) => vec!["compile-only"],
            Support::Unsupported(_) => vec!["unsupported"],
        }
    };
    let mut seen = 0;
    for line in page.lines() {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split(" | ")
            .map(str::trim)
            .collect();
        let Some(triple) = cells
            .first()
            .and_then(|c| c.strip_prefix('`')?.strip_suffix('`'))
        else {
            continue;
        };
        let Some(cell) = SUPPORT.iter().find(|c| c.triple == triple) else {
            continue;
        };
        assert_eq!(
            cells.len(),
            5,
            "docs/support.md: the row for {triple} is malformed: {line}"
        );
        assert!(
            label(cell.llvm, line).contains(&cells[1]),
            "docs/support.md says {triple} is `{}` under LLVM; the compiler says {}",
            cells[1],
            cell.llvm.name()
        );
        assert!(
            label(cell.cranelift, line).contains(&cells[2]),
            "docs/support.md says {triple} is `{}` under Cranelift; the compiler says {}",
            cells[2],
            cell.cranelift.name()
        );
        assert_eq!(
            cells[4], cell.debugger,
            "docs/support.md: the debugger cell for {triple}"
        );
        seen += 1;
    }
    assert_eq!(
        seen,
        SUPPORT.len(),
        "docs/support.md must have one row per target the compiler accepts"
    );
    for (what, why) in NON_GOALS {
        assert!(
            page.contains(&format!("| {what} | {why} |")),
            "docs/support.md does not state the non-goal `{what}` as the compiler does"
        );
    }
}

/// Every anchored section of `docs/spec.md` is classified for 1.0, once, in one of the six
/// classes, and the table names no section that does not exist (Gate 1). A section added
/// without a class would be a rule nobody has said is stable.
#[test]
fn every_spec_section_is_classified_for_1_0() {
    let spec = read("docs/spec.md");
    let ids: BTreeSet<String> = spec
        .lines()
        .filter_map(|l| l.trim().strip_prefix("<!-- id:"))
        .filter_map(|r| r.strip_suffix("-->"))
        .map(|id| id.trim().to_owned())
        .collect();
    assert!(ids.len() > 60, "only {} anchors", ids.len());

    let table = spec
        .split("## Stability classification for 1.0")
        .nth(1)
        .expect("the classification section");
    let table = table.split("\n## ").next().unwrap_or_default();
    const CLASSES: [&str; 7] = [
        "STABLE 1.0",
        "COMPATIBLE EXTENSION POINT",
        "EXPERIMENTAL",
        "RESEARCH/OPEN",
        "NON-GOAL",
        "HISTORICAL",
        "DEBUG",
    ];
    let mut classified: Vec<String> = Vec::new();
    let mut open_family = false;
    for row in table.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let (section, class) = (cells[1], cells[2]);
        assert!(
            class.split(" / ").all(|c| CLASSES.contains(&c)),
            "`{section}` has class `{class}`"
        );
        if section.contains("every `open-*`") {
            open_family = true;
        }
        let id = section.split('`').nth(1).unwrap_or_default().to_owned();
        assert!(
            ids.contains(&id),
            "the table classifies `{id}`, which is no section"
        );
        assert!(!classified.contains(&id), "`{id}` is classified twice");
        classified.push(id);
    }
    for id in &ids {
        let covered = classified.contains(id) || (open_family && id.starts_with("open-"));
        assert!(covered, "spec section `{id}` has no 1.0 classification");
    }
}

/// `docs/stability.md` puts every command of this build in exactly one class, and classifies
/// every schema `schema/` holds (Gate 1).
#[test]
fn the_stability_contract_classifies_every_command_and_schema_file() {
    let out = Command::new(NAZM)
        .args(["capabilities", "--json"])
        .output()
        .expect("run");
    let inv: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let stability = read("docs/stability.md");
    let commands = stability
        .split("### The command line")
        .nth(1)
        .expect("the command-line section");
    let rows: Vec<&str> = commands
        .lines()
        .filter(|l| {
            l.starts_with("| **Stable public**")
                || l.starts_with("| **Experimental**")
                || l.starts_with("| **Debug/internal**")
        })
        .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    for c in inv["commands"].as_array().unwrap() {
        let c = c.as_str().unwrap();
        let n = rows
            .iter()
            .filter(|r| r.contains(&format!("`{c}`")))
            .count();
        assert_eq!(n, 1, "`nazm {c}` is in {n} classes of docs/stability.md");
    }

    let schemas = stability
        .split("### Machine-readable output")
        .nth(1)
        .expect("the schema section");
    for e in std::fs::read_dir(repo_root().join("schema"))
        .unwrap()
        .flatten()
    {
        let file = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".json") else {
            continue;
        };
        // `nazm.api-doc-2.json` is `nazm.api-doc/2`.
        let (name, major) = stem.rsplit_once('-').unwrap();
        let id = format!("`{name}/{major}`");
        assert!(
            schemas.contains(&id),
            "{id} ({file}) is not classified in docs/stability.md"
        );
    }
}

#[test]
fn the_stability_contract_states_the_current_identities() {
    // M1: `docs/stability.md` restates the identities a build carries, and every Gate 2 slice
    // moves one. The table is held to the constants rather than to whoever remembered.
    let text = read("docs/stability.md");
    let row = |name: &str| -> String {
        text.lines()
            .find(|l| l.starts_with(&format!("| **{name}** |")))
            .unwrap_or_else(|| panic!("docs/stability.md has no `{name}` row"))
            .to_owned()
    };
    let current = |line: &str| -> String {
        // The third cell: what the identity is now.
        line.split('|').nth(3).unwrap_or("").trim().to_owned()
    };
    let epoch = row("Semantic epoch");
    assert_eq!(
        current(&epoch),
        format!("**{}**", nazm_cache::SEMANTIC_EPOCH),
        "docs/stability.md's semantic epoch row is stale: {epoch}"
    );
    let abi = row("Runtime ABI");
    assert_eq!(
        current(&abi),
        format!("**{}**", nazm_lir::runtime::RUNTIME_ABI_REVISION),
        "docs/stability.md's runtime ABI row is stale: {abi}"
    );
    let iface = row("Interface schema");
    assert_eq!(
        current(&iface),
        format!("**`{}`**", nazm_iface::SCHEMA),
        "docs/stability.md's interface schema row is stale: {iface}"
    );
    let toolchain = row("Toolchain version");
    assert_eq!(
        current(&toolchain),
        format!("**{}**", env!("CARGO_PKG_VERSION")),
        "docs/stability.md's toolchain row is stale: {toolchain}"
    );
}
