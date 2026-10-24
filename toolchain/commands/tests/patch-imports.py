# The first file `FILE` imports (`use "x.nz"`, relative to its directory), and three of that file's names with the new name to give each: `OTHER BYTE NEWNAME` lines, for
# `patch-gen.sh`'s cases of a file other than the root. Nothing for a file with no import that is a file.
import os
import re
import sys


def main():
    path = sys.argv[1]
    try:
        with open(path, "rb") as f:
            text = f.read().decode("utf-8")
    except (OSError, UnicodeDecodeError):
        return
    for m in re.finditer(r'use\s+"([^"@][^"]*)"', text):
        other = os.path.normpath(os.path.join(os.path.dirname(path), m.group(1)))
        if other == path or not os.path.isfile(other):
            continue
        try:
            with open(other, "rb") as f:
                body = f.read().decode("utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        found = list(re.finditer(r"[A-Za-z_][A-Za-z0-9_]*", body))
        if not found:
            continue
        picked = found if len(found) <= 3 else [found[int(i * len(found) / 3)] for i in range(3)]
        for n in picked:
            inside = n.start() + max(1, (n.end() - n.start()) // 2)
            print(other, len(body[:inside].encode("utf-8")), "Zrenamed" if n.group(0)[0].isupper() else "zz_renamed")
        return


main()
