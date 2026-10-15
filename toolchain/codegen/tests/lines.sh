#!/bin/bash
# Line tables (design D10) and unwind information (5.2), against LLVM's.
#
#     NAZM=… toolchain/codegen/tests/lines.sh [FILE.nz ...]      (NZCG_TARGET as tests/run.sh)
#
# Needs a `nazm` whose `lir` has `--debug`. Per input: the generator's objects for
# `nazm lir --ops --debug` must pass `llvm-dwarfdump --verify`; every row of the line tables of
# LLVM's objects for `nazm build --debug`, as (file, line, column), must be a row of the
# generator's. Line 0 and column 0 are left out: a declaration's row has no column, and LLVM
# makes column-0 rows of its own where it merges two positions on one line. The generator may
# have more rows — a position whose only code LLVM folded into the next instruction, such as a
# constant, has code of its own here. And the program linked from them must run as the LLVM
# debug build does.
# Prints `N agree, M differ, K refused, S skipped`.
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
# The reference: the Rust `nazm` built from this checkout (rebuild it when library/, crates/ or
# the runtime change); the 0.10.0 release above only builds the generator.
REF=${NAZM_REF:-$HOME/.cache/nazm-target-track9/debug/nazm}
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
TARGET=${NZCG_TARGET:-aarch64-apple-darwin}
ARCH=${TARGET%%-*}
[ "$ARCH" = aarch64 ] && ARCH=arm64
NZCG=$OUT/nzcg
DWARFDUMP=${DWARFDUMP:-$(xcrun -f llvm-dwarfdump 2>/dev/null || echo /Library/Developer/CommandLineTools/usr/bin/llvm-dwarfdump)}
if ! "$REF" lir --help 2>/dev/null | grep -q -- '--debug'; then
  echo "skipped: $NAZM has no \`lir --debug\`"; exit 0
fi
mkdir -p "$OUT"
"$NAZM" build "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
if [ $# -gt 0 ]; then
  inputs=("$@")
else
  inputs=("$ROOT/bench/compare/nazm/fib.nz")
  for f in "$ROOT"/tests/compat/v1/run/*.nz "$HERE"/tests/programs/*.nz; do inputs+=("$f"); done
  for d in "$ROOT"/tests/compat/v1/run/*/; do [ -f "${d}main.nz" ] && inputs+=("${d}main.nz"); done
fi
limited() { perl -e 'alarm shift; exec @ARGV or exit 127' 60 "$@" </dev/null; }
# The rows of an object's line table: `file line column`, line or column 0 left out, unique.
rows() {
  "$DWARFDUMP" --debug-line "$1" | awk '
    /^file_names\[ *[0-9]+\]:/ { n = $0; gsub(/[^0-9]/, "", n) }
    /^include_directories\[ *[0-9]+\] = "/ { i = $0; sub(/\].*/, "", i); gsub(/[^0-9]/, "", i)
      s = $0; sub(/^[^"]*"/, "", s); sub(/"$/, "", s); dir[i] = s }
    /^ +name: "/ { s = $0; sub(/^ +name: "/, "", s); sub(/"$/, "", s); name[n] = s }
    /^ +dir_index: [0-9]+/ { if ($2 > 0) name[n] = dir[$2] "/" name[n] }
    /^0x[0-9a-f]+ +[0-9]+ +[0-9]+ +[0-9]+/ { if ($2 != 0 && $3 != 0) print name[$4], $2, $3 }' | sort -u
}

agree=0; differ=0; refused=0; skipped=0
for src in "${inputs[@]}"; do
  rel=${src#"$ROOT"/}
  W=$OUT/lines-$TARGET/$(echo "$rel" | tr '/.' '__')
  rm -rf "$W"; mkdir -p "$W/obj" "$W/gen"
  if ! "$REF" build --target "$TARGET" --debug "$src" -o "$W/ref" >/dev/null 2>&1 ||
     ! "$REF" build --target "$TARGET" --debug --objects "$W/obj" "$src" >/dev/null 2>&1 ||
     ! "$REF" lir --target "$TARGET" --ops --debug "$src" >"$W/lir.txt" 2>/dev/null; then
    skipped=$((skipped+1)); echo "skipped  $rel"; continue
  fi
  "$NZCG" --target "$TARGET" "$W/lir.txt" "$W/gen" >/dev/null 2>"$W/gen.err"
  st=$?
  if [ $st -eq 3 ]; then refused=$((refused+1)); echo "refused  $rel: $(head -1 "$W/gen.err")"; continue; fi
  if [ $st -ne 0 ]; then differ=$((differ+1)); echo "differ   $rel: the generator failed"; continue; fi
  why=""
  k=0; list=()
  for o in $(sed -n 's/^objects //p' "$W/obj/link.txt"); do
    if [[ $o =~ -[mg][0-9]+\.o$ ]]; then
      ours=$W/gen/unit-$k.o; theirs=$W/obj/$o
      "$DWARFDUMP" --verify "$ours" >"$W/verify-$k.txt" 2>&1 || why="$why verify($o)"
      rows "$ours" >"$W/ours-$k.rows"
      rows "$theirs" >"$W/theirs-$k.rows"
      missing=$(comm -13 "$W/ours-$k.rows" "$W/theirs-$k.rows" | wc -l | tr -d ' ')
      [ "$missing" = 0 ] || why="$why rows($o: $missing of LLVM's missing)"
      cp "$ours" "$theirs"; k=$((k+1))
    fi
    list+=("$W/obj/$o")
  done
  if cc -arch "$ARCH" -o "$W/gen.exe" "${list[@]}" $(sed -n 's/^libraries //p' "$W/obj/link.txt") >"$W/link.log" 2>&1; then
    (cd "$W" && limited ./ref >ref.out 2>&1; echo $? >>ref.out)
    (cd "$W" && limited ./gen.exe >gen.out 2>&1; echo $? >>gen.out)
    cmp -s "$W/ref.out" "$W/gen.out" || why="$why run"
  else
    why="$why link"
  fi
  if [ -z "$why" ]; then agree=$((agree+1)); else differ=$((differ+1)); echo "differ   $rel:$why"; fi
done
echo "$agree agree, $differ differ, $refused refused, $skipped skipped"
[ $differ -eq 0 ]
