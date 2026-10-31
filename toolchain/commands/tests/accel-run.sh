#!/bin/bash
# R8 slice 11d, task 10.2: `nazm accel`'s run in Nazm, against the Rust one.
#
#     toolchain/commands/tests/accel-run.sh [PART...] [-- KERNEL.nz ...]
#     PART: cpu zip reduce opts auto opencl errors     (all by default; ACCEL_PARTS="cpu" does the same)
#
#   cpu      every function of tests/accel with the ints and the floats of the fixtures, `--provider cpu`
#   zip      every function with the ints twice
#   reduce   every function with `--reduce add|min|max`, `--provider cpu`
#   opts     `--scalar`, `--no-check`, `--print-kernel`
#   auto     the default provider (the GPU where it can run the kernel, as the reference chooses) over the first inputs
#   opencl   `--provider opencl`
#   errors   a bad input, a wrong number of inputs, unequal lengths, an input that cannot be read, a name that is not there
#
# The inputs are tiny (at most seven numbers). A case agrees when standard output (a measured time masked, see below), standard
# error and status are the same bytes. INTENDED DIFFERENCE, shape compared only: every field that is a measured time. In the report
# these are `ms` (a transfer's and the fold's), `build` and `run` of `timings_ms`: each is replaced by `N` in BOTH outputs before they
# are compared. No time is asserted or recorded. A status 6 from the Nazm side is counted apart (refused), never an agreement.
# The reference is $NAZM_REF (default $NAZM); a Nazm-built `nazm` is one `host-run` at a time, heap limit 2G (`toolchain/lib/stage.sh`).
set -u
stage_name=accel-run
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${ACCEL_OUT:-${TMPDIR:-/tmp}/nazm-accel-run}
mkdir -p "$OUT/bin" "$OUT/work" "$OUT/fx"

parts=()
files=()
while [ $# -gt 0 ]; do
    case $1 in
        --) shift; files=("$@"); break ;;
        *) parts+=("$1"); shift ;;
    esac
done
[ ${#parts[@]} -gt 0 ] || read -r -a parts <<< "${ACCEL_PARTS:-cpu zip reduce opts auto opencl errors}"

if [ -z "${NAZM_PORT:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/nazm.nz" -o "$OUT/bin/nazm" > "$OUT/bin/nazm.log" 2>&1; then
        echo "accel-run: nazm did not build:"; cat "$OUT/bin/nazm.log"; exit 2
    fi
fi
PORT=${NAZM_PORT:-$OUT/bin/nazm}

printf '1 2 3 -4 0 9223372036854775807 -9223372036854775808\n' > "$OUT/fx/ints.txt"
printf '3 5 8\n' > "$OUT/fx/smallints.txt"
printf '7 8\n' > "$OUT/fx/shortints.txt"
printf '1.5 2 -3.25 0.1 1e3 inf nan -nan 16777217 0.30000000000000004\n' > "$OUT/fx/floats.txt"
printf '1 x 3\n' > "$OUT/fx/bad.txt"
: > "$OUT/fx/empty.txt"

if [ ${#files[@]} -eq 0 ]; then
    while IFS= read -r f; do files+=("$f"); done < <(ls "$ROOT"/toolchain/commands/tests/accel/*.nz | sed "s|^$ROOT/||")
fi
export NAZM_LIBRARY="$ROOT/library"
cd "$ROOT" || exit 2

# A measured time, in both outputs, before they are compared.
mask() { sed -E 's/("(ms|build|run)":)[-0-9.eE+]+/\1N/g' "$1" > "$1.m"; }

agree=0; differ=0; refused=0; total_differ=0
case_run() {
    label=$1
    shift
    W=$OUT/work/case
    rm -rf "$W"; mkdir -p "$W"
    "$REF" "$@" > "$W/ref.out" 2> "$W/ref.err" < /dev/null
    echo $? > "$W/ref.status"
    while :; do
        "$STAGE_RUN" "${ACCEL_DEADLINE:-300}" "$PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/got.err"; then sleep 10; continue; fi
        break
    done
    echo $rc > "$W/got.status"
    if [ "$rc" = 6 ]; then
        refused=$((refused + 1)); echo "refused  $label: $(head -c 200 "$W/got.err" | tr '\n' ' ')"; return
    fi
    mask "$W/ref.out"; mask "$W/got.out"
    if cmp -s "$W/ref.out.m" "$W/got.out.m" && cmp -s "$W/ref.err" "$W/got.err" && cmp -s "$W/ref.status" "$W/got.status"; then
        agree=$((agree + 1)); return
    fi
    differ=$((differ + 1))
    echo "differ   $label (status $(cat "$W/ref.status") vs $(cat "$W/got.status"))"
    for x in out.m err; do
        cmp -s "$W/ref.$x" "$W/got.$x" || diff "$W/ref.$x" "$W/got.$x" | head -${DIFF_LINES:-4} | cut -c1-300 | sed 's/^/    /'
    done
    if [ -n "${KEEP:-}" ]; then
        k=$KEEP/$(echo "$label" | tr '/ ' '__'); mkdir -p "$k" && cp "$W"/* "$k"/
    fi
}

fns() { grep -o '^fn [A-Za-z_0-9]*' "$1" | sed 's/^fn //'; }
FX=$OUT/fx
for p in "${parts[@]}"; do
    agree=0; differ=0; refused=0
    for f in "${files[@]}"; do
        for name in $(fns "$f"); do
            case $p in
                cpu)
                    for kind in ints floats; do case_run "cpu $f $name $kind" accel "$f" "$name" --input "$FX/$kind.txt" --provider cpu; done ;;
                zip)
                    case_run "zip $f $name" accel "$f" "$name" --input "$FX/smallints.txt" --input "$FX/smallints.txt" --provider cpu
                    case_run "zip floats $f $name" accel "$f" "$name" --input "$FX/floats.txt" --input "$FX/floats.txt" --provider cpu ;;
                reduce)
                    # Every operation over every kernel is ACCEL_ALL_OPS=1; by default `add` of the ints and `max` of the floats.
                    if [ -n "${ACCEL_ALL_OPS:-}" ]; then
                        for op in add min max; do
                            case_run "reduce $op $f $name" accel "$f" "$name" --input "$FX/smallints.txt" --provider cpu --reduce $op
                            case_run "reduce $op floats $f $name" accel "$f" "$name" --input "$FX/floats.txt" --provider cpu --reduce $op
                        done
                    else
                        case_run "reduce add $f $name" accel "$f" "$name" --input "$FX/ints.txt" --provider cpu --reduce add
                        case_run "reduce max floats $f $name" accel "$f" "$name" --input "$FX/floats.txt" --provider cpu --reduce max
                    fi ;;
                opts)
                    case_run "scalar $f $name" accel "$f" "$name" --input "$FX/smallints.txt" --provider cpu --scalar
                    case_run "no-check $f $name" accel "$f" "$name" --input "$FX/ints.txt" --provider cpu --no-check --reduce min
                    case_run "empty $f $name" accel "$f" "$name" --input "$FX/empty.txt" --provider cpu --reduce max ;;
                auto)
                    case_run "auto $f $name" accel "$f" "$name" --input "$FX/smallints.txt"
                    [ -n "${ACCEL_ALL_OPS:-}" ] && case_run "auto floats $f $name" accel "$f" "$name" --input "$FX/floats.txt" --reduce add ;;
                opencl)
                    case_run "opencl $f $name" accel "$f" "$name" --input "$FX/smallints.txt" --provider opencl --reduce min
                    [ -n "${ACCEL_ALL_OPS:-}" ] && case_run "opencl floats $f $name" accel "$f" "$name" --input "$FX/floats.txt" --provider opencl ;;
                errors)
                    case_run "bad $f $name" accel "$f" "$name" --input "$FX/bad.txt" --provider cpu
                    case_run "two $f $name" accel "$f" "$name" --input "$FX/ints.txt" --input "$FX/shortints.txt" --provider cpu
                    case_run "three $f $name" accel "$f" "$name" --input "$FX/ints.txt" --input "$FX/ints.txt" --input "$FX/ints.txt" --provider cpu
                    case_run "missing input $f $name" accel "$f" "$name" --input "$FX/nosuch.txt" --provider cpu ;;
                *) echo "accel-run: no part $p"; exit 2 ;;
            esac
        done
        [ "$p" = errors ] && case_run "no function $f" accel "$f" nosuch --input "$FX/ints.txt" --provider cpu
    done
    echo "$p: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
done
[ "$total_differ" -eq 0 ]
