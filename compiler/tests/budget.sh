#!/bin/sh
# R2's front-end budgets (`openspec/changes/r2-front-end/design.md`, D12): the Nazm lexer and
# parser against the reference's, on the largest sources there are — wall time (best of three)
# and peak resident memory, one line per file and stage. Run after `run.sh`, which builds both
# sides into $OUT (default target/r2).

set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
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

printf 'file | stage | Nazm | reference\n'
for f in compiler/analyse.nz compiler/emit.nz compiler/syntax.nz; do
  for stage in tokens tree; do
    nazm=$(measure "$out/bin/$stage" "$root/$f")
    reference=$(measure "$ref" "$stage" "$root/$f")
    printf '%s | %s | %s | %s\n' "$f" "$stage" "$nazm" "$reference"
  done
done
