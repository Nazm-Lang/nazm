//! v1 track C, C2: `@std/http`, HTTP/1.1 over TCP.
//!
//! `docs/spec.md`, *HTTP*. What the programs show, on the interpreter and every backend: a corpus
//! of tricky requests and responses, each parsed or refused with the status RFC 9112 asks for and
//! none crashing; seeded random damage to valid requests refused, never a trap; the writers'
//! framing; URLs; the router's parameters, wildcard, 404 and 405 with `Allow`; and a server and a
//! client talking over loopback — keep-alive, pipelining, chunked bodies, `100-continue`, a head
//! deadline, malformed requests answered on the wire, connections served in tasks of their own.
//!
//! The pure programs go through `agrees`: identical output and identical memory reports. The
//! networked ones compare output and require everything reclaimed on every tier, but not equal
//! allocation counts: the interpreter and the compiled tiers have been seen to count a few strings
//! differently for one exchange over a socket (reported to the coordinator), and how a socket
//! splits what it delivers is the system's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

use differential::{agrees, everywhere, memory_line};

const PRE: &str = r#"use "@std/ioerror";
use "@std/net";
use "@std/http";
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }
// `|` is CRLF, `~` a bare LF, `^` a bare CR, a backtick a NUL byte.
fn wire(s: Str) -> Str {
    let parts = strs_new();
    let mut i = 0;
    while i < str_len(s) {
        let b = str_byte(s, i);
        if b == 124 { strs_push(parts, str_concat(str_from_byte(13), str_from_byte(10))); } else {
            if b == 126 { strs_push(parts, str_from_byte(10)); } else {
                if b == 94 { strs_push(parts, str_from_byte(13)); } else {
                    if b == 96 { strs_push(parts, str_from_byte(0)); } else { strs_push(parts, str_from_byte(b)); }
                }
            }
        }
        i = i + 1;
    }
    str_join(parts, "")
}
"#;

fn prog(body: &str) -> String {
    format!("{PRE}{body}")
}

/// Every tier prints `expected` and reclaims everything; allocation counts may differ (above).
fn agrees_networked(src: &str, expected: &str) {
    for (label, r) in everywhere(&[("prog.nz", src)]) {
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

const REQUESTS: &[(&str, &str)] = &[
    ("GET / HTTP/1.1|Host: a||", "ok GET / ? []"),
    ("GET /a/b?x=1&y HTTP/1.1|Host: a||", "ok GET /a/b ?x=1&y []"),
    ("||GET / HTTP/1.1|Host: a||", "ok GET / ? []"),
    ("GET / HTTP/1.0||", "ok GET / ? []"),
    ("GET / HTTP/1.1||", "400 an HTTP/1.1 request without Host"),
    (
        "GET / HTTP/1.1|Host: a|Host: b||",
        "400 more than one Host field",
    ),
    (
        "GET / HTTP/2.0|Host: a||",
        "505 a version other than HTTP/1.1 and HTTP/1.0",
    ),
    ("GET / HTTP/1.1x|Host: a||", "400 a malformed version"),
    ("GET / http/1.1|Host: a||", "400 a malformed version"),
    (
        "GET  / HTTP/1.1|Host: a||",
        "400 a request line without a target",
    ),
    (
        " GET / HTTP/1.1|Host: a||",
        "400 a request line without a method",
    ),
    (
        "G@T / HTTP/1.1|Host: a||",
        "400 a method that is not a token",
    ),
    ("GET /a b HTTP/1.1|Host: a||", "400 a malformed version"),
    ("GET / HTTP/1.1~Host: a~~", "400 a bare line feed"),
    ("GET / HTTP/1.1|Host: a^x||", "400 a bare carriage return"),
    (
        "GET / HTTP/1.1|Host : a||",
        "400 a header name that is not a token",
    ),
    (
        "GET / HTTP/1.1|Host: a| folded||",
        "400 a folded header line",
    ),
    (
        "GET / HTTP/1.1|Host: a|: b||",
        "400 a header line without a name and a colon",
    ),
    (
        "GET / HTTP/1.1|Host: a|X: a`b||",
        "400 a control byte in a header value",
    ),
    ("GET / HTTP/1.1|Host: a|X: a\tb  ||", "ok GET / ? []"),
    (
        "GET /\u{e9} HTTP/1.1|Host: a||",
        "400 a request target with a byte a URI cannot hold",
    ),
    (
        "GET example HTTP/1.1|Host: a||",
        "400 a request target in no form this server serves",
    ),
    ("GET http://h:8/x?y HTTP/1.1|Host: h:8||", "ok GET /x ?y []"),
    ("GET HTTPS://h HTTP/1.1|Host: h||", "ok GET / ? []"),
    ("OPTIONS * HTTP/1.1|Host: a||", "ok OPTIONS * ? []"),
    (
        "GET * HTTP/1.1|Host: a||",
        "400 the asterisk target outside OPTIONS",
    ),
    (
        "CONNECT h:443 HTTP/1.1|Host: h||",
        "501 CONNECT is not served",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 5||hello",
        "ok POST / ? [hello]",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 5, 5||hello",
        "ok POST / ? [hello]",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 5|Content-Length: 6||hello!",
        "400 Content-Length values that differ",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: +5||hello",
        "400 a Content-Length that is not a decimal count",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 0x5||hello",
        "400 a Content-Length that is not a decimal count",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 99999999999999999999||",
        "413 the body is too large",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 2000000||",
        "413 the body is too large",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 9||hello",
        "400 an incomplete body",
    ),
    (
        "POST / HTTP/1.1|Host: a|Content-Length: 2||hello",
        "400 bytes after the message",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked|Content-Length: 5||0||",
        "400 both Transfer-Encoding and Content-Length",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||5|hello|6;ext=1| world|0||",
        "ok POST / ? [hello world]",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: Chunked||3|abc|0|Trailer: t||",
        "ok POST / ? [abc]",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: gzip, chunked||0||",
        "501 a transfer coding other than chunked",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked, chunked||0||",
        "400 chunked more than once, or no coding",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: ,||0||",
        "400 chunked more than once, or no coding",
    ),
    (
        "POST / HTTP/1.0|Transfer-Encoding: chunked||0||",
        "400 Transfer-Encoding in an HTTP/1.0 message",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||zz|hi|0||",
        "400 a chunk size that is not hexadecimal",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||2x|hi|0||",
        "400 text after a chunk size",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||2;a`|hi|0||",
        "400 a control byte in a chunk extension",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||2|hix|0||",
        "400 a chunk not followed by its line end",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||ffffffffffffffffff|",
        "413 a chunk size too large",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||200000|",
        "413 the body is too large",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||2|hi|",
        "400 an incomplete body",
    ),
    (
        "POST / HTTP/1.1|Host: a|Transfer-Encoding: chunked||0|Bad Trailer: x||",
        "400 a malformed trailer field",
    ),
    ("GET / HTTP/1.1|Host: a|", "400 an incomplete head"),
    ("", "400 an incomplete head"),
];

#[test]
fn a_request_corpus_is_parsed_or_refused_alike_everywhere() {
    let mut calls = String::new();
    let mut expected = String::new();
    for (raw, want) in REQUESTS {
        let lit = raw
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\t', "\\t");
        calls.push_str(&format!("    case(out, o, \"{lit}\");\n"));
        expected.push_str(want);
        expected.push('\n');
    }
    // Limits: a long target, too many fields, too large a head.
    calls.push_str(
        "    case(out, o, str_concat(str_concat(\"GET /\", text_rep(\"a\", 8200)), \" HTTP/1.1|Host: a||\"));\n\
         \x20   case(out, o, str_concat(str_concat(\"GET / HTTP/1.1|Host: a|\", text_rep(\"X: y|\", 100)), \"|\"));\n\
         \x20   case(out, o, str_concat(str_concat(\"GET / HTTP/1.1|Host: a|X: \", text_rep(\"y\", 17000)), \"||\"));\n",
    );
    expected.push_str(
        "414 the target is too long\n431 too many header fields\n431 the head is too large\n0",
    );
    let src = prog(&format!(
        "fn text_rep(s: Str, n: Int) -> Str {{ let p = strs_new(); let mut i = 0; while i < n {{ strs_push(p, s); i = i + 1; }} str_join(p, \"\") }}\n\
         fn case(out: OutCap, o: ServeOptions, raw: Str) -> Int {{\n\
             line(out, match http_parse_request(wire(raw), o) {{\n\
                 Result.Ok(value: r) => str_concat(str_concat(str_concat(\"ok \", r.method), str_concat(\" \", r.path)), str_concat(str_concat(\" ?\", r.query), str_concat(str_concat(\" [\", r.body), \"]\"))),\n\
                 Result.Err(error: e) => http_error_show(e),\n\
             }})\n\
         }}\n\
         fn main(out: OutCap) -> Int {{\n\
             let o = http_serve_options();\n\
         {calls}\
             0\n\
         }}\n"
    ));
    agrees(&src, &expected);
}

#[test]
fn damaged_requests_are_refused_and_never_trap() {
    // Seeded damage — a byte replaced by one the grammar cares about, removed or doubled — to
    // valid requests; every outcome is a value, tallied, and the tally is the same everywhere.
    let src = prog(
        "use \"@std/random\";\n\
         fn damage(rng: Rng, s: Str) -> Str {\n\
             let k = random_int(rng, 1, 4);\n\
             let mut t = s;\n\
             let mut j = 0;\n\
             while j < k && str_len(t) > 0 {\n\
                 let at = random_int(rng, 0, str_len(t));\n\
                 let how = random_int(rng, 0, 3);\n\
                 let picks = wire(\"|~^`: ;0aF\");\n\
                 let b = random_int(rng, 0, str_len(picks));\n\
                 let rep = str_slice(picks, b, b + 1);\n\
                 if how == 0 { t = str_concat(str_concat(str_slice(t, 0, at), rep), str_slice(t, at + 1, str_len(t))); } else {\n\
                     if how == 1 { t = str_concat(str_slice(t, 0, at), str_slice(t, at + 1, str_len(t))); } else { t = str_concat(str_concat(str_slice(t, 0, at), str_slice(t, at, at + 1)), str_slice(t, at, str_len(t))); }\n\
                 }\n\
                 j = j + 1;\n\
             }\n\
             t\n\
         }\n\
         fn main(out: OutCap) -> Int {\n\
             let rng = random_new(UInt64(20261016));\n\
             let seeds = strs_new();\n\
             strs_push(seeds, wire(\"GET /a?b=c HTTP/1.1|Host: h|Accept: */*||\"));\n\
             strs_push(seeds, wire(\"POST /p HTTP/1.1|Host: h|Content-Length: 5||hello\"));\n\
             strs_push(seeds, wire(\"POST /p HTTP/1.1|Host: h|Transfer-Encoding: chunked||5;x=y|hello|0|T: v||\"));\n\
             let o = http_serve_options();\n\
             let mut ok = 0;\n\
             let mut refused = 0;\n\
             let mut other = 0;\n\
             let mut n = 0;\n\
             while n < 1500 {\n\
                 let raw = damage(rng, strs_get(seeds, n % 3));\n\
                 match http_parse_request(raw, o) {\n\
                     Result.Ok(value: r) => { ok = ok + 1; },\n\
                     Result.Err(error: e) => { match e { HttpError.Protocol(status: s, why: w) => { if s >= 400 && s < 600 { refused = refused + 1; } else { other = other + 1; } }, HttpError.Io(error: x) => { other = other + 1; }, HttpError.Unsupported(why: w) => { other = other + 1; }, HttpError.Tls(error: t) => { other = other + 1; }, }; },\n\
                 };\n\
                 n = n + 1;\n\
             }\n\
             line(out, str_concat(str_concat(int_to_str(ok + refused), \" \"), int_to_str(other)));\n\
             line(out, if ok > 0 && refused > 0 { \"both\" } else { \"one-sided\" });\n\
             0\n\
         }\n",
    );
    agrees(&src, "1500 0\nboth\n0");
}

#[test]
fn responses_are_read_as_a_client_reads_them() {
    let src = prog(
        "fn case(out: OutCap, raw: Str, method: Str) -> Int {\n\
             line(out, match http_parse_response(wire(raw), method, http_client_options()) {\n\
                 Result.Ok(value: r) => str_concat(str_concat(int_to_str(r.status), \" [\"), str_concat(r.body, \"]\")),\n\
                 Result.Err(error: e) => http_error_show(e),\n\
             })\n\
         }\n\
         fn main(out: OutCap) -> Int {\n\
             case(out, \"HTTP/1.1 200 OK|Content-Length: 2||hi\", \"GET\");\n\
             case(out, \"HTTP/1.1 200 OK|Content-Length: 2||\", \"HEAD\");\n\
             case(out, \"HTTP/1.1 204 No Content|Content-Length: 9||\", \"GET\");\n\
             case(out, \"HTTP/1.1 200 OK|Transfer-Encoding: chunked||2|hi|1|!|0||\", \"GET\");\n\
             case(out, \"HTTP/1.0 200 OK||to the close\", \"GET\");\n\
             case(out, \"HTTP/1.1 200|Content-Length: 0||\", \"GET\");\n\
             case(out, \"HTTP/1.1 404 |Content-Length: 1||x\", \"GET\");\n\
             case(out, \"HTTP/2 200 OK||\", \"GET\");\n\
             case(out, \"HTTP/1.1 20 OK||\", \"GET\");\n\
             case(out, \"HTTP/1.1 099 Low||\", \"GET\");\n\
             case(out, \"HTTP/1.1 200 OK|Content-Length: 1|Content-Length: 2||xy\", \"GET\");\n\
             case(out, \"HTTP/1.1 200 OK|Transfer-Encoding: gzip, chunked||0||\", \"GET\");\n\
             case(out, \"HTTP/1.1 200 OK|Content-Length: 5||hi\", \"GET\");\n\
             0\n\
         }\n",
    );
    agrees(
        &src,
        "200 [hi]\n200 []\n204 []\n200 [hi!]\n200 [to the close]\n200 []\n404 [x]\n\
         502 a malformed status line\n502 a malformed status line\n502 a malformed status code\n\
         502 400 Content-Length values that differ\nunsupported: a transfer coding other than chunked\n\
         502 an incomplete body\n0",
    );
}

#[test]
fn messages_are_written_with_the_framing_the_writer_owns() {
    let src = prog(
        "fn show(out: OutCap, r: Result[Str, HttpError]) -> Int {\n\
             line(out, match r { Result.Ok(value: t) => text_vis(t), Result.Err(error: e) => http_error_show(e), })\n\
         }\n\
         fn text_vis(s: Str) -> Str {\n\
             let p = strs_new(); let mut i = 0;\n\
             while i < str_len(s) { let b = str_byte(s, i); if b == 13 { strs_push(p, \"\"); } else { if b == 10 { strs_push(p, \"|\"); } else { strs_push(p, str_from_byte(b)); } } i = i + 1; }\n\
             str_join(p, \"\")\n\
         }\n\
         fn main(out: OutCap) -> Int {\n\
             let r = http_with_header(http_with_header(http_text(200, \"hi\"), \"Content-Length\", \"99\"), \"X-A\", \"1\");\n\
             show(out, http_format_response(r, false, false));\n\
             show(out, http_format_response(r, true, true));\n\
             show(out, http_format_response(http_with_header(http_empty(204), \"x\", \"y\"), false, false));\n\
             show(out, http_format_response(http_with_header(http_text(200, \"\"), \"bad name\", \"v\"), false, false));\n\
             show(out, http_format_response(http_with_header(http_text(200, \"\"), \"n\", wire(\"a|b\")), false, false));\n\
             show(out, http_format_response(http_text(1000, \"\"), false, false));\n\
             show(out, http_format_request(http_request(\"POST\", \"/p?q\", \"\"), \"h:8\", true));\n\
             show(out, http_format_request(http_request_with_header(http_request(\"GET\", \"/\", \"\"), \"Host\", \"other\"), \"h\", false));\n\
             show(out, http_format_request(http_request(\"GE T\", \"/\", \"\"), \"h\", false));\n\
             show(out, http_format_request(http_request(\"GET\", \"/a b\", \"\"), \"h\", false));\n\
             0\n\
         }\n",
    );
    agrees(
        &src,
        "HTTP/1.1 200 OK|content-type: text/plain; charset=utf-8|x-a: 1|content-length: 2||hi\n\
         HTTP/1.1 200 OK|content-type: text/plain; charset=utf-8|x-a: 1|content-length: 2|connection: close||\n\
         HTTP/1.1 204 No Content|x: y||\n\
         500 an invalid header field\n500 an invalid header field\n500 a status outside 100 to 999\n\
         POST /p?q HTTP/1.1|host: h:8|content-length: 0|connection: close||\n\
         GET / HTTP/1.1|host: other||\n\
         400 a method that is not a token\n400 a target with a byte a URI cannot hold\n0",
    );
}

#[test]
fn headers_queries_and_urls_behave_at_their_edges() {
    let src = prog(
        "fn opt(o: Option[Str]) -> Str { match o { Option.Some(value: v) => str_concat(\"=\", v), Option.None() => \"none\", } }\n\
         fn url(out: OutCap, s: Str) -> Int {\n\
             line(out, match http_url_parse(s) { Result.Ok(value: u) => str_concat(str_concat(str_concat(u.scheme, \" \"), str_concat(u.host, \" \")), str_concat(str_concat(int_to_str(u.port), \" \"), u.target)), Result.Err(error: e) => http_error_show(e), })\n\
         }\n\
         fn main(out: OutCap) -> Int {\n\
             let h = http_header_add(http_header_add(http_header_add(http_headers(), \"Connection\", \" keep-alive, Close \"), \"X\", \"1\"), \"x\", \"2\");\n\
             line(out, int_to_str(http_header_count(h)));\n\
             line(out, str_concat(http_header_name_at(h, 0), str_concat(\":\", http_header_value_at(h, 0))));\n\
             line(out, str_join(http_header_all(h, \"X\"), \",\"));\n\
             line(out, opt(http_header_get(h, \"CONNECTION\")));\n\
             line(out, if http_header_has_token(h, \"connection\", \"close\") { \"close\" } else { \"open\" });\n\
             line(out, int_to_str(http_header_count(http_header_remove(h, \"x\"))));\n\
             line(out, str_concat(http_header_name_at(h, 9), opt(http_header_get(Headers(lines: \"broken\"), \"broken\"))));\n\
             let r = http_request(\"GET\", \"/s?a=1&b=x+y%21&c&a=2&d=%zz\", \"\");\n\
             line(out, str_join(strs_of(opt(http_query(r, \"a\")), opt(http_query(r, \"b\")), opt(http_query(r, \"c\"))), \" \"));\n\
             line(out, str_concat(opt(http_query(r, \"d\")), opt(http_query(r, \"e\"))));\n\
             line(out, str_concat(opt(http_percent_decode(\"a%2Fb%2\")), opt(http_percent_decode(\"%41%62\"))));\n\
             line(out, opt(http_param(Request(method: \"\", target: \"\", path: \"\", query: \"\", version: \"\", headers: http_headers(), body: \"\", params: \"9:x\", peer: \"\"), \"x\")));\n\
             url(out, \"http://example.org\");\n\
             url(out, \"HTTPS://example.org:8443/a?b\");\n\
             url(out, \"http://[::1]:8080?q\");\n\
             url(out, \"http://127.0.0.1:0/\");\n\
             url(out, \"http://u@h/\");\n\
             url(out, \"http://h/#f\");\n\
             url(out, \"ftp://h/\");\n\
             url(out, \"http://h:/\");\n\
             url(out, \"http:///x\");\n\
             url(out, \"http://[::1/\");\n\
             line(out, http_reason(431));\n\
             0\n\
         }\n\
         fn strs_of(a: Str, b: Str, c: Str) -> Strs { let s = strs_new(); strs_push(s, a); strs_push(s, b); strs_push(s, c); s }\n",
    );
    agrees(
        &src,
        "3\nconnection:keep-alive, Close\n1,2\n=keep-alive, Close\nclose\n1\nnone\n=1 =x y! =\nnonenone\n\
         none=Ab\nnone\nhttp example.org 80 /\nhttps example.org 8443 /a?b\nhttp [::1] 8080 /?q\n\
         400 a port outside 1 to 65535\n400 userinfo in a URL\n400 a fragment in a URL\n\
         400 a URL that is not http or https\n400 an empty port\n400 a URL without a host\n\
         400 an IPv6 host without its bracket\nRequest Header Fields Too Large\n0",
    );
}

#[test]
fn a_router_matches_parameters_wildcards_and_methods() {
    let src = prog(
        "fn hello(r: Request) -> Response ! {} {\n\
             http_text(200, str_concat(\"hello \", match http_param(r, \"name\") { Option.Some(value: n) => n, Option.None() => \"?\", }))\n\
         }\n\
         fn files(r: Request) -> Response ! {} {\n\
             http_text(200, str_concat(\"file \", match http_param(r, \"*\") { Option.Some(value: n) => n, Option.None() => \"?\", }))\n\
         }\n\
         fn first(r: Request) -> Response ! {} { http_text(201, \"first\") }\n\
         fn any(r: Request) -> Response ! {} { http_text(202, r.method) }\n\
         fn ask(out: OutCap, h: fn(Request) -> Response ! { io }, method: Str, target: Str) -> Int ! { io } {\n\
             let r = h(http_request(method, target, \"\"));\n\
             line(out, str_concat(str_concat(str_concat(int_to_str(r.status), \" \"), r.body), match http_header_get(r.headers, \"allow\") { Option.Some(value: a) => str_concat(\" allow=\", a), Option.None() => \"\", }))\n\
         }\n\
         fn main(out: OutCap) -> Int ! { io } {\n\
             let rt = http_router_get(http_router(), \"/hello/{name}\", hello);\n\
             let rt2 = http_router_post(rt, \"/hello/{name}\", hello);\n\
             let rt3 = http_route(http_route(http_router_get(rt2, \"/static/*\", files), \"PUT\", \"/x\", first), \"\", \"/any\", any);\n\
             let rt4 = http_router_get(http_router_get(rt3, \"/dup\", first), \"/dup\", hello);\n\
             let h = http_router_handler(rt4);\n\
             ask(out, h, \"GET\", \"/hello/w%C3%A9rld?x=1\");\n\
             ask(out, h, \"HEAD\", \"/hello/a\");\n\
             ask(out, h, \"POST\", \"/hello/b\");\n\
             ask(out, h, \"DELETE\", \"/hello/c\");\n\
             ask(out, h, \"GET\", \"/hello/\");\n\
             ask(out, h, \"GET\", \"/hello/a/b\");\n\
             ask(out, h, \"GET\", \"/hello/%zz\");\n\
             ask(out, h, \"GET\", \"/static/css/site.css\");\n\
             ask(out, h, \"GET\", \"/static/\");\n\
             ask(out, h, \"GET\", \"/x\");\n\
             ask(out, h, \"PATCH\", \"/any\");\n\
             ask(out, h, \"GET\", \"/dup\");\n\
             ask(out, h, \"GET\", \"/nowhere\");\n\
             ask(out, h, \"GET\", \"*\");\n\
             0\n\
         }\n",
    );
    agrees(
        &src,
        "200 hello w\u{e9}rld\n200 hello a\n200 hello b\n405 method not allowed\n allow=GET, POST\n\
         404 not found\n\n404 not found\n\n404 not found\n\n200 file css/site.css\n200 file \n\
         405 method not allowed\n allow=PUT\n202 PATCH\n201 first\n404 not found\n\n404 not found\n\n0",
    );
}

/// The handler every server test routes to.
const SERVICE: &str = "fn hello(r: Request) -> Response ! {} {\n\
         http_text(200, str_concat(str_concat(r.method, \" \"), str_concat(str_concat(match http_param(r, \"name\") { Option.Some(value: n) => n, Option.None() => \"?\", }, \" \"), r.body)))\n\
     }\n\
     fn routes() -> fn(Request) -> Response ! { io } ! {} {\n\
         http_router_handler(http_router_post(http_router_get(http_router(), \"/hello/{name}\", hello), \"/echo/{name}\", hello))\n\
     }\n\
     fn shown(r: Result[Response, HttpError]) -> Str {\n\
         match r {\n\
             Result.Ok(value: x) => str_concat(str_concat(int_to_str(x.status), \" \"), x.body),\n\
             Result.Err(error: e) => str_concat(\"ERR \", http_error_show(e)),\n\
         }\n\
     }\n\
     fn quick() -> ServeOptions {\n\
         let d = http_serve_options();\n\
         ServeOptions(max_head: d.max_head, max_headers: d.max_headers, max_target: d.max_target, max_body: d.max_body, head_timeout_ms: 300, idle_timeout_ms: 2000, body_timeout_ms: 300, write_timeout_ms: 2000, max_requests: d.max_requests, max_connections: d.max_connections)\n\
     }\n";

#[test]
fn a_server_and_a_client_talk_over_loopback_alike_everywhere() {
    let src = prog(&format!(
        "{SERVICE}\
         fn client(net: NetCap, s: HttpServer, lines: Chan[Str]) -> Int ! {{ io }} {{\n\
             let base = str_concat(\"http://\", http_server_addr(s));\n\
             let o = http_client_options();\n\
             chan_send_of(lines, shown(http_get(net, str_concat(base, \"/hello/world\"), o)));\n\
             chan_send_of(lines, shown(http_fetch(net, \"POST\", str_concat(base, \"/echo/x\"), \"payload\", o)));\n\
             chan_send_of(lines, shown(http_get(net, str_concat(base, \"/nowhere\"), o)));\n\
             chan_send_of(lines, shown(http_fetch(net, \"DELETE\", str_concat(base, \"/hello/a\"), \"\", o)));\n\
             chan_send_of(lines, shown(http_get(net, \"https://127.0.0.1:1/\", o)));\n\
             match http_connect(net, base, o) {{\n\
                 Result.Ok(value: c) => {{\n\
                     chan_send_of(lines, shown(http_send(c, http_request(\"GET\", \"/hello/one\", \"\"))));\n\
                     chan_send_of(lines, shown(http_send(c, http_request(\"POST\", \"/echo/two\", \"2\"))));\n\
                     chan_send_of(lines, shown(http_send(c, http_request(\"HEAD\", \"/hello/three\", \"\"))));\n\
                     chan_send_of(lines, shown(http_send(c, http_request(\"GET\", \"/hello/four\", \"\"))));\n\
                     http_close(c);\n\
                 }},\n\
                 Result.Err(error: e) => {{ chan_send_of(lines, http_error_show(e)); }},\n\
             }};\n\
             http_drain(s);\n\
             chan_close_of(lines)\n\
         }}\n\
         fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int ! {{ io, spawn }} {{\n\
             match http_listen(net, \"127.0.0.1:0\", quick()) {{\n\
                 Result.Ok(value: s) => {{\n\
                     let lines = chan_new_of[Str](64);\n\
                     let h = routes();\n\
                     let mut stats = \"\";\n\
                     scope {{\n\
                         spawn client(net, s, lines);\n\
                         match http_serve(t, s, h) {{\n\
                             Result.Ok(value: st) => {{ stats = str_concat(str_concat(int_to_str(st.requests), \" requests on \"), int_to_str(st.connections)); }},\n\
                             Result.Err(error: e) => {{ stats = io_error_show(e); }},\n\
                         }};\n\
                     }}\n\
                     let got = vec_new[Str]();\n\
                     while chan_recv_of(lines, got) {{ line(out, vec_pop(got)); }}\n\
                     line(out, stats);\n\
                     line(out, if http_draining(s) {{ \"drained\" }} else {{ \"still open\" }});\n\
                 }},\n\
                 Result.Err(error: e) => {{ line(out, io_error_show(e)); }},\n\
             }};\n\
             0\n\
         }}\n"
    ));
    agrees_networked(
        &src,
        "200 GET world \n200 POST x payload\n404 not found\n\n405 method not allowed\n\n\
         ERR 127.0.0.1:1: Connection refused (ConnectionRefused)\n200 GET one \n200 POST two 2\n200 \n200 GET four \n\
         8 requests on 5\ndrained\n0",
    );
}

#[test]
fn malformed_requests_are_answered_on_the_wire_and_the_server_keeps_serving() {
    // Raw clients: what each request is answered with, the status line and how the connection
    // ended; pipelining; 100-continue; a head that stops arriving; a connection asking to close.
    let src = prog(&format!(
        "{SERVICE}\
         fn raw(net: NetCap, addr: Str, text: Str) -> Str ! {{ io }} {{\n\
             match tcp_connect(net, addr, 2000) {{\n\
                 Result.Ok(value: c) => {{\n\
                     tcp_set_timeout(c, 3000);\n\
                     tcp_write_str(c, wire(text));\n\
                     let mut all = \"\";\n\
                     let buf = bytes_new(4096);\n\
                     let mut going = true;\n\
                     while going {{\n\
                         match tcp_read(c, buf) {{\n\
                             Result.Ok(value: n) => {{ if n == 0 {{ going = false; }} else {{ all = str_concat(all, bytes_to_str(bytes_slice(buf, 0, n))); }} }},\n\
                             Result.Err(error: e) => {{ all = str_concat(all, str_concat(\" [\", str_concat(io_kind_name(e.kind), \"]\"))); going = false; }},\n\
                         }};\n\
                     }}\n\
                     tcp_close(c);\n\
                     statuses(all)\n\
                 }},\n\
                 Result.Err(error: e) => io_error_show(e),\n\
             }}\n\
         }}\n\
         // Every status line in what came back, and the last body.\n\
         fn statuses(all: Str) -> Str {{\n\
             let p = strs_new();\n\
             let mut i = 0;\n\
             while i + 12 <= str_len(all) {{\n\
                 if str_slice(all, i, i + 9) == \"HTTP/1.1 \" && str_byte(all, i + 9) >= 48 && str_byte(all, i + 9) <= 57 {{ strs_push(p, str_slice(all, i + 9, i + 12)); }}\n\
                 i = i + 1;\n\
             }}\n\
             str_join(p, \",\")\n\
         }}\n\
         fn continue_then_body(net: NetCap, addr: Str) -> Str ! {{ io }} {{\n\
             match tcp_connect(net, addr, 2000) {{\n\
                 Result.Ok(value: c) => {{\n\
                     tcp_set_timeout(c, 3000);\n\
                     tcp_write_str(c, wire(\"POST /echo/e HTTP/1.1|Host: h|Expect: 100-continue|Content-Length: 4|Connection: close||\"));\n\
                     let buf = bytes_new(25);\n\
                     let first = match tcp_read_exact(c, buf) {{ Result.Ok(value: n) => statuses(bytes_to_str(buf)), Result.Err(error: e) => io_kind_name(e.kind), }};\n\
                     tcp_write_str(c, \"body\");\n\
                     let rest = bytes_new(4096);\n\
                     let mut all = \"\";\n\
                     let mut going = true;\n\
                     while going {{ match tcp_read(c, rest) {{ Result.Ok(value: n) => {{ if n == 0 {{ going = false; }} else {{ all = str_concat(all, bytes_to_str(bytes_slice(rest, 0, n))); }} }}, Result.Err(error: e) => {{ going = false; }}, }}; }}\n\
                     tcp_close(c);\n\
                     str_concat(str_concat(first, \" then \"), str_concat(statuses(all), if str_len(all) >= 11 && str_slice(all, str_len(all) - 11, str_len(all)) == \"POST e body\" {{ \" POST e body\" }} else {{ \" ?\" }}))\n\
                 }},\n\
                 Result.Err(error: e) => io_error_show(e),\n\
             }}\n\
         }}\n\
         fn client(net: NetCap, s: HttpServer, lines: Chan[Str], tm: TimeCap) -> Int ! {{ io }} {{\n\
             let a = http_server_addr(s);\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1|Host: h|Connection: close||\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1||\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1~Host: h~~\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/3.0|Host: h||\"));\n\
             chan_send_of(lines, raw(net, a, \"POST /echo/a HTTP/1.1|Host: h|Transfer-Encoding: chunked|Content-Length: 3||abc\"));\n\
             chan_send_of(lines, raw(net, a, \"POST /echo/a HTTP/1.1|Host: h|Transfer-Encoding: chunked||zz|\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1|Host: h|Expect: magic||\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1|Host: h\"));\n\
             chan_send_of(lines, raw(net, a, \"POST /echo/a HTTP/1.1|Host: h|Content-Length: 10||abc\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/1 HTTP/1.1|Host: h||POST /echo/2 HTTP/1.1|Host: h|Transfer-Encoding: chunked||1|x|0||GET /hello/3 HTTP/1.0||\"));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/1 HTTP/1.1|Host: h||GET /nowhere HTTP/1.1|Host: h|Connection: close||GET /hello/3 HTTP/1.1|Host: h||\"));\n\
             chan_send_of(lines, continue_then_body(net, a));\n\
             chan_send_of(lines, raw(net, a, \"GET /hello/a HTTP/1.1|Host: h|Connection: close||\"));\n\
             http_drain(s);\n\
             chan_close_of(lines)\n\
         }}\n\
         fn main(net: NetCap, t: SpawnCap, tm: TimeCap, out: OutCap) -> Int ! {{ io, spawn }} {{\n\
             match http_listen(net, \"127.0.0.1:0\", quick()) {{\n\
                 Result.Ok(value: s) => {{\n\
                     let lines = chan_new_of[Str](64);\n\
                     let h = routes();\n\
                     let mut stats = \"\";\n\
                     scope {{\n\
                         spawn client(net, s, lines, tm);\n\
                         match http_serve(t, s, h) {{\n\
                             Result.Ok(value: st) => {{ stats = str_concat(str_concat(int_to_str(st.requests), \" requests on \"), int_to_str(st.connections)); }},\n\
                             Result.Err(error: e) => {{ stats = io_error_show(e); }},\n\
                         }};\n\
                     }}\n\
                     let got = vec_new[Str]();\n\
                     while chan_recv_of(lines, got) {{ line(out, vec_pop(got)); }}\n\
                     line(out, stats);\n\
                 }},\n\
                 Result.Err(error: e) => {{ line(out, io_error_show(e)); }},\n\
             }};\n\
             0\n\
         }}\n"
    ));
    agrees_networked(
        &src,
        "200\n400\n400\n505\n400\n400\n417\n408\n408\n200,200,200\n200,404\n100 then 200 POST e body\n200\n\
         8 requests on 13\n0",
    );
}

#[test]
fn connections_served_in_tasks_of_their_own_answer_concurrent_clients() {
    // The parallel form: the program's own accept loop spawns a task per connection, and each
    // task names its handler. Four clients at once, each with three keep-alive requests.
    let src = prog(&format!(
        "{SERVICE}\
         fn conn(s: HttpServer, c: HttpConn) -> Int ! {{ io }} {{\n\
             http_serve_conn(s, c, routes())\n\
         }}\n\
         fn accept_all(t: SpawnCap, s: HttpServer) -> Int ! {{ io, spawn }} {{\n\
             let mut running = true;\n\
             scope {{\n\
                 while running {{\n\
                     match http_accept(s) {{\n\
                         Result.Ok(value: c) => {{ spawn conn(s, c); }},\n\
                         Result.Err(error: e) => {{ running = false; }},\n\
                     }};\n\
                 }}\n\
             }}\n\
             0\n\
         }}\n\
         fn client(net: NetCap, base: Str, k: Int, done: Chan[Str]) -> Int ! {{ io }} {{\n\
             let mut ok = 0;\n\
             match http_connect(net, base, http_client_options()) {{\n\
                 Result.Ok(value: c) => {{\n\
                     let mut i = 0;\n\
                     while i < 3 {{\n\
                         let want = str_concat(str_concat(\"POST c\", int_to_str(k)), str_concat(\" \", int_to_str(i)));\n\
                         if shown(http_send(c, http_request(\"POST\", str_concat(\"/echo/c\", int_to_str(k)), int_to_str(i)))) == str_concat(\"200 \", want) {{ ok = ok + 1; }}\n\
                         i = i + 1;\n\
                     }}\n\
                     http_close(c);\n\
                 }},\n\
                 Result.Err(error: e) => {{}},\n\
             }};\n\
             chan_send_of(done, int_to_str(ok));\n\
             0\n\
         }}\n\
         fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int ! {{ io, spawn }} {{\n\
             match http_listen(net, \"127.0.0.1:0\", quick()) {{\n\
                 Result.Ok(value: s) => {{\n\
                     let base = str_concat(\"http://\", http_server_addr(s));\n\
                     let done = chan_new_of[Str](8);\n\
                     let got = vec_new[Str]();\n\
                     let mut total = 0;\n\
                     scope {{\n\
                         spawn accept_all(t, s);\n\
                         scope {{\n\
                             spawn client(net, base, 1, done);\n\
                             spawn client(net, base, 2, done);\n\
                             spawn client(net, base, 3, done);\n\
                             spawn client(net, base, 4, done);\n\
                         }}\n\
                         let mut k = 0;\n\
                         while k < 4 {{ if chan_recv_of(done, got) {{ if vec_pop(got) == \"3\" {{ total = total + 1; }} }} k = k + 1; }}\n\
                         http_drain(s);\n\
                     }}\n\
                     line(out, int_to_str(total));\n\
                     line(out, str_concat(int_to_str(atomic_load(s.served)), str_concat(\" on \", int_to_str(atomic_load(s.accepted)))));\n\
                 }},\n\
                 Result.Err(error: e) => {{ line(out, io_error_show(e)); }},\n\
             }};\n\
             0\n\
         }}\n"
    ));
    agrees_networked(&src, "4\n12 on 4\n0");
}
