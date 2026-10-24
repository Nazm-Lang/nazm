# The scripted sessions run.sh's `lsp` part gives each input (design D16): the handshake, the file
# opened with its text as it is on disk and closed, `shutdown` and `exit` — each message framed as
# an editor frames it; with `hover`, hover at a few positions before the file is closed; with `nav`,
# what a client that reads versioned edits asks of up to forty of the file's names (definition, references
# with and without the declaration, prepareRename, rename); with `syms`, the document's symbols and the
# workspace's for a client that reads nested symbols; with `flat` and `symsflat`, the same for a client
# that reads neither.
#
#   python3 -I lsp-session.py ABSOLUTE-PATH [hover|nav|flat] > session.bin
#
# Exits 1, writing nothing, for a file that is not UTF-8: no editor can send it.

import json
import os
import re
import sys


def frame(message):
    body = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body


def utf16(s):
    return len(s.encode("utf-16-le")) // 2


def names(text, wanted):
    """Up to `wanted` identifiers of the text, evenly spread, as (line, character) inside each."""
    found = []
    for n, line in enumerate(text.split("\n")):
        for m in re.finditer(r"[A-Za-z_][A-Za-z0-9_]*", line):
            found.append((n, utf16(line[: m.start() + max(1, (m.end() - m.start()) // 2)]), m.group(0)))
    if len(found) <= wanted:
        return found
    step = len(found) / wanted
    return [found[int(i * step)] for i in range(wanted)]


def main():
    path = sys.argv[1]
    mode = sys.argv[2] if sys.argv[2:] else ""
    try:
        with open(path, "rb") as f:
            text = f.read().decode("utf-8")
    except (OSError, UnicodeDecodeError):
        sys.exit(1)
    # LSP_AS=FROM=TO names the file as the machine that will run the session sees it (a container mounts the checkout elsewhere).
    named = path
    if os.environ.get("LSP_AS"):
        old, new = os.environ["LSP_AS"].split("=", 1)
        if path.startswith(old):
            named = new + path[len(old):]
    uri = "file://" + "".join(
        c if (c.isascii() and (c.isalnum() or c in "-._~/")) else "".join("%%%02X" % b for b in c.encode("utf-8"))
        for c in named
    )
    doc = {"uri": uri}
    caps = {}
    if mode in ("nav", "syms", "actions"):
        caps = {
            "workspace": {"workspaceEdit": {"documentChanges": True}},
            "textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": True}},
        }
    out = [
        frame({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": caps}}),
        frame({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        frame({"jsonrpc": "2.0", "method": "textDocument/didOpen",
               "params": {"textDocument": {"uri": uri, "languageId": "nazm", "version": 1, "text": text}}}),
    ]
    n = 2

    def ask(method, params):
        nonlocal n
        out.append(frame({"jsonrpc": "2.0", "id": n, "method": method, "params": params}))
        n += 1

    if mode == "hover":
        for line, character in [(0, 4), (1, 8), (2, 9), (3, 12), (5, 6), (8, 10), (13, 16)]:
            ask("textDocument/hover", {"textDocument": doc, "position": {"line": line, "character": character}})
    elif mode == "hoverall":
        for line, character, _ in names(text, 60):
            ask("textDocument/hover", {"textDocument": doc, "position": {"line": line, "character": character}})
    elif mode == "hier":
        # Prepare at each of forty names; then the calls into and out of each function the text declares, with an item built as the
        # server writes it (a client sends back what `prepare` answered: `data` is what binds it; version 1 and generation 1 are the
        # document as opened). A stale generation and a position that declares nothing are asked once each.
        for line, character, _ in names(text, 40):
            ask("textDocument/prepareCallHierarchy", {"textDocument": doc, "position": {"line": line, "character": character}})
        raw = text.encode("utf-8")
        zero = {"line": 0, "character": 0}
        found = [m for m in re.finditer(rb"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", raw)][:30]
        for number, m in enumerate(found):
            start, end = m.start(1), m.end(1)
            for generation in ([1, 2] if number == 0 else [1]):
                item = {"name": m.group(1).decode(), "kind": 12, "uri": uri, "range": {"start": zero, "end": zero},
                        "selectionRange": {"start": zero, "end": zero},
                        "data": {"root": named, "version": 1, "generation": generation, "file": named, "start": start, "end": end}}
                ask("callHierarchy/incomingCalls", {"item": item})
                ask("callHierarchy/outgoingCalls", {"item": item})
        if found:
            item = {"name": "x", "kind": 12, "uri": uri, "range": {"start": zero, "end": zero}, "selectionRange": {"start": zero, "end": zero},
                    "data": {"root": named, "version": 1, "generation": 1, "file": named, "start": 0, "end": 1}}
            ask("callHierarchy/incomingCalls", {"item": item})
            item["data"] = None
            ask("callHierarchy/outgoingCalls", {"item": item})
    elif mode == "actions":
        lines = text.split("\n")
        # The whole document, then each line's start, middle and end: whatever a fix is attached to.
        ask("textDocument/codeAction", {"textDocument": doc, "range": {"start": {"line": 0, "character": 0}, "end": {"line": len(lines), "character": 0}}, "context": {"diagnostics": []}})
        ask("textDocument/codeAction", {"textDocument": doc, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "context": {"diagnostics": [], "only": ["refactor"]}})
        step = max(1, len(lines) // 40)
        for line in range(0, len(lines), step):
            width = utf16(lines[line])
            for character in sorted({0, width // 2, width}):
                at = {"line": line, "character": character}
                ask("textDocument/codeAction", {"textDocument": doc, "range": {"start": at, "end": at}, "context": {"diagnostics": [], "only": ["quickfix"]}})
    elif mode == "tokens":
        ask("textDocument/semanticTokens/full", {"textDocument": doc})
    elif mode in ("syms", "symsflat"):
        ask("textDocument/documentSymbol", {"textDocument": doc})
        ask("workspace/symbol", {"query": ""})
        ask("workspace/symbol", {"query": "a"})
    elif mode in ("nav", "flat"):
        for number, (line, character, word) in enumerate(names(text, 40 if mode == "nav" else 12)):
            at = {"textDocument": doc, "position": {"line": line, "character": character}}
            ask("textDocument/definition", at)
            ask("textDocument/references", dict(at, context={"includeDeclaration": True}))
            if mode == "nav":
                ask("textDocument/references", dict(at, context={"includeDeclaration": False}))
            if number % 5 == 0:
                # Each rename checks the program again: not at every name of a file of sixteen thousand lines.
                ask("textDocument/prepareRename", at)
                ask("textDocument/rename", dict(at, newName="Zrenamed" if word[0].isupper() else "zz_renamed"))
        if mode == "nav" and names(text, 1):
            line, character, _ = names(text, 1)[0]
            at = {"textDocument": doc, "position": {"line": line, "character": character}}
            for bad in ["", "_", "fn", "9x", "a b", "main"]:
                ask("textDocument/rename", dict(at, newName=bad))
    out.append(frame({"jsonrpc": "2.0", "method": "textDocument/didClose", "params": {"textDocument": doc}}))
    out.append(frame({"jsonrpc": "2.0", "id": n, "method": "shutdown"}))
    out.append(frame({"jsonrpc": "2.0", "method": "exit"}))
    sys.stdout.buffer.write(b"".join(out))


main()
