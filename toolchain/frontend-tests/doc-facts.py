#!/usr/bin/env python3
"""The reference side of `run.sh facts` (R2, design D24).

Reads `nazm doc FILE`'s standard output and exit status and prints what `compiler/facts-dump.nz`
prints from the Nazm front end's facts: one line per field of every item,
`MODULE<TAB>ID<TAB>FIELD=VALUE`, the value as compact JSON with sorted keys, the derived `anchor`
and `description` left out; `fails` for a program `nazm doc` refuses.

    doc-facts.py STATUS < STDOUT
"""

import json
import sys


def main() -> int:
    status = int(sys.argv[1])
    text = sys.stdin.read()
    if status != 0:
        print("fails")
        return 0
    doc = json.loads(text)
    out = []
    for module in doc["modules"]:
        key = module["module"]
        for item in module["items"]:
            for name in sorted(item):
                if name in ("anchor", "description"):
                    continue
                value = json.dumps(item[name], ensure_ascii=False, separators=(",", ":"), sort_keys=True)
                out.append(f"{key}\t{item['id']}\t{name}={value}\n")
    sys.stdout.write("".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
