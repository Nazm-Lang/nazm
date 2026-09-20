//! Lexing and parsing for Nazm v0.1.

pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{BinOp, Block, Expr, Function, Let, Param, Program, TypeName};
pub use lexer::{lex, Token, TokenKind};
pub use parser::parse;

/// Lex and parse in one step, collecting every diagnostic from both.
///
/// Parsing proceeds even when lexing found errors, so a file with one bad character still
/// reports its type errors. One diagnostic per compile is the loop this avoids.
#[must_use]
pub fn parse_source(text: &str) -> (Program, Vec<nazm_diag::Diagnostic>) {
    let (tokens, mut errors) = lex(text);
    let (program, parse_errors) = parse(&tokens);
    errors.extend(parse_errors);
    (program, errors)
}
