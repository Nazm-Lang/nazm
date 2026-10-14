//! N69: contracts — the `web3` profile, a contract's facts, and its simulator.
//!
//! `docs/architecture.md` §7.71. What the command shows: a contract's state, entrypoints and read
//! and write sets as `nazm.contract/1`, conservative where the analysis cannot see; transactions
//! run deterministically, a revert leaving the state as it was, a runtime failure reverting with its
//! code, and a transaction that changes the conserved quantity rejected (`N0396`); the `web3`
//! profile refusing nondeterminism and outside authority before anything runs; and a pure function
//! meaning the same under it as under no profile.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const TOKEN: &str = "pub struct State { owner: Int, alice: Int, bob: Int, supply: Int, paused: Bool }\n\
fn init() -> State ! {} { State(owner: 1, alice: 100, bob: 0, supply: 100, paused: false) }\n\
fn conserved(s: State) -> Int ! {} { s.alice + s.bob }\n\
pub fn transfer(s: State, caller: Int, amount: Int) -> Result[State, Int] ! {} {\n\
    if s.paused { return Result[State, Int].Err(error: 3); }\n\
    if amount <= 0 { return Result[State, Int].Err(error: 1); }\n\
    if caller == 1 {\n\
        if s.alice < amount { return Result[State, Int].Err(error: 2); }\n\
        Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice - amount, bob: s.bob + amount, supply: s.supply, paused: s.paused))\n\
    } else {\n\
        if s.bob < amount { return Result[State, Int].Err(error: 2); }\n\
        Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice + amount, bob: s.bob - amount, supply: s.supply, paused: s.paused))\n\
    }\n\
}\n\
pub fn pause(s: State, caller: Int, on: Int) -> Result[State, Int] ! {} {\n\
    if caller != s.owner { return Result[State, Int].Err(error: 4); }\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice, bob: s.bob, supply: s.supply, paused: on == 1))\n\
}\n\
pub fn airdrop(s: State, caller: Int, amount: Int) -> Result[State, Int] ! {} {\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice, bob: s.bob + amount, supply: s.supply, paused: s.paused))\n\
}\n\
fn fee(amount: Int) -> Int ! {} { let mut f = 0; let mut i = 0; while i < amount { f = f + 1; i = i + 10; } f }\n\
pub fn charged(s: State, caller: Int, amount: Int) -> Result[State, Int] ! {} {\n\
    let f = fee(amount);\n\
    if caller != 1 || s.alice < amount + f { return Result[State, Int].Err(error: 2); }\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice - amount - f, bob: s.bob + amount + f, supply: s.supply, paused: s.paused))\n\
}\n\
fn audit(s: State) -> Int ! {} { s.alice }\n\
pub fn opaque(s: State, caller: Int) -> Result[State, Int] ! {} {\n\
    if audit(s) > 1000 { return Result[State, Int].Err(error: 9); }\n\
    Result[State, Int].Ok(value: s)\n\
}\n\
fn main() -> Int ! {} { 0 }\n";

const TXS: &str = "1 transfer 30\n2 transfer 50\n2 transfer 10\n2 pause 1\n1 pause 1\n1 transfer 5\n1 pause 0\n# a comment\n\n1 transfer 9223372036854775807\n1 charged 25\n2 airdrop 7\n1 transfer -1\n1 nothing 3\n1 transfer\n";

fn setup(src: &str, txs: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("c.nz"), src).unwrap();
    std::fs::write(d.path().join("txs.txt"), txs).unwrap();
    d
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_contracts_facts_name_each_entrypoints_reads_and_writes() {
    let d = setup(TOKEN, "");
    let o = nazm(d.path(), &["contract", "c.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = json(&o);
    assert_eq!(v["schema"], "nazm.contract/1");
    assert_eq!(v["profile"], "web3");
    assert_eq!(v["conserved"], true);
    let fields: Vec<&str> = v["state"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["owner", "alice", "bob", "supply", "paused"]);
    let e = |name: &str| {
        v["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(
        e("transfer")["reads"],
        serde_json::json!(["alice", "bob", "paused"])
    );
    assert_eq!(e("transfer")["writes"], serde_json::json!(["alice", "bob"]));
    assert_eq!(e("transfer")["args"], serde_json::json!(["amount"]));
    assert_eq!(e("pause")["reads"], serde_json::json!(["owner"]));
    assert_eq!(e("pause")["writes"], serde_json::json!(["paused"]));
    assert_eq!(e("airdrop")["writes"], serde_json::json!(["bob"]));
    // Handed to a helper, the state is opaque: every field may be read; returned whole, nothing
    // is written.
    assert_eq!(e("opaque")["reads"], "all");
    assert_eq!(e("opaque")["writes"], serde_json::json!([]));
    // Not entrypoints: private, or not of the shape.
    assert!(v["entrypoints"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["name"] != "fee" && e["name"] != "audit"));
}

#[test]
fn transactions_run_deterministically_and_a_revert_changes_nothing() {
    let d = setup(TOKEN, TXS);
    let o = nazm(d.path(), &["contract", "c.nz", "--txs", "txs.txt"]);
    // `airdrop` broke conservation: the run reports it and fails.
    assert_eq!(o.status.code(), Some(1));
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("error[N0396]: transaction 12 (`airdrop`) changed the conserved quantity from 100 to 107"), "{e}");
    let v = json(&o);
    assert_eq!(v["schema"], "nazm.contract-run/1");
    let t = v["transactions"].as_array().unwrap();
    let outcomes: Vec<(u64, &str, String)> = t
        .iter()
        .map(|x| {
            (
                x["line"].as_u64().unwrap(),
                x["outcome"].as_str().unwrap(),
                x["code"].as_str().map(str::to_owned).unwrap_or_default(),
            )
        })
        .collect();
    fn s(l: u64, o: &'static str, c: &str) -> (u64, &'static str, String) {
        (l, o, c.to_owned())
    }
    assert_eq!(
        outcomes,
        [
            s(1, "ok", ""),
            s(2, "revert", "2"),
            s(3, "ok", ""),
            s(4, "revert", "4"),
            s(5, "ok", ""),
            s(6, "revert", "3"),
            s(7, "ok", ""),
            s(10, "revert", "2"),
            s(11, "ok", ""),
            s(12, "rejected", "N0396"),
            s(13, "revert", "1"),
            s(14, "no-such-entrypoint", ""),
            s(15, "wrong-arity", ""),
        ]
    );
    assert_eq!(t[0]["changed"], serde_json::json!(["alice", "bob"]));
    assert_eq!(t[4]["changed"], serde_json::json!(["paused"]));
    // The fee's helper call and its three loop turns are metered; a plain transfer has none.
    assert_eq!(
        t[8]["meter"],
        serde_json::json!({ "calls": 1, "loop_turns": 3 })
    );
    assert_eq!(
        t[0]["meter"],
        serde_json::json!({ "calls": 0, "loop_turns": 0 })
    );
    // 100 → 70/30 → 80/20 → charged 25 + fee 3: 52/48; the rejected airdrop left it there.
    assert_eq!(
        v["state"],
        serde_json::json!({ "owner": "1", "alice": "52", "bob": "48", "supply": "100", "paused": "false" })
    );
    // Two runs, the same bytes.
    let again = nazm(d.path(), &["contract", "c.nz", "--txs", "txs.txt"]);
    assert_eq!(again.stdout, o.stdout);
}

#[test]
fn a_runtime_failure_reverts_with_its_code() {
    let src = TOKEN.replace(
        "fn main() -> Int ! {} { 0 }",
        "pub fn boom(s: State, caller: Int, n: Int) -> Result[State, Int] ! {} { Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice * n, bob: s.bob, supply: s.supply, paused: s.paused)) }\nfn main() -> Int ! {} { 0 }",
    );
    let d = setup(&src, "1 boom 9223372036854775807\n1 transfer 1\n");
    let o = nazm(d.path(), &["contract", "c.nz", "--txs", "txs.txt"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = json(&o);
    assert_eq!(v["transactions"][0]["outcome"], "fail");
    assert_eq!(v["transactions"][0]["code"], "N0400");
    assert_eq!(v["state"]["alice"], "99");
}

#[test]
fn the_web3_profile_refuses_nondeterminism_and_outside_authority() {
    for (body, rule) in [
        ("fn clock(t: TimeCap) -> Int ! {} { time_now_ms() }\nfn main(t: TimeCap) -> Int ! {} { clock(t) }", "no-ambient-time"),
        ("fn main(io: IoCap) -> Int ! { io } { print(\"x\"); 0 }", "no-io"),
        ("fn down(n: Int) -> Int ! {} { if n == 0 { 0 } else { down(n - 1) } }\nfn main() -> Int ! {} { down(3) }", "no-recursion"),
        ("fn main() -> Int { 0 }", "declared-effects"),
        ("fn main() -> Int ! {} { let c = chan_new(1); if chan_send(c, 1) { 1 } else { 0 } }", "no-blocking"),
    ] {
        let d = setup(&format!("{body}\n"), "");
        let o = nazm(d.path(), &["contract", "c.nz"]);
        assert!(!o.status.success(), "{rule} admitted");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains(&format!("rule `{rule}`")), "{rule}: {e}");
        assert!(o.stdout.is_empty());
    }
}

#[test]
fn a_module_that_is_not_a_contract_is_refused_by_what_it_lacks() {
    for (src, why) in [
        ("fn main() -> Int ! {} { 0 }", "a contract declares a record `State`"),
        ("pub struct State { name: Str }\nfn main() -> Int ! {} { 0 }", "`Int` and `Bool` fields"),
        ("pub struct State { n: Int }\nfn main() -> Int ! {} { 0 }", "`fn init() -> State`"),
        ("pub struct State { n: Int }\nfn init() -> State ! {} { State(n: 0) }\nfn main() -> Int ! {} { 0 }", "a contract has an entrypoint"),
    ] {
        let d = setup(&format!("{src}\n"), "");
        let o = nazm(d.path(), &["contract", "c.nz"]);
        assert!(!o.status.success());
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains("error[N0395]"), "{e}");
        assert!(e.contains(why), "{why}: {e}");
    }
}

#[test]
fn a_pure_function_means_the_same_under_web3_as_under_no_profile() {
    let d = setup(
        "fn f(x: Int) -> Int ! {} { x * 3 - 7 }\nfn main() -> Int ! {} { f(14) + f(-2) }\n",
        "",
    );
    let plain = nazm(d.path(), &["run", "--interpret", "c.nz"]);
    let web3 = nazm(
        d.path(),
        &["run", "--interpret", "--profile", "web3", "c.nz"],
    );
    assert!(plain.status.success() && web3.status.success());
    assert_eq!(plain.stdout, web3.stdout);
    assert_eq!(String::from_utf8_lossy(&plain.stdout), "22\n");
}

#[test]
fn a_state_the_analysis_does_not_follow_may_write_every_field() {
    let src = "pub struct State { a: Int, b: Int }\n\
               fn init() -> State ! {} { State(a: 0, b: 0) }\n\
               pub fn rebuilt(s: State, caller: Int) -> Result[State, Int] ! {} { let t = State(a: s.a, b: caller); Result[State, Int].Ok(value: t) }\n\
               pub fn direct(s: State, caller: Int) -> Result[State, Int] ! {} { Result[State, Int].Ok(value: State(a: s.a, b: caller)) }\n\
               fn main() -> Int ! {} { 0 }\n";
    let d = setup(src, "");
    let v = json(&nazm(d.path(), &["contract", "c.nz"]));
    let e = |name: &str| {
        v["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(e("rebuilt")["writes"], "all");
    assert_eq!(e("direct")["writes"], serde_json::json!(["b"]));
    assert_eq!(e("direct")["reads"], serde_json::json!([]));
}
