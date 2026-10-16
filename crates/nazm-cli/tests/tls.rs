//! v1 track C, C1: `@std/tls` over the runtime's `tls` part.
//!
//! `docs/spec.md`, *TLS*. A test CA, server certificates and a stranger's are made for each test
//! by the `openssl` command line (OpenSSL 3, the engine's own toolkit), never checked in. What the
//! programs show, built by every backend — LLVM at `-O0` and `-O2`, Cranelift, and Nazm's own code
//! generator on macOS: a server and a client over loopback agreeing a version and an ALPN name,
//! echoing data across many records and closing TLS cleanly; a client refusing an expired
//! certificate, one for another name, a self-signed one, one from a root it does not trust and the
//! test CA's against the platform's store, each with its kind, the server seeing the refusal; a
//! configuration refused for a key that is not the certificate's, for bytes that are not PEM and
//! for an empty ALPN name; and `@std/http` served and fetched over TLS. The interpreter, which calls
//! no C, refuses TLS as a value (`-ENOSYS`), and a target without a binding refuses the build.
//!
//! The engine's own sentences differ by platform (the Security framework's on macOS, OpenSSL's on
//! Linux), so the programs print each failure's kind, never its text.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::{self, run_bounded, Ran};
use crate::differential::{memory_line, nzcg, NAZM};

/// The `openssl` command line: `NAZM_OPENSSL_DIR`'s, Homebrew's, or the one on `PATH`.
fn openssl() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = std::env::var_os("NAZM_OPENSSL_DIR")
        .map(|d| vec![PathBuf::from(d).join("bin/openssl")])
        .unwrap_or_default();
    places.push(PathBuf::from("/opt/homebrew/opt/openssl@3/bin/openssl"));
    places.push(PathBuf::from("/usr/local/opt/openssl@3/bin/openssl"));
    if !cfg!(target_os = "macos") {
        // macOS's own `openssl` is LibreSSL; elsewhere the system's is OpenSSL 3.
        places.push(PathBuf::from("openssl"));
    }
    places.into_iter().find(|p| {
        Command::new(p).arg("version").output().is_ok_and(|o| {
            o.status.success() && String::from_utf8_lossy(&o.stdout).starts_with("OpenSSL 3")
        })
    })
}

const CA_CONFIG: &str = "[ca]\ndefault_ca = d\n[d]\ndir = .\ndatabase = index.txt\n\
new_certs_dir = .\nserial = serial\ndefault_md = sha256\npolicy = p\nunique_subject = no\n\
[p]\ncommonName = supplied\n[server]\nsubjectAltName = DNS:localhost,IP:127.0.0.1\n\
extendedKeyUsage = serverAuth\nbasicConstraints = CA:FALSE\nkeyUsage = critical,digitalSignature\n";

fn run_openssl(tool: &Path, dir: &Path, args: &[&str]) {
    let r = run_bounded(Command::new(tool).current_dir(dir).args(args), "openssl");
    assert_eq!(r.code, Some(0), "openssl {args:?}: {}", r.stderr);
}

/// A test CA (`ca.pem`), another nobody trusts (`other.pem`), and server certificates: `srv.pem`
/// for `localhost` and `127.0.0.1` from the test CA, valid for 30 days; `old.pem`, the same name
/// and key, expired in 2020; `stranger.pem`, the same from the other CA; and `self.pem`,
/// self-signed with a key of its own (`self.key`).
fn certificates(dir: &Path) -> bool {
    let Some(tool) = openssl() else {
        return false;
    };
    let ec = [
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:P-256",
        "-nodes",
    ];
    for (name, subject) in [("ca", "/CN=Nazm Test CA"), ("other", "/CN=Nazm Other CA")] {
        let (key, pem) = (format!("{name}.key"), format!("{name}.pem"));
        let mut args = vec!["req", "-x509"];
        args.extend(ec);
        args.extend([
            "-keyout",
            &key,
            "-out",
            &pem,
            "-subj",
            subject,
            "-days",
            "30",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ]);
        run_openssl(&tool, dir, &args);
    }
    let mut args = vec!["req"];
    args.extend(ec);
    args.extend([
        "-keyout",
        "srv.key",
        "-out",
        "srv.csr",
        "-subj",
        "/CN=localhost",
    ]);
    run_openssl(&tool, dir, &args);
    std::fs::write(dir.join("ca.cnf"), CA_CONFIG).unwrap();
    std::fs::write(dir.join("index.txt"), "").unwrap();
    std::fs::write(dir.join("serial"), "1000\n").unwrap();
    let sign = |ca: &str, out: &str, dates: &[&str]| {
        let (cert, key) = (format!("{ca}.pem"), format!("{ca}.key"));
        let mut args = vec![
            "ca",
            "-batch",
            "-notext",
            "-config",
            "ca.cnf",
            "-extensions",
            "server",
            "-cert",
            &cert,
            "-keyfile",
            &key,
            "-in",
            "srv.csr",
            "-out",
            out,
        ];
        args.extend(dates);
        run_openssl(&tool, dir, &args);
    };
    sign("ca", "srv.pem", &["-days", "30"]);
    sign(
        "ca",
        "old.pem",
        &[
            "-startdate",
            "20200101000000Z",
            "-enddate",
            "20200201000000Z",
        ],
    );
    sign("other", "stranger.pem", &["-days", "30"]);
    let mut args = vec!["req", "-x509"];
    args.extend(ec);
    args.extend([
        "-keyout",
        "self.key",
        "-out",
        "self.pem",
        "-subj",
        "/CN=localhost",
        "-days",
        "30",
        "-addext",
        "subjectAltName=DNS:localhost",
        "-addext",
        "extendedKeyUsage=serverAuth",
    ]);
    run_openssl(&tool, dir, &args);
    true
}

const PRE: &str = r#"use "@std/ioerror";
use "@std/file";
use "@std/net";
use "@std/tls";
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }
fn bytes_of(io: IoCap, path: Str) -> Bytes ! { io } {
    match file_read_all(io, path) {
        Result.Ok(value: b) => b,
        Result.Err(error: e) => bytes_new(0),
    }
}
fn names(a: Str, b: Str) -> Strs ! {} {
    let n = strs_new();
    if str_len(a) > 0 { strs_push(n, a); }
    if str_len(b) > 0 { strs_push(n, b); }
    n
}
fn kind(e: TlsError) -> Str ! {} {
    match e {
        TlsError.Io(error: x) => str_concat("Io ", io_kind_name(x.kind)),
        TlsError.Untrusted(why: w) => "Untrusted",
        TlsError.Expired(why: w) => "Expired",
        TlsError.WrongHost(why: w) => "WrongHost",
        TlsError.Protocol(why: w) => "Protocol",
        TlsError.BadKey(why: w) => "BadKey",
        TlsError.Truncated() => "Truncated",
        TlsError.Closed() => "Closed",
    }
}
fn server_config(io: IoCap, cert: Str, key: Str, alpn: Strs) -> TlsConfig ! { io } {
    match tls_server(bytes_of(io, cert), bytes_of(io, key), alpn) {
        Result.Ok(value: c) => c,
        Result.Err(error: e) => TlsConfig(id: 0),
    }
}
fn client_config(io: IoCap, ca: Str, alpn: Strs) -> TlsConfig ! { io } {
    match tls_client(if str_len(ca) > 0 { bytes_of(io, ca) } else { bytes_new(0) }, alpn) {
        Result.Ok(value: c) => c,
        Result.Err(error: e) => TlsConfig(id: 0),
    }
}
// Accept one connection: echo whatever arrives until the peer closes TLS. Into `seen`: the bytes
// echoed, or -1 for a failure after the handshake, -2 for a handshake that failed.
fn echo_one(cfg: TlsConfig, l: TcpListener, seen: Atomic) -> Int ! { io } {
    match tcp_accept(l) {
        Result.Ok(value: c) => {
            tcp_set_timeout(c, 5000);
            match tls_accept(cfg, c) {
                Result.Ok(value: t) => {
                    let buf = bytes_new(5000);
                    let mut total = 0;
                    let mut going = true;
                    while going {
                        match tls_read(t, buf) {
                            Result.Ok(value: n) => {
                                if n == 0 { going = false; } else {
                                    tls_write(t, bytes_slice(buf, 0, n));
                                    total = total + n;
                                }
                            },
                            Result.Err(error: e) => { going = false; total = 0 - 1; },
                        };
                    }
                    atomic_store(seen, total);
                    tls_close(t);
                    0
                },
                Result.Err(error: e) => { atomic_store(seen, 0 - 2); tcp_close(c); 0 },
            }
        },
        Result.Err(error: e) => { atomic_store(seen, 0 - 3); 0 },
    }
}
fn seen_as(n: Int) -> Str ! {} {
    if n == 0 - 2 { "the server's handshake failed" } else { if n < 0 { "the server failed" } else { str_concat(str_concat("the server echoed ", int_to_str(n)), " bytes") } }
}
"#;

fn prog(body: &str) -> String {
    format!("{PRE}{body}")
}

/// The compiled tiers, each building `src` in `dir` (beside the certificates) and running it there:
/// every one prints `expected`, ends 0 and reclaims everything.
fn compiled_agree(dir: &Path, src: &str, expected: &str) {
    std::fs::write(dir.join("prog.nz"), src).unwrap();
    let mut backends = vec![
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ];
    if cfg!(target_os = "macos") {
        backends.push(("nazm", "nazm", "0"));
    }
    for (label, backend, opt) in backends {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let mut build = Command::new(NAZM);
        build.current_dir(dir).args([
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ]);
        if backend == "nazm" {
            build.env(nazm_codegen_nazm::VARIABLE, nzcg());
        }
        let b = run_bounded(&mut build, "nazm build");
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(dir).env(nazm_lir::MEMORY_VARIABLE, "1");
        let r: Ran = run_bounded(&mut c, label);
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        let line = memory_line(&r);
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
}

/// A directory with the certificates, or `None` (skipped, by `common::skip_without`) on a host
/// without OpenSSL 3's command line.
fn with_certificates() -> Option<tempfile::TempDir> {
    let d = tempfile::tempdir().unwrap();
    let made = certificates(d.path());
    if common::skip_without(
        made,
        "OpenSSL 3's `openssl` command makes the test certificates",
    ) {
        return None;
    }
    Some(d)
}

const LOOPBACK: &str = r#"
// Client: connect, report the session, send `total` bytes in 16 KiB pieces and read each echo.
fn client(net: NetCap, out: OutCap, cfg: TlsConfig, addr: Str, host: Str, total: Int) -> Int ! { io } {
    match tcp_connect(net, addr, 2000) {
        Result.Ok(value: s) => {
            tcp_set_timeout(s, 5000);
            match tls_connect(cfg, s, host) {
                Result.Ok(value: t) => {
                    line(out, str_concat(str_concat(str_concat(tls_version(t), " ["), tls_alpn(t)), "]"));
                    line(out, tls_peer(t));
                    let piece = bytes_new(16384);
                    let back = bytes_new(16384);
                    let mut sent = 0;
                    let mut same = true;
                    while sent < total {
                        let mut i = 0;
                        while i < 16384 { bytes_set(piece, i, UInt8((sent + i) % 251)); i = i + 1; }
                        tls_write(t, piece);
                        let mut got = 0;
                        while got < 16384 {
                            match tls_read(t, bytes_slice(back, got, 16384)) {
                                Result.Ok(value: n) => { if n == 0 { got = 16384; same = false; } else { got = got + n; } },
                                Result.Err(error: e) => { got = 16384; same = false; },
                            };
                        }
                        i = 0;
                        while i < 16384 { if bytes_get(back, i) != bytes_get(piece, i) { same = false; } i = i + 1; }
                        sent = sent + 16384;
                    }
                    line(out, str_concat(str_concat(int_to_str(sent), " bytes echoed "), if same { "intact" } else { "damaged" }));
                    tls_close(t);
                },
                Result.Err(error: e) => { line(out, str_concat("client: ", kind(e))); tcp_close(s); },
            };
            0
        },
        Result.Err(error: e) => { line(out, "no connection"); 0 },
    }
}
fn trial(io: IoCap, net: NetCap, tasks: SpawnCap, out: OutCap, server: Strs, offered: Strs, total: Int) -> Int ! { io, spawn } {
    let sc = server_config(io, "srv.pem", "srv.key", server);
    let cc = client_config(io, "ca.pem", offered);
    let seen = atomic_new(0);
    match tcp_listen(net, "127.0.0.1:0") {
        Result.Ok(value: l) => {
            let addr = tcp_listener_addr(l);
            scope {
                spawn echo_one(sc, l, seen);
                client(net, out, cc, addr, "localhost", total);
            }
            tcp_listener_close(l);
        },
        Result.Err(error: e) => { line(out, "no listener"); },
    };
    line(out, seen_as(atomic_load(seen)));
    tls_config_close(sc);
    tls_config_close(cc);
    0
}
fn main(io: IoCap, net: NetCap, tasks: SpawnCap, out: OutCap) -> Int ! { io, spawn } {
    // The server's preference decides among what the client offers.
    trial(io, net, tasks, out, names("http/1.1", "h2"), names("h2", "http/1.1"), 49152);
    // Nothing in common: the handshake goes on without a name.
    trial(io, net, tasks, out, names("http/1.1", ""), names("h2", ""), 16384);
    0
}
"#;

#[test]
fn a_server_and_a_client_agree_and_echo_over_loopback_on_every_backend() {
    let Some(d) = with_certificates() else {
        return;
    };
    compiled_agree(
        d.path(),
        &prog(LOOPBACK),
        "TLSv1.3 [http/1.1]\n/CN=localhost\n49152 bytes echoed intact\nthe server echoed 49152 bytes\n\
         TLSv1.3 []\n/CN=localhost\n16384 bytes echoed intact\nthe server echoed 16384 bytes\n0",
    );
}

const REFUSALS: &str = r#"
// One connection to a server presenting `cert`, verified as `host` with `ca` (empty: the
// platform's store): the client's verdict and what the server saw.
fn case(io: IoCap, net: NetCap, tasks: SpawnCap, out: OutCap, what: Str, cert: Str, key: Str, ca: Str, host: Str) -> Int ! { io, spawn } {
    let sc = server_config(io, cert, key, names("http/1.1", ""));
    let cc = client_config(io, ca, names("http/1.1", ""));
    let seen = atomic_new(0);
    let mut verdict = "";
    match tcp_listen(net, "127.0.0.1:0") {
        Result.Ok(value: l) => {
            let addr = tcp_listener_addr(l);
            scope {
                spawn echo_one(sc, l, seen);
                match tcp_connect(net, addr, 2000) {
                    Result.Ok(value: s) => {
                        tcp_set_timeout(s, 5000);
                        match tls_connect(cc, s, host) {
                            Result.Ok(value: t) => { verdict = str_concat("ok ", tls_version(t)); tls_close(t); },
                            Result.Err(error: e) => { verdict = kind(e); tcp_close(s); },
                        };
                    },
                    Result.Err(error: e) => { verdict = "no connection"; },
                };
            }
            tcp_listener_close(l);
        },
        Result.Err(error: e) => { verdict = "no listener"; },
    };
    line(out, str_concat(str_concat(str_concat(str_concat(what, ": "), verdict), "; "), seen_as(atomic_load(seen))));
    tls_config_close(sc);
    tls_config_close(cc);
    0
}
fn config_error(r: Result[TlsConfig, TlsError]) -> Str ! { io } {
    match r {
        Result.Ok(value: c) => { tls_config_close(c); "accepted" },
        Result.Err(error: e) => kind(e),
    }
}
fn main(io: IoCap, net: NetCap, tasks: SpawnCap, out: OutCap) -> Int ! { io, spawn } {
    case(io, net, tasks, out, "trusted", "srv.pem", "srv.key", "ca.pem", "localhost");
    case(io, net, tasks, out, "by address", "srv.pem", "srv.key", "ca.pem", "127.0.0.1");
    case(io, net, tasks, out, "expired", "old.pem", "srv.key", "ca.pem", "localhost");
    case(io, net, tasks, out, "another name", "srv.pem", "srv.key", "ca.pem", "example.org");
    case(io, net, tasks, out, "self-signed", "self.pem", "self.key", "ca.pem", "localhost");
    case(io, net, tasks, out, "untrusted root", "stranger.pem", "srv.key", "ca.pem", "localhost");
    case(io, net, tasks, out, "platform store", "srv.pem", "srv.key", "", "localhost");
    line(out, str_concat("a key not the certificate's: ", config_error(tls_server(bytes_of(io, "srv.pem"), bytes_of(io, "self.key"), names("", "")))));
    line(out, str_concat("not PEM: ", config_error(tls_server(bytes_from_str("hello"), bytes_from_str("world"), names("", "")))));
    line(out, str_concat("anchors not PEM: ", config_error(tls_client(bytes_from_str("hello"), names("", "")))));
    let long = str_pad_end("", 256, "x");
    line(out, str_concat("an ALPN name too long: ", config_error(tls_client(bytes_new(0), names(long, "")))));
    let gone = client_config(io, "ca.pem", names("", ""));
    tls_config_close(gone);
    match tcp_listen(net, "127.0.0.1:0") {
        Result.Ok(value: l) => {
            match tcp_connect(net, tcp_listener_addr(l), 2000) {
                Result.Ok(value: s) => {
                    match tls_connect(gone, s, "localhost") {
                        Result.Ok(value: t) => { line(out, "a closed configuration: accepted"); tls_close(t); },
                        Result.Err(error: e) => { line(out, str_concat("a closed configuration: ", kind(e))); tcp_close(s); },
                    };
                    match tls_connect(TlsConfig(id: 12345), s, "localhost") {
                        Result.Ok(value: t) => { line(out, "a made-up id: accepted"); },
                        Result.Err(error: e) => { line(out, str_concat("a made-up id: ", kind(e))); },
                    };
                    // A live configuration's slot under a generation it does not have: closing it
                    // answers `Closed` and leaves the live one open.
                    let live = client_config(io, "ca.pem", names("", ""));
                    let forged = os_tls_close(live.id + 65536, bytes_new(0));
                    let session = os_tls_connect(live.id, "localhost");
                    line(out, str_concat(str_concat("another generation: ", int_to_str(forged)), if session > 0 { ", the live one open" } else { ", the live one closed" }));
                    if session > 0 { os_tls_close(session, bytes_new(0)); }
                    tls_config_close(live);
                },
                Result.Err(error: e) => { line(out, "no connection"); },
            };
            tcp_listener_close(l);
        },
        Result.Err(error: e) => { line(out, "no listener"); },
    };
    0
}
"#;

#[test]
fn a_client_refuses_every_certificate_it_cannot_verify_and_says_why() {
    let Some(d) = with_certificates() else {
        return;
    };
    compiled_agree(
        d.path(),
        &prog(REFUSALS),
        "trusted: ok TLSv1.3; the server echoed 0 bytes\n\
         by address: ok TLSv1.3; the server echoed 0 bytes\n\
         expired: Expired; the server's handshake failed\n\
         another name: WrongHost; the server's handshake failed\n\
         self-signed: Untrusted; the server's handshake failed\n\
         untrusted root: Untrusted; the server's handshake failed\n\
         platform store: Untrusted; the server's handshake failed\n\
         a key not the certificate's: BadKey\n\
         not PEM: BadKey\n\
         anchors not PEM: BadKey\n\
         an ALPN name too long: Io InvalidInput\n\
         a closed configuration: Closed\n\
         a made-up id: Closed\n\
         another generation: -65536, the live one open\n0",
    );
}

const HTTPS: &str = r#"
fn hello(r: Request) -> Response ! {} {
    match http_param(r, "name") {
        Option.Some(value: n) => http_text(200, str_concat("hello ", n)),
        Option.None() => http_text(400, "who?"),
    }
}
fn size(r: Request) -> Response ! {} {
    http_text(200, int_to_str(str_len(r.body)))
}
fn routes() -> fn(Request) -> Response ! { io } ! {} {
    http_router_handler(http_router_post(http_router_get(http_router(), "/hello/{name}", hello), "/size", size))
}
fn shown(r: Result[Response, HttpError]) -> Str ! {} {
    match r {
        Result.Ok(value: x) => str_concat(str_concat(int_to_str(x.status), " "), x.body),
        Result.Err(error: e) => match e {
            HttpError.Tls(error: t) => str_concat("ERR tls ", kind(t)),
            HttpError.Io(error: x) => str_concat("ERR io ", io_kind_name(x.kind)),
            HttpError.Protocol(status: s, why: w) => str_concat("ERR ", int_to_str(s)),
            HttpError.Unsupported(why: w) => "ERR unsupported",
        },
    }
}
fn client(io: IoCap, net: NetCap, s: HttpServer, lines: Chan[Str]) -> Int ! { io } {
    let base = str_concat("https://", http_server_addr(s));
    let o = http_client_options();
    let cc = client_config(io, "ca.pem", names("http/1.1", ""));
    match http_connect_tls(net, base, o, cc) {
        Result.Ok(value: c) => {
            chan_send_of(lines, shown(http_send(c, http_request("GET", "/hello/one", ""))));
            chan_send_of(lines, shown(http_send(c, http_request("POST", "/size", str_pad_end("", 40000, "z")))));
            chan_send_of(lines, shown(http_send(c, http_request("GET", "/hello/two", ""))));
            http_close(c);
        },
        Result.Err(error: e) => { chan_send_of(lines, http_error_show(e)); },
    };
    // The platform's store does not know the test CA.
    chan_send_of(lines, shown(http_get(net, str_concat(base, "/hello/three"), o)));
    // Plain HTTP to a TLS server: no response the client can read.
    chan_send_of(lines, shown(http_get(net, str_concat(str_concat("http://", http_server_addr(s)), "/hello/four"), o)));
    tls_config_close(cc);
    http_drain(s);
    chan_close_of(lines)
}
fn main(io: IoCap, net: NetCap, t: SpawnCap, out: OutCap) -> Int ! { io, spawn } {
    let sc = server_config(io, "srv.pem", "srv.key", names("http/1.1", ""));
    let d = http_serve_options();
    let o = ServeOptions(max_head: d.max_head, max_headers: d.max_headers, max_target: d.max_target, max_body: d.max_body, head_timeout_ms: 2000, idle_timeout_ms: 2000, body_timeout_ms: 2000, write_timeout_ms: 2000, max_requests: d.max_requests, max_connections: d.max_connections);
    match http_listen_tls(net, "127.0.0.1:0", o, sc) {
        Result.Ok(value: s) => {
            let lines = chan_new_of[Str](64);
            let h = routes();
            let mut stats = "";
            scope {
                spawn client(io, net, s, lines);
                match http_serve(t, s, h) {
                    Result.Ok(value: st) => { stats = str_concat(str_concat(int_to_str(st.requests), " requests on "), int_to_str(st.connections)); },
                    Result.Err(error: e) => { stats = io_error_show(e); },
                };
            }
            let got = vec_new[Str]();
            while chan_recv_of(lines, got) { line(out, vec_pop(got)); }
            line(out, stats);
        },
        Result.Err(error: e) => { line(out, io_error_show(e)); },
    };
    tls_config_close(sc);
    0
}
"#;

#[test]
fn http_is_served_and_fetched_over_tls() {
    let Some(d) = with_certificates() else {
        return;
    };
    compiled_agree(
        d.path(),
        &format!("use \"@std/http\";\n{}", prog(HTTPS)),
        "200 hello one\n200 40000\n200 hello two\nERR tls Untrusted\nERR 502\n3 requests on 3\n0",
    );
}

#[test]
fn the_interpreter_refuses_tls_as_a_value() {
    let d = tempfile::tempdir().unwrap();
    let src = prog(
        r#"fn main(io: IoCap, out: OutCap) -> Int ! { io } {
    match tls_client(bytes_new(0), names("", "")) {
        Result.Ok(value: c) => { line(out, "configured"); },
        Result.Err(error: e) => { line(out, kind(e)); },
    };
    match tls_read(TlsStream(stream: TcpStream(handle: os_stdin()), session: 70000), bytes_new(8)) {
        Result.Ok(value: n) => { line(out, "read"); },
        Result.Err(error: e) => { line(out, kind(e)); },
    };
    0
}
"#,
    );
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "--interpret", "prog.nz"]),
        "nazm run --interpret",
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(r.stdout.trim_end(), "Io Other\nClosed\n0");
}

#[test]
fn a_target_without_a_tls_binding_refuses_the_build_by_name() {
    let d = tempfile::tempdir().unwrap();
    let src = prog(
        r#"fn main(io: IoCap, out: OutCap) -> Int ! { io } {
    match tls_client(bytes_new(0), names("", "")) {
        Result.Ok(value: c) => 0,
        Result.Err(error: e) => 1,
    }
}
"#,
    );
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    // Windows' binding (SChannel) is not written yet: `N0830`, before anything is compiled.
    let r = run_bounded(
        Command::new(NAZM).current_dir(d.path()).args([
            "build",
            "--target",
            "x86_64-pc-windows-gnu",
            "--objects",
            "objs",
            "prog.nz",
        ]),
        "nazm build",
    );
    assert_ne!(r.code, Some(0));
    assert!(r.stderr.contains("N0830"), "{}", r.stderr);
    // A WebAssembly module: its host terminates TLS, and the runtime has no TLS part to give it.
    let r = run_bounded(
        Command::new(NAZM).current_dir(d.path()).args([
            "build",
            "--target",
            "wasm32-wasip1",
            "--objects",
            "wobjs",
            "prog.nz",
        ]),
        "nazm build",
    );
    assert_ne!(r.code, Some(0));
    assert!(r.stderr.contains("TLS"), "{}", r.stderr);
}
