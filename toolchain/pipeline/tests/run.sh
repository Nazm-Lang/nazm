#!/bin/bash
# The R3 pipeline against the Rust toolchain: build it with $NAZM, run it over every input that
# has Core IR, and compare its MIR with `nazm mir`, byte for byte. Prints each difference's file and
# first differing line (or the pipeline's refusal), then one summary line: `N agree, M differ`.
#
#     NAZM=$HOME/.local/nazm-0.10.0/bin/nazm toolchain/pipeline/tests/run.sh [-v] [FILE.nz ...]
#
# Run from the repository root. Never normalises either side: a difference is a bug here or a
# finding to report (openspec/changes/r3-pipeline/design.md).
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
VERBOSE=0
if [ "${1:-}" = "-v" ]; then VERBOSE=1; shift; fi
ROOT=$(pwd)
OUT="$ROOT/toolchain/pipeline/target"
mkdir -p "$OUT/cmp"
"$NAZM" build toolchain/pipeline/main.nz -o "$OUT/pipeline" > "$OUT/build.log" 2>&1 || {
    cat "$OUT/build.log"; echo "pipeline: the build failed"; exit 2; }
if [ $# -gt 0 ]; then
    FILES=("$@")
else
    FILES=()
    while IFS= read -r f; do FILES+=("$f"); done < <(find tests/compat/v1 corpus examples bench/compare/nazm -name '*.nz' | sort)
fi
agree=0; differ=0; skipped=0; made=0; caught=0; missed=0
for f in "${FILES[@]}"; do
    o="$OUT/cmp/$(echo "$f" | tr '/' '_')"
    # An input with no Core IR (a program that does not check) or no MIR is not this stage's.
    if ! "$NAZM" core-ir --json "$f" > "$o.cir" 2>/dev/null; then skipped=$((skipped + 1)); continue; fi
    if ! "$NAZM" mir "$f" > "$o.want" 2>/dev/null; then skipped=$((skipped + 1)); continue; fi
    # The validator against its mutants: every one must be caught.
    if "$OUT/pipeline" mutants "$o.cir" > "$o.mut" 2>&1 || grep -q "^missed" "$o.mut"; then :; fi
    grep "^missed" "$o.mut" | sed "s|^|validator: $f: |"
    if s=$(grep -E "^[0-9]+ mutants, [0-9]+ caught$" "$o.mut"); then
        m=${s%% mutants*}; k=${s##*, }; k=${k%% caught}
        made=$((made + m)); caught=$((caught + k)); missed=$((missed + m - k))
    fi
    if "$OUT/pipeline" mir "$o.cir" > "$o.got" 2> "$o.err" && cmp -s "$o.got" "$o.want"; then
        agree=$((agree + 1))
    else
        differ=$((differ + 1))
        if [ -s "$o.err" ]; then
            echo "differ: $f: $(head -1 "$o.err")"
        else
            echo "differ: $f: $(diff "$o.want" "$o.got" | head -3 | cut -c1-240 | tr '\n' ' ')"
        fi
        if [ $VERBOSE = 1 ]; then diff "$o.want" "$o.got" | head -20; fi
    fi
done
echo "validator: $made mutants, $caught caught, $missed missed"
echo "mir: $agree agree, $differ differ ($skipped inputs without Core IR or MIR)"
