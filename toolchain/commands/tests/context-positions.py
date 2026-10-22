#!/usr/bin/env python3
"""The byte offsets `agent.sh context` asks of a file: where each declaration's name starts and ends, a few uses of
those names, and the places that are no name (the start, a comment, the end, past the end).

    context-positions.py FILE [MAX]     one offset per line, at most MAX (default 8)

The choice is by regular expression and is only a choice of where to ask: what the answer is, is the reference's
and the port's to agree on.
"""
import re
import sys

path = sys.argv[1]
limit = int(sys.argv[2]) if len(sys.argv) > 2 else 8
data = open(path, 'rb').read()
text = data.decode('utf-8', errors='replace')
# Offsets are bytes: a map from character index to byte offset.
byte_at = []
n = 0
for ch in text:
    byte_at.append(n)
    n += len(ch.encode('utf-8'))
byte_at.append(n)
total = n

declared = []
for m in re.finditer(r'\b(?:fn|struct|enum|record|trait)\s+([A-Za-z_][A-Za-z_0-9]*)', text):
    declared.append((m.group(1), byte_at[m.start(1)], byte_at[m.end(1)]))

offsets = []
for name, s, e in declared:
    offsets.append(s)
    offsets.append(e)
uses = []
for name, s, e in declared:
    count = 0
    for m in re.finditer(r'\b' + re.escape(name) + r'\b', text):
        if byte_at[m.start()] != s:
            uses.append(byte_at[m.start()])
            count += 1
            if count == 2:
                break
# Interleave: the declarations first, then the uses, then the places that are no name.
chosen = []
seen = set()
def add(o):
    if o not in seen and 0 <= o <= total + 2:
        seen.add(o)
        chosen.append(o)
decl = [o for (_, s, e) in declared for o in (s, e)]
head = decl[:max(limit - 4, 1)]
for o in head:
    add(o)
for o in uses[:max(limit - len(head) - 3, 0)]:
    add(o)
add(0)
comment = text.find('//')
if comment >= 0:
    add(byte_at[comment + 1])
add(total)
add(total + 1)
add(total + 2)
for o in chosen[:limit + 4]:
    print(o)
