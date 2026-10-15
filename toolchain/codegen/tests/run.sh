#!/bin/bash
# The differential suite of Nazm's own code generator (R9, design D8).
#
#     toolchain/codegen/tests/run.sh [FILE.nz ...]
#     NZCG_TARGET=x86_64-apple-darwin toolchain/codegen/tests/run.sh [FILE.nz ...]
#
# The target is aarch64-apple-darwin unless NZCG_TARGET says otherwise; x86_64-apple-darwin runs
# under Rosetta on an arm64 Mac.
#
# Builds the generator with the 0.10.0 release ($NAZM); the reference is the Rust `nazm` built from
# this checkout ($NAZM_REF, by default CARGO_TARGET_DIR=~/.cache/nazm-target-track9). For each input (by default
# bench/compare/nazm/fib.nz, every program of tests/compat/v1/run/ and every corpus program):
#
#   reference  `nazm build FILE` (LLVM), run;
#   generated  `nazm build --objects DIR FILE`, every unit object (a module's and a generic
#              instantiation's) replaced by the generator's for `nazm lir --ops FILE`, linked with `cc` in link.txt's order, run;
#
# and compares standard output, standard error and exit status byte for byte. An input the
# generator refuses is `refused`; one the reference cannot build is `skipped`; neither is ever
# counted as agreeing. Prints one line per input that does not agree, then the summary
# `N agree, M differ, K refused, S skipped`. Exit status 0 only when nothing differs.
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
# The reference: the Rust `nazm` built from this checkout (rebuild it when library/, crates/ or
# the runtime change); the 0.10.0 release above only builds the generator.
REF=${NAZM_REF:-$HOME/.cache/nazm-target-track9/debug/nazm}
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
WORK=$OUT/work-${NZCG_TARGET:-aarch64-apple-darwin}
NZCG=$OUT/nzcg
TIMEOUT=${NZCG_TIMEOUT:-60}
TARGET=${NZCG_TARGET:-aarch64-apple-darwin}
ARCH=${TARGET%%-*}
[ "$ARCH" = aarch64 ] && ARCH=arm64
mkdir -p "$OUT"
if ! "$NAZM" build "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1; then
  echo "nzcg did not build:"; cat "$OUT/build.log"; exit 2
fi

# Run with a time limit; stdin empty.
limited() { perl -e 'alarm shift; exec @ARGV or exit 127' "$TIMEOUT" "$@" </dev/null; }

if [ $# -gt 0 ]; then
  inputs=("$@")
else
  inputs=("$ROOT/bench/compare/nazm/fib.nz")
  for f in "$ROOT"/tests/compat/v1/run/*.nz; do inputs+=("$f"); done
  for d in "$ROOT"/tests/compat/v1/run/*/; do
    [ -f "$d/main.nz" ] && inputs+=("$d/main.nz")
  done
  for f in "$ROOT"/tests/compat/v1/trap/*.nz "$HERE"/tests/programs/*.nz \
           "$ROOT"/bench/compare/nazm/*.nz "$ROOT"/bench/programs/*.nz \
           "$ROOT"/examples/*.nz "$ROOT"/examples/tour/*.nz "$ROOT"/examples/std/*.nz \
           "$ROOT"/crates/nazm-cli/tests/transfers/*.nz; do
    [ -f "$f" ] && [ "$f" != "$ROOT/bench/compare/nazm/fib.nz" ] && inputs+=("$f")
  done
  while IFS= read -r f; do inputs+=("$f"); done < <(find "$ROOT/corpus" -name '*.nz' | sort)
fi

agree=0; differ=0; refused=0; skipped=0
for src in "${inputs[@]}"; do
  rel=${src#"$ROOT"/}
  name=$(echo "$rel" | tr '/.' '__')
  W=$WORK/$name
  rm -rf "$W"; mkdir -p "$W/obj" "$W/gen"
  if ! "$REF" build --target "$TARGET" "$src" -o "$W/ref" >"$W/ref.build" 2>&1; then
    echo "skipped  $rel (the reference does not build)"; skipped=$((skipped+1)); continue
  fi
  if ! "$REF" build --target "$TARGET" --objects "$W/obj" "$src" >"$W/obj.build" 2>&1 ||
     ! "$REF" lir --target "$TARGET" --ops "$src" >"$W/lir.txt" 2>"$W/lir.err"; then
    echo "skipped  $rel (no objects or LIR)"; skipped=$((skipped+1)); continue
  fi
  "$NZCG" --target "$TARGET" "$W/lir.txt" "$W/gen" >"$W/gen.out" 2>"$W/gen.err"
  st=$?
  if [ $st -eq 3 ]; then
    echo "refused  $rel: $(head -1 "$W/gen.err")"; refused=$((refused+1)); continue
  elif [ $st -ne 0 ]; then
    echo "differ   $rel: the generator failed ($st): $(head -3 "$W/gen.err")"; differ=$((differ+1)); continue
  fi
  # Deterministic (D7): a second run writes the same bytes.
  mkdir -p "$W/gen2"
  "$NZCG" --target "$TARGET" "$W/lir.txt" "$W/gen2" >/dev/null 2>&1
  if ! diff -r "$W/gen" "$W/gen2" >/dev/null; then
    echo "differ   $rel: two runs of the generator wrote different objects"; differ=$((differ+1)); continue
  fi
  # The unit objects, in link order, replaced by the generator's.
  objs=$(sed -n 's/^objects //p' "$W/obj/link.txt")
  libs=$(sed -n 's/^libraries //p' "$W/obj/link.txt")
  k=0; list=()
  for o in $objs; do
    if [[ $o =~ -[mg][0-9]+\.o$ ]]; then
      cp "$W/gen/unit-$k.o" "$W/obj/$o"; k=$((k+1))
    fi
    list+=("$W/obj/$o")
  done
  # Every unit object — a module's (-mK) and a generic instantiation's (-gK) — is the
  # generator's: on hosted targets LLVM's objects unwind on a failure and the generator's keep
  # the bit (performance-leadership 4.2), so the two are never mixed in one program.
  if [ "$k" -ne "$(ls "$W/gen" | wc -l)" ]; then
    echo "differ   $rel: $k unit objects in link.txt, $(ls "$W/gen" | wc -l | tr -d ' ') written"; differ=$((differ+1)); continue
  fi
  if ! cc -arch "$ARCH" -o "$W/gen.exe" "${list[@]}" $libs >"$W/link.log" 2>&1; then
    echo "differ   $rel: the link failed: $(head -3 "$W/link.log")"; differ=$((differ+1)); continue
  fi
  (cd "$W" && limited ./ref >ref.stdout 2>ref.stderr; echo $? >ref.status)
  (cd "$W" && limited ./gen.exe >gen.stdout 2>gen.stderr; echo $? >gen.status)
  if cmp -s "$W/ref.stdout" "$W/gen.stdout" && cmp -s "$W/ref.stderr" "$W/gen.stderr" &&
     cmp -s "$W/ref.status" "$W/gen.status"; then
    agree=$((agree+1))
  else
    what=""
    cmp -s "$W/ref.stdout" "$W/gen.stdout" || what="$what stdout"
    cmp -s "$W/ref.stderr" "$W/gen.stderr" || what="$what stderr"
    cmp -s "$W/ref.status" "$W/gen.status" || what="$what status($(cat "$W/ref.status")/$(cat "$W/gen.status"))"
    echo "differ   $rel:$what"; differ=$((differ+1))
  fi
done
echo "$agree agree, $differ differ, $refused refused, $skipped skipped"
[ $differ -eq 0 ]
