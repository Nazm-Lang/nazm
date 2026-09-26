//! `nazm lsp` — the Language Server Protocol, over stdio. N16.
//!
//! # A shell, not a compiler
//!
//! This file speaks the protocol and nothing else. Every question about a program goes to
//! [`nazm_service::LanguageService`], which answers from the loader, the parser and the
//! checker every other command uses; what is here is framing (`lsp-server`), message types
//! (`lsp-types`), and the one translation the protocol forces on us: **positions**. Nazm
//! spans are byte offsets into a file; an LSP position is a line and a count of UTF-16
//! code units. [`LineIndex`] converts between the two, here and nowhere else, so no
//! compiler structure ever holds a protocol position.
//!
//! # What is offered
//!
//! Exactly what is implemented, and the initialize response says so: full-document
//! synchronisation (open, change, close), published diagnostics, definition, hover,
//! references (N17), prepare-rename and rename (N18), completion (N19) — of a value, a
//! call's head or a type, invoked on an identifier — and
//! signature help (N20), of the call the checker checked, triggered by `(` and `,` — and,
//! since N21, completion of a field after `.`, a variant after `E.` and a construction's
//! or pattern's label, and signature help for a record or variant construction — and,
//! since N26, the same completion with nothing written yet, right after a `.` or a `(`,
//! anchored by a completion probe, with `.` as the one trigger character — and,
//! since N22, document symbols (one file's declarations, nested, in source order) and
//! workspace symbols (every declaration of the files the open documents' compilations
//! load, filtered by a case-sensitive substring, with nothing to resolve later) — and, since
//! N23, `semanticTokens/full`: every identifier the checker resolved, by its class and
//! whether it declares, under a fixed legend; no range or delta requests, and no lexical
//! tokens — and, since N24, `codeAction`: the fixes current diagnostics carry, as quick
//! fixes with versioned edits; no refactors, source actions or resolve — and, since N25, call
//! hierarchy: prepare, then incoming and outgoing calls, one level per request, from the
//! checked calls of the compilation the item was prepared in, with the item's opaque `data`
//! binding it to that compilation's current generation. No formatting, watched files or
//! cancellation — a client is never told something exists that the server would then
//! ignore.
//!
//! # What a rename can promise over this protocol
//!
//! The service's plan carries, for every file it edits, the exact text it was computed
//! from. The protocol can hold a client to less: a document **version**, and only for a
//! document the client has open — a `null` version means "whatever is on disk", with no
//! precondition at all. So a rename is returned only as versioned `documentChanges`, only to
//! a client that says it accepts them, and only when every file the plan edits is an open
//! document at the version the plan was validated against. Otherwise the rename is refused
//! with a message, rather than sent as an edit the client could apply to other bytes.
//!
//! # Standard output is the protocol
//!
//! Every byte on stdout is a framed JSON-RPC message written by `lsp-server`. Anything
//! else — a log line, a stray `print!` — would corrupt the framing, so this module never
//! prints; the protocol test reads stdout strictly and a unit test checks the source.
//!
//! # One state owner
//!
//! Messages are handled one at a time, in order, on one thread whose stack is the one
//! parsing requires. A request is answered from the document version current when it is
//! handled; diagnostics carry the version they were computed for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, CompletionTextEdit, Diagnostic as LspDiagnostic, DiagnosticSeverity,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover as LspHover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeResult, Location as LspLocation, MarkupContent, MarkupKind, NumberOrString, OneOf,
    ParameterInformation, ParameterLabel, Position, PositionEncodingKind, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, ReferenceParams, RenameOptions, RenameParams,
    ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions, SignatureHelpParams,
    SignatureInformation, SymbolInformation, SymbolKind as LspSymbolKind,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, Uri, WorkspaceSymbolOptions, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};
use nazm_service::{
    Help, HierarchyFunction, LanguageService, Locator, Published, RenamePlan, Rendered, Role,
    SemanticClass, SemanticIdentifier, Symbol, SymbolKind, Target,
};

/// `nazm lsp`: serve one client on stdin and stdout until it says `exit`.
pub(crate) fn run(source_root: Option<&Path>) -> ExitCode {
    let (connection, io) = Connection::stdio();
    let root = source_root.map(Path::to_path_buf);
    let served = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("nazm-lsp".to_owned())
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, || serve(&connection, root))
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the language service stopped unexpectedly".to_owned())?
    });
    // The writer thread flushes what is queued once the connection is gone.
    drop(connection);
    let joined = io.join();
    match (served, joined) {
        (Ok(()), Ok(())) => ExitCode::SUCCESS,
        (Err(e), _) => {
            eprintln!("nazm lsp: {e}");
            ExitCode::FAILURE
        }
        (_, Err(e)) => {
            eprintln!("nazm lsp: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The capabilities this server implements, and only those.
fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..TextDocumentSyncOptions::default()
            },
        )),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        // Invoked, and triggered by `.` alone (N26): after a `.` a field or variant is
        // what comes next. Not `(`, which signature help answers, nor `,` or `:`. What a
        // request is about is decided from the current source, never from the trigger the
        // client reports. Every item is final — there is nothing more to resolve.
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_owned()]),
            ..CompletionOptions::default()
        }),
        // `(` opens an argument list and `,` moves to the next argument: the two
        // characters after which the active call or parameter changes. No retrigger
        // characters — nothing else is claimed.
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: None,
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        }),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        // Prepare, then incoming and outgoing calls: one level per request, from the
        // compilation the item was prepared in.
        call_hierarchy_provider: Some(lsp_types::CallHierarchyServerCapability::Simple(true)),
        // Quick fixes only, each complete when sent: no refactors, no source actions, no
        // resolve.
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
            lsp_types::CodeActionOptions {
                code_action_kinds: Some(vec![lsp_types::CodeActionKind::QUICKFIX]),
                work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
                resolve_provider: Some(false),
            },
        )),
        // Every symbol is complete when it is sent: there is no `workspaceSymbol/resolve`.
        workspace_symbol_provider: Some(OneOf::Right(WorkspaceSymbolOptions {
            resolve_provider: Some(false),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        })),
        // The whole document only: no range requests and no deltas, so nothing is kept
        // between requests to compute one from.
        semantic_tokens_provider: Some(
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                lsp_types::SemanticTokensOptions {
                    work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
                    legend: legend(),
                    range: Some(false),
                    full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                },
            ),
        ),
        ..ServerCapabilities::default()
    }
}

/// The token types, in the legend's order: a token's type is its index here.
const TOKEN_TYPES: [&str; 9] = [
    "type",
    "struct",
    "enum",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "function",
];

/// The token modifiers, in the legend's order: modifier `i` is bit `1 << i`.
const TOKEN_MODIFIERS: [&str; 2] = ["declaration", "defaultLibrary"];

/// The one legend this server encodes against, fixed at compile time.
fn legend() -> lsp_types::SemanticTokensLegend {
    lsp_types::SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .map(|t| lsp_types::SemanticTokenType::new(t))
            .collect(),
        token_modifiers: TOKEN_MODIFIERS
            .iter()
            .map(|m| lsp_types::SemanticTokenModifier::new(m))
            .collect(),
    }
}

/// A class's token type, as its index in [`TOKEN_TYPES`], and whether it is compiler-owned.
const fn token_type(class: SemanticClass) -> (u32, bool) {
    match class {
        SemanticClass::BuiltinType => (0, true),
        SemanticClass::Record => (1, false),
        SemanticClass::Enum => (2, false),
        SemanticClass::TypeParameter => (3, false),
        SemanticClass::Parameter => (4, false),
        SemanticClass::Variable => (5, false),
        SemanticClass::Field | SemanticClass::PayloadField => (6, false),
        SemanticClass::Variant => (7, false),
        SemanticClass::Function => (8, false),
        SemanticClass::Intrinsic => (8, true),
    }
}

/// `declaration` and `defaultLibrary`, as bits of [`TOKEN_MODIFIERS`].
const DECLARATION: u32 = 1 << 0;
const DEFAULT_LIBRARY: u32 = 1 << 1;

/// The protocol's relative encoding of `identifiers` in `text`: per token, the lines since
/// the previous one, its start column (relative to the previous token's on the same line),
/// its length, its type and its modifiers — columns in UTF-16 units.
///
/// Refused, not encoded, when the identifiers are not in strictly increasing order, overlap,
/// or one spans lines: each would be a token array that means something else.
fn encode_tokens(text: &str, identifiers: &[SemanticIdentifier]) -> Result<Vec<u32>, String> {
    let index = LineIndex::new(text);
    let mut data = Vec::with_capacity(identifiers.len() * 5);
    let mut previous: Option<(u32, Position)> = None;
    for id in identifiers {
        if id.start >= id.end {
            return Err(format!("an empty identifier at {}", id.start));
        }
        if let Some((end, _)) = previous {
            if id.start < end {
                return Err(format!(
                    "identifiers overlap or are out of order at {}",
                    id.start
                ));
            }
        }
        let start = index.position(id.start);
        let stop = index.position(id.end);
        if start.line != stop.line {
            return Err(format!("an identifier spans lines at {}", id.start));
        }
        let (kind, owned) = token_type(id.class);
        let mut modifiers = 0;
        if id.role == Role::Declaration {
            modifiers |= DECLARATION;
        }
        if owned {
            modifiers |= DEFAULT_LIBRARY;
        }
        let (line, column) = match previous {
            Some((_, p)) if p.line == start.line => (0, start.character - p.character),
            Some((_, p)) => (start.line - p.line, start.character),
            None => (start.line, start.character),
        };
        data.extend([
            line,
            column,
            stop.character - start.character,
            kind,
            modifiers,
        ]);
        previous = Some((id.end, start));
    }
    Ok(data)
}

fn serve(connection: &Connection, root: Option<PathBuf>) -> Result<(), String> {
    let init = InitializeResult {
        capabilities: capabilities(),
        server_info: Some(ServerInfo {
            name: "nazm".to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
    };
    let init = serde_json::to_value(init).map_err(|e| e.to_string())?;
    // `initialize_start`/`initialize_finish` rather than `initialize`, which takes only the
    // capabilities and would wrap them again: the response must be one `InitializeResult`.
    let (id, params) = connection
        .initialize_start()
        .map_err(|e| format!("initialize: {e}"))?;
    // Whether the client accepts `documentChanges`, which is the only form in which an
    // edit can carry the document version it is valid against.
    let versioned_edits = params
        .pointer("/capabilities/workspace/workspaceEdit/documentChanges")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // Whether the client reads nested document symbols. One that does not is sent the flat
    // form, each symbol naming what it is declared in.
    let nested_symbols = params
        .pointer("/capabilities/textDocument/documentSymbol/hierarchicalDocumentSymbolSupport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    connection
        .initialize_finish(id, init)
        .map_err(|e| format!("initialize: {e}"))?;

    let mut server = Server {
        service: LanguageService::with_source_root(root),
        versions: HashMap::new(),
        versioned_edits,
        nested_symbols,
    };
    for message in &connection.receiver {
        match message {
            Message::Request(req) => {
                if connection
                    .handle_shutdown(&req)
                    .map_err(|e| format!("shutdown: {e}"))?
                {
                    return Ok(());
                }
                let response = server.request(req);
                send(connection, Message::Response(response))?;
            }
            Message::Notification(n) => {
                for published in server.notification(n) {
                    // A path that is not valid UTF-8 has no `file:` URI to report under,
                    // and could not have been opened by URI in the first place.
                    if let Some(n) = publish(&published) {
                        send(connection, Message::Notification(n))?;
                    }
                }
            }
            // This server sends no requests, so no response can be for it.
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn send(connection: &Connection, message: Message) -> Result<(), String> {
    connection
        .sender
        .send(message)
        .map_err(|_| "the client went away".to_owned())
}

struct Server {
    service: LanguageService,
    /// The current version of each open document, by path.
    versions: HashMap<PathBuf, i32>,
    /// Whether the client said it accepts versioned `documentChanges`.
    versioned_edits: bool,
    /// Whether the client said it reads hierarchical document symbols.
    nested_symbols: bool,
}

impl Server {
    fn notification(&mut self, n: Notification) -> Vec<Published> {
        use lsp_types::notification::{
            DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
        };
        match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(p) =
                    serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(n.params)
                else {
                    return Vec::new();
                };
                let Some(path) = path_of(&p.text_document.uri) else {
                    return Vec::new();
                };
                self.versions.insert(path.clone(), p.text_document.version);
                self.service
                    .open(&path, p.text_document.version, p.text_document.text)
            }
            DidChangeTextDocument::METHOD => {
                let Ok(p) =
                    serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(n.params)
                else {
                    return Vec::new();
                };
                let Some(path) = path_of(&p.text_document.uri) else {
                    return Vec::new();
                };
                // Full synchronisation: the last change is the whole document.
                let Some(text) = p.content_changes.into_iter().last().map(|c| c.text) else {
                    return Vec::new();
                };
                self.versions.insert(path.clone(), p.text_document.version);
                self.service.change(&path, p.text_document.version, text)
            }
            DidCloseTextDocument::METHOD => {
                let Ok(p) =
                    serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(n.params)
                else {
                    return Vec::new();
                };
                let Some(path) = path_of(&p.text_document.uri) else {
                    return Vec::new();
                };
                self.versions.remove(&path);
                self.service.close(&path)
            }
            // `initialized`, `exit` before shutdown, `$/cancelRequest`, `$/setTrace` and
            // anything else: nothing to do, and a notification has no reply.
            _ => Vec::new(),
        }
    }

    fn request(&self, req: Request) -> Response {
        use lsp_types::request::{
            CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
            CodeActionRequest, Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest,
            PrepareRenameRequest, References, Rename, SemanticTokensFullRequest,
            SignatureHelpRequest, WorkspaceSymbolRequest,
        };
        let id = req.id.clone();
        let result = match req.method.as_str() {
            GotoDefinition::METHOD => serde_json::from_value::<GotoDefinitionParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|p| self.definition(&p)),
            HoverRequest::METHOD => serde_json::from_value::<HoverParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|p| self.hover(&p)),
            References::METHOD => serde_json::from_value::<ReferenceParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|p| self.references(&p)),
            Completion::METHOD => serde_json::from_value::<CompletionParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|p| self.completion(&p)),
            SignatureHelpRequest::METHOD => {
                serde_json::from_value::<SignatureHelpParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.signature_help(&p))
            }
            CodeActionRequest::METHOD => {
                serde_json::from_value::<lsp_types::CodeActionParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.code_actions(&p))
            }
            DocumentSymbolRequest::METHOD => {
                serde_json::from_value::<DocumentSymbolParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.document_symbols(&p))
            }
            WorkspaceSymbolRequest::METHOD => {
                serde_json::from_value::<WorkspaceSymbolParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.workspace_symbols(&p))
            }
            SemanticTokensFullRequest::METHOD => {
                match serde_json::from_value::<lsp_types::SemanticTokensParams>(req.params) {
                    Ok(p) => match self.semantic_tokens(&p) {
                        Ok(tokens) => Ok(tokens),
                        Err(why) => {
                            return Response::new_err(id, ErrorCode::InternalError as i32, why);
                        }
                    },
                    Err(e) => Err(e.to_string()),
                }
            }
            CallHierarchyPrepare::METHOD => {
                serde_json::from_value::<lsp_types::CallHierarchyPrepareParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.prepare_call_hierarchy(&p))
            }
            CallHierarchyIncomingCalls::METHOD => {
                serde_json::from_value::<lsp_types::CallHierarchyIncomingCallsParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.incoming_calls(&p.item))
            }
            CallHierarchyOutgoingCalls::METHOD => {
                serde_json::from_value::<lsp_types::CallHierarchyOutgoingCallsParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.outgoing_calls(&p.item))
            }
            PrepareRenameRequest::METHOD => {
                serde_json::from_value::<TextDocumentPositionParams>(req.params)
                    .map_err(|e| e.to_string())
                    .map(|p| self.prepare_rename(&p))
            }
            Rename::METHOD => match serde_json::from_value::<RenameParams>(req.params) {
                Ok(p) => match self.rename(&p) {
                    Ok(edit) => Ok(edit),
                    Err(why) => {
                        return Response::new_err(id, ErrorCode::RequestFailed as i32, why);
                    }
                },
                Err(e) => Err(e.to_string()),
            },
            other => {
                return Response::new_err(
                    id,
                    ErrorCode::MethodNotFound as i32,
                    format!("`{other}` is not implemented by nazm lsp"),
                );
            }
        };
        match result {
            Ok(value) => Response::new_ok(id, value),
            Err(e) => Response::new_err(id, ErrorCode::InvalidParams as i32, e),
        }
    }

    /// The document at `uri`, its current version, and the byte offset of `at` in it.
    fn locate(&self, uri: &Uri, at: Position) -> Option<(PathBuf, i32, u32)> {
        let path = path_of(uri)?;
        let version = *self.versions.get(&path)?;
        let text = self.service.text(&path)?;
        let offset = LineIndex::new(&text).offset(at);
        Some((path, version, offset))
    }

    fn definition(&self, p: &GotoDefinitionParams) -> serde_json::Value {
        let tdp = &p.text_document_position_params;
        let found = self
            .locate(&tdp.text_document.uri, tdp.position)
            .and_then(|(path, version, offset)| self.service.definition(&path, version, offset));
        let location = found.and_then(|d| match d.target {
            Target::File(l) => {
                let index = LineIndex::new(&l.text);
                Some(LspLocation {
                    uri: uri_of(&l.path)?,
                    range: Range {
                        start: index.position(l.start),
                        end: index.position(l.end),
                    },
                })
            }
            // The core prelude is compiled into the toolchain: there is no file to open.
            Target::Prelude { .. } => None,
        });
        match location {
            Some(l) => serde_json::to_value(GotoDefinitionResponse::Scalar(l))
                .unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    }

    /// `textDocument/references`: the service's answer, with the declaration included when
    /// the client asks for it, and each byte span turned into a UTF-16 range in the text
    /// it was computed from. `null` when the position refers to nothing.
    fn references(&self, p: &ReferenceParams) -> serde_json::Value {
        let tdp = &p.text_document_position;
        let Some(found) = self
            .locate(&tdp.text_document.uri, tdp.position)
            .and_then(|(path, version, offset)| self.service.references(&path, version, offset))
        else {
            return serde_json::Value::Null;
        };
        let found = found.locations(p.context.include_declaration);
        // One line table per file, not per location: the locations are ordered by file.
        let mut index: Option<(&str, LineIndex<'_>)> = None;
        let mut out = Vec::with_capacity(found.len());
        for l in &found {
            if !index
                .as_ref()
                .is_some_and(|(t, _)| std::ptr::eq(*t, &*l.text))
            {
                index = Some((&l.text, LineIndex::new(&l.text)));
            }
            let (Some((_, lines)), Some(uri)) = (&index, uri_of(&l.path)) else {
                continue;
            };
            out.push(LspLocation {
                uri,
                range: Range {
                    start: lines.position(l.start),
                    end: lines.position(l.end),
                },
            });
        }
        serde_json::to_value(out).unwrap_or(serde_json::Value::Null)
    }

    /// `textDocument/completion`: the service's candidates for the identifier under the
    /// cursor, each replacing that whole identifier, in the service's order — or `null`
    /// where the service has no complete answer.
    fn completion(&self, p: &CompletionParams) -> serde_json::Value {
        let tdp = &p.text_document_position;
        let Some((path, version, offset)) = self.locate(&tdp.text_document.uri, tdp.position)
        else {
            return serde_json::Value::Null;
        };
        let (Ok(found), Some(text)) = (
            self.service.completion(&path, version, offset),
            self.service.text(&path),
        ) else {
            return serde_json::Value::Null;
        };
        let index = LineIndex::new(&text);
        let range = Range {
            start: index.position(found.start),
            end: index.position(found.end),
        };
        let items = found
            .items
            .into_iter()
            .enumerate()
            .map(|(i, c)| CompletionItem {
                kind: Some(match c.kind {
                    "local" => CompletionItemKind::VARIABLE,
                    "function" | "built-in function" => CompletionItemKind::FUNCTION,
                    "field" | "payload field" => CompletionItemKind::FIELD,
                    "variant" => CompletionItemKind::ENUM_MEMBER,
                    "enum" => CompletionItemKind::ENUM,
                    "type parameter" => CompletionItemKind::TYPE_PARAMETER,
                    _ => CompletionItemKind::STRUCT,
                }),
                detail: c.detail.or_else(|| Some(c.kind.to_owned())),
                // The service's canonical order, kept by the client's sort.
                sort_text: Some(format!("{i:06}")),
                text_edit: Some(CompletionTextEdit::Edit(lsp_types::TextEdit {
                    range,
                    new_text: c.name.clone(),
                })),
                label: c.name,
                ..CompletionItem::default()
            })
            .collect();
        serde_json::to_value(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items,
        }))
        .unwrap_or(serde_json::Value::Null)
    }

    /// `textDocument/signatureHelp`: the one signature of the call the checker checked, with
    /// each parameter as its UTF-16 range in the label — or `null` where the service has
    /// none.
    ///
    /// Also `null` for a position past the last parameter of a call with too many
    /// arguments: the protocol reads an absent active parameter as the first one, so the
    /// only answer that does not point at the wrong parameter is none. A callee with no
    /// parameters has its signature and no active parameter, which the protocol ignores.
    fn signature_help(&self, p: &SignatureHelpParams) -> serde_json::Value {
        let tdp = &p.text_document_position_params;
        let Some((path, version, offset)) = self.locate(&tdp.text_document.uri, tdp.position)
        else {
            return serde_json::Value::Null;
        };
        let help = match self.service.help_at(&path, version, offset) {
            Ok(Help::Call(help)) => help,
            // A construction (N21): its shape, and its active field when the position is in
            // an initialiser whose label resolved. Otherwise no parameter is marked — the
            // parameters are left out, since an absent index would mark the first.
            Ok(Help::Constructor(c)) => {
                return signature_json(
                    &c.rendered,
                    c.active_parameter,
                    c.active_parameter.is_some(),
                )
            }
            Err(_) => return serde_json::Value::Null,
        };
        let params = &help.rendered.params;
        if help.active_parameter.is_none() && !params.is_empty() {
            return serde_json::Value::Null;
        }
        signature_json(&help.rendered, help.active_parameter, true)
    }

    /// `textDocument/prepareRename`: the token and its current spelling, or `null` for a
    /// position the service would not rename — whatever the reason, which `rename` states.
    fn prepare_rename(&self, p: &TextDocumentPositionParams) -> serde_json::Value {
        let Some((path, version, offset)) = self.locate(&p.text_document.uri, p.position) else {
            return serde_json::Value::Null;
        };
        let (Ok(r), Some(text)) = (
            self.service.prepare_rename(&path, version, offset),
            self.service.text(&path),
        ) else {
            return serde_json::Value::Null;
        };
        let index = LineIndex::new(&text);
        serde_json::to_value(PrepareRenameResponse::RangeWithPlaceholder {
            range: Range {
                start: index.position(r.start),
                end: index.position(r.end),
            },
            placeholder: r.name,
        })
        .unwrap_or(serde_json::Value::Null)
    }

    /// `textDocument/rename`: the service's validated plan as a versioned edit, or the
    /// reason there is none.
    fn rename(&self, p: &RenameParams) -> Result<serde_json::Value, String> {
        let tdp = &p.text_document_position;
        let (path, version, offset) = self
            .locate(&tdp.text_document.uri, tdp.position)
            .ok_or_else(|| "the document is not open".to_owned())?;
        let plan = self
            .service
            .rename(&path, version, offset, &p.new_name)
            .map_err(|r| format!("nazm cannot rename this: {r}"))?;
        if !self.service.plan_is_current(&plan) {
            return Err("the program changed while the rename was being planned".to_owned());
        }
        let edit = workspace_edit(&plan, self.versioned_edits)?;
        serde_json::to_value(edit).map_err(|e| e.to_string())
    }

    /// `textDocument/prepareCallHierarchy`: the source function the position declares or
    /// calls, as one item whose `data` binds it to this document's current compilation —
    /// or `null` for any other position, or a document that is not open.
    fn prepare_call_hierarchy(
        &self,
        p: &lsp_types::CallHierarchyPrepareParams,
    ) -> serde_json::Value {
        let tdp = &p.text_document_position_params;
        let item = self
            .locate(&tdp.text_document.uri, tdp.position)
            .and_then(|(path, version, offset)| {
                self.service
                    .prepare_call_hierarchy(&path, version, offset)
                    .ok()
                    .flatten()
            })
            .and_then(|f| hierarchy_item(&f, &mut Lines::default()));
        match item {
            Some(item) => serde_json::to_value([item]).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    }

    /// `callHierarchy/incomingCalls`: each function of the item's compilation that calls
    /// it, with every call in the caller's file — or `null` for an item whose `data` is no
    /// longer current, which must be prepared again.
    fn incoming_calls(&self, item: &lsp_types::CallHierarchyItem) -> serde_json::Value {
        let Some(Ok(calls)) = locator(item).map(|l| self.service.incoming_calls(&l)) else {
            return serde_json::Value::Null;
        };
        let mut lines = Lines::default();
        let out: Option<Vec<lsp_types::CallHierarchyIncomingCall>> = calls
            .iter()
            .map(|c| {
                Some(lsp_types::CallHierarchyIncomingCall {
                    from: hierarchy_item(&c.from, &mut lines)?,
                    from_ranges: call_ranges(&c.calls, &mut lines),
                })
            })
            .collect();
        out.and_then(|o| serde_json::to_value(o).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// `callHierarchy/outgoingCalls`: each source function the item calls, with every call
    /// in the item's file — or `null`, as for incoming calls.
    fn outgoing_calls(&self, item: &lsp_types::CallHierarchyItem) -> serde_json::Value {
        let Some(Ok(calls)) = locator(item).map(|l| self.service.outgoing_calls(&l)) else {
            return serde_json::Value::Null;
        };
        let mut lines = Lines::default();
        let out: Option<Vec<lsp_types::CallHierarchyOutgoingCall>> = calls
            .iter()
            .map(|c| {
                Some(lsp_types::CallHierarchyOutgoingCall {
                    to: hierarchy_item(&c.to, &mut lines)?,
                    from_ranges: call_ranges(&c.calls, &mut lines),
                })
            })
            .collect();
        out.and_then(|o| serde_json::to_value(o).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// `textDocument/documentSymbol`: the document's declarations as the service derived
    /// them — nested, to a client that reads nesting, and otherwise flat with each naming
    /// its container — or `null` for a document that is not open.
    fn document_symbols(&self, p: &DocumentSymbolParams) -> serde_json::Value {
        let uri = &p.text_document.uri;
        let Some(found) = path_of(uri).and_then(|path| {
            let version = *self.versions.get(&path)?;
            self.service.document_symbols(&path, version).ok()
        }) else {
            return serde_json::Value::Null;
        };
        let index = LineIndex::new(&found.text);
        let response = if self.nested_symbols {
            DocumentSymbolResponse::Nested(
                found
                    .symbols
                    .iter()
                    .map(|s| document_symbol(s, &index))
                    .collect(),
            )
        } else {
            let mut flat = Vec::new();
            for s in &found.symbols {
                flat_symbols(s, None, uri, &index, &mut flat);
            }
            DocumentSymbolResponse::Flat(flat)
        };
        serde_json::to_value(response).unwrap_or(serde_json::Value::Null)
    }

    /// `workspace/symbol`: every declaration of the analysed source universe whose name
    /// contains the query, each at its declared name — where definition goes — in the
    /// service's order.
    fn workspace_symbols(&self, p: &WorkspaceSymbolParams) -> serde_json::Value {
        let found = self.service.workspace_symbols(&p.query);
        let mut index: Option<(&str, LineIndex<'_>)> = None;
        let mut out = Vec::with_capacity(found.len());
        for w in &found {
            let l = &w.selection;
            if !index
                .as_ref()
                .is_some_and(|(t, _)| std::ptr::eq(*t, &*l.text))
            {
                index = Some((&l.text, LineIndex::new(&l.text)));
            }
            let (Some((_, lines)), Some(uri)) = (&index, uri_of(&l.path)) else {
                continue;
            };
            out.push(lsp_types::WorkspaceSymbol {
                name: w.name.clone(),
                kind: symbol_kind(w.kind),
                tags: None,
                container_name: w.container.clone(),
                location: OneOf::Left(LspLocation {
                    uri,
                    range: Range {
                        start: lines.position(l.start),
                        end: lines.position(l.end),
                    },
                }),
                data: None,
            });
        }
        serde_json::to_value(WorkspaceSymbolResponse::Nested(out))
            .unwrap_or(serde_json::Value::Null)
    }

    /// `textDocument/semanticTokens/full`: the document's classified identifiers, relatively
    /// encoded against [`legend`], with no `resultId` — there are no deltas to ask for — or
    /// `null` for a document that is not open. An identifier list the encoder refuses is an
    /// error, never a token array that means something else.
    fn semantic_tokens(
        &self,
        p: &lsp_types::SemanticTokensParams,
    ) -> Result<serde_json::Value, String> {
        let Some(found) = path_of(&p.text_document.uri).and_then(|path| {
            let version = *self.versions.get(&path)?;
            self.service.semantic_identifiers(&path, version).ok()
        }) else {
            return Ok(serde_json::Value::Null);
        };
        let data = encode_tokens(&found.text, &found.identifiers)?;
        Ok(serde_json::json!({ "data": data }))
    }

    /// `textDocument/codeAction`: one quick fix per fix of each current diagnostic the range
    /// touches, as the service planned it — never from the diagnostics the client sends,
    /// which are not consulted. Each carries its versioned edit; a fix the protocol cannot
    /// protect (a client without versioned changes, a file not open) is left out, never sent
    /// weaker. Empty when the client asks only for kinds other than quick fixes.
    fn code_actions(&self, p: &lsp_types::CodeActionParams) -> serde_json::Value {
        let quickfix = lsp_types::CodeActionKind::QUICKFIX;
        if let Some(only) = &p.context.only {
            // A requested kind covers the kinds below it: `quickfix` and the empty base.
            if !only.iter().any(|k| k.as_str().is_empty() || *k == quickfix) {
                return serde_json::json!([]);
            }
        }
        let Some((path, version)) = path_of(&p.text_document.uri).and_then(|path| {
            let version = *self.versions.get(&path)?;
            Some((path, version))
        }) else {
            return serde_json::Value::Null;
        };
        let Some(text) = self.service.text(&path) else {
            return serde_json::Value::Null;
        };
        let index = LineIndex::new(&text);
        let (start, end) = (index.offset(p.range.start), index.offset(p.range.end));
        let Ok(plans) = self.service.fix_plans(&path, version, start, end) else {
            return serde_json::Value::Null;
        };
        let mut actions = Vec::new();
        for plan in &plans {
            if !self.service.fix_plan_is_current(plan) {
                continue;
            }
            let Ok(edit) = versioned_edit(std::slice::from_ref(&plan.file), self.versioned_edits)
            else {
                continue;
            };
            actions.push(lsp_types::CodeAction {
                title: fix_title(plan),
                kind: Some(quickfix.clone()),
                diagnostics: Some(vec![lsp_diagnostic(&plan.diagnostic, &index)]),
                edit: Some(edit),
                command: None,
                is_preferred: preferred(plan).then_some(true),
                disabled: None,
                data: None,
            });
        }
        serde_json::to_value(actions).unwrap_or(serde_json::Value::Null)
    }

    fn hover(&self, p: &HoverParams) -> serde_json::Value {
        let tdp = &p.text_document_position_params;
        let Some((path, version, offset)) = self.locate(&tdp.text_document.uri, tdp.position)
        else {
            return serde_json::Value::Null;
        };
        let (Some(h), Some(text)) = (
            self.service.hover(&path, version, offset),
            self.service.text(&path),
        ) else {
            return serde_json::Value::Null;
        };
        let index = LineIndex::new(&text);
        serde_json::to_value(LspHover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::PlainText,
                value: h.text,
            }),
            range: Some(Range {
                start: index.position(h.start),
                end: index.position(h.end),
            }),
        })
        .unwrap_or(serde_json::Value::Null)
    }
}

/// A source function as a call hierarchy item: its declaration as `range`, its name as
/// `selectionRange`, and its locator as opaque `data` — `None` for a path no URI spells.
fn hierarchy_item<'a>(
    f: &'a HierarchyFunction,
    lines: &mut Lines<'a>,
) -> Option<lsp_types::CallHierarchyItem> {
    let index = lines.of(&f.declaration.text)?;
    let range = |start: u32, end: u32| Range {
        start: index.position(start),
        end: index.position(end),
    };
    let l = &f.locator;
    Some(lsp_types::CallHierarchyItem {
        name: f.name.clone(),
        kind: LspSymbolKind::FUNCTION,
        tags: None,
        detail: f.detail.clone(),
        uri: uri_of(&f.declaration.path)?,
        range: range(f.declaration.start, f.declaration.end),
        selection_range: range(f.selection.start, f.selection.end),
        data: Some(serde_json::json!({
            "root": l.root.to_str()?,
            "version": l.version,
            "generation": l.generation,
            "file": l.file.to_str()?,
            "start": l.name_start,
            "end": l.name_end,
        })),
    })
}

/// The locator an item's `data` carries, exactly as [`hierarchy_item`] wrote it — `None`
/// for an item this server did not prepare.
fn locator(item: &lsp_types::CallHierarchyItem) -> Option<Locator> {
    let data = item.data.as_ref()?;
    let text = |k: &str| data.get(k)?.as_str().map(PathBuf::from);
    let number = |k: &str| data.get(k)?.as_u64();
    Some(Locator {
        root: text("root")?,
        version: i32::try_from(data.get("version")?.as_i64()?).ok()?,
        generation: number("generation")?,
        file: text("file")?,
        name_start: u32::try_from(number("start")?).ok()?,
        name_end: u32::try_from(number("end")?).ok()?,
    })
}

/// Calls as ranges of the file they are in, in order.
fn call_ranges<'a>(calls: &'a [nazm_service::Location], lines: &mut Lines<'a>) -> Vec<Range> {
    calls
        .iter()
        .filter_map(|l| {
            let index = lines.of(&l.text)?;
            Some(Range {
                start: index.position(l.start),
                end: index.position(l.end),
            })
        })
        .collect()
}

/// The line index of each file text one response mentions, built once per text rather than
/// once per item or range: a compiler-sized file has hundreds of calls in one answer.
#[derive(Default)]
struct Lines<'a> {
    built: Vec<LineIndex<'a>>,
}

impl<'a> Lines<'a> {
    fn of(&mut self, text: &'a str) -> Option<&LineIndex<'a>> {
        let at = match self.built.iter().position(|l| std::ptr::eq(l.text, text)) {
            Some(at) => at,
            None => {
                self.built.push(LineIndex::new(text));
                self.built.len() - 1
            }
        };
        self.built.get(at)
    }
}

/// A symbol kind as the protocol names it.
fn symbol_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Record => LspSymbolKind::STRUCT,
        SymbolKind::Enum => LspSymbolKind::ENUM,
        SymbolKind::Variant => LspSymbolKind::ENUM_MEMBER,
        SymbolKind::Field | SymbolKind::PayloadField => LspSymbolKind::FIELD,
    }
}

/// A symbol and its children, with the declaration as `range` and the name as
/// `selectionRange`.
fn document_symbol(s: &Symbol, index: &LineIndex<'_>) -> DocumentSymbol {
    let range = |start: u32, end: u32| Range {
        start: index.position(start),
        end: index.position(end),
    };
    #[allow(deprecated)] // `deprecated` is a required field of the struct, left unset.
    DocumentSymbol {
        name: s.name.clone(),
        detail: s.detail.clone(),
        kind: symbol_kind(s.kind),
        tags: None,
        deprecated: None,
        range: range(s.start, s.end),
        selection_range: range(s.name_start, s.name_end),
        children: (!s.children.is_empty()).then(|| {
            s.children
                .iter()
                .map(|c| document_symbol(c, index))
                .collect()
        }),
    }
}

/// A symbol and its children, flat, each at its whole declaration and naming the symbol
/// it is declared in.
fn flat_symbols(
    s: &Symbol,
    container: Option<&str>,
    uri: &Uri,
    index: &LineIndex<'_>,
    out: &mut Vec<SymbolInformation>,
) {
    #[allow(deprecated)] // `deprecated` is a required field of the struct, left unset.
    out.push(SymbolInformation {
        name: s.name.clone(),
        kind: symbol_kind(s.kind),
        tags: None,
        deprecated: None,
        location: LspLocation {
            uri: uri.clone(),
            range: Range {
                start: index.position(s.start),
                end: index.position(s.end),
            },
        },
        container_name: container.map(str::to_owned),
    });
    for c in &s.children {
        flat_symbols(c, Some(&s.name), uri, index, out);
    }
}

/// One `SignatureHelp` with one signature: the label, and — with `parameters` — each
/// parameter as its offsets into the label in the position encoding. The label is one line,
/// so its own line index converts them.
fn signature_json(rendered: &Rendered, active: Option<u32>, parameters: bool) -> serde_json::Value {
    let label = LineIndex::new(&rendered.label);
    let offsets = rendered
        .params
        .iter()
        .map(|(start, end)| ParameterInformation {
            label: ParameterLabel::LabelOffsets([
                label.position(*start).character,
                label.position(*end).character,
            ]),
            documentation: None,
        })
        .collect();
    serde_json::to_value(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: rendered.label.clone(),
            documentation: None,
            parameters: parameters.then_some(offsets),
            active_parameter: active,
        }],
        active_signature: Some(0),
        active_parameter: active,
    })
    .unwrap_or(serde_json::Value::Null)
}

/// A rename plan as the protocol's edit. See [`versioned_edit`].
fn workspace_edit(plan: &RenamePlan, versioned: bool) -> Result<lsp_types::WorkspaceEdit, String> {
    versioned_edit(&plan.files, versioned)
}

/// Edits as the protocol's edit: `documentChanges`, one per file, each tied to the version
/// of the open document the edits were validated against. The one form in which this server
/// sends any edit — a rename (N18) or a quick fix (N24).
///
/// Refused, not weakened, when the protocol cannot hold the client to the edits'
/// precondition: a client that does not accept versioned changes, or a file to edit that is
/// not open — whose version would be `null`, which the protocol defines as "the disk is the
/// master", with no check of what the disk now holds.
fn versioned_edit(
    files: &[nazm_service::FileEdit],
    versioned: bool,
) -> Result<lsp_types::WorkspaceEdit, String> {
    use lsp_types::{
        DocumentChanges, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit, TextEdit,
    };
    if !versioned {
        return Err(
            "this client does not accept versioned edits, so a rename could be applied to a              document that has since changed"
                .to_owned(),
        );
    }
    let mut changes = Vec::with_capacity(files.len());
    for f in files {
        let Some(version) = f.version else {
            return Err(format!(
                "{} must change and is not open; a closed file's content cannot be made a                  condition of the edit, so open it and ask again",
                f.path.display()
            ));
        };
        let uri = uri_of(&f.path).ok_or_else(|| format!("{} has no URI", f.path.display()))?;
        let index = LineIndex::new(&f.text);
        let edits = f
            .edits
            .iter()
            .map(|e| {
                OneOf::Left(TextEdit {
                    range: Range {
                        start: index.position(e.start),
                        end: index.position(e.end),
                    },
                    new_text: e.replacement.clone(),
                })
            })
            .collect();
        changes.push(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri,
                version: Some(version),
            },
            edits,
        });
    }
    Ok(lsp_types::WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Edits(changes)),
        change_annotations: None,
    })
}

/// One document's complete diagnostic set, as a `textDocument/publishDiagnostics`.
fn publish(p: &Published) -> Option<Notification> {
    let index = LineIndex::new(&p.text);
    let diagnostics = p
        .diagnostics
        .iter()
        .map(|d| lsp_diagnostic(d, &index))
        .collect();
    let params = PublishDiagnosticsParams {
        uri: uri_of(&p.path)?,
        diagnostics,
        version: p.version,
    };
    Some(Notification::new(
        lsp_types::notification::PublishDiagnostics::METHOD.to_owned(),
        params,
    ))
}

/// A Nazm diagnostic as the protocol's, positioned in the text `index` is of: the one
/// conversion both published diagnostics and quick fixes use.
fn lsp_diagnostic(d: &nazm_diag::Diagnostic, index: &LineIndex<'_>) -> LspDiagnostic {
    let span = d.primary.span;
    LspDiagnostic {
        range: Range {
            start: index.position(span.start),
            end: index.position(span.end),
        },
        severity: Some(match d.severity {
            nazm_diag::Severity::Error => DiagnosticSeverity::ERROR,
        }),
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("nazm".to_owned()),
        message: d.message.clone(),
        ..LspDiagnostic::default()
    }
}

/// A fix plan's title: what it does and which diagnostic it repairs — and, for a fix that
/// needs review, when it is right, so the choice is made knowing that.
fn fix_title(plan: &nazm_service::FixPlan) -> String {
    match plan.applicability {
        nazm_diag::Applicability::Automatic => {
            format!("Fix {}: {}", plan.diagnostic.code, plan.description)
        }
        nazm_diag::Applicability::NeedsReview => format!(
            "Review fix {}: {} — right only if {}",
            plan.diagnostic.code, plan.description, plan.precondition
        ),
    }
}

/// Whether a quick fix is the one an editor may apply without asking: an automatic fix
/// that is its diagnostic's only one. A fix that needs review never is, and neither is one
/// of several alternatives, which this server has no ground to rank.
fn preferred(plan: &nazm_service::FixPlan) -> bool {
    plan.applicability == nazm_diag::Applicability::Automatic && plan.alternatives == 1
}

// ---------------------------------------------------------------------------------------
// URIs.
// ---------------------------------------------------------------------------------------

/// The filesystem path a `file:` URI names, percent-decoded. `None` for any other scheme:
/// an unsaved buffer with no file (`untitled:`) has no place in a module graph.
fn path_of(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    // `file://host/path` names another machine; only the local form is a path here.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = percent_decode(path)?;
    let text = String::from_utf8(bytes).ok()?;
    Some(PathBuf::from(text))
}

/// A `file:` URI for `path`, with everything outside the unreserved set and `/`
/// percent-encoded.
fn uri_of(path: &Path) -> Option<Uri> {
    let text = path.to_str()?;
    let mut out = String::from("file://");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Uri::from_str(&out).ok()
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------
// Positions: byte offsets in, UTF-16 lines and columns out, and back.
// ---------------------------------------------------------------------------------------

/// Where each line of a text starts, for converting between byte offsets and LSP
/// positions.
///
/// A line ends at `\n`, at `\r\n`, or at a `\r` not followed by `\n` — the three
/// terminators the protocol recognises. A column is a count of **UTF-16 code units** from
/// the start of the line: one for every character in the Basic Multilingual Plane, two for
/// one outside it. Bytes are never counted as characters.
pub(crate) struct LineIndex<'a> {
    text: &'a str,
    /// Byte offset of the first byte of each line.
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        let bytes = text.as_bytes();
        let mut i = 0;
        while let Some(&b) = bytes.get(i) {
            match b {
                b'\n' => starts.push(i + 1),
                b'\r' if bytes.get(i + 1) != Some(&b'\n') => starts.push(i + 1),
                _ => {}
            }
            i += 1;
        }
        Self { text, starts }
    }

    /// The LSP position of byte `offset`, clamped to the end of the text.
    pub(crate) fn position(&self, offset: u32) -> Position {
        let offset = (offset as usize).min(self.text.len());
        let line = self
            .starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1);
        let start = self.starts.get(line).copied().unwrap_or(0);
        let column: usize = self
            .text
            .get(start..offset)
            .map_or(0, |s| s.chars().map(char::len_utf16).sum());
        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            character: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// The byte offset of LSP position `at`. A column past the end of its line is the end
    /// of the line, and a line past the last is the end of the text, as the protocol asks.
    /// A column inside a surrogate pair is the start of that character.
    pub(crate) fn offset(&self, at: Position) -> u32 {
        let Some(&start) = self.starts.get(at.line as usize) else {
            return u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        };
        let end = self
            .starts
            .get(at.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        let line = self.text.get(start..end).unwrap_or_default();
        // The terminator is not part of the line's columns.
        let content = line.trim_end_matches(['\n', '\r']);
        let mut units = 0usize;
        let mut byte = start;
        for c in content.chars() {
            let next = units + c.len_utf16();
            if next > at.character as usize {
                break;
            }
            units = next;
            byte += c.len_utf8();
        }
        u32::try_from(byte).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn ascii_columns_are_byte_columns() {
        let i = LineIndex::new("ab\ncd");
        assert_eq!(i.position(0), pos(0, 0));
        assert_eq!(i.position(2), pos(0, 2), "end of line, before the newline");
        assert_eq!(i.position(3), pos(1, 0));
        assert_eq!(i.position(5), pos(1, 2), "end of file");
        assert_eq!(i.offset(pos(1, 1)), 4);
    }

    #[test]
    fn a_multibyte_character_is_one_utf16_unit_and_several_bytes() {
        // `é` is two bytes and one UTF-16 unit; `→` three bytes, one unit.
        let text = "é→x";
        let i = LineIndex::new(text);
        let x = text.find('x').unwrap();
        assert_eq!(x, 5);
        assert_eq!(i.position(u32::try_from(x).unwrap()), pos(0, 2));
        assert_eq!(i.offset(pos(0, 2)) as usize, x);
    }

    #[test]
    fn a_character_outside_the_bmp_is_two_utf16_units() {
        // `😀` is four bytes and a surrogate pair: two units.
        let text = "a😀b";
        let i = LineIndex::new(text);
        let b = text.find('b').unwrap();
        assert_eq!(i.position(u32::try_from(b).unwrap()), pos(0, 3));
        assert_eq!(i.offset(pos(0, 3)) as usize, b);
        // Inside the pair is the start of the character, never the middle of its bytes.
        assert_eq!(i.offset(pos(0, 2)), 1);
    }

    #[test]
    fn crlf_and_a_lone_cr_end_lines_and_are_not_columns() {
        let text = "ab\r\ncd\ref";
        let i = LineIndex::new(text);
        assert_eq!(i.position(4), pos(1, 0));
        assert_eq!(i.position(7), pos(2, 0));
        assert_eq!(
            i.offset(pos(0, 99)),
            2,
            "past the end is the end, not the `\\r`"
        );
        assert_eq!(i.offset(pos(9, 0)) as usize, text.len());
    }

    #[test]
    fn non_ascii_on_an_earlier_line_does_not_move_a_later_one() {
        let text = "// ☕😀\nfn x";
        let i = LineIndex::new(text);
        let x = text.find('x').unwrap();
        assert_eq!(i.position(u32::try_from(x).unwrap()), pos(1, 3));
    }

    #[test]
    fn a_path_survives_the_round_trip_through_a_uri() {
        let p = PathBuf::from("/tmp/a dir/ünï code/main.nz");
        let uri = uri_of(&p).unwrap();
        assert!(uri.as_str().starts_with("file:///tmp/a%20dir/"));
        assert_eq!(path_of(&uri).unwrap(), p);
        assert_eq!(
            path_of(&Uri::from_str("untitled:Untitled-1").unwrap()),
            None
        );
    }

    fn plan(version: Option<i32>) -> RenamePlan {
        // `é` is two bytes and one UTF-16 unit, `😀` four and two: the edit at byte 9 is
        // at column 6.
        let text = "é😀 = old;";
        let at = u32::try_from(text.find("old").unwrap()).unwrap();
        RenamePlan {
            kind: "local",
            old_name: "old".into(),
            new_name: "new".into(),
            generation: 1,
            files: vec![nazm_service::FileEdit {
                path: PathBuf::from("/tmp/m.nz"),
                version,
                text: text.into(),
                edits: vec![nazm_service::Edit {
                    start: at,
                    end: at + 3,
                    expected: "old".into(),
                    replacement: "new".into(),
                }],
            }],
        }
    }

    #[test]
    fn a_rename_edit_is_versioned_and_positioned_in_utf16() {
        let edit = serde_json::to_value(workspace_edit(&plan(Some(7)), true).unwrap()).unwrap();
        assert_eq!(
            edit["changes"],
            serde_json::Value::Null,
            "never the unversioned form"
        );
        let change = &edit["documentChanges"][0];
        assert_eq!(change["textDocument"]["version"], 7);
        let r = &change["edits"][0]["range"];
        assert_eq!(r["start"], serde_json::json!({"line": 0, "character": 6}));
        assert_eq!(r["end"], serde_json::json!({"line": 0, "character": 9}));
        assert_eq!(change["edits"][0]["newText"], "new");
    }

    #[test]
    fn a_rename_that_the_protocol_cannot_protect_is_refused() {
        // A file that is not open would go out with a `null` version — "the disk is the
        // master" — and nothing would stop it being applied to other bytes.
        let why = workspace_edit(&plan(None), true).unwrap_err();
        assert!(why.contains("not open"), "{why}");
        // A client that does not accept versioned changes gets no edit at all.
        assert!(workspace_edit(&plan(Some(1)), false).is_err());
    }

    #[test]
    fn the_semantic_token_legend_is_fixed_and_every_class_is_in_it() {
        // The legend is a protocol contract: its order is what every token's type and
        // modifier bits mean.
        let l = serde_json::to_value(legend()).unwrap();
        assert_eq!(
            l,
            serde_json::json!({
                "tokenTypes": ["type", "struct", "enum", "typeParameter", "parameter",
                               "variable", "property", "enumMember", "function"],
                "tokenModifiers": ["declaration", "defaultLibrary"]
            })
        );
        use SemanticClass as C;
        let named = |c: C| {
            let (i, owned) = token_type(c);
            (TOKEN_TYPES[i as usize], owned)
        };
        assert_eq!(named(C::Function), ("function", false));
        assert_eq!(named(C::Intrinsic), ("function", true));
        assert_eq!(named(C::Parameter), ("parameter", false));
        assert_eq!(named(C::Variable), ("variable", false));
        assert_eq!(named(C::Record), ("struct", false));
        assert_eq!(named(C::Enum), ("enum", false));
        assert_eq!(named(C::Variant), ("enumMember", false));
        assert_eq!(named(C::Field), ("property", false));
        assert_eq!(named(C::PayloadField), ("property", false));
        assert_eq!(named(C::TypeParameter), ("typeParameter", false));
        assert_eq!(named(C::BuiltinType), ("type", true));
        assert_eq!(
            TOKEN_MODIFIERS[DECLARATION.trailing_zeros() as usize],
            "declaration"
        );
        assert_eq!(
            TOKEN_MODIFIERS[DEFAULT_LIBRARY.trailing_zeros() as usize],
            "defaultLibrary"
        );
    }

    fn ident(start: u32, end: u32, class: SemanticClass, role: Role) -> SemanticIdentifier {
        SemanticIdentifier {
            start,
            end,
            class,
            role,
            target: nazm_service::SemanticTarget::Intrinsic(nazm_core::sema::Intrinsic::StrLen),
        }
    }

    #[test]
    fn semantic_tokens_are_relative_and_counted_in_utf16() {
        // Line 0: `é😀 ab cd` — `ab` at bytes 7..9, UTF-16 column 4; `cd` at 10..12, 7.
        // Line 1 is blank. Line 2: `😀x` — `x` at byte 18, column 2.
        let text = "é😀 ab cd\n\n😀x";
        let ids = [
            ident(7, 9, SemanticClass::Function, Role::Declaration),
            ident(10, 12, SemanticClass::Intrinsic, Role::Reference),
            ident(18, 19, SemanticClass::Variable, Role::Reference),
        ];
        assert_eq!(&text[18..19], "x");
        assert_eq!(
            encode_tokens(text, &ids).unwrap(),
            [0, 4, 2, 8, 1, 0, 3, 2, 8, 2, 2, 2, 1, 5, 0],
            "same line: start relative to the previous token; new line: absolute column"
        );
    }

    #[test]
    fn an_identifier_list_that_would_mean_something_else_is_refused() {
        let text = "abcd efgh\nij";
        let v = |s, e| ident(s, e, SemanticClass::Variable, Role::Reference);
        assert!(encode_tokens(text, &[v(0, 4), v(2, 6)]).is_err(), "overlap");
        assert!(
            encode_tokens(text, &[v(5, 9), v(0, 4)]).is_err(),
            "out of order"
        );
        assert!(
            encode_tokens(text, &[v(0, 4), v(0, 4)]).is_err(),
            "one span twice"
        );
        assert!(encode_tokens(text, &[v(5, 12)]).is_err(), "across lines");
        assert!(encode_tokens(text, &[v(3, 3)]).is_err(), "empty");
        assert!(encode_tokens(text, &[v(0, 4), v(5, 9), v(10, 12)]).is_ok());
    }

    fn fix_plan(
        applicability: nazm_diag::Applicability,
        alternatives: usize,
    ) -> nazm_service::FixPlan {
        let span = nazm_span::Span::new(nazm_span::FileId::ROOT, 0, 1);
        nazm_service::FixPlan {
            diagnostic: nazm_diag::Diagnostic::error(
                nazm_diag::codes::ASSIGN_TO_IMMUTABLE,
                span,
                "m",
            ),
            index: 0,
            alternatives,
            description: "make the binding mutable".to_owned(),
            applicability,
            precondition: "nothing else makes it illegal".to_owned(),
            generation: 1,
            file: plan(Some(1)).files.remove(0),
        }
    }

    #[test]
    fn a_quick_fix_is_preferred_only_when_automatic_and_alone_and_review_says_when() {
        use nazm_diag::Applicability::{Automatic, NeedsReview};
        assert!(preferred(&fix_plan(Automatic, 1)));
        assert!(
            !preferred(&fix_plan(Automatic, 2)),
            "one of several alternatives"
        );
        assert!(
            !preferred(&fix_plan(NeedsReview, 1)),
            "review is never preferred"
        );
        assert_eq!(
            fix_title(&fix_plan(Automatic, 1)),
            "Fix N0310: make the binding mutable"
        );
        assert_eq!(
            fix_title(&fix_plan(NeedsReview, 1)),
            "Review fix N0310: make the binding mutable — right only if nothing else makes it illegal"
        );
    }

    #[test]
    fn a_hierarchy_item_carries_its_locator_and_nothing_more() {
        let text: std::sync::Arc<str> = std::sync::Arc::from("// é😀\nfn f() -> Int { 1 }\n");
        let at = |start: u32, end: u32| nazm_service::Location {
            path: PathBuf::from("/p/main.nz"),
            start,
            end,
            text: text.clone(),
        };
        let bound = Locator {
            root: PathBuf::from("/p/root.nz"),
            version: 7,
            generation: 42,
            file: PathBuf::from("/p/main.nz"),
            name_start: 13,
            name_end: 14,
        };
        let f = HierarchyFunction {
            locator: bound.clone(),
            name: "f".to_owned(),
            detail: None,
            declaration: at(10, 29),
            selection: at(13, 14),
        };
        let item = hierarchy_item(&f, &mut Lines::default()).unwrap();
        assert_eq!(item.kind, LspSymbolKind::FUNCTION);
        assert_eq!(
            (item.range.start, item.selection_range.start),
            (Position::new(1, 0), Position::new(1, 3))
        );
        let data = item.data.clone().unwrap();
        let mut keys: Vec<&String> = data.as_object().unwrap().keys().collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["end", "file", "generation", "root", "start", "version"]
        );
        assert_eq!(locator(&item), Some(bound));
    }

    #[test]
    fn the_server_never_writes_a_source_file() {
        // A quick fix or a rename is returned as an edit for the client to apply; nothing in
        // this module writes a file. Spelled in halves so this test does not find itself.
        let source = include_str!("lsp.rs");
        for forbidden in [concat!("fs::", "write"), concat!("File::", "create")] {
            assert!(!source.contains(forbidden), "`{forbidden}` in lsp.rs");
        }
    }

    #[test]
    fn this_module_never_writes_to_stdout() {
        // Standard output is the protocol stream. The only writer is `lsp-server`.
        // `eprint!` and `eprintln!` go to stderr and are allowed; they are removed first so
        // they do not match the forbidden spellings they contain.
        let source = include_str!("lsp.rs")
            .replace("eprintln!(", "")
            .replace("eprint!(", "");
        for forbidden in ["print!(", "println!(", "dbg!(", "std::io::stdout"] {
            let written = source
                .lines()
                .filter(|l| !l.trim_start().starts_with("//") && !l.contains("for forbidden"))
                .any(|l| l.contains(forbidden));
            assert!(
                !written,
                "`{forbidden}` in lsp.rs would corrupt the protocol"
            );
        }
    }
}
