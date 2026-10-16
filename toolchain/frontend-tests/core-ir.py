#!/usr/bin/env python3
"""`run.sh core` (R2, design D25): Core IR, function by function, then the document's other parts.

    core-ir.py want STATUS < CORE_IR      the reference's lines: `fails` for a program
                                          `nazm core-ir` refuses, else each function's identity,
                                          a tab, and its object, `digest` included, as compact JSON
                                          in the printed member order; then one line per other
                                          part of the document — `%entry`, `%modules`, `%types`,
                                          `%statics`, `%impls`, `%derived_hash` — a tab and its
                                          value, compact
    core-ir.py compare WANT GOT DOC       compares them: prints `T C A P Q D E` — the
                                          reference's functions, those the Nazm lowering carried,
                                          those of them that agree, the parts the Nazm side
                                          carried, those of them that agree, the documents it
                                          printed whole (0 or 1) and those that are DOC (what
                                          `nazm core-ir` printed) byte for byte — and exits 1 when
                                          a carried function or part differs, the two lists of
                                          functions do, or a whole document is not DOC's bytes

The Nazm side (`diagnose --core`) prints `stops` where the checker stopped, `fails`, or a line per
function and per part: its value, or `- REASON` where the lowering stopped; and where it carried
every one, `%%document` on a line of its own and the whole document as `nazm core-ir` prints it.
"""

import json
import re
import sys

PARTS = ("entry", "modules", "types", "statics", "impls", "derived_hash")


def compact(v) -> str:
    # As `print.rs` quotes: only `\n`, `\r` and `\t` of the control characters by name, the rest
    # as `\u00XX` — where Python's encoder also names `\b` and `\f`.
    text = json.dumps(v, ensure_ascii=False, separators=(",", ":"))
    return re.sub(r"(?<!\\)((?:\\\\)*)\\([bf])",
                  lambda m: m.group(1) + ("\\u0008" if m.group(2) == "b" else "\\u000c"), text)


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


def compare(want_path: str, got_path: str, doc_path: str) -> int:
    w = lines(want_path)
    with open(got_path, "rb") as f:
        got = f.read()
    marker = b"\n%%document\n"
    document = None
    if marker in got:
        got, document = got.split(marker, 1)
        got += b"\n"
    g = got.decode("utf-8", errors="surrogateescape").splitlines()
    if g == ["stops"]:
        print("0 0 0 0 0 0 0")
        return 0
    if w == ["fails"] or g == ["fails"]:
        print("0 0 0 0 0 0 0")
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
    documents = documents_agree = 0
    if document is not None:
        documents = 1
        with open(doc_path, "rb") as f:
            if f.read() == document:
                documents_agree = 1
            else:
                bad = True
    print(f"{total} {carried} {agree} {parts} {parts_agree} {documents} {documents_agree}")
    return 1 if bad else 0


def main() -> int:
    if sys.argv[1] == "want":
        return want(int(sys.argv[2]))
    return compare(sys.argv[2], sys.argv[3], sys.argv[4])


if __name__ == "__main__":
    sys.exit(main())
