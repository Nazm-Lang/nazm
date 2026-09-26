//! `nazm lsp`, as an editor sees it: the real process, real framing, over stdio. N16.
//!
//! Compiler behaviour is tested in `crates/nazm-service/tests/service.rs`, without a
//! protocol. What is tested here is the shell: the handshake, what is advertised, how
//! diagnostics and locations cross the protocol boundary — above all their UTF-16
//! positions after non-ASCII text — and that nothing but framed messages reaches stdout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

const DEADLINE: Duration = Duration::from_secs(30);

/// The server, and every message it has sent that nobody has taken yet.
struct Client {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Result<Value, String>>,
    next_id: i64,
}

impl Client {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nazm"))
            .arg("lsp")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start nazm lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, incoming) = mpsc::channel();
        // Reads stdout **strictly**: a frame is `Content-Length: N`, a blank line and N
        // bytes of JSON, and anything else is an error the test reports. A stray line on
        // stdout would be exactly such an error.
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut header = String::new();
                match r.read_line(&mut header) {
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(Err(format!("read: {e}")));
                        return;
                    }
                }
                let Some(n) = header
                    .strip_prefix("Content-Length: ")
                    .and_then(|v| v.trim_end().parse::<usize>().ok())
                else {
                    let _ = tx.send(Err(format!("not a frame header: {header:?}")));
                    return;
                };
                let mut blank = String::new();
                if r.read_line(&mut blank).is_err() || blank != "\r\n" {
                    let _ = tx.send(Err(format!("expected a blank line, got {blank:?}")));
                    return;
                }
                let mut body = vec![0u8; n];
                if r.read_exact(&mut body).is_err() {
                    let _ = tx.send(Err("truncated frame".to_owned()));
                    return;
                }
                let parsed = serde_json::from_slice(&body).map_err(|e| format!("json: {e}"));
                if tx.send(parsed).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin,
            incoming,
            next_id: 1,
        }
    }

    fn write(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn next(&mut self) -> Value {
        match self.incoming.recv_timeout(DEADLINE) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => panic!("the server's stdout is not the protocol: {e}"),
            Err(e) => panic!("no message from the server: {e}"),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let m = self.next();
            if m.get("id") == Some(&json!(id)) {
                return m;
            }
            assert!(m.get("method").is_some(), "an unexpected response: {m}");
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// The next `publishDiagnostics` for `uri`, skipping any for other documents.
    fn diagnostics_for(&mut self, uri: &str) -> Value {
        loop {
            let m = self.next();
            if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri {
                return m["params"].clone();
            }
        }
    }

    fn initialize(&mut self, root: &Path) -> Value {
        self.initialize_with(
            root,
            json!({"workspace": {"workspaceEdit": {"documentChanges": true}}}),
        )
    }

    fn initialize_with(&mut self, root: &Path, capabilities: Value) -> Value {
        let r = self.request(
            "initialize",
            json!({"processId": null, "rootUri": uri(root), "capabilities": capabilities}),
        );
        self.notify("initialized", json!({}));
        r
    }

    fn open(&mut self, path: &Path, version: i32, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri(path), "languageId": "nazm", "version": version, "text": text}}),
        );
    }

    fn change(&mut self, path: &Path, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri(path), "version": version}, "contentChanges": [{"text": text}]}),
        );
    }

    fn close(&mut self, path: &Path) {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri(path)}}),
        );
    }

    fn definition(&mut self, path: &Path, line: u32, character: u32) -> Value {
        self.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri(path)}, "position": {"line": line, "character": character}}),
        )["result"]
            .clone()
    }

    /// `shutdown`, `exit`, and the exit status and stderr.
    fn finish(mut self) -> (Option<i32>, String) {
        let r = self.request("shutdown", Value::Null);
        assert_eq!(r["result"], Value::Null, "{r}");
        self.notify("exit", Value::Null);
        drop(self.stdin);
        let status = wait(&mut self.child);
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        (status, err)
    }
}

fn wait(child: &mut Child) -> Option<i32> {
    let start = std::time::Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            panic!("the server did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A `file:` URI, percent-encoding what the server would.
fn uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_str().unwrap().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The project directory, by its canonical path, so URIs compare as the server writes them.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    (dir, root)
}

/// The UTF-16 column of byte `at` of `line`: how an editor counts, computed independently
/// of the server.
fn utf16(line: &str, at: usize) -> u32 {
    u32::try_from(line[..at].encode_utf16().count()).unwrap()
}

// Non-ASCII before every position the tests check: a two-byte `é`, a three-byte `→`
// and a four-byte `😀` that is two UTF-16 units. A server that counted bytes as columns
// would be off by 1 + 2 + 2 = 5 here; one that counted characters, by 1.
const LIB: &str = "pub fn tag() -> Str { \"é→😀\" } pub fn helper(n: Int) -> Int { n }\n";
const MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { let s = \"é→😀\"; helper(s) }\n";
const MAIN_OK: &str = "use \"lib.nz\";\nfn main() -> Int { let s = \"é→😀\"; helper(1) }\n";

#[test]
fn the_server_speaks_the_protocol_end_to_end() {
    let (_dir, root) = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (root.join("lib.nz"), root.join("main.nz"));
    let mut c = Client::start(&root);

    // 1. The handshake, and exactly what is implemented.
    let init = c.initialize(&root);
    let caps = init["result"]["capabilities"].as_object().unwrap().clone();
    let mut advertised: Vec<&str> = caps.keys().map(String::as_str).collect();
    advertised.sort_unstable();
    assert_eq!(
        advertised,
        [
            "codeActionProvider",
            "completionProvider",
            "definitionProvider",
            "documentSymbolProvider",
            "hoverProvider",
            "positionEncoding",
            "referencesProvider",
            "renameProvider",
            "semanticTokensProvider",
            "signatureHelpProvider",
            "textDocumentSync",
            "workspaceSymbolProvider"
        ],
        "only what is implemented is advertised: {init}"
    );
    assert_eq!(caps["positionEncoding"], "utf-16");
    assert_eq!(
        caps["textDocumentSync"]["change"], 1,
        "full synchronisation"
    );
    assert_eq!(caps["textDocumentSync"]["openClose"], true);
    assert_eq!(caps["referencesProvider"], true);
    assert_eq!(caps["renameProvider"], json!({"prepareProvider": true}));
    assert_eq!(
        caps["completionProvider"],
        json!({"resolveProvider": false}),
        "invoked only: no trigger characters, nothing to resolve"
    );
    assert_eq!(
        caps["signatureHelpProvider"],
        json!({"triggerCharacters": ["(", ","]}),
        "triggered where the active call or argument changes; no retrigger characters"
    );
    assert_eq!(caps["documentSymbolProvider"], true);
    assert_eq!(
        caps["codeActionProvider"],
        json!({"codeActionKinds": ["quickfix"], "resolveProvider": false}),
        "quick fixes only, each complete when sent"
    );
    assert_eq!(
        caps["semanticTokensProvider"]["full"], true,
        "the whole document, with no deltas"
    );
    assert_eq!(caps["semanticTokensProvider"]["range"], false);
    assert_eq!(
        caps["workspaceSymbolProvider"],
        json!({"resolveProvider": false}),
        "every workspace symbol is complete when sent: there is no resolve"
    );

    // 2. Open: a diagnostic, its code, and its exact UTF-16 range after non-ASCII text.
    c.open(&main, 1, MAIN);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["version"], 1);
    let diag = &d["diagnostics"][0];
    assert_eq!(diag["code"], "N0300", "{d}");
    assert_eq!(diag["source"], "nazm");
    assert_eq!(diag["severity"], 1);
    let line = MAIN.lines().nth(1).unwrap();
    let s = line.rfind("(s)").unwrap() + 1;
    assert_eq!(
        diag["range"]["start"],
        json!({"line": 1, "character": utf16(line, s)})
    );
    assert_eq!(
        diag["range"]["end"],
        json!({"line": 1, "character": utf16(line, s + 1)})
    );

    // 3. Change to valid source: the set is published empty, so the error is cleared.
    c.change(&main, 2, MAIN_OK);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["version"], 2);
    assert_eq!(d["diagnostics"], json!([]));

    // 4. Definition, across files, to the defining token, in UTF-16 after non-ASCII text.
    let line = MAIN_OK.lines().nth(1).unwrap();
    let call = line.find("helper").unwrap();
    let loc = c.definition(&main, 1, utf16(line, call) + 2);
    assert_eq!(loc["uri"], uri(&lib), "{loc}");
    let lib_line = LIB.lines().next().unwrap();
    let def = lib_line.find("helper").unwrap();
    assert_eq!(
        loc["range"]["start"],
        json!({"line": 0, "character": utf16(lib_line, def)})
    );
    assert_eq!(
        loc["range"]["end"],
        json!({"line": 0, "character": utf16(lib_line, def + "helper".len())})
    );
    // And hover, from the resolution.
    let h = c.request(
        "textDocument/hover",
        json!({"textDocument": {"uri": uri(&main)}, "position": {"line": 1, "character": utf16(line, call)}}),
    );
    assert_eq!(
        h["result"]["contents"]["value"], "fn helper(n: Int) -> Int",
        "{h}"
    );

    // 5. An unsaved dependency: `helper` takes a string in the buffer, not on disk. The
    //    importer's diagnostics are recomputed against the buffer.
    let lib_unsaved = "pub fn helper(n: Str) -> Int { 0 }\n";
    c.open(&lib, 1, lib_unsaved);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(
        d["diagnostics"][0]["code"], "N0300",
        "the importer sees the buffer: {d}"
    );
    let loc = c.definition(&main, 1, utf16(line, call));
    assert_eq!(
        loc["range"]["start"],
        json!({"line": 0, "character": 7}),
        "the location is in the buffer's text: {loc}"
    );

    // 6. Close the dependency: the disk is authoritative again.
    c.close(&lib);
    let mut cleared_lib = false;
    let mut main_again = None;
    while !(cleared_lib && main_again.is_some()) {
        let m = c.next();
        if m["method"] != "textDocument/publishDiagnostics" {
            continue;
        }
        if m["params"]["uri"] == uri(&lib) {
            assert_eq!(m["params"]["diagnostics"], json!([]));
            cleared_lib = true;
        } else if m["params"]["uri"] == uri(&main) {
            main_again = Some(m["params"].clone());
        }
    }
    assert_eq!(
        main_again.unwrap()["diagnostics"],
        json!([]),
        "disk truth is back"
    );

    // 7. Something not implemented is an error response, and the server carries on.
    let r = c.request("textDocument/prepareCallHierarchy", json!({}));
    assert_eq!(r["error"]["code"], -32601, "{r}");
    let r = c.request("textDocument/definition", json!({"nonsense": true}));
    assert_eq!(r["error"]["code"], -32602, "{r}");

    // 8. Shutdown and exit, cleanly, with nothing but protocol on stdout throughout.
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn malformed_source_is_diagnosed_and_never_stops_the_server() {
    let (_dir, root) = project(&[]);
    let m = root.join("m.nz");
    let mut c = Client::start(&root);
    c.initialize(&root);
    for (version, text) in [
        "fn main( {{{ ☕ \"open",
        "}}}}))))",
        "fn main() -> Int { let x = ; x }",
        "",
        "@@@ é 😀",
    ]
    .iter()
    .enumerate()
    {
        let version = i32::try_from(version).unwrap() + 1;
        if version == 1 {
            c.open(&m, version, text);
        } else {
            c.change(&m, version, text);
        }
        let d = c.diagnostics_for(&uri(&m));
        assert_eq!(d["version"], version);
        // A query in the middle of it all is answered. On `main`'s name that is `main`
        // itself where the declaration survived (N17: a declaration's definition is
        // itself), and nothing where it did not; on a name inside a body that did not
        // parse, it is nothing.
        let at_main = c.definition(&m, 0, 3);
        assert!(at_main.is_null() || at_main["uri"] == uri(&m), "{at_main}");
        if let Some(x) = text.rfind("x }") {
            let x = u32::try_from(x).unwrap();
            assert_eq!(c.definition(&m, 0, x), Value::Null, "{text}");
            let r = c.request(
                "textDocument/references",
                json!({"textDocument": {"uri": uri(&m)}, "position": {"line": 0, "character": x}, "context": {"includeDeclaration": true}}),
            );
            assert_eq!(r["result"], Value::Null, "{r}");
        }
    }
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn a_frame_that_is_not_json_rpc_ends_the_session_without_a_panic() {
    // Protocol errors are the protocol library's to handle; they are not Nazm
    // diagnostics, and the process must not panic on one.
    let (_dir, root) = project(&[]);
    let mut c = Client::start(&root);
    c.initialize(&root);
    write!(c.stdin, "Content-Length: 9\r\n\r\nnot json!").unwrap();
    c.stdin.flush().unwrap();
    let status = wait(&mut c.child);
    let mut err = String::new();
    c.child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(!err.contains("panicked"), "{err}");
    assert_ne!(status, None);
}

impl Client {
    fn references(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Value {
        self.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": uri(path)},
                "position": {"line": line, "character": character},
                "context": {"includeDeclaration": include_declaration}
            }),
        )["result"]
            .clone()
    }
}

/// A location as the protocol writes it: `(uri, start line, start column, end column)`.
fn located(v: &Value) -> Vec<(String, u64, u64, u64)> {
    v.as_array()
        .unwrap_or_else(|| panic!("not a list of locations: {v}"))
        .iter()
        .map(|l| {
            let r = &l["range"];
            assert_eq!(r["start"]["line"], r["end"]["line"], "{l}");
            (
                l["uri"].as_str().unwrap().to_owned(),
                r["start"]["line"].as_u64().unwrap(),
                r["start"]["character"].as_u64().unwrap(),
                r["end"]["character"].as_u64().unwrap(),
            )
        })
        .collect()
}

// Non-ASCII before the queried name and before every name returned, on the same line.
const REF_LIB: &str = "pub fn tag() -> Str { \"é→😀\" } pub fn helper(n: Int) -> Int { n }\n\
pub fn again() -> Int { let s = \"😀→\"; helper(2) }\n";
const REF_MAIN: &str =
    "use \"lib.nz\";\nfn main() -> Int { let s = \"é→😀\"; helper(1) + again() }\n";

#[test]
fn references_cross_the_protocol_in_utf16_from_the_current_buffers() {
    let (_dir, root) = project(&[("lib.nz", REF_LIB), ("main.nz", REF_MAIN)]);
    let (lib, main) = (root.join("lib.nz"), root.join("main.nz"));
    let mut c = Client::start(&root);
    let init = c.initialize(&root);
    assert_eq!(init["result"]["capabilities"]["referencesProvider"], true);
    c.open(&main, 1, REF_MAIN);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["diagnostics"], json!([]), "{d}");

    // Every expected column is counted here, independently, in UTF-16 units.
    let main_line = REF_MAIN.lines().nth(1).unwrap();
    let lib0 = REF_LIB.lines().next().unwrap();
    let lib1 = REF_LIB.lines().nth(1).unwrap();
    let span = |uri: &str, n: u64, line: &str, at: usize| {
        (
            uri.to_owned(),
            n,
            u64::from(utf16(line, at)),
            u64::from(utf16(line, at + "helper".len())),
        )
    };
    let (lu, mu) = (uri(&lib), uri(&main));
    let declaration = span(&lu, 0, lib0, lib0.find("helper").unwrap());
    let in_lib = span(&lu, 1, lib1, lib1.find("helper").unwrap());
    let call = main_line.find("helper").unwrap();
    let in_main = span(&mu, 1, main_line, call);
    // Sanity: the bytes and the units differ on every one of these lines.
    assert_ne!(utf16(main_line, call) as usize, call);

    // From a use in the middle of the name, without and with the declaration, in the
    // canonical order: by file, then position.
    let q = utf16(main_line, call) + 3;
    assert_eq!(
        located(&c.references(&main, 1, q, false)),
        [in_lib.clone(), in_main.clone()]
    );
    assert_eq!(
        located(&c.references(&main, 1, q, true)),
        [declaration.clone(), in_lib.clone(), in_main.clone()]
    );

    // An unsaved change: a new use appears, at its UTF-16 position in the buffer.
    let edited =
        "use \"lib.nz\";\nfn main() -> Int { let s = \"é→😀\"; helper(1) + again() + helper(3) }\n";
    c.change(&main, 2, edited);
    c.diagnostics_for(&uri(&main));
    let edited_line = edited.lines().nth(1).unwrap();
    let third = edited_line.rfind("helper").unwrap();
    assert_eq!(
        located(&c.references(&main, 1, q, false)),
        [in_lib, in_main, span(&mu, 1, edited_line, third)]
    );

    // Nothing to refer to: a string's contents, and a keyword.
    let in_string = utf16(edited_line, edited_line.find('→').unwrap());
    assert_eq!(c.references(&main, 1, in_string, true), Value::Null);
    assert_eq!(c.references(&main, 1, 1, true), Value::Null);
    // And a malformed request is an error, not a crash.
    let r = c.request("textDocument/references", json!({"nonsense": true}));
    assert_eq!(r["error"]["code"], -32602, "{r}");

    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// The references `nazm lsp` gives for `helper` in `main.nz` of the project at `root`,
/// with the project's own directory taken out of every URI so two projects compare.
fn references_in(root: &Path) -> Vec<(String, u64, u64, u64)> {
    let main = root.join("main.nz");
    let mut c = Client::start(root);
    c.initialize(root);
    c.open(&main, 1, REF_MAIN);
    c.diagnostics_for(&uri(&main));
    let line = REF_MAIN.lines().nth(1).unwrap();
    let got = c.references(&main, 1, utf16(line, line.find("helper").unwrap()), true);
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    let prefix = uri(root);
    located(&got)
        .into_iter()
        .map(|(u, l, s, e)| (u.replacen(&prefix, "", 1), l, s, e))
        .collect()
}

#[test]
fn references_do_not_depend_on_whether_semantic_analysis_was_cached() {
    // The language service reads no persistent cache: an unsaved buffer is not state a
    // cache may record, so every analysis is a full one. This is the evidence that a warm
    // cache beside the sources changes nothing about the answer.
    let (_warm_dir, warm) = project(&[("lib.nz", REF_LIB), ("main.nz", REF_MAIN)]);
    let (_cold_dir, cold) = project(&[("lib.nz", REF_LIB), ("main.nz", REF_MAIN)]);
    let check = |dir: &Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_nazm"))
            .args(["check", "--cache-report", "main.nz"])
            .current_dir(dir)
            .env_remove("NAZM_CACHE_DIR")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let report = String::from_utf8(out.stdout).unwrap();
        let line = report.lines().next().unwrap().to_owned();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    let first = check(&warm);
    assert_eq!(
        first["stored"].as_u64().map(|n| n > 0),
        Some(true),
        "{first}"
    );
    let second = check(&warm);
    assert_eq!(
        second["reused"].as_u64().map(|n| n > 0),
        Some(true),
        "the cache is warm: {second}"
    );

    let from_warm = references_in(&warm);
    let from_cold = references_in(&cold);
    assert_eq!(from_warm.len(), 3, "{from_warm:?}");
    assert_eq!(from_warm, from_cold);
}

// Non-ASCII before the declaration and before both calls, on their lines.
const RENAME_LIB: &str = "pub fn exported() -> Int { 1 }\n";
const RENAME_MAIN: &str = "use \"lib.nz\";\nfn helper(n: Int) -> Int { n } // é→😀\n\
fn main() -> Int { let s = \"é→😀\"; helper(str_len(s)) + helper(exported()) }\n";

impl Client {
    fn at(&mut self, method: &str, path: &Path, line: u32, character: u32, extra: Value) -> Value {
        let mut params = json!({"textDocument": {"uri": uri(path)}, "position": {"line": line, "character": character}});
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        self.request(method, params)
    }
}

#[test]
fn rename_crosses_the_protocol_as_a_versioned_edit_in_utf16() {
    let (_dir, root) = project(&[("lib.nz", RENAME_LIB), ("main.nz", RENAME_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize(&root);
    c.open(&main, 1, RENAME_MAIN);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["diagnostics"], json!([]), "{d}");

    let decl_line = RENAME_MAIN.lines().nth(1).unwrap();
    let call_line = RENAME_MAIN.lines().nth(2).unwrap();
    let first = call_line.find("helper").unwrap();
    let second = call_line.rfind("helper").unwrap();
    let range = |line: u64, text: &str, at: usize| {
        json!({"start": {"line": line, "character": utf16(text, at)},
               "end": {"line": line, "character": utf16(text, at + "helper".len())}})
    };

    // prepareRename: the token under the cursor and its spelling.
    let q = utf16(call_line, second) + 2;
    let r = c.at("textDocument/prepareRename", &main, 2, q, json!({}));
    assert_eq!(r["result"]["range"], range(2, call_line, second), "{r}");
    assert_eq!(r["result"]["placeholder"], "helper");
    // ...and nothing for a built-in, a string, or an exported function.
    for (character, what) in [
        (
            utf16(call_line, call_line.find("str_len").unwrap()) + 1,
            "a built-in",
        ),
        (utf16(call_line, call_line.find('→').unwrap()), "a string"),
        (
            utf16(call_line, call_line.find("exported").unwrap()) + 1,
            "an exported function",
        ),
    ] {
        let r = c.at("textDocument/prepareRename", &main, 2, character, json!({}));
        assert_eq!(r["result"], Value::Null, "{what}: {r}");
    }

    // rename: one versioned change for the document, three edits, every range in UTF-16.
    let r = c.at(
        "textDocument/rename",
        &main,
        2,
        q,
        json!({"newName": "assist"}),
    );
    let edit = &r["result"];
    assert_eq!(
        edit["changes"],
        Value::Null,
        "never the unversioned form: {r}"
    );
    let change = &edit["documentChanges"][0];
    assert_eq!(edit["documentChanges"].as_array().unwrap().len(), 1);
    assert_eq!(
        change["textDocument"],
        json!({"uri": uri(&main), "version": 1})
    );
    let ranges: Vec<Value> = change["edits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            assert_eq!(e["newText"], "assist");
            e["range"].clone()
        })
        .collect();
    assert_eq!(
        ranges,
        [
            range(1, decl_line, decl_line.find("helper").unwrap()),
            range(2, call_line, first),
            range(2, call_line, second),
        ]
    );

    // After an unsaved edit the rename is of the new version.
    let edited = RENAME_MAIN.replace("+ helper(exported())", "+ helper(exported()) + helper(3)");
    c.change(&main, 2, &edited);
    c.diagnostics_for(&uri(&main));
    let r = c.at(
        "textDocument/rename",
        &main,
        2,
        q,
        json!({"newName": "assist"}),
    );
    let change = &r["result"]["documentChanges"][0];
    assert_eq!(change["textDocument"]["version"], 2, "{r}");
    assert_eq!(change["edits"].as_array().unwrap().len(), 4);

    // Refusals are errors with the reason: an exported function, an invalid name.
    let exported = utf16(call_line, call_line.find("exported").unwrap()) + 1;
    let r = c.at(
        "textDocument/rename",
        &main,
        2,
        exported,
        json!({"newName": "x"}),
    );
    assert_eq!(r["error"]["code"], -32803, "{r}");
    assert!(
        r["error"]["message"].as_str().unwrap().contains("exported"),
        "{r}"
    );
    let r = c.at("textDocument/rename", &main, 2, q, json!({"newName": "fn"}));
    assert_eq!(r["error"]["code"], -32803, "{r}");

    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn a_client_that_cannot_take_versioned_edits_is_refused_a_rename() {
    let (_dir, root) = project(&[("lib.nz", RENAME_LIB), ("main.nz", RENAME_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize_with(&root, json!({}));
    c.open(&main, 1, RENAME_MAIN);
    c.diagnostics_for(&uri(&main));
    let call_line = RENAME_MAIN.lines().nth(2).unwrap();
    let q = utf16(call_line, call_line.rfind("helper").unwrap());
    // prepareRename still answers — the entity is renameable — but the edit cannot be
    // tied to a version, so no edit is sent.
    let r = c.at("textDocument/prepareRename", &main, 2, q, json!({}));
    assert_eq!(r["result"]["placeholder"], "helper", "{r}");
    let r = c.at(
        "textDocument/rename",
        &main,
        2,
        q,
        json!({"newName": "assist"}),
    );
    assert_eq!(r["error"]["code"], -32803, "{r}");
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("versioned"),
        "{r}"
    );
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

// Non-ASCII before the identifier being completed, on its line.
const COMPLETE_MAIN: &str = "fn helper(n: Int) -> Int { n }\nfn help_me() -> Int { 1 }\n\
fn main() -> Int { let s = \"é😀\"; let hel = str_len(s); hel + help }\n";

#[test]
fn completion_crosses_the_protocol_replacing_the_identifier_in_utf16() {
    let (_dir, root) = project(&[("main.nz", COMPLETE_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize(&root);
    c.open(&main, 1, COMPLETE_MAIN);
    c.diagnostics_for(&uri(&main));
    let line = COMPLETE_MAIN.lines().nth(2).unwrap();

    // `hel` in `hel + help`, with the cursor at its end: a value position whose prefix is
    // `hel` — the local `hel`, and nothing else (the functions `helper` and `help_me` are
    // not values).
    let word = line.rfind("hel +").unwrap();
    let r = c.at(
        "textDocument/completion",
        &main,
        2,
        utf16(line, word + 3),
        json!({}),
    );
    let list = &r["result"];
    assert_eq!(list["isIncomplete"], false, "{r}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{r}");
    assert_eq!(items[0]["label"], "hel");
    assert_eq!(items[0]["kind"], 6, "a variable");
    assert_eq!(items[0]["detail"], "hel: Int");
    // The edit replaces the whole identifier, in UTF-16 after `é😀`.
    let range = json!({"start": {"line": 2, "character": utf16(line, word)},
                       "end": {"line": 2, "character": utf16(line, word + 3)}});
    assert_eq!(items[0]["textEdit"]["range"], range);
    assert_eq!(items[0]["textEdit"]["newText"], "hel");
    assert_ne!(
        utf16(line, word) as usize,
        word,
        "bytes and units differ here"
    );

    // A call head: `str_len` with the prefix `str`, from the compiler's built-ins; the
    // user's functions only where the prefix fits.
    let call = line.find("str_len").unwrap();
    let r = c.at(
        "textDocument/completion",
        &main,
        2,
        utf16(line, call + 3),
        json!({}),
    );
    let labels: Vec<&str> = r["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"str_len") && labels.iter().all(|l| l.starts_with("str")),
        "{r}"
    );
    assert_eq!(
        r["result"]["items"][0]["textEdit"]["range"]["end"],
        json!({"line": 2, "character": utf16(line, call + "str_len".len())}),
        "the whole call head is replaced, not only its prefix"
    );
    // Nowhere to complete: a string's contents.
    let r = c.at(
        "textDocument/completion",
        &main,
        2,
        utf16(line, line.find('😀').unwrap()),
        json!({}),
    );
    assert_eq!(r["result"], Value::Null, "{r}");

    // An unsaved edit is what is completed.
    let edited = COMPLETE_MAIN.replace(
        "let hel = str_len(s);",
        "let hel = str_len(s); let helium = 2;",
    );
    c.change(&main, 2, &edited);
    c.diagnostics_for(&uri(&main));
    let line = edited.lines().nth(2).unwrap();
    let word = line.rfind("hel +").unwrap();
    let r = c.at(
        "textDocument/completion",
        &main,
        2,
        utf16(line, word + 3),
        json!({}),
    );
    let labels: Vec<&str> = r["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["hel", "helium"], "{r}");

    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

// Non-ASCII before the call and inside its first argument, on its line.
const SIGNATURE_MAIN: &str = "fn pair(a: Str, b: Int) -> Int { b }\nfn ping() -> Int { 0 }\n\
fn main() -> Int { let s = \"é😀\"; pair(\"😀, é\", str_len(s)) + ping() + pair(s, 1, 2) }\n";

#[test]
fn signature_help_crosses_the_protocol_with_utf16_positions_and_label_offsets() {
    let (_dir, root) = project(&[("main.nz", SIGNATURE_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize(&root);
    c.open(&main, 1, SIGNATURE_MAIN);
    c.diagnostics_for(&uri(&main));
    let line = SIGNATURE_MAIN.lines().nth(2).unwrap();
    let help = |c: &mut Client, byte: usize| {
        c.at(
            "textDocument/signatureHelp",
            &main,
            2,
            utf16(line, byte),
            json!({}),
        )["result"]
            .clone()
    };

    // Inside the first argument, a string with a comma after `😀`: parameter 0.
    let first = line.find("😀, é").unwrap() + "😀,".len();
    let r = help(&mut c, first);
    assert_eq!(r["signatures"].as_array().unwrap().len(), 1, "{r}");
    let sig = &r["signatures"][0];
    assert_eq!(sig["label"], "fn pair(a: Str, b: Int) -> Int");
    assert_eq!(
        sig["parameters"],
        json!([{"label": [8, 14]}, {"label": [16, 22]}]),
        "each parameter as offsets into the label"
    );
    assert_eq!(
        (r["activeSignature"].clone(), r["activeParameter"].clone()),
        (json!(0), json!(0))
    );
    assert_eq!(sig["activeParameter"], 0);
    assert_ne!(
        utf16(line, first) as usize,
        first,
        "bytes and units differ here"
    );

    // After the separator, in the nested call's argument: the nested call.
    let nested = line.find("str_len(s)").unwrap() + "str_len(".len();
    let r = help(&mut c, nested);
    assert_eq!(r["signatures"][0]["label"], "fn str_len(Str) -> Int");
    assert_eq!(
        r["signatures"][0]["parameters"],
        json!([{"label": [11, 14]}])
    );
    // After that call's `)`, still in the outer second argument.
    let r = help(&mut c, nested + "s)".len());
    assert_eq!(
        r["signatures"][0]["label"],
        "fn pair(a: Str, b: Int) -> Int"
    );
    assert_eq!(r["activeParameter"], 1);

    // No parameters: the signature, and no active parameter at all.
    let r = help(&mut c, line.find("ping()").unwrap() + "ping(".len());
    assert_eq!(r["signatures"][0]["label"], "fn ping() -> Int");
    assert_eq!(r["signatures"][0]["parameters"], json!([]));
    assert!(r.get("activeParameter").is_none(), "{r}");
    assert!(r["signatures"][0].get("activeParameter").is_none(), "{r}");

    // Past the last parameter of a call with too many arguments: nothing, rather than an
    // absent index the protocol would read as the first parameter.
    let r = help(&mut c, line.rfind(", 2)").unwrap() + 2);
    assert_eq!(r, Value::Null);
    // Outside any call's argument list: nothing, not the nearest call.
    let r = help(&mut c, line.find(") + ping").unwrap() + 1);
    assert_eq!(r, Value::Null);

    // The current, unsaved text.
    let edited = SIGNATURE_MAIN.replace("fn pair(a: Str, b: Int)", "fn pair(first: Str, b: Int)");
    c.change(&main, 2, &edited);
    c.diagnostics_for(&uri(&main));
    let line = edited.lines().nth(2).unwrap();
    let r = c.at(
        "textDocument/signatureHelp",
        &main,
        2,
        utf16(line, line.find("😀, é").unwrap()),
        json!({}),
    );
    assert_eq!(
        r["result"]["signatures"][0]["label"],
        "fn pair(first: Str, b: Int) -> Int"
    );
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

// Non-ASCII before every structure site, on its line.
const STRUCTURE_MAIN: &str = "struct Point { x: Int, label: Str, }\nenum State { Done(code: Int), Idle, }\n\
fn main() -> Int { let s = \"é😀\"; let p = Point(x: 1, label: s); let d = State.Done(code: 2); let e = State.Idle(); p.label; match d { State.Done(code: c) => c, State.Idle() => p.x, } }\n";

#[test]
fn structure_completion_and_constructor_help_cross_the_protocol_in_utf16() {
    let (_dir, root) = project(&[("main.nz", STRUCTURE_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    let init = c.initialize(&root);
    assert_eq!(
        init["result"]["capabilities"]["completionProvider"],
        json!({"resolveProvider": false}),
        "still invoked only: no `.` trigger, since a name must be there to complete"
    );
    c.open(&main, 1, STRUCTURE_MAIN);
    c.diagnostics_for(&uri(&main));
    let line = STRUCTURE_MAIN.lines().nth(2).unwrap();
    let complete = |c: &mut Client, byte: usize| {
        c.at(
            "textDocument/completion",
            &main,
            2,
            utf16(line, byte),
            json!({}),
        )["result"]["items"]
            .clone()
    };
    let labels = |items: &Value| -> Vec<(String, u64)> {
        items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["label"].as_str().unwrap().to_owned(),
                    i["kind"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    const FIELD: u64 = 5;
    const ENUM_MEMBER: u64 = 20;

    // A member, at the end of `lab` in `p.label`: `label`, a field, replacing `label` whole.
    let member = line.find("p.label;").unwrap() + 2;
    let items = complete(&mut c, member + 3);
    assert_eq!(labels(&items), [("label".to_owned(), FIELD)]);
    assert_eq!(items[0]["detail"], "label: Str");
    assert_eq!(
        items[0]["textEdit"]["range"],
        json!({"start": {"line": 2, "character": utf16(line, member)},
               "end": {"line": 2, "character": utf16(line, member + "label".len())}})
    );
    assert_ne!(
        utf16(line, member) as usize,
        member,
        "bytes and units differ here"
    );
    // A variant, from the start of `Idle` in `State.Idle()`.
    let variant = line.find("State.Idle()").unwrap() + "State.".len();
    let items = complete(&mut c, variant);
    assert_eq!(
        labels(&items),
        [
            ("Done".to_owned(), ENUM_MEMBER),
            ("Idle".to_owned(), ENUM_MEMBER)
        ]
    );
    assert_eq!(items[0]["detail"], "Done(code: Int)");
    // A construction label: `x` is given, so `label`.
    let label = line.find("label: s").unwrap();
    assert_eq!(
        labels(&complete(&mut c, label)),
        [("label".to_owned(), FIELD)]
    );
    // A payload label, in a construction and in a pattern.
    let code = line.find("code: 2").unwrap();
    assert_eq!(
        labels(&complete(&mut c, code)),
        [("code".to_owned(), FIELD)]
    );
    let pattern = line.find("code: c").unwrap();
    assert_eq!(
        labels(&complete(&mut c, pattern)),
        [("code".to_owned(), FIELD)]
    );

    // Constructor signature help, inside `x`'s initialiser: `x` is parameter 1 of the
    // canonical `Point(label: Str, x: Int)`, though written first.
    let r = c.at(
        "textDocument/signatureHelp",
        &main,
        2,
        utf16(line, line.find("x: 1").unwrap() + 3),
        json!({}),
    );
    let sig = &r["result"]["signatures"][0];
    assert_eq!(sig["label"], "Point(label: Str, x: Int)", "{r}");
    assert_eq!(
        sig["parameters"],
        json!([{"label": [6, 16]}, {"label": [18, 24]}])
    );
    assert_eq!(r["result"]["activeParameter"], 1);
    // Between initialisers: the shape, and no parameter marked.
    let r = c.at(
        "textDocument/signatureHelp",
        &main,
        2,
        utf16(line, line.find(", label: s").unwrap() + 1),
        json!({}),
    );
    assert_eq!(
        r["result"]["signatures"][0]["label"],
        "Point(label: Str, x: Int)"
    );
    assert!(
        r["result"]["signatures"][0].get("parameters").is_none(),
        "{r}"
    );
    assert!(r["result"].get("activeParameter").is_none(), "{r}");
    // A variant construction.
    let r = c.at(
        "textDocument/signatureHelp",
        &main,
        2,
        utf16(line, code + "code: ".len()),
        json!({}),
    );
    assert_eq!(
        r["result"]["signatures"][0]["label"],
        "State.Done(code: Int)"
    );
    assert_eq!(r["result"]["activeParameter"], 0);
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

// N22: `é` is two bytes and one UTF-16 unit, `😀` four bytes and two units, before every
// declaration whose position is checked.
const SYMBOL_LIB: &str =
    "pub fn tag() -> Str { \"é😀\" } pub struct Pair { left: Int, right: Str }\n";
const SYMBOL_MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { let s = \"é😀\"; 0 } enum State { Idle, Done(code: Int) }\n";

#[test]
fn symbols_cross_the_protocol_nested_and_in_utf16() {
    const FUNCTION: u64 = 12;
    const ENUM: u64 = 10;
    const ENUM_MEMBER: u64 = 22;
    const FIELD: u64 = 8;
    const STRUCT: u64 = 23;
    let (_dir, root) = project(&[("lib.nz", SYMBOL_LIB), ("main.nz", SYMBOL_MAIN)]);
    let (lib, main) = (root.join("lib.nz"), root.join("main.nz"));
    let line = SYMBOL_MAIN.lines().nth(1).unwrap();
    let range = |line: &str, n: u32, text: &str| {
        let start = line.find(text).unwrap();
        json!({"start": {"line": n, "character": utf16(line, start)},
               "end": {"line": n, "character": utf16(line, start + text.len())}})
    };

    let mut c = Client::start(&root);
    c.initialize_with(
        &root,
        json!({"textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": true}}}),
    );
    c.open(&main, 1, SYMBOL_MAIN);
    c.diagnostics_for(&uri(&main));
    let r = c.request(
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": uri(&main)}}),
    );
    let symbols = &r["result"];
    // Only this file's declarations: `lib.nz`'s are imported, not declared here.
    assert_eq!(symbols.as_array().unwrap().len(), 2, "{r}");
    assert_eq!(symbols[0]["name"], "main");
    assert_eq!(symbols[0]["kind"], FUNCTION);
    assert_eq!(symbols[0]["detail"], "fn main() -> Int");
    assert!(symbols[0].get("children").is_none(), "no locals: {r}");
    let state = &symbols[1];
    assert_eq!(state["name"], "State");
    assert_eq!(state["kind"], ENUM);
    // The whole declaration, and the name, in UTF-16 columns after `é😀`.
    assert_eq!(
        state["range"],
        range(line, 1, "enum State { Idle, Done(code: Int) }")
    );
    assert_eq!(state["selectionRange"], range(line, 1, "State"));
    assert_ne!(
        utf16(line, line.find("State").unwrap()) as usize,
        line.find("State").unwrap(),
        "bytes and units differ here"
    );
    let variants: Vec<(&str, u64)> = state["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["name"].as_str().unwrap(), v["kind"].as_u64().unwrap()))
        .collect();
    assert_eq!(variants, [("Idle", ENUM_MEMBER), ("Done", ENUM_MEMBER)]);
    let code = &state["children"][1]["children"][0];
    assert_eq!(code["name"], "code");
    assert_eq!(code["kind"], FIELD);
    assert_eq!(code["selectionRange"], range(line, 1, "code"));
    assert_eq!(code["detail"], "Int");

    // Workspace symbols: `lib.nz` is loaded by the open program, and is searched.
    let r = c.request("workspace/symbol", json!({"query": "Pai"}));
    let lib_line = SYMBOL_LIB.lines().next().unwrap();
    assert_eq!(
        r["result"],
        json!([{"name": "Pair", "kind": STRUCT,
                "location": {"uri": uri(&lib), "range": range(lib_line, 0, "Pair")}}]),
        "{r}"
    );
    let r = c.request("workspace/symbol", json!({"query": ""}));
    let listed: Vec<(&str, u64, Option<&str>)> = r["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["name"].as_str().unwrap(),
                w["kind"].as_u64().unwrap(),
                w["containerName"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("Done", ENUM_MEMBER, Some("State")),
            ("Idle", ENUM_MEMBER, Some("State")),
            ("Pair", STRUCT, None),
            ("State", ENUM, None),
            ("code", FIELD, Some("State.Done")),
            ("left", FIELD, Some("Pair")),
            ("main", FUNCTION, None),
            ("right", FIELD, Some("Pair")),
            ("tag", FUNCTION, None),
        ]
    );
    let code = r["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "code")
        .unwrap();
    assert_eq!(code["location"]["range"], range(line, 1, "code"));
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");

    // A client that does not read nested symbols is sent them flat, each naming its
    // container, each at its whole declaration.
    let mut c = Client::start(&root);
    c.initialize_with(&root, json!({}));
    c.open(&main, 1, SYMBOL_MAIN);
    c.diagnostics_for(&uri(&main));
    let r = c.request(
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": uri(&main)}}),
    );
    let flat: Vec<(&str, Option<&str>)> = r["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["containerName"].as_str()))
        .collect();
    assert_eq!(
        flat,
        [
            ("main", None),
            ("State", None),
            ("Idle", Some("State")),
            ("Done", Some("State")),
            ("code", Some("Done")),
        ]
    );
    assert_eq!(
        r["result"][1]["location"],
        json!({"uri": uri(&main), "range": range(line, 1, "enum State { Idle, Done(code: Int) }")})
    );
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

// N23: `é` is two bytes and one UTF-16 unit, `😀` four bytes and two units, before the
// identifiers of the last line; a blank line between.
const TOKENS_MAIN: &str = "struct Pt[T] {\n    x: T,\n}\n\nfn main() -> Int { let s = \"é😀\"; let p = Pt[Int](x: str_len(s)); p.x }\n";

#[test]
fn semantic_tokens_cross_the_protocol_relative_and_in_utf16() {
    const TYPE: u32 = 0;
    const STRUCT: u32 = 1;
    const TYPE_PARAMETER: u32 = 3;
    const VARIABLE: u32 = 5;
    const PROPERTY: u32 = 6;
    const FUNCTION: u32 = 8;
    const DECLARATION: u32 = 1;
    const DEFAULT_LIBRARY: u32 = 2;
    let (_dir, root) = project(&[("main.nz", TOKENS_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    let init = c.initialize(&root);
    assert_eq!(
        init["result"]["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"][8],
        "function"
    );
    c.open(&main, 1, TOKENS_MAIN);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["diagnostics"], json!([]), "{d}");

    // The expected tokens, as (line, text of the n-th occurrence on that line, type,
    // modifiers), positioned here by counting UTF-16 units independently of the server.
    let lines: Vec<&str> = TOKENS_MAIN.lines().collect();
    let expected: [(usize, &str, usize, u32, u32); 15] = [
        (0, "Pt", 0, STRUCT, DECLARATION),
        (0, "T", 0, TYPE_PARAMETER, DECLARATION),
        (1, "x", 0, PROPERTY, DECLARATION),
        (1, "T", 0, TYPE_PARAMETER, 0),
        (4, "main", 0, FUNCTION, DECLARATION),
        (4, "Int", 0, TYPE, DEFAULT_LIBRARY),
        (4, "s", 0, VARIABLE, DECLARATION),
        (4, "p", 0, VARIABLE, DECLARATION),
        (4, "Pt", 0, STRUCT, 0),
        (4, "Int", 1, TYPE, DEFAULT_LIBRARY),
        (4, "x", 0, PROPERTY, 0),
        (4, "str_len", 0, FUNCTION, DEFAULT_LIBRARY),
        (4, "s", 1, VARIABLE, 0),
        (4, "p", 1, VARIABLE, 0),
        (4, "x", 1, PROPERTY, 0),
    ];
    // Each needle as a whole word: the n-th occurrence bounded by non-identifier bytes.
    let word = |line: &str, needle: &str, n: usize| -> usize {
        let b = line.as_bytes();
        let is = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        (0..line.len())
            .filter(|&i| {
                line.is_char_boundary(i)
                    && line[i..].starts_with(needle)
                    && (i == 0 || !is(b[i - 1]))
                    && b.get(i + needle.len()).is_none_or(|&c| !is(c))
            })
            .nth(n)
            .unwrap_or_else(|| panic!("{needle:?} #{n} on {line:?}"))
    };
    let mut data = Vec::new();
    let (mut previous_line, mut previous_start) = (0u32, 0u32);
    for (line, needle, n, kind, modifiers) in expected {
        let at = word(lines[line], needle, n);
        let start = utf16(lines[line], at);
        let length = u32::try_from(needle.encode_utf16().count()).unwrap();
        let line = u32::try_from(line).unwrap();
        let (dl, ds) = if line == previous_line {
            (0, start - previous_start)
        } else {
            (line - previous_line, start)
        };
        data.extend([dl, ds, length, kind, modifiers]);
        (previous_line, previous_start) = (line, start);
    }
    assert_ne!(
        utf16(lines[4], word(lines[4], "p", 1)) as usize,
        word(lines[4], "p", 1),
        "bytes and units differ on the last line"
    );
    let r = c.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument": {"uri": uri(&main)}}),
    );
    assert_eq!(r["result"], json!({"data": data}), "{r}");
    assert!(
        r["result"].get("resultId").is_none(),
        "no deltas, so no result id"
    );

    // Neither a range nor a delta is implemented, and neither is answered.
    for method in [
        "textDocument/semanticTokens/range",
        "textDocument/semanticTokens/full/delta",
    ] {
        let r = c.request(method, json!({}));
        assert_eq!(r["error"]["code"], -32601, "{method}: {r}");
    }
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

// N24: `é` is two bytes and one UTF-16 unit, `😀` four bytes and two units, before the
// edit on line 1; the diagnostic is on line 2.
const FIX_MAIN: &str =
    "fn main() -> Int {\n    let s = \"é😀\"; let x = str_len(s);\n    x = 2;\n    x\n}\n";
const FIXED_MAIN: &str =
    "fn main() -> Int {\n    let s = \"é😀\"; let mut x = str_len(s);\n    x = 2;\n    x\n}\n";

impl Client {
    fn code_action(&mut self, path: &Path, range: Value, context: Value) -> Value {
        self.request(
            "textDocument/codeAction",
            json!({"textDocument": {"uri": uri(path)}, "range": range, "context": context}),
        )
    }
}

fn point(line: u32, character: u32) -> Value {
    json!({"start": {"line": line, "character": character}, "end": {"line": line, "character": character}})
}

#[test]
fn a_quick_fix_crosses_the_protocol_versioned_in_utf16_and_changes_nothing_until_applied() {
    let (_dir, root) = project(&[("main.nz", FIX_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize(&root);
    c.open(&main, 3, FIX_MAIN);
    let d = c.diagnostics_for(&uri(&main));
    let diagnostic = d["diagnostics"][0].clone();
    assert_eq!(diagnostic["code"], "N0310", "{d}");
    let lines: Vec<&str> = FIX_MAIN.lines().collect();

    // A cursor on the assignment: the diagnostic's line, not the edit's.
    let r = c.code_action(&main, point(2, 5), json!({"diagnostics": []}));
    let actions = r["result"].as_array().unwrap().clone();
    assert_eq!(actions.len(), 1, "{r}");
    let a = &actions[0];
    assert_eq!(a["title"], "Fix N0310: make the binding mutable");
    assert_eq!(a["kind"], "quickfix");
    assert_eq!(a["isPreferred"], true);
    assert!(a.get("command").is_none() && a.get("data").is_none(), "{a}");
    // The current diagnostic, exactly as it was published.
    assert_eq!(a["diagnostics"], json!([diagnostic]));
    // The edit: versioned, at the `let`'s `x` after `é😀` — columns counted independently.
    let x = lines[1].find("x = str_len").unwrap();
    assert_eq!(
        a["edit"],
        json!({"documentChanges": [{
            "textDocument": {"uri": uri(&main), "version": 3},
            "edits": [{
                "range": {"start": {"line": 1, "character": utf16(lines[1], x)},
                          "end": {"line": 1, "character": utf16(lines[1], x + 1)}},
                "newText": "mut x"
            }]
        }]}),
        "{a}"
    );
    assert_ne!(
        utf16(lines[1], x) as usize,
        x,
        "bytes and units differ here"
    );
    assert!(
        a["edit"].get("changes").is_none(),
        "never the unversioned form"
    );

    // Returning it changed nothing: the same request gives the same answer.
    let again = c.code_action(&main, point(2, 5), json!({"diagnostics": []}));
    assert_eq!(again["result"], r["result"]);

    // The client applies it and says so: the diagnostic and the action are gone.
    c.change(&main, 4, FIXED_MAIN);
    let d = c.diagnostics_for(&uri(&main));
    assert_eq!(d["diagnostics"], json!([]), "{d}");
    assert_eq!(d["version"], 4);
    let r = c.code_action(&main, point(2, 5), json!({"diagnostics": [diagnostic]}));
    assert_eq!(
        r["result"],
        json!([]),
        "the client's stale copy of the diagnostic resurrects nothing"
    );
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

#[test]
fn quick_fixes_come_from_current_diagnostics_only_and_only_as_quick_fixes() {
    const REVIEW: &str =
        "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; i }\n    i\n}\n";
    let (_dir, root) = project(&[("main.nz", FIXED_MAIN), ("loop.nz", REVIEW)]);
    let (main, looped) = (root.join("main.nz"), root.join("loop.nz"));
    let mut c = Client::start(&root);
    c.initialize(&root);
    c.open(&main, 1, FIXED_MAIN);
    c.diagnostics_for(&uri(&main));
    // A plausible diagnostic the source does not have, with a real code: nothing.
    let fabricated = json!({
        "range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 5}},
        "severity": 1, "code": "N0310", "source": "nazm",
        "message": "`x` is not mutable, so it cannot be assigned"
    });
    let r = c.code_action(&main, point(2, 4), json!({"diagnostics": [fabricated]}));
    assert_eq!(r["result"], json!([]), "{r}");

    // A fix that needs review: offered, never preferred, its precondition in the title.
    c.open(&looped, 1, REVIEW);
    c.diagnostics_for(&uri(&looped));
    let line = REVIEW.lines().nth(2).unwrap();
    let tail = u32::try_from(line.find("i }").unwrap()).unwrap();
    let r = c.code_action(&looped, point(2, tail), json!({"diagnostics": []}));
    let a = &r["result"][0];
    assert_eq!(a["kind"], "quickfix", "{r}");
    let title = a["title"].as_str().unwrap();
    assert!(
        title.starts_with("Review fix N0300: discard the value — right only if "),
        "{title}"
    );
    assert!(title.contains("meant to be discarded"), "{title}");
    assert!(a.get("isPreferred").is_none(), "{a}");
    assert_eq!(a["edit"]["documentChanges"][0]["edits"][0]["newText"], ";");

    // Only refactors or source actions asked for: none; quick fixes asked for: the same.
    for only in [json!(["refactor"]), json!(["source", "source.fixAll"])] {
        let r = c.code_action(
            &looped,
            point(2, tail),
            json!({"diagnostics": [], "only": only}),
        );
        assert_eq!(r["result"], json!([]), "{only}: {r}");
    }
    let r = c.code_action(
        &looped,
        point(2, tail),
        json!({"diagnostics": [], "only": ["quickfix"]}),
    );
    assert_eq!(r["result"].as_array().unwrap().len(), 1, "{r}");
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}

#[test]
fn a_client_that_cannot_take_versioned_edits_is_offered_no_quick_fix() {
    let (_dir, root) = project(&[("main.nz", FIX_MAIN)]);
    let main = root.join("main.nz");
    let mut c = Client::start(&root);
    c.initialize_with(&root, json!({}));
    c.open(&main, 1, FIX_MAIN);
    c.diagnostics_for(&uri(&main));
    let r = c.code_action(&main, point(2, 5), json!({"diagnostics": []}));
    assert_eq!(
        r["result"],
        json!([]),
        "an edit the client could apply to another version is not sent"
    );
    let (status, stderr) = c.finish();
    assert_eq!(status, Some(0), "{stderr}");
}
