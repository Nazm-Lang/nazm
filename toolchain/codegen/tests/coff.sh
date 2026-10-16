#!/bin/bash
# The PE/COFF objects' check (section 8): `x86_64-pc-windows-gnu` objects compared structurally
# with LLVM's for the same triple, and linked by MinGW-w64 — this Mac runs no Windows program,
# so the result is *linked*, never *agree* (scripts/windows-host.sh runs them on a Windows host).
#
#     toolchain/codegen/tests/coff.sh [FILE.nz ...]
#
# Per input: the reference writes `nazm build --target x86_64-pc-windows-gnu --objects` and
# `nazm lir --ops` for the triple; the generator writes the unit objects, twice, the same bytes
# (D7). Per unit object: MinGW-w64's `objdump` reads it without a warning and disassembles no
# `(bad)` instruction; it defines the same global symbols as LLVM's object for the unit, and
# every local function LLVM's does; every relocation is one of ADDR64, ADDR32NB, REL32 and
# SECREL and names a symbol; it imports nothing LLVM's does not but `___chkstk_ms` and
# `nz.failed_now` (which LLVM reads in place, the runtime's thread-local flag); `.pdata`
# holds one RUNTIME_FUNCTION per function, each starting at a function, its UNWIND_INFO read
# back (version 1, the prologue's codes), every function LLVM's object gives one among them
# (LLVM gives none to a function without a frame; here every function has one). With a `nazm` whose
# `lir` has `--debug`, the line tables pass `llvm-dwarfdump --verify` and hold every row of
# LLVM's. Then the program is linked with `x86_64-w64-mingw32-gcc` as link.txt says, the
# generator's objects in place of LLVM's unit objects. Prints one line per input that fails, then
# `N structurally sound and linked, M not, K refused, S skipped`.
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
REF=${NAZM_REF:-$HOME/.cache/nazm-target-track9/debug/nazm}
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
NZCG=$OUT/nzcg
TARGET=x86_64-pc-windows-gnu
CC=${MINGW_CC:-x86_64-w64-mingw32-gcc}
OBJDUMP=${MINGW_OBJDUMP:-x86_64-w64-mingw32-objdump}
NM=${MINGW_NM:-x86_64-w64-mingw32-nm}
DWARFDUMP=${DWARFDUMP:-$(xcrun -f llvm-dwarfdump 2>/dev/null || echo /Library/Developer/CommandLineTools/usr/bin/llvm-dwarfdump)}
command -v "$CC" >/dev/null || { echo "coff.sh: $CC is not installed"; exit 2; }
DEBUG=()
"$REF" lir --help 2>/dev/null | grep -q -- '--debug' && DEBUG=(--debug)
mkdir -p "$OUT"
"$NAZM" build "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
if [ $# -gt 0 ]; then
  inputs=("$@")
else
  inputs=("$ROOT/bench/compare/nazm/fib.nz")
  for f in "$ROOT"/tests/compat/v1/run/*.nz; do inputs+=("$f"); done
  for d in "$ROOT"/tests/compat/v1/run/*/; do [ -f "${d}main.nz" ] && inputs+=("${d}main.nz"); done
  for f in "$ROOT"/tests/compat/v1/trap/*.nz "$HERE"/tests/programs/*.nz \
           "$ROOT"/bench/compare/nazm/*.nz "$ROOT"/bench/programs/*.nz \
           "$ROOT"/examples/*.nz "$ROOT"/examples/tour/*.nz "$ROOT"/examples/std/*.nz \
           "$ROOT"/crates/nazm-cli/tests/transfers/*.nz; do
    [ -f "$f" ] && [ "$f" != "$ROOT/bench/compare/nazm/fib.nz" ] && inputs+=("$f")
  done
  while IFS= read -r f; do inputs+=("$f"); done < <(find "$ROOT/corpus" -name '*.nz' | sort)
fi

# `--one-thread` when the reference's runtime defines the stack limit as a plain global (7.3).
one_thread() { "$NM" "$1"/*.o 2>/dev/null | grep -qE " [BDbd] nz\.stack_limit\$" && echo --one-thread; }
# The symbol table as `section class type value name` lines (a name may hold spaces).
table() { "$OBJDUMP" -t "$1" | awk '/^\[/ { match($0, / 0x[0-9a-f]+ /); v = substr($0, RSTART + 3, RLENGTH - 4)
  n = substr($0, RSTART + RLENGTH); s = $0; sub(/.*\(sec +/, "", s); sub(/\).*/, "", s)
  c = $0; sub(/.*\(scl +/, "", c); sub(/\).*/, "", c); t = $0; sub(/.*\(ty +/, "", t); sub(/\).*/, "", t)
  print s "\t" c "\t" t "\t" v "\t" n }'; }
# Defined symbols of storage class $2 (not sections'), functions, undefined symbols, by name.
syms() { table "$1" | awk -F'\t' -v scl="$2" '$1 > 0 && $2 == scl && $5 !~ /^[.@]/ { print $5 }' | sort; }
funcs() { table "$1" | awk -F'\t' '$1 > 0 && $3 == 20 { print $5 }' | sort; }
undefs() { table "$1" | awk -F'\t' '$1 == 0 { print $5 }' | sort; }
fn_addrs() { table "$1" | awk -F'\t' '$1 == 1 && $3 == 20 { print $4 }' | sort; }
# The functions `.pdata` holds a RUNTIME_FUNCTION for, by name.
unwound() { pdata_begins "$1" >"$1.begins"; table "$1" | awk -F'\t' 'NR == FNR { b[$1] = 1; next } $1 == 1 && $3 == 20 && ($4 in b) { print $5 }' "$1.begins" - | sort; rm -f "$1.begins"; }
rows() {
  "$DWARFDUMP" --debug-line "$1" | awk '
    /^include_directories\[ *[0-9]+\] = "/ { i = $0; sub(/\].*/, "", i); gsub(/[^0-9]/, "", i)
      s = $0; sub(/^[^"]*"/, "", s); sub(/"$/, "", s); dir[i] = s }
    /^file_names\[ *[0-9]+\]:/ { n = $0; gsub(/[^0-9]/, "", n) }
    /^ +name: "/ { s = $0; sub(/^ +name: "/, "", s); sub(/"$/, "", s); name[n] = s }
    /^ +dir_index: [0-9]+/ { if ($2 > 0) name[n] = dir[$2] "/" name[n] }
    /^0x[0-9a-f]+ +[0-9]+ +[0-9]+ +[0-9]+/ { if ($2 != 0 && $3 != 0) print name[$4], $2, $3 }' | sort -u
}
# The `.pdata` begin addresses, and the UNWIND_INFO entries read back.
pdata_begins() { "$OBJDUMP" -x "$1" | awk '/^The Function Table/ { on = 1; next } on && /^ [0-9a-f]+:/ { print $2 } on && /^$/ { on = 0 }' | sort; }
unwinds() { "$OBJDUMP" -x "$1" | grep -c '^	Version: 1, Flags: none'; }

good=0; bad=0; refused=0; skipped=0
for src in "${inputs[@]}"; do
  rel=${src#"$ROOT"/}
  W=$OUT/coff/$(echo "$rel" | tr '/.' '__')
  rm -rf "$W"; mkdir -p "$W/obj" "$W/gen" "$W/gen2"
  link=()
  if [ -f "${src%.nz}.c" ]; then
    if ! "$CC" -c "${src%.nz}.c" -o "$W/c.o" >"$W/c.build" 2>&1; then
      echo "skipped  $rel (its C does not compile)"; skipped=$((skipped+1)); continue
    fi
    link=(--link "$W/c.o")
  fi
  if ! "$REF" build --target "$TARGET" ${link[@]+"${link[@]}"} ${DEBUG[@]+"${DEBUG[@]}"} --objects "$W/obj" "$src" >"$W/obj.build" 2>&1 ||
     ! "$REF" lir --target "$TARGET" --ops ${DEBUG[@]+"${DEBUG[@]}"} "$src" >"$W/lir.txt" 2>/dev/null; then
    echo "skipped  $rel (no objects or LIR)"; skipped=$((skipped+1)); continue
  fi
  ot=$(one_thread "$W/obj")
  "$NZCG" --target "$TARGET" $ot "$W/lir.txt" "$W/gen" >/dev/null 2>"$W/gen.err"
  st=$?
  if [ $st -eq 3 ]; then refused=$((refused+1)); echo "refused  $rel: $(head -1 "$W/gen.err")"; continue; fi
  if [ $st -ne 0 ]; then bad=$((bad+1)); echo "not      $rel: the generator failed ($st)"; continue; fi
  "$NZCG" --target "$TARGET" $ot "$W/lir.txt" "$W/gen2" >/dev/null 2>&1
  why=""
  diff -r "$W/gen" "$W/gen2" >/dev/null || why="$why not-deterministic"
  k=0; list=()
  for o in $(sed -n 's/^objects //p' "$W/obj/link.txt"); do
    if [[ $o =~ -[mg][0-9]+\.o$ ]]; then
      ours=$W/gen/unit-$k.o; theirs=$W/obj/$o; k=$((k+1))
      if [ ! -f "$ours" ]; then why="$why missing(unit-$((k-1)).o)"; list+=("$theirs"); continue; fi
      "$OBJDUMP" -d -r -t -x "$ours" >"$W/dump-$k.txt" 2>"$W/dump-$k.err" || why="$why objdump-failed($o)"
      [ -s "$W/dump-$k.err" ] && why="$why objdump-warned($o)"
      grep -q '(bad)' "$W/dump-$k.txt" && why="$why bad-instruction($o)"
      diff <(syms "$ours" 2) <(syms "$theirs" 2) >"$W/globals-$k.diff" || why="$why globals($o)"
      missing=$(comm -13 <(funcs "$ours") <(funcs "$theirs") | tr '\n' ' ')
      [ -n "$missing" ] && why="$why functions($o: $missing)"
      extra=$(comm -23 <(undefs "$ours") <(undefs "$theirs") | grep -vxE '___chkstk_ms|nz\.failed_now' | tr '\n' ' ')
      [ -n "$extra" ] && why="$why imports($o: $extra)"
      awk '/^RELOCATION RECORDS/ { on = 1; next } on && /^[0-9a-f]+ / { if ($2 !~ /^IMAGE_REL_AMD64_(ADDR64|ADDR32NB|REL32|SECREL)$/ || $3 == "") print }' \
        "$W/dump-$k.txt" | grep -q . && why="$why relocation($o)"
      [ "$(pdata_begins "$ours")" = "$(fn_addrs "$ours")" ] || why="$why pdata-not-per-function($o)"
      nf=$(fn_addrs "$ours" | wc -l | tr -d ' ')
      [ "$(unwinds "$ours")" = "$nf" ] || why="$why unwind-info($o: $(unwinds "$ours") of $nf)"
      # LLVM gives no entry to a function without a frame; every function here has one.
      lost=$(comm -13 <(unwound "$ours") <(unwound "$theirs") | tr '\n' ' ')
      [ -n "$lost" ] && why="$why unwind-missing($o: $lost)"
      if [ ${#DEBUG[@]} -gt 0 ]; then
        "$DWARFDUMP" --verify "$ours" >"$W/verify-$k.txt" 2>&1 || why="$why dwarf-verify($o)"
        miss=$(comm -13 <(rows "$ours") <(rows "$theirs") | wc -l | tr -d ' ')
        [ "$miss" = 0 ] || why="$why rows($o: $miss of LLVM's missing)"
      fi
      list+=("$ours")
    else
      list+=("$W/obj/$o")
    fi
  done
  [ "$k" -eq "$(ls "$W/gen" | wc -l | tr -d ' ')" ] || why="$why units($k in link.txt, $(ls "$W/gen" | wc -l | tr -d ' ') written)"
  libs=$(sed -n 's/^libraries //p' "$W/obj/link.txt")
  "$CC" -o "$W/gen.exe" "${list[@]}" $libs >"$W/link.log" 2>&1 || why="$why link($(head -1 "$W/link.log"))"
  if [ -z "$why" ]; then good=$((good+1)); else bad=$((bad+1)); echo "not      $rel:$why"; fi
done
echo "$TARGET: $good structurally sound and linked, $bad not, $refused refused, $skipped skipped (not run: no Windows host here)"
[ $bad -eq 0 ]
