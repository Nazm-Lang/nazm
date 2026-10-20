#!/bin/bash
# R8 slice 8: the inspection commands written in Nazm against the Rust ones
# (`openspec/changes/r8-cut-over/design.md`, task 8.1; R5's method, `openspec/changes/r5-commands/`).
#
#     toolchain/commands/tests/inspect.sh [PART...] [-- FILE.nz ...]
#     PART: core-ir mir lir capabilities args obligations inspect explain-cost explain-flow trace profile
#           (all by default; INSPECT_PARTS="mir lir" does the same)
#
# The Nazm `nazm` (toolchain/commands/nazm.nz), the pipeline (`NAZM_PIPELINE`, which `mir` and `lir` run
# as a stage) are built by the checkout's `nazm` ($NAZM) with a heap limit of at most 2 GiB
# (`toolchain/lib/stage.sh`, the R6 rule). Each Nazm run is one `host-run`, one at a time (a refusal
# because another stage is running is waited out, not counted); the Rust side runs directly. Both run
# from the repository's root with the file named by its path under it, so a diagnostic names the same
# file on both sides. A case agrees when standard output, standard error and the exit status are the
# same bytes. A status 6 from the Nazm side (what it does not carry) is counted apart, with its
# reason, and is never an agreement. Prints a line per case that does not agree, then one line per
# part: `PART: A agree, D differ, R refused`. Exit 0 only when nothing differs.
#
# The reference is $NAZM_REF (default $NAZM). The inputs are the tracked programs of the
# conformance, compatibility, example and front-end corpora; BIG=1 adds the toolchain's own
# sources. KEEP=DIR keeps each difference's two records.
set -u
stage_name=inspect-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${INSPECT_OUT:-${TMPDIR:-/tmp}/nazm-inspect-compare}
mkdir -p "$OUT/bin" "$OUT/work"

parts=()
files=()
while [ $# -gt 0 ]; do
    case $1 in
        --) shift; files=("$@"); break ;;
        *) parts+=("$1"); shift ;;
    esac
done
[ ${#parts[@]} -gt 0 ] || read -r -a parts <<< "${INSPECT_PARTS:-core-ir mir lir capabilities args obligations inspect explain-cost explain-flow trace profile}"

for p in nazm:toolchain/commands/nazm.nz nazm-pipeline:toolchain/pipeline/main.nz; do
    name=${p%%:*}
    if [ -z "${NAZM_PORT:-}" ] || [ "$name" != nazm ]; then
        if ! stage_build "$NAZM" --stack-size 256M "$ROOT/${p#*:}" -o "$OUT/bin/$name" > "$OUT/bin/$name.log" 2>&1; then
            echo "inspect-compare: $name did not build:"
            cat "$OUT/bin/$name.log"
            exit 2
        fi
    fi
done
PORT=${NAZM_PORT:-$OUT/bin/nazm}

if [ ${#files[@]} -eq 0 ]; then
    cd "$ROOT" || exit 2
    while IFS= read -r f; do files+=("$f"); done < <(
        {
            git ls-files 'examples/*.nz' 'examples/bad/*.nz' 'tests/compat/v1/*.nz' 'compiler/conformance/*.nz' \
                'toolchain/codegen/tests/programs/*.nz' 'crates/nazm-cli/tests/refusals/*.nz' 'crates/nazm-cli/tests/transfers/*.nz' \
                'toolchain/frontend-tests/inputs/check/*.nz' 'toolchain/frontend-tests/inputs/modules/*.nz'
            if [ "${BIG:-}" = 1 ]; then git ls-files 'compiler/*.nz' 'toolchain/commands/*.nz' 'toolchain/pipeline/*.nz'; fi
        } | LC_ALL=C sort -u
    )
fi

export NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline"
cd "$ROOT" || exit 2

agree=0
differ=0
refused=0
total_differ=0

# `case LABEL ARG...`: both sides run `nazm ARG...`; counts and reports.
case_run() {
    label=$1
    shift
    W=$OUT/work/case
    rm -rf "$W"
    mkdir -p "$W"
    "$REF" "$@" > "$W/ref.out" 2> "$W/ref.err" < /dev/null
    echo $? > "$W/ref.status"
    while :; do
        "$STAGE_RUN" "${INSPECT_DEADLINE:-300}" "$PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        # Another stage or a contained workload (the other lanes') has the host: wait for it, not counted.
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

# `part NAME MODE...`: every input under each MODE (its words, the file appended).
part_files() {
    name=$1
    shift
    agree=0
    differ=0
    refused=0
    for f in "${files[@]}"; do
        for mode in "$@"; do
            # shellcheck disable=SC2086
            case_run "$name $mode $f" $name $mode "$f"
        done
    done
    echo "$name: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
}

# `part_lines NAME`: every command line of tests/args/NAME.txt, run in `$OUT/fixture`.
part_args() {
    name=$1
    agree=0
    differ=0
    refused=0
    list=$ROOT/toolchain/commands/tests/args/$name.txt
    [ -f "$list" ] || { echo "$name args: no $list"; return; }
    while IFS= read -r line; do
        case $line in '' | '#'*) continue ;; esac
        [ "$line" = '(none)' ] && line=
        # shellcheck disable=SC2086
        case_run "$name args: ${line:-(none)}" $name $line
    done < "$list"
    echo "$name args: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
}

for p in "${parts[@]}"; do
    case $p in
        core-ir) part_files core-ir "" --json --no-spans --with-sources ;;
        mir) part_files mir "" --json --spans ;;
        lir) part_files lir "" --ops "--ops --debug" "--target x86_64-apple-darwin" ;;
        capabilities)
            agree=0; differ=0; refused=0
            case_run "capabilities" capabilities
            case_run "capabilities --json" capabilities --json
            echo "capabilities: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        args) for c in core-ir mir lir capabilities obligations inspect explain-cost explain-flow trace profile; do part_args "$c"; done ;;
        obligations) part_files obligations "" --json ;;
        inspect) part_files inspect "" ;;
        explain-cost) part_files explain-cost "" --json ;;
        explain-flow) part_files explain-flow "" --json ;;
        trace)
            # Every requirements file of tests/trace against the heater (examples/requirements) and the program
            # with a failing test; the Nazm side runs the tests by the reference (NAZM_TEST_DRIVER).
            agree=0; differ=0; refused=0
            export NAZM_TEST_DRIVER=$REF
            for r in toolchain/commands/tests/trace/*.toml examples/requirements/requirements.toml; do
                for prog in examples/requirements/heater.nz toolchain/commands/tests/trace/failing.nz; do
                    case_run "trace $prog --requirements $r" trace "$prog" --requirements "$r"
                done
            done
            case_run "trace --source-root" trace examples/requirements/heater.nz --requirements examples/requirements/requirements.toml --source-root examples/requirements
            echo "trace: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        profile)
            agree=0; differ=0; refused=0
            for f in examples/max.nz examples/bad/assign_to_immutable.nz; do case_run "profile $f" profile "$f"; done
            echo "profile: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        *) echo "inspect-compare: no part $p"; exit 2 ;;
    esac
done
[ "$total_differ" -eq 0 ]
