#!/bin/bash
# The generator's performance report (design D3, stage B): run time of its executables against
# the LLVM build at `nazm build`'s default (-O0) and at `--opt-level 2`, on the comparison's
# `fib` and `words` and on bench/programs. Reported, not gated. Each program's output is also
# compared with the LLVM build's, so a fast wrong answer is never reported as a time.
#
#     toolchain/codegen/tests/perf.sh            (NZCG_TARGET as tests/run.sh reads it)
set -u
# The generator is a program written in Nazm: built by the checkout's `nazm` with a heap limit of at
# most 2 GiB and run through `cargo xtask host-run` on macOS (one at a time) or contained
# (`toolchain/lib/stage.sh`; the runbook's R6 rule).
stage_name=codegen-perf
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
NAZM=${NAZM:-$(stage_checkout_nazm "$stage_root")}
# The reference: the Rust `nazm` built from this checkout (rebuild it when library/, crates/ or
# the runtime change); the checkout's `nazm` above builds the generator.
REF=${NAZM_REF:-$HOME/.cache/nazm-target-track9/debug/nazm}
# `--one-thread` when the reference made the program one thread: its runtime then defines the
# stack limit as a plain global, not a thread-local one (the generator inlines the check, 7.3).
one_thread() { nm -m "$1"/*.o 2>/dev/null | grep -qE "\(__DATA,__(common|bss|data)\) .*_nz\.stack_limit\$" && echo --one-thread; }
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
TARGET=${NZCG_TARGET:-aarch64-apple-darwin}
ARCH=${TARGET%%-*}
[ "$ARCH" = aarch64 ] && ARCH=arm64
P=$OUT/perf-$TARGET
NZCG=$OUT/nzcg
mkdir -p "$P"
stage_build "$NAZM" "$HERE/main.nz" -o "$NZCG" >"$P/build.log" 2>&1 || { cat "$P/build.log"; exit 2; }
[ -f "$P/words.txt" ] || python3 "$ROOT/bench/compare/gen_words.py" "$P/words.txt"

# The best of five wall-clock runs, in milliseconds.
best() {
  local b=999999999 t0 t1 ms
  for _ in 1 2 3 4 5; do
    t0=$(perl -MTime::HiRes=time -e 'printf "%.0f", time*1000')
    "$@" >/dev/null 2>&1 </dev/null
    t1=$(perl -MTime::HiRes=time -e 'printf "%.0f", time*1000')
    ms=$((t1 - t0)); [ $ms -lt $b ] && b=$ms
  done
  echo $b
}

printf '%-28s %10s %10s %10s %8s\n' program "LLVM -O0" "LLVM -O2" nzcg "agrees"
for src in "$ROOT/bench/compare/nazm/fib.nz" "$ROOT/bench/compare/nazm/words.nz" "$ROOT"/bench/programs/*.nz; do
  name=$(basename "$src" .nz)
  W=$P/$name
  rm -rf "$W"; mkdir -p "$W/obj" "$W/gen"
  args=()
  [ "$name" = words ] && args=("$P/words.txt")
  "$REF" build --target "$TARGET" "$src" -o "$W/o0" >/dev/null 2>&1 || { echo "$name: no reference"; continue; }
  "$REF" build --target "$TARGET" --opt-level 2 "$src" -o "$W/o2" >/dev/null 2>&1
  "$REF" build --target "$TARGET" --objects "$W/obj" "$src" >/dev/null 2>&1
  "$REF" lir --target "$TARGET" --ops "$src" >"$W/lir.txt" 2>/dev/null
  if ! "$STAGE_RUN" 300 "$NZCG" --target "$TARGET" $(one_thread "$W/obj") "$W/lir.txt" "$W/gen" 2>"$W/gen.err"; then
    echo "$name: refused: $(head -1 "$W/gen.err")"; continue
  fi
  k=0; list=()
  for o in $(sed -n 's/^objects //p' "$W/obj/link.txt"); do
    if [[ $o =~ -[mg][0-9]+\.o$ ]]; then cp "$W/gen/unit-$k.o" "$W/obj/$o"; k=$((k+1)); fi
    list+=("$W/obj/$o")
  done
  cc -arch "$ARCH" -o "$W/gen.exe" "${list[@]}" $(sed -n 's/^libraries //p' "$W/obj/link.txt") 2>/dev/null
  "$W/o0" ${args[@]+"${args[@]}"} >"$W/o0.out" 2>&1 </dev/null; echo $? >>"$W/o0.out"
  "$W/gen.exe" ${args[@]+"${args[@]}"} >"$W/gen.out" 2>&1 </dev/null; echo $? >>"$W/gen.out"
  same=$(cmp -s "$W/o0.out" "$W/gen.out" && echo yes || echo NO)
  printf '%-28s %8sms %8sms %8sms %8s\n' "$name" "$(best "$W/o0" ${args[@]+"${args[@]}"})" \
    "$(best "$W/o2" ${args[@]+"${args[@]}"})" "$(best "$W/gen.exe" ${args[@]+"${args[@]}"})" "$same"
done
