//! v1 track C, C4: what a long-running service needs besides HTTP.
//!
//! `docs/spec.md`, *Calendar time*, *Metrics*, *Configuration*, *Logging* and *HTTP*'s drain. What
//! the programs show, on the interpreter and every backend: calendar dates against known days in
//! both directions over ±2000 years, RFC 3339 and HTTP dates written and read with every refusal,
//! durations as text; counters, gauges and a histogram counted from several tasks and written as
//! Prometheus text, with every registration refused that should be; settings from a TOML file and
//! the environment into a derived record, each kind of bad value named; the request and trace
//! fields of a log record; and a drain that closes an idle connection at once, lets a request in
//! flight finish with `connection: close`, and closes one still running at its deadline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

use differential::{agrees, everywhere_with_env, memory_line};

/// Every tier prints `expected` and reclaims everything (allocation counts may differ for programs
/// that talk over sockets, as `http.rs` records).
fn agrees_reclaimed(files: &[(&str, &str)], env: &[(&str, &str)], expected: &str) {
    for (label, r) in everywhere_with_env(files, env) {
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

const CALENDAR: &str = r#"use "@std/time";
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }
fn shown(r: Result[DateTime, Str]) -> Str {
    match r { Result.Ok(value: dt) => time_format_rfc3339(dt), Result.Err(error: e) => str_concat("err ", e), }
}
fn dshown(r: Result[Duration, Str]) -> Str {
    match r { Result.Ok(value: d) => time_duration_show(d), Result.Err(error: e) => str_concat("err ", e), }
}
fn ymd(d: Date) -> Str { f"{d.year}-{d.month}-{d.day}" }
fn main(out: OutCap) -> Int ! { io } {
    // Known days, both ways.
    line(out, int_to_str(time_date_to_days(Date(year: 1970, month: 1, day: 1))));
    line(out, int_to_str(time_date_to_days(Date(year: 2000, month: 3, day: 1))));
    line(out, int_to_str(time_date_to_days(Date(year: 1600, month: 2, day: 29))));
    line(out, ymd(time_date_from_days(0 - 1)));
    line(out, ymd(time_date_from_days(20742)));
    line(out, ymd(time_date_from_days(0 - 719468)));
    line(out, ymd(time_date_add_days(Date(year: 2024, month: 2, day: 28), 1)));
    line(out, ymd(time_date_add_days(Date(year: 2023, month: 2, day: 28), 1)));
    // Every 997th day for ±2000 years goes there and back, each a valid date, and weekdays follow.
    let mut bad = 0;
    let mut z = 0 - 730000;
    let mut last = time_weekday(time_date_from_days(z - 997));
    while z < 730000 {
        let d = time_date_from_days(z);
        if time_date_to_days(d) != z { bad = bad + 1; }
        if d.day < 1 || d.day > time_days_in_month(d.year, d.month) { bad = bad + 1; }
        let w = time_weekday(d);
        if w != (last - 1 + 997) % 7 + 1 { bad = bad + 1; }
        last = w;
        z = z + 997;
    }
    line(out, str_concat("mismatches ", int_to_str(bad)));
    line(out, int_to_str(time_weekday(Date(year: 2026, month: 10, day: 16))));
    line(out, if time_is_leap_year(1900) { "1900 leap" } else { "1900 common" });
    line(out, if time_is_leap_year(2000) { "2000 leap" } else { "2000 common" });
    line(out, int_to_str(time_days_in_month(2025, 13)));
    line(out, match time_date(2025, 2, 29) { Option.Some(value: d) => "a date", Option.None() => "no such date", });
    // The wall clock and RFC 3339.
    line(out, time_format_rfc3339(time_utc(WallTime(unix_nanos: 0))));
    line(out, time_format_rfc3339(time_utc(WallTime(unix_nanos: 0 - 1))));
    line(out, time_format_rfc3339(time_utc(WallTime(unix_nanos: 1792143000120000000))));
    line(out, time_format_http(time_utc(WallTime(unix_nanos: 784111777000000000))));
    line(out, shown(time_parse_rfc3339("1985-04-12T23:20:50.52Z")));
    line(out, shown(time_parse_rfc3339("1996-12-19T16:39:57-08:00")));
    line(out, shown(time_parse_rfc3339("2024-02-29t00:00:00.000000001z")));
    line(out, shown(time_parse_rfc3339("1990-12-31T23:59:60Z")));
    line(out, shown(time_parse_rfc3339("2026-02-29T00:00:00Z")));
    line(out, shown(time_parse_rfc3339("2026-10-16T24:00:00Z")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00")));
    line(out, shown(time_parse_rfc3339("2026-10-16 09:30:00Z")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00.Z")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00.1234567890Z")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00+24:00")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00+0530")));
    line(out, shown(time_parse_rfc3339("2026-10-16T09:30:00Zx")));
    line(out, shown(time_parse_rfc3339("26-10-16T09:30:00Z")));
    line(out, shown(time_parse_rfc3339("")));
    match time_parse_rfc3339("1996-12-19T16:39:57-08:00") {
        Result.Ok(value: dt) => {
            line(out, time_format_rfc3339(time_to_utc(dt)));
            line(out, int_to_str(time_unix_seconds(dt)));
            line(out, time_format_http(dt));
            line(out, match time_to_wall(dt) { Option.Some(value: w) => int_to_str(w.unix_nanos), Option.None() => "out of range", });
        },
        Result.Err(error: e) => {},
    };
    line(out, match time_to_wall(time_from_unix(0 - 62135596800, 0)) { Option.Some(value: w) => "in range", Option.None() => "out of range", });
    // Durations.
    line(out, dshown(time_duration_parse("1h30m")));
    line(out, dshown(time_duration_parse("1500ms")));
    line(out, dshown(time_duration_parse("-2s3ns")));
    line(out, dshown(time_duration_parse("90s")));
    line(out, dshown(time_duration_parse("0")));
    line(out, dshown(time_duration_parse("10")));
    line(out, dshown(time_duration_parse("5d")));
    line(out, dshown(time_duration_parse("ms")));
    line(out, dshown(time_duration_parse("-")));
    line(out, dshown(time_duration_parse("99999999999999999999h")));
    line(out, dshown(time_duration_parse("2562048h")));
    line(out, time_duration_show(Duration(nanos: 0 - 9223372036854775807 - 1)));
    line(out, int_to_str(time_duration_to_ms(time_duration_secs(3))));
    0
}
"#;

#[test]
fn calendar_dates_rfc_3339_and_durations_agree_everywhere() {
    agrees(
        CALENDAR,
        "0\n11017\n-135081\n1969-12-31\n2026-10-16\n0-3-1\n2024-2-29\n2023-3-1\n\
         mismatches 0\n5\n1900 common\n2000 leap\n0\nno such date\n\
         1970-01-01T00:00:00Z\n1969-12-31T23:59:59.999999999Z\n2026-10-16T09:30:00.12Z\n\
         Sun, 06 Nov 1994 08:49:37 GMT\n1985-04-12T23:20:50.52Z\n1996-12-19T16:39:57-08:00\n\
         2024-02-29T00:00:00.000000001Z\nerr a leap second\nerr a date or time out of range\n\
         err a date or time out of range\nerr no offset, Z or +hh:mm\n\
         err no T between the date and the time\nerr a fraction without digits\n\
         err more than nine fractional digits\nerr an offset out of range\n\
         err an offset that is not +hh:mm\nerr text after the date-time\n\
         err not a full-date, YYYY-MM-DD\nerr not a full-date, YYYY-MM-DD\n\
         1996-12-20T00:39:57Z\n851042397\nFri, 20 Dec 1996 00:39:57 GMT\n851042397000000000\n\
         out of range\n\
         1h30m\n1s500ms\n-2s3ns\n1m30s\n0s\nerr a number without a unit\n\
         err a unit that is not h, m, s, ms, us or ns\nerr a unit without a number\n\
         err an empty duration\nerr a duration too long to hold\nerr a duration too long to hold\n\
         -2562047h47m16s854ms775us808ns\n3000\n0",
    );
}

const METRICS: &str = r#"use "@std/metrics";
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }
fn shown(r: Result[Int, Str]) -> Str {
    match r { Result.Ok(value: n) => int_to_str(n), Result.Err(error: e) => str_concat("err ", e), }
}
fn kv2(a: Str, b: Str) -> Strs { let s = strs_new(); strs_push(s, a); strs_push(s, b); s }
fn kv4(a: Str, b: Str, c: Str, d: Str) -> Strs { let s = kv2(a, b); strs_push(s, c); strs_push(s, d); s }
fn work(c: Counter, h: Histogram, k: Int) -> Int ! {} {
    let mut i = 0;
    while i < 100 { metrics_inc(c); metrics_observe(h, (i * k) % 300); i = i + 1; }
    0
}
fn bounds(xs: Str) -> Ints {
    let b = ints_new();
    let mut v = 0;
    let mut i = 0;
    while i < str_len(xs) {
        if str_byte(xs, i) == 44 { ints_push(b, v); v = 0; } else { v = v * 10 + str_byte(xs, i) - 48; }
        i = i + 1;
    }
    ints_push(b, v);
    b
}
fn main(t: SpawnCap, out: OutCap) -> Int ! { io, spawn } {
    let reg = metrics_registry();
    let get = metrics_counter_labelled("http_requests_total", "Requests answered.", kv4("method", "GET", "code", "200"));
    let post = metrics_counter_labelled("http_requests_total", "Requests answered.", kv4("method", "POST", "code", "200"));
    let g = metrics_gauge("in_flight", "Requests in flight,\nnow \\ here.");
    let h = metrics_histogram("latency_ms", "Latency.", bounds("10,100,250"));
    line(out, shown(metrics_register_counter(reg, get)));
    line(out, shown(metrics_register_gauge(reg, g)));
    line(out, shown(metrics_register_histogram(reg, h)));
    line(out, shown(metrics_register_counter(reg, post)));
    line(out, shown(metrics_register_counter(reg, get)));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge("latency_ms", ""))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge("9bad", ""))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge("a-b", ""))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge_labelled("ok", "", kv2("__x", "1")))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge_labelled("ok", "", kv2("le", "1")))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge_labelled("ok", "", kv4("a", "1", "a", "2")))));
    let odd = strs_new();
    strs_push(odd, "a");
    line(out, shown(metrics_register_gauge(reg, metrics_gauge_labelled("ok", "", odd))));
    line(out, shown(metrics_register_histogram(reg, metrics_histogram("h2", "", bounds("5,5")))));
    line(out, shown(metrics_register_histogram(reg, metrics_histogram("h3", "", bounds("1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17")))));
    line(out, shown(metrics_register_gauge(reg, metrics_gauge_labelled("ok", "", kv2("x", "a\"b\\c\nd")))));
    scope { spawn work(get, h, 1); spawn work(get, h, 3); spawn work(post, h, 7); }
    metrics_gauge_set(g, 5);
    metrics_gauge_add(g, 0 - 7);
    line(out, if metrics_add(post, 0 - 1) { "went down" } else { "a counter never goes down" });
    metrics_add(post, 2);
    line(out, f"{metrics_counter_value(get)} {metrics_counter_value(post)} {metrics_gauge_value(g)} {metrics_histogram_count(h)} {metrics_histogram_sum(h)}");
    line(out, metrics_content_type());
    print(metrics_render(reg));
    0
}
"#;

#[test]
fn metrics_count_across_tasks_and_are_written_as_prometheus_text() {
    // Three tasks count into the same cells; the histogram's buckets are cumulative, `+Inf` is the
    // count; each name is written once with its help and type, its samples together.
    agrees_reclaimed(
        &[("prog.nz", METRICS)],
        &[],
        "0\n1\n2\n3\n\
         err http_requests_total is already registered with these labels\n\
         err latency_ms is already a histogram\n\
         err a metric name that is not one: 9bad\nerr a metric name that is not one: a-b\n\
         err a label name that is not one: __x\nerr a label name that is not one: le\n\
         err a label given twice: a\nerr a label without a value\n\
         err bucket bounds that do not increase\nerr more than sixteen bucket bounds\n4\n\
         a counter never goes down\n200 102 -2 300 33150\n\
         text/plain; version=0.0.4; charset=utf-8\n\
         # HELP http_requests_total Requests answered.\n\
         # TYPE http_requests_total counter\n\
         http_requests_total{method=\"GET\",code=\"200\"} 200\n\
         http_requests_total{method=\"POST\",code=\"200\"} 102\n\
         # HELP in_flight Requests in flight,\\nnow \\\\ here.\n\
         # TYPE in_flight gauge\n\
         in_flight -2\n\
         # HELP latency_ms Latency.\n\
         # TYPE latency_ms histogram\n\
         latency_ms_bucket{le=\"10\"} 21\n\
         latency_ms_bucket{le=\"100\"} 178\n\
         latency_ms_bucket{le=\"250\"} 270\n\
         latency_ms_bucket{le=\"+Inf\"} 300\n\
         latency_ms_sum 33150\n\
         latency_ms_count 300\n\
         # TYPE ok gauge\n\
         ok{x=\"a\\\"b\\\\c\\nd\"} 0\n0",
    );
}

const CONFIG: &str = r#"use "@std/config";

@derive(json)
@codec(rename_all = "camelCase")
struct Settings { addr: Str, drain_ms: Int, verbose: Bool, ratio: Float64, name: Option[Str], level: Level, limits: Limits, }

@derive(json)
enum Level { Debug, Info, }

@derive(json)
struct Limits { body: Int, }

fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }

fn shown(r: Result[Settings, Str]) -> Str {
    match r {
        Result.Ok(value: s) => settings_to_json(s),
        Result.Err(error: e) => str_concat("err ", e),
    }
}

fn main(io: IoCap, out: OutCap) -> Int ! { io } {
    line(out, config_env_name("APP_", "displayName"));
    line(out, config_env_name("APP_", "drain-ms"));
    line(out, config_env_name("", "log.level2X"));
    let schema = settings_json_schema();
    line(out, shown(config_load(io, "service.toml", "C4A_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4B_", schema, settings_from_json)));
    line(out, shown(config_from_env(io, "C4A_", schema, settings_from_json)));
    line(out, shown(config_load(io, "missing.toml", "C4A_", schema, settings_from_json)));
    line(out, shown(config_load(io, "broken.toml", "C4A_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4C_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4D_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4E_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4F_", schema, settings_from_json)));
    line(out, shown(config_load(io, "service.toml", "C4G_", schema, settings_from_json)));
    line(out, match config_document(io, "", "C4A_", "[1]") { Result.Ok(value: t) => t, Result.Err(error: e) => str_concat("err ", e), });
    0
}
"#;

const SERVICE_TOML: &str = "addr = \"0.0.0.0:8080\"\ndrainMs = 1000\nverbose = false\nratio = 0.5\nlevel = \"Info\"\n\n[limits]\nbody = 4096\n";

#[test]
fn settings_come_from_a_file_and_the_environment_into_a_derived_record() {
    // The file, then each variable that is set over it; each kind of bad value named by its
    // variable; a member that is a record only from the file.
    agrees_reclaimed(
        &[
            ("prog.nz", CONFIG),
            ("service.toml", SERVICE_TOML),
            ("broken.toml", "addr = \n"),
        ],
        &[
            ("C4A_DRAIN_MS", "250"),
            ("C4A_VERBOSE", "true"),
            ("C4A_NAME", "svc"),
            ("C4A_RATIO", "1e-3"),
            ("C4A_LEVEL", "Debug"),
            ("C4C_RATIO", "abc"),
            ("C4D_LIMITS", "3"),
            ("C4E_DRAIN_MS", "12x"),
            ("C4F_VERBOSE", "yes"),
            ("C4G_LEVEL", "Loud"),
        ],
        "APP_DISPLAY_NAME\nAPP_DRAIN_MS\nLOG_LEVEL2_X\n\
         {\"addr\":\"0.0.0.0:8080\",\"drainMs\":250,\"verbose\":true,\"ratio\":0.001,\"name\":\"svc\",\"level\":\"Debug\",\"limits\":{\"body\":4096}}\n\
         {\"addr\":\"0.0.0.0:8080\",\"drainMs\":1000,\"verbose\":false,\"ratio\":0.5,\"name\":null,\"level\":\"Info\",\"limits\":{\"body\":4096}}\n\
         err $: no member `addr`\n\
         err missing.toml: No such file or directory (NotFound)\n\
         err broken.toml:1:8: expected a value\n\
         err C4C_RATIO: expected a number\n\
         err C4D_LIMITS: a member an environment variable cannot carry\n\
         err C4E_DRAIN_MS: expected an integer, not a digit\n\
         err C4F_VERBOSE: expected true or false\n\
         err $.level: `Loud` is not a variant of `Level` without a payload\n\
         err the schema is not a record's\n0",
    );
}

const LOG: &str = r#"use "@std/log";
use "@std/time";
fn main(out: OutCap) -> Int ! { io } {
    let l = log_with(log_new(LogLevel.Info()), log_trace("4bf92f3577b34da6a3ce929d0e0e4736", "00f067aa0ba902b7"));
    let f = log_fields();
    strs_push(f, log_request_id("r-17"));
    strs_push(f, log_time(WallTime(unix_nanos: 1792143000500000000)));
    print(log_line(l, LogLevel.Info(), "served", f));
    0
}
"#;

#[test]
fn a_log_record_carries_its_request_its_trace_and_its_time() {
    agrees(
        LOG,
        "{\"level\":\"info\",\"msg\":\"served\",\"trace_id\":\"4bf92f3577b34da6a3ce929d0e0e4736\",\
         \"span_id\":\"00f067aa0ba902b7\",\"request_id\":\"r-17\",\"time\":\"2026-10-16T09:30:00.5Z\"}0",
    );
}

const DRAIN: &str = r#"use "@std/ioerror";
use "@std/net";
use "@std/http";
fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, str_from_byte(10))) }
fn crlf() -> Str { str_concat(str_from_byte(13), str_from_byte(10)) }
fn quick() -> ServeOptions {
    let d = http_serve_options();
    ServeOptions(max_head: d.max_head, max_headers: d.max_headers, max_target: d.max_target, max_body: d.max_body, head_timeout_ms: 5000, idle_timeout_ms: 20000, body_timeout_ms: 5000, write_timeout_ms: 5000, max_requests: d.max_requests, max_connections: 16)
}
fn slow_ms(p: Str) -> Int {
    let mut v = 0;
    if str_len(p) > 6 && str_slice(p, 0, 6) == "/slow/" {
        let mut i = 6;
        while i < str_len(p) { v = v * 10 + str_byte(p, i) - 48; i = i + 1; }
    }
    v
}
fn routes(t: TimeCap) -> fn(Request) -> Response ! { io } {
    fn(r: Request) -> Response ! { io } {
        let ms = slow_ms(r.path);
        if ms > 0 { time_sleep_ms(ms); }
        http_text(200, "done")
    }
}
fn conn(t: TimeCap, s: HttpServer, c: HttpConn) -> Int ! { io } { http_serve_conn(s, c, routes(t)) }
fn accept_all(tasks: SpawnCap, t: TimeCap, s: HttpServer) -> Int ! { io, spawn } {
    let mut running = true;
    scope {
        while running {
            match http_accept(s) {
                Result.Ok(value: c) => { spawn conn(t, s, c); },
                Result.Err(error: e) => { running = false; },
            };
        }
    }
    0
}
fn count(s: Str, needle: Str) -> Int {
    let mut n = 0;
    let mut i = 0;
    while i + str_len(needle) <= str_len(s) { if str_slice(s, i, i + str_len(needle)) == needle { n = n + 1; } i = i + 1; }
    n
}
// Send `req`, read until the server closes, and say what came back.
fn talk(net: NetCap, t: TimeCap, addr: Str, path: Str, lines: Chan[Str], tag: Str) -> Int ! { io } {
    let t0 = time_now_ms();
    let req = str_concat(str_concat(str_concat("GET ", path), str_concat(" HTTP/1.1", crlf())), str_concat(str_concat("Host: h", crlf()), crlf()));
    match tcp_connect(net, addr, 2000) {
        Result.Ok(value: c) => {
            tcp_set_timeout(c, 10000);
            tcp_write_str(c, req);
            let mut all = "";
            let buf = bytes_new(4096);
            let mut going = true;
            while going {
                match tcp_read(c, buf) {
                    Result.Ok(value: n) => { if n == 0 { going = false; } else { all = str_concat(all, bytes_to_str(bytes_slice(buf, 0, n))); } },
                    Result.Err(error: e) => { all = str_concat(all, str_concat(" [", str_concat(io_kind_name(e.kind), "]"))); going = false; },
                };
            }
            tcp_close(c);
            let quick = if time_now_ms() - t0 < 2000 { "ended at once" } else { "ended late" };
            chan_send_of(lines, f"{tag}: answered {count(all, "HTTP/1.1 200")}, closing {count(all, "connection: close")}, {quick}");
            0
        },
        Result.Err(error: e) => { chan_send_of(lines, io_error_show(e)); 0 },
    }
}
fn main(net: NetCap, tasks: SpawnCap, t: TimeCap, out: OutCap) -> Int ! { io, spawn } {
    match http_listen(net, "127.0.0.1:0", quick()) {
        Result.Ok(value: s) => {
            let a = http_server_addr(s);
            let lines = chan_new_of[Str](8);
            let mut forced = 0 - 1;
            let mut took = 0;
            let mut warm = 0;
            scope {
                spawn accept_all(tasks, t, s);
                // More connections, one after another, than the registry has room for unless
                // the ended ones are dropped.
                let mut k = 0;
                while k < 40 {
                    match http_get(net, str_concat(str_concat("http://", a), "/warm"), http_client_options()) {
                        Result.Ok(value: r) => { if r.status == 200 { warm = warm + 1; } },
                        Result.Err(error: e) => {},
                    };
                    k = k + 1;
                }
                spawn talk(net, t, a, "/now", lines, "idle after one answer");
                spawn talk(net, t, a, "/slow/300", lines, "in flight");
                spawn talk(net, t, a, "/slow/2500", lines, "past the deadline");
                time_sleep_ms(150);
                let d0 = time_now_ms();
                forced = http_drain_within(t, s, 800);
                took = time_now_ms() - d0;
            }
            chan_close_of(lines);
            let got = vec_new[Str]();
            let all = strs_new();
            while chan_recv_of(lines, got) { strs_push(all, vec_pop(got)); }
            sort_strs(all);
            line(out, str_join(all, "\n"));
            line(out, f"warm-up answers: {warm}");
            line(out, f"closed at the deadline: {forced}");
            line(out, if took >= 700 && took < 1500 { "the drain kept its deadline" } else { int_to_str(took) });
            line(out, if http_draining(s) { "draining" } else { "serving" });
            line(out, f"open {atomic_load(s.open)}");
        },
        Result.Err(error: e) => { line(out, io_error_show(e)); },
    };
    0
}
fn sort_strs(v: Strs) -> Int {
    let mut i = 1;
    while i < strs_len(v) {
        let mut j = i;
        while j > 0 && str_lt(strs_get(v, j), strs_get(v, j - 1)) {
            let x = strs_get(v, j);
            strs_set(v, j, strs_get(v, j - 1));
            strs_set(v, j - 1, x);
            j = j - 1;
        }
        i = i + 1;
    }
    0
}
fn str_lt(a: Str, b: Str) -> Bool {
    let mut i = 0;
    while i < str_len(a) && i < str_len(b) {
        if str_byte(a, i) != str_byte(b, i) { return str_byte(a, i) < str_byte(b, i); }
        i = i + 1;
    }
    str_len(a) < str_len(b)
}
"#;

#[cfg_attr(
    windows,
    ignore = "N0830: a Windows build reaching TLS is refused until the SChannel binding has passed on a Windows host"
)]
#[test]
fn a_drain_closes_idle_connections_and_keeps_its_deadline() {
    // An idle keep-alive connection is closed at once; a request in flight is answered with
    // `connection: close`; one still running at the deadline is closed unanswered, and the drain
    // returns at its deadline though the handler runs on.
    agrees_reclaimed(
        &[("prog.nz", DRAIN)],
        &[],
        "idle after one answer: answered 1, closing 0, ended at once\n\
         in flight: answered 1, closing 1, ended at once\n\
         past the deadline: answered 0, closing 0, ended at once\n\
         warm-up answers: 40\n\
         closed at the deadline: 1\n\
         the drain kept its deadline\n\
         draining\n\
         open 0\n0",
    );
}
