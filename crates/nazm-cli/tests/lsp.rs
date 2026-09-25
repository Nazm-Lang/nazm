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
        let r = self.request(
            "initialize",
            json!({"processId": null, "rootUri": uri(root), "capabilities": {}}),
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
            "definitionProvider",
            "hoverProvider",
            "positionEncoding",
            "textDocumentSync"
        ],
        "only what is implemented is advertised: {init}"
    );
    assert_eq!(caps["positionEncoding"], "utf-16");
    assert_eq!(
        caps["textDocumentSync"]["change"], 1,
        "full synchronisation"
    );
    assert_eq!(caps["textDocumentSync"]["openClose"], true);

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
        h["result"]["contents"]["value"], "fn helper(Int) -> Int",
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
    let r = c.request("textDocument/completion", json!({}));
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
        // A query in the middle of it all is answered, with no answer.
        assert_eq!(c.definition(&m, 0, 3), Value::Null);
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
