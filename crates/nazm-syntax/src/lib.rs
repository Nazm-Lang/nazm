//! Lexing and parsing for Nazm.
//!
//! One lossless scan ([`lexer::lex_lossless_in`]) feeds one parser, which builds two
//! trees from the same decisions: the abstract [`Program`] the checker reads and the
//! lossless [`cst::Cst`] that keeps every byte of the file. [`parse_file`] returns the
//! first, [`parse_cst`] the second; both come from [`parse_both`].

pub mod ast;
pub mod cst;
pub mod format;
pub mod incremental;
pub mod lexer;
pub mod parser;

pub use ast::{
    Assign, BinOp, Block, Const, Contract, ContractKind, EffectDecl, Enum, Expr, Field, FieldInit,
    FnTypeName, For, ForIter, ForeignDecl, ForeignStruct, Function, HoleSite, ImplDecl, Import,
    InterpPart, Let, MatchArm, MatchSource, MethodOf, Param, Pattern, PatternField, Place,
    PlaceField, PlaceStep, Program, Qualifier, Record, Reexport, ReexportName, Requirement, Return,
    Scope, Spawn, Stmt, Trait, TraitMethod, TypeName, TypeParam, Variant, VariantPattern, While,
};
pub use cst::{Cst, CstNode, CstParse, Leaf, SyntaxKind};
pub use format::format;
pub use incremental::{EditError, ParseStats, Revision, TextEdit};
pub use lexer::{
    lex, lex_in, lex_lossless_in, lex_with_comments, lex_with_comments_in, Align, FPiece,
    FormatSpec, Lexeme, Token, TokenKind,
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
    let (program, errors, _) = parse_file_with_tokens(file, text);
    (program, errors)
}

/// As [`parse_file`], with the file's tokens — the ones [`lex_in`] gives, ending with
/// [`TokenKind::Eof`] — for a caller that reads them too (the loader, which looks for the
/// names a module uses).
///
/// The same scan and parse as [`parse_both`], without building the lossless tree it would
/// drop: building it was most of a parse's work, paid for every module of every compilation
/// (performance leadership, `nazm check`).
pub fn parse_file_with_tokens(
    file: nazm_span::FileId,
    text: &str,
) -> (Program, Vec<nazm_diag::Diagnostic>, Vec<Token>) {
    let (lexemes, mut errors) = lex_lossless_in(file, text);
    let tokens: Vec<&Token> = lexemes
        .iter()
        .filter_map(|l| match l {
            Lexeme::Token(t) => Some(t),
            _ => None,
        })
        .collect();
    let (program, parse_errors, _) = parser::parse_with(file, text, &lexemes, &tokens, None, false);
    errors.extend(parse_errors);
    drop(tokens);
    let tokens = lexemes
        .into_iter()
        .filter_map(|l| match l {
            Lexeme::Token(t) => Some(t),
            _ => None,
        })
        .collect();
    (program, errors, tokens)
}

/// A parse kept for the next parse of the same file's same text: [`keep`].
struct Kept {
    text: String,
    program: Program,
    errors: Vec<nazm_diag::Diagnostic>,
}

/// At most one kept parse per file id; a later [`keep`] of the same id replaces it.
static KEPT: std::sync::Mutex<Vec<(nazm_span::FileId, Kept)>> = std::sync::Mutex::new(Vec::new());

/// Keep `file`'s parse of `text` for the next [`parse_map`] of it, which takes it instead of
/// parsing the same text again. A loader parses every file for its imports and a checker
/// parsed every one again (performance leadership, `nazm check`): parsing is a function of
/// the file's id and text alone, so a parse of exactly this text under this id is the one
/// that would be made. Taken once; a different text under the id is parsed afresh.
pub fn keep(
    file: nazm_span::FileId,
    text: &str,
    program: Program,
    errors: Vec<nazm_diag::Diagnostic>,
) {
    let entry = Kept {
        text: text.to_owned(),
        program,
        errors,
    };
    let replaced = if let Ok(mut kept) = KEPT.lock() {
        match kept.iter_mut().find(|(id, _)| *id == file) {
            Some((_, k)) => Some(std::mem::replace(k, entry)),
            None => {
                kept.push((file, entry));
                None
            }
        }
    } else {
        None
    };
    // Dropped outside the lock.
    drop(replaced);
}

/// `file`'s parse of `text`: the kept one if there is one for exactly this text, else a new one.
fn parse_kept(file: nazm_span::FileId, text: &str) -> (Program, Vec<nazm_diag::Diagnostic>) {
    let taken = KEPT.lock().ok().and_then(|mut kept| {
        let at = kept
            .iter()
            .position(|(id, k)| *id == file && k.text == text)?;
        Some(kept.swap_remove(at).1)
    });
    match taken {
        Some(k) => (k.program, k.errors),
        None => parse_file(file, text),
    }
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
    let (mut lexemes, mut errors) = lex_lossless_in(file, text);
    if let Some(h) = hole {
        split_dots_at(&mut lexemes, h);
    }
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

/// A completion probe's cursor between the two dots of `..` (R1a): `a.|.c` is a member chain
/// missing a name — the editor's view — not a range, so the probe reads the two dots as two
/// `.`s and finds its hole after the first. An ordinary parse never splits one.
fn split_dots_at(lexemes: &mut Vec<Lexeme>, hole: u32) {
    let Some(i) = lexemes.iter().position(|l| {
        matches!(l, Lexeme::Token(t) if t.kind == TokenKind::DotDot && t.span.start + 1 == hole)
    }) else {
        return;
    };
    let Some(Lexeme::Token(t)) = lexemes.get(i).cloned() else {
        return;
    };
    let dot = |start: u32| {
        Lexeme::Token(Token {
            kind: TokenKind::Dot,
            span: nazm_span::Span::new(t.span.file, start, start + 1),
        })
    };
    lexemes.splice(i..=i, [dot(t.span.start), dot(t.span.start + 1)]);
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

/// As [`parse_map`], with the root file's parse taken from `root` — an editor's revision of
/// it (N76) — when that revision is of exactly the root file's text. Returns whether it
/// was. By [`Revision::edit`]'s contract the two parses are equal, so this changes what is
/// computed, never what is answered; a revision of other text is not used, and the root is
/// parsed as [`parse_map`] parses it.
pub fn parse_map_with_root(
    map: &nazm_span::SourceMap,
    root: Option<&Revision>,
) -> (Program, Vec<nazm_diag::Diagnostic>, bool) {
    let reusable = root.filter(|r| {
        r.file() == nazm_span::FileId::ROOT
            && map.ids().any(|id| id == nazm_span::FileId::ROOT)
            && map.text(nazm_span::FileId::ROOT) == r.text()
    });
    let mut program = Program {
        functions: Vec::new(),
        records: Vec::new(),
        enums: Vec::new(),
        imports: Vec::new(),
        traits: Vec::new(),
        impls: Vec::new(),
        consts: Vec::new(),
        hole: None,
    };
    let mut errors = Vec::new();
    for id in map.ids() {
        let (p, file_errors) = match reusable {
            Some(r) if id == nazm_span::FileId::ROOT => (r.program(), r.diagnostics()),
            _ => parse_kept(id, map.text(id)),
        };
        program.functions.extend(p.functions);
        program.records.extend(p.records);
        program.enums.extend(p.enums);
        program.imports.extend(p.imports);
        program.traits.extend(p.traits);
        program.impls.extend(p.impls);
        program.consts.extend(p.consts);
        errors.extend(file_errors);
    }
    (program, errors, reusable.is_some())
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
    let mut traits = Vec::new();
    let mut impls = Vec::new();
    let mut consts = Vec::new();
    let mut errors = Vec::new();
    let mut found = None;
    let mut root = None;
    for id in map.ids() {
        let at = if id == nazm_span::FileId::ROOT {
            hole
        } else {
            None
        };
        // The concrete tree only where it is returned — a probe's root file. Every other file is
        // parsed as `parse_file` parses it, which is `parse_both_at` with no hole and no tree:
        // the tree was built for every file of every compilation `parse_map` read and dropped,
        // most of a parse's work (performance leadership, a warm `nazm check`).
        let (program, file_errors) = if id == nazm_span::FileId::ROOT && hole.is_some() {
            let (program, cst, file_errors) = parse_both_at(id, map.text(id), at);
            found = program.hole;
            root = Some(cst);
            (program, file_errors)
        } else {
            parse_kept(id, map.text(id))
        };
        functions.extend(program.functions);
        records.extend(program.records);
        enums.extend(program.enums);
        imports.extend(program.imports);
        traits.extend(program.traits);
        impls.extend(program.impls);
        consts.extend(program.consts);
        errors.extend(file_errors);
    }
    (
        Program {
            functions,
            records,
            enums,
            imports,
            traits,
            impls,
            consts,
            hole: found,
        },
        errors,
        root,
    )
}
