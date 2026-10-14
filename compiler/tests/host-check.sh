#!/bin/sh
# R2's check that the compiler still compiles itself, as far as this host may run it
# (`openspec/changes/r2-front-end/design.md`, D6).
#
#   1. The reference checks the compiler's source.
#   2. The reference builds C1 from it, with a heap limit.
#   3. C1 compiles the compiler's source — stage 2's emission — and clang assembles the IR.
#   4. C1 compiles every conformance case; each runs and prints what it expects. C1 compiles every
#      multi-module case, and refuses every refusal case with its code.
#
# What it does not do: run C2 or C3. Their IR is emitted by the Nazm compiler, which links no heap
# limit, so the fixpoint C2 = C3 stays `cargo xtask contained bootstrap`, the coordinator's.

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
nazm=${NAZM:-"$HOME/.local/nazm-0.10.0/bin/nazm"}
out=${OUT:-"$root/target/r2"}/host-check
heap=${R2_HEAP_LIMIT:-2G}
deadline=${R2_DEADLINE:-120}
core="$root/library/core/prelude.nz"
S="$root/compiler/emit.nz"

fail() { printf 'host-check: %s\n' "$*" >&2; exit 1; }

rm -rf "$out"
mkdir -p "$out"

"$nazm" check --no-cache "$S" >/dev/null || fail "the compiler source does not check under the reference"
echo "1. the reference checks compiler/emit.nz"

"$nazm" build --no-cache --heap-limit "$heap" "$S" -o "$out/c1" >/dev/null || fail "the reference could not build C1"
echo "2. the reference builds C1 (heap limit $heap)"

timeout "$deadline" "$out/c1" "$S" "$core" > "$out/c2.ll" || fail "C1 could not compile the compiler source"
clang -Wno-override-module -c -o "$out/c2.o" "$out/c2.ll" || fail "clang rejected the IR C1 emitted for C2"
echo "3. C1 compiles compiler/emit.nz ($(wc -l < "$out/c2.ll" | tr -d ' ') lines of IR) and clang assembles it"

corpus="$root/compiler/conformance"
checked=0
for case in "$corpus"/*.nz; do
  name=$(basename "$case" .nz)
  expected=$(cat "$corpus/$name.expected")
  timeout "$deadline" "$out/c1" "$case" "$core" > "$out/$name.ll" || fail "C1 refused $name"
  clang -Wno-override-module -o "$out/$name" "$out/$name.ll" || fail "clang rejected C1's IR for $name"
  got=$(timeout "$deadline" "$out/$name" 2>&1 || true)
  [ "$got" = "$expected" ] || fail "$name gave '$got', expected '$expected'"
  checked=$((checked + 1))
done
for case in "$corpus"/modules/*/; do
  name=$(basename "$case")
  expected=$(cat "$case/expected")
  timeout "$deadline" "$out/c1" "$case/main.nz" "$core" > "$out/$name.ll" || fail "C1 refused module case $name"
  clang -Wno-override-module -o "$out/$name" "$out/$name.ll" || fail "clang rejected C1's IR for $name"
  got=$(timeout "$deadline" "$out/$name" 2>&1 || true)
  [ "$got" = "$expected" ] || fail "$name gave '$got', expected '$expected'"
  checked=$((checked + 1))
done
refused=0
for case in "$corpus"/refused/*.nz; do
  name=$(basename "$case" .nz)
  code=$(sed -n '1s#^// expect: ##p' "$case")
  if timeout "$deadline" "$out/c1" "$case" "$core" > "$out/$name.ll" 2> "$out/$name.err"; then
    fail "C1 accepted refusal case $name"
  fi
  grep -q "error\[$code\]" "$out/$name.err" || fail "C1 refused $name without $code"
  refused=$((refused + 1))
done
echo "4. C1 compiles $checked conformance cases that print what they expect, and refuses $refused with their codes"
echo "host-check: passed (C2 = C3 is the contained bootstrap's)"
