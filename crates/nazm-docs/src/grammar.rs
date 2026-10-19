//! `docs/user/grammar.ebnf` as sections: one per production, not Markdown.
//!
//! The file is a header comment and then a sequence of rules, each a production
//! `name = … ;` with the comments written directly above it — no blank line between —
//! attached to it. That attachment is the one `crates/nazm-syntax/tests/grammar.rs` and
//! `xtask`'s guide generator already read: an `@example` belongs to the rule below it. A
//! comment a blank line separates from every rule after the header is refused, because it
//! would belong to no section.
//!
//! Nothing here reads a production's meaning. What is recorded is structural: its exact
//! text, the examples attached to it, and the nonterminals it names — a grammar reference,
//! not a semantic dependency.

use crate::Problem;

/// One production and the comments attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) name: String,
    /// The block: its attached comments and its production, through the line with `;`.
    pub(crate) block: (usize, usize),
    /// The production alone, `name` through `;`.
    pub(crate) production: (usize, usize),
    /// The line the block starts on, counted from zero.
    pub(crate) line: usize,
    /// Each `@example`, exactly as written after the marker.
    pub(crate) examples: Vec<String>,
    /// The nonterminals the production names, in first-mention order, without repeats.
    pub(crate) references: Vec<String>,
}

/// The header's end and every rule, in source order — or every problem.
pub(crate) fn rules(document: &str, text: &str) -> Result<(usize, Vec<Rule>), Vec<Problem>> {
    let mut problems = Vec::new();
    let mut rules: Vec<Rule> = Vec::new();
    let mut header_end: Option<usize> = None;
    // Comments since the last blank line or rule: where they start, and their examples.
    let mut attached: Option<(usize, usize)> = None;
    let mut examples: Vec<String> = Vec::new();
    let mut in_comment = false;
    // The rule being read: its name, where its block and production start, and its text.
    let mut open: Option<(String, usize, usize, usize, String)> = None;
    let mut in_quote = false;
    let mut offset = 0usize;

    for (line, raw) in text.split_inclusive('\n').enumerate() {
        let here = offset;
        offset += raw.len();
        let l = raw.trim_end_matches(['\n', '\r']);
        let trimmed = l.trim();

        if let Some((name, block, production, first_line, mut body)) = open.take() {
            // Inside a production: find its `;` outside a quoted terminal.
            match terminator(l, &mut in_quote) {
                Some(Ok(at)) => {
                    body.push_str(l.get(..at).unwrap_or_default());
                    let end = here + at + 1;
                    rules.push(finish(
                        name,
                        block,
                        (production, end),
                        offset,
                        first_line,
                        &body,
                        &mut examples,
                    ));
                }
                Some(Err(())) => problems.push(Problem::at("grammar_syntax", document, line, l)),
                None => {
                    body.push_str(l);
                    body.push('\n');
                    open = Some((name, block, production, first_line, body));
                }
            }
            continue;
        }

        if in_comment {
            example(trimmed, &mut examples);
            if trimmed.ends_with("*)") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("(*") {
            if attached.is_none() {
                attached = Some((here, line));
            }
            example(trimmed, &mut examples);
            if !trimmed.ends_with("*)") {
                in_comment = true;
            }
            continue;
        }
        if trimmed.is_empty() {
            if let Some((_, at)) = attached.take() {
                if rules.is_empty() && header_end.is_none() {
                    header_end = Some(offset);
                } else {
                    problems.push(Problem::at("detached_comment", document, at, ""));
                }
                examples.clear();
            }
            continue;
        }

        // A production starts: `name = …`.
        let Some((name, rest)) = l.split_once('=') else {
            problems.push(Problem::at("grammar_syntax", document, line, l));
            continue;
        };
        let name = name.trim();
        if !is_nonterminal(name) || l.starts_with(char::is_whitespace) {
            problems.push(Problem::at("grammar_syntax", document, line, l));
            continue;
        }
        let (block, first_line) = attached.take().unwrap_or((here, line));
        let rest_at = l.len() - rest.len();
        in_quote = false;
        match terminator(rest, &mut in_quote) {
            Some(Ok(at)) => {
                let end = here + rest_at + at + 1;
                let body = rest.get(..at).unwrap_or_default().to_owned();
                rules.push(finish(
                    name.to_owned(),
                    block,
                    (here, end),
                    offset,
                    first_line,
                    &body,
                    &mut examples,
                ));
            }
            Some(Err(())) => problems.push(Problem::at("grammar_syntax", document, line, l)),
            None => {
                open = Some((
                    name.to_owned(),
                    block,
                    here,
                    first_line,
                    format!("{rest}\n"),
                ));
            }
        }
    }
    if open.is_some() || in_comment {
        problems.push(Problem::of("grammar_syntax", document));
    }
    if let Some((_, at)) = attached {
        problems.push(Problem::at("detached_comment", document, at, ""));
    }

    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    for (i, r) in rules.iter().enumerate() {
        if names.iter().take(i).any(|n| *n == r.name) {
            problems.push(Problem::at("duplicate_id", document, r.line, &r.name));
        }
        for reference in &r.references {
            if !names.contains(&reference.as_str()) {
                problems.push(Problem::at(
                    "undefined_reference",
                    document,
                    r.line,
                    reference,
                ));
            }
        }
    }
    if problems.is_empty() {
        let first = rules.first().map_or(text.len(), |r| r.block.0);
        Ok((header_end.unwrap_or(first).min(first), rules))
    } else {
        Err(problems)
    }
}

fn finish(
    name: String,
    block: usize,
    production: (usize, usize),
    line_end: usize,
    line: usize,
    body: &str,
    examples: &mut Vec<String>,
) -> Rule {
    Rule {
        references: references(&name, body),
        name,
        block: (block, line_end),
        production,
        line,
        examples: std::mem::take(examples),
    }
}

/// Where `;` ends the production in `text`, outside a quoted terminal; `Err` if anything
/// but whitespace follows it, or a comment opens inside the production.
fn terminator(text: &str, in_quote: &mut bool) -> Option<Result<usize, ()>> {
    let mut previous = '\0';
    for (i, c) in text.char_indices() {
        if c == '"' {
            *in_quote = !*in_quote;
        } else if !*in_quote {
            if previous == '(' && c == '*' {
                return Some(Err(()));
            }
            if c == ';' {
                let after = text.get(i + 1..).unwrap_or_default();
                return Some(if after.trim().is_empty() {
                    Ok(i)
                } else {
                    Err(())
                });
            }
        }
        previous = c;
    }
    None
}

/// An `@example` on a comment line, exactly as the guide generator reads it.
fn example(trimmed: &str, examples: &mut Vec<String>) {
    let rest = trimmed
        .strip_prefix("(* @example ")
        .or_else(|| trimmed.strip_prefix("* @example "));
    if let Some(rest) = rest {
        examples.push(rest.trim_end_matches("*)").trim().to_owned());
    }
}

/// A nonterminal's name: lower-case letters, digits and `_`, starting with a letter.
fn is_nonterminal(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The nonterminals `body` names outside quoted terminals — upper-case names are token
/// classes, not rules — in first-mention order.
fn references(name: &str, body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut quoted = false;
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<String>| {
        if is_nonterminal(word) && word != name && !out.contains(word) {
            out.push(word.clone());
        }
        word.clear();
    };
    for c in body.chars() {
        if c == '"' {
            flush(&mut word, &mut out);
            quoted = !quoted;
        } else if !quoted && (c.is_ascii_alphanumeric() || c == '_') {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const G: &str = "(* header\n *)\n\n(* prose *)\n(* @example a b *)\na = b , \";\" , TOKEN ;\n\nb = \"x\"\n  | a ;\n";

    #[test]
    fn a_rule_is_its_attached_comments_and_its_production_exactly() {
        let (header, rules) = rules("g", G).unwrap();
        assert_eq!(&G[..header], "(* header\n *)\n\n");
        assert_eq!(rules.len(), 2);
        let a = &rules[0];
        assert_eq!(
            &G[a.block.0..a.block.1],
            "(* prose *)\n(* @example a b *)\na = b , \";\" , TOKEN ;\n"
        );
        assert_eq!(
            &G[a.production.0..a.production.1],
            "a = b , \";\" , TOKEN ;"
        );
        assert_eq!(a.examples, ["a b"]);
        assert_eq!(a.references, ["b"]);
        let b = &rules[1];
        assert_eq!(&G[b.production.0..b.production.1], "b = \"x\"\n  | a ;");
        assert_eq!(b.references, ["a"]);
    }

    #[test]
    fn broken_grammar_is_refused() {
        for (text, kind) in [
            ("a = b ;\n", "undefined_reference"),
            ("a = \"x\" ;\na = \"y\" ;\n", "duplicate_id"),
            ("a = \"x\" ; b\n", "grammar_syntax"),
            ("a = \"x\"\n", "grammar_syntax"),
            ("A = \"x\" ;\n", "grammar_syntax"),
            (
                "a = \"x\" ;\n\n(* stray *)\n\nb = a ;\n",
                "detached_comment",
            ),
        ] {
            let e = rules("g", text).unwrap_err();
            assert_eq!(e[0].kind, kind, "{text}");
        }
    }
}
