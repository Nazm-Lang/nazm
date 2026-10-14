#!/bin/sh
# R4's comparison (openspec/changes/r4-interpreter): build the interpreter written in Nazm with one
# prebuilt `nazm`, run it over every program of its inputs, and compare standard output, standard
# error and exit status with the Rust toolchain's `nazm run` — once plainly, once with the memory
# and result reports asked for. Prints each difference, then `N agree, M differ` (and how many
# inputs did not check, which are not the interpreter's to run).
#
#   toolchain/interp/tests/run.sh [-v] [FILE.nz ...]
#
# NAZM names the Rust toolchain (default: the 0.10.0 release). -v shows each difference's diff.
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
verbose=0
if [ "${1:-}" = "-v" ]; then verbose=1; shift; fi
work=$(mktemp -d "${TMPDIR:-/tmp}/r4-interp.XXXXXX")
trap 'rm -rf "$work"' EXIT

if ! "$NAZM" build --opt-level 2 "$repo/toolchain/interp/main.nz" -o "$work/interp" >"$work/build.log" 2>&1; then
    grep -v '^warning' "$work/build.log" | grep -A8 '^error' | head -40
    echo "interp: the build failed"
    exit 1
fi

# Every input: the compatibility corpus's run and trap cases, the examples and this track's own.
if [ $# -gt 0 ]; then
    cases="$*"
else
    cases=$( (ls "$repo"/tests/compat/v1/run/*.nz "$repo"/tests/compat/v1/run/*/main.nz \
        "$repo"/tests/compat/v1/trap/*.nz "$repo"/toolchain/interp/tests/cases/*.nz;
        find "$repo/examples" -name '*.nz' | sort) 2>/dev/null)
fi

# A run is bounded, so a hang in either interpreter is a difference rather than a stuck suite.
bounded() { perl -e 'alarm shift; exec @ARGV or exit 127' 60 "$@"; }

agree=0
differ=0
unchecked=0
for path in $cases; do
    dir=$(dirname "$path")
    file=$(basename "$path")
    name=${path#"$repo"/}
    # A program that does not check, or a library with no `main`, is the checker's to refuse.
    if ! (cd "$dir" && "$NAZM" core-ir --json "$file" >"$work/ir.json" 2>/dev/null) ||
        grep -q '^{"schema":"nazm.core-ir/2","entry":null' "$work/ir.json"; then
        unchecked=$((unchecked + 1))
        continue
    fi
    # Plainly; with the reports; and, for a program that fails at run time, with `--json`.
    modes="plain reports"
    while [ -n "$modes" ]; do
        mode=${modes%% *}
        if [ "$mode" = "$modes" ]; then modes=""; else modes=${modes#* }; fi
        # The interpreter takes the reports as flags: as a compiled Nazm program, its own runtime
        # would answer the variables about itself (main.nz).
        json=""
        if [ "$mode" = reports ]; then
            set -- env NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 NAZM_RESULT_REPORT=1
            flags="--memory-report --sched-report --result-report"
        else
            set -- env
            flags=""
            [ "$mode" = json ] && json="--json"
        fi
        (cd "$dir" && bounded "$@" "$NAZM" run $json "$file" </dev/null >"$work/rust.out" 2>"$work/rust.err")
        rust=$?
        (cd "$dir" && bounded "$work/interp" $json $flags --std "$repo/library" "$work/ir.json" "$file" \
            </dev/null >"$work/nazm.out" 2>"$work/nazm.err")
        nazm=$?
        [ "$mode" = plain ] && [ "$rust" = 2 ] && modes="$modes json"
        what=""
        cmp -s "$work/rust.out" "$work/nazm.out" || what="$what stdout"
        cmp -s "$work/rust.err" "$work/nazm.err" || what="$what stderr"
        [ "$rust" = "$nazm" ] || what="$what status($rust/$nazm)"
        if [ -z "$what" ]; then
            agree=$((agree + 1))
        else
            differ=$((differ + 1))
            echo "differ: $name [$mode]$what"
            if [ $verbose = 1 ]; then
                diff "$work/rust.out" "$work/nazm.out" | head -10
                diff "$work/rust.err" "$work/nazm.err" | head -10
            fi
        fi
    done
done
echo "$agree agree, $differ differ ($unchecked inputs do not check)"
[ $differ = 0 ]
