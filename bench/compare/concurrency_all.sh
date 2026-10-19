#!/bin/bash
# Nazm's tasks against every installed language's concurrency model (docs/internal/performance-log.md,
# *Concurrency against every installed language*). The programs are bench/compare/<lang>/{spawn,
# pingpong,fanin,idle,httpd}.<ext> (Go and Nazm's are the 6.1 pair; Java's are Spawn.java ...) and each
# language's existing primes (the parallel job):
#
#   spawn 1000000     1,000,000 tasks, each adding 1 to a shared atomic (OS-thread languages spawn and
#                     join them 1,000 at a time: a process cannot hold a million threads)
#   pingpong 100000   round trips between two tasks over two channels of capacity 1
#   fanin 8           8 producers, 1,000,000 values, one channel of capacity 1,024
#   idle N            peak RSS with N tasks blocked at once (OS-thread languages: 4,000 against 1,000;
#                     the others 100,000 against 1,000)
#   httpd ADDR        HTTP/1.1 requests/s under go/httpload (64 keep-alive connections, 10 s)
#   primes W          the parallel CPU job, W = 1 and the processors
#
#     bash bench/compare/concurrency_all.sh NAZM OUT [--wait | --force]
#
# Same refusal rule as concurrency.sh: nothing is timed while the 1-minute load is 2 or more or a
# cargo, nextest or rustc process runs (--wait waits up to an hour; --force marks the numbers as taken
# under load). A language whose tool is missing is skipped and listed. Results: OUT/concurrency_all.txt
# (the load before each measurement) and OUT/*.json (hyperfine's).
set -uo pipefail
NAZM=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
OUT=$2
MODE=${3:-}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT/bin"
OUT=$(cd "$OUT" && pwd)
LOG=$OUT/concurrency_all.txt
load() { uptime | sed 's/.*averages*: //' | tr -d ',' | awk '{print $1}'; }
note() { echo "$*" | tee -a "$LOG"; }
: >"$LOG"
CPUS=$(getconf _NPROCESSORS_ONLN)
JAVA_HOME_BIN=/opt/homebrew/opt/openjdk/bin
DOTNET=${DOTNET:-$HOME/.dotnet/dotnet}
export DOTNET_ROOT=${DOTNET_ROOT:-$(dirname "$DOTNET")}
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
B=$OUT/bin

note "host: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $CPUS processors"
note "nazm: $("$NAZM" --version)"

LANGS=()
SKIPPED=()
have() { command -v "$1" >/dev/null 2>&1; }
# ---- builds: each language gets the programs it has ----
if have go; then
    note "go: $(go version)"
    GO111MODULE=off go build -o "$B/go-httpload" "$HERE/go/httpload.go"
    for p in spawn pingpong fanin idle httpd primes; do GO111MODULE=off go build -o "$B/go-$p" "$HERE/go/$p.go"; done
    LANGS+=(go)
else SKIPPED+=("go: not installed"); fi
for p in spawn pingpong fanin idle httpd primes; do
    "$NAZM" build --opt-level 2 "$HERE/nazm/$p.nz" -o "$B/nazm-$p" >/dev/null 2>&1 || note "nazm $p failed to build"
done
LANGS+=(nazm)
if have rustc; then
    note "rust: $(rustc --version)"
    for p in spawn pingpong fanin idle primes; do rustc -O -o "$B/rust-$p" "$HERE/rust/$p.rs" 2>/dev/null || note "rust $p failed to build"; done
    LANGS+=(rust)
else SKIPPED+=("rust: not installed"); fi
if have clang; then
    note "c: $(clang --version | head -1)"
    for p in spawn pingpong fanin idle primes; do clang -O2 -pthread -o "$B/c-$p" "$HERE/c/$p.c" 2>/dev/null || note "c $p failed to build"; done
    for p in spawn pingpong fanin idle primes; do clang++ -O2 -std=c++20 -fexperimental-library -pthread -o "$B/cpp-$p" "$HERE/cpp/$p.cpp" 2>/dev/null || note "cpp $p failed to build"; done
    LANGS+=(c cpp)
else SKIPPED+=("c, c++: clang not installed"); fi
if [ -x "$JAVA_HOME_BIN/javac" ]; then
    note "java: $("$JAVA_HOME_BIN/java" -version 2>&1 | head -1)"
    mkdir -p "$B/java"
    "$JAVA_HOME_BIN/javac" -d "$B/java" "$HERE"/java/{Spawn,Pingpong,Fanin,Idle,Httpd,Primes}.java || note "java build failed"
    LANGS+=(java)
else SKIPPED+=("java: no javac"); fi
if [ -x "$DOTNET" ]; then
    note "dotnet: $("$DOTNET" --version)"
    major=$("$DOTNET" --version | cut -d. -f1)
    for p in spawn pingpong fanin idle httpd primes; do
        d=$B/cs-$p.proj; mkdir -p "$d"; cp "$HERE/csharp/$p.cs" "$d/"
        printf '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net%s.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><AssemblyName>p</AssemblyName><ServerGarbageCollector>false</ServerGarbageCollector></PropertyGroup></Project>\n' "$major" >"$d/p.csproj"
        "$DOTNET" build "$d/p.csproj" -c Release -o "$B/cs-$p.dir" -nologo -v q >/dev/null 2>&1 || note "csharp $p failed to build"
    done
    LANGS+=(csharp)
else SKIPPED+=("csharp: no dotnet"); fi
have node && { note "node: $(node --version)"; LANGS+=(node); } || SKIPPED+=("node: not installed")
have python3 && { note "python: $(python3 --version)"; LANGS+=(python); } || SKIPPED+=("python: not installed")
for l in erl elixir kotlinc ghc; do have $l || SKIPPED+=("$l: not installed"); done
SKIPPED+=("tokio: not available offline (no cargo registry cache)")
note "skipped: ${SKIPPED[*]}"

# cmd LANG PROG ARG -> the command line
cmd() {
    case $1 in
        go|nazm|rust|c|cpp) echo "$B/$1-$2 $3" ;;
        java) c=$(echo "$2" | awk '{print toupper(substr($0,1,1)) substr($0,2)}'); echo "$JAVA_HOME_BIN/java -cp $B/java $c $3" ;;
        csharp) echo "$B/cs-$2.dir/p $3" ;;
        node) echo "node $HERE/javascript/$2.js $3" ;;
        python) echo "python3 $HERE/python/$2.py $3" ;;
    esac
}
has() { # LANG PROG
    case $1 in
        go|nazm|rust|c|cpp) [ -x "$B/$1-$2" ] ;;
        java) c=$(echo "$2" | awk '{print toupper(substr($0,1,1)) substr($0,2)}'); [ -f "$B/java/$c.class" ] ;;
        csharp) [ -x "$B/cs-$2.dir/p" ] ;;
        node) [ -f "$HERE/javascript/$2.js" ] ;;
        python) [ -f "$HERE/python/$2.py" ] ;;
    esac
}
JAVA_PRIMES=Primes
cmd_fix() { echo "$1" | sed "s/ Primes / $JAVA_PRIMES /"; }
# Thread-per-task languages hold OS threads: their idle sizes are smaller.
hi_of() { case $1 in rust|c|cpp) echo 4000 ;; *) echo 100000 ;; esac; }

# ---- same answers first ----
for args in "spawn 1000 1000" "pingpong 1000 500500" "fanin 8 499999500000" "idle 1000 1000" "primes 4 216816"; do
    set -- $args
    for l in "${LANGS[@]}"; do
        has $l $1 || continue
        got=$($(cmd $l $1 $2) 2>/dev/null)
        # pingpong's answer depends on N: N(N+1)/2 for the +1 echo
        if [ "$1" = pingpong ]; then want=$(( 1000 * 999 / 2 + 1000 )); else want=$3; fi
        [ "$got" = "$want" ] && note "agree $1 $l" || note "DIFFER $1 $l: got '$got', want '$want'"
    done
done

[ "$MODE" = "--build-only" ] && exit 0
busy() { echo "$(pgrep -x cargo; pgrep -x cargo-nextest; pgrep -x rustc)"; }
quiet() { [ -z "$(busy | tr -d ' \n')" ] && awk -v l="$(load)" 'BEGIN { exit !(l < 2) }'; }
if [ "$MODE" = "--wait" ]; then
    tries=0
    while ! quiet && [ $tries -lt 240 ]; do sleep 15; tries=$((tries + 1)); done
fi
if [ "$MODE" != "--force" ] && ! quiet; then
    note "REFUSED: the host is not quiet (load $(load)); nothing timed"
    exit 2
fi
[ "$MODE" = "--force" ] && note "FORCED: numbers below may have been taken under load; do not record them"

# settle: wait (up to ten minutes) for the 1-minute load to fall below 2 again after our own threads;
# the load a measurement began at is recorded beside it.
settle() {
    local t=0
    while ! quiet && [ $t -lt 120 ]; do sleep 5; t=$((t + 1)); done
}
time_all() { # PROG ARG RUNS
    local prog=$1 arg=$2 runs=$3 l
    note "== $prog $arg"
    for l in "${LANGS[@]}"; do
        has $l $prog || continue
        settle
        local ld=$(load)
        hyperfine -N --warmup 1 --runs "$runs" --export-json "$OUT/$prog-$arg-$l.json" "$(cmd $l $prog $arg)" >/dev/null 2>&1
        python3 -I - "$OUT/$prog-$arg-$l.json" "$l" "$ld" <<'PYEOF' | tee -a "$LOG"
import json, statistics, sys
r = json.load(open(sys.argv[1]))["results"][0]
t = [m["time_wall_clock"]["value"] for m in r["measurements"]]
mem = [m["peak_memory"]["value"] for m in r["measurements"] if "peak_memory" in m]
print(f"  {sys.argv[2]:7s} {statistics.mean(t)*1000:10.1f} ± {statistics.pstdev(t)*1000:8.1f} ms  min {min(t)*1000:.1f}  mem {max(mem)/1048576 if mem else 0:.1f} MiB  load {sys.argv[3]}")
PYEOF
    done
}
rss() { /usr/bin/time -l $1 2>&1 >/dev/null | awk '/maximum resident/ {print $1}'; }

time_all spawn 1000000 5
time_all pingpong 100000 10
time_all fanin 8 10
for l in "${LANGS[@]}"; do
    has $l idle || continue
    hi=$(hi_of $l)
    settle
    note "== idle $l, load $(load)"
    a=$(rss "$(cmd $l idle $hi)"); b=$(rss "$(cmd $l idle 1000)")
    note "$l idle: peak RSS $a bytes at $hi, $b at 1000: $(( (a - b) / (hi - 1000) )) bytes per task"
done
port=8100
for round in 1 2 3; do
    for l in nazm go java csharp node python; do
        has $l httpd || continue
        port=$((port + 1))
        $(cmd $l httpd 127.0.0.1:$port) >/dev/null 2>&1 & pid=$!
        tries=0
        until curl -s -o /dev/null "http://127.0.0.1:$port/" || [ $tries -ge 100 ]; do sleep 0.2; tries=$((tries + 1)); done
        sleep 1
        settle
        note "== http round $round $l, load $(load)"
        "$B/go-httpload" "http://127.0.0.1:$port/" 64 10 | tee -a "$LOG"
        note "server RSS $(ps -o rss= -p $pid) KiB"
        kill $pid 2>/dev/null; wait $pid 2>/dev/null
    done
done
JAVA_PRIMES=Primes
time_all primes 1 10
time_all primes "$CPUS" 10
note "== end, load $(load)"
