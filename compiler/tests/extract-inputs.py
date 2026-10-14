#!/usr/bin/env python3
"""Extract the Nazm programs written inside `crates/nazm-cli/tests/*.rs` into
`compiler/tests/inputs/rs/`, one file per distinct program (R2, `openspec/changes/r2-front-end/`).

A program is a Rust string literal — `"…"` with its escapes decoded, `r"…"` or `r#"…"#` — that
contains `fn ` and a `{`. A literal that is a `format!` template (it holds `{name}` or `{}`
placeholders beside `{{`) is skipped: it is not a program until Rust fills it in. Duplicates are
written once. The output is deterministic: files are named after the test file and the
literal's ordinal among that file's programs, and the folder is rewritten whole.

Usage: python3 -I compiler/tests/extract-inputs.py   (from the repository root)
"""
import hashlib
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "crates", "nazm-cli", "tests")
OUT = os.path.join(ROOT, "compiler", "tests", "inputs", "rs")


def literals(text):
    """Every string literal in Rust source, decoded, in order. Comments are skipped."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
            continue
        if c == "'":
            # A char literal ('"', '\'', 'x') or a lifetime ('a): skip either without
            # mistaking a quote inside it for a string.
            m = re.match(r"'(\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
            i += 1
            continue
        if c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            m = re.match(r"b?r(#*)\"", text[i:])
            if m:
                hashes = m.group(1)
                start = i + m.end()
                end = text.find('"' + hashes, start)
                if end < 0:
                    return
                yield text[start:end]
                i = end + 1 + len(hashes)
                continue
            if text.startswith('b"', i):
                i += 1
                c = '"'
        if c == '"':
            j = i + 1
            buf = []
            while j < n and text[j] != '"':
                if text[j] == "\\":
                    e = text[j + 1]
                    if e == "n":
                        buf.append("\n")
                    elif e == "t":
                        buf.append("\t")
                    elif e == "r":
                        buf.append("\r")
                    elif e == "0":
                        buf.append("\0")
                    elif e in "\\\"'":
                        buf.append(e)
                    elif e == "\n":
                        # A line continuation: the newline and the next line's indentation go.
                        k = j + 2
                        while k < n and text[k] in " \t\n\r":
                            k += 1
                        j = k
                        continue
                    elif e == "x":
                        buf.append(chr(int(text[j + 2 : j + 4], 16)))
                        j += 4
                        continue
                    elif e == "u":
                        close = text.find("}", j)
                        buf.append(chr(int(text[j + 3 : close], 16)))
                        j = close + 1
                        continue
                    else:
                        buf.append("\\" + e)
                    j += 2
                    continue
                buf.append(text[j])
                j += 1
            yield "".join(buf)
            i = j + 1
            continue
        i += 1


PLACEHOLDER = re.compile(r"(?<!\{)\{(?:[A-Za-z_][A-Za-z0-9_]*)?(?::[^{}]*)?\}(?!\})")


def is_program(s):
    if "fn " not in s or "{" not in s:
        return False
    if "{{" in s and PLACEHOLDER.search(s.replace("{{", "").replace("}}", "")):
        return False
    return True


def main():
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        if f.endswith(".nz"):
            os.remove(os.path.join(OUT, f))
    seen = set()
    written = 0
    for name in sorted(os.listdir(SRC)):
        if not name.endswith(".rs"):
            continue
        with open(os.path.join(SRC, name), encoding="utf-8") as fh:
            text = fh.read()
        k = 0
        for lit in literals(text):
            if not is_program(lit):
                continue
            digest = hashlib.sha256(lit.encode("utf-8")).hexdigest()
            if digest in seen:
                continue
            seen.add(digest)
            k += 1
            path = os.path.join(OUT, "%s-%03d.nz" % (name[:-3], k))
            with open(path, "w", encoding="utf-8", newline="") as out:
                out.write(lit)
            written += 1
    print("extracted %d programs into %s" % (written, os.path.relpath(OUT, ROOT)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
