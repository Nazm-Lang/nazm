//! The runtime parity gate: the runtime text the Rust compiler emits and the text the compiler
//! written in Nazm emits are the same.

/// The two compilers emit **the same runtime**, derived rather than transcribed.
///
/// `compiler/emit.nz` writes the same helpers `crates/nazm-lir/src/emit.rs` writes, because
/// the compiler written in Nazm has to give its programs the memory semantics the compiler
/// written in Rust gives its own. Until N8 the two were separate transcriptions kept in
/// step by hand, and they fell out of step in exactly the way that arrangement predicts:
/// N7 taught the Rust emitter to reclaim sequence storage and the Nazm one went on emitting
/// a header with no reference count at all, for a milestone, silently.
///
/// So the Nazm text is now **derived**: every `strs_push` line below is a line of the Rust
/// runtime constants with `hidden` changed to `internal`. This gate performs that
/// derivation again and refuses a difference. It is a pure file read; it builds nothing.
///
/// What it does *not* check is that the two emitters make the same *decisions* — where to
/// retain, where to release. Nothing textual can check that, and it is what the
/// differential memory tests in `crates/nazm-cli/tests/selfhost.rs` are for.
pub(crate) fn check_runtime_parity() -> Result<(), Vec<String>> {
    const RUST: &str = "crates/nazm-runtime/src/core.rs";
    const NAZM: &str = "compiler/emit.nz";
    // Which Rust runtime constants make up each Nazm function, in order.
    const GROUPS: &[(&str, &[&str])] = &[
        (
            "runtime",
            &[
                "SEQUENCE_RUNTIME",
                "STRING_RUNTIME",
                "IO_RUNTIME",
                "JOIN_RUNTIME",
            ],
        ),
        ("task_runtime", &["TASK_RUNTIME"]),
        ("channel_runtime", &["CHANNEL_RUNTIME"]),
        // The stack check (N49), since the compiler written in Nazm compiles recursion (N102).
        ("stack_runtime", &["STACK_RUNTIME"]),
        // Closure objects (N50), since the compiler written in Nazm compiles closures (N102).
        ("closure_runtime", &["CLOSURE_RUNTIME"]),
    ];

    let mut bad = Vec::new();
    let rust = match std::fs::read_to_string(RUST) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{RUST}: {e}")]),
    };
    let nazm = match std::fs::read_to_string(NAZM) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{NAZM}: {e}")]),
    };

    for (function, constants) in GROUPS {
        let mut lines: Vec<String> = Vec::new();
        let mut missing = false;
        for name in *constants {
            match raw_string_constant(&rust, name) {
                Some(body) => {
                    lines.extend(body);
                    lines.push(String::new());
                }
                None => {
                    bad.push(format!("{RUST} has no runtime constant `{name}`"));
                    missing = true;
                }
            }
        }
        if missing {
            continue;
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let mut want = String::from("    let r = strs_new();\n");
        for line in &lines {
            let line = line.replace("define hidden ", "define internal ");
            if line.contains('"') || line.contains('\\') {
                bad.push(format!(
                    "the runtime line `{line}` needs an escape Nazm string literals do not have"
                ));
            }
            want.push_str(&format!("    strs_push(r, \"{line}\");\n"));
        }
        want.push_str("    str_concat(str_join(r, nl()), nl())\n");
        match nazm_function_body(&nazm, function) {
            None => bad.push(format!("{NAZM} has no `fn {function}() -> Str`")),
            Some(have) if have == want => {}
            Some(have) => bad.push(format!(
                "{NAZM}'s `{function}` is not `{RUST}`'s runtime: {}. Regenerate it rather \
                 than editing it — the Rust runtime is the one that is written",
                first_difference(&want, &have)
            )),
        }
    }

    // The memory report's two strings, so a program built by either compiler prints the
    // same line and the differential tests can compare them.
    for (constant, function) in [
        ("MEMORY_VARIABLE", "memory_variable"),
        ("MEMORY_FORMAT", "memory_format"),
    ] {
        let Some(want) = rust_string_constant(&rust, constant) else {
            bad.push(format!("{RUST} has no `{constant}`"));
            continue;
        };
        match nazm_function_body(&nazm, function) {
            None => bad.push(format!("{NAZM} has no `fn {function}() -> Str`")),
            Some(have) if have.contains(&format!("\"{want}\"")) => {}
            Some(_) => bad.push(format!(
                "{NAZM}'s `{function}` does not return `{constant}`'s text, so the two \
                 compilers' programs would print different memory reports"
            )),
        }
    }

    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

/// The lines of `const NAME: &str = r"…";`, without its leading and trailing blank lines.
fn raw_string_constant(source: &str, name: &str) -> Option<Vec<String>> {
    let open = format!("const {name}: &str = r\"");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\";")?;
    let mut lines: Vec<String> = rest.get(..end)?.lines().map(str::to_owned).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    Some(lines)
}

/// The text of `pub const NAME: &str = "…";`, with Rust's line continuations resolved.
fn rust_string_constant(source: &str, name: &str) -> Option<String> {
    let open = format!("pub const {name}: &str = \"");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\";")?;
    let raw = rest.get(..end)?;
    // A `\` at the end of a line continues the literal and eats the following indentation.
    let mut out = String::new();
    let mut parts = raw.split('\\');
    out.push_str(parts.next().unwrap_or_default());
    for part in parts {
        out.push_str(part.trim_start());
    }
    Some(out)
}

/// The body of `fn NAME() -> Str { … }`, between its braces, as written.
fn nazm_function_body(source: &str, name: &str) -> Option<String> {
    let open = format!("\nfn {name}() -> Str {{\n");
    let at = source.find(&open)? + open.len();
    let rest = source.get(at..)?;
    let end = rest.find("\n}\n")?;
    Some(rest.get(..=end)?.to_owned())
}

/// The first line where two texts differ, for a message that names it rather than printing
/// two thousand lines and leaving the reader to diff them.
fn first_difference(want: &str, have: &str) -> String {
    for (n, (a, b)) in want.lines().zip(have.lines()).enumerate() {
        if a != b {
            return format!("line {} is `{}`, expected `{}`", n + 1, b.trim(), a.trim());
        }
    }
    format!(
        "it has {} lines where {} were derived",
        have.lines().count(),
        want.lines().count()
    )
}
