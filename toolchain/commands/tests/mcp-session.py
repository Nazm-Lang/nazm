#!/usr/bin/env python3
"""One MCP session over stdio, for tests/run.sh's `mcp` part.

    mcp-session.py PROGRAM [ARGS...] < messages.jsonl

Starts PROGRAM with ARGS (`nazm-mcp --root X`, or `nazm mcp --root X`), writes every line of
standard input to it, closes its input, and prints what it answered: one line per response,
ordered by request id (the reference answers a tool call when its analysis is done, so two calls
need not come back in the order they were asked), then `exit N` on a line of its own and, where
it wrote any, its standard error after `stderr:`. The standard error is only printed for a
program that did not end with status 0: a server's log is not its protocol.
"""
import json
import subprocess
import sys


def main():
    data = sys.stdin.buffer.read()
    p = subprocess.run(sys.argv[1:], input=data, capture_output=True, timeout=300)
    rows = []
    for n, line in enumerate(p.stdout.decode('utf-8', 'replace').split('\n')):
        if not line:
            continue
        try:
            key = json.loads(line).get('id')
        except Exception:
            key = None
        rows.append(((0, key, n) if isinstance(key, int) else (1, 0, n), line))
    rows.sort(key=lambda r: r[0])
    out = [r[1] for r in rows]
    out.append('exit %d' % p.returncode)
    if p.returncode != 0:
        out.append('stderr: ' + p.stderr.decode('utf-8', 'replace').strip())
    sys.stdout.write('\n'.join(out) + '\n')


main()
