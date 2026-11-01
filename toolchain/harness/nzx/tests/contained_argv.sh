#!/bin/bash
# R8 slice 13 (S13.3): `nzx contained --print-argv` against `xtask contained --print-argv`, element
# for element, for every workload and profile.
#
#   toolchain/harness/nzx/tests/contained_argv.sh            (from anywhere; runs in the checkout)
#
# Environment: NZX (default `target/nzx/nzx`), XTASK (default `$CARGO_TARGET_DIR/debug/xtask`).
# Each case runs both runners from the checkout's root and diffs standard output (the `docker run`
# arguments, one a line) and standard error (the plan: deadline, bounds, export policy) and the
# status. `--print-argv` starts no container, creates no volume and no directory; two cases
# (`bench`, `archive`) and the probe's printing ask the image's platform with one `docker run --rm
# ... uname -sm`, so the script takes the shared container turn first (`~/.cache/nazm-scripts/
# contained-turn`) unless NZX_HAVE_TURN=1 says the caller holds it. It also asserts, on the nzx
# side, that the isolation flags are present and the never-present list (`--privileged`, `--cap-add`,
# `--device`, `--pid`, `--ipc`, `--userns`, `-v`, `--volume`, the Docker socket) is not, before the image.
# Ends `N cases agree (E argv elements compared), M differ`; exits 1 when anything differs.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
NZX=${NZX:-$root/target/nzx/nzx}
XTASK=${XTASK:-${CARGO_TARGET_DIR:-$root/target}/debug/xtask}
cd "$root" || exit 2
agree=0
differ=0
elements=0
work=$(mktemp -d "${TMPDIR:-/tmp}/nzx-contained-argv.XXXXXX")
turn=$HOME/.cache/nazm-scripts/contained-turn
took=0
cleanup() {
    rm -rf "$work"
    [ "$took" = 1 ] && rm -rf "$turn"
}
trap cleanup EXIT
if [ "${NZX_HAVE_TURN:-0}" != 1 ]; then
    mkdir -p "$(dirname "$turn")"
    until mkdir "$turn" 2> /dev/null; do
        echo "waiting for the container turn ($(cat "$turn/owner" 2> /dev/null))"
        sleep 60
    done
    echo "r8-slice13-contained-argv" > "$turn/owner"
    took=1
fi
have_amd64=0
docker image inspect nazm-contained-amd64:1.98.1 > /dev/null 2>&1 && have_amd64=1
[ -x "$NZX" ] || { echo "no nzx at $NZX: toolchain/harness/build.sh nzx" >&2; exit 2; }
[ -x "$XTASK" ] || { echo "no xtask at $XTASK: cargo build -p xtask" >&2; exit 2; }

# Never in the arguments before the image.
check_flags() {
    local file=$1 bad=0 saw=""
    local image
    image=$(grep -n -m1 '^nazm-contained' "$file" | cut -d: -f1)
    [ -n "$image" ] || { echo "no image line"; return 1; }
    local head
    head=$(head -n "$((image - 1))" "$file")
    for f in --privileged --cap-add --device --pid --ipc --userns -v --volume; do
        if printf '%s\n' "$head" | grep -qx -- "$f"; then echo "forbidden $f"; bad=1; fi
    done
    if printf '%s\n' "$head" | grep -q 'docker.sock'; then echo "forbidden docker.sock"; bad=1; fi
    for f in --init --memory --memory-swap --cpus --pids-limit --storage-opt --ulimit --tmpfs '--network' --log-driver; do
        printf '%s\n' "$head" | grep -qx -- "$f" || { echo "missing $f"; bad=1; }
    done
    return $bad
}

# `case_ NAME ARGS...`: both runners, the same arguments.
case_() {
    local name=$1
    shift
    # `--print-argv` goes after the options and before the workload, as both runners read it.
    local opts=()
    while [ $# -gt 0 ]; do
        case $1 in
            --platform | --profile | --cache) opts+=("$1"); [ $# -gt 1 ] && opts+=("$2"); shift; [ $# -gt 0 ] && shift ;;
            *) break ;;
        esac
    done
    "$XTASK" contained "${opts[@]+"${opts[@]}"}" --print-argv "$@" > "$work/x.out" 2> "$work/x.err"
    local xs=$?
    "$NZX" contained "${opts[@]+"${opts[@]}"}" --print-argv "$@" > "$work/n.out" 2> "$work/n.err"
    local ns=$?
    local why=""
    # A case that agrees by both refusing is no evidence: a plan must succeed, a refusal must say why.
    # Plans that read the amd64 image (its cache volume, its platform) refuse where it is not built.
    local want=plan
    case $name in refuse*) want=refuse ;; amd64*) [ "$have_amd64" = 1 ] || { [ "$name" = "amd64 release" ] || want=refuse; } ;; esac
    case $want in
        refuse) { [ "$ns" != 0 ] && [ -s "$work/n.err" ]; } || why="a refusal was expected" ;;
        *) [ "$ns" = 0 ] || why="a plan was expected, status $ns" ;;
    esac
    [ "$xs" = "$ns" ] || why="$why; status xtask $xs, nzx $ns"
    cmp -s "$work/x.out" "$work/n.out" || why="$why; stdout differs"
    cmp -s "$work/x.err" "$work/n.err" || why="$why; stderr differs"
    if [ "$name" != probe ] && [ "$xs" = 0 ]; then
        local m
        m=$(check_flags "$work/n.out") || why="$why; $m"
    fi
    if [ -z "$why" ]; then
        local n
        n=$(wc -l < "$work/n.out" | tr -d ' ')
        elements=$((elements + n))
        agree=$((agree + 1))
        printf 'agree   %-34s %s argv elements, status %s\n' "$name" "$n" "$ns"
    else
        differ=$((differ + 1))
        printf 'DIFFER  %-34s %s\n' "$name" "$why"
        diff "$work/x.out" "$work/n.out" | head -6
        diff "$work/x.err" "$work/n.err" | head -6
    fi
}

while IFS='|' read -r name args; do
    [ -n "$name" ] || continue
    # shellcheck disable=SC2086
    case_ "$name" $args
done << 'CASES'
selfhost|selfhost
selfhost p8t8|--profile p8t8 selfhost
selfhost p1|--profile p1 selfhost
tests|tests
tests p1|--profile p1 tests
tests p8t8|--profile p8t8 tests
tests p4w1t4|--profile p4w1t4 tests
tests p4w2t2|--profile p4w2t2 tests
tests p4w3t2|--profile p4w3t2 tests
tests p4w4t1|--profile p4w4t1 tests
tests p4w2t4|--profile p4w2t4 tests
bootstrap|bootstrap
bootstrap p8t8|--profile p8t8 bootstrap
selfbuild|selfbuild
selfbuild p8t8|--profile p8t8 selfbuild
selfbuild stages|selfbuild stages=2
selfbuild llvm|selfbuild stages=3 backend=llvm
selfbuild nazm|selfbuild backend=nazm
release|release
mutate|mutate
mutate p4t4|--profile p4t4 mutate
mutate plan|mutate --plan
mutate resume|mutate --resume
mutate only|mutate --only m1:x --strategy fast
mutate budget|mutate --budget-secs 100
mutate killer|mutate --verify-killer abc.d
bench|bench
bench save|bench --save
bench cache|--cache bench run true
fuzz|fuzz
fuzz 60|fuzz 60
fuzz targets|fuzz 60 front lower bogus
fuzz p1|--profile p1 fuzz 30 run
archive|archive
run|run echo hi
run spaces|run echo   hi there
run p8t8 cache|--profile p8t8 --cache bench run ls
amd64 selfhost|--platform amd64 selfhost
amd64 tests|--platform amd64 --profile p4t4 tests
amd64 bench|--platform amd64 bench --save
amd64 archive|--platform amd64 archive
amd64 release|--platform amd64 release
probe|probe
refuse platform|--platform arm64 selfhost
refuse profile|--profile p6 selfhost
refuse profile none|--profile
refuse cache|--cache src run x
refuse selfbuild|selfbuild bogus
refuse selfbuild digits|selfbuild stages=x
refuse mutate option|mutate --frobnicate
refuse mutate value|mutate --only
refuse mutate chars|mutate --only a;b
refuse run|run
refuse unknown|nonsense
refuse none|
refuse cache print|cache
CASES
echo "$agree cases agree ($elements argv elements compared), $differ differ"
[ "$differ" -eq 0 ]
