//! N70: the EVM backend — the contract model as bytecode, held to the simulator.
//!
//! `docs/internal/architecture.md` §7.72. On any host: the artefacts — selectors from the compiler's
//! keccak of each `int64` signature, storage slots from each field's name, a static gas bound for
//! every loop-free entrypoint and none for one with a loop, the same bytes twice; a construct the
//! backend does not emit refused before emission (`N0399`); the upgrade check. With
//! `nazm-evm:n70` (the ignored test): deployed in py-evm and sent the simulator's transactions from
//! the same senders, every outcome, revert code and the final storage equal the simulator's, and
//! measured gas stays inside every stated bound.

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
fn bonus() -> Int ! {} { let mut b = 0; let mut i = 0; while i < 8 { b = b + i; i = i + 1; } b }\n\
pub fn rebate(s: State, caller: Int) -> Result[State, Int] ! {} {\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice, bob: s.bob + bonus(), supply: s.supply, paused: s.paused))\n\
}\n\
pub fn arith(s: State, caller: Int, a: Int, b: Int) -> Result[State, Int] ! {} {\n\
    let q = a / b;\n\
    let r = a % b;\n\
    let n = -a;\n\
    Result[State, Int].Err(error: q * 1000000 + r * 1000 + n)\n\
}\n\
fn main() -> Int ! {} { 0 }\n";

/// Transfers, reverts by code, the owner's authority, a conservation breach, a loop, overflow,
/// division by zero, `i64::MIN / -1`, truncating division and the remainder's sign, and bad input.
const TXS: &str = "1 transfer 30\n2 transfer 50\n2 transfer 10\n2 pause 1\n1 pause 1\n1 transfer 5\n1 pause 0\n\
1 transfer 9223372036854775807\n2 airdrop 7\n1 transfer -1\n1 charged 25\n1 arith 7 2\n1 arith -7 2\n1 arith 7 -2\n\
1 arith 5 0\n1 arith -9223372036854775808 -1\n1 arith -9223372036854775808 1\n2 transfer 1000000\n1 nothing 1\n1 rebate\n";

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("c.nz"), TOKEN).unwrap();
    std::fs::write(d.path().join("txs.txt"), TXS).unwrap();
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
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

#[test]
fn the_backend_writes_selectors_slots_and_gas_bounds_the_same_twice() {
    let d = setup();
    let o = nazm(d.path(), &["contract", "c.nz", "--evm", "out"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = json(&o);
    assert_eq!(v["schema"], "nazm.evm/1");
    let abi: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/abi.json")).unwrap())
            .unwrap();
    let e = |name: &str| {
        abi["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(e("transfer")["signature"], "transfer(int64)");
    assert_eq!(e("arith")["signature"], "arith(int64,int64)");
    // Selectors and slots are keccak-256, the compiler's own; its vectors are held in
    // `nazm-service`'s unit tests. Every selector is four distinct bytes here.
    let selectors: std::collections::BTreeSet<String> = abi["entrypoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["selector"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(selectors.len(), 6);
    // Pinned against `eth_hash`'s keccak in `nazm-evm:n70`.
    assert_eq!(e("transfer")["selector"], "10022b66");
    assert_eq!(e("arith")["selector"], "5faab410");
    assert!(selectors.iter().all(|s| s.len() == 8));
    // Loop-free entrypoints have a bound; one that calls a helper with a loop has none.
    assert!(e("transfer")["gas_bound"].as_u64().unwrap() > 20_000);
    // A loop whose bound is a parameter leaves the bound open; one in the counted form (Gate 3)
    // states it: eight turns of its condition and body, and the last condition.
    assert!(e("charged")["gas_bound"].is_null());
    assert!(e("rebate")["gas_bound"].as_u64().unwrap() > 0);
    let storage: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/storage.json")).unwrap())
            .unwrap();
    assert_eq!(storage["schema"], "nazm.evm-storage/1");
    let fields: Vec<&str> = storage["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["owner", "alice", "bob", "supply", "paused"]);
    assert_eq!(storage["fields"][4]["type"], "Bool");
    assert_eq!(
        storage["fields"][1]["slot"],
        "0f66a9ac52373aea2ea84192a98c645c5109ada1e238603ca6b6712e72a4ecee"
    );
    let deploy = std::fs::read_to_string(d.path().join("out/deploy.hex")).unwrap();
    let runtime = std::fs::read_to_string(d.path().join("out/runtime.hex")).unwrap();
    assert!(
        deploy.ends_with(&runtime),
        "the deployment code returns the runtime it carries"
    );
    // The runtime raises its own two codes as five-byte reverts: `N0397` for a sender that is not
    // an `Int`, `N0398` for calldata that is no entrypoint's.
    for (code, bytes) in [("N0397", "4e30333937"), ("N0398", "4e30333938")] {
        assert!(
            runtime.contains(bytes),
            "{code} is not raised by the runtime"
        );
    }
    // And it copies the runtime from exactly where it starts: `PUSH2 offset; PUSH1 0; CODECOPY`.
    let start = (deploy.len() - runtime.len()) >> 1;
    let at = deploy.find("600039").expect("a CODECOPY");
    assert_eq!(&deploy[at - 6..at - 4], "61", "the offset is a PUSH2");
    let offset = usize::from_str_radix(&deploy[at - 4..at], 16).unwrap();
    assert_eq!(
        offset, start,
        "the runtime is copied from {offset}, but starts at {start}"
    );
    assert!(nazm(d.path(), &["contract", "c.nz", "--evm", "again"])
        .status
        .success());
    assert_eq!(
        std::fs::read_to_string(d.path().join("again/deploy.hex")).unwrap(),
        deploy
    );
}

#[test]
fn a_construct_the_backend_does_not_emit_is_refused_before_emission() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("c.nz"),
        "pub struct State { n: Int }\nfn init() -> State ! {} { State(n: 0) }\n\
         pub fn name(s: State, caller: Int) -> Result[State, Int] ! {} { Result[State, Int].Ok(value: State(n: str_len(\"abc\"))) }\n\
         fn main() -> Int ! {} { 0 }\n",
    )
    .unwrap();
    let o = nazm(d.path(), &["contract", "c.nz", "--evm", "out"]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("error[N0399]"), "{e}");
    assert!(
        e.contains("it calls the built-in `str_len`, which has no EVM form"),
        "{e}"
    );
    assert!(
        !d.path().join("out/deploy.hex").exists(),
        "something was emitted"
    );
}

#[test]
fn the_upgrade_check_allows_an_added_field_and_refuses_a_removed_or_retyped_one() {
    let d = tempfile::tempdir().unwrap();
    let v = |fields: &str, init: &str| {
        format!(
            "pub struct State {{ {fields} }}\nfn init() -> State ! {{}} {{ State({init}) }}\n\
             pub fn touch(s: State, caller: Int) -> Result[State, Int] ! {{}} {{ Result[State, Int].Ok(value: s) }}\n\
             fn main() -> Int ! {{}} {{ 0 }}\n"
        )
    };
    std::fs::write(
        d.path().join("v1.nz"),
        v("a: Int, b: Bool", "a: 1, b: true"),
    )
    .unwrap();
    // Reordered and one added: compatible — slots are by name.
    std::fs::write(
        d.path().join("v2.nz"),
        v("c: Int, b: Bool, a: Int", "a: 1, b: true, c: 0"),
    )
    .unwrap();
    std::fs::write(d.path().join("v3.nz"), v("a: Bool", "a: true")).unwrap();
    let o = nazm(d.path(), &["contract", "v2.nz", "--upgrade-check", "v1.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r = json(&o);
    assert_eq!(r["schema"], "nazm.evm-upgrade/1");
    assert_eq!(r["outcome"], "compatible");
    assert_eq!(r["changes"][0]["field"], "c");
    let o = nazm(d.path(), &["contract", "v3.nz", "--upgrade-check", "v1.nz"]);
    assert!(!o.status.success());
    let r = json(&o);
    assert_eq!(r["outcome"], "incompatible");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("`a` cannot be upgraded in place: its type changed from `Int` to `Bool`"),
        "{e}"
    );
    assert!(
        e.contains("`b` cannot be upgraded in place: removed"),
        "{e}"
    );
}

/// Needs Docker and `nazm-evm:n70`: `cargo test -p nazm-cli --test cli_e_l -- --ignored evm::`.
#[test]
#[ignore = "starts a container"]
fn the_evm_agrees_with_the_simulator_and_stays_inside_its_gas_bounds() {
    let d = setup();
    assert!(nazm(d.path(), &["contract", "c.nz", "--evm", "out"])
        .status
        .success());
    let sim = nazm(d.path(), &["contract", "c.nz", "--txs", "txs.txt"]);
    let sim = json(&sim);
    let o = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args([
            "-w",
            "/w",
            "nazm-evm:n70",
            "python",
            "out/evm_run.py",
            "out",
            "txs.txt",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let evm = json(&o);
    let key = |t: &serde_json::Value| (t["line"].clone(), t["outcome"].clone(), t["code"].clone());
    let s: Vec<_> = sim["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(key)
        .collect();
    let e: Vec<_> = evm["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(key)
        .collect();
    assert_eq!(s, e);
    assert_eq!(sim["state"], evm["state"]);
    // The arithmetic the language defines, on the EVM: q·10⁶ + r·10³ − a for 7/2 (3 r 1), −7/2
    // (−3 r −1), 7/−2 (−3 r 1); division by zero; `i64::MIN / -1`; and `i64::MIN · 10⁶`.
    let codes: Vec<&str> = evm["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["entry"] == "arith")
        .map(|t| t["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        ["3000993", "-3000993", "-2999007", "N0401", "N0400", "N0400"]
    );
    let abi: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/abi.json")).unwrap())
            .unwrap();
    for t in evm["transactions"].as_array().unwrap() {
        let Some(bound) = abi["entrypoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == t["entry"])
            .and_then(|e| e["gas_bound"].as_u64())
        else {
            continue;
        };
        let used = t["gas"].as_u64().unwrap();
        assert!(used <= bound, "{t}: used {used} > bound {bound}");
    }
}
