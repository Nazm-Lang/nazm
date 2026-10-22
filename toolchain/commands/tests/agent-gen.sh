#!/bin/bash
# R8 slice 11: the agent commands' cases, written on the host (they need Python and the reference's own answers, which a container has not), for
# `refusals.sh --cases FILE` to run contained, in parallel.
#
#     toolchain/commands/tests/agent-gen.sh REF OUTDIR [FILE.nz ...]
#
# REF is the reference `nazm`. For every input (the corpus `refusals.sh` runs, or the FILEs named) it writes lines of OUTDIR/cases.txt, `LABEL||ARGS`:
# `context` at the offsets `context-positions.py` chooses; `references` of the first three definitions the reference's snapshot lists; `delta` against that
# snapshot and two baselines made wrong; `diagnostics --detail` of the first three diagnostics; `patch rename` at six names and `patch fix` at the diagnostics that
# carry a fix (`patch-positions.py`). The baselines are OUTDIR/base/NAME.json (`/src/.cases/base/NAME.json` where OUTDIR is `.cases` of a checkout a container
# mounts at /src).
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
ref=$1
out=$2
shift 2
mkdir -p "$out/base"
: > "$out/cases.txt"
cd "$root" || exit 2
if [ $# -gt 0 ]; then
    files=("$@")
else
    files=()
    while IFS= read -r f; do files+=("$f"); done < <(
        find examples tests/compat/v1 compiler/conformance toolchain/codegen/tests/programs crates/nazm-cli/tests/refusals \
            crates/nazm-cli/tests/transfers toolchain/frontend-tests/inputs/check toolchain/frontend-tests/inputs/modules -name '*.nz' | LC_ALL=C sort -u
    )
fi
shown=${AGENT_BASE_PREFIX:-$out}
for f in "${files[@]}"; do
    n=$(echo "$f" | tr '/' '_')
    k=0
    while IFS= read -r b; do
        echo "context $f @$b||context --root $f --file $f --byte $b --json" >> "$out/cases.txt"
        k=$((k + 1))
        [ $k -le 3 ] && echo "context-indented $f @$b||context --root $f --file $f --byte $b" >> "$out/cases.txt"
    done < <(python3 "$here/context-positions.py" "$f" "${AGENT_POSITIONS:-6}")
    "$ref" snapshot --json --root "$f" > "$out/base/$n.json" 2> /dev/null
    if [ -s "$out/base/$n.json" ]; then
        i=0
        while IFS= read -r id; do
            i=$((i + 1))
            echo "references $f #$i||references '$id' $f" >> "$out/cases.txt"
        done < <(grep -o '"identity":"[^"]*"' "$out/base/$n.json" | head -3 | sed 's/"identity":"\(.*\)"/\1/')
        sed 's/"source":"\([0-9a-e]\)/"source":"f/' "$out/base/$n.json" > "$out/base/$n.tweak.json"
        sed 's/"identity":"\([^"]*\)::fn \([a-z_]*\)"/"identity":"\1::fn zz_\2"/' "$out/base/$n.json" > "$out/base/$n.renamed.json"
        echo "delta $f||delta --json --root $f --baseline $shown/base/$n.json" >> "$out/cases.txt"
        echo "delta-indented $f||delta --root $f --baseline $shown/base/$n.tweak.json" >> "$out/cases.txt"
        echo "delta-renamed $f||delta --json --root $f --baseline $shown/base/$n.renamed.json" >> "$out/cases.txt"
    fi
    "$ref" diagnostics --json --root "$f" > "$out/base/$n.index.json" 2> /dev/null
    state=$(grep -o '"source":"[0-9a-f]*"' "$out/base/$n.index.json" | head -1 | sed 's/"source":"\(.*\)"/\1/')
    if [ -n "$state" ]; then
        i=0
        for id in $(grep -o '"id":"[^"]*"' "$out/base/$n.index.json" | head -3 | sed 's/"id":"\(.*\)"/\1/'); do
            i=$((i + 1))
            echo "detail $f #$i||diagnostics --json --root $f --detail $id --state $state" >> "$out/cases.txt"
        done
    fi
    { "$ref" check --json --no-cache "$f" 2>&1 > /dev/null | python3 -I "$here/patch-positions.py" "$root/$f"; } 2> /dev/null > "$out/base/$n.positions"
    i=0
    while read -r kind byte new; do
        [ -n "$kind" ] || continue
        i=$((i + 1))
        if [ "$kind" = rename ]; then echo "patch-rename $f #$i||patch rename --json --root $f --file $f --byte $byte --new-name $new" >> "$out/cases.txt"; fi
        if [ "$kind" = fix ]; then echo "patch-fix $f #$i||patch fix --json --root $f --file $f --byte $byte" >> "$out/cases.txt"; fi
    done < "$out/base/$n.positions"
done
wc -l < "$out/cases.txt"
