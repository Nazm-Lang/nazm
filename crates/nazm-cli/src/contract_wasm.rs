//! The WebAssembly contract adapter (N71, `docs/architecture.md` §7.73): LLVM text appended to the
//! contract's own unit, exporting `nazm_init` and `nazm_call_<entry>` and moving the state across
//! the host imports `nazm_host.state_get`, `state_set` and `revert`.
//!
//! Since N105 it reads the program, not the text: each function's symbol from its signature, and
//! the state's and result's bytes from `abi` on `wasm32` — LIR's layout, which the unit's code
//! uses.

use std::fmt::Write as _;

/// What the adapter is built from: the contract's shape, by name.
#[derive(Debug, Clone)]
pub(crate) struct Shape {
    /// The state's fields in declaration order, and whether each is a `Bool`.
    pub(crate) fields: Vec<(String, bool)>,
    /// Each entrypoint's name and how many arguments it takes after `caller`.
    pub(crate) entries: Vec<(String, usize)>,
    pub(crate) conserved: bool,
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "_{b:02X}");
        }
    }
    out
}

/// One Nazm function the adapter calls: its symbol, by its source name.
fn symbol_of(native: &nazm_lir::Native, name: &str) -> Option<(nazm_mir::FuncId, String)> {
    native
        .program
        .functions
        .iter()
        .find(|f| f.name == name && f.type_args.is_empty())
        .and_then(|f| {
            native
                .sigs
                .get(f.id.index())
                .map(|s| (f.id, s.symbol.as_str().to_owned()))
        })
}

/// Each state field, in declaration order — the host's index — with its offset in the record
/// and whether it is a `Bool`. The record is laid out in its canonical order, not the declared
/// one; `of_declaration` maps one to the other. The adapter's loads and stores both read this
/// table, so a wrong map round-trips the host's state unchanged and only the contract's own code
/// sees fields swapped — which is why `tests` holds it directly rather than through a run.
fn field_offsets(bools: &[bool], of_declaration: &[u32], offsets: &[u32]) -> Vec<(u32, bool)> {
    bools
        .iter()
        .enumerate()
        .map(|(k, is_bool)| {
            let pos = of_declaration.get(k).copied().unwrap_or(0) as usize;
            (offsets.get(pos).copied().unwrap_or(0), *is_bool)
        })
        .collect()
}

/// The adapter for `shape`, to append to `unit`, the LLVM text of the contract's own unit (N71,
/// N105). It calls the contract's functions by LIR's convention — the state record and the
/// result in memory at the offsets `abi` gives them on `wasm32`, the result through a pointer
/// passed first — so nothing about a value's bytes is decided here.
///
/// # Errors
///
/// A definition the adapter needs is not in the program, or its types are not a contract's.
#[allow(clippy::too_many_lines)]
pub(crate) fn adapter(
    unit: &str,
    shape: &Shape,
    native: &nazm_lir::Native,
) -> Result<String, String> {
    use nazm_lir::abi::{ByteLayouts, DataLayout, PAYLOAD};
    use nazm_lir::Ty;
    let (init_id, init) = symbol_of(native, "init").ok_or("the unit defines no `init`")?;
    let home = nazm_lir::backend::units(native)
        .into_iter()
        .map(|(_, u, _)| u)
        .find(|u| u.functions.contains(&init_id))
        .ok_or("no unit defines `init`")?;
    let lay = ByteLayouts::of(home, DataLayout::WASM32);
    let state_ty = native
        .sigs
        .get(init_id.index())
        .map(|s| s.ret)
        .ok_or("`init` has no signature")?;
    let Ty::Record(state_id) = state_ty else {
        return Err("`init` does not return a record".to_owned());
    };
    let state = lay
        .records
        .get(&state_id.0)
        .cloned()
        .ok_or("the state has no layout")?;
    let decl = home
        .records
        .iter()
        .find(|r| r.id == state_id)
        .map(|r| r.of_declaration.clone())
        .ok_or("the state has no table")?;
    let bools: Vec<bool> = shape.fields.iter().map(|(_, b)| *b).collect();
    let fields = field_offsets(&bools, &decl, &state.offsets);
    let size = state.lay.size.max(1);
    let mut attrs = vec![
        "\"wasm-import-module\"=\"nazm_host\" \"wasm-import-name\"=\"state_get\"".to_owned(),
        "\"wasm-import-module\"=\"nazm_host\" \"wasm-import-name\"=\"state_set\"".to_owned(),
        "\"wasm-import-module\"=\"nazm_host\" \"wasm-import-name\"=\"revert\"".to_owned(),
    ];
    let mut out =
        String::from("\n; The WebAssembly contract adapter (N71, docs/architecture.md §7.73).\n");
    let _ = writeln!(out, "declare i64 @nz.host_state_get(i64) #90");
    let _ = writeln!(out, "declare void @nz.host_state_set(i64, i64) #91");
    let _ = writeln!(out, "declare void @nz.host_revert(i64) #92");
    let _ = writeln!(
        out,
        "declare void @nz.host_report(ptr, i32) #{}",
        94 + shape.entries.len()
    );
    // The failure state the runtime records: declared by the unit already where it can fail.
    if !unit.contains("@nz.failed = external global") {
        let _ = writeln!(out, "@nz.failed = external global i1");
    }
    let _ = writeln!(out, "@nz.errmsg = external global ptr");
    let _ = writeln!(out, "@nz.errlen = external global i64");
    // Read the state from the host into memory at `at`, field by field.
    let load = |out: &mut String, at: &str| {
        for (k, (off, is_bool)) in fields.iter().enumerate() {
            let _ = writeln!(out, "  %{at}.raw{k} = call i64 @nz.host_state_get(i64 {k})");
            let _ = writeln!(out, "  %{at}.p{k} = getelementptr i8, ptr %{at}, i64 {off}");
            if *is_bool {
                let _ = writeln!(out, "  %{at}.v{k} = icmp ne i64 %{at}.raw{k}, 0");
                let _ = writeln!(out, "  store i1 %{at}.v{k}, ptr %{at}.p{k}");
            } else {
                let _ = writeln!(out, "  store i64 %{at}.raw{k}, ptr %{at}.p{k}");
            }
        }
    };
    // Write a state in memory at `from` back to the host, by index.
    let store = |out: &mut String, var: &str, from: &str| {
        for (k, (off, is_bool)) in fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "  %{var}.p{k} = getelementptr i8, ptr {from}, i64 {off}"
            );
            if *is_bool {
                let _ = writeln!(out, "  %{var}.b{k} = load i1, ptr %{var}.p{k}");
                let _ = writeln!(out, "  %{var}.w{k} = zext i1 %{var}.b{k} to i64");
            } else {
                let _ = writeln!(out, "  %{var}.w{k} = load i64, ptr %{var}.p{k}");
            }
            let _ = writeln!(
                out,
                "  call void @nz.host_state_set(i64 {k}, i64 %{var}.w{k})"
            );
        }
    };

    let _ = writeln!(out, "define i64 @nazm_init() #93 {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %s = alloca [{size} x i8], align 8");
    let _ = writeln!(out, "  call void @{init}(ptr %s)");
    store(&mut out, "i", "%s");
    let _ = writeln!(out, "  ret i64 0");
    let _ = writeln!(out, "}}");
    attrs.push("\"wasm-export-name\"=\"nazm_init\"".to_owned());

    let conserved = if shape.conserved {
        Some(
            symbol_of(native, "conserved")
                .ok_or("the unit defines no `conserved`")?
                .1,
        )
    } else {
        None
    };
    for (i, (name, n)) in shape.entries.iter().enumerate() {
        let (id, symbol) =
            symbol_of(native, name).ok_or_else(|| format!("the unit defines no `{name}`"))?;
        let Some(Ty::Enum(result_id)) = native.sigs.get(id.index()).map(|s| s.ret) else {
            return Err(format!("`{name}` does not return a `Result`"));
        };
        let result = lay
            .enums
            .get(&result_id.0)
            .cloned()
            .ok_or("the result has no layout")?;
        let table = home
            .enums
            .iter()
            .find(|e| e.id == result_id)
            .ok_or("the result has no table")?;
        // `Err` and `Ok`, by their tags: canonical order is by name.
        let tag_of = |v: &str| table.variants.iter().position(|x| x.source == v);
        let (Some(err), Some(ok)) = (tag_of("Err"), tag_of("Ok")) else {
            return Err(format!("`{name}` does not return a `Result`"));
        };
        let payload = |tag: usize| {
            PAYLOAD
                + result
                    .variants
                    .get(tag)
                    .and_then(|v| v.offsets.first().copied())
                    .unwrap_or(0)
        };
        let attr = 94 + i;
        let params: Vec<String> = (0..*n).map(|k| format!("i64 %a{k}")).collect();
        let _ = writeln!(
            out,
            "define i64 @nazm_call_{}(i64 %caller{}{}) #{attr} {{",
            encode(name),
            if params.is_empty() { "" } else { ", " },
            params.join(", ")
        );
        let _ = writeln!(out, "entry:");
        let _ = writeln!(out, "  %s = alloca [{size} x i8], align 8");
        let _ = writeln!(
            out,
            "  %r = alloca [{} x i8], align 8",
            result.lay.size.max(1)
        );
        load(&mut out, "s");
        let args: Vec<String> = (0..*n).map(|k| format!("i64 %a{k}")).collect();
        let _ = writeln!(
            out,
            "  call void @{symbol}(ptr %r, ptr %s, i64 %caller{}{})",
            if args.is_empty() { "" } else { ", " },
            args.join(", ")
        );
        let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
        let _ = writeln!(out, "  br i1 %failed, label %fail, label %done");
        let _ = writeln!(out, "fail:");
        let _ = writeln!(out, "  %m = load ptr, ptr @nz.errmsg");
        let _ = writeln!(out, "  %len = load i64, ptr @nz.errlen");
        let _ = writeln!(out, "  %len32 = trunc i64 %len to i32");
        let _ = writeln!(out, "  call void @nz.host_report(ptr %m, i32 %len32)");
        let _ = writeln!(out, "  ret i64 3");
        let _ = writeln!(out, "done:");
        let _ = writeln!(out, "  %tag = load i64, ptr %r");
        let _ = writeln!(out, "  %ok = icmp eq i64 %tag, {ok}");
        let _ = writeln!(out, "  br i1 %ok, label %commit, label %revert");
        let _ = writeln!(out, "revert:");
        let _ = writeln!(
            out,
            "  %cp = getelementptr i8, ptr %r, i64 {}",
            payload(err)
        );
        let _ = writeln!(out, "  %code = load i64, ptr %cp");
        let _ = writeln!(out, "  call void @nz.host_revert(i64 %code)");
        let _ = writeln!(out, "  ret i64 1");
        let _ = writeln!(out, "commit:");
        let _ = writeln!(
            out,
            "  %new = getelementptr i8, ptr %r, i64 {}",
            payload(ok)
        );
        if let Some(c) = &conserved {
            let _ = writeln!(out, "  %before = call i64 @{c}(ptr %s)");
            let _ = writeln!(out, "  %after = call i64 @{c}(ptr %new)");
            let _ = writeln!(out, "  %same = icmp eq i64 %before, %after");
            let _ = writeln!(out, "  br i1 %same, label %write, label %rejected");
            let _ = writeln!(out, "rejected:");
            let _ = writeln!(out, "  ret i64 2");
            let _ = writeln!(out, "write:");
        }
        store(&mut out, "n", "%new");
        let _ = writeln!(out, "  ret i64 0");
        let _ = writeln!(out, "}}");
        attrs.push(format!(
            "\"wasm-export-name\"=\"nazm_call_{}\"",
            encode(name)
        ));
    }
    attrs.push("\"wasm-import-module\"=\"nazm_host\" \"wasm-import-name\"=\"report\"".to_owned());
    for (i, a) in attrs.iter().enumerate() {
        let _ = writeln!(out, "attributes #{} = {{ {a} }}", 90 + i);
    }
    Ok(out)
}

/// The reference host for a contract module (`contract_host.mjs`), for Node: it keeps the state,
/// runs each transaction in a fresh instance, and prints `nazm.wasm-contract-run/1`.
pub(crate) const HOST: &str = r#"// Written by nazm contract --wasm (N71, docs/architecture.md §7.73).
// node contract_host.mjs module.wasm contract.json txs.txt
import { readFileSync } from 'node:fs';
const [wasm, meta, txs] = process.argv.slice(2);
const module = new WebAssembly.Module(readFileSync(wasm));
const contract = JSON.parse(readFileSync(meta, 'utf8'));
const granted = new Set(['state_get', 'state_set', 'revert', 'report', 'write']);
for (const i of WebAssembly.Module.imports(module)) {
  if (i.module !== 'nazm_host' || !granted.has(i.name)) {
    console.error(`nazm host: the module asks for ${i.module}.${i.name}, which this host does not grant`);
    process.exit(3);
  }
}
let state = contract.state.fields.map(() => 0n);
function run(name, args) {
  const pending = [...state];
  let code = null;
  let failure = null;
  let memory;
  const nazm_host = {
    state_get: (k) => pending[Number(k)],
    state_set: (k, v) => { pending[Number(k)] = v; },
    revert: (c) => { code = c; },
    report: (p, n) => { failure = Buffer.from(memory.buffer, p, n).toString(); },
    write: () => {},
  };
  const instance = new WebAssembly.Instance(module, { nazm_host });
  memory = instance.exports.memory;
  const status = Number(instance.exports[name](...args));
  return { status, pending, code, failure };
}
const init = run('nazm_init', []);
state = init.pending;
const runs = [];
readFileSync(txs, 'utf8').split('\n').forEach((text, i) => {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0 || words[0].startsWith('#')) return;
  const line = i + 1;
  const entry = contract.entrypoints.find((e) => e.name === words[1]);
  if (!entry) { runs.push({ line, entry: words[1], outcome: 'no-such-entrypoint' }); return; }
  const args = words.slice(2).map((w) => BigInt(w));
  if (args.length !== entry.args.length) { runs.push({ line, entry: entry.name, outcome: 'wrong-arity' }); return; }
  const caller = BigInt(words[0]);
  const r = run(entry.export, [caller, ...args]);
  const record = { line, caller: Number(caller), entry: entry.name };
  if (r.status === 0) { record.outcome = 'ok'; state = r.pending; }
  else if (r.status === 1) { record.outcome = 'revert'; record.code = String(r.code); }
  else if (r.status === 2) { record.outcome = 'rejected'; record.code = 'N0396'; }
  else { record.outcome = 'fail'; record.code = (r.failure ?? '').match(/N0\d{3}/)?.[0] ?? null; }
  runs.push(record);
});
const final = {};
contract.state.fields.forEach((f, k) => { final[f.name] = f.type === 'Bool' ? String(state[k] !== 0n) : String(state[k]); });
console.log(JSON.stringify({ schema: 'nazm.wasm-contract-run/1', transactions: runs, state: final }));
"#;

#[cfg(test)]
mod tests {
    use super::field_offsets;

    #[test]
    fn a_state_field_is_found_at_its_canonical_offset_not_its_declared_one() {
        // `State { owner: Int, alice: Int, paused: Bool }` laid out by name: alice, owner,
        // paused — owner is declared first and sits second.
        let fields = field_offsets(&[false, false, true], &[1, 0, 2], &[0, 8, 16]);
        assert_eq!(fields, [(8, false), (0, false), (16, true)]);
    }
}
