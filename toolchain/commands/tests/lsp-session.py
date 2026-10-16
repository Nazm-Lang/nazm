# The scripted sessions run.sh's `lsp` part gives each input (design D16): the handshake, the file
# opened with its text as it is on disk and closed, `shutdown` and `exit` — each message framed as
# an editor frames it; with `hover`, hover at a few positions before the file is closed.
#
#   python3 -I lsp-session.py ABSOLUTE-PATH [hover] > session.bin
#
# Exits 1, writing nothing, for a file that is not UTF-8: no editor can send it.

import json
import sys


def frame(message):
    body = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body


def main():
    path = sys.argv[1]
    try:
        with open(path, "rb") as f:
            text = f.read().decode("utf-8")
    except (OSError, UnicodeDecodeError):
        sys.exit(1)
    uri = "file://" + "".join(
        c if (c.isascii() and (c.isalnum() or c in "-._~/")) else "".join("%%%02X" % b for b in c.encode("utf-8"))
        for c in path
    )
    doc = {"uri": uri}
    out = [
        frame({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}),
        frame({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        frame({"jsonrpc": "2.0", "method": "textDocument/didOpen",
               "params": {"textDocument": {"uri": uri, "languageId": "nazm", "version": 1, "text": text}}}),
    ]
    n = 2
    positions = [(0, 4), (1, 8), (2, 9), (3, 12), (5, 6), (8, 10), (13, 16)] if sys.argv[2:] == ["hover"] else []
    for line, character in positions:
        out.append(frame({"jsonrpc": "2.0", "id": n, "method": "textDocument/hover",
                          "params": {"textDocument": doc, "position": {"line": line, "character": character}}}))
        n += 1
    out.append(frame({"jsonrpc": "2.0", "method": "textDocument/didClose", "params": {"textDocument": doc}}))
    out.append(frame({"jsonrpc": "2.0", "id": n, "method": "shutdown"}))
    out.append(frame({"jsonrpc": "2.0", "method": "exit"}))
    sys.stdout.buffer.write(b"".join(out))


main()
