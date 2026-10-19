#!/bin/bash
# The encoders held to the system assembler (design D8). Builds tests/encode.nz and
# tests/encode_x64.nz, which print
# every encoding the selection uses beside the assembly it means; assembles that assembly with
# the platform's `cc` for arm64 and x86_64; and compares each instruction's bytes, as
# `llvm-objdump` shows them, with the encoder's. Prints `N agree, M differ` per architecture.
set -u
# The generator is a program written in Nazm: built by the checkout's `nazm` with a heap limit of at
# most 2 GiB and run through `cargo xtask host-run` on macOS (one at a time) or contained
# (`toolchain/lib/stage.sh`; the runbook's R6 rule).
stage_name=codegen-encode
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
NAZM=${NAZM:-$(stage_checkout_nazm "$stage_root")}
HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}/encode
OBJDUMP=${OBJDUMP:-$(xcrun -f llvm-objdump 2>/dev/null || echo /Library/Developer/CommandLineTools/usr/bin/llvm-objdump)}
mkdir -p "$OUT"
stage_build "$NAZM" "$HERE/tests/encode.nz" -o "$OUT/encode" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
stage_build "$NAZM" "$HERE/tests/encode_x64.nz" -o "$OUT/encode_x64" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
{ "$STAGE_RUN" 300 "$OUT/encode" && "$STAGE_RUN" 300 "$OUT/encode_x64"; } >"$OUT/table.txt" || exit 2
status=0
for arch in arm64 x86_64; do
  # One label per encoding, then its assembly.
  awk -F'\t' -v a="$arch" '$1 == a { printf "t_%d:\n    %s\n", n++, $3 }' "$OUT/table.txt" >"$OUT/$arch.s"
  awk -F'\t' -v a="$arch" '$1 == a { printf "t_%d %s %s\n", n++, $2, $3 }' "$OUT/table.txt" >"$OUT/$arch.expected"
  if ! cc -c -arch "$arch" -x assembler "$OUT/$arch.s" -o "$OUT/$arch.o" 2>"$OUT/$arch.err"; then
    echo "$arch: the assembler refused the table:"; head -5 "$OUT/$arch.err"; status=1; continue
  fi
  # Each label's bytes, as llvm-objdump prints them, spaces removed.
  "$OBJDUMP" -d "$OUT/$arch.o" | awk '
    /^[0-9a-f]+ <t_[0-9]+>:/ { if (l != "") print l, h; l = $2; gsub(/[<>:]/, "", l); h = ""; next }
    /^ +[0-9a-f]+:/ { sub(/^ +[0-9a-f]+:[ \t]*/, ""); split($0, f, "\t"); x = f[1]; gsub(/ /, "", x); h = h x }
    END { if (l != "") print l, h }' >"$OUT/$arch.actual"
  agree=0; differ=0
  while read -r label want asm; do
    got=$(awk -v l="$label" '$1 == l { print $2 }' "$OUT/$arch.actual")
    if [ "$got" = "$want" ]; then agree=$((agree+1)); else differ=$((differ+1)); echo "differ   $arch $asm: the encoder wrote $want, the assembler $got"; fi
  done <"$OUT/$arch.expected"
  echo "$arch: $agree agree, $differ differ"
  [ $differ -eq 0 ] || status=1
done
exit $status
