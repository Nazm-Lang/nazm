//! `nazm-mcp` — a read-only Model Context Protocol server over stdio. N27–N31.
//!
//! # What it is
//!
//! An adapter. It speaks MCP through the official Rust SDK (`rmcp`) and serves seven tools,
//! each result exactly the object the matching command prints with `--json`:
//!
//! | Tool | Result | Command |
//! | --- | --- | --- |
//! | `nazm.semantic_context` | `nazm.context/1` for a position (N27) | `nazm context` |
//! | `nazm.semantic_snapshot` | `nazm.snapshot/1` of the root's compilation (N28) | `nazm snapshot` |
//! | `nazm.semantic_delta` | `nazm.delta/1` against a baseline snapshot the client supplies (N28) | `nazm delta` |
//! | `nazm.semantic_patch` | `nazm.patch/1`, a planned rename or fix (N29) | `nazm patch` |
//! | `nazm.diagnostics` | `nazm.diagnostic-index/1`, every current diagnostic compactly (N30) | `nazm diagnostics` |
//! | `nazm.diagnostic_detail` | `nazm.diagnostic-detail/1`, one of them in full (N30) | `nazm diagnostics --detail` |
//! | `nazm.command_summary` | `nazm.command-summary/1` of a check (N31) | `nazm check --summary-json --no-cache` |
//!
//! `nazm.command_summary` checks and does not build: a build writes an executable, and
//! running a program's tests runs programs, which may write files — neither is read-only,
//! so neither is offered. The check runs without the check cache, so it writes nothing.
//!
//! Each is carried once, as the result's structured content, with no second copy as text.
//! It creates no semantic fact of its own: every answer is the compiler's, derived for the
//! call and dropped.
//!
//! # A delta is stateless
//!
//! The server keeps no previous snapshot. A client that wants to know what changed keeps
//! the snapshot it was given and hands it back as `baseline` — data, validated as
//! `nazm.snapshot/1` and compared with a snapshot derived from the disk at that call. A
//! baseline is never a path: the server reads no file a client names.
//!
//! # A patch is a plan
//!
//! `nazm.semantic_patch` plans a rename or a diagnostic fix and returns it; it applies
//! nothing, and there is no tool that does. A patch is a proposal bound to exact source
//! state — the root's snapshot, each file's digest, each edit's expected bytes — not
//! permission to change a file. No plan is kept: the same request later is planned again
//! from the disk as it is then.
//!
//! # Which program, and which text
//!
//! The root given at startup fixes the compilation for the server's lifetime; a call cannot
//! name another. Each call opens that root **from disk as it is at that moment**, analyses
//! it, derives the packet and drops both — so an edit an agent has just written is what the
//! next call sees, with no watcher and nothing cached, and a call never answers from an
//! earlier analysis. It does not see an editor's unsaved buffers.
//!
//! # What it cannot do
//!
//! Write, rename, run anything, or read a file the root's compilation did not load: the
//! `file` argument is matched against the compilation's own files by identity, and a path
//! that is not one of them — `/etc/passwd`, a `.nz` file nothing imports — is refused
//! without being opened. Only stdio: no HTTP, no authentication. No resources, prompts or
//! sampling. Standard output is the protocol and nothing else writes to it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use nazm_service::{
    ContextResult, DeltaResult, DetailResult, IndexResult, LanguageService, PatchResult,
    SnapshotResult,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};

/// The context tool (N27).
const TOOL: &str = "nazm.semantic_context";

/// `nazm.context/1`, the context tool's output schema, exactly as the repository publishes it.
const OUTPUT_SCHEMA: &str = include_str!("../../../schema/nazm.context-1.json");

/// The snapshot tool (N28).
const SNAPSHOT: &str = "nazm.semantic_snapshot";

/// `nazm.snapshot/1`, exactly as the repository publishes it.
const SNAPSHOT_SCHEMA: &str = include_str!("../../../schema/nazm.snapshot-1.json");

/// The delta tool (N28).
const DELTA: &str = "nazm.semantic_delta";

/// `nazm.delta/1`, exactly as the repository publishes it.
const DELTA_SCHEMA: &str = include_str!("../../../schema/nazm.delta-1.json");

/// The patch-planning tool (N29).
const PATCH: &str = "nazm.semantic_patch";

/// `nazm.patch/1`, exactly as the repository publishes it.
const PATCH_SCHEMA: &str = include_str!("../../../schema/nazm.patch-1.json");

/// The diagnostic index tool (N30).
const DIAGNOSTICS: &str = "nazm.diagnostics";

/// `nazm.diagnostic-index/1`, exactly as the repository publishes it.
const INDEX_SCHEMA: &str = include_str!("../../../schema/nazm.diagnostic-index-1.json");

/// The diagnostic detail tool (N30).
const DETAIL: &str = "nazm.diagnostic_detail";

/// `nazm.diagnostic-detail/1`, exactly as the repository publishes it.
const DETAIL_SCHEMA: &str = include_str!("../../../schema/nazm.diagnostic-detail-1.json");

/// The command summary tool (N31).
const SUMMARY: &str = "nazm.command_summary";

/// `nazm.command-summary/1`, exactly as the repository publishes it.
const SUMMARY_SCHEMA: &str = include_str!("../../../schema/nazm.command-summary-1.json");

/// What `nazm.command_summary` is asked: one operation, from a finite set that is read-only
/// — only `check`. Typed, so its input schema is the SDK's derivation of it.
#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum SummaryRequest {
    /// `nazm check` of the root, as it is on disk now, without the check cache.
    Check {},
}

/// What `nazm.diagnostic_detail` is asked: one diagnostic, by the id and the state an index
/// gave it. Typed, so its input schema is the SDK's derivation of it.
#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
struct DetailRequest {
    /// A diagnostic's `id`, from `nazm.diagnostics`.
    id: String,
    /// That index's `state.source`: the sources the id is of.
    state: String,
}

/// What `nazm.semantic_patch` is asked, tagged by `operation`: typed, so its input schema is
/// the SDK's derivation of it and an argument it does not define is refused.
#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum PatchRequest {
    /// Rename the entity declared or used at `byte_offset` of `file`: locals and private
    /// definitions only, as the language service validates a rename.
    Rename {
        /// A file of the root's compilation, relative to its source root or absolute.
        file: String,
        /// A UTF-8 byte offset into that file.
        byte_offset: u32,
        /// The new name: one identifier.
        new_name: String,
    },
    /// The fix at index `fix` among those the current diagnostics at `byte_offset` of
    /// `file` carry, in the order the compiler attached them.
    DiagnosticFix {
        /// A file of the root's compilation, relative to its source root or absolute.
        file: String,
        /// A UTF-8 byte offset into that file: a diagnostic's primary span there.
        byte_offset: u32,
        /// Which of the fixes at that position.
        fix: u32,
    },
}

fn main() -> ExitCode {
    let (root, source_root) = match arguments(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nazm-mcp: {e}\nusage: nazm-mcp --root <file.nz> [--source-root <dir>]");
            return ExitCode::from(2);
        }
    };
    if !root.is_file() {
        eprintln!("nazm-mcp: {} is not a file", root.display());
        return ExitCode::from(2);
    }
    // Standard input and output are served by tokio's blocking pool, so the runtime needs
    // no I/O reactor of its own; the SDK's shutdown uses timers.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    let server = Server { root, source_root };
    let served = runtime.block_on(async move {
        let running = server.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    });
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nazm-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `--root <file>` and an optional `--source-root <dir>`, and nothing else.
fn arguments(mut args: impl Iterator<Item = String>) -> Result<(PathBuf, Option<PathBuf>), String> {
    let (mut root, mut source_root) = (None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = Some(PathBuf::from(args.next().ok_or("--root needs a file")?)),
            "--source-root" => {
                source_root = Some(PathBuf::from(
                    args.next().ok_or("--source-root needs a directory")?,
                ));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok((root.ok_or("--root is required")?, source_root))
}

/// The server: a root, fixed for its lifetime, and nothing else — no analysis, no packet,
/// no snapshot and no history is kept between calls.
#[derive(Debug, Clone)]
struct Server {
    root: PathBuf,
    source_root: Option<PathBuf>,
}

impl Server {
    /// Every tool the server has.
    fn tools() -> Vec<Tool> {
        vec![
            Self::tool(),
            Self::snapshot_tool(),
            Self::delta_tool(),
            Self::patch_tool(),
            Self::diagnostics_tool(),
            Self::detail_tool(),
            Self::summary_tool(),
        ]
    }

    /// The context tool, with `nazm.context/1` as its output schema.
    fn tool() -> Tool {
        let input: JsonObject = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "A file of the root's compilation, relative to its source root (as packets name files) or absolute."
                },
                "byte_offset": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": u32::MAX,
                    "description": "The position, as a UTF-8 byte offset into that file."
                }
            },
            "required": ["file", "byte_offset"],
            "additionalProperties": false
        }))
        .unwrap_or_default();
        let output: JsonObject = serde_json::from_str(OUTPUT_SCHEMA).unwrap_or_default();
        let mut tool = Tool::new(
            TOOL,
            "The semantic context packet (nazm.context/1) of the function, record or enum declared or used at a byte offset: its exact source, and compact links — identity, kind, name, place — to what it uses, what uses it, what it calls and what calls it, in the server's root compilation as it is on disk now. Effects, capabilities, tests and semantic changes are reported unsupported.",
            Arc::new(input),
        )
        .annotate(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        );
        tool.output_schema = Some(Arc::new(output));
        tool
    }

    /// The snapshot tool: no arguments, `nazm.snapshot/1` as its output schema.
    fn snapshot_tool() -> Tool {
        let input: JsonObject = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }))
        .unwrap_or_default();
        Self::read_only(
            SNAPSHOT,
            "The semantic snapshot (nazm.snapshot/1) of the server's root compilation as it is on disk now: every durable function, record and enum, by identity, with one digest per semantic section the compiler records — source, shape, dependencies, related types, references, callers, callees, diagnostics. Keep it and pass it to nazm.semantic_delta later. Digest equality is not behavioural equivalence; effects, capabilities and tests are unsupported.",
            input,
            SNAPSHOT_SCHEMA,
        )
    }

    /// The delta tool: a baseline snapshot in, `nazm.delta/1` as its output schema.
    fn delta_tool() -> Tool {
        let input: JsonObject = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "baseline": {
                    "type": "object",
                    "description": "A nazm.snapshot/1 object exactly as nazm.semantic_snapshot returned it. Data, never a path."
                }
            },
            "required": ["baseline"],
            "additionalProperties": false
        }))
        .unwrap_or_default();
        Self::read_only(
            DELTA,
            "What changed (nazm.delta/1) between a baseline nazm.snapshot/1 the client kept and the server's root compilation as it is on disk now: durable definitions added, removed, and changed, with exactly which recorded sections changed. Matched by durable key only — a rename is a removal and an addition. A baseline of another root or compiler, or not exactly nazm.snapshot/1, is refused. The server keeps no previous snapshot.",
            input,
            DELTA_SCHEMA,
        )
    }

    /// The patch tool: a tagged operation in, `nazm.patch/1` as its output schema.
    fn patch_tool() -> Tool {
        let mut input = (*rmcp::handler::server::common::schema_for_type::<PatchRequest>()).clone();
        // A tool's input is an object; the derivation of a tagged enum says so per variant.
        input
            .entry("type")
            .or_insert_with(|| serde_json::Value::from("object"));
        Self::read_only(
            PATCH,
            "Plan a semantic patch (nazm.patch/1) in the server's root compilation as it is on disk now: a rename (operation `rename`: file, byte_offset, new_name — locals and private definitions only; exported ones are refused) or a compiler-owned diagnostic fix (operation `diagnostic_fix`: file, byte_offset, fix). The result is minimal exact-byte edits bound to the root's snapshot digest, each file's digest and each edit's expected bytes, with a fix's applicability and precondition. It plans and never applies: nothing is written.",
            input,
            PATCH_SCHEMA,
        )
    }

    /// The index tool: no arguments, `nazm.diagnostic-index/1` as its output schema.
    fn diagnostics_tool() -> Tool {
        let input: JsonObject = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }))
        .unwrap_or_default();
        Self::read_only(
            DIAGNOSTICS,
            "Every current diagnostic of the server's root compilation, from disk now, compactly (nazm.diagnostic-index/1): each one's id, code, severity, file and byte range, owning definition and fix counts — no message or help text. Ask nazm.diagnostic_detail for one in full. Syntax errors included.",
            input,
            INDEX_SCHEMA,
        )
    }

    /// The detail tool: an id and a state in, `nazm.diagnostic-detail/1` as its output schema.
    fn detail_tool() -> Tool {
        let mut input =
            (*rmcp::handler::server::common::schema_for_type::<DetailRequest>()).clone();
        input
            .entry("type")
            .or_insert_with(|| serde_json::Value::from("object"));
        Self::read_only(
            DETAIL,
            "One current diagnostic in full (nazm.diagnostic-detail/1), by the id and state.source a nazm.diagnostics index gave it: the compiler's diagnostic exactly as nazm.diagnostic/1 publishes it, its places in their files, its owner, and for each fix the nazm.semantic_patch request that plans it where one exists. Refused as stale_state if the sources have changed since the index.",
            input,
            DETAIL_SCHEMA,
        )
    }

    /// The summary tool: a typed operation in, `nazm.command-summary/1` as its output schema.
    fn summary_tool() -> Tool {
        let mut input =
            (*rmcp::handler::server::common::schema_for_type::<SummaryRequest>()).clone();
        input
            .entry("type")
            .or_insert_with(|| serde_json::Value::from("object"));
        Self::read_only(
            SUMMARY,
            "What `nazm check` concludes for the server's root as it is on disk now, compactly (nazm.command-summary/1): its status and exit status, and a reference to every diagnostic it reports — code, place, fix counts and, where nazm.diagnostics indexes it, its id — including a program's missing `main`. No prose. operation: `check` only; building and testing write files and are not offered. Nothing is written.",
            input,
            SUMMARY_SCHEMA,
        )
    }

    fn read_only(
        name: &'static str,
        description: &'static str,
        input: JsonObject,
        output: &str,
    ) -> Tool {
        let output: JsonObject = serde_json::from_str(output).unwrap_or_default();
        let mut tool = Tool::new(name, description, Arc::new(input)).annotate(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        );
        tool.output_schema = Some(Arc::new(output));
        tool
    }

    /// The packet for `file` at `offset`, from the disk as it is now.
    fn packet(&self, file: &str, offset: u32) -> Result<ContextResult, String> {
        // Relative to the source root, as a packet names files; the path is only ever
        // compared with the compilation's own files, never opened.
        let base = nazm_service::root::decide(&self.root, self.source_root.as_deref())
            .map(|r| r.path().to_path_buf())
            .map_err(|e| e.to_string())?;
        let file = base.join(Path::new(file));
        self.analysed(move |service, root| service.context(root, 0, &file, offset))
    }

    /// Open the root from disk as it is now, in a fresh service on the parser's stack, ask
    /// it `f`, and drop both.
    fn analysed<T: Send>(
        &self,
        f: impl FnOnce(&mut LanguageService, &Path) -> T + Send,
    ) -> Result<T, String> {
        let (root, source_root) = (self.root.clone(), self.source_root.clone());
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                .spawn_scoped(scope, move || {
                    let mut service = LanguageService::with_source_root(source_root);
                    service
                        .open_from_disk(&root)
                        .map_err(|e| format!("{}: {e}", root.display()))?;
                    Ok(f(&mut service, &root))
                })
                .map_err(|e| e.to_string())?
                .join()
                .map_err(|_| "the analysis stopped unexpectedly".to_owned())?
        })
    }
}

/// One result, once: as structured content, with no second copy as text. The SDK's
/// constructors also copy it into a text block, as the specification suggests for older
/// clients; that would send the whole object twice, so the text is left out. `content` is
/// a required array with no minimum length, so an empty one is valid in every version the
/// SDK negotiates.
fn once(value: serde_json::Value, available: bool) -> CallToolResponse {
    let mut answer = if available {
        CallToolResult::structured(value)
    } else {
        CallToolResult::structured_error(value)
    };
    answer.content.clear();
    answer.into()
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nazm-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read-only tools over this server's root compilation, read from disk at each \
                 call. nazm.semantic_context: the compiler's semantic context packet for a \
                 definition; follow a link by calling it again at the linked declaration. \
                 nazm.semantic_snapshot: the compilation's durable definitions with a digest \
                 per recorded section; keep it. nazm.semantic_delta: pass a kept snapshot as \
                 baseline to learn what was added, removed or changed since. \
                 nazm.semantic_patch: plan a rename or a diagnostic fix as exact edits bound to \
                 the current source; it never applies one. nazm.diagnostics: every current \
                 diagnostic compactly; nazm.diagnostic_detail: one in full, by its id and the \
                 index's state. nazm.command_summary: what `nazm check` concludes, compactly. \
                 The server keeps nothing between calls.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: Self::tools(),
            ..ListToolsResult::default()
        })
    }

    // Resources, prompts and completions are not offered, and asking for them is a method
    // the server does not have — not an empty list, which would claim the feature exists.
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        Err(ErrorData::method_not_found::<
            rmcp::model::ListResourcesRequestMethod,
        >())
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult, ErrorData> {
        Err(ErrorData::method_not_found::<
            rmcp::model::ListResourceTemplatesRequestMethod,
        >())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListPromptsResult, ErrorData> {
        Err(ErrorData::method_not_found::<
            rmcp::model::ListPromptsRequestMethod,
        >())
    }

    async fn complete(
        &self,
        _request: rmcp::model::CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CompleteResult, ErrorData> {
        Err(ErrorData::method_not_found::<
            rmcp::model::CompleteRequestMethod,
        >())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::tools().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        match &*request.name {
            TOOL => {}
            SNAPSHOT => return self.snapshot(&args).await,
            DELTA => return self.delta(args).await,
            PATCH => return self.patch(args).await,
            DIAGNOSTICS => return self.diagnostics(&args).await,
            DETAIL => return self.detail(args).await,
            SUMMARY => return self.summary(args).await,
            _ => return Err(ErrorData::invalid_params("tool not found", None)),
        }
        let file = args
            .get("file")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ErrorData::invalid_params("`file` must be a string", None))?
            .to_owned();
        let offset = args
            .get("byte_offset")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                ErrorData::invalid_params("`byte_offset` must be an integer from 0 to 2^32-1", None)
            })?;
        if let Some(extra) = args.keys().find(|k| *k != "file" && *k != "byte_offset") {
            return Err(ErrorData::invalid_params(
                format!("unexpected argument `{extra}`"),
                None,
            ));
        }
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || server.packet(&file, offset))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, ContextResult::Available(_))))
    }
}

impl Server {
    /// `nazm.semantic_snapshot`: no arguments.
    async fn snapshot(&self, args: &JsonObject) -> Result<CallToolResponse, ErrorData> {
        if let Some(extra) = args.keys().next() {
            return Err(ErrorData::invalid_params(
                format!("unexpected argument `{extra}`"),
                None,
            ));
        }
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            server.analysed(|service, root| service.semantic_snapshot(root, 0))
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, SnapshotResult::Available(_))))
    }

    /// `nazm.diagnostics`: no arguments.
    async fn diagnostics(&self, args: &JsonObject) -> Result<CallToolResponse, ErrorData> {
        if let Some(extra) = args.keys().next() {
            return Err(ErrorData::invalid_params(
                format!("unexpected argument `{extra}`"),
                None,
            ));
        }
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            server.analysed(|service, root| service.diagnostic_index(root, 0))
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, IndexResult::Available(_))))
    }

    /// `nazm.command_summary`: a check of the root, from the disk as it is now.
    async fn summary(&self, args: JsonObject) -> Result<CallToolResponse, ErrorData> {
        let SummaryRequest::Check {} = serde_json::from_value(serde_json::Value::Object(args))
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let (root, source_root) = (self.root.clone(), self.source_root.clone());
        let summary = tokio::task::spawn_blocking(move || {
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                    .spawn_scoped(scope, || {
                        nazm_service::summary::check(&root, source_root.as_deref())
                    })
                    .map_err(|e| e.to_string())?
                    .join()
                    .map_err(|_| "the check stopped unexpectedly".to_owned())
            })
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&summary)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        // The summary is the answer whatever the check concluded: a rejected program is a
        // result reported, not a tool error. Its `status` says how the check ended.
        Ok(once(value, true))
    }

    /// `nazm.diagnostic_detail`: an id and the state it is of.
    async fn detail(&self, args: JsonObject) -> Result<CallToolResponse, ErrorData> {
        let request: DetailRequest = serde_json::from_value(serde_json::Value::Object(args))
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            server.analysed(move |service, root| {
                service.diagnostic_detail(root, 0, &request.id, &request.state)
            })
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, DetailResult::Available(_))))
    }

    /// `nazm.semantic_patch`: a tagged operation, planned against the disk as it is now.
    async fn patch(&self, args: JsonObject) -> Result<CallToolResponse, ErrorData> {
        let request: PatchRequest = serde_json::from_value(serde_json::Value::Object(args))
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let base = nazm_service::root::decide(&self.root, self.source_root.as_deref())
            .map(|r| r.path().to_path_buf())
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            // Relative to the source root, as a patch names files; the path is only ever
            // compared with the compilation's own files, never opened.
            server.analysed(move |service, root| match request {
                PatchRequest::Rename {
                    file,
                    byte_offset,
                    new_name,
                } => service.patch_rename(root, 0, &base.join(file), byte_offset, &new_name),
                PatchRequest::DiagnosticFix {
                    file,
                    byte_offset,
                    fix,
                } => service.patch_fix(root, 0, &base.join(file), byte_offset, fix as usize),
            })
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, PatchResult::Available(_))))
    }

    /// `nazm.semantic_delta`: exactly one argument, the baseline snapshot, as data.
    async fn delta(&self, mut args: JsonObject) -> Result<CallToolResponse, ErrorData> {
        let baseline = args
            .remove("baseline")
            .ok_or_else(|| ErrorData::invalid_params("`baseline` is required", None))?;
        if let Some(extra) = args.keys().next() {
            return Err(ErrorData::invalid_params(
                format!("unexpected argument `{extra}`"),
                None,
            ));
        }
        let server = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            server.analysed(move |service, root| service.semantic_delta(root, 0, &baseline))
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let value = serde_json::to_value(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(once(value, matches!(result, DeltaResult::Available(_))))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    /// The structural half of "read-only": nothing here writes, renames or deletes a file,
    /// runs a command, or prints to standard output, which is the protocol.
    #[test]
    fn the_server_writes_nothing_runs_nothing_and_prints_nothing() {
        let source = include_str!("main.rs");
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
            concat!("fs::", "write"),
            concat!("remove_", "file"),
            concat!("fs::", "rename"),
            concat!("create_", "dir"),
            concat!("File::", "create"),
            concat!("process::", "Command"),
            concat!("Command::", "new"),
            concat!("std", "out"),
            concat!("read_", "to_string"),
            concat!("fs::", "read"),
        ] {
            assert!(!code.contains(forbidden), "`{forbidden}` in nazm-mcp");
        }
        // Printing only to standard error: every `print` is an `eprint`.
        for printing in [concat!("print", "ln!"), concat!("print", "!(")] {
            for (i, _) in code.match_indices(printing) {
                assert!(
                    i > 0 && code.as_bytes()[i - 1] == b'e',
                    "`{printing}` to standard output in nazm-mcp"
                );
            }
        }
    }

    #[test]
    fn the_output_schema_is_the_published_one() {
        let tools = super::Server::tools();
        for (tool, (name, schema)) in tools.into_iter().zip([
            (super::TOOL, super::OUTPUT_SCHEMA),
            (super::SNAPSHOT, super::SNAPSHOT_SCHEMA),
            (super::DELTA, super::DELTA_SCHEMA),
            (super::PATCH, super::PATCH_SCHEMA),
            (super::DIAGNOSTICS, super::INDEX_SCHEMA),
            (super::DETAIL, super::DETAIL_SCHEMA),
            (super::SUMMARY, super::SUMMARY_SCHEMA),
        ]) {
            let published: serde_json::Value = serde_json::from_str(schema).unwrap();
            assert_eq!(
                serde_json::Value::Object((*tool.output_schema.unwrap()).clone()),
                published
            );
            assert_eq!(tool.name, name);
        }
    }
}
