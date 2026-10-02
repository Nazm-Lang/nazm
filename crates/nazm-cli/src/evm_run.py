# Written by nazm contract --evm (N70, docs/architecture.md §7.72): runs a compiled contract in
# py-evm (docker/evm.Dockerfile) — deploys it, sends each transaction from its caller's address,
# and prints one line of JSON: each outcome, revert code, gas used, and the final storage.
# python3 evm_run.py DIR TXS
import json
import sys

from eth import constants
from eth.chains.base import MiningChain
from eth.db.atomic import AtomicDB
from eth.vm.forks import ShanghaiVM
from eth.vm.message import Message

d, txs = sys.argv[1], sys.argv[2]
deploy = bytes.fromhex(open(f"{d}/deploy.hex").read().strip())
abi = json.load(open(f"{d}/abi.json"))["entrypoints"]
storage = json.load(open(f"{d}/storage.json"))["fields"]

chain = MiningChain.configure(__name__="Nazm", vm_configuration=((0, ShanghaiVM),), chain_id=1)
genesis = dict(difficulty=0, gas_limit=30_000_000, timestamp=0, coinbase=constants.ZERO_ADDRESS, extra_data=b"", nonce=b"\0" * 8)
state = chain.from_genesis(AtomicDB(), genesis, {}).get_vm().state
contract = b"\0" * 19 + b"\x99"


def address(n):
    return int(n).to_bytes(20, "big")


def call(sender, data, code):
    ctx = state.get_transaction_context_class()(gas_price=0, origin=sender)
    msg = Message(gas=10_000_000, to=contract, sender=sender, value=0, data=data, code=code)
    snapshot = state.snapshot()
    c = state.computation_class.apply_message(state, msg, ctx)
    if c.is_error:
        state.revert(snapshot)
    else:
        state.commit(snapshot)
    return c


def signed(v):
    return v - (1 << 256) if v >= 1 << 255 else v


created = call(address(0), b"", deploy)
if created.is_error:
    print(json.dumps({"error": "deployment failed", "output": created.output.hex()}))
    sys.exit(3)
state.set_code(contract, created.output)
runtime = state.get_code(contract)
by_name = {e["name"]: e for e in abi}
runs = []
for line, text in enumerate(open(txs).read().splitlines(), 1):
    words = text.split()
    if not words or words[0].startswith("#"):
        continue
    try:
        caller, entry, args = int(words[0]), words[1], [int(w) for w in words[2:]]
    except (ValueError, IndexError):
        runs.append({"line": line, "outcome": "malformed"})
        continue
    e = by_name.get(entry)
    if e is None:
        runs.append({"line": line, "entry": entry, "outcome": "no-such-entrypoint"})
        continue
    if len(args) != len(e["args"]):
        runs.append({"line": line, "entry": entry, "outcome": "wrong-arity"})
        continue
    data = bytes.fromhex(e["selector"]) + b"".join((a % (1 << 256)).to_bytes(32, "big") for a in args)
    c = call(address(caller), data, runtime)
    record = {"line": line, "caller": caller, "entry": entry, "gas": c.get_gas_used()}
    if not c.is_error:
        record["outcome"] = "ok"
    elif len(c.output) == 5 and c.output.startswith(b"N0"):
        code = c.output.decode()
        record["outcome"] = "rejected" if code == "N0396" else "fail"
        record["code"] = code
    elif len(c.output) == 32:
        record["outcome"] = "revert"
        record["code"] = str(signed(int.from_bytes(c.output, "big")))
    else:
        record["outcome"] = "error"
        record["output"] = c.output.hex()
    runs.append(record)
final = {}
for f in storage:
    v = signed(state.get_storage(contract, int(f["slot"], 16)))
    final[f["name"]] = ("true" if v else "false") if f["type"] == "Bool" else str(v)
print(json.dumps({"schema": "nazm.evm-run/1", "transactions": runs, "state": final}))
