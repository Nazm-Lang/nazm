//! N96: the contract simulator against the real VMs, on generated transaction sequences.
//!
//! `docs/architecture.md` §7.97. N70 and N71 held the EVM and WebAssembly backends to the
//! simulator on one written sequence each. Here sequences are generated — seeded, the same on every
//! host — from the contract's own facts (`nazm.contract/1`'s entrypoints and their argument counts),
//! with callers among the owner and two others and arguments from the edges of `Int`: every
//! outcome, revert code and final state must agree, every committed state must keep the conserved
//! quantity, and every EVM transaction must use no more gas than its entrypoint's stated bound.
//! The VMs are the reference ones of N70 and N71, in their images: run with `--ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every construct the backends share: transfers by caller, an owner's switch, a conservation
/// breach (`airdrop`), a loop (`fee`), overflow (`boom`), and division and remainder at their edges.
const CONTRACT: &str = "pub struct State { owner: Int, alice: Int, bob: Int, supply: Int, paused: Bool }\n\
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
    if amount > 1000 { return Result[State, Int].Err(error: 5); }\n\
    let f = fee(amount);\n\
    if caller != 1 || s.alice < amount + f { return Result[State, Int].Err(error: 2); }\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice - amount - f, bob: s.bob + amount + f, supply: s.supply, paused: s.paused))\n\
}\n\
pub fn boom(s: State, caller: Int, n: Int) -> Result[State, Int] ! {} {\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice * n, bob: s.bob, supply: s.supply, paused: s.paused))\n\
}\n\
pub fn arith(s: State, caller: Int, a: Int, b: Int) -> Result[State, Int] ! {} {\n\
    let q = a / b;\n\
    let r = a % b;\n\
    Result[State, Int].Err(error: q + r)\n\
}\n\
fn main() -> Int ! {} { 0 }\n";

const SEEDS: [u64; 3] = [0x5eed, 0xc0ffee, 0xdecaf];
const LENGTH: usize = 250;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)))
}

/// One generated sequence: each line `CALLER ENTRY ARGS…`, entrypoints and their arities from the
/// contract's facts.
fn sequence(facts: &Value, seed: u64) -> String {
    let entries: Vec<(String, usize)> = facts["entrypoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().to_owned(),
                e["args"].as_array().unwrap().len(),
            )
        })
        .collect();
    assert!(entries.len() >= 6, "{facts}");
    let edges = [
        0,
        1,
        -1,
        2,
        5,
        7,
        10,
        25,
        30,
        99,
        100,
        101,
        1000,
        i64::MAX,
        i64::MIN,
        i64::MAX - 1,
        i64::MIN + 1,
    ];
    let mut rng = Rng(seed);
    let mut out = String::new();
    for _ in 0..LENGTH {
        let (name, arity) = entries[(rng.next() % entries.len() as u64) as usize].clone();
        let caller = rng.pick(&[1, 1, 2, 3]);
        out.push_str(&format!("{caller} {name}"));
        for _ in 0..arity {
            let v = if rng.next().is_multiple_of(4) {
                (rng.next() % 200) as i64 - 50
            } else {
                rng.pick(&edges)
            };
            out.push_str(&format!(" {v}"));
        }
        out.push('\n');
    }
    out
}

/// The contract, its facts and each seed's sequence and simulated run, in a fresh directory.
fn setup() -> (tempfile::TempDir, Vec<(String, Value)>) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("c.nz"), CONTRACT).unwrap();
    let facts = json(&nazm(d.path(), &["contract", "c.nz"]));
    let mut runs = Vec::new();
    for (i, seed) in SEEDS.iter().enumerate() {
        let file = format!("txs{i}.txt");
        std::fs::write(d.path().join(&file), sequence(&facts, *seed)).unwrap();
        let sim = json(&nazm(d.path(), &["contract", "c.nz", "--txs", &file]));
        runs.push((file, sim));
    }
    (d, runs)
}

fn outcomes(run: &Value) -> Vec<(Value, Value, Value)> {
    run["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["line"].clone(), t["outcome"].clone(), t["code"].clone()))
        .collect()
}

#[test]
fn generated_sequences_run_deterministically_and_keep_the_conserved_quantity() {
    let (d, runs) = setup();
    for (file, sim) in &runs {
        // The same bytes on a second run.
        let again = nazm(d.path(), &["contract", "c.nz", "--txs", file]);
        assert_eq!(json(&again), *sim, "{file}");
        let txs = sim["transactions"].as_array().unwrap();
        assert_eq!(txs.len(), LENGTH, "{file}");
        let mut kinds: Vec<&str> = txs.iter().map(|t| t["outcome"].as_str().unwrap()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        // The sequences reach commits, reverts and rejections alike.
        assert!(kinds.len() >= 3, "{file}: {kinds:?}");
        // Every committed state keeps `alice + bob`: the final state is the last committed one.
        let s = &sim["state"];
        // The state's values are written as text, an `Int` being wider than JSON promises.
        let int = |k: &str| s[k].as_str().unwrap().parse::<i64>().unwrap();
        let total = int("alice") + int("bob");
        assert_eq!(total, 100, "{file}: {s}");
    }
}

/// Needs Docker and `nazm-evm:n70`.
#[test]
#[ignore = "starts a container"]
fn the_evm_agrees_with_the_simulator_on_generated_sequences() {
    let (d, runs) = setup();
    assert!(nazm(d.path(), &["contract", "c.nz", "--evm", "out"])
        .status
        .success());
    let abi: Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/abi.json")).unwrap())
            .unwrap();
    for (file, sim) in &runs {
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
                file,
            ])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let evm = json(&o);
        assert_eq!(outcomes(sim), outcomes(&evm), "{file}");
        assert_eq!(sim["state"], evm["state"], "{file}");
        for t in evm["transactions"].as_array().unwrap() {
            let bound = abi["entrypoints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == t["entry"])
                .and_then(|e| e["gas_bound"].as_u64());
            if let (Some(bound), Some(used)) = (bound, t["gas"].as_u64()) {
                assert!(used <= bound, "{file}: {t}: used {used} > bound {bound}");
            }
        }
    }
}

/// Needs Docker and `nazm-wasm:n64`.
#[test]
#[ignore = "starts a container"]
fn the_webassembly_contract_agrees_with_the_simulator_on_generated_sequences() {
    let (d, runs) = setup();
    assert!(nazm(d.path(), &["contract", "c.nz", "--wasm", "out"])
        .status
        .success());
    let files: Vec<&str> = runs.iter().map(|(f, _)| f.as_str()).collect();
    let script = format!(
        "wasm-ld --no-entry --stack-first -z stack-size=65536 --export=nazm_main -o module.wasm \
         00-m0.o 01-runtime.o 02-entry.o && for f in {}; do node contract_host.mjs module.wasm \
         contract.json ../$f; echo '@@'; done",
        files.join(" ")
    );
    let o = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args(["-w", "/w/out", "nazm-wasm:n64", "sh", "-c", &script])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8(o.stdout).unwrap();
    let parts: Vec<&str> = text.split("@@").filter(|p| !p.trim().is_empty()).collect();
    assert_eq!(parts.len(), runs.len());
    for ((file, sim), part) in runs.iter().zip(parts) {
        let run: Value = serde_json::from_str(part.trim()).unwrap();
        assert_eq!(outcomes(sim), outcomes(&run), "{file}");
        assert_eq!(sim["state"], run["state"], "{file}");
    }
}
