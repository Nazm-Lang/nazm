//! Lexing and parsing for Nazm v0.1.

pub mod ast;
pub mod format;
pub mod lexer;
pub mod parser;

pub use ast::{
    Assign, BinOp, Block, Expr, Field, FieldInit, Function, Import, Let, Param, Place, PlaceField,
    Program, Record, Return, Scope, Spawn, Stmt, TypeName, While,
};
pub use format::format;
pub use lexer::{lex, lex_in, lex_with_comments, lex_with_comments_in, Token, TokenKind};
pub use parser::{parse, parse_in, MAX_NESTING, PARSE_STACK_BYTES};

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
    let (tokens, mut errors) = lex_in(file, text);
    let (program, parse_errors) = parse_in(file, &tokens);
    errors.extend(parse_errors);
    (program, errors)
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
    let mut functions = Vec::new();
    let mut records = Vec::new();
    let mut imports = Vec::new();
    let mut errors = Vec::new();
    for id in map.ids() {
        let (program, file_errors) = parse_file(id, map.text(id));
        functions.extend(program.functions);
        records.extend(program.records);
        imports.extend(program.imports);
        errors.extend(file_errors);
    }
    (
        Program {
            functions,
            records,
            imports,
        },
        errors,
    )
}
