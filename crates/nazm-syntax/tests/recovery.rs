//! Recovery inside declarations and lists (N76, `architecture.md` §7.77).
//!
//! A malformed member of a delimited list — a record field, an enum variant or payload
//! field, a parameter, a call argument, a field initialiser, a `match` arm — is an error
//! node of its own; its siblings stay typed nodes, and a later independent mistake in the
//! same list is its own diagnostic. A broken `use` ends at its `;`. The abstract tree's
//! contract does not move: a declaration that needed recovery is absent, a function whose
//! body needed recovery keeps its signature and has no body.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_diag::codes;
use nazm_span::FileId;
use nazm_syntax::{parse_both, Cst, Program, SyntaxKind, PARSE_STACK_BYTES};

fn parse(text: &str) -> (Program, Cst, Vec<nazm_diag::Diagnostic>) {
    let text = text.to_owned();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || parse_both(FileId::ROOT, &text))
            .expect("spawn")
            .join()
            .expect("join")
    })
}

/// The kinds of a node's children that are nodes, in order.
fn child_kinds(cst: &Cst, kind: SyntaxKind) -> Vec<SyntaxKind> {
    let node = cst
        .nodes()
        .into_iter()
        .find(|n| n.kind() == kind)
        .unwrap_or_else(|| panic!("no {kind:?} node"));
    node.children().iter().map(|c| c.kind()).collect()
}

/// The text each diagnostic points at.
fn at<'a>(text: &'a str, d: &[nazm_diag::Diagnostic]) -> Vec<&'a str> {
    d.iter()
        .map(|d| &text[d.primary.span.start as usize..d.primary.span.end as usize])
        .collect()
}

#[test]
fn two_bad_fields_in_one_record_are_two_diagnostics_and_the_good_ones_stay_fields() {
    let text = "struct S { a: Int, 1: Int, b: Bool, c Str, d: Int, }\nfn main() -> Int { 0 }\n";
    let (program, cst, d) = parse(text);
    assert_eq!(at(text, &d), ["1", "Str"], "{d:?}");
    assert!(d.iter().all(|d| d.code == codes::UNEXPECTED_TOKEN));
    assert_eq!(
        child_kinds(&cst, SyntaxKind::FieldList),
        [
            SyntaxKind::Field,
            SyntaxKind::Error,
            SyntaxKind::Field,
            SyntaxKind::Error,
            SyntaxKind::Field
        ]
    );
    // The record is a node of its own kind, and not in the abstract tree.
    assert!(cst
        .nodes()
        .iter()
        .any(|n| n.kind() == SyntaxKind::RecordItem));
    assert!(program.records.is_empty());
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn a_bad_variant_and_a_bad_payload_field_are_each_their_own_diagnostic() {
    let text = "enum E { A, 2, B(x: Int, y), C, }\nfn main() -> Int { 0 }\n";
    let (program, cst, d) = parse(text);
    assert_eq!(at(text, &d), ["2", ")"], "{d:?}");
    assert_eq!(
        child_kinds(&cst, SyntaxKind::VariantList),
        [
            SyntaxKind::Variant,
            SyntaxKind::Error,
            SyntaxKind::Variant,
            SyntaxKind::Variant
        ]
    );
    assert!(program.enums.is_empty());
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn an_empty_payload_is_refused_and_the_variants_after_it_are_read() {
    let text = "enum E { A(), B(x: 1), }\n";
    let (_, _, d) = parse(text);
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(d[0].code, codes::UNSUPPORTED_CONSTRUCT);
    assert_eq!(at(text, &d)[1], "1");
}

#[test]
fn a_bad_parameter_leaves_the_function_out_and_its_body_still_reports() {
    let text = "fn f(a: Int, 1, b: Int) -> Int { let = 2; a }\nfn g() -> Int { 1 }\n";
    let (program, cst, d) = parse(text);
    assert_eq!(at(text, &d), ["1", "="], "{d:?}");
    assert_eq!(
        child_kinds(&cst, SyntaxKind::ParamList),
        [SyntaxKind::Param, SyntaxKind::Error, SyntaxKind::Param]
    );
    let names: Vec<&str> = program.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["g"]);
}

#[test]
fn bad_arguments_cost_the_argument_not_the_statement() {
    let text = "fn main() -> Int { let x = f(1 +, 2, 3 *); let y = 2; x }\n";
    let (program, cst, d) = parse(text);
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(at(text, &d), [",", ")"]);
    // The `let` is a `let`, its call a call; the arguments that parsed are expressions.
    assert!(cst.nodes().iter().any(|n| n.kind() == SyntaxKind::CallExpr));
    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|n| n.kind() == SyntaxKind::LetStmt)
            .count(),
        2
    );
    // The function keeps its signature and has no body.
    assert_eq!(program.functions.len(), 1);
    assert!(program.functions[0].body.is_none());
}

#[test]
fn bad_field_initialisers_cost_the_field() {
    let text = "fn main() -> Int { let p = P(x: , y: 2, z: 3 +); 0 }\n";
    let (_, cst, d) = parse(text);
    assert_eq!(at(text, &d), [",", ")"], "{d:?}");
    assert_eq!(
        child_kinds(&cst, SyntaxKind::FieldInitList),
        [SyntaxKind::Error, SyntaxKind::FieldInit, SyntaxKind::Error]
    );
}

#[test]
fn a_bad_arm_costs_the_arm() {
    let text = "fn main() -> Int { match e { E.A() => , E.B() => 2, 3 => 4, } }\n";
    let (_, cst, d) = parse(text);
    assert_eq!(at(text, &d), [",", "3"], "{d:?}");
    assert_eq!(
        child_kinds(&cst, SyntaxKind::MatchArmList),
        [SyntaxKind::Error, SyntaxKind::MatchArm, SyntaxKind::Error]
    );
}

#[test]
fn a_broken_import_ends_at_its_semicolon_and_the_next_import_is_read() {
    let text =
        "use 5 extra;\nuse \"b.nz\";\nuse \"c.nz\" as;\nuse \"d.nz\";\nfn main() -> Int { 0 }\n";
    let (program, _, d) = parse(text);
    assert_eq!(at(text, &d), ["5", ";"], "{d:?}");
    let paths: Vec<&str> = program.imports.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["b.nz", "d.nz"]);
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn a_list_that_never_closes_is_given_up_at_the_next_item() {
    // Recovery inside a list stops where no list can continue; the item recovers as
    // before, and the next item is intact.
    let text = "struct S { a: Int, b: 1\nfn main() -> Int { 0 }\n";
    let (program, cst, d) = parse(text);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(program.functions.len(), 1);
    assert!(cst.nodes().iter().any(|n| n.kind() == SyntaxKind::FnItem));
}

#[test]
fn a_member_left_unbalanced_mutes_what_follows_as_before() {
    // `(` left open in a parameter: the item's delimiters do not balance, so what it
    // reports after its first recovery is withheld as a cascade (§7.17).
    let text = "fn f(a: (Int, b: Int) -> Int { let = 1; a }\nfn g() -> Int { 1 }\n";
    let (_, _, d) = parse(text);
    assert_eq!(d.len(), 1, "{d:?}");
}

#[test]
fn every_recovered_tree_is_still_lossless() {
    for text in [
        "struct S { a: Int, 1: Int, b: Bool, c Str, d: Int, }",
        "enum E { A, 2, B(x: Int, y), C, }",
        "fn f(a: Int, 1, b: Int) -> Int { let = 2; a }",
        "fn main() -> Int { let x = f(1 +, 2, 3 *); x }",
        "use 5 extra;\nuse \"b.nz\";",
        "fn main() -> Int { match e { E.A() => , E.B() => 2, } }",
    ] {
        let (_, cst, _) = parse(text);
        assert_eq!(cst.text(), text);
    }
}
