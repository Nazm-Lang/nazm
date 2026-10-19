//! MIR's printed form, `nazm.mir/1`: deterministic JSON for tests, debugging and regression
//! comparison. Internal and unstable, debug-only, and never read back — a reader of it gets no
//! promise that a later build prints the same (`docs/internal/architecture.md` §7.42).
//!
//! Statements and terminators are the canonical text [`crate::text`] also digests, so the
//! printed program and the digest cannot disagree about what a function does. Spans are left out
//! unless a caller supplies a way to name one.

use std::fmt::Write as _;

use nazm_span::Span;

use crate::ir::{BlockOrigin, LocalOrigin, Program};
use crate::{digest, text, SCHEMA};

/// How to print.
#[allow(missing_debug_implementations)]
pub struct Options<'a> {
    /// Name a span, as `file:line:column`, or leave spans out.
    pub span: Option<&'a dyn Fn(Span) -> String>,
    /// One line rather than indented.
    pub compact: bool,
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The whole program.
#[must_use]
pub fn print(program: &Program, options: &Options<'_>) -> String {
    let (nl, ind) = if options.compact {
        ("", "")
    } else {
        ("\n", "  ")
    };
    let mut s = String::new();
    let _ = write!(s, "{{{nl}{ind}\"schema\": {},{nl}", escape(SCHEMA));
    let entry = program.entry.and_then(|e| program.function(e)).map_or_else(
        || "null".to_owned(),
        |f| escape(&text::identity(&f.identity)),
    );
    let _ = write!(s, "{ind}\"entry\": {entry},{nl}");
    let _ = write!(s, "{ind}\"functions\": [");
    for (i, f) in program.functions.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{nl}{ind}{ind}{{");
        let _ = write!(s, "\"id\": {}, ", f.id.0);
        let _ = write!(
            s,
            "\"identity\": {}, ",
            escape(&text::identity(&f.identity))
        );
        let _ = write!(s, "\"name\": {}, ", escape(&f.name));
        let effects: Vec<String> = f.effects.iter().map(|e| escape(e.name())).collect();
        let _ = write!(s, "\"effects\": [{}], ", effects.join(", "));
        if let Some(sym) = &f.foreign {
            let _ = write!(s, "\"foreign\": {}, ", escape(sym));
        }
        let _ = write!(s, "\"params\": {}, ", f.params);
        let _ = write!(s, "\"ret\": {}, ", escape(&text::ty(program, f.ret)));
        let _ = write!(
            s,
            "\"digest\": {}, ",
            escape(&digest::executable(program, f))
        );
        if let Some(name) = options.span {
            let _ = write!(s, "\"at\": {}, ", escape(&name(f.span)));
        }
        let _ = write!(s, "\"locals\": [");
        for (j, l) in f.locals.iter().enumerate() {
            if j > 0 {
                s.push_str(", ");
            }
            let origin = match l.origin {
                LocalOrigin::Param => "param",
                LocalOrigin::Binding { mutable: true } => "binding mut",
                LocalOrigin::Binding { mutable: false } => "binding",
                LocalOrigin::Temp => "temp",
            };
            let _ = write!(
                s,
                "{{\"local\": \"_{j}\", \"ty\": {}, \"name\": {}, \"origin\": \"{origin}\"}}",
                escape(&text::local_ty(program, l.ty)),
                escape(&l.name)
            );
        }
        let _ = write!(s, "], \"blocks\": [");
        for (j, b) in f.blocks.iter().enumerate() {
            if j > 0 {
                s.push_str(", ");
            }
            let origin = match b.origin {
                BlockOrigin::Source => "source",
                BlockOrigin::FailureCleanup => "failure-cleanup",
                BlockOrigin::Unwind => "unwind",
            };
            let _ = write!(
                s,
                "{nl}{ind}{ind}{ind}{{\"block\": \"bb{j}\", \"origin\": \"{origin}\", \"stmts\": ["
            );
            for (k, st) in b.stmts.iter().enumerate() {
                if k > 0 {
                    s.push_str(", ");
                }
                let body = text::statement(program, &st.kind);
                match options.span {
                    Some(name) if st.at != Span::synthetic() => {
                        let _ = write!(s, "{}", escape(&format!("{body} @ {}", name(st.at))));
                    }
                    _ => {
                        let _ = write!(s, "{}", escape(&body));
                    }
                }
            }
            let _ = write!(
                s,
                "], \"term\": {}}}",
                escape(&text::terminator(program, &b.term))
            );
        }
        let _ = write!(s, "]}}");
    }
    let _ = write!(s, "{nl}{ind}]{nl}}}{nl}");
    s
}
