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
# `fuzz-seed` is the reference's `fuzz` (the Rust fuzzer over the seed), compared the same way.
# `mutate-seed` is the reference's `mutate` (the Rust campaign over the seed's entries): its cases run
# `xtask contained mutate` against `nzx contained mutate-seed`, the workload's name normalised, and with a
# `--only` the plans are element for element equal; with none the selection is the seed's entries by name
# (S13.4), so the script must be the reference's script with that selection added, and nothing else. The
# Nazm `mutate` (13.4) has no reference: its cases check the plan's facts. Ends `N cases agree (E argv
# elements compared), M differ`; exits 1 when anything differs.
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
    "$NZX" contained "${opts[@]+"${opts[@]}"}" --print-argv "$@" > "$work/n.out" 2> "$work/n.err0"
    local ns=$?
    # The unknown-workload message lists `mutate-seed` too, the one workload the reference does not have.
    sed 's/ | mutate-seed//; s/ | fuzz-seed \[SECONDS\] \[TARGET...\]//' "$work/n.err0" > "$work/n.err"
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

# `case_seed NAME ARGS...`: `xtask contained ... mutate ...` against `nzx contained ... mutate-seed ...`.
case_seed() {
    local name=$1
    shift
    local opts=() xargs=() nargs=() explicit=0
    while [ $# -gt 0 ]; do
        case $1 in
            --platform | --profile | --cache) opts+=("$1"); [ $# -gt 1 ] && opts+=("$2"); shift; [ $# -gt 0 ] && shift ;;
            *) break ;;
        esac
    done
    for a in "$@"; do
        xargs+=("$a")
        if [ "$a" = mutate ]; then nargs+=(mutate-seed); elif [ "$a" = fuzz ]; then nargs+=(fuzz-seed); else nargs+=("$a"); fi
        case $a in --only | --plan | --verify-killer | fuzz) explicit=1 ;; esac
    done
    "$XTASK" contained "${opts[@]+"${opts[@]}"}" --print-argv "${xargs[@]}" > "$work/x.out" 2> "$work/x.err"
    local xs=$?
    "$NZX" contained "${opts[@]+"${opts[@]}"}" --print-argv "${nargs[@]}" 2> "$work/n.err0" | sed 's/mutate-seed/mutate/g; s/fuzz-seed/fuzz/g' > "$work/n.out"
    local ns=${PIPESTATUS[0]}
    sed 's/mutate-seed/mutate/g; s/fuzz-seed/fuzz/g' "$work/n.err0" > "$work/n.err"
    local why=""
    [ "$xs" = 0 ] && [ "$ns" = 0 ] || why="status xtask $xs, nzx $ns"
    cmp -s "$work/x.err" "$work/n.err" || why="$why; stderr differs"
    if [ "$explicit" = 1 ]; then
        cmp -s "$work/x.out" "$work/n.out" || why="$why; stdout differs"
    else
        # Everything but the script (the last element) is the reference's; the script is the reference's with the
        # seed's names chosen first and passed last.
        local xl nl xr nr
        xl=$(wc -l < "$work/x.out"); nl=$(wc -l < "$work/n.out")
        [ "$xl" = "$nl" ] || why="$why; $xl argv lines against $nl"
        head -n "$((xl - 1))" "$work/x.out" | cmp -s - <(head -n "$((nl - 1))" "$work/n.out") || why="$why; the arguments before the script differ"
        xr=$(tail -n 1 "$work/x.out"); nr=$(tail -n 1 "$work/n.out")
        # The script follows the cache guard (`s=$(du ...); ...; cd /src && SCRIPT`): remove the selection's
        # `only=$(awk ...) && ` after `cd /src && ` and the ` $only` at the end, and the reference's line remains.
        local before=${nr%%cd /src && *} after=${nr#*cd /src && }
        after=${after#only=\$(awk *) && }
        nr="${before}cd /src && ${after}"
        [ "$nr" = "$xr \$only" ] || why="$why; the script is not the reference's plus the seed's selection"
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
        diff "$work/x.out" "$work/n.out" | head -4 | cut -c1-200
    fi
}

# `nzx_case NAME PATTERNS ARGS...`: a plan with no reference (the Nazm campaign): it must succeed, and its
# last argv element (the script) must hold every pattern (separated by `~`).
nzx_case() {
    local name=$1 pats=$2
    shift 2
    local opts=()
    while [ $# -gt 0 ]; do
        case $1 in
            --platform | --profile | --cache) opts+=("$1"); [ $# -gt 1 ] && opts+=("$2"); shift; [ $# -gt 0 ] && shift ;;
            *) break ;;
        esac
    done
    "$NZX" contained "${opts[@]+"${opts[@]}"}" --print-argv "$@" > "$work/n.out" 2> "$work/n.err"
    local ns=$? why=""
    [ "$ns" = 0 ] || why="a plan was expected, status $ns"
    local last
    last=$(tail -n 1 "$work/n.out")
    local IFS='~' p
    for p in $pats; do
        case $last in *"$p"*) ;; *) why="$why; the script lacks $p" ;; esac
    done
    if [ "$ns" = 0 ]; then
        local m
        m=$(check_flags "$work/n.out") || why="$why; $m"
    fi
    if [ -z "$why" ]; then
        agree=$((agree + 1))
        elements=$((elements + $(wc -l < "$work/n.out" | tr -d ' ')))
        printf 'agree   %-34s plan facts hold\n' "$name"
    else
        differ=$((differ + 1))
        printf 'DIFFER  %-34s %s\n' "$name" "$why"
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
bench|bench
bench save|bench --save
bench cache|--cache bench run true
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
while IFS='|' read -r name args; do
    [ -n "$name" ] || continue
    # shellcheck disable=SC2086
    case_seed "$name" $args
done << 'SEED'
fuzz-seed|fuzz
fuzz-seed 60|fuzz 60
fuzz-seed targets|fuzz 60 front lower bogus
fuzz-seed p1|--profile p1 fuzz 30 run
mutate-seed|mutate
mutate-seed p4t4|--profile p4t4 mutate
mutate-seed plan|mutate --plan
mutate-seed resume|mutate --resume
mutate-seed only|mutate --only m1:x --strategy fast
mutate-seed only exact|mutate --only =m1 --resume
mutate-seed only p4t4|--profile p4t4 mutate --only m1
mutate-seed budget|mutate --budget-secs 100
mutate-seed killer|mutate --verify-killer abc.d
SEED
nzx_case "mutate (nazm)" "nazm-mutate session /work/mut~--seed /work/target/debug/nazm~--rebuild-cmd 'cargo build -q --locked -p nazm-cli --bin nazm --jobs 1'~--reset-derived-caches~--export-journal~--resume-from /src/target/mutation-campaigns-nz~--cargo-jobs 1 --test-threads 1~--budget-secs 2400" mutate
nzx_case "fuzz (nazm)" "nazm-fuzz run 300 ~--src /src --nazm \$NAZM~nazm-fuzz diff 600 121364944846963~cargo build -q --locked -p nazm-cli --bin nazm --jobs 4~build.sh nazm-fuzz" fuzz
nzx_case "fuzz (nazm) targets" "nazm-fuzz run 60 front lower --src /src~nazm-fuzz diff 120 121364944846963" fuzz 60 front lower bogus
nzx_case "fuzz (nazm) p1" "nazm-fuzz run 30 run --src /src~--jobs 1" --profile p1 fuzz 30 run
nzx_case "mutate (nazm) only resume" "--only 'a-b_c'~--resume~--strategy 'full'" mutate --only a-b_c --strategy full --resume
nzx_case "mutate (nazm) verify" "--verify-killer 'abc.d'~--killer 'foo::bar'" mutate --verify-killer abc.d --killer foo::bar
nzx_case "mutate (nazm) plan" "nazm-mutate plan /work/mut --only 'x1'" mutate --plan --only x1
nzx_case "mutate (nazm) p4t4" "--cargo-jobs 4 --test-threads 4" --profile p4t4 mutate
echo "$agree cases agree ($elements argv elements compared), $differ differ"
[ "$differ" -eq 0 ]
