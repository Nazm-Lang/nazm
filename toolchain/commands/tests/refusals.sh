#!/bin/bash
# R8 slice 11: what the Nazm `nazm` still refuses (`openspec/changes/r8-cut-over/design.md`, *Slice 11*, task 11.1).
#
#     toolchain/commands/tests/refusals.sh [-j N] [--cmds "CMD ..."] [-- FILE.nz ...]
#     CMD: check snapshot diagnostics resolve obligations explain-cost explain-flow mir fix contract profile-NAME lsp-MODE
#
# For every input of the corpus (the tracked programs of the example, compatibility, conformance, code-generator, refusal, transfer and
# front-end corpora, found with `find`, so that it runs where there is no Git) and every command asked, both sides run the same command line from the
# repository's root and the whole observable behaviour (standard output, standard error, status) is compared. A status 6 from the Nazm
# side is a refusal: counted apart, with its reason (the first line of its standard error, the file's name taken out), never an
# agreement. Prints one line per case that does not agree, then per command `CMD: A agree, D differ, R refused`, then the reasons
# of the refusals with their counts. Exit 0 only when nothing differs and nothing is refused.
#
# It works where run.sh and the other scripts of this directory cannot: contained (`cargo xtask contained --profile p8t8 run '...'`, the
# cases in parallel, no Git or Python in the image) and on a macOS host (one `host-run` at a time). The scripted sessions of `lsp-MODE`
# (a host step: `lsp-gen.sh`, Python) are read from LSP_DIR/MODE/.
#
# The Nazm `nazm` is built by the checkout's `nazm` ($NAZM, absolute) with a heap limit of at most 2 GiB; the reference is $NAZM_REF (default
# $NAZM). NAZM_PORT names a port already built. REFUSALS_OUT is the scratch directory; KEEP=DIR keeps each difference's two records.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../lib/stage.sh"

if [ "${1:-}" = --worker ]; then
    n=$2
    line=$(sed -n "${n}p" "$RF_OUT/cases")
    label=${line%%|*}
    rest=${line#*|}
    stdin=${rest%%|*}
    args=${rest#*|}
    W=$RF_OUT/w/$n
    mkdir -p "$W"
    cd "$RF_ROOT" || exit 2
    eval "set -- $args"
    inp=/dev/null
    [ -n "$stdin" ] && inp=$stdin
    NAZM_CACHE_DIR=$W/cache-ref "$RF_REF" "$@" > "$W/ref.out" 2> "$W/ref.err" < "$inp"
    echo $? > "$W/ref.status"
    while :; do
        NAZM_CACHE_DIR=$W/cache-got STAGE_STDIN=${stdin:-/dev/null} "$STAGE_RUN" "${RF_DEADLINE:-300}" "$RF_PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/got.err"; then sleep 10; continue; fi
        break
    done
    echo $rc > "$W/got.status"
    if [ "$rc" = 6 ] && [ "$(cat "$W/ref.status")" != 6 ]; then
        reason=$(head -n 1 "$W/got.err" | sed -e 's#[A-Za-z0-9_./-]*\.nz#FILE#g' | cut -c1-160)
        echo "refused|$label|$reason" > "$RF_OUT/res/$n"
    elif cmp -s "$W/ref.out" "$W/got.out" && cmp -s "$W/ref.err" "$W/got.err" && cmp -s "$W/ref.status" "$W/got.status"; then
        echo "agree|$label|" > "$RF_OUT/res/$n"
    else
        echo "differ|$label|status $(cat "$W/ref.status") vs $(cat "$W/got.status")" > "$RF_OUT/res/$n"
        if [ -n "${RF_KEEP:-}" ]; then mkdir -p "$RF_KEEP/$n" && cp "$W"/ref.* "$W"/got.* "$RF_KEEP/$n/" && echo "$label" > "$RF_KEEP/$n/label"; fi
    fi
    rm -rf "$W"
    exit 0
fi

stage_name=refusals
stage_init "$(cd "$here/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${REFUSALS_OUT:-${TMPDIR:-/tmp}/nazm-refusals}
rm -rf "$OUT/res" "$OUT/w" "$OUT/cases"
mkdir -p "$OUT/bin" "$OUT/res" "$OUT/w"
jobs=8
cmds="check snapshot diagnostics resolve obligations explain-cost explain-flow mir fix contract"
files=()
while [ $# -gt 0 ]; do
    case $1 in
        -j) jobs=$2; shift 2 ;;
        --cmds) cmds=$2; shift 2 ;;
        --) shift; files=("$@"); break ;;
        *) echo "refusals: unknown argument $1"; exit 2 ;;
    esac
done
[ "$STAGE_MODE" = host ] && jobs=1

if [ -z "${NAZM_PORT:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/nazm.nz" -o "$OUT/bin/nazm" > "$OUT/bin/nazm.log" 2>&1; then
        echo "refusals: nazm did not build:"; cat "$OUT/bin/nazm.log"; exit 2
    fi
fi
PORT=${NAZM_PORT:-$OUT/bin/nazm}
if [ -z "${NAZM_PIPELINE:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/pipeline/main.nz" -o "$OUT/bin/nazm-pipeline" > "$OUT/bin/pipeline.log" 2>&1; then
        echo "refusals: nazm-pipeline did not build:"; cat "$OUT/bin/pipeline.log"; exit 2
    fi
    export NAZM_PIPELINE="$OUT/bin/nazm-pipeline"
fi

cd "$ROOT" || exit 2
if [ ${#files[@]} -eq 0 ]; then
    while IFS= read -r f; do files+=("$f"); done < <(
        find examples tests/compat/v1 compiler/conformance toolchain/codegen/tests/programs crates/nazm-cli/tests/refusals \
            crates/nazm-cli/tests/transfers toolchain/frontend-tests/inputs/check toolchain/frontend-tests/inputs/modules -name '*.nz' | LC_ALL=C sort -u
    )
fi

: > "$OUT/cases"
add() { printf '%s|%s|%s\n' "$1" "$2" "$3" >> "$OUT/cases"; }
for f in "${files[@]}"; do
    for c in $cmds; do
        case $c in
            check) add "check $f" "" "check --no-cache $f"; add "check-json $f" "" "check --no-cache --json $f" ;;
            snapshot) add "snapshot $f" "" "snapshot --root $f --json" ;;
            diagnostics) add "diagnostics $f" "" "diagnostics --root $f --json" ;;
            resolve) add "resolve $f" "" "resolve $f" ;;
            obligations) add "obligations $f" "" "obligations $f" ;;
            explain-cost) add "explain-cost $f" "" "explain-cost $f" ;;
            explain-flow) add "explain-flow $f" "" "explain-flow $f" ;;
            mir) add "mir-spans $f" "" "mir --spans $f" ;;
            fix) add "fix $f" "" "fix $f" ;;
            contract) add "contract $f" "" "contract $f" ;;
            profile-*) add "$c $f" "" "check --no-cache --profile ${c#profile-} $f" ;;
            lsp-*)
                k=${c#lsp-}
                s=${LSP_DIR:?lsp needs LSP_DIR (lsp-gen.sh writes it)}/$k/$(echo "$f" | tr '/' '_').bin
                [ -f "$s" ] && add "lsp-$k $f" "$s" "lsp" ;;
            *) echo "refusals: no command $c"; exit 2 ;;
        esac
    done
done
total=$(wc -l < "$OUT/cases" | tr -d ' ')
export RF_OUT=$OUT RF_ROOT=$ROOT RF_PORT=$PORT RF_REF=$REF RF_KEEP=${KEEP:-}
export NAZM_LIBRARY="${NAZM_LIBRARY:-$ROOT/library}"
seq 1 "$total" | xargs -P "$jobs" -I{} bash "$here/refusals.sh" --worker {}

cat "$OUT"/res/* > "$OUT/all" 2> /dev/null
grep -v '^agree|' "$OUT/all" | sed -e 's/|/  /' -e 's/|/  /' | head -"${REFUSALS_LINES:-2000}"
for c in $(awk -F'|' '{split($2, a, " "); print a[1]}' "$OUT/all" | sort -u); do
    a=$(awk -F'|' -v c="$c" '$1=="agree"{split($2,x," "); if (x[1]==c) n++} END{print n+0}' "$OUT/all")
    d=$(awk -F'|' -v c="$c" '$1=="differ"{split($2,x," "); if (x[1]==c) n++} END{print n+0}' "$OUT/all")
    r=$(awk -F'|' -v c="$c" '$1=="refused"{split($2,x," "); if (x[1]==c) n++} END{print n+0}' "$OUT/all")
    echo "$c: $a agree, $d differ, $r refused"
done
echo "-- reasons"
awk -F'|' '$1=="refused"{split($2,x," "); print x[1] "  " $3}' "$OUT/all" | sort | uniq -c | sort -rn
bad=$(grep -c -v '^agree|' "$OUT/all")
[ "$bad" -eq 0 ]
