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
//! references (N17), prepare-rename and rename (N18), and completion (N19) — of a value, a
//! call's head or a type, invoked on an identifier, with no trigger characters. No signature
//! help, formatting, symbols, code actions, semantic tokens, watched files or cancellation — a
//! client is never told something exists that the server would then ignore.
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
    GotoDefinitionParams, GotoDefinitionResponse, Hover as LspHover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeResult, Location as LspLocation, MarkupContent, MarkupKind,
    NumberOrString, OneOf, Position, PositionEncodingKind, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, ReferenceParams, RenameOptions, RenameParams,
    ServerCapabilities, ServerInfo, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};
use nazm_service::{LanguageService, Published, RenamePlan, Target};

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
        // Invoked, not triggered: completion is of an identifier being written, and every
        // item is final — there is nothing more to resolve.
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            ..CompletionOptions::default()
        }),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        })),
        ..ServerCapabilities::default()
    }
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
    connection
        .initialize_finish(id, init)
        .map_err(|e| format!("initialize: {e}"))?;

    let mut server = Server {
        service: LanguageService::with_source_root(root),
        versions: HashMap::new(),
        versioned_edits,
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
            Completion, GotoDefinition, HoverRequest, PrepareRenameRequest, References, Rename,
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

/// A rename plan as the protocol's edit: `documentChanges`, one per file, each tied to the
/// version of the open document the plan was validated against.
///
/// Refused, not weakened, when the protocol cannot hold the client to the plan's
/// precondition: a client that does not accept versioned changes, or a file the plan edits
/// that is not open — whose version would be `null`, which the protocol defines as "the
/// disk is the master", with no check of what the disk now holds.
fn workspace_edit(plan: &RenamePlan, versioned: bool) -> Result<lsp_types::WorkspaceEdit, String> {
    use lsp_types::{
        DocumentChanges, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit, TextEdit,
    };
    if !versioned {
        return Err(
            "this client does not accept versioned edits, so a rename could be applied to a              document that has since changed"
                .to_owned(),
        );
    }
    let mut changes = Vec::with_capacity(plan.files.len());
    for f in &plan.files {
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
        .map(|d| {
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
        })
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
