#!/bin/bash
# R8 slice 10: `bench`, `accel` and `contract` written in Nazm, against the Rust ones
# (`openspec/changes/r8-cut-over/design.md`, *Slice 10*, task 10.1; R5's method, `openspec/changes/r5-commands/`).
#
#     toolchain/commands/tests/tools.sh [PART...] [-- FILE.nz ...]
#     PART: args bench shape accel contract     (all by default; TOOLS_PARTS="bench" does the same)
#
#   args      every command line of tests/args/{bench,accel,contract}.txt, run in the repository's root: clap's usage errors and help, and what
#             each command then does. `@FX@` in a line is the directory of fixtures this script writes (a file that is not UTF-8, one that does
#             not parse, a program that does not check, an empty directory).
#   bench     `bench` over the fixtures of tests/bench (and the ones above): the errors, the files that gather nothing, the functions that do not
#             build. It NEVER takes a timing: nothing here runs a benchmark, and a program with a `@bench` function that builds is not an input
#             (design, S10.2).
#   shape     the line `bench` prints for a measurement and the parse of the harness's line, on their own (tests/bench-shape.nz against
#             tests/bench-shape.expected, which holds the reference's format strings applied by hand): the clock is never read.
#   accel     `accel FILE KERNEL --input F` over every input of check, for the first functions of each, with and without `--print-kernel`.
#   contract  `contract FILE` (and `--interface`, `--txs`, `--accounts`, `--upgrade-check`) over every input of check.
#
# The Nazm `nazm` (toolchain/commands/nazm.nz) is built by the checkout's `nazm` ($NAZM) with a heap limit of at most 2 GiB
# (`toolchain/lib/stage.sh`, the R6 rule). Each Nazm run is one `host-run`, one at a time (a refusal because another stage is running is
# waited out, not counted); the Rust side runs directly. Both run from the repository's root with the file named by its path under it. A case
# agrees when standard output, standard error and the exit status are the same bytes; a status 6 from the Nazm side (what it does not carry)
# is counted apart, with its reason, and is never an agreement. Prints a line per case that does not agree, then one line per part:
# `PART: A agree, D differ, R refused`. Exit 0 only when nothing differs.
#
# The reference is $NAZM_REF (default $NAZM). KEEP=DIR keeps each difference's two records.
set -u
stage_name=tools-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${TOOLS_OUT:-${TMPDIR:-/tmp}/nazm-tools-compare}
mkdir -p "$OUT/bin" "$OUT/work" "$OUT/fx/emptydir"

parts=()
files=()
while [ $# -gt 0 ]; do
    case $1 in
        --) shift; files=("$@"); break ;;
        *) parts+=("$1"); shift ;;
    esac
done
[ ${#parts[@]} -gt 0 ] || read -r -a parts <<< "${TOOLS_PARTS:-args bench shape accel contract}"

if [ -z "${NAZM_PORT:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/nazm.nz" -o "$OUT/bin/nazm" > "$OUT/bin/nazm.log" 2>&1; then
        echo "tools-compare: nazm did not build:"
        cat "$OUT/bin/nazm.log"
        exit 2
    fi
fi
PORT=${NAZM_PORT:-$OUT/bin/nazm}
# `bench` has the Nazm `nazm` build each `@bench` harness, and `build` runs the pipeline as a stage.
if [ -z "${NAZM_PIPELINE:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/pipeline/main.nz" -o "$OUT/bin/nazm-pipeline" > "$OUT/bin/pipeline.log" 2>&1; then
        echo "tools-compare: nazm-pipeline did not build:"
        cat "$OUT/bin/pipeline.log"
        exit 2
    fi
    export NAZM_PIPELINE="$OUT/bin/nazm-pipeline"
fi

# The fixtures no tracked file can be: bytes that are no UTF-8, a program that does not parse.
printf '@bench\nfn b() -> Int { 1 }\n\377\376\n' > "$OUT/fx/binary.nz"
printf 'fn main() -> Int { 0 }\n@bench\nfn broken( -> Int { 1 }\n' > "$OUT/fx/syntax.nz"
# Every `@bench` function of these fails to build: nothing in this script runs a benchmark.
printf 'fn main() -> Int { 0 }\n@bench\nfn b() -> Str { "" }\n' > "$OUT/fx/notanz.txt"
: > "$OUT/fx/empty.nz"
# What `accel` and `contract` read.
printf '1 2 3 -4 0 9223372036854775807 -9223372036854775808\n' > "$OUT/fx/ints.txt"
printf '7 8\n' > "$OUT/fx/shortints.txt"
printf '1.5 2 -3.25 0.1 1e3 inf nan\n' > "$OUT/fx/floats.txt"
printf '0.5\n' > "$OUT/fx/shortfloats.txt"
printf '1 x 3\n' > "$OUT/fx/bad.txt"
printf '1 99999999999999999999\n' > "$OUT/fx/huge.txt"
printf '1 \377 3\n' > "$OUT/fx/binary.txt"
: > "$OUT/fx/empty.txt"
printf '# a ledger\n1 deposit 5\n1 withdraw 2\n2 nosuch 1\n' > "$OUT/fx/txs.txt"
printf 'fn k(x: Int) -> Int { x + "a" }\nfn main() -> Int { 0 }\n' > "$OUT/fx/typeerror.nz"

if [ ${#files[@]} -eq 0 ]; then
    cd "$ROOT" || exit 2
    while IFS= read -r f; do files+=("$f"); done < <(
        git ls-files 'examples/*.nz' 'examples/bad/*.nz' 'tests/compat/v1/*.nz' 'compiler/conformance/*.nz' \
            'toolchain/codegen/tests/programs/*.nz' 'crates/nazm-cli/tests/refusals/*.nz' 'crates/nazm-cli/tests/transfers/*.nz' \
            'toolchain/frontend-tests/inputs/check/*.nz' 'toolchain/frontend-tests/inputs/modules/*.nz' | LC_ALL=C sort -u
    )
fi

# NAZM_NO_LIBRARY=1 leaves NAZM_LIBRARY unset: the Nazm `nazm` then reads the library it carries (R8 slice 12).
if [ -n "${NAZM_NO_LIBRARY:-}" ]; then unset NAZM_LIBRARY; else export NAZM_LIBRARY="$ROOT/library"; fi
cd "$ROOT" || exit 2

agree=0
differ=0
refused=0
total_differ=0

# `case_run LABEL ARG...`: both sides run `nazm ARG...`; counts and reports.
case_run() {
    label=$1
    shift
    W=$OUT/work/case
    rm -rf "$W"
    mkdir -p "$W"
    "$REF" "$@" > "$W/ref.out" 2> "$W/ref.err" < /dev/null
    echo $? > "$W/ref.status"
    while :; do
        "$STAGE_RUN" "${TOOLS_DEADLINE:-300}" "$PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        # Another stage or a contained workload has the host: wait for it, not counted.
        if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/got.err"; then sleep 10; continue; fi
        break
    done
    echo $rc > "$W/got.status"
    if [ "$rc" = 6 ]; then
        refused=$((refused + 1))
        echo "refused  $label: $(head -c 200 "$W/got.err" | tr '\n' ' ')"
        return
    fi
    if cmp -s "$W/ref.out" "$W/got.out" && cmp -s "$W/ref.err" "$W/got.err" && cmp -s "$W/ref.status" "$W/got.status"; then
        agree=$((agree + 1))
        return
    fi
    differ=$((differ + 1))
    echo "differ   $label (status $(cat "$W/ref.status") vs $(cat "$W/got.status"))"
    for x in out err; do
        cmp -s "$W/ref.$x" "$W/got.$x" || diff "$W/ref.$x" "$W/got.$x" | head -${DIFF_LINES:-4} | cut -c1-200 | sed 's/^/    /'
    done
    if [ -n "${KEEP:-}" ]; then
        k=$KEEP/$(echo "$label" | tr '/ ' '__')
        mkdir -p "$k" && cp "$W"/* "$k"/
    fi
}

# `part_args NAME`: every command line of tests/args/NAME.txt, run from the repository's root.
part_args() {
    name=$1
    list=$ROOT/toolchain/commands/tests/args/$name.txt
    [ -f "$list" ] || { echo "$name: no $list"; return; }
    while IFS= read -r line; do
        case $line in '' | '#'*) continue ;; esac
        [ "$line" = '(none)' ] && line=
        line=${line//@FX@/$OUT/fx}
        # The words of a line as the shell reads them.
        eval "set -- $line"
        case_run "$name args: ${line:-(none)}" "$name" "$@"
    done < "$list"
}

for p in "${parts[@]}"; do
    agree=0; differ=0; refused=0
    case $p in
        args)
            for c in bench accel contract; do part_args $c; done ;;
        bench)
            # The fixtures, one at a time and together; every function that fails before it is run, and the files that hold none.
            fx=toolchain/commands/tests/bench
            for f in "$fx"/none.nz "$fx"/refused.nz "$fx"/two.nz "$fx"/cfg.nz "$fx"/dir "$fx"/empty "$fx"/dir/sub.nz "$fx"/dir/b.nz \
                "$OUT/fx/binary.nz" "$OUT/fx/syntax.nz" "$OUT/fx/notanz.txt" "$OUT/fx/empty.nz" "$OUT/fx/emptydir" "$OUT/fx/missing.nz"; do
                case_run "bench $f" bench "$f"
                case_run "bench --json $f" bench --json "$f"
                case_run "bench --iterations 7 $f" bench --iterations 7 "$f"
            done
            case_run "bench several" bench "$fx"/none.nz "$fx"/refused.nz "$fx"/two.nz
            case_run "bench several, one missing" bench "$fx"/refused.nz "$OUT/fx/missing.nz" "$fx"/two.nz
            ;;
        shape)
            # The row `bench` prints and the line it reads, on their own (tests/bench-shape.nz): nothing is measured.
            if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/tests/bench-shape.nz" -o "$OUT/bin/bench-shape" > "$OUT/bin/shape.log" 2>&1; then
                echo "tools-compare: bench-shape did not build:"; cat "$OUT/bin/shape.log"; exit 2
            fi
            while :; do
                "$STAGE_RUN" 60 "$OUT/bin/bench-shape" > "$OUT/work/shape.out" 2> "$OUT/work/shape.err"
                if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$OUT/work/shape.err"; then sleep 10; continue; fi
                break
            done
            n=$(wc -l < "$ROOT/toolchain/commands/tests/bench-shape.expected")
            if cmp -s "$OUT/work/shape.out" "$ROOT/toolchain/commands/tests/bench-shape.expected"; then agree=$n; else
                differ=1; echo "differ   bench-shape"; diff "$OUT/work/shape.out" "$ROOT/toolchain/commands/tests/bench-shape.expected" | head -6
            fi
            ;;
        accel)
            # The kernels of tests/accel (every function of each, with ints and floats, `--print-kernel` and not), then the first functions
            # of every input of check, each with `--print-kernel`.
            for f in toolchain/commands/tests/accel/*.nz; do
                for name in $(grep -o '^fn [A-Za-z_0-9]*' "$f" | sed 's/^fn //'); do
                    for kind in ints floats; do
                        case_run "accel $f $name $kind" accel "$f" "$name" --input "$OUT/fx/$kind.txt" --print-kernel
                    done
                    case_run "accel $f $name zip" accel "$f" "$name" --input "$OUT/fx/ints.txt" --input "$OUT/fx/ints.txt" --print-kernel
                    # What stops before a provider would run (the inputs and their number); the run itself is `accel-run.sh`'s (slice 11d).
                    case_run "accel $f $name bad input" accel "$f" "$name" --input "$OUT/fx/bad.txt"
                    case_run "accel $f $name two inputs" accel "$f" "$name" --input "$OUT/fx/ints.txt" --input "$OUT/fx/shortints.txt"
                    case_run "accel $f $name three inputs" accel "$f" "$name" --input "$OUT/fx/ints.txt" --input "$OUT/fx/ints.txt" --input "$OUT/fx/ints.txt"
                done
            done
            for f in "${files[@]}"; do
                for name in $(grep -o '^fn [A-Za-z_0-9]*' "$f" | sed 's/^fn //' | head -${TOOLS_NAMES:-3}) nosuch; do
                    case_run "accel $f $name" accel "$f" "$name" --input "$OUT/fx/ints.txt" --print-kernel
                done
            done
            ;;
        contract)
            for f in "${files[@]}"; do
                case_run "contract $f" contract "$f"
                case_run "contract $f --interface" contract "$f" --interface
            done
            for f in examples/contracts/*.nz toolchain/commands/tests/accel/int_map.nz crates/nazm-cli/templates/contract/src/main.nz \
                toolchain/frontend-tests/inputs/rs/{contract,accounts,evm,wasm_contract,contract_property}*.nz; do
                case_run "contract $f --txs" contract "$f" --txs "$OUT/fx/txs.txt"
                case_run "contract $f --accounts" contract "$f" --accounts "$OUT/fx/out"
                case_run "contract $f --upgrade-check itself" contract "$f" --upgrade-check "$f"
            done
            ;;
        *) echo "tools-compare: no part $p"; exit 2 ;;
    esac
    echo "$p: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
done
[ "$total_differ" -eq 0 ]
