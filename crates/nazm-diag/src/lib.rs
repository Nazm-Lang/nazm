//! Diagnostics as data first, prose second.
//!
//! Every diagnostic carries a **stable code**, a span, and where possible a suggested
//! repair. The terminal rendering is a view of that structure, not the structure itself.
//!
//! The reason is the project's whole premise: an agent that has to regex a prose message
//! to find out what went wrong spends turns discovering what the compiler already knew.
//! A code it can match on, a span it can edit at, and a suggestion it can apply turn a
//! repair loop into a single step. Codes are therefore part of the contract — renaming
//! one is a breaking change, and they are never reused.

use nazm_span::{SourceFile, Span};
use serde::{Deserialize, Serialize};

/// A stable diagnostic code. The number never changes meaning and is never reused.
///
/// A `Cow` rather than a `&'static str`: the constants below want to be `const`, and a
/// diagnostic read back from JSON owns its string. Borrowing covers the first, owning the
/// second, and callers compare them without caring which.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Code(pub std::borrow::Cow<'static, str>);

impl Code {
    #[must_use]
    pub const fn new(s: &'static str) -> Self {
        Self(std::borrow::Cow::Borrowed(s))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every code the v0.1 front end can produce.
///
/// Listed in one place on purpose: a code introduced ad hoc at its throw site is a code
/// nobody documents, and the set an agent can expect to see is part of the language's
/// interface.
pub mod codes {
    use super::Code;

    // Lexing
    pub const UNKNOWN_CHARACTER: Code = Code::new("N0001");
    pub const UNTERMINATED_STRING: Code = Code::new("N0002");
    pub const INTEGER_TOO_LARGE: Code = Code::new("N0003");

    // Parsing
    /// An expression or block nested past the parser's limit. A parse-time bound, so
    /// every later pass over the tree — checking, evaluating, and dropping it — inherits
    /// it without needing a limit of its own.
    pub const NESTING_TOO_DEEP: Code = Code::new("N0102");
    pub const UNEXPECTED_TOKEN: Code = Code::new("N0100");
    pub const UNSUPPORTED_CONSTRUCT: Code = Code::new("N0101");

    // Names and scope
    pub const UNDEFINED_VARIABLE: Code = Code::new("N0200");
    pub const UNDEFINED_FUNCTION: Code = Code::new("N0201");
    pub const ALREADY_DEFINED_IN_SCOPE: Code = Code::new("N0202");
    pub const DUPLICATE_FUNCTION: Code = Code::new("N0203");
    pub const MISSING_MAIN: Code = Code::new("N0204");
    /// `use "path";` named a file that could not be read.
    pub const MODULE_NOT_FOUND: Code = Code::new("N0205");

    // Types
    pub const TYPE_MISMATCH: Code = Code::new("N0300");
    pub const CONDITION_NOT_BOOL: Code = Code::new("N0301");
    pub const BRANCH_TYPES_DIFFER: Code = Code::new("N0302");
    pub const WRONG_ARGUMENT_COUNT: Code = Code::new("N0303");
    pub const INVALID_OPERANDS: Code = Code::new("N0304");
    pub const UNKNOWN_TYPE: Code = Code::new("N0305");

    // Mutation
    pub const ASSIGN_TO_IMMUTABLE: Code = Code::new("N0310");
    pub const MUTABLE_PARAMETER: Code = Code::new("N0311");

    /// `break` or `continue` outside a loop.
    pub const NOT_IN_A_LOOP: Code = Code::new("N0312");
    /// A statement that provably cannot run.
    pub const UNREACHABLE_STATEMENT: Code = Code::new("N0313");
    /// `break` or `continue` where a value is expected.
    pub const CONTROL_FLOW_AS_VALUE: Code = Code::new("N0314");

    /// Runaway recursion, caught before the stack is exhausted.
    pub const CALL_DEPTH_LIMIT: Code = Code::new("N0403");

    // Execution
    pub const INTEGER_OVERFLOW: Code = Code::new("N0400");
    pub const DIVISION_BY_ZERO: Code = Code::new("N0401");
    /// The interpreter's iteration guard, not a language rule. See `docs/spec.md`.
    pub const ITERATION_LIMIT: Code = Code::new("N0402");
    /// The interpreter could not start. Not the program's fault, and said so plainly
    /// rather than reported as a failure of the code being run.
    pub const INTERPRETER_UNAVAILABLE: Code = Code::new("N0404");

    /// An index outside a value's bounds: `str_byte` past the end, a slice whose range
    /// is inverted or runs off either end. A *language* rule, unlike `N0402`/`N0403`:
    /// `docs/spec.md` specifies the failure, so every implementation must produce it.
    pub const INDEX_OUT_OF_BOUNDS: Code = Code::new("N0405");

    /// The allocator refused. Reported where an implementation can detect it: a compiled
    /// program checks what `malloc` returned. The interpreter cannot produce this,
    /// because its host aborts the process on allocation failure — a stated difference
    /// between the two, not a gap in the specification.
    pub const ALLOCATION_FAILED: Code = Code::new("N0406");

    /// A file could not be read or written. The path is in the help, not the message,
    /// so the two implementations report the same sentence.
    pub const IO_FAILED: Code = Code::new("N0407");

    /// `spawn` outside a `scope`.
    pub const SPAWN_OUTSIDE_SCOPE: Code = Code::new("N0320");
    /// A value that may not cross into a task: an `Ints` or a `Strs`, which are handles
    /// to unsynchronised shared mutable storage.
    pub const ILLEGAL_TASK_CAPTURE: Code = Code::new("N0321");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
}

/// A span with something to say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// An edit that would resolve the diagnostic.
///
/// Machine-applicable: `replace this span with this text`. Not advice in prose, which a
/// tool cannot act on without guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// The span the error is *about*. Always present: a diagnostic without a position
    /// makes the reader search, which is the cost this design exists to remove.
    pub primary: Label,
    /// Additional spans that explain it — a previous definition, a conflicting branch.
    pub secondary: Vec<Label>,
    /// Why, or what to do. Prose, and optional.
    pub help: Option<String>,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(code: Code, span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code,
            severity: Severity::Error,
            primary: Label {
                span,
                message: message.clone(),
            },
            message,
            secondary: Vec::new(),
            help: None,
            fixes: Vec::new(),
        }
    }

    /// Give the primary span its own, narrower label.
    #[must_use]
    pub fn primary_label(mut self, message: impl Into<String>) -> Self {
        self.primary.message = message.into();
        self
    }

    #[must_use]
    pub fn secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    #[must_use]
    pub fn fix(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.fixes.push(Fix {
            span,
            replacement: replacement.into(),
            description: description.into(),
        });
        self
    }

    /// The machine-readable form. One JSON object per diagnostic.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// The human-readable form: message, location, source line, and a caret.
    #[must_use]
    pub fn render(&self, file: &SourceFile) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let at = file.line_col(self.primary.span.start);

        let _ = writeln!(out, "error[{}]: {}", self.code, self.message);
        // `name_at`, not `name`: a merged multi-file buffer must name the file a reader
        // would open, and for a single-file one the two are the same.
        let _ = writeln!(
            out,
            "  --> {}:{}:{}",
            file.name_at(self.primary.span.start),
            at.line,
            at.column
        );

        let line = file.line_text(self.primary.span.start);
        let gutter = at.line.to_string();
        let pad = " ".repeat(gutter.len());

        let _ = writeln!(out, "{pad} |");
        let _ = writeln!(out, "{gutter} | {line}");

        // The caret spans the offending text, clamped to the line so a multi-line span
        // does not draw a caret across the whole file.
        let caret_col = at.column.saturating_sub(1) as usize;
        let width = usize::try_from(self.primary.span.len()).unwrap_or(1).max(1);
        let remaining = line.chars().count().saturating_sub(caret_col).max(1);
        let _ = writeln!(
            out,
            "{pad} | {}{} {}",
            " ".repeat(caret_col),
            "^".repeat(width.min(remaining)),
            self.primary.message
        );

        for s in &self.secondary {
            let sat = file.line_col(s.span.start);
            // A secondary in *another* file names it. Without this, "declared here" in a
            // multi-file program gives a line number with no file to look it up in —
            // and silently means the wrong file's line.
            let here = file.name_at(self.primary.span.start);
            let there = file.name_at(s.span.start);
            let where_ = if here == there {
                format!("{}:{}", sat.line, sat.column)
            } else {
                format!("{there}:{}:{}", sat.line, sat.column)
            };
            let _ = writeln!(out, "{pad} = at {where_}: {}", s.message);
        }
        if let Some(h) = &self.help {
            let _ = writeln!(out, "{pad} = help: {h}");
        }
        for f in &self.fixes {
            let _ = writeln!(out, "{pad} = fix: {} -> `{}`", f.description, f.replacement);
        }
        out
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn a_diagnostic_round_trips_as_json() {
        // The machine-readable form is the contract; the rendering is a view of it.
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(4, 5),
            "`x` is not defined",
        )
        .help("declare it with `let x = ...;` first")
        .fix(Span::new(4, 5), "y", "did you mean `y`?");
        let json = d.to_json().unwrap();
        assert!(json.contains("N0200"), "{json}");
        assert_eq!(serde_json::from_str::<Diagnostic>(&json).unwrap(), d);
    }

    #[test]
    fn rendering_points_at_the_offending_text() {
        let file = SourceFile::new("x.nz", "fn main() -> Int {\n  qux\n}");
        let at = u32::try_from(file.text().find("qux").unwrap()).unwrap();
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(at, at + 3),
            "`qux` is not defined",
        );
        let out = d.render(&file);

        assert!(out.contains("error[N0200]"), "{out}");
        assert!(out.contains("x.nz:2:3"), "{out}");
        assert!(out.contains("  qux"), "{out}");
        assert!(out.contains("^^^"), "the caret must cover the name: {out}");
    }

    #[test]
    fn every_code_is_distinct() {
        // A reused code means two different failures are indistinguishable to anything
        // matching on it, which is the one thing codes exist to prevent.
        let all = [
            codes::UNKNOWN_CHARACTER,
            codes::UNTERMINATED_STRING,
            codes::INTEGER_TOO_LARGE,
            codes::UNEXPECTED_TOKEN,
            codes::UNSUPPORTED_CONSTRUCT,
            codes::UNDEFINED_VARIABLE,
            codes::UNDEFINED_FUNCTION,
            codes::ALREADY_DEFINED_IN_SCOPE,
            codes::DUPLICATE_FUNCTION,
            codes::MISSING_MAIN,
            codes::TYPE_MISMATCH,
            codes::CONDITION_NOT_BOOL,
            codes::BRANCH_TYPES_DIFFER,
            codes::WRONG_ARGUMENT_COUNT,
            codes::INVALID_OPERANDS,
            codes::UNKNOWN_TYPE,
            codes::ASSIGN_TO_IMMUTABLE,
            codes::MUTABLE_PARAMETER,
            codes::NOT_IN_A_LOOP,
            codes::UNREACHABLE_STATEMENT,
            codes::CONTROL_FLOW_AS_VALUE,
            codes::INTEGER_OVERFLOW,
            codes::DIVISION_BY_ZERO,
            codes::ITERATION_LIMIT,
            codes::CALL_DEPTH_LIMIT,
        ];
        let mut seen: Vec<&str> = all.iter().map(Code::as_str).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "a diagnostic code is used twice");
    }
}
