#!/usr/bin/env python3
"""`run.sh core` (R2, design D25): Core IR, function by function.

    core-ir.py want STATUS < CORE_IR      the reference's lines: `fails` for a program
                                          `nazm core-ir` refuses, else each function's identity,
                                          a tab, and its object without `digest` as compact JSON
                                          in the printed member order
    core-ir.py compare WANT GOT           compares them: prints `T C A` — the reference's
                                          functions, those the Nazm lowering carried, those of
                                          them that agree — and exits 1 when a carried one
                                          differs, or the two lists of functions do

The Nazm side (`diagnose --core`) prints `stops` where the checker stopped, `fails`, or a line per
function: its object, or `- REASON` where the lowering stopped.
"""

import json
import sys


def want(status: int) -> int:
    text = sys.stdin.read()
    if status != 0:
        print("fails")
        return 0
    doc = json.loads(text)
    out = []
    for f in doc["functions"]:
        f.pop("digest", None)
        out.append(f["def"] + "\t" + json.dumps(f, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.write("".join(out))
    return 0


def lines(path: str) -> list:
    with open(path, encoding="utf-8", errors="surrogateescape") as f:
        return f.read().splitlines()


def compare(want_path: str, got_path: str) -> int:
    w, g = lines(want_path), lines(got_path)
    if g == ["stops"]:
        print("0 0 0")
        return 0
    if w == ["fails"] or g == ["fails"]:
        print("0 0 0")
        return 0 if w == g else 1
    total = len(w)
    carried = agree = 0
    bad = False
    if [x.split("\t", 1)[0] for x in w] != [x.split("\t", 1)[0] for x in g]:
        bad = True
    for a, b in zip(w, g):
        body = b.split("\t", 1)[1] if "\t" in b else ""
        if body.startswith("- "):
            continue
        carried += 1
        if a == b:
            agree += 1
        else:
            bad = True
    print(f"{total} {carried} {agree}")
    return 1 if bad else 0


def main() -> int:
    if sys.argv[1] == "want":
        return want(int(sys.argv[2]))
    return compare(sys.argv[2], sys.argv[3])


if __name__ == "__main__":
    sys.exit(main())
