#!/bin/bash
# `nazm build` admits its stages as `host-run` admits a program (`toolchain/commands/stagerun.nz`,
# `sr_admitted`; `xtask/src/host_run.rs`, `heap_limit_of`), a stripped release stage included.
#
#     toolchain/commands/tests/stage-admission.sh
#
# The 0.11.0 archives shipped a release `nzcg` (stripped: no symbol table, so no `nz.heap_malloc`)
# that `nazm run` refused as `no-counted-heap`, because `build.nz` kept a stale private copy of the
# admission that host-run itself had outgrown (R8 slice 16). This test holds the two together:
#
#   1. a stage built as the release builds it (`--release`) carries no `nz.heap_malloc` but the heap
#      unit's whole `N0406` message: `nazm build --backend nazm` admits it and builds a program;
#   2. a stage that only mentions a heap limit (no symbol, not the unit's whole line) is refused
#      `no-counted-heap`, and so is one with neither (a plain file).
#
# The Nazm `nazm` and its stages are built by the checkout's `nazm` ($NAZM) with a heap limit of at
# most 2 GiB and run through `stage-run` (one `host-run` at a time on the macOS host).
set -u
stage_name=stage-admission
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
OUT=${STAGE_ADMISSION_OUT:-${TMPDIR:-/tmp}/nazm-stage-admission}
rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/work"
fails=0
fail() { echo "FAIL: $*"; fails=$((fails + 1)); }

build() { # NAME SOURCE [ARGS...]
    n=$1 src=$2
    shift 2
    stage_build "$NAZM" --stack-size 256M "$@" "$ROOT/$src" -o "$OUT/bin/$n" > "$OUT/bin/$n.log" 2>&1 \
        || { echo "stage-admission: $n did not build:"; cat "$OUT/bin/$n.log"; exit 2; }
}
build nazm toolchain/commands/nazm.nz
build nazm-pipeline toolchain/pipeline/main.nz
build nzcg toolchain/codegen/main.nz --release

# The case is only a test of the stripped form if the release stage really lost the symbol.
if LC_ALL=C grep -aq 'nz\.heap_malloc' "$OUT/bin/nzcg"; then
    fail "the --release nzcg still names nz.heap_malloc: it is not stripped, so case 1 tests nothing"
fi
LC_ALL=C grep -aq 'nazm: error\[N0406\]: out of memory: the heap limit of 2147483648 bytes is reached' "$OUT/bin/nzcg" \
    || fail "the --release nzcg does not carry the heap unit's whole N0406 message at 2 GiB"

printf 'fn main() {\n    print("admitted")\n}\n' > "$OUT/work/hello.nz"

# `nazm build --backend nazm hello.nz` with NZCG as the code generator: status, standard error.
run_build() { # NZCG
    (cd "$OUT/work" && rm -f hello && NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline" NAZM_NZCG="$1" \
        NAZM_CACHE_DIR="$OUT/work/cache" "$STAGE_RUN" 900 "$OUT/bin/nazm" build --backend nazm --no-cache hello.nz -o hello \
        > "$OUT/work/out" 2> "$OUT/work/err")
}

# 1. The stripped release stage is admitted and does its work.
run_build "$OUT/bin/nzcg"
st=$?
if grep -q 'is not admitted as a stage' "$OUT/work/err"; then
    fail "the stripped release nzcg is refused: $(grep 'is not admitted' "$OUT/work/err" | head -1)"
elif [ "$st" -ne 0 ] || [ ! -f "$OUT/work/hello" ]; then
    fail "nazm build with the release nzcg ended $st without a program: $(head -3 "$OUT/work/err")"
fi

# 2. A mention of a limit, and a plain file, are still refused, by name.
printf '#!/bin/sh\n# the heap limit of 4194304 bytes is reached\nexit 3\n' > "$OUT/bin/mention"
printf '#!/bin/sh\nexit 3\n' > "$OUT/bin/plain"
chmod +x "$OUT/bin/mention" "$OUT/bin/plain"
for fake in mention plain; do
    run_build "$OUT/bin/$fake"
    st=$?
    grep -q "nzcg (\`$OUT/bin/$fake\`) is not admitted as a stage: no-counted-heap" "$OUT/work/err" \
        || fail "a $fake nzcg is not refused no-counted-heap (status $st): $(head -2 "$OUT/work/err")"
    [ ! -f "$OUT/work/hello" ] || fail "a $fake nzcg was refused but a program was written"
done

if [ "$fails" -eq 0 ]; then
    echo "stage-admission: 3 of 3 agree (stripped release stage admitted; mention and plain refused)"
    rm -rf "$OUT"
    exit 0
fi
echo "stage-admission: $fails failed; scratch kept in $OUT"
exit 1
