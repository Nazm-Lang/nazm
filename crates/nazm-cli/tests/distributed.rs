//! Gate 3: the distributed foundation (`library/std/dist.nz`, `examples/apps/ledger/`,
//! `docs/internal/systems-domains.md` Part N).
//!
//! In one process, the `Local` transport: an answer lost, a timeout through `select`, a retry with
//! the same idempotency key, a frame decoded and a torn one refused — the same under the
//! interpreter and natively. Between two processes, over TCP: a ledger server that stops the moment
//! a credit is logged and before it answers, restarted on the same address, and a client whose
//! retries carry it through — every credit applied exactly once. No consensus, replication or
//! partition tolerance is claimed: two nodes, one of them restarted.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn manifest(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

#[test]
fn in_one_process_a_lost_answer_times_out_and_a_retry_is_applied_once() {
    let d = tempfile::tempdir().unwrap();
    let src = manifest("tests/distributed/local.nz");
    let want =
        "1 -> 3\n2 -> 6\n3 -> 9\nagain 2 -> 6\na hello\na torn frame is refused\nframe bytes 28\n";
    let r = nazm(d.path(), &["run", "--interpret", src.to_str().unwrap()]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(r.stdout, format!("{want}0\n"));
    let exe = d.path().join(crate::common::exe_name("local"));
    let r = nazm(
        d.path(),
        &["build", src.to_str().unwrap(), "-o", exe.to_str().unwrap()],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let r = run_bounded(&mut Command::new(&exe), "local");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(r.stdout, format!("{want}0\n"), "natively, the same");
}

/// A port nothing listens on now.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `child` waited for at most `limit`, then killed.
fn wait(child: &mut std::process::Child, limit: Duration) -> Option<i32> {
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_client_retries_through_a_server_restart_and_every_credit_is_applied_once() {
    let d = tempfile::tempdir().unwrap();
    let exe = d.path().join(crate::common::exe_name("ledger"));
    let src = manifest("../../examples/apps/ledger/ledger.nz");
    let r = nazm(
        d.path(),
        &["build", src.to_str().unwrap(), "-o", exe.to_str().unwrap()],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let addr = format!("127.0.0.1:{}", free_port());
    let log = d.path().join("log.txt");
    // The first server stops the moment credit 5 is in its log, before it answers.
    let mut first = Command::new(&exe)
        .args(["serve", &addr, log.to_str().unwrap(), "die-after", "5"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // It is listening once it says so.
    let mut line = [0u8; 9];
    first
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut line)
        .unwrap();
    assert_eq!(&line, b"listening");
    let mut client = Command::new(&exe)
        .args(["client", &addr, "8"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(
        wait(&mut first, Duration::from_secs(30)),
        Some(9),
        "the first server stopped"
    );
    let mut why = String::new();
    first
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut why)
        .unwrap();
    assert!(why.contains("stopping after logging key 5"), "{why}");
    // Restarted on the same address, it replays its log: credits 1 to 5, 150.
    let mut second = Command::new(&exe)
        .args(["serve", &addr, log.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let code = wait(&mut client, Duration::from_secs(60));
    let _ = second.kill();
    let _ = second.wait();
    let mut said = String::new();
    second
        .stdout
        .as_mut()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    assert!(said.starts_with("listening, balance 150\n"), "{said}");
    let mut out = String::new();
    client
        .stdout
        .as_mut()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert_eq!(code, Some(0), "{out}");
    // Each credit's balance as of it: credit 5's answer is the log's, 150, not a second 50 on top.
    let want: String = (1..=8)
        .map(|k: i64| format!("{k} -> {}\n", (1..=k).map(|j| j * 10).sum::<i64>()))
        .collect();
    assert_eq!(out, format!("{want}balance 360\n0\n"));
    // And the log holds each key once.
    let logged = std::fs::read_to_string(&log).unwrap();
    let keys: Vec<&str> = logged
        .lines()
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(keys, ["1", "2", "3", "4", "5", "6", "7", "8"]);
}
