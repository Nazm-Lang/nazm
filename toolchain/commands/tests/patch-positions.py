# The positions `run.sh patch` asks `nazm patch` about in one input (design D24): up to six names of the file, for
# `patch rename` (each with the new name to give it), and up to three diagnostics that carry a fix, for `patch fix`.
#
#   nazm check --json FILE | python3 -I patch-positions.py FILE
#
# Prints `rename BYTE NEWNAME` and `fix BYTE` lines; nothing for a file that is not UTF-8.

import json
import re
import sys


def main():
    path = sys.argv[1]
    try:
        with open(path, "rb") as f:
            text = f.read().decode("utf-8")
    except (OSError, UnicodeDecodeError):
        return
    found = [m for m in re.finditer(r"[A-Za-z_][A-Za-z0-9_]*", text)]
    wanted = 6
    picked = found if len(found) <= wanted else [found[int(i * len(found) / wanted)] for i in range(wanted)]
    for m in picked:
        inside = m.start() + max(1, (m.end() - m.start()) // 2)
        name = "Zrenamed" if m.group(0)[0].isupper() else "zz_renamed"
        print("rename", len(text[:inside].encode("utf-8")), name)
    fixes = []
    for line in sys.stdin:
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict) and d.get("fixes"):
            fixes.append(d["primary"]["span"]["start"])
    for start in fixes[:3]:
        print("fix", start)


main()
