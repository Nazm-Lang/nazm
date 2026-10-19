#!/bin/bash
# Performance leadership 6.1: Nazm's tasks against Go's goroutines (docs/internal/performance-log.md,
# *Concurrency*). Builds bench/compare/{nazm,go}/{spawn,pingpong,fanin,idle,httpd,primes} and
# go/httpload.go into OUT, checks that both languages print the same answers, then measures:
#
#   spawn 1000000     1,000,000 tasks in one scope, each adding 1 to an atomic   (hyperfine)
#   pingpong 100000   round trips between two tasks over two channels of capacity 1 (hyperfine)
#   fanin 8           8 producers, 1,000,000 values, one channel of capacity 1,024  (hyperfine)
#   idle N            peak RSS with N tasks blocked at once, N = 100,000 against 1,000
#   httpd ADDR        HTTP/1.1 requests/s under go/httpload (64 keep-alive connections, 10 s)
#   primes W          the multicore section's parallel CPU job, W = 1 and the processors
#
#     bash bench/compare/concurrency.sh NAZM OUT [--wait | --force]
#
# NAZM is the release `nazm` to measure. Timing is refused while the 1-minute load average is 2 or
# more, or a cargo, nextest or rustc process runs (the host must be quiet; numbers taken under load are not
# recorded) unless --force, which marks every number as taken under load; --wait waits up to an
# hour for a quiet host first. Results go to
# OUT/concurrency.txt, each with the load before it.
set -euo pipefail
NAZM=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
OUT=$2
FORCE=${3:-}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT/bin"
OUT=$(cd "$OUT" && pwd)
LOG=$OUT/concurrency.txt
load() { uptime | sed 's/.*averages*: //' | tr -d ',' | awk '{print $1}'; }
note() { echo "$*" | tee -a "$LOG"; }

: >"$LOG"
note "host: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $(getconf _NPROCESSORS_ONLN) processors"
note "go: $(go version)"
note "nazm: $("$NAZM" --version)"
GO111MODULE=off go build -o "$OUT/bin/go-httpload" "$HERE/go/httpload.go"
for p in spawn pingpong fanin idle httpd primes; do
    GO111MODULE=off go build -o "$OUT/bin/go-$p" "$HERE/go/$p.go"
    "$NAZM" build --opt-level 2 "$HERE/nazm/$p.nz" -o "$OUT/bin/nazm-$p" >/dev/null 2>&1
done
cd "$OUT/bin"

# Same answers first: a program that disagrees is not measured.
for args in "spawn 1000" "pingpong 1000" "fanin 8" "idle 1000" "primes 4"; do
    set -- $args
    a=$("./go-$1" "$2"); b=$("./nazm-$1" "$2")
    [ "$a" = "$b" ] || { note "DIFFER $args: go $a, nazm $b"; exit 1; }
    note "agree $args: $a"
done

busy=$(pgrep -l 'cargo|nextest|rustc' || true)
quiet() { [ -z "$(pgrep -l 'cargo|nextest|rustc' || true)" ] && awk -v l="$(load)" 'BEGIN { exit !(l < 2) }'; }
if [ "$FORCE" = "--wait" ]; then
    # Up to an hour for the host to fall quiet (after this script's own builds).
    tries=0
    while ! quiet && [ $tries -lt 240 ]; do sleep 15; tries=$((tries + 1)); done
    busy=$(pgrep -l 'cargo|nextest|rustc' || true)
fi
if [ "$FORCE" != "--force" ]; then
    if [ -n "$busy" ] || awk -v l="$(load)" 'BEGIN { exit !(l >= 2) }'; then
        note "REFUSED: the host is not quiet (load $(load)); nothing timed"
        [ -n "$busy" ] && note "$busy"
        exit 2
    fi
else
    note "FORCED: numbers below were taken on a host that may be loaded; do not record them"
fi

time_both() { # name arg runs
    note "== $1 $2, load $(load)"
    hyperfine -N --warmup 2 --runs "$3" --export-json "$OUT/$1-$2.json" \
        "./go-$1 $2" "./nazm-$1 $2" 2>&1 | grep -E "Time|Memory|±" | sed 's/^ *//' | tee -a "$LOG"
}
rss() { /usr/bin/time -l "./$1" "$2" 2>&1 >/dev/null | awk '/maximum resident/ {print $1}'; }

time_both spawn 1000000 5
time_both pingpong 100000 10
time_both fanin 8 10
for side in go nazm; do
    note "== idle $side, load $(load)"
    hi=$(rss "$side-idle" 100000); lo=$(rss "$side-idle" 1000)
    note "$side idle: peak RSS $hi bytes at 100000, $lo at 1000: $(( (hi - lo) / 99000 )) bytes per task"
done
for round in 1 2 3; do
    for side in nazm go; do
        port=$([ $side = nazm ] && echo 8081 || echo 8082)
        "./$side-httpd" "127.0.0.1:$port" & pid=$!
        sleep 0.5
        note "== http round $round $side, load $(load)"
        ./go-httpload "http://127.0.0.1:$port/" 64 10 | tee -a "$LOG"
        note "server RSS $(ps -o rss= -p $pid) KiB"
        kill $pid; wait $pid 2>/dev/null || true
    done
done
time_both primes 1 10
time_both primes "$(getconf _NPROCESSORS_ONLN)" 10
note "== end, load $(load)"
