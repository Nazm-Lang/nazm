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
            "completionProvider",
            "definitionProvider",
            "hoverProvider",
            "positionEncoding",
            "referencesProvider",
            "renameProvider",
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
    assert_eq!(caps["referencesProvider"], true);
    assert_eq!(caps["renameProvider"], json!({"prepareProvider": true}));
    assert_eq!(
        caps["completionProvider"],
        json!({"resolveProvider": false}),
        "invoked only: no trigger characters, nothing to resolve"
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
    let r = c.request("textDocument/signatureHelp", json!({}));
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
