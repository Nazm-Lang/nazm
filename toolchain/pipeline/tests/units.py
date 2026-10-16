"""Compare `pipeline runtime`'s units with the ones `nazm build --emit-ir` wrote.

    python3 units.py GOT.txt BASE

GOT.txt is `pipeline runtime`'s output (each unit after `;;; unit NAME`); BASE is the `-o` path
`nazm build --emit-ir` was given, beside which it wrote `BASE.NAME.ll` for every unit. Compares the
runtime's units — `runtime`, `entry`, `switch`, `init`, `stack`, `errno`, `tls`, `os`, `heap`, `small` — byte for
byte, and that the build wrote exactly the ones the pipeline did. Prints `agree`, or `differ` and
the first differing unit and line."""
import os
import sys

RUNTIME = ["runtime", "switch", "init", "stack", "errno", "tls", "os", "heap", "small", "entry"]
got_path, base = sys.argv[1], sys.argv[2]
raw = open(got_path).read()
got = {}
order = []
mark = ";;; unit "
at = raw.find(mark)
while at >= 0:
    eol = raw.index("\n", at)
    name = raw[at + len(mark):eol]
    nxt = raw.find("\n" + mark, eol)
    end = len(raw) if nxt < 0 else nxt + 1
    got[name] = raw[eol + 1:end]
    order.append(name)
    at = -1 if nxt < 0 else nxt + 1
want_names = [n for n in RUNTIME if os.path.exists(base + "." + n + ".ll")]
if sorted(want_names) != sorted(order):
    print("differ units: got " + ",".join(order) + " want " + ",".join(want_names))
    sys.exit(1)
for n in order:
    want = open(base + "." + n + ".ll").read()
    g = got[n]
    if g != want:
        gl, wl = g.split("\n"), want.split("\n")
        for i in range(max(len(gl), len(wl))):
            a = gl[i] if i < len(gl) else "<end>"
            b = wl[i] if i < len(wl) else "<end>"
            if a != b:
                print("differ " + n + ":" + str(i + 1) + ": got " + a[:100] + " | want " + b[:100])
                sys.exit(1)
        print("differ " + n + ": trailing bytes")
        sys.exit(1)
print("agree")
