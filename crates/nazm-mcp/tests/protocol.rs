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
const SNAPSHOT: &str = "nazm.semantic_snapshot";
const DELTA: &str = "nazm.semantic_delta";
const PATCH: &str = "nazm.semantic_patch";
const DIAGNOSTICS: &str = "nazm.diagnostics";
const DETAIL: &str = "nazm.diagnostic_detail";
const SUMMARY: &str = "nazm.command_summary";
const DOCS_INDEX: &str = "nazm.docs_index";
const DOCS_SECTION: &str = "nazm.docs_section";
const REPOSITORY_MAP: &str = "nazm.repository_map";
const TASK_CONTEXT: &str = "nazm.task_context";

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

/// A snapshot or delta tool call, and whether it was a tool error — checking, as for a
/// packet, that the result travels once.
async fn call_with(
    client: &RunningService<RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> (Value, bool) {
    let r = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(serde_json::from_value(arguments).unwrap()),
        )
        .await
        .unwrap();
    assert!(r.content.is_empty(), "{:?}", r.content);
    (r.structured_content.unwrap(), r.is_error.unwrap_or(false))
}

/// The snapshot the language service gives for the same root, directly.
fn direct_snapshot(root: &Path) -> Value {
    let root = root.to_path_buf();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut s = nazm_service::LanguageService::new();
                s.open_from_disk(&root).unwrap();
                serde_json::to_value(s.semantic_snapshot(&root, 0)).unwrap()
            })
            .unwrap()
            .join()
            .unwrap()
    })
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
fn the_server_offers_read_only_tools_and_nothing_else() {
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
        let names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
        assert_eq!(
            names,
            [
                TOOL,
                SNAPSHOT,
                DELTA,
                PATCH,
                DIAGNOSTICS,
                DETAIL,
                SUMMARY,
                DOCS_INDEX,
                DOCS_SECTION,
                REPOSITORY_MAP,
                TASK_CONTEXT
            ]
        );
        for (tool, schema) in tools.iter().zip([
            "nazm.context-3.json",
            "nazm.snapshot-3.json",
            "nazm.delta-3.json",
            "nazm.patch-1.json",
            "nazm.diagnostic-index-1.json",
            "nazm.diagnostic-detail-1.json",
            "nazm.command-summary-1.json",
            "nazm.docs-index-1.json",
            "nazm.docs-section-1.json",
            "nazm.repository-map-1.json",
            "nazm.task-context-1.json",
        ]) {
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.read_only_hint, Some(true), "{}", tool.name);
            assert_eq!(annotations.destructive_hint, Some(false), "{}", tool.name);
            assert_eq!(annotations.open_world_hint, Some(false), "{}", tool.name);
            // Each output schema is exactly the one the repository publishes.
            let published: Value = serde_json::from_str(
                &std::fs::read_to_string(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../schema")
                        .join(schema),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                Value::Object((*tool.output_schema.clone().unwrap()).clone()),
                published,
                "{}",
                tool.name
            );
        }
        // The snapshot takes nothing; the delta takes one baseline, as data.
        let snapshot = Value::Object((*tools[1].input_schema).clone());
        assert_eq!(snapshot["properties"], json!({}));
        assert_eq!(snapshot["additionalProperties"], false);
        let delta = Value::Object((*tools[2].input_schema).clone());
        assert_eq!(delta["required"], json!(["baseline"]));
        assert_eq!(delta["properties"]["baseline"]["type"], "object");
        assert_eq!(delta["additionalProperties"], false);
        // The patch tool's input is the SDK's derivation of its typed request: an object,
        // tagged by `operation`, with no root among its arguments.
        let patch = Value::Object((*tools[3].input_schema).clone());
        assert_eq!(patch["type"], "object");
        let text = patch.to_string();
        for word in [
            "\"rename\"",
            "\"diagnostic_fix\"",
            "new_name",
            "byte_offset",
            "\"fix\"",
        ] {
            assert!(text.contains(word), "{word}: {text}");
        }
        assert!(!text.contains("\"root\""), "{text}");
        // The index takes nothing; the detail takes an id and a state, and no root or file.
        let index = Value::Object((*tools[4].input_schema).clone());
        assert_eq!(index["properties"], json!({}));
        let detail = Value::Object((*tools[5].input_schema).clone());
        assert_eq!(detail["type"], "object");
        let mut required: Vec<&str> = detail["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        required.sort_unstable();
        assert_eq!(required, ["id", "state"]);
        // The summary takes one operation, and the only one is `check`: no build, no test,
        // no command.
        let summary = Value::Object((*tools[6].input_schema).clone()).to_string();
        assert!(summary.contains("\"check\""), "{summary}");
        for word in ["build", "\"test\"", "shell", "command\"", "\"root\""] {
            assert!(!summary.contains(word), "{word}: {summary}");
        }
        // The documentation tools take an id and a state, or an optional document — never a
        // path, a file or a root.
        let docs_index = Value::Object((*tools[7].input_schema).clone());
        let keys: Vec<&String> = docs_index["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, ["document"]);
        let docs_section = Value::Object((*tools[8].input_schema).clone());
        let mut required: Vec<&str> = docs_section["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        required.sort_unstable();
        assert_eq!(required, ["id", "state"]);
        for schema in [&docs_index, &docs_section] {
            let text = schema.to_string();
            for word in ["\"path\"", "\"file\"", "\"root\"", "\"query\""] {
                assert!(!text.contains(word), "{word}: {text}");
            }
        }
        // The repository map takes nothing; a task takes seeds and a state, and optionally an
        // intent and a budget — never a path, a file or a root.
        let map = Value::Object((*tools[9].input_schema).clone());
        assert!(
            map["properties"]
                .as_object()
                .is_none_or(serde_json::Map::is_empty),
            "{map}"
        );
        assert_eq!(map["additionalProperties"], false);
        let task = Value::Object((*tools[10].input_schema).clone());
        let mut required: Vec<&str> = task["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        required.sort_unstable();
        assert_eq!(required, ["seeds", "state"]);
        let mut keys: Vec<&String> = task["properties"].as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "intent",
                "max_doc_bytes",
                "max_entities",
                "max_source_bytes",
                "seeds",
                "state"
            ]
        );
        for word in ["\"path\"", "\"file\"", "\"root\"", "\"query\""] {
            assert!(!task.to_string().contains(word), "{word}");
        }
        let tool = &tools[0];
        let input = Value::Object((*tool.input_schema).clone());
        assert_eq!(input["required"], json!(["file", "byte_offset"]));
        assert_eq!(input["additionalProperties"], false);
        let mut props: Vec<&String> = input["properties"].as_object().unwrap().keys().collect();
        props.sort();
        assert_eq!(props, ["byte_offset", "file"]);
        // The output schema is `nazm.context/3`, exactly as the repository publishes it.
        let published: Value = serde_json::from_str(
            &std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema/nazm.context-3.json"),
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
                json!({"schema": "nazm.context/3", "status": "unavailable", "reason": "not_in_compilation"}),
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

/// The process's peak resident set (`VmHWM`), where the platform reports one: Linux, which is
/// where the release benchmark runs. `None` elsewhere.
fn peak_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// Warm-up for a memory measurement: requests between RSS samples.
const SAMPLE_EVERY: usize = 250;
/// Consecutive samples that must agree before a baseline is taken: five samples span 1,000
/// requests, as long as the measured window after them — a process is settled at the scale
/// it is then measured at, not merely between two neighbouring samples.
const STABLE_SAMPLES: usize = 5;
/// At most this many warm-up requests; a process that has not settled by then fails.
/// Three measured windows: the one-time allocator step N38 met came about 800 requests in.
const MAX_WARMUP: usize = 3000;
/// What the settled samples may differ by, and what the measured window may then grow by.
const TOLERANCE_KB: u64 = 4096;

/// Whether the last [`STABLE_SAMPLES`] RSS samples lie within [`TOLERANCE_KB`] of each other.
///
/// Separates one-time settling — glibc giving another worker thread its own arena is a single
/// step of tens of megabytes, then nothing — from growth correlated with requests, which no
/// stretch of samples would stay inside a fixed tolerance through. The measured window's own
/// limit is not changed by it.
fn settled(samples: &[u64]) -> bool {
    let Some(tail) = samples
        .len()
        .checked_sub(STABLE_SAMPLES)
        .map(|at| &samples[at..])
    else {
        return false;
    };
    let (lo, hi) = tail
        .iter()
        .fold((u64::MAX, 0), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    hi - lo <= TOLERANCE_KB
}

#[test]
fn a_baseline_waits_for_a_flat_stretch_as_long_as_the_window_it_precedes() {
    // Flat from the start: settled once there are enough samples, and not before.
    assert!(!settled(&[50_000; 4]));
    assert!(settled(&[50_000; 5]));
    // N38's allocator step: flat, one step of 40 MB, then flat. Not settled while the step is
    // inside the last five samples; settled once five samples after it agree.
    let step = [
        53_252, 53_252, 53_252, 53_252, 93_416, 93_416, 93_416, 93_416,
    ];
    assert!(!settled(&step[..5]));
    assert!(!settled(&step));
    assert!(settled(&[&step[..], &[93_416]].concat()));
    // Growth with every batch never settles, however long it is watched.
    let growing: Vec<u64> = (0..13).map(|i| 50_000 + i * 2_000).collect();
    assert!(!settled(&growing));
    // Noise within the tolerance is settled.
    assert!(settled(&[60_000, 61_000, 59_500, 62_000, 60_500]));
}

/// What a measured window's RSS samples say about memory growth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Growth {
    /// Every sample within [`TOLERANCE_KB`] of the baseline.
    Flat,
    /// One increase larger than [`TOLERANCE_KB`] between adjacent samples — glibc giving a
    /// worker thread its own arena — at this sample, with every sample before it within the
    /// tolerance of the baseline and every sample after it within the tolerance of the first
    /// post-step sample: a plateau.
    OneStep { at: usize, plateau_delta: u64 },
    /// Anything else: two or more large steps, growth beyond the tolerance before or after a
    /// step, or a step at the last sample, after which no plateau could be observed.
    Grows,
}

/// Classify RSS samples taken at even intervals across a measured window, the first being the
/// baseline. At most one allocator discontinuity is allowed, and only when a plateau after it
/// is observed; every other change stays inside the +4 MB bound the benchmark has always used.
fn growth(samples: &[u64]) -> Growth {
    let large: Vec<usize> = (1..samples.len())
        .filter(|&i| samples[i].saturating_sub(samples[i - 1]) > TOLERANCE_KB)
        .collect();
    let within = |from: usize, to: usize, base: u64| {
        samples[from..to].iter().all(|&x| x <= base + TOLERANCE_KB)
    };
    match large.as_slice() {
        [] if within(0, samples.len(), samples[0]) => Growth::Flat,
        // A plateau needs a sample after the step to be seen at all.
        [at] if *at + 1 < samples.len()
            && within(0, *at, samples[0])
            && within(*at, samples.len(), samples[*at]) =>
        {
            let plateau = samples[*at];
            let top = samples[*at..].iter().copied().max().unwrap_or(plateau);
            Growth::OneStep {
                at: *at,
                plateau_delta: top - plateau,
            }
        }
        _ => Growth::Grows,
    }
}

#[test]
fn a_measured_window_allows_one_allocator_step_and_only_with_a_plateau_after_it() {
    use Growth::{Flat, Grows, OneStep};
    // Flat, and drift within the tolerance: the original law.
    assert_eq!(growth(&[53_000, 53_000, 53_000, 53_000, 53_000]), Flat);
    assert_eq!(growth(&[53_000, 54_000, 55_500, 56_000, 57_000]), Flat);
    assert_eq!(
        growth(&[53_000, 54_000, 55_500, 56_000, 57_200]),
        Grows,
        "past +4 MB"
    );
    // One step, then flat or within +4 MB of the first sample after it.
    assert_eq!(
        growth(&[53_000, 53_000, 94_000, 94_000, 94_000]),
        OneStep {
            at: 2,
            plateau_delta: 0
        }
    );
    assert_eq!(
        growth(&[53_000, 94_000, 95_000, 96_500, 97_000]),
        OneStep {
            at: 1,
            plateau_delta: 3_000
        }
    );
    // Two large steps.
    assert_eq!(growth(&[53_000, 94_000, 94_000, 135_000, 135_000]), Grows);
    // A steady leak: every step is large.
    assert_eq!(growth(&[53_000, 65_000, 78_000, 91_000, 104_000]), Grows);
    // A step, then growth continuing past +4 MB of the plateau.
    assert_eq!(growth(&[53_000, 94_000, 97_000, 100_000, 103_000]), Grows);
    // Growth past +4 MB before the step.
    assert_eq!(growth(&[53_000, 56_000, 58_000, 99_000, 99_000]), Grows);
    // A large step at the last sample: no plateau was observed, so not a pass.
    assert_eq!(growth(&[53_000, 54_000, 55_000, 56_000, 94_000]), Grows);
    // A fall is not growth.
    assert_eq!(growth(&[94_000, 53_000, 53_000, 53_000, 53_000]), Flat);
}

/// The working set on Windows, from `tasklist`'s memory column (`"12,345 K"`).
#[cfg(windows)]
fn rss_kb(pid: u32) -> u64 {
    let out = Command::new("tasklist")
        .args(["/NH", "/FO", "CSV", "/FI", &format!("PID eq {pid}")])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .rsplit("\",\"")
        .next()
        .map(|m| m.chars().filter(char::is_ascii_digit).collect::<String>())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0)
}

#[cfg(not(windows))]
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
    assert_eq!(wire.matches("nazm.context/3").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"availability\"").count(), 1, "{wire}");
    assert!(by(4).get("error").is_some(), "unknown tool: {}", by(4));
    assert!(by(5).get("error").is_some(), "no resources: {}", by(5));
    assert!(by(6).get("error").is_some(), "no prompts: {}", by(6));
}

#[test]
fn a_snapshot_crosses_the_protocol_unchanged_and_a_delta_needs_only_it() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let (snapshot, error) = call_with(&client, SNAPSHOT, json!({})).await;
        assert!(!error, "{snapshot}");
        assert_eq!(snapshot["schema"], "nazm.snapshot/3");
        assert_eq!(snapshot, direct_snapshot(&root));
        // The client keeps the snapshot; the server keeps nothing. Handed back unchanged
        // as the baseline, nothing has changed.
        let (delta, error) = call_with(&client, DELTA, json!({"baseline": snapshot})).await;
        assert!(!error, "{delta}");
        assert_eq!(delta["schema"], "nazm.delta/3");
        assert_eq!(delta["changed"], json!([]));
        assert_eq!(
            delta["unchanged"],
            snapshot["definitions"].as_array().unwrap().len()
        );
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_delta_is_of_the_disk_at_that_call_and_its_baseline_is_only_data() {
    const ONE: &str = "fn h() -> Int { 1 }\nfn f() -> Int { h() }\n";
    const TWO: &str = "fn h() -> Int { 1 }\nfn k() -> Int { 2 }\nfn f() -> Int { h() + k() }\n";
    let dir = project(&[("main.nz", ONE)]);
    std::fs::write(dir.path().join("notes.txt"), "secret\n").unwrap();
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let (a, _) = call_with(&client, SNAPSHOT, json!({})).await;
        // An agent edits the file: the delta against A sees it, with no restart.
        std::fs::write(&root, TWO).unwrap();
        let (d, error) = call_with(&client, DELTA, json!({"baseline": a})).await;
        assert!(!error, "{d}");
        assert_eq!(d["added"][0]["identity"], "main.nz::fn k");
        assert_eq!(d["changed"][0]["identity"], "main.nz::fn f");
        assert_eq!(d["changed"][0]["sections"]["callees"], true);
        // A breaking edit: no delta — not one from before.
        std::fs::write(
            &root,
            format!("{TWO}fn broken() -> Int {{ let x = ; 1 }}\n"),
        )
        .unwrap();
        let (broken, error) = call_with(&client, DELTA, json!({"baseline": a})).await;
        assert!(error);
        assert_eq!(broken["reason"], "incomplete_compilation");
        let (no_snapshot, error) = call_with(&client, SNAPSHOT, json!({})).await;
        assert!(error);
        assert_eq!(no_snapshot["reason"], "incomplete_compilation");
        // Repaired: the same delta as before the break.
        std::fs::write(&root, TWO).unwrap();
        let (again, _) = call_with(&client, DELTA, json!({"baseline": a})).await;
        assert_eq!(again, d);

        // A baseline is data: a path is not opened, and is not a snapshot.
        for baseline in [
            json!("/etc/passwd"),
            json!("notes.txt"),
            json!({"path": "/etc/passwd"}),
        ] {
            let (v, error) = call_with(&client, DELTA, json!({"baseline": baseline})).await;
            assert!(error);
            assert_eq!(v["reason"], "baseline_wrong_schema", "{baseline}");
            assert!(!v.to_string().contains("secret"));
        }
        // Protocol errors: no baseline, an argument the schema does not have, an argument
        // to the snapshot, which takes none.
        for (tool, arguments) in [
            (DELTA, json!({})),
            (
                DELTA,
                json!({"baseline": a, "baseline_file": "/etc/passwd"}),
            ),
            (SNAPSHOT, json!({"root": "/"})),
        ] {
            let request = CallToolRequestParams::new(tool)
                .with_arguments(serde_json::from_value(arguments).unwrap());
            assert!(client.call_tool(request).await.is_err(), "{tool}");
        }
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_snapshot_and_a_delta_travel_once() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let baseline = direct_snapshot(&root);
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
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": SNAPSHOT, "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": DELTA, "arguments": {"baseline": baseline}}}),
    ];
    for m in &messages {
        writeln!(stdin, "{m}").unwrap();
    }
    stdin.flush().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut seen = Vec::new();
    while seen.len() < 3 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        seen.push((serde_json::from_str::<Value>(&line).unwrap(), line));
    }
    drop(stdin);
    let _ = child.wait();
    let by = |id: i64| seen.iter().find(|(v, _)| v["id"] == id).unwrap();
    let (snapshot, wire) = by(2);
    assert_eq!(snapshot["result"]["content"], json!([]));
    assert_eq!(snapshot["result"]["structuredContent"], baseline);
    assert_eq!(wire.matches("nazm.snapshot/3").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"unsupported\"").count(), 1, "{wire}");
    let (delta, wire) = by(3);
    assert_eq!(delta["result"]["content"], json!([]));
    assert_eq!(delta["result"]["structuredContent"]["status"], "available");
    assert_eq!(wire.matches("nazm.delta/3").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"unsupported\"").count(), 1, "{wire}");
    eprintln!(
        "nazm-mcp frames: snapshot {} bytes (structured {}), delta {} bytes (structured {})",
        by(2).1.trim_end().len(),
        snapshot["result"]["structuredContent"].to_string().len(),
        by(3).1.trim_end().len(),
        delta["result"]["structuredContent"].to_string().len(),
    );
}

#[test]
fn a_thousand_snapshots_and_deltas_keep_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let (baseline, _) = call_with(&client, SNAPSHOT, json!({})).await;
        let (first_delta, _) = call_with(&client, DELTA, json!({"baseline": baseline})).await;
        for _ in 0..25 {
            call_with(&client, SNAPSHOT, json!({})).await;
            call_with(&client, DELTA, json!({"baseline": baseline})).await;
        }
        let settled = rss_kb(pid);
        for i in 0..500 {
            let (s, _) = call_with(&client, SNAPSHOT, json!({})).await;
            assert_eq!(s, baseline, "snapshot {i}");
            let (d, _) = call_with(&client, DELTA, json!({"baseline": baseline})).await;
            assert_eq!(d, first_delta, "delta {i}");
        }
        let after = rss_kb(pid);
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!(
            "nazm-mcp RSS: settled {settled} kB, after 1,000 snapshot and delta calls {after} kB"
        );
        client.cancel().await.unwrap();
    });
}

/// The patch the language service plans for the same request, directly.
fn direct_patch(root: &Path, file: &Path, offset: u32, rename: Option<&str>) -> Value {
    let (root, file, rename) = (
        root.to_path_buf(),
        file.to_path_buf(),
        rename.map(str::to_owned),
    );
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut s = nazm_service::LanguageService::new();
                s.open_from_disk(&root).unwrap();
                let r = match rename {
                    Some(name) => s.patch_rename(&root, 0, &file, offset, &name),
                    None => s.patch_fix(&root, 0, &file, offset, 0),
                };
                serde_json::to_value(r).unwrap()
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

const BROKEN: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n";

#[test]
fn a_patch_crosses_the_protocol_unchanged_and_is_only_a_plan() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let fixes = project(&[("main.nz", BROKEN)]);
    let root = dir.path().join("main.nz");
    let broken = fixes.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let offset = at(MAIN, "f(p");
        let (patch, error) = call_with(
            &client,
            PATCH,
            json!({"operation": "rename", "file": "main.nz", "byte_offset": offset, "new_name": "go"}),
        )
        .await;
        assert!(!error, "{patch}");
        assert_eq!(patch["schema"], "nazm.patch/1");
        assert_eq!(patch["operation"]["kind"], "rename");
        assert_eq!(patch, direct_patch(&root, &root, offset, Some("go")));
        // Planned, not applied: the file is exactly as it was.
        assert_eq!(std::fs::read_to_string(&root).unwrap(), MAIN);
        // Exported: refused, as a tool result that says why.
        let (far, error) = call_with(
            &client,
            PATCH,
            json!({"operation": "rename", "file": "lib.nz", "byte_offset": 7, "new_name": "near"}),
        )
        .await;
        assert!(error);
        assert_eq!(far["reason"], "exported");
        client.cancel().await.unwrap();

        let (client, _) = connect(&broken).await;
        let offset = at(BROKEN, "x = 2");
        let (fixed, error) = call_with(
            &client,
            PATCH,
            json!({"operation": "diagnostic_fix", "file": "main.nz", "byte_offset": offset, "fix": 0}),
        )
        .await;
        assert!(!error, "{fixed}");
        assert_eq!(fixed["operation"]["applicability"], "automatic");
        assert!(fixed["operation"]["precondition"].as_str().is_some());
        assert_eq!(fixed, direct_patch(&broken, &broken, offset, None));
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), BROKEN);
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_patch_request_names_only_files_of_the_root_and_never_another_root() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    std::fs::write(dir.path().join("loose.nz"), "fn loose() -> Int { 0 }\n").unwrap();
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        for file in ["/etc/passwd", "../../etc/passwd", "loose.nz", "notes.txt"] {
            let (v, error) = call_with(
                &client,
                PATCH,
                json!({"operation": "rename", "file": file, "byte_offset": 0, "new_name": "x"}),
            )
            .await;
            assert!(error, "{file}");
            assert_eq!(v["reason"], "not_in_compilation", "{file}");
        }
        // Protocol errors: no operation, an unknown one, a root, a missing or extra
        // argument, a text replacement a client wrote itself.
        for arguments in [
            json!({"file": "main.nz", "byte_offset": 0, "new_name": "x"}),
            json!({"operation": "apply", "file": "main.nz"}),
            json!({"operation": "rename", "file": "main.nz", "byte_offset": 0, "new_name": "x", "root": "/"}),
            json!({"operation": "rename", "file": "main.nz", "byte_offset": 0}),
            json!({"operation": "diagnostic_fix", "file": "main.nz", "byte_offset": 0, "fix": 0, "replacement": "x"}),
            json!({"operation": "rename", "file": "main.nz", "byte_offset": -1, "new_name": "x"}),
        ] {
            let request = CallToolRequestParams::new(PATCH)
                .with_arguments(serde_json::from_value(arguments.clone()).unwrap());
            assert!(client.call_tool(request).await.is_err(), "{arguments}");
        }
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_patch_is_of_the_disk_at_that_call() {
    const ONE: &str = "fn h() -> Int { 1 }\nfn f() -> Int { h() }\n";
    const TWO: &str = "fn h() -> Int { 1 }\nfn f() -> Int { h() + h() }\n";
    let dir = project(&[("main.nz", ONE)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let ask =
            json!({"operation": "rename", "file": "main.nz", "byte_offset": 3, "new_name": "g"});
        let (p1, _) = call_with(&client, PATCH, ask.clone()).await;
        assert_eq!(p1["files"][0]["edits"].as_array().unwrap().len(), 2);
        std::fs::write(&root, TWO).unwrap();
        let (p2, _) = call_with(&client, PATCH, ask.clone()).await;
        assert_eq!(p2["files"][0]["edits"].as_array().unwrap().len(), 3);
        assert_ne!(p1["files"][0]["digest"], p2["files"][0]["digest"]);
        assert_ne!(p1["baseline"], p2["baseline"]);
        // A broken edit: no plan — not the one from before.
        std::fs::write(&root, "fn h() -> Int { 1 }\nfn f() -> Int { h( }\n").unwrap();
        let (p3, error) = call_with(&client, PATCH, ask.clone()).await;
        assert!(error);
        assert_eq!(p3["reason"], "incomplete_compilation");
        // The target gone: refused, never the old plan.
        std::fs::write(&root, "\n\n\n\nfn f() -> Int { 1 }\n").unwrap();
        let (p4, error) = call_with(&client, PATCH, ask).await;
        assert!(error);
        assert_eq!(p4["reason"], "no_entity");
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_patch_travels_once() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let expected = direct_patch(&root, &root, at(MAIN, "f(p"), Some("go"));
    let mut child = Command::new(SERVER)
        .arg("--root")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "raw", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": PATCH, "arguments": {"operation": "rename", "file": "main.nz", "byte_offset": at(MAIN, "f(p"), "new_name": "go"}}}),
    ] {
        writeln!(stdin, "{m}").unwrap();
    }
    stdin.flush().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut wire = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == 2 {
            wire = line;
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let v: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(v["result"]["content"], json!([]));
    assert_eq!(v["result"]["structuredContent"], expected);
    assert_eq!(wire.matches("nazm.patch/1").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"baseline\"").count(), 1, "{wire}");
    eprintln!(
        "nazm-mcp patch frame: {} bytes (structured {})",
        wire.trim_end().len(),
        expected.to_string().len()
    );
}

#[test]
fn a_thousand_plans_keep_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let asks = [
            json!({"operation": "rename", "file": "main.nz", "byte_offset": at(MAIN, "f(p"), "new_name": "go"}),
            json!({"operation": "rename", "file": "main.nz", "byte_offset": at(MAIN, "P {"), "new_name": "Q"}),
            json!({"operation": "diagnostic_fix", "file": "main.nz", "byte_offset": 0, "fix": 0}),
        ];
        let mut first = Vec::new();
        for a in &asks {
            first.push(call_with(&client, PATCH, a.clone()).await.0);
        }
        assert_eq!(first[2]["reason"], "no_such_fix");
        for _ in 0..25 {
            for a in &asks {
                call_with(&client, PATCH, a.clone()).await;
            }
        }
        let settled = rss_kb(pid);
        for i in 0..1000 {
            let k = i % asks.len();
            let (v, _) = call_with(&client, PATCH, asks[k].clone()).await;
            assert_eq!(v, first[k], "call {i}");
        }
        let after = rss_kb(pid);
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!("nazm-mcp RSS: settled {settled} kB, after 1,000 patch calls {after} kB");
        client.cancel().await.unwrap();
    });
}

/// The index and a detail the language service gives for the same root, directly.
fn direct_diagnostics(root: &Path, id: Option<(&str, &str)>) -> Value {
    let (root, id) = (
        root.to_path_buf(),
        id.map(|(a, b)| (a.to_owned(), b.to_owned())),
    );
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let mut s = nazm_service::LanguageService::new();
                s.open_from_disk(&root).unwrap();
                match id {
                    None => serde_json::to_value(s.diagnostic_index(&root, 0)).unwrap(),
                    Some((id, state)) => {
                        serde_json::to_value(s.diagnostic_detail(&root, 0, &id, &state)).unwrap()
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

const WRONG: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x + true\n}\n";

#[test]
fn diagnostics_cross_the_protocol_compactly_and_a_detail_on_request() {
    let dir = project(&[("main.nz", WRONG)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let (index, error) = call_with(&client, DIAGNOSTICS, json!({})).await;
        assert!(!error, "{index}");
        assert_eq!(index, direct_diagnostics(&root, None));
        assert_eq!(index["counts"]["total"], 2);
        assert!(!index.to_string().contains("message"));
        let state = index["state"]["source"].as_str().unwrap().to_owned();
        for d in index["diagnostics"].as_array().unwrap() {
            let id = d["id"].as_str().unwrap();
            let (detail, error) =
                call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
            assert!(!error, "{detail}");
            assert_eq!(detail, direct_diagnostics(&root, Some((id, &state))));
            assert!(detail["diagnostic"]["message"].as_str().is_some());
        }
        // A detail names only an id and a state: no root, no file, nothing else.
        for arguments in [
            json!({"id": "x"}),
            json!({"id": "x", "state": state, "root": "/"}),
            json!({"id": "x", "state": state, "file": "/etc/passwd"}),
        ] {
            let request = CallToolRequestParams::new(DETAIL)
                .with_arguments(serde_json::from_value(arguments.clone()).unwrap());
            assert!(client.call_tool(request).await.is_err(), "{arguments}");
        }
        let request = CallToolRequestParams::new(DIAGNOSTICS)
            .with_arguments(serde_json::from_value(json!({"root": "/"})).unwrap());
        assert!(client.call_tool(request).await.is_err());
        // The files are exactly as they were: nothing was fixed.
        assert_eq!(std::fs::read_to_string(&root).unwrap(), WRONG);
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_diagnostic_id_is_of_its_state_and_a_later_edit_makes_it_stale() {
    let dir = project(&[("main.nz", WRONG)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let (a, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
        let id = a["diagnostics"][0]["id"].as_str().unwrap().to_owned();
        let state = a["state"]["source"].as_str().unwrap().to_owned();
        std::fs::write(&root, format!("// moved\n{WRONG}")).unwrap();
        let (stale, error) = call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
        assert!(error);
        assert_eq!(stale["reason"], "stale_state");
        let (b, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
        assert_ne!(b["state"], a["state"]);
        // Broken: syntax diagnostics are indexed too, with no snapshot.
        std::fs::write(&root, "fn main() -> Int { 1 +  }\n").unwrap();
        let (c, error) = call_with(&client, DIAGNOSTICS, json!({})).await;
        assert!(!error);
        assert_eq!(c["state"]["snapshot"], Value::Null);
        assert!(c["counts"]["total"].as_u64().unwrap() >= 1);
        // Repaired: nothing to report, and the old id stays stale.
        std::fs::write(&root, "fn main() -> Int { 1 }\n").unwrap();
        let (d, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
        assert_eq!(d["counts"]["total"], 0);
        let (gone, _) = call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
        assert_eq!(gone["reason"], "stale_state");
        client.cancel().await.unwrap();
    });
}

#[test]
fn an_index_travels_once() {
    let dir = project(&[("main.nz", WRONG)]);
    let root = dir.path().join("main.nz");
    let expected = direct_diagnostics(&root, None);
    let mut child = Command::new(SERVER)
        .arg("--root")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "raw", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": DIAGNOSTICS, "arguments": {}}}),
    ] {
        writeln!(stdin, "{m}").unwrap();
    }
    stdin.flush().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut wire = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == 2 {
            wire = line;
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let v: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(v["result"]["content"], json!([]));
    assert_eq!(v["result"]["structuredContent"], expected);
    assert_eq!(wire.matches("nazm.diagnostic-index/1").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"counts\"").count(), 1, "{wire}");
}

#[test]
fn a_thousand_diagnostic_calls_are_each_of_the_disk_and_keep_nothing() {
    let dir = project(&[("main.nz", WRONG)]);
    let root = dir.path().join("main.nz");
    let fixed = "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    x\n}\n";
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let (wrong, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
        let id = wrong["diagnostics"][0]["id"].as_str().unwrap().to_owned();
        let state = wrong["state"]["source"].as_str().unwrap().to_owned();
        let (first_detail, _) = call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
        for _ in 0..10 {
            call_with(&client, DIAGNOSTICS, json!({})).await;
        }
        let settled = rss_kb(pid);
        for i in 0..250 {
            // index, detail, an edit that repairs it, a fresh index, the old detail refused,
            // and back.
            let (a, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
            assert_eq!(a, wrong, "{i}");
            let (d, _) = call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
            assert_eq!(d, first_detail, "{i}");
            std::fs::write(&root, fixed).unwrap();
            let (b, _) = call_with(&client, DIAGNOSTICS, json!({})).await;
            assert_eq!(b["counts"]["total"], 0, "{i}");
            let (stale, _) = call_with(&client, DETAIL, json!({"id": id, "state": state})).await;
            assert_eq!(stale["reason"], "stale_state", "{i}");
            std::fs::write(&root, WRONG).unwrap();
        }
        let after = rss_kb(pid);
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!("nazm-mcp RSS: settled {settled} kB, after 1,000 diagnostic calls {after} kB");
        client.cancel().await.unwrap();
    });
}

/// The service's summary for the same root, directly.
fn direct_summary(root: &Path) -> Value {
    let root = root.to_path_buf();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || {
                serde_json::to_value(nazm_service::summary::check(&root, None)).unwrap()
            })
            .unwrap()
            .join()
            .unwrap()
    })
}

const NO_MAIN: &str = "fn helper() -> Int { 1 + true }\n";

#[test]
fn a_check_summary_crosses_the_protocol_and_is_nazm_check_s() {
    let dir = project(&[("main.nz", NO_MAIN)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, _) = connect(&root).await;
        let (s, error) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
        assert!(!error, "{s}");
        assert_eq!(s["schema"], "nazm.command-summary/1");
        assert_eq!(s["status"], "rejected");
        // The service's summary, which `nazm check --summary-json` equals
        // (`crates/nazm-cli/tests/summary.rs`).
        assert_eq!(s, direct_summary(&root));
        // The program's missing `main` is the check's own, and kept.
        assert!(s["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "N0204" && d["id"].is_null()));
        // Only `check`, and only it, with nothing else.
        for arguments in [
            json!({}),
            json!({"operation": "build"}),
            json!({"operation": "test"}),
            json!({"operation": "check", "root": "/"}),
            json!({"operation": "check", "args": ["--cache-report"]}),
            json!({"command": "sh -c true"}),
        ] {
            let request = CallToolRequestParams::new(SUMMARY)
                .with_arguments(serde_json::from_value(arguments.clone()).unwrap());
            assert!(client.call_tool(request).await.is_err(), "{arguments}");
        }
        // Nothing was written: the source is as it was, and no cache or executable appeared.
        assert_eq!(std::fs::read_to_string(&root).unwrap(), NO_MAIN);
        let entries: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, ["main.nz"], "{entries:?}");
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_thousand_check_summaries_are_each_of_the_disk_and_keep_nothing() {
    let dir = project(&[("main.nz", NO_MAIN)]);
    let root = dir.path().join("main.nz");
    let fixed = "fn main() -> Int { 1 }\n";
    let broken = "fn main() -> Int { 1 +  }\n";
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let (first, _) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
        for _ in 0..10 {
            call_with(&client, SUMMARY, json!({"operation": "check"})).await;
        }
        let settled = rss_kb(pid);
        for i in 0..250 {
            let (a, _) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
            assert_eq!(a, first, "{i}");
            std::fs::write(&root, broken).unwrap();
            let (b, _) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
            assert_eq!(b["status"], "rejected", "{i}");
            assert_eq!(b["diagnostics"][0]["code"], "N0100", "{i}");
            std::fs::write(&root, fixed).unwrap();
            let (c, _) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
            assert_eq!(c["status"], "success", "{i}");
            assert_eq!(c["diagnostics"], json!([]), "{i}");
            std::fs::write(&root, NO_MAIN).unwrap();
            let (d, _) = call_with(&client, SUMMARY, json!({"operation": "check"})).await;
            assert_eq!(d, first, "{i}");
        }
        let after = rss_kb(pid);
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!("nazm-mcp RSS: settled {settled} kB, after 1,000 summary calls {after} kB");
        client.cancel().await.unwrap();
    });
}

// --- Nazm's own documentation (N32). ---

/// A server whose documentation tools read `docs`.
async fn connect_docs(root: &Path, docs: &Path) -> (RunningService<RoleClient, ()>, u32) {
    let transport = TokioChildProcess::new(tokio::process::Command::new(SERVER).configure(|c| {
        c.arg("--root").arg(root).arg("--docs").arg(docs);
    }))
    .unwrap();
    let pid = transport.id().unwrap();
    (().serve(transport).await.unwrap(), pid)
}

/// The smallest corpus the manifest accepts: every document, a title and one anchored
/// section each; the guide's generated region naming the one production.
fn tiny_corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for d in nazm_docs::CORPUS {
        let text = match (d.format, d.authority) {
            (nazm_docs::Format::Ebnf, _) => "(* header *)\n\na = \"x\" ;\n".to_owned(),
            (_, nazm_docs::Authority::Guide) => format!(
                "# guide\n\n{}\n### `a`\n\n{}\n",
                nazm_docs::GUIDE_BEGIN,
                nazm_docs::GUIDE_END
            ),
            _ => format!(
                "# {id}\n\n## One\n<!-- id: one -->\ntext of {id}\n",
                id = d.id
            ),
        };
        let p = dir.path().join(d.path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

#[test]
fn documentation_crosses_the_protocol_unchanged_by_id_and_never_by_path() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let docs = nazm_docs::default_root();
    runtime().block_on(async {
        // No `--docs`: the repository this server was built from.
        let (client, _) = connect(&root).await;
        let (index, error) = call_with(&client, DOCS_INDEX, json!({})).await;
        assert!(!error);
        assert_eq!(
            index,
            serde_json::to_value(nazm_docs::index(&docs, None)).unwrap()
        );
        let state = index["state"].as_str().unwrap().to_owned();
        let (one, _) = call_with(&client, DOCS_INDEX, json!({"document": "goals"})).await;
        assert_eq!(one["documents"].as_array().unwrap().len(), 1);
        let (g78, error) = call_with(
            &client,
            DOCS_SECTION,
            json!({"id": "goals:G78", "state": state}),
        )
        .await;
        assert!(!error);
        assert_eq!(
            g78,
            serde_json::to_value(nazm_docs::section(&docs, "goals:G78", Some(&state))).unwrap()
        );
        assert_eq!(g78["authority"], "goal");
        // An id that names no section is not found — a path is only an id no section has,
        // and it is never opened.
        for id in [
            "../README.md",
            "/etc/passwd",
            "docs/spec.md",
            "target/debug/nazm",
            "spec:gener",
        ] {
            let (v, error) =
                call_with(&client, DOCS_SECTION, json!({"id": id, "state": state})).await;
            assert_eq!(v["status"], "not_found", "{id}");
            assert!(error, "{id}");
        }
        let (v, _) = call_with(&client, DOCS_INDEX, json!({"document": "../README.md"})).await;
        assert_eq!(v["status"], "not_found");
        // A path, a root or a missing state is not an argument these tools have.
        for (tool, arguments) in [
            (
                DOCS_SECTION,
                json!({"id": "spec", "state": state, "path": "/etc/passwd"}),
            ),
            (DOCS_SECTION, json!({"path": "docs/spec.md"})),
            (DOCS_SECTION, json!({"id": "spec"})),
            (DOCS_INDEX, json!({"root": "/"})),
            (DOCS_INDEX, json!({"query": "generics"})),
        ] {
            let r = client
                .call_tool(
                    CallToolRequestParams::new(tool)
                        .with_arguments(serde_json::from_value(arguments.clone()).unwrap()),
                )
                .await;
            assert!(r.is_err(), "{tool} {arguments}");
        }
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_documentation_section_is_of_the_disk_at_that_call_and_an_old_state_is_stale() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let docs = tiny_corpus();
    let spec = docs.path().join("docs/spec.md");
    runtime().block_on(async {
        let (client, _) = connect_docs(&root, docs.path()).await;
        let (index, _) = call_with(&client, DOCS_INDEX, json!({})).await;
        let old = index["state"].as_str().unwrap().to_owned();
        let (s, _) = call_with(
            &client,
            DOCS_SECTION,
            json!({"id": "spec:one", "state": old}),
        )
        .await;
        assert_eq!(s["body"], "## One\n<!-- id: one -->\ntext of spec\n");
        std::fs::write(
            &spec,
            "# spec\n\n## One, reworded\n<!-- id: one -->\nedited\n",
        )
        .unwrap();
        let (s, error) = call_with(
            &client,
            DOCS_SECTION,
            json!({"id": "spec:one", "state": old}),
        )
        .await;
        assert_eq!(s["status"], "stale_state");
        assert!(error);
        let (index, _) = call_with(&client, DOCS_INDEX, json!({})).await;
        let new = index["state"].as_str().unwrap().to_owned();
        assert_ne!(new, old);
        let (s, _) = call_with(
            &client,
            DOCS_SECTION,
            json!({"id": "spec:one", "state": new}),
        )
        .await;
        assert_eq!(s["body"], "## One, reworded\n<!-- id: one -->\nedited\n");
        // A corpus that stops sectioning is unavailable, never its last good index.
        std::fs::write(&spec, "no title\n").unwrap();
        let (i, error) = call_with(&client, DOCS_INDEX, json!({})).await;
        assert_eq!(i["status"], "unavailable");
        assert!(error);
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_thousand_documentation_calls_are_each_of_the_disk_and_keep_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let docs = tiny_corpus();
    let goals = docs.path().join("docs/NAZM_LANGUAGE_GOALS.md");
    let original = std::fs::read_to_string(&goals).unwrap();
    runtime().block_on(async {
        let (client, pid) = connect_docs(&root, docs.path()).await;
        let (first, _) = call_with(&client, DOCS_INDEX, json!({})).await;
        let state = first["state"].as_str().unwrap().to_owned();
        for _ in 0..10 {
            call_with(&client, DOCS_INDEX, json!({})).await;
        }
        let settled = rss_kb(pid);
        let mut calls = 0;
        for i in 0..143 {
            let (a, _) = call_with(&client, DOCS_INDEX, json!({})).await;
            assert_eq!(a, first, "{i}");
            let (s, _) = call_with(
                &client,
                DOCS_SECTION,
                json!({"id": "goals:one", "state": state}),
            )
            .await;
            assert_eq!(
                s["body"], "## One\n<!-- id: one -->\ntext of goals\n",
                "{i}"
            );
            let (s, _) = call_with(
                &client,
                DOCS_SECTION,
                json!({"id": "grammar:a", "state": state}),
            )
            .await;
            assert_eq!(s["production"]["text"], "a = \"x\" ;", "{i}");
            std::fs::write(&goals, format!("{original}edit {i}\n")).unwrap();
            let (b, _) = call_with(&client, DOCS_INDEX, json!({})).await;
            let edited = b["state"].as_str().unwrap().to_owned();
            assert_ne!(edited, state, "{i}");
            let (s, _) = call_with(
                &client,
                DOCS_SECTION,
                json!({"id": "goals:one", "state": state}),
            )
            .await;
            assert_eq!(s["status"], "stale_state", "{i}");
            let (s, _) = call_with(
                &client,
                DOCS_SECTION,
                json!({"id": "goals:one", "state": edited}),
            )
            .await;
            assert!(
                s["body"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("edit {i}\n")),
                "{i}"
            );
            std::fs::write(&goals, &original).unwrap();
            calls += 7;
        }
        assert_eq!(calls, 1001);
        let after = rss_kb(pid);
        assert!(after <= settled + 1024, "{settled} kB -> {after} kB");
        eprintln!("nazm-mcp RSS: settled {settled} kB, after 1,001 documentation calls {after} kB");
        client.cancel().await.unwrap();
    });
}

/// The smallest repository the manifest accepts: the tiny corpus, one small compilation for
/// every root the manifest names, and a catalogue with one profile.
fn tiny_repository() -> tempfile::TempDir {
    let dir = tiny_corpus();
    // The sections the manifest's source roots name.
    std::fs::write(
        dir.path().join("docs/bootstrap.md"),
        "# bootstrap\n\n## 2. What the chain does\n\nstages\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("docs/guide.md"),
        format!(
            "# guide\n\n## A whole program\n\nexample\n\n{}\n### `a`\n\n{}\n",
            nazm_docs::GUIDE_BEGIN,
            nazm_docs::GUIDE_END
        ),
    )
    .unwrap();
    for s in nazm_repo::SOURCES {
        for r in s.roots {
            let p = dir.path().join(s.dir).join(r);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(
                p,
                "fn helper() -> Int { 1 }\n\nfn main() -> Int { helper() }\n",
            )
            .unwrap();
        }
    }
    let catalogue = dir.path().join("xtask/mutations/mutations.toml");
    std::fs::create_dir_all(catalogue.parent().unwrap()).unwrap();
    std::fs::write(
        &catalogue,
        "[[profile]]\nname = \"selfhost\"\nwhy = \"x\"\nowns = [\"compiler/\"]\ntargets = []\n",
    )
    .unwrap();
    dir
}

#[test]
fn the_repository_crosses_the_protocol_unchanged_by_id_and_never_by_path() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let repo = nazm_docs::default_root();
    runtime().block_on(async {
        // No `--docs`: the repository this server was built from.
        let (client, _) = connect(&root).await;
        let (map, error) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        assert!(!error);
        assert_eq!(
            map,
            serde_json::from_str::<Value>(&nazm_repo::map(&repo).to_json()).unwrap()
        );
        let state = map["state"].as_str().unwrap().to_owned();
        let seed = "def:compiler:module.nz::fn core_module";
        let (task, error) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": state, "intent": "test"}),
        )
        .await;
        assert!(!error, "{task}");
        let direct = nazm_repo::task(
            &repo,
            &nazm_repo::TaskRequest {
                intent: Some(nazm_repo::Intent::Test),
                seeds: vec![seed.to_owned()],
                state: Some(state.clone()),
                budget: nazm_repo::Budget::default(),
            },
        );
        assert_eq!(
            task,
            serde_json::from_str::<Value>(&direct.to_json()).unwrap()
        );
        assert_eq!(task["items"][0]["id"], seed);
        // A budget travels, and what it cuts is named.
        let (partial, error) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": state, "intent": "edit", "max_entities": 1}),
        )
        .await;
        assert!(!error);
        assert_eq!(partial["status"], "partial");
        // A path is not a seed: refused, and never opened.
        for id in [
            "/etc/passwd",
            "../README.md",
            "module:compiler:../README.md",
            "docs/spec.md",
        ] {
            let (v, error) = call_with(
                &client,
                TASK_CONTEXT,
                json!({"seeds": [id], "state": state}),
            )
            .await;
            assert_eq!(v["status"], "invalid_request", "{id}: {v}");
            assert!(error, "{id}");
        }
        let (v, error) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": "0".repeat(64)}),
        )
        .await;
        assert_eq!(v["status"], "stale_state");
        assert!(error);
        // A missing state, a path, a root or an unknown intent is not an argument.
        for (tool, arguments) in [
            (TASK_CONTEXT, json!({"seeds": [seed]})),
            (
                TASK_CONTEXT,
                json!({"seeds": [seed], "state": state, "path": "/etc/passwd"}),
            ),
            (
                TASK_CONTEXT,
                json!({"seeds": [seed], "state": state, "root": "/"}),
            ),
            (
                TASK_CONTEXT,
                json!({"seeds": [seed], "state": state, "intent": "guess"}),
            ),
            (REPOSITORY_MAP, json!({"root": "/"})),
        ] {
            let r = client
                .call_tool(
                    CallToolRequestParams::new(tool)
                        .with_arguments(serde_json::from_value(arguments.clone()).unwrap()),
                )
                .await;
            assert!(r.is_err(), "{tool} {arguments}");
        }
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_repository_answer_is_of_the_disk_at_that_call_and_an_old_state_is_stale() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let repo = tiny_repository();
    let emit = repo.path().join("compiler/emit.nz");
    runtime().block_on(async {
        let (client, _) = connect_docs(&root, repo.path()).await;
        let (map, error) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        assert!(!error, "{map}");
        let old = map["state"].as_str().unwrap().to_owned();
        let seed = "def:compiler:emit.nz::fn helper";
        let (t, _) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": old}),
        )
        .await;
        assert_eq!(
            t["items"][0]["content"]["source"],
            "fn helper() -> Int { 1 }"
        );
        std::fs::write(
            &emit,
            "fn helper() -> Int { 2 }\n\nfn main() -> Int { helper() }\n",
        )
        .unwrap();
        let (t, error) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": old}),
        )
        .await;
        assert_eq!(t["status"], "stale_state");
        assert!(error);
        let (map, _) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        let new = map["state"].as_str().unwrap().to_owned();
        assert_ne!(new, old);
        let (t, _) = call_with(
            &client,
            TASK_CONTEXT,
            json!({"seeds": [seed], "state": new}),
        )
        .await;
        assert_eq!(
            t["items"][0]["content"]["source"],
            "fn helper() -> Int { 2 }"
        );
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_repository_answer_travels_once() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let repo = tiny_repository();
    runtime().block_on(async {
        let (client, _) = connect_docs(&root, repo.path()).await;
        let (map, _) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        let state = map["state"].as_str().unwrap().to_owned();
        for (tool, arguments) in [
            (REPOSITORY_MAP, json!({})),
            (
                TASK_CONTEXT,
                json!({"seeds": ["def:compiler:emit.nz::fn helper"], "state": state}),
            ),
            (
                TASK_CONTEXT,
                json!({"seeds": ["/etc/passwd"], "state": state}),
            ),
        ] {
            let r = client
                .call_tool(
                    CallToolRequestParams::new(tool)
                        .with_arguments(serde_json::from_value(arguments).unwrap()),
                )
                .await
                .unwrap();
            assert!(r.content.is_empty(), "{tool}: {:?}", r.content);
            assert!(r.structured_content.is_some(), "{tool}");
        }
        client.cancel().await.unwrap();
    });
}

#[test]
fn a_thousand_repository_calls_are_each_of_the_disk_and_keep_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    let repo = tiny_repository();
    runtime().block_on(async {
        let (client, pid) = connect_docs(&root, repo.path()).await;
        let (map, _) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        let state = map["state"].as_str().unwrap().to_owned();
        let asks = [
            json!({"seeds": ["def:compiler:emit.nz::fn helper"], "state": state}),
            json!({"seeds": ["doc:spec:one"], "state": state, "intent": "understand"}),
            json!({"seeds": ["root:examples:sum.nz"], "state": state, "intent": "understand"}),
            json!({"seeds": ["def:examples-bad:bare_return.nz::fn main"], "state": state, "intent": "test", "max_entities": 1}),
        ];
        let mut first = Vec::new();
        for a in &asks {
            first.push(call_with(&client, TASK_CONTEXT, a.clone()).await.0);
        }
        for _ in 0..10 {
            for a in &asks {
                call_with(&client, TASK_CONTEXT, a.clone()).await;
            }
        }
        // Not a fixed 1 MB after 44 requests: the allocator (glibc) may still take one step of a
        // megabyte or two after them (CI, Linux: 19,268 -> 20,580 kB, while macOS stays flat for
        // 6,000 calls), when a worker thread meets a request shape for the first time. A leak is
        // growth that goes on with the requests, so: warm up until RSS has agreed with itself
        // over five samples (`settled`), then measure two windows of 1,000 calls. A one-time
        // step is over by the second window; a leak of even 1 kB a call shows between them.
        let mut samples = vec![rss_kb(pid)];
        let mut warmup = 0;
        while !settled(&samples) {
            assert!(
                warmup < MAX_WARMUP,
                "RSS did not settle within {MAX_WARMUP} warm-up requests: {samples:?} kB"
            );
            for i in 0..SAMPLE_EVERY {
                let k = i % asks.len();
                call_with(&client, TASK_CONTEXT, asks[k].clone()).await;
            }
            warmup += SAMPLE_EVERY;
            samples.push(rss_kb(pid));
        }
        let settled_kb = rss_kb(pid);
        let mut windows = Vec::new();
        for _ in 0..2 {
            for i in 0..1000 {
                let k = i % asks.len();
                let (v, _) = call_with(&client, TASK_CONTEXT, asks[k].clone()).await;
                assert_eq!(v, first[k], "call {i}");
            }
            windows.push(rss_kb(pid));
        }
        eprintln!(
            "nazm-mcp RSS: settled {settled_kb} kB after {warmup} warm-up calls, then {windows:?} kB after 2 x 1,000 task contexts"
        );
        assert!(
            windows[1] <= windows[0] + 1024,
            "growth continues with the requests: {settled_kb} -> {windows:?} kB"
        );
        assert!(
            windows[1] <= settled_kb + TOLERANCE_KB,
            "{settled_kb} -> {windows:?} kB"
        );
        client.cancel().await.unwrap();
    });
}

/// The measurement law's own evidence: 3,000 sequential task contexts under the normal
/// allocator, sampled every 250, to show that after at most one step RSS plateaus. Run by hand
/// (`cargo test --release -p nazm-mcp --test protocol -- --ignored`).
#[test]
#[ignore = "a 3,000-request confirmation of the memory law; minutes in a release build"]
fn three_thousand_task_contexts_plateau_after_at_most_one_step() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let (map, _) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        let state = map["state"].as_str().unwrap().to_owned();
        let asks = [
            json!({"seeds": ["def:compiler:module.nz::fn attach_core"], "state": state, "intent": "edit"}),
            json!({"seeds": ["doc:spec:returns"], "state": state, "intent": "understand"}),
            json!({"seeds": ["package:nazm-docs"], "state": state, "intent": "test"}),
            json!({"seeds": ["def:compiler:lex.nz::fn is_alnum"], "state": state, "intent": "understand"}),
        ];
        let mut first = Vec::new();
        for a in &asks {
            first.push(call_with(&client, TASK_CONTEXT, a.clone()).await.0);
        }
        let mut samples = vec![rss_kb(pid)];
        for i in 0..3000 {
            let k = i % asks.len();
            let (v, _) = call_with(&client, TASK_CONTEXT, asks[k].clone()).await;
            assert_eq!(v, first[k], "call {i}");
            if (i + 1) % SAMPLE_EVERY == 0 {
                samples.push(rss_kb(pid));
            }
        }
        let verdict = growth(&samples);
        let peak = peak_kb(pid).map_or_else(|| "unreported".to_owned(), |p| format!("{p} kB"));
        eprintln!("3,000 task contexts: RSS {samples:?} kB, {verdict:?}, peak {peak}");
        assert_ne!(verdict, Growth::Grows, "{samples:?} kB");
        client.cancel().await.unwrap();
    });
}

/// The 1,000-call measurement over this repository itself, for `performance.md`: a release
/// build, run by hand (`cargo test --release -p nazm-mcp --test protocol -- --ignored`).
#[test]
#[ignore = "a measurement over the real repository; minutes in a debug build"]
fn a_thousand_task_contexts_over_this_repository() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let root = dir.path().join("main.nz");
    runtime().block_on(async {
        let (client, pid) = connect(&root).await;
        let start = std::time::Instant::now();
        let (map, _) = call_with(&client, REPOSITORY_MAP, json!({})).await;
        let map_ms = start.elapsed().as_millis();
        let state = map["state"].as_str().unwrap().to_owned();
        let asks = [
            json!({"seeds": ["def:compiler:module.nz::fn attach_core"], "state": state, "intent": "edit"}),
            json!({"seeds": ["doc:spec:returns"], "state": state, "intent": "understand"}),
            json!({"seeds": ["package:nazm-docs"], "state": state, "intent": "test"}),
            json!({"seeds": ["diagnostic:examples-bad:return_type_mismatch.nz:N0300@return_type_mismatch.nz:59-69#0"], "state": state, "intent": "diagnose"}),
            json!({"seeds": ["def:compiler:lex.nz::fn is_alnum"], "state": state, "intent": "understand"}),
        ];
        let mut first = Vec::new();
        for a in &asks {
            first.push(call_with(&client, TASK_CONTEXT, a.clone()).await.0);
        }
        for _ in 0..4 {
            for a in &asks {
                call_with(&client, TASK_CONTEXT, a.clone()).await;
            }
        }
        // Stabilise before the baseline: bounded batches, an RSS sample after each, until a
        // stretch as long as the measured window agrees. A process that never settles fails.
        let mut samples = vec![rss_kb(pid)];
        let mut warmup = 0;
        while !settled(&samples) {
            assert!(
                warmup < MAX_WARMUP,
                "RSS did not settle within {MAX_WARMUP} warm-up requests: {samples:?} kB"
            );
            for i in 0..SAMPLE_EVERY {
                let k = i % asks.len();
                let (v, _) = call_with(&client, TASK_CONTEXT, asks[k].clone()).await;
                assert_eq!(v, first[k], "warm-up call {}", warmup + i);
            }
            warmup += SAMPLE_EVERY;
            samples.push(rss_kb(pid));
        }
        // The measured window, sampled every 250 requests; the first sample is the baseline.
        let mut window = vec![rss_kb(pid)];
        let start = std::time::Instant::now();
        for i in 0..1000 {
            let k = i % asks.len();
            let (v, _) = call_with(&client, TASK_CONTEXT, asks[k].clone()).await;
            assert_eq!(v, first[k], "call {i}");
            if (i + 1) % SAMPLE_EVERY == 0 {
                window.push(rss_kb(pid));
            }
        }
        let per = start.elapsed().as_secs_f64();
        let peak = peak_kb(pid).map_or_else(|| "unreported".to_owned(), |p| format!("{p} kB"));
        let step = samples.windows(2).any(|w| w[1].saturating_sub(w[0]) > TOLERANCE_KB);
        let verdict = growth(&window);
        eprintln!(
            "nazm-mcp over this repository: map {map_ms} ms; 1,000 task contexts, {per:.3} ms each on average; warm-up {warmup} requests, RSS samples {samples:?} kB, a step during warm-up: {step}; measured window {window:?} kB, {verdict:?}, peak {peak}"
        );
        assert_ne!(verdict, Growth::Grows, "{window:?} kB");
        client.cancel().await.unwrap();
    });
}
