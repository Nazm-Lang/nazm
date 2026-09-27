//! `nazm-mcp` as an MCP client sees it: the real process, driven by the official Rust SDK's
//! client over stdio. N27.
//!
//! What a packet contains is tested in `crates/nazm-service/tests/context.rs`, without a
//! protocol. What is tested here is the adapter: the handshake and what is advertised, the
//! one tool and its schemas, the packet crossing unchanged, the errors, the security
//! boundary, the disk read at every call, and nothing but protocol on stdout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

const SERVER: &str = env!("CARGO_BIN_EXE_nazm-mcp");
const TOOL: &str = "nazm.semantic_context";

const LIB: &str = "pub fn far() -> Int { 1 }\n";
const MAIN: &str = "use \"lib.nz\";\nstruct P {\n    x: Int,\n}\nenum S {\n    A,\n    B(v: Int),\n}\nfn f(p: P) -> Int { far() + p.x }\nfn g() -> Int { f(P(x: 1)) }\n";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

async fn connect(root: &Path) -> (RunningService<RoleClient, ()>, u32) {
    let transport = TokioChildProcess::new(tokio::process::Command::new(SERVER).configure(|c| {
        c.arg("--root").arg(root);
    }))
    .unwrap();
    let pid = transport.id().unwrap();
    (().serve(transport).await.unwrap(), pid)
}

fn args(file: &str, offset: Value) -> JsonObject {
    serde_json::from_value(json!({"file": file, "byte_offset": offset})).unwrap()
}

async fn call(client: &RunningService<RoleClient, ()>, file: &str, offset: u32) -> (Value, bool) {
    let r = client
        .call_tool(CallToolRequestParams::new(TOOL).with_arguments(args(file, json!(offset))))
        .await
        .unwrap();
    // The packet once: as structured content, and not again as text.
    assert!(r.content.is_empty(), "{:?}", r.content);
    (r.structured_content.unwrap(), r.is_error.unwrap_or(false))
}

/// The packet the language service gives for the same question, directly.
fn direct(root: &Path, file: &Path, offset: u32) -> Value {
    let (root, file) = (root.to_path_buf(), file.to_path_buf());
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut s = nazm_service::LanguageService::new();
                s.open_from_disk(&root).unwrap();
                serde_json::to_value(s.context(&root, 0, &file, offset)).unwrap()
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

fn at(text: &str, needle: &str) -> u32 {
    u32::try_from(text.find(needle).unwrap()).unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn the_server_offers_one_read_only_tool_and_nothing_else() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let info = client.peer_info().unwrap();
        let caps = serde_json::to_value(&info.capabilities).unwrap();
        let keys: Vec<&String> = caps.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["tools"], "only tools are advertised: {caps}");
        assert_eq!(info.server_info.as_ref().unwrap().name, "nazm-mcp");
        let tools = client.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool.name, TOOL);
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        let input = Value::Object((*tool.input_schema).clone());
        assert_eq!(input["required"], json!(["file", "byte_offset"]));
        assert_eq!(input["additionalProperties"], false);
        let mut props: Vec<&String> = input["properties"].as_object().unwrap().keys().collect();
        props.sort();
        assert_eq!(props, ["byte_offset", "file"]);
        // The output schema is `nazm.context/1`, exactly as the repository publishes it.
        let published: Value = serde_json::from_str(
            &std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema/nazm.context-1.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            Value::Object((*tool.output_schema.clone().unwrap()).clone()),
            published
        );
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_packet_crosses_the_protocol_unchanged() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        for needle in ["f(p", "P {", "S {"] {
            let (packet, error) = call(&client, "main.nz", at(MAIN, needle)).await;
            assert!(!error, "{packet}");
            assert_eq!(packet["status"], "available");
            assert_eq!(packet, direct(&root, &root, at(MAIN, needle)), "{needle}");
        }
        let (f, _) = call(&client, "main.nz", at(MAIN, "f(p")).await;
        assert_eq!(f["target"]["kind"], "function");
        assert_eq!(f["callers"]["items"][0]["name"], "g");
        let (p, _) = call(&client, "main.nz", at(MAIN, "P {")).await;
        assert_eq!(p["callers"]["status"], "not_applicable");
        // A file named relative to the source root, as packets name files.
        let (far, _) = call(&client, "lib.nz", at(LIB, "far")).await;
        assert_eq!(far["target"]["identity"]["definition"], "lib.nz::fn far");
        client.cancel().await.unwrap();
    });
}

#[test]
fn bad_requests_are_errors_and_other_files_are_never_read() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    std::fs::write(dir.path().join("loose.nz"), "fn loose() -> Int { 0 }\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "secret\n").unwrap();
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        // Protocol errors: an unknown tool, a missing or mistyped argument, a negative or
        // oversized offset, an argument the schema does not have.
        let bad = [
            CallToolRequestParams::new("write_file").with_arguments(args("main.nz", json!(0))),
            CallToolRequestParams::new(TOOL)
                .with_arguments(serde_json::from_value(json!({"byte_offset": 0})).unwrap()),
            CallToolRequestParams::new(TOOL).with_arguments(args("main.nz", json!("zero"))),
            CallToolRequestParams::new(TOOL).with_arguments(args("main.nz", json!(-1))),
            CallToolRequestParams::new(TOOL).with_arguments(args("main.nz", json!(u64::MAX))),
            CallToolRequestParams::new(TOOL).with_arguments(
                serde_json::from_value(json!({"file": "main.nz", "byte_offset": 0, "root": "x"}))
                    .unwrap(),
            ),
        ];
        for request in bad {
            let name = request.name.clone();
            assert!(client.call_tool(request).await.is_err(), "{name}");
        }
        // Tool results that say why there is no packet: past the end, and files the
        // compilation did not load — however they are named.
        let (past, error) = call(&client, "main.nz", 100_000).await;
        assert!(error);
        assert_eq!(past["reason"], "out_of_range");
        for file in ["/etc/passwd", "../../etc/passwd", "notes.txt", "loose.nz"] {
            let (v, error) = call(&client, file, 0).await;
            assert!(error, "{file}");
            assert_eq!(
                v,
                json!({"schema": "nazm.context/1", "status": "unavailable", "reason": "not_in_compilation"}),
                "{file}"
            );
            assert!(!v.to_string().contains("secret"));
        }
        // And the server carries on.
        let (ok, _) = call(&client, "main.nz", at(MAIN, "g()")).await;
        assert_eq!(ok["status"], "available");
        client.cancel().await.unwrap();
    });
}

#[test]
fn every_call_reads_the_disk_as_it_is_then() {
    const ONE: &str = "fn h() -> Int { 1 }\nfn f() -> Int { h() }\n";
    const TWO: &str = "fn h() -> Int { 1 }\nfn k() -> Int { 2 }\nfn f() -> Int { h() + k() }\n";
    let dir = project(&[("main.nz", ONE)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let callees = |v: &Value| -> Vec<String> {
            v["callees"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap().to_owned())
                .collect()
        };
        let (a, _) = call(&client, "main.nz", at(ONE, "f()")).await;
        assert_eq!(callees(&a), ["h"]);
        // An agent edits the file: the next call sees it, with no restart.
        std::fs::write(&root, TWO).unwrap();
        let (b, _) = call(&client, "main.nz", at(TWO, "f()")).await;
        assert_eq!(callees(&b), ["h", "k"]);
        assert_ne!(a, b);
        // A breaking edit: no packet — not the one from before.
        std::fs::write(
            &root,
            format!("{TWO}fn broken() -> Int {{ let x = ; 1 }}\n"),
        )
        .unwrap();
        let (c, error) = call(&client, "main.nz", at(TWO, "f()")).await;
        assert!(error);
        assert_eq!(c["reason"], "incomplete_compilation");
        // Repaired: a fresh packet.
        std::fs::write(&root, TWO).unwrap();
        let (d, _) = call(&client, "main.nz", at(TWO, "f()")).await;
        assert_eq!(d, b);
        client.cancel().await.unwrap();
    });
}

#[test]
fn two_servers_with_two_roots_answer_from_their_own_compilations() {
    const COMMON: &str = "pub fn shared() -> Int { 0 }\n";
    const A: &str = "use \"common.nz\";\nfn only_a() -> Int { shared() }\n";
    const B: &str = "use \"common.nz\";\nfn only_b() -> Int { shared() + shared() }\n";
    let dir = project(&[("common.nz", COMMON), ("a.nz", A), ("b.nz", B)]);
    runtime().block_on(async {
        let (a, _) = connect(&dir.path().join("a.nz")).await;
        let (b, _) = connect(&dir.path().join("b.nz")).await;
        let (via_a, _) = call(&a, "common.nz", 7).await;
        let (via_b, _) = call(&b, "common.nz", 7).await;
        assert_eq!(via_a["callers"]["items"][0]["name"], "only_a");
        assert_eq!(via_b["callers"]["items"][0]["name"], "only_b");
        assert_eq!(via_a["references"].as_array().unwrap().len(), 1);
        assert_eq!(via_b["references"].as_array().unwrap().len(), 2);
        a.cancel().await.unwrap();
        b.cancel().await.unwrap();
    });
}

fn rss_kb(pid: u32) -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[test]
fn a_thousand_calls_keep_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let targets = [
            at(MAIN, "f(p"),
            at(MAIN, "P {"),
            at(MAIN, "S {"),
            at(MAIN, "g()"),
        ];
        let first: Vec<Value> = {
            let mut v = Vec::new();
            for t in targets {
                v.push(call(&client, "main.nz", t).await.0);
            }
            v
        };
        // Warm: the allocator's high-water mark after one round of every shape.
        for _ in 0..25 {
            for t in targets {
                call(&client, "main.nz", t).await;
            }
        }
        let settled = rss_kb(pid);
        for i in 0..1000 {
            let k = i % targets.len();
            let (v, _) = call(&client, "main.nz", targets[k]).await;
            assert_eq!(v, first[k], "call {i}");
        }
        let after = rss_kb(pid);
        // Nothing accumulates: resident memory after a thousand more calls is within a
        // megabyte of where it had settled.
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!("nazm-mcp RSS: settled {settled} kB, after 1,000 calls {after} kB");
        client.cancel().await.unwrap();
    });
}

#[test]
fn standard_output_is_the_protocol_and_nothing_else() {
    // Raw stdio, newline-delimited JSON-RPC as MCP's stdio transport frames it: every line
    // the server writes, across a handshake, a listing, a packet, an unknown tool and a
    // broken request, is one JSON-RPC message.
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let mut child = Command::new(SERVER)
        .arg("--root")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let messages = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "raw", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": TOOL, "arguments": {"file": "main.nz", "byte_offset": at(MAIN, "f(p")}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "shell", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "prompts/list"}),
    ];
    for m in &messages {
        writeln!(stdin, "{m}").unwrap();
    }
    stdin.flush().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut seen = Vec::new();
    while seen.len() < 6 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let v: Value =
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("not protocol: {line:?}: {e}"));
        assert_eq!(v["jsonrpc"], "2.0", "{line}");
        seen.push(v);
    }
    drop(stdin);
    let _ = child.wait();
    // Responses may come in any order — a tool call is answered when its analysis is done —
    // and each is matched to its request by id.
    let mut ids: Vec<i64> = seen.iter().filter_map(|v| v["id"].as_i64()).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6]);
    let by = |id: i64| seen.iter().find(|v| v["id"] == id).unwrap();
    // The packet's schema field is on the wire once: the packet travels once, as
    // `structuredContent`, with an empty `content` — no second serialisation as text.
    let answer = by(3);
    assert_eq!(answer["result"]["structuredContent"]["status"], "available");
    assert_eq!(answer["result"]["content"], json!([]));
    assert_eq!(
        answer["result"]["structuredContent"],
        direct(&root, &root, at(MAIN, "f(p"))
    );
    let wire = answer.to_string();
    assert_eq!(wire.matches("nazm.context/1").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"availability\"").count(), 1, "{wire}");
    assert!(by(4).get("error").is_some(), "unknown tool: {}", by(4));
    assert!(by(5).get("error").is_some(), "no resources: {}", by(5));
    assert!(by(6).get("error").is_some(), "no prompts: {}", by(6));
}
