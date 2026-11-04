#!/bin/bash
# R8 slice 13 (S13.2): `xtask/tests/host_run.rs`, ported, run against both host runners.
#
#   toolchain/harness/nzx/tests/host_run.sh [nzx|xtask|both]      (default: both)
#
# Environment: NZX (default `target/nzx/nzx`, built by `toolchain/harness/build.sh`), XTASK
# (default `$CARGO_TARGET_DIR/debug/xtask`), NAZM (the seed's `nazm`, for the one case that builds
# a fixture), NZX_OUT (where `build.sh` puts what it builds). Prints one `agree`/`DIFFER` line a
# case and runner, and ends `N agree, M differ, K skipped`; exits 1 when anything differs.
#
# Host-safe like the Rust file: the stages are small shell scripts carrying the two things a
# runner reads (the symbol `nz.heap_malloc` and an `N0406` message with a limit), and one fixture
# built with `nazm build --heap-limit 4M`. The driver is this script, not a Nazm program: a Nazm
# test program started on the host would be a third Nazm-built process beside the runner and its
# stage, which question 5's acceptance does not cover (`design.md`, S13.2: logged difference).
# The case that needs `nazm build` runs only when NAZM is set. Cases that need a Linux host
# (`not-macos`) are skipped on macOS, as the Rust file ignores them there.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
which=${1:-both}
NZX=${NZX:-$root/target/nzx/nzx}
XTASK=${XTASK:-${CARGO_TARGET_DIR:-$root/target}/debug/xtask}
agree=0
differ=0
skipped=0
work=$(mktemp -d "${TMPDIR:-/tmp}/nzx-host-run.XXXXXX")
holders=()
hold_files=()
cleanup() {
    for p in "${holders[@]:-}"; do [ -n "$p" ] && kill "$p" 2> /dev/null; done
    rm -rf "$work"
}
trap cleanup EXIT

# `hr STATE ARGS...`: the runner under test, its state in STATE and neither containment variable
# set; its standard output in $work/out, standard error in $work/err, status in $status.
hr() {
    local state=$1
    shift
    env -u NAZM_CONTAINED -u NAZM_ALLOW_UNCONTAINED NAZM_HOST_RUN_DIR="$state" "${runner[@]}" host-run "$@" > "$work/out" 2> "$work/err"
    status=$?
}

# A script that carries what the runner reads, then runs BODY: `script DIR NAME SYMBOL LIMIT BODY`
# (SYMBOL `1` or `0`; LIMIT empty for none).
script() {
    local dir=$1 name=$2 symbol=$3 limit=$4 body=$5
    {
        echo '#!/bin/sh'
        [ "$symbol" = 1 ] && echo '# _nz.heap_malloc'
        [ -n "$limit" ] && echo "# nazm: error[N0406]: out of memory: the heap limit of $limit bytes is reached"
        printf '%s\n' "$body"
    } > "$dir/$name"
    chmod 755 "$dir/$name"
}

# The failures of one case are collected, then reported as one line.
fails=()
need() { [ "$1" = ok ] || fails+=("$2"); }
expect_eq() { [ "$1" = "$2" ] || fails+=("$3: wanted [$2], got [$1]"); }
expect_status() { [ "$status" -eq "$1" ] || fails+=("$2: status $status, wanted $1: $(head -c 300 "$work/err")"); }
expect_in_err() { grep -qF -- "$1" "$work/err" || fails+=("$2: stderr lacks [$1]: $(head -c 300 "$work/err")"); }
refused_as() {
    [ "$status" -ne 0 ] || fails+=("$2: succeeded, wanted refusal $1")
    grep -qF "refused [$1]" "$work/err" || fails+=("$2: wanted refusal $1: $(head -c 300 "$work/err")")
}
no_withdrawn() { [ ! -e "$1/withdrawn" ] || fails+=("a withdrawn marker was left"); }

case_start() { fails=(); d=$(mktemp -d "$work/case.XXXXXX"); }
case_end() {
    if [ ${#fails[@]} -eq 0 ]; then
        agree=$((agree + 1))
        printf 'agree   %-5s %s\n' "$kind" "$1"
    else
        differ=$((differ + 1))
        printf 'DIFFER  %-5s %s: %s\n' "$kind" "$1" "${fails[0]}"
    fi
}

cases() {
    case_start
    script "$d" ok 1 4194304 'echo the stage ran'
    hr "$d" -- "$d/ok"
    expect_status 0 "run"
    expect_eq "$(cat "$work/out")" "the stage ran" "stdout"
    expect_in_err "host-run: heap limit 4194304 bytes; peak resident " "report"
    grep -q 'peak resident unknown' "$work/err" && fails+=("peak unknown")
    no_withdrawn "$d"
    case_end a_stage_within_its_limit_runs_and_its_peak_is_recorded

    case_start
    for code in 0 1 2 7 255; do
        script "$d" status 1 4194304 "echo out-line; echo err-line >&2; exit $code"
        hr "$d" -- "$d/status"
        expect_status "$code" "code $code"
        expect_eq "$(cat "$work/out")" "out-line" "stdout"
        [ "$(head -1 "$work/err")" = err-line ] || fails+=("stderr starts with the program's")
        expect_in_err "host-run: heap limit 4194304 bytes; peak resident " "report"
        expect_in_err "exit Some($code)" "exit"
        no_withdrawn "$d"
    done
    case_end the_programs_exit_status_is_the_runners_and_its_report_is_not_on_stdout

    case_start
    script "$d" status 1 4194304 'echo out-line; echo err-line >&2; exit 3'
    for _ in 1 2; do
        hr "$d" --report "$d/peaks.log" -- "$d/status"
        expect_status 3 "report file"
        expect_eq "$(cat "$work/out")" "out-line" "stdout"
        expect_eq "$(cat "$work/err")" "err-line" "stderr"
    done
    expect_eq "$(wc -l < "$d/peaks.log" | tr -d ' ')" 2 "one line per run"
    [ "$(grep -c 'heap limit 4194304 bytes; peak resident .*exit Some(3)' "$d/peaks.log")" = 2 ] || fails+=("report lines")
    case_end a_report_file_leaves_standard_error_the_programs_alone

    case_start
    script "$d" raw 1 4194304 "printf '\\377\\376ok\\n'"
    hr "$d" -- "$d/raw"
    expect_status 0 "raw"
    printf '\377\376ok\n' > "$d/want"
    cmp -s "$work/out" "$d/want" || fails+=("bytes that are not UTF-8 arrive as written")
    script "$d" big 1 4194304 'dd if=/dev/zero bs=1048576 count=2 2> /dev/null | tr "\\0" a'
    hr "$d" --max-output-mib 1 -- "$d/big"
    expect_status 126 "truncated"
    expect_eq "$(wc -c < "$work/out" | tr -d ' ')" 1048576 "kept output"
    expect_in_err "OUTPUT TRUNCATED at 1048576 bytes" "truncation report"
    hr "$d" --max-output-mib 3 -- "$d/big"
    expect_status 0 "within the cap"
    expect_eq "$(wc -c < "$work/out" | tr -d ' ')" 2097152 "whole output"
    hr "$d" --max-output-mib 257 -- "$d/big"
    refused_as usage "257 MiB"
    case_end output_is_passed_as_bytes_and_a_truncation_is_a_failure_of_its_own

    case_start
    script "$d" plain 0 4194304 'exit 0'
    hr "$d" -- "$d/plain"
    expect_status 1 "refusal"
    script "$d" hangs 1 4194304 'sleep 30'
    hr "$d" --deadline-secs 1 -- "$d/hangs"
    expect_status 124 "deadline"
    case_end a_refusal_and_a_deadline_have_statuses_of_their_own

    case_start
    script "$d" plain 0 4194304 'echo ran'
    hr "$d" -- "$d/plain"
    refused_as no-counted-heap "plain"
    grep -q '^ran' "$work/out" && fails+=("the program ran")
    case_end a_program_without_the_counted_heap_is_refused_by_name

    case_start
    script "$d" nolimit 1 "" 'echo ran'
    hr "$d" -- "$d/nolimit"
    refused_as no-heap-limit "nolimit"
    case_end a_program_with_no_limit_is_refused_by_name

    case_start
    script "$d" big 1 2147483649 'echo ran'
    hr "$d" -- "$d/big"
    refused_as heap-limit-too-large "over"
    script "$d" edge 1 2147483648 'echo ran'
    hr "$d" -- "$d/edge"
    expect_status 0 "exactly 2 GiB is allowed"
    case_end a_limit_over_two_gib_is_refused_by_name

    case_start
    hr "$d" -- "$d/absent"
    refused_as no-executable "absent"
    echo '# nz.heap_malloc the heap limit of 1 bytes is reached' > "$d/data"
    hr "$d" -- "$d/data"
    refused_as no-executable "not executable"
    case_end a_missing_or_non_executable_file_is_refused_by_name

    case_start
    script "$d" ok 1 4194304 'echo ran'
    hold "$d/lock"
    hr "$d" -- "$d/ok"
    expect_status 0 "a second stage runs beside the first"
    hold "$d/lock-2"
    hr "$d" -- "$d/ok"
    refused_as another-stage-running "third"
    release "$d/lock"
    hr "$d" -- "$d/ok"
    expect_status 0 "a slot is free once a stage ends"
    release "$d/lock-2"
    case_end a_third_stage_is_refused_while_two_run

    case_start
    script "$d" ok 1 4194304 'echo ran'
    for var in NAZM_CONTAINED NAZM_ALLOW_UNCONTAINED; do
        env "$var=1" NAZM_HOST_RUN_DIR="$d" "${runner[@]}" host-run -- "$d/ok" > "$work/out" 2> "$work/err"
        status=$?
        refused_as host-environment "$var"
    done
    case_end a_host_that_declares_containment_is_refused_by_name

    case_start
    script "$d" over 1 1 'echo ran'
    hr "$d" --margin-bytes 0 -- "$d/over"
    [ "$status" -ne 0 ] || fails+=("a breach succeeded")
    expect_in_err "WITHDRAWAL CONDITION" "breach"
    [ -e "$d/withdrawn" ] || fails+=("no marker")
    script "$d" fine 1 4194304 'echo ran'
    hr "$d" -- "$d/fine"
    refused_as withdrawn "after the marker"
    rm -f "$d/withdrawn"
    hr "$d" -- "$d/fine"
    expect_status 0 "after the marker is removed"
    case_end a_peak_past_the_limit_is_the_withdrawal_condition_and_later_runs_are_refused

    case_start
    script "$d" ignores 1 4194304 "echo 'nazm: error[N0406]: out of memory' >&2; exit 0"
    hr "$d" -- "$d/ignores"
    [ "$status" -ne 0 ] || fails+=("succeeded")
    expect_in_err "WITHDRAWAL CONDITION" "N0406"
    [ -e "$d/withdrawn" ] || fails+=("no marker")
    case_end an_n0406_that_does_not_stop_the_program_is_the_withdrawal_condition

    case_start
    script "$d" stops 1 4194304 "echo 'nazm: error[N0406]: out of memory' >&2; exit 2"
    hr "$d" -- "$d/stops"
    [ "$status" -ne 0 ] || fails+=("the stage failed")
    grep -q WITHDRAWAL "$work/err" && fails+=("withdrawn")
    no_withdrawn "$d"
    case_end a_stage_that_stops_with_n0406_and_status_2_is_not_a_withdrawal

    case_start
    script "$d" hangs 1 4194304 "sleep 30 &
echo \$! > $d/child.pid
wait"
    hr "$d" --deadline-secs 2 -- "$d/hangs"
    [ "$status" -ne 0 ] || fails+=("succeeded")
    expect_in_err "DEADLINE reached" "deadline"
    pid=$(tr -d ' \n' < "$d/child.pid" 2> /dev/null)
    if [ -z "$pid" ] || kill -0 "$pid" 2> /dev/null; then fails+=("the stage's child $pid outlived its deadline"); fi
    case_end a_stage_past_its_deadline_is_killed_with_its_children

    # New with the port (S13.2): a running stage holds a slot, and a stage that leaves a process
    # holding its pipes does not hang the runner.
    case_start
    script "$d" slow 1 4194304 'sleep 4'
    ( env -u NAZM_CONTAINED -u NAZM_ALLOW_UNCONTAINED NAZM_HOST_RUN_DIR="$d" "${runner[@]}" host-run -- "$d/slow" > /dev/null 2>&1 ) &
    bg=$!
    sleep 2
    if held "$d/lock"; then :; else fails+=("a running stage does not hold its slot"); fi
    wait "$bg"
    if held "$d/lock"; then fails+=("the slot is held after the stage ended"); fi
    case_end a_running_stage_holds_a_slot_until_it_ends

    case_start
    script "$d" usage 1 4194304 'echo ran'
    for bad in "--deadline-secs 0" "--deadline-secs 3601" "--margin-bytes 268435457" "--max-output-mib 0" "--bogus 1"; do
        # shellcheck disable=SC2086
        hr "$d" $bad -- "$d/usage"
        refused_as usage "$bad"
    done
    hr "$d" --
    refused_as usage "no executable"
    hr "$d" --margin-bytes 0 -- "$d/usage"
    expect_status 0 "a margin of 0 is allowed"
    case_end the_arguments_are_checked_and_a_refusal_is_named_usage

    if [ -n "${NAZM:-}" ] && [ -x "${NAZM:-}" ]; then
        case_start
        cat > "$d/prog.nz" <<'NZ'
fn main(out: OutCap) -> Int ! {io} {
    let xs = strs_new();
    let mut i = 0;
    while i < 100 {
        strs_push(xs, str_concat("item ", int_to_str(i)));
        i = i + 1;
    }
    print(f"hello from a bounded heap {strs_len(xs)}\n");
    0
}
NZ
        for limit in 4M 8M; do
            (cd "$d" && "$NAZM" build --no-cache prog.nz -o "$d/prog" --heap-limit "$limit" > "$work/build" 2>&1) || fails+=("build $limit: $(head -c 200 "$work/build")")
            hr "$d" -- "$d/prog"
            expect_status 0 "limit $limit"
            want=4194304
            [ "$limit" = 8M ] && want=8388608
            expect_in_err "heap limit $want bytes; peak resident " "report $limit"
            grep -q 'hello from a bounded heap' "$work/out" || fails+=("the program's output")
        done
        (cd "$d" && "$NAZM" build --no-cache prog.nz -o "$d/prog" > "$work/build" 2>&1) || fails+=("build without a limit")
        hr "$d" -- "$d/prog"
        refused_as no-counted-heap "built without a limit"
        case_end a_program_built_with_a_small_heap_limit_runs
    else
        skipped=$((skipped + 1))
        printf 'skipped %-5s %s (NAZM is not set)\n' "$kind" a_program_built_with_a_small_heap_limit_runs
    fi

    if [ "$kind" = nzx ]; then
        # The runner checks its own image first (question 5): one built with a 2 GiB heap limit (the
        # most R6 lets any Nazm program have; a build with none is covered by host_run_self.nz) is
        # refused before it reads a word of its command line.
        if [ -n "${NAZM:-}" ] && [ -x "${NAZM:-}" ]; then
            case_start
            script "$d" ok 1 4194304 'echo ran'
            "$NAZM" build --no-cache --heap-limit 2G "$here/../main.nz" -o "$d/nzx-big" > "$work/build" 2>&1 || fails+=("build: $(head -c 200 "$work/build")")
            env -u NAZM_CONTAINED NAZM_HOST_RUN_DIR="$d" "$d/nzx-big" host-run -- "$d/ok" > "$work/out" 2> "$work/err"
            status=$?
            expect_status 1 "runner-unbounded"
            expect_in_err "refused [runner-unbounded]" "runner-unbounded"
            grep -q '^ran' "$work/out" && fails+=("the stage ran")
            case_end a_runner_built_with_a_heap_over_256_mib_refuses_itself
        else
            skipped=$((skipped + 1))
        fi
    fi

    if [ "$(uname -s)" = Darwin ]; then
        skipped=$((skipped + 1))
        printf 'skipped %-5s %s (macOS)\n' "$kind" not_macos_is_refused_by_name
    else
        case_start
        script "$d" ok 1 4194304 'echo ran'
        hr "$d" -- "$d/ok"
        refused_as not-macos "linux"
        case_end not_macos_is_refused_by_name
    fi
}

# `hold FILE`: take FILE's exclusive `flock`, in a background process, until `release FILE`;
# `held FILE`: whether another process holds it.
hold() {
    perl -e 'use Fcntl qw(:flock); $| = 1; open(F, ">>", $ARGV[0]) or die; flock(F, LOCK_EX | LOCK_NB) or die "busy"; print "held\n"; sleep 120;' "$1" > "$work/held" &
    holders+=("$!")
    hold_files+=("$1")
    while [ ! -s "$work/held" ]; do sleep 0.05; done
    : > "$work/held"
}
release() {
    local i
    for i in "${!holders[@]}"; do
        if [ "${hold_files[$i]}" = "$1" ] && [ -n "${holders[$i]}" ]; then kill "${holders[$i]}" 2> /dev/null; wait "${holders[$i]}" 2> /dev/null; holders[$i]=""; fi
    done
}
held() { perl -e 'use Fcntl qw(:flock); open(F, ">>", $ARGV[0]) or exit 2; exit(flock(F, LOCK_EX | LOCK_NB) ? 1 : 0);' "$1"; }

# `compare`: the same runs through both runners; standard output, standard error and the report
# file (the peak and the instruction count masked: they are measurements; the runner's name and the
# case directory masked: they are the only words that must differ) and status compared byte for byte.
cmp_n=0
cmp_same=0
cmp_diff=0
norm() {
    sed -E 's/cargo xtask/RUNNER/g; s/peak resident [0-9]+ bytes/peak resident N bytes/; s/instructions retired [0-9]+/instructions retired N/; s#[^ ]*/cmp[0-9]+/(nzx|xtask)/#DIR/#g; s/`(nzx|xtask) contained`/`RUNNER contained`/g; s/(xtask|nzx) host-run/RUNNER host-run/g' "$1"
}
# `cmp_case SYMBOL LIMIT BODY [OPTIONS...]`: one stage through both.
cmp_case() {
    local symbol=$1 limit=$2 body=$3 sd f
    shift 3
    cmp_n=$((cmp_n + 1))
    sd="$work/cmp$cmp_n"
    mkdir -p "$sd/nzx" "$sd/xtask"
    script "$sd" stage "$symbol" "$limit" "$body"
    for f in nzx xtask; do
        local bin=$NZX
        [ "$f" = xtask ] && bin=$XTASK
        local extra=("$@")
        local k
        for k in "${!extra[@]}"; do extra[k]=${extra[k]//@REPORT@/$sd/$f.log}; done
        env -u NAZM_CONTAINED NAZM_HOST_RUN_DIR="$sd/$f" "$bin" host-run "${extra[@]+"${extra[@]}"}" -- "$sd/stage" > "$sd/$f.out" 2> "$sd/$f.err"
        echo $? > "$sd/$f.st"
        norm "$sd/$f.err" > "$sd/$f.err.n"
        [ -f "$sd/$f.log" ] && norm "$sd/$f.log" > "$sd/$f.log.n"
    done
    if cmp -s "$sd/nzx.out" "$sd/xtask.out" && cmp -s "$sd/nzx.st" "$sd/xtask.st" && cmp -s "$sd/nzx.err.n" "$sd/xtask.err.n" \
        && { [ ! -f "$sd/nzx.log.n" ] || cmp -s "$sd/nzx.log.n" "$sd/xtask.log.n"; }; then
        cmp_same=$((cmp_same + 1))
        printf 'agree   case %-2s status %-3s %s\n' "$cmp_n" "$(cat "$sd/nzx.st")" "$body"
    else
        cmp_diff=$((cmp_diff + 1))
        printf 'DIFFER  case %-2s %s\n' "$cmp_n" "$body"
        diff "$sd/nzx.err.n" "$sd/xtask.err.n" | head -6
        echo "status $(cat "$sd/nzx.st") vs $(cat "$sd/xtask.st")"
    fi
}
compare() {
    cmp_case 1 4194304 'echo out; echo err >&2; exit 3'
    cmp_case 1 4194304 "printf '\\377\\376ok\\n'"
    cmp_case 1 4194304 'exit 0' --report @REPORT@
    cmp_case 1 4194304 'echo x' --deadline-secs 0
    cmp_case 0 4194304 'echo x'
    cmp_case 1 "" 'echo x'
    cmp_case 1 2147483649 'echo x'
    cmp_case 1 2147483648 'echo x'
    cmp_case 1 1 'echo x' --margin-bytes 0
    cmp_case 1 4194304 "echo 'nazm: error[N0406]' >&2; exit 0"
    cmp_case 1 4194304 "echo 'nazm: error[N0406]' >&2; exit 2"
    cmp_case 1 4194304 'dd if=/dev/zero bs=1048576 count=2 2> /dev/null | tr "\\0" a' --max-output-mib 1
    cmp_case 1 4194304 'dd if=/dev/zero bs=1048576 count=2 2> /dev/null | tr "\\0" a' --max-output-mib 3
    cmp_case 1 4194304 'sleep 30' --deadline-secs 1
    cmp_case 1 4194304 'kill -9 $$'
    cmp_case 1 4194304 'echo x' --bogus
    cmp_case 1 4194304 'echo x' --max-output-mib 257
    cmp_case 1 4194304 'echo x' --deadline-secs 3601
    cmp_case 1 4194304 'echo x' --margin-bytes 268435457
    cmp_case 1 4194304 'echo x' --report
    cmp_case 1 4194304 'echo +5' --deadline-secs +5
    echo "compare: $cmp_same agree, $cmp_diff differ"
    [ "$cmp_diff" -eq 0 ]
    exit $?
}

[ "$which" = compare ] && compare

for kind in nzx xtask; do
    case $which in both | "$kind") ;; *) continue ;; esac
    case $kind in
        nzx) runner=("$NZX") ;;
        xtask) runner=("$XTASK") ;;
    esac
    [ -x "${runner[0]}" ] || { echo "no runner at ${runner[0]}" >&2; exit 2; }
    cases
done
echo "$agree agree, $differ differ, $skipped skipped"
[ "$differ" -eq 0 ]
