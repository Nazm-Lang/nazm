//! The WebAssembly contract adapter (N71, `docs/architecture.md` §7.73): LLVM text appended to the
//! contract's own unit, exporting `nazm_init` and `nazm_call_<entry>` and moving the state across
//! the host imports `nazm_host.state_get`, `state_set` and `revert`.
//!
//! Everything it needs is read from the unit the ordinary backend emitted: each function's
//! definition line names its symbol and its `State` and `Result` types, and the `State` type's
//! definition gives its fields' layout order — by name, as every record's is (§7.9).

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

/// The definition line of the function named `name` in `unit`: its symbol, result type and
/// parameter types.
fn define<'a>(unit: &'a str, name: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let suffix = format!(".{}(", encode(name));
    unit.lines().find_map(|l| {
        if !l.starts_with("define ") || !l.contains(&suffix) {
            return None;
        }
        let at = l.find(" @nz.m.")?;
        let open = l[at..].find('(')? + at;
        let close = l.rfind(')')?;
        let ret_start = l[..at].rfind(' ').map_or(0, |i| i + 1);
        // The result type may itself contain spaces (`{ i64, … }`): it is what lies between the
        // linkage keyword and the symbol.
        let head = &l[..at];
        let ret = head
            .strip_prefix("define internal ")
            .or_else(|| head.strip_prefix("define hidden "))
            .or_else(|| head.strip_prefix("define "))
            .unwrap_or(&head[ret_start..]);
        // The symbol without its `@`.
        Some((&l[at + 2..open], ret, &l[open + 1..close]))
    })
}

/// The adapter for `shape`, to append to `unit`.
///
/// # Errors
///
/// A definition the adapter needs is not in the unit.
pub(crate) fn adapter(unit: &str, shape: &Shape) -> Result<String, String> {
    let (init, state_ty, _) = define(unit, "init").ok_or("the unit defines no `init`")?;
    let state_ty = state_ty.to_owned();
    // Layout order is by field name.
    let mut by_name: Vec<&str> = shape.fields.iter().map(|(n, _)| n.as_str()).collect();
    by_name.sort_unstable();
    let layout = |name: &str| by_name.iter().position(|n| *n == name).unwrap_or(0);
    let ty = |is_bool: bool| if is_bool { "i1" } else { "i64" };
    // Attributes are numbered from 90, clear of the unit's own.
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
    if !unit.contains("@nz.errmsg = external global") {
        let _ = writeln!(out, "@nz.errmsg = external global ptr");
    }
    if !unit.contains("@nz.errlen = external global") {
        let _ = writeln!(out, "@nz.errlen = external global i64");
    }

    // Read the state from the host: one field per index, into its layout position.
    let load = |out: &mut String, var: &str| {
        let mut agg = String::from("poison");
        for (k, (name, is_bool)) in shape.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "  %{var}.raw{k} = call i64 @nz.host_state_get(i64 {k})"
            );
            let v = if *is_bool {
                let _ = writeln!(out, "  %{var}.v{k} = icmp ne i64 %{var}.raw{k}, 0");
                format!("%{var}.v{k}")
            } else {
                format!("%{var}.raw{k}")
            };
            let _ = writeln!(
                out,
                "  %{var}.a{k} = insertvalue {state_ty} {agg}, {} {v}, {}",
                ty(*is_bool),
                layout(name)
            );
            agg = format!("%{var}.a{k}");
        }
        agg
    };
    // Write a state back: every field, by index.
    let store = |out: &mut String, var: &str, from: &str| {
        for (k, (name, is_bool)) in shape.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "  %{var}.x{k} = extractvalue {state_ty} {from}, {}",
                layout(name)
            );
            let v = if *is_bool {
                let _ = writeln!(out, "  %{var}.w{k} = zext i1 %{var}.x{k} to i64");
                format!("%{var}.w{k}")
            } else {
                format!("%{var}.x{k}")
            };
            let _ = writeln!(out, "  call void @nz.host_state_set(i64 {k}, i64 {v})");
        }
    };

    let _ = writeln!(out, "define i64 @nazm_init() #93 {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %s = call {state_ty} @{init}()");
    store(&mut out, "i", "%s");
    let _ = writeln!(out, "  ret i64 0");
    let _ = writeln!(out, "}}");
    attrs.push("\"wasm-export-name\"=\"nazm_init\"".to_owned());

    let conserved = if shape.conserved {
        Some(
            define(unit, "conserved")
                .ok_or("the unit defines no `conserved`")?
                .0,
        )
    } else {
        None
    };
    for (i, (name, n)) in shape.entries.iter().enumerate() {
        let (symbol, result_ty, _) =
            define(unit, name).ok_or_else(|| format!("the unit defines no `{name}`"))?;
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
        let s = load(&mut out, "s");
        let args: Vec<String> = (0..*n).map(|k| format!("i64 %a{k}")).collect();
        let _ = writeln!(
            out,
            "  %r = call {result_ty} @{symbol}({state_ty} {s}, i64 %caller{}{})",
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
        // `Err` is the canonical first variant, `Ok` the second: by name.
        let _ = writeln!(out, "  %tag = extractvalue {result_ty} %r, 0");
        let _ = writeln!(out, "  %ok = icmp eq i64 %tag, 1");
        let _ = writeln!(out, "  br i1 %ok, label %commit, label %revert");
        let _ = writeln!(out, "revert:");
        let _ = writeln!(out, "  %code = extractvalue {result_ty} %r, 1, 0");
        let _ = writeln!(out, "  call void @nz.host_revert(i64 %code)");
        let _ = writeln!(out, "  ret i64 1");
        let _ = writeln!(out, "commit:");
        let _ = writeln!(out, "  %new = extractvalue {result_ty} %r, 2, 0");
        if let Some(c) = conserved {
            let _ = writeln!(out, "  %before = call i64 @{c}({state_ty} {s})");
            let _ = writeln!(out, "  %after = call i64 @{c}({state_ty} %new)");
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
