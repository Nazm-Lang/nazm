//! Lexing and parsing for Nazm v0.1.

pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{
    Assign, BinOp, Block, Expr, Function, Import, Let, Param, Program, Return, Scope, Spawn,
    Stmt, TypeName, While,
};
pub use lexer::{lex, Token, TokenKind};
pub use parser::{parse, MAX_NESTING, PARSE_STACK_BYTES};

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
    let (tokens, mut errors) = lex(text);
    let (program, parse_errors) = parse(&tokens);
    errors.extend(parse_errors);
    (program, errors)
}
