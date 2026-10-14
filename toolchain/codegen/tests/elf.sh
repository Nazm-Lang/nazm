#!/bin/bash
# The ELF objects' structural check (design D11): this host links and runs no Linux program, so
# for `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` the generator's objects are
# checked, not run — the result is *unexecuted*, never *agree*.
#
#     toolchain/codegen/tests/elf.sh [FILE.nz ...]
#
# Per input and target: `llvm-objdump` reads the object without a warning and disassembles it
# with no unknown instruction; it defines the same global symbols, with the same visibility, as
# LLVM's object for the unit, and every local one LLVM's does (LLVM names no private data, so
# the generator may define more); every relocation names a symbol of the object; its
# `.eh_frame` (5.2) reads back with an FDE for every function. With a `nazm` whose `lir` has
# `--debug`, the objects carry line tables (D10) too, which must pass `llvm-dwarfdump --verify`
# and hold every row of LLVM's (as tests/lines.sh compares them). Prints
# `N structurally sound, M not, K refused` per target.
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
NZCG=$OUT/nzcg
OBJDUMP=${OBJDUMP:-$(xcrun -f llvm-objdump 2>/dev/null || echo /Library/Developer/CommandLineTools/usr/bin/llvm-objdump)}
DWARFDUMP=${DWARFDUMP:-$(xcrun -f llvm-dwarfdump 2>/dev/null || echo /Library/Developer/CommandLineTools/usr/bin/llvm-dwarfdump)}
DEBUG=()
"$NAZM" lir --help 2>/dev/null | grep -q -- '--debug' && DEBUG=(--debug)
mkdir -p "$OUT"
"$NAZM" build "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
if [ $# -gt 0 ]; then
  inputs=("$@")
else
  inputs=("$ROOT/bench/compare/nazm/fib.nz")
  for f in "$ROOT"/tests/compat/v1/run/*.nz "$HERE"/tests/programs/*.nz; do inputs+=("$f"); done
  for d in "$ROOT"/tests/compat/v1/run/*/; do [ -f "${d}main.nz" ] && inputs+=("${d}main.nz"); done
fi

# Defined symbols as `name binding visibility`, and undefined ones as `name`.
defs() { "$OBJDUMP" -t "$1" | awk '$2 ~ /^[lg]$/ && $0 !~ /\*UND\*/ && $NF !~ /^\$/ && $NF !~ /^\.L/ && $3 !~ /^d/ && $3 != "df" {
  hid = ($(NF-1) == ".hidden") ? "hidden" : "default"; print $NF, $2, hid }' | sort; }
globals() { defs "$1" | awk '$2 == "g"'; }
locals() { defs "$1" | awk '$2 == "l"'; }
# Line-table rows as tests/lines.sh reads them.
rows() {
  "$DWARFDUMP" --debug-line "$1" | awk '
    /^include_directories\[ *[0-9]+\] = "/ { i = $0; sub(/\].*/, "", i); gsub(/[^0-9]/, "", i)
      s = $0; sub(/^[^"]*"/, "", s); sub(/"$/, "", s); dir[i] = s }
    /^file_names\[ *[0-9]+\]:/ { n = $0; gsub(/[^0-9]/, "", n) }
    /^ +name: "/ { s = $0; sub(/^ +name: "/, "", s); sub(/"$/, "", s); name[n] = s }
    /^ +dir_index: [0-9]+/ { if ($2 > 0) name[n] = dir[$2] "/" name[n] }
    /^0x[0-9a-f]+ +[0-9]+ +[0-9]+ +[0-9]+/ { if ($2 != 0 && $3 != 0) print name[$4], $2, $3 }' | sort -u
}

status=0
for target in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  good=0; bad=0; refused=0
  for src in "${inputs[@]}"; do
    rel=${src#"$ROOT"/}
    W=$OUT/elf-$target/$(echo "$rel" | tr '/.' '__')
    rm -rf "$W"; mkdir -p "$W/obj" "$W/gen"
    if ! "$NAZM" build --target "$target" ${DEBUG[@]+"${DEBUG[@]}"} --objects "$W/obj" "$src" >"$W/obj.build" 2>&1 ||
       ! "$NAZM" lir --target "$target" --ops ${DEBUG[@]+"${DEBUG[@]}"} "$src" >"$W/lir.txt" 2>/dev/null; then
      continue
    fi
    "$NZCG" --target "$target" "$W/lir.txt" "$W/gen" >/dev/null 2>"$W/gen.err"
    st=$?
    if [ $st -eq 3 ]; then refused=$((refused+1)); echo "refused  $target $rel: $(head -1 "$W/gen.err")"; continue; fi
    if [ $st -ne 0 ]; then bad=$((bad+1)); echo "not      $target $rel: the generator failed"; continue; fi
    why=""
    k=0
    for o in $(sed -n 's/^objects //p' "$W/obj/link.txt"); do
      [[ $o =~ -m[0-9]+\.o$ ]] || continue
      ours=$W/gen/unit-$k.o; theirs=$W/obj/$o; k=$((k+1))
      if "$OBJDUMP" -d -r -t "$ours" >"$W/dump-$k.txt" 2>"$W/dump-$k.err"; then :; else why="$why objdump-failed"; fi
      [ -s "$W/dump-$k.err" ] && why="$why objdump-warned"
      grep -q '<unknown>' "$W/dump-$k.txt" && why="$why unknown-instruction"
      diff <(globals "$ours") <(globals "$theirs") >"$W/defs-$k.diff" || why="$why globals($o)"
      missing=$(comm -13 <(locals "$ours") <(locals "$theirs"))
      [ -n "$missing" ] && why="$why locals($o: $missing)"
      grep -q 'R_[A-Z0-9_]*[[:space:]]*$' "$W/dump-$k.txt" && why="$why a-relocation-to-nothing"
      "$DWARFDUMP" --eh-frame "$ours" >"$W/eh-$k.txt" 2>&1 || why="$why eh-frame-unread"
      grep -qi 'error\|warning' "$W/eh-$k.txt" && why="$why eh-frame-warned"
      fdes=$(grep -c ' FDE ' "$W/eh-$k.txt"); funcs=$("$OBJDUMP" -t "$ours" | grep -c ' F \.text')
      [ "$fdes" = "$funcs" ] || why="$why fdes($fdes of $funcs)"
      if [ ${#DEBUG[@]} -gt 0 ]; then
        "$DWARFDUMP" --verify "$ours" >"$W/verify-$k.txt" 2>&1 || why="$why dwarf-verify($o)"
        miss=$(comm -13 <(rows "$ours") <(rows "$theirs") | wc -l | tr -d ' ')
        [ "$miss" = 0 ] || why="$why rows($o: $miss of LLVM's missing)"
      fi
    done
    if [ -z "$why" ]; then good=$((good+1)); else bad=$((bad+1)); echo "not      $target $rel:$why"; fi
  done
  echo "$target: $good structurally sound, $bad not, $refused refused (unexecuted: no Linux host)"
  [ $bad -eq 0 ] || status=1
done
exit $status
