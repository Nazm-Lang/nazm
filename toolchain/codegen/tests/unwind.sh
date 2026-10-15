#!/bin/bash
# Unwind information (5.2), held to LLVM's: a C function, linked into a Nazm program, walks the
# stack with the platform unwinder (`_Unwind_Backtrace`, which reads the image's unwind table
# the linker made from each object's compact unwind) and prints every frame's symbol. The
# program is built by LLVM and again from the generator's objects; the frames printed — through
# Nazm functions that keep values in callee-saved registers across the call — must be the same.
#
#     toolchain/codegen/tests/unwind.sh            (NZCG_TARGET as tests/run.sh)
#
# The program is written here rather than as a `.nz` file in the tree: the compiler written in
# Nazm reads every `.nz` file there and does not know foreign declarations (examples/ffi).
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
# The reference: the Rust `nazm` built from this checkout (rebuild it when library/, crates/ or
# the runtime change); the 0.10.0 release above only builds the generator.
REF=${NAZM_REF:-$HOME/.cache/nazm-target-track9/debug/nazm}
# `--one-thread` when the reference made the program one thread: its runtime then defines the
# stack limit as a plain global, not a thread-local one (the generator inlines the check, 7.3).
one_thread() { nm -m "$1"/*.o 2>/dev/null | grep -qE "\(__DATA,__(common|bss|data)\) .*_nz\.stack_limit\$" && echo --one-thread; }
HERE=$(cd "$(dirname "$0")/.." && pwd)
OUT=${NZCG_OUT:-$HERE/out}
TARGET=${NZCG_TARGET:-aarch64-apple-darwin}
ARCH=${TARGET%%-*}
[ "$ARCH" = aarch64 ] && ARCH=arm64
W=$OUT/unwind-$TARGET
NZCG=$OUT/nzcg
rm -rf "$W"; mkdir -p "$W/obj" "$W/gen"
"$NAZM" build "$HERE/main.nz" -o "$NZCG" >"$OUT/build.log" 2>&1 || { cat "$OUT/build.log"; exit 2; }
cat >"$W/walk.c" <<'EOF'
#include <unwind.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdint.h>
static _Unwind_Reason_Code step(struct _Unwind_Context *ctx, void *arg) {
  (void)arg;
  uintptr_t pc = _Unwind_GetIP(ctx);
  Dl_info info;
  if (pc && dladdr((void *)(pc - 1), &info) && info.dli_sname) printf("frame %s\n", info.dli_sname);
  else printf("frame ?\n");
  return _URC_NO_REASON;
}
int64_t walk(int64_t n) { _Unwind_Backtrace(step, 0); fflush(stdout); return n; }
EOF
cat >"$W/uw.nz" <<'EOF'
extern "C" fn walk(n: Int) -> Int = "walk";

fn inner(f: ForeignCap, n: Int) -> Int ! { foreign } {
    let a = n * 3
    let b = a + 7
    let c = b * b
    let e = c - a
    let g = e + b * 2
    let h = g * 3 + a
    let d = walk(n) + a + b + c + e + g + h
    d + a * b - c + e - g + h
}

fn middle(f: ForeignCap, n: Int) -> Int ! { foreign } {
    let k = n * 5
    inner(f, n) * 2 + k
}

fn outer(f: ForeignCap, n: Int) -> Int ! { foreign } {
    middle(f, n + 1) + 3
}

fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } {
    print(int_to_str(outer(f, 4)));
    0
}
EOF
cc -arch "$ARCH" -c "$W/walk.c" -o "$W/walk.o" || exit 2
"$REF" build --target "$TARGET" "$W/uw.nz" --link "$W/walk.o" -o "$W/ref" >"$W/ref.build" 2>&1 || { cat "$W/ref.build"; exit 2; }
"$REF" build --target "$TARGET" --objects "$W/obj" "$W/uw.nz" >"$W/obj.build" 2>&1 || { cat "$W/obj.build"; exit 2; }
"$REF" lir --target "$TARGET" --ops "$W/uw.nz" >"$W/lir.txt" || exit 2
"$NZCG" --target "$TARGET" $(one_thread "$W/obj") "$W/lir.txt" "$W/gen" || exit 2
k=0; list=()
for o in $(sed -n 's/^objects //p' "$W/obj/link.txt"); do
  if [[ $o =~ -[mg][0-9]+\.o$ ]]; then cp "$W/gen/unit-$k.o" "$W/obj/$o"; k=$((k+1)); fi
  list+=("$W/obj/$o")
done
cc -arch "$ARCH" -o "$W/gen.exe" "${list[@]}" "$W/walk.o" $(sed -n 's/^libraries //p' "$W/obj/link.txt") || exit 2
"$W/ref" >"$W/ref.out" 2>&1; echo "status $?" >>"$W/ref.out"
"$W/gen.exe" >"$W/gen.out" 2>&1; echo "status $?" >>"$W/gen.out"
frames=$(grep -c '^frame' "$W/gen.out")
if cmp -s "$W/ref.out" "$W/gen.out"; then
  echo "unwind $TARGET: agree ($frames frames walked)"
else
  echo "unwind $TARGET: differ"; diff "$W/ref.out" "$W/gen.out"; exit 1
fi
