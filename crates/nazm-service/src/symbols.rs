//! Symbols: the declarations the current analysis knows, where each is written, and how
//! structural declarations nest. N22.
//!
//! # The contract
//!
//! **A symbol is a declaration the checker declared, never a name found in text.** Identity
//! and kind come from the definition tables the checker built — a function's [`FnId`], a
//! record's [`RecordId`] and each of its fields, an enum's [`EnumId`], each variant and each
//! variant's payload fields — read by position, exactly as the reference index reads them
//! (`nazm_sema::references`). Where each is written comes from the syntax the checker
//! declared it from: the table's own `decl` index into the parsed program gives the item,
//! and the item's span is the declaration's range; the table's `name_span` is its name.
//! A child is attached to the record, enum or variant whose table holds it, and its range
//! is found in that parent's item by the child's name span — a position, not a spelling.
//!
//! So a symbol exists exactly when the checker declared something:
//!
//! - a function whose body did not parse keeps its declaration — the parser keeps the
//!   signature so that calls still check — and so keeps its symbol;
//! - a body with type errors is still a declared function;
//! - an item the parser could not read at all was never declared, and has no symbol: this
//!   is not an outline of the text;
//! - a second declaration the checker refused — a duplicate function, type, variant or
//!   field — was not declared, and only the first has a symbol;
//! - a built-in, an intrinsic, the core prelude's definitions and anything described only
//!   by a persisted interface have no source an editor can open, and are not symbols.
//!
//! Locals, parameters, pattern bindings and type parameters are not symbols: an outline is
//! a file's structure, and what is visible inside a body is the scope trace's (N19).
//!
//! # Document and workspace
//!
//! [`LanguageService::document_symbols`] is one document's declarations, from its own
//! compilation, in **source order** — the order the author wrote them, which is what an
//! outline presents. It is deliberately not the canonical order N21 shows candidates in:
//! moving two fields changes nothing a record means and does change the file.
//!
//! [`LanguageService::workspace_symbols`] is every declaration of the **current analysed
//! source universe**: every file of every compilation the service holds — one rooted at
//! each open document, with everything it imports. Not a directory: a `.nz` file that no
//! open document's compilation loads is in no program being analysed and is never read. A
//! declaration loaded by two compilations is one declaration, identified as N17 identifies
//! it across compilations — the same file, the same kind, the same name span — and never by
//! spelling, so two modules' `helper` are two symbols. Visibility does not filter it: the
//! question is what exists, not what one file may name.
//!
//! **This is not a project boundary.** Finding every declaration currently loaded does not
//! prove every module that could import one is loaded, so workspace symbols do not close
//! N18's refusal to rename an exported definition.
//!
//! # What is kept
//!
//! Nothing. Both queries are derived on demand from the analyses the service already holds;
//! no table of symbols is built when a document is analysed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nazm_core::sema::{EnumId, FieldRef, FnId, PayloadRef, RecordId, ReferenceTarget, VariantRef};
use nazm_core::Resolution;
use nazm_span::{FileId, Span};

use crate::service::{FileKey, LanguageService, Location, Snapshot, Target};

/// What kind of declaration a symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Function,
    Record,
    Enum,
    Variant,
    /// A record's field.
    Field,
    /// A variant's payload field.
    PayloadField,
}

impl SymbolKind {
    /// The kind in the words an editor shows, as [`ReferenceTarget::kind`] says them.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Record => "record",
            Self::Enum => "enum",
            Self::Variant => "variant",
            Self::Field => "field",
            Self::PayloadField => "payload field",
        }
    }
}

/// One declaration, with the declarations it structurally contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The entity, as this analysis identifies it — the same identity a reference or a
    /// rename of it uses. Session-local, like every such id.
    pub target: ReferenceTarget,
    pub kind: SymbolKind,
    /// As written.
    pub name: String,
    /// For a function, record or enum, whether it is `pub`. `None` for what is declared
    /// inside one, which has no visibility of its own.
    pub exported: Option<bool>,
    /// The whole declaration, in the file's text.
    pub start: u32,
    pub end: u32,
    /// The declared name.
    pub name_start: u32,
    pub name_end: u32,
    /// What it is, rendered by the renderers other answers use: a function's signature, a
    /// field's type, a variant's payload.
    pub detail: Option<String>,
    /// A record's fields, an enum's variants, a variant's payload fields, in source order.
    pub children: Vec<Symbol>,
}

/// One document's declarations, and the text their offsets are into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbols {
    pub path: PathBuf,
    pub text: Arc<str>,
    /// Top-level declarations in source order, each with its children.
    pub symbols: Vec<Symbol>,
}

/// Why there are no document symbols. There are never historical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSymbols {
    /// The document is not open at that version.
    NotCurrent,
}

/// One declaration of the analysed source universe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub kind: SymbolKind,
    pub name: String,
    /// What it is declared in: the record of a field, the enum of a variant, the enum and
    /// variant of a payload field (`State.Done`). Read from the parent it is declared in,
    /// not from text.
    pub container: Option<String>,
    pub exported: Option<bool>,
    pub detail: Option<String>,
    /// The whole declaration.
    pub declaration: Location,
    /// The declared name: where definition goes for any use of it.
    pub selection: Location,
}

impl LanguageService {
    /// The declarations of `path`, at `version`, that are written in `path`.
    ///
    /// # Errors
    ///
    /// [`NoSymbols::NotCurrent`] unless the document is open at `version` and analysed at
    /// the current generation.
    pub fn document_symbols(
        &self,
        path: &Path,
        version: i32,
    ) -> Result<DocumentSymbols, NoSymbols> {
        let s = self.current(path, version).ok_or(NoSymbols::NotCurrent)?;
        let text = s.texts.first().cloned().unwrap_or_else(|| Arc::from(""));
        Ok(DocumentSymbols {
            path: path.to_path_buf(),
            text,
            symbols: declared_in(s, FileId::ROOT),
        })
    }

    /// Every declaration of every file the current compilations hold whose name contains
    /// `query` — case-sensitively, as a substring; the empty query is every declaration.
    ///
    /// Ordered by name, then file, then where the name is, then kind. The filter chooses
    /// among declarations already known; it loads, resolves and discovers nothing.
    #[must_use]
    pub fn workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
        let mut found: BTreeMap<(FileKey, u32, u32, SymbolKind), WorkspaceSymbol> = BTreeMap::new();
        for s in self.documents.values().filter_map(|d| d.analysis.as_ref()) {
            if s.generation != self.generation {
                continue;
            }
            for (f, key) in s.files.iter().enumerate() {
                // The core prelude is compiled into the toolchain and has no file an editor
                // could open; [`Snapshot::target`] would refuse its spans too.
                if !matches!(key, FileKey::Path(_)) {
                    continue;
                }
                let file = FileId::from_index(f);
                let mut flat = Vec::new();
                for symbol in declared_in(s, file) {
                    flatten(symbol, None, &mut flat);
                }
                for (symbol, container) in flat {
                    let id = (key.clone(), symbol.name_start, symbol.name_end, symbol.kind);
                    if found.contains_key(&id) {
                        continue;
                    }
                    let (Some(Target::File(declaration)), Some(Target::File(selection))) = (
                        s.target(Span::new(file, symbol.start, symbol.end)),
                        s.target(Span::new(file, symbol.name_start, symbol.name_end)),
                    ) else {
                        continue;
                    };
                    found.insert(
                        id,
                        WorkspaceSymbol {
                            kind: symbol.kind,
                            name: symbol.name,
                            container,
                            exported: symbol.exported,
                            detail: symbol.detail,
                            declaration,
                            selection,
                        },
                    );
                }
            }
        }
        let mut out: Vec<WorkspaceSymbol> = found
            .into_values()
            .filter(|w| w.name.contains(query))
            .collect();
        out.sort_by(|a, b| {
            (&a.name, &a.selection.path, a.selection.start, a.kind).cmp(&(
                &b.name,
                &b.selection.path,
                b.selection.start,
                b.kind,
            ))
        });
        out
    }
}

/// A symbol and everything under it, each with the name of what it is declared in.
fn flatten(symbol: Symbol, container: Option<&str>, out: &mut Vec<(Symbol, Option<String>)>) {
    let mut symbol = symbol;
    let children = std::mem::take(&mut symbol.children);
    let inner = match container {
        Some(c) if symbol.kind == SymbolKind::Variant => format!("{c}.{}", symbol.name),
        _ => symbol.name.clone(),
    };
    out.push((symbol, container.map(str::to_owned)));
    for child in children {
        flatten(child, Some(&inner), out);
    }
}

/// The declarations written in `file` of `s`'s compilation, top level in source order.
fn declared_in(s: &Snapshot, file: FileId) -> Vec<Symbol> {
    let res = &s.analysis.resolution;
    let program = &s.analysis.program;
    let here = |at: Span| at.file == file && at != Span::synthetic();
    let mut out = Vec::new();

    for (i, def) in res.functions().iter().enumerate() {
        if !here(def.name_span) {
            continue;
        }
        let Some(item) = program
            .functions
            .get(def.decl as usize)
            .filter(|f| f.name_span == def.name_span)
        else {
            continue;
        };
        let id = FnId::from_index(i);
        out.push(Symbol {
            target: ReferenceTarget::Function(id),
            kind: SymbolKind::Function,
            name: def.name.clone(),
            exported: Some(def.visibility.is_exported()),
            start: item.span.start,
            end: item.span.end,
            name_start: def.name_span.start,
            name_end: def.name_span.end,
            detail: crate::Signature::of_function(res, id).map(|s| s.render(res, None).label),
            children: Vec::new(),
        });
    }

    for (r, def) in res.records().all().iter().enumerate() {
        if def.instance.is_some() || !here(def.name_span) {
            continue;
        }
        let Some(item) = program
            .records
            .get(def.decl as usize)
            .filter(|i| i.name_span == def.name_span)
        else {
            continue;
        };
        let record = RecordId::from_index(r);
        let mut children: Vec<Symbol> = def
            .fields
            .iter()
            .enumerate()
            .filter_map(|(k, f)| {
                let written = item.fields.iter().find(|w| w.name_span == f.name_span)?;
                let field = u32::try_from(k).ok()?;
                Some(member(
                    res,
                    ReferenceTarget::Field(FieldRef { record, field }),
                    SymbolKind::Field,
                    f,
                    written.span,
                ))
            })
            .collect();
        in_source_order(&mut children);
        out.push(Symbol {
            target: ReferenceTarget::Record(record),
            kind: SymbolKind::Record,
            name: def.name.clone(),
            exported: Some(def.visibility.is_exported()),
            start: item.span.start,
            end: item.span.end,
            name_start: def.name_span.start,
            name_end: def.name_span.end,
            detail: None,
            children,
        });
    }

    for (e, def) in res.enums().all().iter().enumerate() {
        if def.instance.is_some() || !here(def.name_span) {
            continue;
        }
        let Some(item) = program
            .enums
            .get(def.decl as usize)
            .filter(|i| i.name_span == def.name_span)
        else {
            continue;
        };
        let enumeration = EnumId::from_index(e);
        let mut variants: Vec<Symbol> = def
            .variants
            .iter()
            .enumerate()
            .filter_map(|(k, v)| {
                let written = item.variants.iter().find(|w| w.name_span == v.name_span)?;
                let variant = u32::try_from(k).ok()?;
                let mut payload: Vec<Symbol> = v
                    .fields
                    .iter()
                    .enumerate()
                    .filter_map(|(j, f)| {
                        let w = written.fields.iter().find(|w| w.name_span == f.name_span)?;
                        let field = u32::try_from(j).ok()?;
                        Some(member(
                            res,
                            ReferenceTarget::PayloadField(PayloadRef {
                                enumeration,
                                variant,
                                field,
                            }),
                            SymbolKind::PayloadField,
                            f,
                            w.span,
                        ))
                    })
                    .collect();
                in_source_order(&mut payload);
                let mut detail = v.name.clone();
                crate::signature::push_list(
                    &mut detail,
                    res,
                    v.fields.iter().map(|f| (Some(f.name.as_str()), f.ty)),
                );
                Some(Symbol {
                    target: ReferenceTarget::Variant(VariantRef {
                        enumeration,
                        variant,
                    }),
                    kind: SymbolKind::Variant,
                    name: v.name.clone(),
                    exported: None,
                    start: written.span.start,
                    end: written.span.end,
                    name_start: v.name_span.start,
                    name_end: v.name_span.end,
                    detail: (!v.fields.is_empty()).then_some(detail),
                    children: payload,
                })
            })
            .collect();
        in_source_order(&mut variants);
        out.push(Symbol {
            target: ReferenceTarget::Enum(enumeration),
            kind: SymbolKind::Enum,
            name: def.name.clone(),
            exported: Some(def.visibility.is_exported()),
            start: item.span.start,
            end: item.span.end,
            name_start: def.name_span.start,
            name_end: def.name_span.end,
            detail: None,
            children: variants,
        });
    }

    in_source_order(&mut out);
    out
}

/// A field or payload field, declared as `f` and written at `span`.
fn member(
    res: &Resolution,
    target: ReferenceTarget,
    kind: SymbolKind,
    f: &nazm_core::sema::FieldDef,
    span: Span,
) -> Symbol {
    Symbol {
        target,
        kind,
        name: f.name.clone(),
        exported: None,
        start: span.start,
        end: span.end,
        name_start: f.name_span.start,
        name_end: f.name_span.end,
        detail: Some(res.types().show(f.ty)),
        children: Vec::new(),
    }
}

/// Where the author wrote them, top to bottom.
fn in_source_order(symbols: &mut [Symbol]) {
    symbols.sort_by_key(|s| (s.start, s.name_start));
}

#[cfg(test)]
mod tests {
    /// The structural half of "never a name found in text": this module reads the
    /// definition tables by position and the parsed items by the spans the tables recorded.
    /// It never searches a table by spelling, never parses or lexes, and never reads a
    /// directory.
    #[test]
    fn symbols_are_never_found_by_spelling_or_by_scanning() {
        let source = include_str!("symbols.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            ".definition(",
            ".declared(",
            ".field(&",
            ".variant(&",
            ".name ==",
            "== name",
            "HashMap<String",
            "BTreeMap<String",
            "parse_cst(",
            "parse_file(",
            "lex(",
            "read_dir",
            "walk(",
            "Disk",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in symbols: it would be finding declarations by spelling or \
                 by scanning"
            );
        }
    }
}
