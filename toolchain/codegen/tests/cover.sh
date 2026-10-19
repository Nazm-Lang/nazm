#!/bin/bash
# How much of LIR the differential suite exercises (design D12).
#
#     toolchain/codegen/tests/cover.sh            (after tests/run.sh; NZCG_TARGET as it reads it)
#
# Reads the LIR of every input tests/run.sh last ran (its work directory), takes the forms each
# program uses (`nzcg --forms`: an operation with the classes it is applied to), and compares them
# with every form LIR admits (`nzcg --universe`). Prints the forms no input uses, then
# `covered N of M forms`. A form is counted only from inputs that agreed with LLVM in that run.
set -u
# The generator is a program written in Nazm: built by the checkout's `nazm` with a heap limit of at
# most 2 GiB and run through `cargo xtask host-run` on macOS (one at a time) or contained
# (`toolchain/lib/stage.sh`; the runbook's R6 rule).
stage_name=codegen-cover
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
NAZM=${NAZM:-$(stage_checkout_nazm "$stage_root")}
HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
WORK=$OUT/work-${NZCG_TARGET:-aarch64-apple-darwin}
NZCG=$OUT/nzcg
mkdir -p "$OUT"
stage_build "$NAZM" "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
"$STAGE_RUN" 300 "$NZCG" --universe | sort -u >"$OUT/universe.txt"
: >"$OUT/seen.all"
for d in "$WORK"/*/; do
  [ -f "$d/gen.status" ] && cmp -s "$d/ref.status" "$d/gen.status" &&
    cmp -s "$d/ref.stdout" "$d/gen.stdout" && cmp -s "$d/ref.stderr" "$d/gen.stderr" || continue
  "$STAGE_RUN" 300 "$NZCG" --forms "$d/lir.txt" >>"$OUT/seen.all"
done
sort -u "$OUT/seen.all" >"$OUT/seen.txt"
extra=$(comm -13 "$OUT/universe.txt" "$OUT/seen.txt")
[ -n "$extra" ] && { echo "forms outside the universe (a defect of forms.nz):"; echo "$extra"; }
echo "not covered:"
comm -23 "$OUT/universe.txt" "$OUT/seen.txt" | sed 's/^/  /'
echo "covered $(comm -12 "$OUT/universe.txt" "$OUT/seen.txt" | wc -l | tr -d ' ') of $(wc -l <"$OUT/universe.txt" | tr -d ' ') forms"
