#!/usr/bin/env python3
"""`run.sh core` (R2, design D25): Core IR, function by function, then the document's other parts.

    core-ir.py want STATUS < CORE_IR      the reference's lines: `fails` for a program
                                          `nazm core-ir` refuses, else each function's identity,
                                          a tab, and its object, `digest` included, as compact JSON
                                          in the printed member order; then one line per other
                                          part of the document — `%entry`, `%modules`, `%types`,
                                          `%statics`, `%impls`, `%derived_hash` — a tab and its
                                          value, compact
    core-ir.py compare WANT GOT           compares them: prints `T C A P Q` — the reference's
                                          functions, those the Nazm lowering carried, those of
                                          them that agree, the parts the Nazm side carried, those
                                          of them that agree — and exits 1 when a carried function
                                          or part differs, or the two lists of functions do

The Nazm side (`diagnose --core`) prints `stops` where the checker stopped, `fails`, or a line per
function and per part: its value, or `- REASON` where the lowering stopped.
"""

import json
import sys

PARTS = ("entry", "modules", "types", "statics", "impls", "derived_hash")


def compact(v) -> str:
    return json.dumps(v, ensure_ascii=False, separators=(",", ":"))


def want(status: int) -> int:
    text = sys.stdin.read()
    if status != 0:
        print("fails")
        return 0
    doc = json.loads(text)
    out = []
    for f in doc["functions"]:
        out.append(f["def"] + "\t" + compact(f) + "\n")
    for p in PARTS:
        out.append("%" + p + "\t" + compact(doc[p]) + "\n")
    sys.stdout.write("".join(out))
    return 0


def lines(path: str) -> list:
    with open(path, encoding="utf-8", errors="surrogateescape") as f:
        return f.read().splitlines()


def compare(want_path: str, got_path: str) -> int:
    w, g = lines(want_path), lines(got_path)
    if g == ["stops"]:
        print("0 0 0 0 0")
        return 0
    if w == ["fails"] or g == ["fails"]:
        print("0 0 0 0 0")
        return 0 if w == g else 1
    wf = [x for x in w if not x.startswith("%")]
    gf = [x for x in g if not x.startswith("%")]
    wp = [x for x in w if x.startswith("%")]
    gp = [x for x in g if x.startswith("%")]
    total = len(wf)
    carried = agree = 0
    bad = False
    if [x.split("\t", 1)[0] for x in wf] != [x.split("\t", 1)[0] for x in gf]:
        bad = True
    for a, b in zip(wf, gf):
        body = b.split("\t", 1)[1] if "\t" in b else ""
        if body.startswith("- "):
            continue
        carried += 1
        if a == b:
            agree += 1
        else:
            bad = True
    parts = parts_agree = 0
    if [x.split("\t", 1)[0] for x in wp] != [x.split("\t", 1)[0] for x in gp]:
        bad = True
    for a, b in zip(wp, gp):
        body = b.split("\t", 1)[1] if "\t" in b else ""
        if body.startswith("- "):
            continue
        parts += 1
        if a == b:
            parts_agree += 1
        else:
            bad = True
    print(f"{total} {carried} {agree} {parts} {parts_agree}")
    return 1 if bad else 0


def main() -> int:
    if sys.argv[1] == "want":
        return want(int(sys.argv[2]))
    return compare(sys.argv[2], sys.argv[3])


if __name__ == "__main__":
    sys.exit(main())
