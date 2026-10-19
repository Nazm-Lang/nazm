#!/bin/sh
# Repair a Nazm file as far as the compiler can, and report the rest.
#
#   scripts/repair.sh FILE [FILE...]
#
# The point of this script is that it is the *whole* workflow, runnable unattended, with
# nothing left for a person to glue together:
#
#   1. Apply every automatic fix, repeatedly, until a pass changes nothing. One round is
#      not enough — an applied fix can reveal the next error, which is the normal shape
#      of a repair and the reason a single `nazm fix --apply` is not a workflow.
#   2. Lay the file out canonically.
#   3. Report whatever is left, as JSON, so the caller can decide what to do with it.
#
# What it will not do is guess. `nazm fix --apply` applies only fixes marked `automatic`;
# anything marked `needs_review` is printed with the condition it rests on and left
# alone. See `docs/user/diagnostics.md`.
#
# Exit status: 0 when nothing is left to report, 1 when something is. Both are ordinary
# outcomes — a file this cannot fully repair is the common case, and the diagnostics on
# stdout are the result, not a failure of the script.

set -eu

nazm=${NAZM:-nazm}
command -v "$nazm" >/dev/null 2>&1 || {
    printf 'repair: %s is not on PATH; set NAZM to the binary\n' "$nazm" >&2
    exit 4
}
[ "$#" -gt 0 ] || { printf 'usage: repair.sh FILE [FILE...]\n' >&2; exit 2; }

# A bound rather than `while true`. Each round must make a file strictly better, so a
# round that changes nothing ends the loop — but a bug that made two fixes undo each
# other would otherwise loop forever, and a repair tool that hangs is worse than one
# that stops and says how far it got.
rounds=0
limit=10
while [ "$rounds" -lt "$limit" ]; do
    before=$(cat "$@")
    for file do
        # Not `set -e`-fatal: a file with errors left is the expected outcome, and the
        # loop's business is whether anything *changed*.
        "$nazm" fix --apply "$file" >/dev/null 2>&1 || true
    done
    after=$(cat "$@")
    rounds=$((rounds + 1))
    [ "$before" != "$after" ] || break
done

if [ "$rounds" -ge "$limit" ]; then
    printf 'repair: still changing after %s rounds; stopping rather than looping\n' "$limit" >&2
    exit 3
fi

# Layout last. Formatting a file that does not lex is refused, so doing it first would
# skip exactly the files a repair just made lexable.
for file do
    "$nazm" fmt "$file" >/dev/null || true
done

left=0
for file do
    "$nazm" check --json "$file" 2>&1 >/dev/null | grep '^{' || true
    "$nazm" check "$file" >/dev/null 2>&1 || left=1
done

printf 'repair: %s round(s) of fixes across %s file(s)\n' "$rounds" "$#" >&2
exit "$left"
