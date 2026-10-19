#!/bin/sh
# R2's front-end budgets (`openspec/changes/r2-front-end/design.md`, D12): the Nazm lexer and
# parser against the reference's, on the largest sources there are — wall time (best of three)
# and peak resident memory, one line per file and stage. Run after `run.sh`, which builds both
# sides into $OUT (default target/r2).
#
# The Nazm side is a program written in Nazm, so on the macOS host it runs only through `cargo xtask
# host-run` (R6, `docs/internal/runbook.md`), which takes no timing: its column is the peak resident memory
# the runner recorded, with `-` for the time.

set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/toolchain/lib/stage.sh"
stage_name=budget
stage_init "$root"
[ -n "${STAGE_PEAKS:-}" ] || { echo "budget: the peaks come from cargo xtask host-run, on macOS" >&2; exit 2; }
out=${OUT:-"$root/target/r2"}
ref=${R2_REF_TARGET:-"$HOME/.cache/nazm-target-r2"}/release/nazm-r2-refdump

measure() {
  best=""
  peak=0
  for _ in 1 2 3; do
    /usr/bin/time -l "$@" > /dev/null 2> "$out/budget.time" || true
    real=$(awk '/ real /{print $1}' "$out/budget.time")
    rss=$(awk '/maximum resident set size/{print $1}' "$out/budget.time")
    if [ -z "$best" ] || [ "$(echo "$real < $best" | bc)" = 1 ]; then best=$real; fi
    peak=$rss
  done
  printf '%ss %sMB' "$best" "$((peak / 1048576))"
}

# The Nazm side: the peak the runner recorded for the last of three runs.
measure_nazm() {
  for _ in 1 2 3; do
    : > "$STAGE_PEAKS"
    "$STAGE_RUN" 300 "$@" > /dev/null 2>&1 || true
  done
  peak=$(sed -n 's/.*peak resident \([0-9]*\) bytes.*/\1/p' "$STAGE_PEAKS" | tail -1)
  printf -- '- %sMB' "$(( ${peak:-0} / 1048576 ))"
}

printf 'file | stage | Nazm | reference\n'
for f in compiler/analyse.nz compiler/emit.nz compiler/syntax.nz; do
  for stage in tokens tree; do
    nazm=$(measure_nazm "$out/bin/$stage" "$root/$f")
    reference=$(measure "$ref" "$stage" "$root/$f")
    printf '%s | %s | %s | %s\n' "$f" "$stage" "$nazm" "$reference"
  done
done
