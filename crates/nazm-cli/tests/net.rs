//! Gate 2: networking.
//!
//! `docs/general-purpose.md` §9 and §10 and the spec's *Networking*. What only a whole run shows:
//! that `@std/net` listens, accepts, connects, reads and writes TCP and sends and receives UDP over
//! loopback; that refusals, deadlines, an address in use and a malformed address are values of the
//! kind they are; that a close wakes a task waiting on the socket; that a peer that has gone is an
//! error and never a signal; and that a task waiting on a socket does not hold its worker — eight
//! tasks parked in the network on **one** worker while a ninth computes — in the interpreter and
//! in both native backends at both optimisation levels.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod differential;

use differential::{agrees, codes, nazm};

const LINE: &str = "fn line(out: OutCap, s: Str) -> Int { print(str_concat(s, \"\\n\")) }\n";

fn prog(body: &str) -> String {
    format!("use \"@std/ioerror\";\nuse \"@std/net\";\n{LINE}{body}")
}

#[test]
fn an_echo_server_answers_clients_alike_everywhere() {
    let src = prog(
        "fn serve(l: TcpListener, n: Int) -> Int ! { io } {\n\
             let mut k = 0;\n\
             while k < n {\n\
                 match tcp_accept(l) {\n\
                     Result.Ok(value: c) => {\n\
                         let buf = bytes_new(5);\n\
                         tcp_read_exact(c, buf);\n\
                         tcp_write(c, buf);\n\
                         tcp_close(c);\n\
                     },\n\
                     Result.Err(error: e) => {},\n\
                 };\n\
                 k = k + 1;\n\
             }\n\
             0\n\
         }\n\
         fn ask(net: NetCap, addr: Str, word: Str) -> Str ! { io } {\n\
             match tcp_connect(net, addr, 2000) {\n\
                 Result.Ok(value: c) => {\n\
                     tcp_write_str(c, word);\n\
                     let buf = bytes_new(5);\n\
                     let got = match tcp_read_exact(c, buf) { Result.Ok(value: n) => bytes_to_str(buf), Result.Err(error: e) => io_kind_name(e.kind), };\n\
                     let peer = tcp_peer_addr(c);\n\
                     tcp_close(c);\n\
                     if peer == addr { got } else { \"wrong peer\" }\n\
                 },\n\
                 Result.Err(error: e) => io_kind_name(e.kind),\n\
             }\n\
         }\n\
         fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int {\n\
             match tcp_listen(net, \"127.0.0.1:0\") {\n\
                 Result.Ok(value: l) => {\n\
                     let addr = tcp_listener_addr(l);\n\
                     line(out, if str_len(addr) > 10 && str_slice(addr, 0, 10) == \"127.0.0.1:\" { \"listening\" } else { addr });\n\
                     scope {\n\
                         spawn serve(l, 3);\n\
                         line(out, ask(net, addr, \"alpha\"));\n\
                         line(out, ask(net, addr, \"bravo\"));\n\
                         line(out, ask(net, addr, \"delta\"));\n\
                     }\n\
                     tcp_listener_close(l);\n\
                 },\n\
                 Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
             };\n\
             0\n\
         }\n",
    );
    agrees(&src, "listening\nalpha\nbravo\ndelta\n0");
}

#[test]
fn a_datagram_goes_and_comes_back_with_its_sender() {
    let src = prog(
        "fn main(net: NetCap, out: OutCap) -> Int {\n\
             match udp_bind(net, \"127.0.0.1:0\") {\n\
                 Result.Ok(value: u) => {\n\
                     let me = udp_local_addr(u);\n\
                     line(out, match udp_send_to(net, u, bytes_from_str(\"ping!\"), me) { Result.Ok(value: n) => int_to_str(n), Result.Err(error: e) => io_kind_name(e.kind), });\n\
                     let buf = bytes_new(4);\n\
                     match udp_recv_from(u, buf) {\n\
                         Result.Ok(value: d) => {\n\
                             line(out, int_to_str(d.size));\n\
                             line(out, bytes_to_str(buf));\n\
                             line(out, if d.from == me { \"from itself\" } else { d.from });\n\
                         },\n\
                         Result.Err(error: e) => { line(out, io_kind_name(e.kind)); },\n\
                     };\n\
                     udp_set_timeout(u, 50);\n\
                     line(out, match udp_recv_from(u, buf) { Result.Ok(value: d) => \"got one\", Result.Err(error: e) => io_kind_name(e.kind), });\n\
                     udp_close(u);\n\
                 },\n\
                 Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
             };\n\
             0\n\
         }\n",
    );
    // A datagram longer than the buffer is cut to it.
    agrees(&src, "5\n4\nping\nfrom itself\nTimedOut\n0");
}

#[test]
fn failures_are_values_of_their_kind() {
    let src = prog(
        "fn kind_of_listen(net: NetCap, a: Str) -> Str ! { io } {\n\
             match tcp_listen(net, a) { Result.Ok(value: l) => \"listening\", Result.Err(error: e) => io_kind_name(e.kind), }\n\
         }\n\
         fn main(net: NetCap, out: OutCap) -> Int {\n\
             match tcp_listen(net, \"127.0.0.1:0\") {\n\
                 Result.Ok(value: l) => {\n\
                     let addr = tcp_listener_addr(l);\n\
                     line(out, kind_of_listen(net, addr));\n\
                     tcp_listener_set_timeout(l, 50);\n\
                     line(out, match tcp_accept(l) { Result.Ok(value: c) => \"accepted\", Result.Err(error: e) => io_kind_name(e.kind), });\n\
                     tcp_listener_close(l);\n\
                     line(out, match tcp_connect(net, addr, 2000) { Result.Ok(value: c) => \"connected\", Result.Err(error: e) => io_kind_name(e.kind), });\n\
                     line(out, match tcp_accept(l) { Result.Ok(value: c) => \"accepted\", Result.Err(error: e) => io_kind_name(e.kind), });\n\
                 },\n\
                 Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
             };\n\
             line(out, kind_of_listen(net, \"localhost:80\"));\n\
             line(out, kind_of_listen(net, \"127.0.0.1:65536\"));\n\
             line(out, kind_of_listen(net, \"[127.0.0.1]:80\"));\n\
             line(out, kind_of_listen(net, \"127.0.0.1\"));\n\
             line(out, match net_resolve(net, \"127.0.0.1\", 8080) { Result.Ok(value: all) => str_join(all, \",\"), Result.Err(error: e) => io_kind_name(e.kind), });\n\
             line(out, match net_resolve(net, \"localhost\", 7) { Result.Ok(value: all) => if strs_contains_any(all, \"127.0.0.1:7\") { \"localhost\" } else { str_join(all, \",\") }, Result.Err(error: e) => io_kind_name(e.kind), });\n\
             0\n\
         }\n\
         fn strs_contains_any(xs: Strs, x: Str) -> Bool {\n\
             let mut i = 0;\n\
             while i < strs_len(xs) { if strs_get(xs, i) == x { return true; } i = i + 1; }\n\
             false\n\
         }\n",
    );
    agrees(
        &src,
        "AddressInUse\nTimedOut\nConnectionRefused\nClosed\nInvalidInput\nInvalidInput\nInvalidInput\nInvalidInput\n127.0.0.1:8080\nlocalhost\n0",
    );
}

#[test]
fn a_close_wakes_a_task_waiting_on_the_socket() {
    let src = prog(
        "fn wait_for_data(c: TcpStream, ready: Chan, answer: Chan[Str]) -> Int ! { io } {\n\
             chan_send(ready, 1);\n\
             let buf = bytes_new(8);\n\
             chan_send_of(answer, match tcp_read(c, buf) { Result.Ok(value: n) => int_to_str(n), Result.Err(error: e) => io_kind_name(e.kind), });\n\
             0\n\
         }\n\
         fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int {\n\
             match tcp_listen(net, \"127.0.0.1:0\") {\n\
                 Result.Ok(value: l) => {\n\
                     match tcp_connect(net, tcp_listener_addr(l), 2000) {\n\
                         Result.Ok(value: c) => {\n\
                             let ready = chan_new(1);\n\
                             let answer = chan_new_of[Str](1);\n\
                             let r = ints_new();\n\
                             let v = vec_new[Str]();\n\
                             scope {\n\
                                 spawn wait_for_data(c, ready, answer);\n\
                                 chan_recv(ready, r);\n\
                                 tcp_close(c);\n\
                                 if chan_recv_of(answer, v) { line(out, vec_get(v, 0)); } else { line(out, \"no answer\"); };\n\
                             }\n\
                         },\n\
                         Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
                     };\n\
                 },\n\
                 Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
             };\n\
             0\n\
         }\n",
    );
    // Whether the close lands before the read parks or while it waits, the read answers `Closed`.
    agrees(&src, "Closed\n0");
}

#[test]
fn writing_to_a_peer_that_has_gone_is_an_error_and_never_a_signal() {
    let src = prog(
        "fn hang_up(l: TcpListener) -> Int ! { io } {\n\
             match tcp_accept(l) { Result.Ok(value: c) => { tcp_close(c); }, Result.Err(error: e) => {}, };\n\
             0\n\
         }\n\
         fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int {\n\
             match tcp_listen(net, \"127.0.0.1:0\") {\n\
                 Result.Ok(value: l) => {\n\
                     match tcp_connect(net, tcp_listener_addr(l), 2000) {\n\
                         Result.Ok(value: c) => {\n\
                             scope { spawn hang_up(l); }\n\
                             let data = bytes_from_str(\"0123456789abcdef\");\n\
                             let buf = bytes_new(1);\n\
                             tcp_set_timeout(c, 5);\n\
                             let mut tries = 0;\n\
                             let mut failed = false;\n\
                             while tries < 400 && !failed {\n\
                                 match tcp_write(c, data) { Result.Ok(value: n) => {}, Result.Err(error: e) => { failed = true; }, };\n\
                                 tcp_read(c, buf);\n\
                                 tries = tries + 1;\n\
                             }\n\
                             line(out, if failed { \"the write failed, and the program goes on\" } else { \"every write went\" });\n\
                         },\n\
                         Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
                     };\n\
                 },\n\
                 Result.Err(error: e) => { line(out, io_error_show(e)); },\n\
             };\n\
             0\n\
         }\n",
    );
    // How many writes go before the peer's reset arrives is the system's timing, so only what the
    // program says and how it ends are compared — a `SIGPIPE` would end it with no output at all.
    for (label, r) in differential::everywhere(&[("prog.nz", &src)]) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(
            r.stdout.trim_end(),
            "the write failed, and the program goes on\n0",
            "{label}"
        );
    }
}

/// The property §10 exists for: on the task pool, a task waiting on a socket releases its worker.
/// With one worker, eight tasks wait to accept and read while a ninth computes; if any wait held
/// the worker, the computation could never run and the program would not end.
const POOL: &str = "fn reader(l: OsHandle, done: Chan) -> Int {\n\
    let c = os_tcp_accept(l);\n\
    let buf = bytes_new(8);\n\
    let n = os_read(c, buf);\n\
    os_close(c);\n\
    chan_send(done, n);\n\
    0\n\
}\n\
fn busy(result: Chan) -> Int {\n\
    let mut i = 0;\n\
    let mut s = 0;\n\
    while i < 100000 { s = s + i % 7; i = i + 1; }\n\
    chan_send(result, s);\n\
    0\n\
}\n\
fn main(net: NetCap, t: SpawnCap, out: OutCap) -> Int {\n\
    let l = os_tcp_listen(\"127.0.0.1:0\");\n\
    let addr = os_local_addr(l);\n\
    let done = chan_new(16);\n\
    let result = chan_new(1);\n\
    let got = ints_new();\n\
    let one = ints_new();\n\
    scope {\n\
        let mut k = 0;\n\
        while k < 8 { spawn reader(l, done); k = k + 1; }\n\
        spawn busy(result);\n\
        chan_recv(result, got);\n\
        let mut j = 0;\n\
        while j < 8 { let c = os_tcp_connect(addr, 2000); os_write(c, bytes_from_str(\"x\")); j = j + 1; }\n\
        let mut m = 0;\n\
        while m < 8 { chan_recv(done, one); m = m + 1; }\n\
    }\n\
    print(str_concat(int_to_str(ints_get(got, 0)), str_concat(\" \", int_to_str(ints_len(one)))));\n\
    0\n\
}\n";

#[test]
fn a_task_waiting_on_a_socket_does_not_hold_its_worker() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), POOL).unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(r.stdout.trim_end(), "299995 80");
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path())
            .env("NAZM_WORKERS", "1")
            .env("NAZM_SCHEDULER_REPORT", "1");
        let ran = common::run_within(&mut c, std::time::Duration::from_secs(30))
            .unwrap_or_else(|| panic!("{backend}: a socket wait held the only worker"));
        assert_eq!(ran.code, Some(0), "{backend}: {}", ran.stderr);
        assert_eq!(ran.stdout.trim_end(), "299995 80", "{backend}");
    }
}

#[test]
fn the_network_is_its_own_authority() {
    // Naming an address needs a `NetCap`; an `IoCap` is files, not the network.
    let no_net =
        "fn main(io: IoCap) -> Int ! { io } { os_status(os_tcp_listen(\"127.0.0.1:0\")) }\n";
    assert!(
        codes(no_net).contains(&"N0369".to_owned()),
        "{:?}",
        codes(no_net)
    );
    let resolve =
        "fn main(io: IoCap) -> Int ! { io } { os_resolve(\"localhost\", 1, bytes_new(64)) }\n";
    assert!(
        codes(resolve).contains(&"N0369".to_owned()),
        "{:?}",
        codes(resolve)
    );
    // A socket already open is its own authority: a function handed one needs nothing else.
    let handed = "use \"@std/ioerror\";\nuse \"@std/net\";\n\
        fn reply(s: TcpStream) -> Int ! { io } { match tcp_write_str(s, \"hi\") { Result.Ok(value: n) => n, Result.Err(error: e) => 0 - 1, } }\n\
        fn main() -> Int { 0 }\n";
    assert!(codes(handed).is_empty(), "{:?}", codes(handed));
    // What a socket says is outside data, as a file's is: it may not name where a file is written.
    let sink = "fn main(net: NetCap, io: IoCap) -> Int ! { io } {\n\
        let u = os_udp_bind(\"127.0.0.1:0\");\n\
        let b = bytes_new(64);\n\
        let f = bytes_new(64);\n\
        os_udp_recv_from(u, b, f);\n\
        write_file(bytes_to_str(b), \"x\")\n\
    }\n";
    assert!(
        codes(sink).contains(&"N0372".to_owned()),
        "{:?}",
        codes(sink)
    );
}
