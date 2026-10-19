//! N71: contracts on WebAssembly — the ordinary backend with a host adapter.
//!
//! `docs/internal/architecture.md` §7.73. On any host: `--wasm` writes WebAssembly objects, the metadata
//! (`nazm.wasm-contract/1`: fields by declaration index, each entrypoint's export, the imports) and
//! the reference host, the same bytes twice; the contract's object asks its host for exactly
//! `state_get`, `state_set`, `revert` and `report`. In `nazm-wasm:n64` (the ignored test): the linked
//! module imports exactly those and exports `nazm_init` and one `nazm_call_*` per entrypoint, and the
//! simulator's transactions give the same outcomes, codes and final state as `nazm contract --txs`.

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
pub fn boom(s: State, caller: Int, n: Int) -> Result[State, Int] ! {} {\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice * n, bob: s.bob, supply: s.supply, paused: s.paused))\n\
}\n\
fn main() -> Int ! {} { 0 }\n";

const TXS: &str = "1 transfer 30\n2 transfer 50\n2 transfer 10\n2 pause 1\n1 pause 1\n1 transfer 5\n1 pause 0\n\
1 transfer 9223372036854775807\n2 airdrop 7\n1 transfer -1\n1 charged 25\n1 boom 9223372036854775807\n1 transfer 1\n1 nothing 1\n";

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

/// The `(module, name)` of every import a WebAssembly binary declares.
fn imports(bytes: &[u8]) -> Vec<(String, String)> {
    let leb = |b: &[u8], at: &mut usize| -> usize {
        let (mut n, mut shift) = (0usize, 0);
        loop {
            let byte = b[*at];
            *at += 1;
            n |= usize::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return n;
            }
        }
    };
    let name = |b: &[u8], at: &mut usize| -> String {
        let n = leb(b, at);
        let s = String::from_utf8(b[*at..*at + n].to_vec()).unwrap();
        *at += n;
        s
    };
    let mut at = 8;
    let mut out = Vec::new();
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = leb(bytes, &mut at);
        let end = at + size;
        if id == 2 {
            let mut p = at;
            for _ in 0..leb(bytes, &mut p) {
                let module = name(bytes, &mut p);
                let field = name(bytes, &mut p);
                let kind = bytes[p];
                p += 1;
                match kind {
                    0 => {
                        leb(bytes, &mut p);
                    }
                    1 => {
                        p += 1;
                        let flags = leb(bytes, &mut p);
                        leb(bytes, &mut p);
                        if flags & 1 == 1 {
                            leb(bytes, &mut p);
                        }
                    }
                    2 => {
                        let flags = leb(bytes, &mut p);
                        leb(bytes, &mut p);
                        if flags & 1 == 1 {
                            leb(bytes, &mut p);
                        }
                    }
                    _ => p += 2,
                }
                out.push((module, field));
            }
        }
        at = end;
    }
    out
}

#[test]
fn the_contract_builds_for_webassembly_with_its_adapter_and_metadata() {
    let d = setup();
    let o = nazm(d.path(), &["contract", "c.nz", "--wasm", "out"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/contract.json")).unwrap())
            .unwrap();
    assert_eq!(meta["schema"], "nazm.wasm-contract/1");
    assert_eq!(meta["version"], 1);
    let fields: Vec<(u64, &str, &str)> = meta["state"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["index"].as_u64().unwrap(),
                f["name"].as_str().unwrap(),
                f["type"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            (0, "owner", "Int"),
            (1, "alice", "Int"),
            (2, "bob", "Int"),
            (3, "supply", "Int"),
            (4, "paused", "Bool")
        ]
    );
    let exports: Vec<&str> = meta["entrypoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["export"].as_str().unwrap())
        .collect();
    assert_eq!(
        exports,
        [
            "nazm_call_transfer",
            "nazm_call_pause",
            "nazm_call_airdrop",
            "nazm_call_charged",
            "nazm_call_boom"
        ]
    );
    assert!(d.path().join("out/contract_host.mjs").exists());
    // The contract's unit asks its host for the state, the revert and the report, and nothing
    // else — no C library, no other module.
    let unit = std::fs::read(d.path().join("out/00-m0.o")).unwrap();
    let asked: std::collections::BTreeSet<String> = imports(&unit)
        .into_iter()
        // `env` names what another unit defines, which the linker resolves; only `nazm_host`
        // reaches the host.
        .filter(|(m, _)| m != "env")
        .map(|(m, f)| format!("{m}.{f}"))
        .collect();
    let want: std::collections::BTreeSet<String> = [
        "nazm_host.state_get",
        "nazm_host.state_set",
        "nazm_host.revert",
        "nazm_host.report",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert!(asked.is_subset(&want), "{asked:?}");
    assert!(
        asked.contains("nazm_host.state_get") && asked.contains("nazm_host.state_set"),
        "{asked:?}"
    );
    // The same bytes twice.
    assert!(nazm(d.path(), &["contract", "c.nz", "--wasm", "again"])
        .status
        .success());
    for f in ["00-m0.o", "contract.json"] {
        assert_eq!(
            std::fs::read(d.path().join("out").join(f)).unwrap(),
            std::fs::read(d.path().join("again").join(f)).unwrap(),
            "{f}"
        );
    }
}

#[test]
fn the_adapter_leaves_the_ordinary_build_alone() {
    // Without `--wasm`, the same contract built for WebAssembly is an ordinary module: no adapter
    // export or host state import.
    let d = setup();
    let o = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--objects",
            "plain",
            "c.nz",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let unit = std::fs::read(d.path().join("plain/00-m0.o")).unwrap();
    assert!(
        imports(&unit)
            .iter()
            .all(|(_, f)| f != "state_get" && f != "state_set"),
        "{:?}",
        imports(&unit)
    );
    let has = |b: &[u8], s: &[u8]| b.windows(s.len()).any(|w| w == s);
    assert!(!has(&unit, b"nazm_call_transfer"));
    let adapted = {
        assert!(nazm(d.path(), &["contract", "c.nz", "--wasm", "out"])
            .status
            .success());
        std::fs::read(d.path().join("out/00-m0.o")).unwrap()
    };
    assert!(has(&adapted, b"nazm_call_transfer"));
}

/// Needs Docker and `nazm-wasm:n64`: `cargo test -p nazm-cli --test cli_s_z -- --ignored wasm_contract::`.
#[test]
#[ignore = "starts a container"]
fn the_webassembly_contract_agrees_with_the_simulator() {
    let d = setup();
    assert!(nazm(d.path(), &["contract", "c.nz", "--wasm", "out"])
        .status
        .success());
    let sim: serde_json::Value =
        serde_json::from_slice(&nazm(d.path(), &["contract", "c.nz", "--txs", "txs.txt"]).stdout)
            .unwrap();
    let o = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args([
            "-w",
            "/w/out",
            "nazm-wasm:n64",
            "sh",
            "-c",
            "wasm-ld --no-entry --stack-first -z stack-size=65536 --export=nazm_main -o module.wasm 00-m0.o 01-runtime.o 02-entry.o && node contract_host.mjs module.wasm contract.json ../txs.txt",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let run: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(run["schema"], "nazm.wasm-contract-run/1");
    let key = |t: &serde_json::Value| (t["line"].clone(), t["outcome"].clone(), t["code"].clone());
    let s: Vec<_> = sim["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(key)
        .collect();
    let w: Vec<_> = run["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(key)
        .collect();
    assert_eq!(s, w);
    assert_eq!(sim["state"], run["state"]);
    // The linked module: exactly the four imports, and an export per entrypoint.
    let module = std::fs::read(d.path().join("out/module.wasm")).unwrap();
    let mut asked: Vec<String> = imports(&module)
        .into_iter()
        .map(|(m, f)| format!("{m}.{f}"))
        .collect();
    asked.sort();
    assert_eq!(
        asked,
        [
            "nazm_host.report",
            "nazm_host.revert",
            "nazm_host.state_get",
            "nazm_host.state_set"
        ]
    );
}
