#!/bin/bash
# The R3 pipeline against the Rust toolchain: build it with $NAZM_BUILD (the 0.10.0 release), run
# it over every input that has Core IR, and compare its MIR with `nazm mir` and its LIR with
# `nazm lir` / `nazm lir --ops` (the reference `nazm`, $NAZM, built from main), byte for byte; and
# hold the validator to its mutants. Prints each difference's file and first differing line (or the
# pipeline's refusal), then the summary lines, over the base set and over the full set.
#
#     NAZM=PATH/TO/main/nazm toolchain/pipeline/tests/run.sh [-v] [-j N] [FILE.nz ...]
#
# The inputs: the base set — the brief's (tests/compat/v1, corpus, examples, bench/compare/nazm),
# the Nazm compiler under compiler/ and this pipeline itself — and the front end's test programs
# (toolchain/frontend-tests/inputs), which with the base set make the full set. $NAZM must print
# `nazm.core-ir/3`. Run from the repository root. Never normalises either side — the tables
# comparison leaves out the member the text comparison covers, and compares every other byte: a
# difference is a bug here or a finding to report (openspec/changes/r3-pipeline/design.md). A
# construct not ported yet stops the unit, and counts as a difference.
set -u
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
NAZM_BUILD=${NAZM_BUILD:-$HOME/.local/nazm-0.10.0/bin/nazm}
VERBOSE=0
JOBS=4
while [ $# -gt 0 ]; do
    case "$1" in
        -v) VERBOSE=1; shift ;;
        -j) JOBS=$2; shift 2 ;;
        *) break ;;
    esac
done
ROOT=$(pwd)
OUT="$ROOT/toolchain/pipeline/target"
mkdir -p "$OUT/cmp"
"$NAZM_BUILD" build toolchain/pipeline/main.nz -o "$OUT/pipeline" > "$OUT/build.log" 2>&1 || {
    cat "$OUT/build.log"; echo "pipeline: the build failed"; exit 2; }
if ! python3 toolchain/pipeline/tests/runtime_table.py --check; then
    echo "pipeline: runtime.nz is not the runtime's registry; run tests/runtime_table.py"; exit 2
fi
if ! python3 toolchain/pipeline/tests/runtime_text.py --check; then
    echo "pipeline: rtext.nz is not the runtime crate's texts; run tests/runtime_text.py"; exit 2
fi
mkdir -p "$OUT/cache"
if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$OUT/inputs"
else
    { find tests/compat/v1 corpus examples bench/compare/nazm compiler toolchain/frontend-tests/inputs -name '*.nz' | sort
      echo toolchain/pipeline/main.nz; } > "$OUT/inputs"
fi

# One input: its results as `KIND VERDICT [DETAIL]` lines in `$o.res`.
one() {
    f=$1
    o="$OUT/cmp/$(echo "$f" | tr '/' '_')"
    r="$o.res"
    : > "$r"
    # An input with no Core IR (a program that does not check) or no MIR is not this stage's.
    if ! "$NAZM" core-ir --json --with-sources "$f" > "$o.cir" 2>/dev/null; then echo "skip" >> "$r"; return; fi
    if ! "$NAZM" mir "$f" > "$o.want" 2>/dev/null; then echo "skip" >> "$r"; return; fi
    # The validator against its mutants: every one must be caught.
    perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" mutants "$o.cir" > "$o.mut" 2>&1
    grep "^missed" "$o.mut" | sed "s|^|validator: $f: |" | head -5 | sed 's/^/note /' >> "$r"
    if s=$(grep -E "^[0-9]+ mutants, [0-9]+ caught$" "$o.mut"); then
        m=${s%% mutants*}; k=${s##*, }; k=${k%% caught}
        echo "mut $m $k" >> "$r"
    fi
    if perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" mir "$o.cir" > "$o.got" 2> "$o.err" && cmp -s "$o.got" "$o.want"; then
        echo "mir agree" >> "$r"
    elif [ -s "$o.err" ]; then
        echo "mir differ $(head -1 "$o.err")" >> "$r"
    else
        echo "mir differ $(diff "$o.want" "$o.got" | head -3 | cut -c1-240 | tr '\n' ' ')" >> "$r"
    fi
    # LIR's tables (task 6.1, 6.2): `nazm lir` with each unit's instruction text taken out
    # (`tables.py`, which re-serialises byte for byte); then the instruction text (6.3) against
    # `nazm lir --ops`; then the whole document, text and digests included.
    if ! "$NAZM" lir "$f" > "$o.lwant0" 2>/dev/null; then echo "noir" >> "$r"; return; fi
    python3 toolchain/pipeline/tests/tables.py < "$o.lwant0" > "$o.lwant"
    "$NAZM" lir --ops "$f" > "$o.owant" 2>/dev/null
    if perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" ops "$o.cir" > "$o.ogot" 2> "$o.oerr" && cmp -s "$o.ogot" "$o.owant"; then
        echo "ops agree" >> "$r"
        if perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" lir "$o.cir" > "$o.fgot" 2>/dev/null && cmp -s "$o.fgot" "$o.lwant0"; then
            echo "doc agree" >> "$r"
        else
            echo "doc differ" >> "$r"
        fi
    elif [ -s "$o.oerr" ]; then
        echo "ops stop $(head -1 "$o.oerr")" >> "$r"
    else
        # Text that differs with no stop: a wrong agreement in the making — always shown.
        echo "ops wrong $(cmp "$o.owant" "$o.ogot" 2>&1 | head -1 | sed "s/.* differ: //")" >> "$r"
    fi
    # The runtime's units (task 4.1's runtime generator): `pipeline runtime` against the units
    # `nazm build --emit-ir` wrote beside the program's, by default and with a stack size and a
    # heap limit (`units.py`); an input `nazm build` refuses is counted apart.
    rm -f "$o".bin*
    if NAZM_CACHE_DIR="$OUT/cache" "$NAZM" build --emit-ir "$f" -o "$o.bin" > /dev/null 2>&1; then
        if perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" runtime "$o.cir" > "$o.rgot" 2> "$o.rerr"; then
            v=$(python3 toolchain/pipeline/tests/units.py "$o.rgot" "$o.bin")
        else
            v="differ $(head -1 "$o.rerr")"
        fi
        if [ "$v" = agree ]; then
            rm -f "$o".bin*
            if NAZM_CACHE_DIR="$OUT/cache" "$NAZM" build --emit-ir --stack-size 1048576 --heap-limit 268435456 "$f" -o "$o.bin" > /dev/null 2>&1 \
                && perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" runtime "$o.cir" 1048576 268435456 > "$o.rgot" 2> "$o.rerr"; then
                v=$(python3 toolchain/pipeline/tests/units.py "$o.rgot" "$o.bin")
                [ "$v" = agree ] || v="$v (with a stack size and a heap limit)"
            fi
        fi
        echo "rt $v" >> "$r"
        rm -f "$o".bin*
    else
        echo "rt nobuild" >> "$r"
    fi
    if perl -e 'alarm 300; exec @ARGV' "$OUT/pipeline" tables "$o.cir" > "$o.lgot" 2> "$o.lerr" && cmp -s "$o.lgot" "$o.lwant"; then
        echo "tables agree" >> "$r"
    elif [ -s "$o.lerr" ]; then
        echo "tables differ $(head -1 "$o.lerr")" >> "$r"
    else
        echo "tables differ $(cmp "$o.lwant" "$o.lgot" 2>&1 | head -1 | sed "s/.* differ: //")" >> "$r"
    fi
}
export -f one
export NAZM OUT
tr '\n' '\0' < "$OUT/inputs" | xargs -0 -n 1 -P "$JOBS" bash -c 'one "$0"'

# The summary over a set of inputs (`base` leaves out the front end's test programs).
summary() {
    label=$1; filter=$2; details=$3
    while IFS= read -r f; do
        case "$f" in toolchain/frontend-tests/*) [ "$filter" = base ] && continue ;; esac
        o="$OUT/cmp/$(echo "$f" | tr '/' '_')"
        sed "s|^|$f	|" "$o.res"
    done < "$OUT/inputs" > "$OUT/results.$label"
    awk -F'\t' -v label="$label" -v verbose="$VERBOSE" -v details="$details" '
        { split($2, w, " "); k = w[1]; v = w[2]; rest = $2; sub(/^[^ ]+ [^ ]+ ?/, "", rest) }
        k == "skip" { skipped++ }
        k == "noir" { lskipped++ }
        k == "note" { if (details) print $2 }
        k == "mut" { made += w[2]; caught += w[3] }
        k == "mir" { if (v == "agree") magree++; else { mdiffer++; if (details) print "differ: " $1 ": " rest } }
        k == "tables" { if (v == "agree") lagree++; else { ldiffer++; if (details) print "lir differ: " $1 ": " rest } }
        k == "ops" {
            if (v == "agree") oagree++
            else if (v == "wrong") { owrong++; if (details) print "ops WRONG TEXT: " $1 ": " rest }
            else { ostop++; if (verbose && details) print "ops stop: " $1 ": " rest }
        }
        k == "doc" { if (v == "agree") fagree++; else { fdiffer++; if (details) print "lir document differ: " $1 } }
        k == "rt" {
            if (v == "agree") ragree++
            else if (v == "nobuild") rnobuild++
            else { rdiffer++; if (details) print "runtime differ: " $1 ": " rest }
        }
        END {
            printf "[%s] validator: %d mutants, %d caught, %d missed\n", label, made, caught, made - caught
            printf "[%s] mir: %d agree, %d differ (%d inputs without Core IR or MIR)\n", label, magree, mdiffer, skipped
            printf "[%s] lir tables: %d agree, %d differ (%d inputs with MIR but no LIR)\n", label, lagree, ldiffer, lskipped
            printf "[%s] lir ops: %d agree, %d differ (%d stops naming an unported construct, %d different text; the whole document: %d agree, %d differ)\n", label, oagree, ostop + owrong, ostop, owrong, fagree, fdiffer
            printf "[%s] runtime units: %d agree, %d differ (%d inputs with LIR that nazm build refuses)\n", label, ragree, rdiffer, rnobuild
        }' "$OUT/results.$label"
}
# The details once: with the full set's summary when there is one, else with the base set's.
if grep -q '^toolchain/frontend-tests/' "$OUT/inputs"; then
    summary base base 0
    summary full full 1
else
    summary base base 1
fi
