//! The module-boundaries gate: a module split by responsibility (M2) keeps its entry thin and
//! says, in each file, what that file is for.
//!
//! # What it guards, and why a size cap would not
//!
//! M2 split the files that had grown by gathering concerns — the checker, the interpreter,
//! `nazm build`, the two command lines — and a split decays the same way the original grew: the
//! next concern is added to the entry module because it is the file already open. A line-count
//! cap was considered and rejected: size is a symptom, and a cap is met by moving the wrong
//! thing. So the rule is about responsibility.
//!
//! - **An entry module defines only what the register names for it:** the façade's entry
//!   points, the shared context its children hang off, and the orchestration that calls them,
//!   plus `mod`, `use` and `impl`s of those types. A new top-level item there fails, naming the
//!   item and asking for a module of its own — or, if it really is façade, context or
//!   orchestration, for its name in [`REGISTER`], where a reviewer sees the decision.
//! - **Every file of a split module opens with a `//!` line** saying what it is responsible for.
//! - **A registered entry that is not in the tree fails**, rather than leaving the rule quietly
//!   checking nothing.

use std::path::Path;

/// One module M2 split, and what its entry may define itself.
struct Entry {
    /// The entry file, repository-relative.
    file: &'static str,
    /// When the entry is a directory module's `mod.rs`, every file under that directory must
    /// open with a responsibility line.
    directory: bool,
    /// The top-level items the entry may define: the façade, the shared context, orchestration.
    may_define: &'static [&'static str],
}

/// The register. Adding a name here is a design decision a reviewer sees; adding an item to an
/// entry without it fails the gate.
const REGISTER: &[Entry] = &[
    Entry {
        file: "crates/nazm-core/src/check/mod.rs",
        directory: true,
        // The checker's context, and the driver.
        may_define: &[
            "Signature",
            "Binding",
            "forged",
            "Scopes",
            "BinaryCheck",
            "Completion",
            "Visible",
            "Types",
            "Env",
            "Unit",
            "Checker",
            "ClosureFrame",
            "Reuse",
            "CheckEverything",
            "check",
            "check_reusing",
        ],
    },
    Entry {
        file: "crates/nazm-core/src/eval/mod.rs",
        directory: true,
        // The interpreter's context, its limits and stack budget, and its control flow; and
        // (Gate 3) `Cells`, the context's static atomics.
        may_define: &[
            "RuntimeError",
            "Cells",
            "Frame",
            "Flow",
            "take",
            "fail",
            "no_value",
            "DEFAULT_MAX_ITERATIONS",
            "STACK_BYTES_PER_CALL",
            "STACK_BASE_BYTES",
            "MAX_SUPPORTED_DEPTH",
            "stack_for",
            "on_interpreter_stack",
            "DEFAULT_MAX_DEPTH",
            "Limits",
            "Interpreter",
            "unreachable_flow",
            "frame",
        ],
    },
    Entry {
        file: "crates/nazm-cli/src/build/mod.rs",
        directory: true,
        // `run`, the sequence of phases, and what it is asked and reports.
        may_define: &["Native", "run", "Summarize", "Choice"],
    },
    Entry {
        file: "crates/nazm-cli/src/main.rs",
        directory: false,
        // Parse, prepare, dispatch; the diagnostic rendering every command shares; and how a
        // command that ran a program ends with `main`'s result (R1, 5b.3), beside `report_ok`.
        may_define: &[
            "main",
            "parse_cli",
            "command",
            "dispatch",
            "report_ok",
            "end_with_result",
            "emit",
        ],
    },
    Entry {
        file: "xtask/src/main.rs",
        directory: false,
        may_define: &["main", "help"],
    },
    Entry {
        file: "xtask/src/checks/mod.rs",
        directory: true,
        // Running and reporting the gates, and the path helpers they share.
        may_define: &[
            "run_checks",
            "report",
            "collect_rs",
            "workspace_root",
            "module_files",
            "source_files",
        ],
    },
    Entry {
        file: "xtask/src/commands/mod.rs",
        directory: true,
        may_define: &[],
    },
];

pub(crate) fn check_module_boundaries(root: &Path) -> Result<(), Vec<String>> {
    let problems = boundary_problems(root, REGISTER);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn boundary_problems(root: &Path, register: &[Entry]) -> Vec<String> {
    let mut problems = Vec::new();
    for entry in register {
        let Ok(text) = std::fs::read_to_string(root.join(entry.file)) else {
            problems.push(format!(
                "{}: registered as a split module's entry, but not in the tree; update the \
                 module-boundaries register",
                entry.file
            ));
            continue;
        };
        for (line, kind, name) in top_level_items(&text) {
            if !entry.may_define.contains(&name.as_str()) {
                problems.push(format!(
                    "{}:{line}: `{kind} {name}` is a new concern in an entry module; put it in a \
                     module of its own, or — if it is the façade, shared context or \
                     orchestration — name it in the module-boundaries register \
                     (xtask/src/checks/modules.rs)",
                    entry.file
                ));
            }
        }
        if entry.directory {
            let dir = Path::new(entry.file).parent().unwrap_or(Path::new(""));
            let Ok(files) = super::collect_rs(&root.join(dir)) else {
                continue;
            };
            let mut files: Vec<String> = files
                .iter()
                .filter_map(|p| p.strip_prefix(root).ok())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect();
            files.sort();
            for file in files {
                let opens = std::fs::read_to_string(root.join(&file))
                    .is_ok_and(|t| t.trim_start().starts_with("//!"));
                if !opens {
                    problems.push(format!(
                        "{file}: no `//!` line saying what this module is responsible for"
                    ));
                }
            }
        }
    }
    problems
}

/// `(line, kind, name)` for each item defined at the top level of `text`: functions, types,
/// traits, constants, statics, macros, and — named by their self type — `impl` blocks. `mod`
/// and `use` are declarations, not definitions, and are not listed.
fn top_level_items(text: &str) -> Vec<(usize, &'static str, String)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut rest = strip_visibility(line);
        // `const fn`, `async fn`, `unsafe fn` are functions.
        for qualifier in ["const ", "async ", "unsafe "] {
            if let Some(f) = rest
                .strip_prefix(qualifier)
                .filter(|r| r.starts_with("fn "))
            {
                rest = f;
            }
        }
        for kind in [
            "fn", "struct", "enum", "trait", "type", "const", "static", "impl",
        ] {
            let Some(after) = rest.strip_prefix(kind) else {
                continue;
            };
            if !after.starts_with([' ', '<']) {
                continue;
            }
            let name = if kind == "impl" {
                impl_self_type(after)
            } else {
                ident(after.trim_start())
            };
            out.push((n + 1, kind, name));
        }
        if let Some(after) = rest.strip_prefix("macro_rules!") {
            out.push((n + 1, "macro_rules!", ident(after.trim_start())));
        }
    }
    out
}

fn strip_visibility(line: &str) -> &str {
    let Some(rest) = line.strip_prefix("pub") else {
        return line;
    };
    let rest = match rest.strip_prefix('(') {
        Some(r) => r.split_once(')').map_or(r, |(_, after)| after),
        None => rest,
    };
    rest.trim_start()
}

/// The leading identifier of `s`.
fn ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// The type an `impl` header is for: `impl<'a> Unit<'a> {` is `Unit`, `impl Drop for X {` is
/// `X`.
fn impl_self_type(after_impl: &str) -> String {
    // Skip the impl's own generic parameters.
    let mut s = after_impl.trim_start();
    if s.starts_with('<') {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        s = s.get(i + 1..).unwrap_or("").trim_start();
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let header = s.split('{').next().unwrap_or(s);
    let self_type = header.split(" for ").nth(1).unwrap_or(header).trim_start();
    ident(self_type.rsplit("::").next().unwrap_or(self_type))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::{boundary_problems, top_level_items, Entry};

    const ENTRY: &[Entry] = &[Entry {
        file: "m/mod.rs",
        directory: true,
        may_define: &["Context", "run"],
    }];

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let p = d.path().join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        d
    }

    const THIN: &str = "//! The thing.\n\nmod part;\n\npub(crate) use part::helper;\n\n\
        pub struct Context {\n    x: u8,\n}\n\nimpl Context {\n    fn new() -> Self { Self { x: 0 } }\n}\n\n\
        impl Default for Context {\n    fn default() -> Self { Self::new() }\n}\n\n\
        pub(crate) fn run() {}\n";

    #[test]
    fn a_thin_entry_with_named_items_and_documented_children_passes() {
        let d = tree(&[
            ("m/mod.rs", THIN),
            ("m/part.rs", "//! A part.\n\npub(super) fn helper() {}\n"),
        ]);
        assert_eq!(boundary_problems(d.path(), ENTRY), Vec::<String>::new());
    }

    #[test]
    fn a_new_function_in_the_entry_fails_naming_it() {
        let d = tree(&[
            ("m/mod.rs", &format!("{THIN}\nfn another_concern() {{}}\n")),
            ("m/part.rs", "//! A part.\n"),
        ]);
        let p = boundary_problems(d.path(), ENTRY);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`fn another_concern`"), "{p:?}");
    }

    #[test]
    fn an_impl_of_a_type_the_entry_does_not_own_fails() {
        let d = tree(&[
            (
                "m/mod.rs",
                &format!("{THIN}\nimpl<'a> Display for Stranger<'a> {{}}\n"),
            ),
            ("m/part.rs", "//! A part.\n"),
        ]);
        let p = boundary_problems(d.path(), ENTRY);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`impl Stranger`"), "{p:?}");
    }

    #[test]
    fn a_file_without_a_responsibility_line_fails() {
        let d = tree(&[
            ("m/mod.rs", THIN),
            ("m/part.rs", "pub(super) fn helper() {}\n"),
            ("m/deeper/leaf.rs", "//! A leaf.\n"),
        ]);
        let p = boundary_problems(d.path(), ENTRY);
        assert_eq!(
            p,
            ["m/part.rs: no `//!` line saying what this module is responsible for"]
        );
    }

    #[test]
    fn a_registered_entry_that_is_not_in_the_tree_fails() {
        let d = tree(&[("elsewhere.rs", "//! x\n")]);
        let p = boundary_problems(d.path(), ENTRY);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("not in the tree"), "{p:?}");
    }

    #[test]
    fn items_are_named_by_what_they_define() {
        let items: Vec<(&str, String)> = top_level_items(
            "pub(crate) fn f<T>() {}\nimpl<'a> Unit<'a> {}\nimpl From<X> for Y {}\n\
             macro_rules! take {}\npub const C: u8 = 0;\nmod m;\nuse a::b;\n    fn nested() {}\n",
        )
        .into_iter()
        .map(|(_, k, n)| (k, n))
        .collect();
        assert_eq!(
            items,
            [
                ("fn", "f".to_owned()),
                ("impl", "Unit".to_owned()),
                ("impl", "Y".to_owned()),
                ("macro_rules!", "take".to_owned()),
                ("const", "C".to_owned()),
            ]
        );
    }
}
