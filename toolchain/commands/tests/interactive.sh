#!/bin/bash
# R8 slice 7: `run`, `repl`, `comptime` and `test` written in Nazm against the Rust ones
# (`openspec/changes/r8-cut-over/design.md`, S7.8, task 7.10; R5's method, `openspec/changes/r5-commands/`).
#
#     toolchain/commands/tests/interactive.sh [PART...] [-- FILE.nz ...]
#     PART: args interpret comptime repl run test   (INTERACTIVE_PARTS="args comptime" does the same)
#
# The Nazm `nazm` (toolchain/commands/nazm.nz) and the pipeline and code generator it runs as stages (`NAZM_PIPELINE`,
# `NAZM_NZCG`) are built by the checkout's `nazm` ($NAZM) with a heap limit of at most 2 GiB
# (`toolchain/lib/stage.sh`, the R6 rule). Each Nazm run is one `host-run` on a macOS host, one at a time (a refusal
# because another stage is running is waited out, not counted), or runs directly under a deadline contained; the Rust
# side runs directly. A host-run has no standard input, so `repl`, `run` and `test` run contained only (they refuse to
# start on a host): `cargo xtask contained run 'cargo build --locked -p nazm-cli && NAZM=target/debug/nazm sh toolchain/commands/tests/interactive.sh'`.
# Both sides run from the repository's root with the file named by its path under it, so a diagnostic names the same
# file on both sides. A case agrees when standard output, standard error and the exit status are the same bytes.
# A status 6 from the Nazm side (what it does not carry) is counted apart, with its reason, and is never an agreement;
# so is a build the code generator refuses and Cranelift compiled (the design's intended difference 4), listed with its
# construct. Prints a line per case that does not agree, then one line per part: `PART: A agree, D differ, R refused`.
# Exit 0 only when nothing differs.
#
#   args       every command line of tests/args/{run,comptime,repl}.txt
#   interpret  `run --interpret` of every input, plain, `--json` and with arguments after `--`
#   comptime   every function of no parameters of every input, `--max-iterations 1000` on each file's first, and one
#              refusal of each kind (no such function, parameters, an effect) per file
#   repl       every session of tests/repl/*.in on standard input
#   run        `run` of every input; with arguments, 1 MiB of standard input, a program ending 0 to 255 and one killed
#              by a signal
#   test       `test` of every directory of inputs (`--json`, `--summary-json`, `--filter`)
#
# The reference is $NAZM_REF (default $NAZM). The inputs are the tracked programs of the conformance, compatibility,
# example and front-end corpora; BIG=1 adds the toolchain's own sources. KEEP=DIR keeps each difference's records.
# INTERACTIVE_SLICE=K/N takes every Nth input from the Kth, for a contained run's hour.
set -u
stage_name=interactive-compare
. "$(dirname "$0")/../../lib/stage.sh"

parts=()
files=()
while [ $# -gt 0 ]; do
    case $1 in
        --) shift; files=("$@"); break ;;
        *) parts+=("$1"); shift ;;
    esac
done
[ ${#parts[@]} -gt 0 ] || read -r -a parts <<< "${INTERACTIVE_PARTS:-args interpret comptime repl run test}"
need_contained=
for p in "${parts[@]}"; do
    case $p in repl | run | test) need_contained=contained ;; esac
done
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)" $need_contained
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${INTERACTIVE_OUT:-${TMPDIR:-/tmp}/nazm-interactive-compare}
mkdir -p "$OUT/bin" "$OUT/work"

for p in nazm:toolchain/commands/nazm.nz nazm-pipeline:toolchain/pipeline/main.nz nzcg:toolchain/codegen/main.nz; do
    name=${p%%:*}
    if [ -z "${NAZM_PORT:-}" ] || [ "$name" != nazm ]; then
        if ! stage_build "$NAZM" --stack-size 256M "$ROOT/${p#*:}" -o "$OUT/bin/$name" > "$OUT/bin/$name.log" 2>&1; then
            echo "interactive-compare: $name did not build:"
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
                'toolchain/frontend-tests/inputs/check/*.nz' 'toolchain/frontend-tests/inputs/modules/*.nz' 'bench/programs/*.nz'
            if [ "${BIG:-}" = 1 ]; then git ls-files 'compiler/*.nz' 'toolchain/commands/*.nz' 'toolchain/pipeline/*.nz'; fi
        } | LC_ALL=C sort -u
    )
fi

# INTERACTIVE_SLICE=K/N: every Nth input from the Kth (a contained run is held to an hour), so the corpus goes in several.
if [ -n "${INTERACTIVE_SLICE:-}" ]; then
    k=${INTERACTIVE_SLICE%/*}
    n=${INTERACTIVE_SLICE#*/}
    sel=()
    for i in "${!files[@]}"; do
        [ $((i % n)) -eq "$k" ] && sel+=("${files[$i]}")
    done
    files=("${sel[@]}")
fi

export NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline" NAZM_NZCG="$OUT/bin/nzcg"
cd "$ROOT" || exit 2

agree=0
differ=0
refused=0
total_differ=0
DEADLINE=${INTERACTIVE_DEADLINE:-300}

# `run_ref STDIN ARG...`: the reference, under a deadline, with `STDIN` (a file) on its standard input.
run_ref() {
    in=$1
    shift
    perl -e 'alarm shift; exec @ARGV or exit 127' "$DEADLINE" "$REF" "$@" < "$in" > "$W/ref.out" 2> "$W/ref.err"
    echo $? > "$W/ref.status"
}

# `case_in LABEL STDIN ARG...`: both sides run `nazm ARG...` with `STDIN` on standard input; counts and reports.
case_in() {
    label=$1
    in=$2
    shift 2
    W=$OUT/work/case
    rm -rf "$W"
    mkdir -p "$W"
    run_ref "$in" "$@"
    while :; do
        STAGE_STDIN=$in "$STAGE_RUN" "$DEADLINE" "$PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        # Another stage or a contained workload (the other lanes') has the host: wait for it, not counted.
        if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/got.err"; then sleep 10; continue; fi
        break
    done
    echo $rc > "$W/got.status"
    # Status 6 is what the port does not carry (it says so: `nazm: not carried: ...`); a program may end 6 of itself.
    if [ "$rc" = 6 ] && grep -q '^\(nazm\|check\|test\): not carried' "$W/got.err"; then
        refused=$((refused + 1))
        echo "refused  $label: $(head -c 200 "$W/got.err" | tr '\n' ' ')"
        return
    fi
    # The code generator refuses what Cranelift compiled: the design's intended difference 4, counted apart.
    if [ "$rc" != "$(cat "$W/ref.status")" ] && grep -q 'nazm: a program the code generator\|Nazm.s code generator' "$W/got.err"; then
        refused=$((refused + 1))
        echo "refused  $label: code generator: $(grep -m1 'code generator' "$W/got.err" | cut -c1-160)"
        return
    fi
    # The runaway-recursion wording is intended difference 2: compared by code, output and status.
    if grep -q 'N0403' "$W/ref.err" && grep -q 'N0403' "$W/got.err" && cmp -s "$W/ref.out" "$W/got.out" && cmp -s "$W/ref.status" "$W/got.status"; then
        agree=$((agree + 1))
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

case_run() {
    label=$1
    shift
    case_in "$label" /dev/null "$@"
}

report() {
    echo "$1: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
}

reset() {
    agree=0
    differ=0
    refused=0
}

part_args() {
    name=$1
    reset
    list=$ROOT/toolchain/commands/tests/args/$name.txt
    [ -f "$list" ] || { echo "$name args: no $list"; return; }
    while IFS= read -r line; do
        case $line in '' | '#'*) continue ;; esac
        case $line in
            '@contained '*)
                line=${line#@contained }
                # A compiled build runs without a heap limit: contained only.
                [ "$STAGE_MODE" = contained ] || continue ;;
        esac
        [ "$line" = '(none)' ] && line=
        # shellcheck disable=SC2086
        case_run "$name args: ${line:-(none)}" $name $line
    done < "$list"
    report "$name args"
}

part_interpret() {
    reset
    for f in "${files[@]}"; do
        case_run "run --interpret $f" run --interpret "$f"
        case_run "run --interpret --json $f" run --interpret --json "$f"
        case_run "run --interpret $f -- a 'b c'" run --interpret "$f" -- a 'b c'
    done
    # The reports the runtime answers for the program, by environment variable: the interpreted program's, never `nazm`'s own.
    for f in examples/max.nz examples/pipeline.nz tests/compat/v1/run/tasks.nz; do
        NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 case_run "run --interpret $f with the memory and scheduler reports" run --interpret "$f"
        NAZM_RESULT_REPORT=1 case_run "run --interpret $f with the result report" run --interpret "$f"
        NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 NAZM_RESULT_REPORT=1 case_run "run --interpret $f with all three reports" run --interpret "$f"
    done
    report interpret
}

# Every function of no parameters of `$1`, by name.
nullary() {
    grep -E '^(pub )?fn [A-Za-z_0-9]+\(\)' "$1" | sed -E 's/^(pub )?fn ([A-Za-z_0-9]+)\(\).*/\2/' | head -${COMPTIME_MAX:-4}
}

part_comptime() {
    reset
    for f in "${files[@]}"; do
        first=
        for fn in $(nullary "$f"); do
            [ -n "$first" ] || first=$fn
            case_run "comptime $f $fn" comptime "$f" "$fn"
        done
        [ -n "$first" ] && case_run "comptime $f $first --max-iterations 1000 --max-depth 4" comptime "$f" "$first" --max-iterations 1000 --max-depth 4
        case_run "comptime $f nz_absent_fn" comptime "$f" nz_absent_fn
        withp=$(grep -E -m1 '^(pub )?fn [A-Za-z_0-9]+\([^)]+\)' "$f" | sed -E 's/^(pub )?fn ([A-Za-z_0-9]+)\(.*/\2/')
        [ -n "$withp" ] && case_run "comptime $f $withp (parameters)" comptime "$f" "$withp"
        effect=$(grep -E -m1 '^(pub )?fn [A-Za-z_0-9]+\(\)[^{]*! *\{ *[a-z]' "$f" | sed -E 's/^(pub )?fn ([A-Za-z_0-9]+)\(.*/\2/')
        [ -n "$effect" ] && case_run "comptime $f $effect (effect)" comptime "$f" "$effect"
    done
    report comptime
}

part_repl() {
    reset
    for s in "$ROOT"/toolchain/commands/tests/repl/*.in; do
        case_in "repl $(basename "$s")" "$s" repl
    done
    report repl
}

part_run() {
    reset
    head -c 1048576 /dev/urandom | base64 | head -c 1048576 > "$OUT/work/big.in"
    for f in "${files[@]}"; do
        case_run "run $f" run "$f"
    done
    # Arguments, standard input, every status and a signal.
    cat > "$OUT/work/args.nz" <<'EOF'
fn main(io: IoCap, out: OutCap) -> Int {
    let mut i = 0
    while i < args_count() {
        print(arg(i))
        print(str_from_byte(10))
        i += 1
    }
    args_count()
}
EOF
    case_run "run args" run "$OUT/work/args.nz" -- one 'two words' ''
    case_run "run args none" run "$OUT/work/args.nz"
    cat > "$OUT/work/echo.nz" <<'EOF'
use "@std/io"

fn main(io: IoCap, out: OutCap) -> Int {
    match io_stdin_read_all(io) {
        Result.Ok(value: b) => {
            print(int_to_str(bytes_len(b)))
            print(str_from_byte(10))
            0
        },
        Result.Err(error: e) => 1,
    }
}
EOF
    case_in "run 1 MiB of standard input" "$OUT/work/big.in" run "$OUT/work/echo.nz"
    for code in 0 1 2 7 42 128 200 255; do
        printf 'fn main() -> Int { %s }\n' "$code" > "$OUT/work/status$code.nz"
        case_run "run status $code" run "$OUT/work/status$code.nz"
    done
    printf 'fn main(io: IoCap) -> Int { exit_with(300) }\n' > "$OUT/work/status300.nz"
    case_run "run exit_with 300" run "$OUT/work/status300.nz"
    printf 'fn main() -> Int { 256 }\n' > "$OUT/work/status256.nz"
    case_run "run status 256" run "$OUT/work/status256.nz"
    printf 'fn main() -> Int { 0 - 1 }\n' > "$OUT/work/statusneg.nz"
    case_run "run status -1" run "$OUT/work/statusneg.nz"
    cat > "$OUT/work/signal.nz" <<'EOF'
use "@std/process"

fn main(io: IoCap, proc: ProcessCap, sp: SpawnCap) -> Int ! { io, spawn } {
    let me = int_to_str(process_id(io))
    let r = process_run(proc, sp, "kill", ["-9", me], "")
    let mut i = 0
    while i >= 0 { i = 0 }
    0
}
EOF
    case_run "run killed by a signal" run "$OUT/work/signal.nz"
    for r in "run --interpret" run; do
        NAZM_RESULT_REPORT=1 case_run "$r with NAZM_RESULT_REPORT" $r examples/max.nz
        NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 case_run "$r with the memory and scheduler reports" $r examples/pipeline.nz
    done
    report run
}

part_test() {
    reset
    export NAZM_TEST_DRIVER=
    unset NAZM_TEST_DRIVER
    for d in $(printf '%s\n' "${files[@]}" | xargs -n1 dirname | LC_ALL=C sort -u); do
        case_run "test $d" test "$d"
        case_run "test --json $d" test --json "$d"
        case_run "test --summary-json $d" test --summary-json "$d"
        case_run "test --interpret-only --filter ::  $d" test --interpret-only --filter :: "$d"
    done
    report test
}

for p in "${parts[@]}"; do
    case $p in
        args) for c in run comptime repl; do part_args "$c"; done ;;
        interpret) part_interpret ;;
        comptime) part_comptime ;;
        repl) part_repl ;;
        run) part_run ;;
        test) part_test ;;
        *) echo "interactive-compare: no part $p"; exit 2 ;;
    esac
done
[ "$total_differ" -eq 0 ]
