//! N72: account-oriented contracts — signed writes and account metadata.
//!
//! `docs/internal/architecture.md` §7.74. What the commands show: the `accounts` profile refuses an
//! entrypoint that writes the state without deciding on its `caller`, by name, and does not trust a
//! check made only in a helper; a read-only entrypoint needs no signer; `--accounts` writes each
//! instruction's discriminator, arguments and account constraints and the state account's layout.
//! Execution on an sBPF target is BLOCKED (no toolchain, validator or emulator): nothing here
//! stands in for it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const HEAD: &str = "pub struct State { owner: Int, alice: Int, bob: Int, paused: Bool }\n\
fn init() -> State ! {} { State(owner: 1, alice: 100, bob: 0, paused: false) }\n";

const SIGNED: &str = "pub fn transfer(s: State, caller: Int, amount: Int) -> Result[State, Int] ! {} {\n\
    if caller != 1 || s.alice < amount { return Result[State, Int].Err(error: 2); }\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice - amount, bob: s.bob + amount, paused: s.paused))\n\
}\n\
pub fn pause(s: State, caller: Int, on: Int) -> Result[State, Int] ! {} {\n\
    if caller != s.owner { return Result[State, Int].Err(error: 4); }\n\
    Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice, bob: s.bob, paused: on == 1))\n\
}\n\
pub fn peek(s: State, caller: Int) -> Result[State, Int] ! {} {\n\
    if s.paused { return Result[State, Int].Err(error: 3); }\n\
    Result[State, Int].Ok(value: s)\n\
}\n";

fn contract(body: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("c.nz"),
        format!("{HEAD}{body}fn main() -> Int ! {{}} {{ 0 }}\n"),
    )
    .unwrap();
    d
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn a_write_no_signer_decides_on_is_refused_by_name() {
    let d = contract(&format!(
        "{SIGNED}pub fn airdrop(s: State, caller: Int, amount: Int) -> Result[State, Int] ! {{}} {{ Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice, bob: s.bob + amount, paused: s.paused)) }}\n"
    ));
    let o = nazm(d.path(), &["check", "--profile", "accounts", "c.nz"]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("rule `signed-writes`"), "{e}");
    assert!(
        e.contains("`airdrop` writes the state and never decides on its caller"),
        "{e}"
    );
    // Only `airdrop`: the others decide on their caller, or write nothing.
    assert!(
        !e.contains("`transfer`") && !e.contains("`pause`") && !e.contains("`peek`"),
        "{e}"
    );
    // `web3` alone does not ask for signers.
    assert!(nazm(d.path(), &["check", "--profile", "web3", "c.nz"])
        .status
        .success());
}

/// Q1-C-03: a new state the analysis cannot see into — made by a helper, read out of a wrapper's
/// field or an array, returned by a trait method — may write every field, so an entrypoint
/// returning one without deciding on its caller is refused, as one building it in place is.
#[test]
fn a_state_built_out_of_sight_is_a_write() {
    let new = "State(owner: s.owner, alice: s.alice, bob: s.bob + n, paused: s.paused)";
    for (what, defs, value) in [
        ("a helper", format!("fn give(s: State, n: Int) -> State ! {{}} {{ {new} }}\n"), "give(s, n)".to_owned()),
        ("a wrapper's field", "struct W { st: State }\n".to_owned(), format!("W(st: {new}).st")),
        ("an array element", String::new(), format!("[{new}][0]")),
        (
            "a trait method",
            format!("trait G {{ fn give(self: Self, n: Int) -> State ! {{}}; }}\nimpl G for State {{ fn give(self: State, n: Int) -> State ! {{}} {{ let s = self; {new} }} }}\n"),
            "s.give(n)".to_owned(),
        ),
    ] {
        let d = contract(&format!(
            "{defs}pub fn airdrop(s: State, caller: Int, n: Int) -> Result[State, Int] ! {{}} {{ Result[State, Int].Ok(value: {value}) }}\n"
        ));
        let o = nazm(d.path(), &["check", "--profile", "accounts", "c.nz"]);
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{what}: {e}");
        assert!(
            e.contains("`airdrop` writes the state and never decides on its caller"),
            "{what}: {e}"
        );
    }
}

#[test]
fn a_check_made_only_in_a_helper_is_not_trusted() {
    let d = contract(
        "fn owns(caller: Int, s: State) -> Bool ! {} { caller == s.owner }\n\
         pub fn mint(s: State, caller: Int, n: Int) -> Result[State, Int] ! {} {\n\
             let ok = owns(caller, s);\n\
             if ok { Result[State, Int].Ok(value: State(owner: s.owner, alice: s.alice + n, bob: s.bob, paused: s.paused)) } else { Result[State, Int].Err(error: 4) }\n\
         }\n",
    );
    let o = nazm(d.path(), &["check", "--profile", "accounts", "c.nz"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr)
        .contains("`mint` writes the state and never decides on its caller"));
}

#[test]
fn the_account_metadata_states_every_instructions_constraints() {
    let d = contract(SIGNED);
    let o = nazm(d.path(), &["contract", "c.nz", "--accounts", "out"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/accounts.json")).unwrap())
            .unwrap();
    assert_eq!(v["schema"], "nazm.sbpf-accounts/1");
    assert_eq!(v["profile"], "accounts");
    assert!(v["execution"].as_str().unwrap().starts_with("BLOCKED"));
    let data = &v["state_account"]["data"];
    assert_eq!(data["version"], 1);
    assert_eq!(data["size"], 1 + 8 * 4);
    let offsets: Vec<u64> = data["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, [1, 9, 17, 25]);
    assert_eq!(data["fields"][3]["type"], "bool as u64");
    let i = |name: &str| {
        v["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == name)
            .cloned()
            .unwrap()
    };
    // The first eight bytes of keccak256("nazm.instruction.transfer").
    assert_eq!(i("transfer")["discriminator"], "75bf341e84e6d97c");
    assert_eq!(i("transfer")["args"][0]["offset"], 8);
    let acc = |name: &str, which: usize| i(name)["accounts"][which].clone();
    assert_eq!(
        (
            acc("transfer", 0)["writable"].as_bool(),
            acc("transfer", 1)["signer"].as_bool()
        ),
        (Some(true), Some(true))
    );
    assert_eq!(
        (
            acc("pause", 0)["writable"].as_bool(),
            acc("pause", 1)["signer"].as_bool()
        ),
        (Some(true), Some(true))
    );
    // Read-only: no write, and no signer asked for.
    assert_eq!(
        (
            acc("peek", 0)["writable"].as_bool(),
            acc("peek", 1)["signer"].as_bool()
        ),
        (Some(false), Some(false))
    );
    assert_eq!(
        v["cross_program_calls"],
        "none: an entrypoint calls only Nazm functions"
    );
}
