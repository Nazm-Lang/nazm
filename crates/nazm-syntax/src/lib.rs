//! Lexing and parsing for Nazm.
//!
//! One lossless scan ([`lexer::lex_lossless_in`]) feeds one parser, which builds two
//! trees from the same decisions: the abstract [`Program`] the checker reads and the
//! lossless [`cst::Cst`] that keeps every byte of the file. [`parse_file`] returns the
//! first, [`parse_cst`] the second; both come from [`parse_both`].

pub mod ast;
pub mod cst;
pub mod format;
pub mod lexer;
pub mod parser;

pub use ast::{
    Assign, BinOp, Block, EffectDecl, Enum, Expr, Field, FieldInit, ForeignDecl, Function,
    HoleSite, Import, Let, MatchArm, Param, PatternField, Place, PlaceField, Program, Qualifier,
    Record, Requirement, Return, Scope, Spawn, Stmt, TypeName, TypeParam, Variant, While,
};
pub use cst::{Cst, CstNode, CstParse, Leaf, SyntaxKind};
pub use format::format;
pub use lexer::{
    lex, lex_in, lex_lossless_in, lex_with_comments, lex_with_comments_in, Lexeme, Token, TokenKind,
};
pub use parser::{MAX_NESTING, PARSE_STACK_BYTES};

/// Lex and parse in one step, collecting every diagnostic from both.
///
/// Parsing proceeds even when lexing found errors, so a file with one bad character still
/// reports its type errors. One diagnostic per compile is the loop this avoids.
#[must_use]
/// # Stack
///
/// Parsing recurses with the shape of the tree, and so does dropping it afterwards. The
/// caller must provide [`PARSE_STACK_BYTES`]; the default 2 MiB a spawned thread gets is
/// not enough for a tree at [`MAX_NESTING`], and there is no way to report running out.
/// `nazm_core::run` and `nazm_core::check_source` both arrange this.
pub fn parse_source(text: &str) -> (Program, Vec<nazm_diag::Diagnostic>) {
    parse_file(nazm_span::FileId::ROOT, text)
}

/// Parse one file, so every span it produces names that file.
pub fn parse_file(file: nazm_span::FileId, text: &str) -> (Program, Vec<nazm_diag::Diagnostic>) {
    let (program, _, errors) = parse_both(file, text);
    (program, errors)
}

/// Parse one file into its lossless concrete tree.
///
/// The same scan, the same parser and the same diagnostics as [`parse_file`]; the
/// abstract tree is built and dropped. The tree reproduces `text` exactly
/// ([`Cst::text`]) whatever the diagnostics say, including for text that does not lex.
///
/// # Stack
///
/// As [`parse_source`]: the caller provides [`PARSE_STACK_BYTES`].
pub fn parse_cst(file: nazm_span::FileId, text: &str) -> CstParse {
    let (_, cst, diagnostics) = parse_both(file, text);
    CstParse { cst, diagnostics }
}

/// Both trees of one file, and every lexical and syntactic diagnostic, lexical first.
///
/// Parsing proceeds even when lexing found errors, so a file with one bad character still
/// reports its type errors.
pub fn parse_both(
    file: nazm_span::FileId,
    text: &str,
) -> (Program, Cst, Vec<nazm_diag::Diagnostic>) {
    parse_both_at(file, text, None)
}

/// As [`parse_both`], for a completion probe: a hole at byte `hole` of this file, if the
/// grammar has one there (N26, [`HoleSite`]). The hole is the one unfinished construct the
/// probe tolerates; everything else is parsed and reported exactly as [`parse_both`] does.
pub fn parse_both_at(
    file: nazm_span::FileId,
    text: &str,
    hole: Option<u32>,
) -> (Program, Cst, Vec<nazm_diag::Diagnostic>) {
    let (lexemes, mut errors) = lex_lossless_in(file, text);
    let tokens: Vec<&Token> = lexemes
        .iter()
        .filter_map(|l| match l {
            Lexeme::Token(t) => Some(t),
            _ => None,
        })
        .collect();
    let (program, parse_errors, cst) = parser::parse_lossless(file, text, &lexemes, &tokens, hole);
    errors.extend(parse_errors);
    (program, cst, errors)
}

/// Parse every file of a compilation, in load order.
///
/// # One program, still
///
/// The functions of all files land in one [`Program`], which is what makes a call from
/// one file to another resolve — the same thing concatenating the files used to achieve.
/// What has changed is that **each file is parsed on its own text**, so a span is a
/// position in a file a reader can open rather than a position in a string that only ever
/// existed inside the compiler. That is the difference separate compilation eventually
/// needs, and it is why this exists rather than a `SourceMap::merged_text()`.
///
/// Order is the map's order, which is the loader's: root first, then imports depth-first
/// in source order. Two functions with the same name are a duplicate wherever they are,
/// exactly as before.
pub fn parse_map(map: &nazm_span::SourceMap) -> (Program, Vec<nazm_diag::Diagnostic>) {
    let (program, errors, _) = parse_map_at(map, None);
    (program, errors)
}

/// As [`parse_map`], for a completion probe (N26): the root file parsed with a hole at byte
/// `hole`, if the grammar has one there, and its concrete tree returned beside the rest.
pub fn parse_map_at(
    map: &nazm_span::SourceMap,
    hole: Option<u32>,
) -> (Program, Vec<nazm_diag::Diagnostic>, Option<Cst>) {
    let mut functions = Vec::new();
    let mut records = Vec::new();
    let mut enums = Vec::new();
    let mut imports = Vec::new();
    let mut errors = Vec::new();
    let mut found = None;
    let mut root = None;
    for id in map.ids() {
        let at = if id == nazm_span::FileId::ROOT {
            hole
        } else {
            None
        };
        let (program, cst, file_errors) = parse_both_at(id, map.text(id), at);
        if id == nazm_span::FileId::ROOT && hole.is_some() {
            found = program.hole;
            root = Some(cst);
        }
        functions.extend(program.functions);
        records.extend(program.records);
        enums.extend(program.enums);
        imports.extend(program.imports);
        errors.extend(file_errors);
    }
    (
        Program {
            functions,
            records,
            enums,
            imports,
            hole: found,
        },
        errors,
        root,
    )
}
