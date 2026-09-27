//! `nazm-mcp` — a read-only Model Context Protocol server over stdio. N27.
//!
//! # What it is
//!
//! An adapter. It speaks MCP through the official Rust SDK (`rmcp`) and serves one tool,
//! `nazm.semantic_context`, whose result is the language service's `nazm.context/1` packet
//! for a position — exactly the object `nazm context --json` prints. It creates no semantic
//! fact of its own: every answer is the compiler's, derived for the call and dropped.
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

use nazm_service::{ContextResult, LanguageService};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};

/// The one tool.
const TOOL: &str = "nazm.semantic_context";

/// `nazm.context/1`, the tool's output schema, exactly as the repository publishes it.
const OUTPUT_SCHEMA: &str = include_str!("../../../schema/nazm.context-1.json");

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

/// The server: a root, fixed for its lifetime, and nothing else — no analysis, no packet
/// and no history is kept between calls.
#[derive(Debug, Clone)]
struct Server {
    root: PathBuf,
    source_root: Option<PathBuf>,
}

impl Server {
    /// The one tool, with `nazm.context/1` as its output schema.
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

    /// The packet for `file` at `offset`, from the disk as it is now.
    fn packet(&self, file: &str, offset: u32) -> Result<ContextResult, String> {
        let (root, source_root) = (self.root.clone(), self.source_root.clone());
        // Relative to the source root, as a packet names files; the path is only ever
        // compared with the compilation's own files, never opened.
        let base = nazm_service::root::decide(&root, source_root.as_deref())
            .map(|r| r.path().to_path_buf())
            .map_err(|e| e.to_string())?;
        let file = base.join(Path::new(file));
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                .spawn_scoped(scope, move || {
                    let mut service = LanguageService::with_source_root(source_root);
                    service
                        .open_from_disk(&root)
                        .map_err(|e| format!("{}: {e}", root.display()))?;
                    Ok(service.context(&root, 0, &file, offset))
                })
                .map_err(|e| e.to_string())?
                .join()
                .map_err(|_| "the analysis stopped unexpectedly".to_owned())?
        })
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nazm-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "One read-only tool, nazm.semantic_context: the compiler's semantic context \
                 packet for a definition in this server's root compilation, read from disk \
                 at each call. Follow a link by calling it again at the linked declaration.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![Self::tool()],
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
        (name == TOOL).then(Self::tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != TOOL {
            return Err(ErrorData::invalid_params("tool not found", None));
        }
        let args = request.arguments.unwrap_or_default();
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
        Ok(match result {
            ContextResult::Available(_) => CallToolResult::structured(value),
            ContextResult::Unavailable(_) => CallToolResult::structured_error(value),
        }
        .into())
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
        let tool = super::Server::tool();
        let published: serde_json::Value = serde_json::from_str(super::OUTPUT_SCHEMA).unwrap();
        assert_eq!(
            serde_json::Value::Object((*tool.output_schema.unwrap()).clone()),
            published
        );
        assert_eq!(tool.name, super::TOOL);
    }
}
