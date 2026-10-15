"""`nazm lir`'s output with each unit's instruction text (`lir`) taken out: the tables alone, which
`pipeline lir` prints until task 6.3 adds the text. Re-serialised as `serde_json` writes it
(compact, keys sorted, UTF-8 as itself), so every byte of the tables is still compared."""
import json
import sys

doc = json.load(sys.stdin)
for unit in doc.get("units", []):
    unit.pop("lir", None)
sys.stdout.write(json.dumps(doc, separators=(",", ":"), sort_keys=True, ensure_ascii=False) + "\n")
