#!/bin/bash
# R8 slice 11: what a tool reads of a compilation that does not check clean (the reference index, the call graph, the provenance, the effects), R2's facts
# against the Rust ones — the comparisons `run.sh` makes only of programs that check.
#
#     toolchain/frontend-tests/errors.sh want DIR [MODE...]      host: the reference dumper's answers (`refdump MODE FILE`, `R2_WITH_ERRORS=1`) for every
#                                                                input, written under DIR/MODE/ (the reference is a Rust binary: `cargo build --release` in
#                                                                `toolchain/frontend-tests/refdump`, `REFDUMP` names it)
#     toolchain/frontend-tests/errors.sh compare DIR [MODE...]   anywhere a Nazm-built program may run (contained, or on a macOS host one at a time): the
#                                                                `compiler/diagnose.nz --MODE` answers of the same inputs against DIR's; prints
#                                                                `MODE: N agree, M differ` and the first differences; exit 1 where any differ
#
# MODE: refs calls provenance effects (the four by default). The inputs are `toolchain/frontend-tests/run.sh`'s, those that do not check clean first:
# every program of the repository's corpora. DIR is under the checkout for a container (which sees it, read-only, at /src).
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cmd=${1:?want or compare}
dir=${2:?a directory}
shift 2
modes=${*:-refs calls provenance effects}
cd "$root" || exit 2
list() {
    find tests/compat/v1 corpus examples compiler/conformance compiler/parity toolchain crates/nazm-cli/tests crates/nazm-cli/templates -name '*.nz' -type f 2> /dev/null
    ls compiler/*.nz
}
flat() { echo "$1" | tr '/' '_'; }
case $cmd in
    want)
        ref=${REFDUMP:?REFDUMP names the reference dumper}
        export R2_WITH_ERRORS=1
        for m in $modes; do
            mkdir -p "$dir/$m"
            list | LC_ALL=C sort -u | while IFS= read -r f; do "$ref" "$m" "$root/$f" > "$dir/$m/$(flat "$f")" 2> /dev/null; done
        done ;;
    compare)
        . "$root/toolchain/lib/stage.sh"
        stage_name=r2-errors
        stage_init "$root"
        nazm=${NAZM:-$(stage_checkout_nazm "$root")}
        exe=${R2_EXE:-${TMPDIR:-/tmp}/r2-errors-diagnose}
        stage_build "$nazm" --stack-size 256M "$root/compiler/diagnose.nz" -o "$exe" > /dev/null || { echo "errors.sh: diagnose did not build"; exit 2; }
        export R2_WITH_ERRORS=1
        total_bad=0
        for m in $modes; do
            res=$(mktemp -d)
            export E_ROOT=$root E_DIR=$dir E_MODE=$m E_EXE=$exe E_RES=$res E_RUN=$STAGE_RUN
            list | LC_ALL=C sort -u | tr '\n' '\0' | xargs -0 -n 1 -P "$(stage_jobs 8)" bash -c '
                f=$1
                n=$(echo "$f" | tr "/" "_")
                [ -f "$E_DIR/$E_MODE/$n" ] || exit 0
                "$E_RUN" 60 "$E_EXE" --"$E_MODE" "$E_ROOT/$f" "$E_ROOT/library" macos aarch64 > "$E_RES/$n.got" 2> /dev/null
                # The files the reference dumped name this checkout by the path the host ran in: the dump holds spans only, never a path.
                if cmp -s "$E_DIR/$E_MODE/$n" "$E_RES/$n.got"; then echo agree > "$E_RES/$n.v"; else echo "$f" > "$E_RES/$n.v"; fi
                ' _
            agree=$(grep -l '^agree$' "$res"/*.v 2> /dev/null | wc -l | tr -d ' ')
            differ=$(grep -L '^agree$' "$res"/*.v 2> /dev/null | wc -l | tr -d ' ')
            for v in $(grep -L '^agree$' "$res"/*.v 2> /dev/null | head -${E_SHOW:-40}); do echo "differ  $m $(cat "$v")"; done
            echo "$m: $agree agree, $differ differ"
            total_bad=$((total_bad + differ))
            rm -rf "$res"
        done
        [ "$total_bad" -eq 0 ] ;;
    *) echo "errors.sh: want or compare"; exit 2 ;;
esac
