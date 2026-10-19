//! `docs/user/grammar.ebnf`, checked against the parser it describes.
//!
//! A grammar that can drift from the implementation is worse than no grammar: it is a
//! document a reader trusts and a maintainer forgets. Three checks, each mechanical:
//!
//! - **Terminals.** Every quoted terminal in the grammar is a real token, and every
//!   token the lexer has appears in the grammar. Adding a keyword to the lexer without
//!   writing it down fails here.
//! - **Structure.** Every nonterminal referenced is defined, and every one defined is
//!   reachable from `program`.
//! - **Examples.** Every rule carries at least one `@example`, and every example is fed
//!   to the real parser and must parse with no diagnostics.
//!
//! What this does not do is prove the parser accepts exactly this language — that needs
//! a generated parser, and this one is written by hand for the sake of its diagnostics.
//! The terminals are exact, the examples are executable, and the shape is reviewed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// One rule: what it is made of, and the programs that demonstrate it.
#[derive(Debug, Default)]
struct Rule {
    body: String,
    examples: Vec<String>,
    line: usize,
}

fn grammar_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("docs/user/grammar.ebnf")
}

/// Read the grammar, pairing each rule with the examples written above it.
fn rules() -> BTreeMap<String, Rule> {
    let path = grammar_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let mut rules: BTreeMap<String, Rule> = BTreeMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut in_comment = false;

    for (n, line) in text.lines().enumerate() {
        let trimmed = line.trim();

        // Comments carry the examples, so they are read rather than stripped. A comment
        // may span lines; only the `@example` marker is looked for inside one.
        if in_comment {
            if let Some(rest) = trimmed.strip_prefix("* @example ") {
                pending.push(rest.trim_end_matches("*)").trim().to_owned());
            }
            if trimmed.ends_with("*)") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("(*") {
            if let Some(rest) = trimmed.strip_prefix("(* @example ") {
                pending.push(rest.trim_end_matches("*)").trim().to_owned());
            }
            if !trimmed.ends_with("*)") {
                in_comment = true;
            }
            continue;
        }
        // A blank line ends the attachment, so a comment separated from a rule by one
        // belongs to neither — which is how the file's own header stays a header.
        if trimmed.is_empty() {
            if current.is_none() {
                pending.clear();
            }
            continue;
        }

        // `name = …` opens a rule; everything up to the `;` is its body.
        if current.is_none() {
            let Some((name, rest)) = trimmed.split_once('=') else {
                panic!(
                    "{}:{}: not a rule and not a comment: {trimmed}",
                    path.display(),
                    n + 1
                );
            };
            let name = name.trim().to_owned();
            assert!(
                !rules.contains_key(&name),
                "{}:{}: `{name}` is defined twice",
                path.display(),
                n + 1
            );
            rules.insert(
                name.clone(),
                Rule {
                    body: rest.to_owned(),
                    examples: std::mem::take(&mut pending),
                    line: n + 1,
                },
            );
            current = Some(name);
        } else if let Some(rule) = current.as_ref().and_then(|n| rules.get_mut(n)) {
            rule.body.push(' ');
            rule.body.push_str(trimmed);
        }

        if trimmed.ends_with(';') {
            current = None;
        }
    }
    assert!(
        current.is_none(),
        "the last rule is not terminated with `;`"
    );
    assert!(
        pending.is_empty(),
        "examples were written above nothing: {pending:?}"
    );
    rules
}

/// Quoted terminals, bare nonterminals and UPPERCASE token classes in one body.
fn symbols(body: &str) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let mut terminals = BTreeSet::new();
    let mut nonterminals = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut chars = body.chars().peekable();
    let mut word = String::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut t = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                t.push(c);
            }
            terminals.insert(t);
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            classify(&word, &mut nonterminals, &mut classes);
            word.clear();
        }
    }
    if !word.is_empty() {
        classify(&word, &mut nonterminals, &mut classes);
    }
    (terminals, nonterminals, classes)
}

fn classify(word: &str, nonterminals: &mut BTreeSet<String>, classes: &mut BTreeSet<String>) {
    if word.chars().all(|c| c.is_ascii_uppercase()) {
        classes.insert(word.to_owned());
    } else {
        nonterminals.insert(word.to_owned());
    }
}

#[test]
fn every_token_the_lexer_has_is_written_in_the_grammar() {
    use nazm_syntax::TokenKind;

    let rules = rules();
    let mut written = BTreeSet::new();
    for rule in rules.values() {
        let (terminals, _, classes) = symbols(&rule.body);
        written.extend(terminals);
        written.extend(classes);
    }

    // Each quoted terminal must lex to exactly one token. A terminal that lexes to two
    // is a grammar writing `->` as `-` and `>`, which reads the same and is not.
    for t in &written {
        if t.chars().all(|c| c.is_ascii_uppercase()) {
            continue;
        }
        let (tokens, errors) = nazm_syntax::lex(t);
        assert!(
            errors.is_empty(),
            "the terminal {t:?} does not lex: {errors:#?}"
        );
        let real: Vec<_> = tokens.iter().filter(|k| k.kind != TokenKind::Eof).collect();
        assert_eq!(real.len(), 1, "the terminal {t:?} is {} tokens", real.len());
    }

    // And the other direction, which is the one that catches drift: a token added to
    // the lexer and never written down.
    let mut missing = Vec::new();
    for kind in TokenKind::ALL {
        match kind.written() {
            Some(spelling) => {
                if !written.contains(spelling) {
                    missing.push(spelling.to_owned());
                }
            }
            // The three classes, named in the grammar by the words below rather than by
            // a spelling they do not have.
            None => {
                let class = match kind {
                    TokenKind::Int(_) => "INTEGER",
                    TokenKind::Float(_) => "FLOAT",
                    TokenKind::Str(_) => "STRING",
                    TokenKind::FStr(_) => "FSTRING",
                    TokenKind::Ident(_) => "IDENT",
                    other => panic!("{other:?} has no spelling and is not a class"),
                };
                if !written.contains(class) {
                    missing.push(class.to_owned());
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these tokens exist in the lexer and not in docs/user/grammar.ebnf: {missing:?}"
    );
}

#[test]
fn every_rule_is_defined_and_reachable() {
    let rules = rules();
    assert!(rules.contains_key("program"), "there is no start symbol");

    for (name, rule) in &rules {
        let (_, nonterminals, _) = symbols(&rule.body);
        for n in nonterminals {
            assert!(
                rules.contains_key(&n),
                "line {}: `{name}` refers to `{n}`, which is not defined",
                rule.line
            );
        }
    }

    // Reachability from the start symbol, not just "referenced by something": two dead
    // rules that refer to each other are referenced and still dead.
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut work = vec!["program".to_owned()];
    while let Some(name) = work.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        if let Some(rule) = rules.get(&name) {
            let (_, nonterminals, _) = symbols(&rule.body);
            work.extend(nonterminals.into_iter().filter(|n| rules.contains_key(n)));
        }
    }
    let unreachable: Vec<&String> = rules.keys().filter(|k| !reached.contains(*k)).collect();
    assert!(
        unreachable.is_empty(),
        "these rules cannot be reached from `program`: {unreachable:?}"
    );
}

#[test]
fn the_examples_between_them_use_every_token() {
    // One example per rule is the weaker requirement, and a rule like `statement` has
    // nine alternatives. Requiring the examples to *collectively* use every token the
    // language has is what makes an undemonstrated alternative visible: each statement
    // form and each operator has a token nothing else would produce.
    use nazm_syntax::TokenKind;

    let mut used: BTreeSet<String> = BTreeSet::new();
    for rule in rules().values() {
        for example in &rule.examples {
            let (tokens, errors) = nazm_syntax::lex(example);
            assert!(errors.is_empty(), "{example:?}: {errors:#?}");
            for t in tokens {
                if t.kind == TokenKind::Eof {
                    continue;
                }
                used.insert(match &t.kind {
                    TokenKind::Int(_) => "INTEGER".to_owned(),
                    TokenKind::Float(_) => "FLOAT".to_owned(),
                    TokenKind::Str(_) => "STRING".to_owned(),
                    TokenKind::FStr(_) => "FSTRING".to_owned(),
                    TokenKind::Ident(_) => "IDENT".to_owned(),
                    other => other.written().unwrap_or("?").to_owned(),
                });
            }
        }
    }

    let mut missing = Vec::new();
    for kind in TokenKind::ALL {
        let want = match kind {
            TokenKind::Int(_) => "INTEGER".to_owned(),
            TokenKind::Float(_) => "FLOAT".to_owned(),
            TokenKind::Str(_) => "STRING".to_owned(),
            TokenKind::FStr(_) => "FSTRING".to_owned(),
            TokenKind::Ident(_) => "IDENT".to_owned(),
            other => other.written().unwrap_or("?").to_owned(),
        };
        if !used.contains(&want) {
            missing.push(want);
        }
    }
    assert!(
        missing.is_empty(),
        "no @example in docs/user/grammar.ebnf uses these tokens, so the grammar describes \
         forms nothing demonstrates: {missing:?}"
    );
}

#[test]
fn every_example_in_the_grammar_parses() {
    let rules = rules();
    let mut checked = 0usize;
    for (name, rule) in &rules {
        assert!(
            !rule.examples.is_empty(),
            "line {}: `{name}` has no @example, so nothing demonstrates it",
            rule.line
        );
        for example in &rule.examples {
            let (program, errors) = nazm_syntax::parse_source(example);
            assert!(
                errors.is_empty(),
                "line {}: `{name}`'s example does not parse:\n  {example}\n  {errors:#?}",
                rule.line
            );
            assert!(
                !program.functions.is_empty(),
                "line {}: `{name}`'s example parsed to nothing:\n  {example}",
                rule.line
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 20,
        "only {checked} examples — too few to mean much"
    );
}
