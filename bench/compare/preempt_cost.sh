#!/bin/bash
# Performance leadership 6.3: what preemption's poll at every loop back edge costs on hot loops.
# Builds bench/compare/nazm/{fib,words,primes} and a tight arithmetic loop with no call (written
# here) with BASE, a `nazm` from before preemption (runtime ABI 42 or earlier), and NEW, one with it
# (43 or later), then times each pair with hyperfine:
#
#     bash bench/compare/preempt_cost.sh BASE NEW OUT [--wait | --force]
#
# The target is no measurable slowdown; more than 1-2% is reported before anything else builds on
# it. Timing is refused while the 1-minute load average is 2 or more, or a cargo, nextest or rustc
# process runs, unless --force, which marks every number as taken under load; --wait waits up to an
# hour for a quiet host first. Results go to OUT/preempt_cost.txt with the load before and after.
set -euo pipefail
abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
BASE=$(abs "$1"); NEW=$(abs "$2"); OUT=$3; MODE=${4:-}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT/bin"
OUT=$(cd "$OUT" && pwd)
LOG=$OUT/preempt_cost.txt
load() { uptime | sed 's/.*averages*: //' | tr -d ',' | awk '{print $1}'; }
note() { echo "$*" | tee -a "$LOG"; }
: >"$LOG"
cat >"$OUT/tight.nz" <<'EOF'
// A tight arithmetic loop with no call: the back edge's poll at its most visible.
fn main() {
    let mut s = 0
    let mut i = 0
    while i < 400000000 {
        s = (s + i * 7) % 1000003
        i += 1
    }
    println(s)
}
EOF
[ -f "$OUT/words.txt" ] || python3 "$HERE/gen_words.py" "$OUT/words.txt"
for side in base new; do
    N=$BASE; [ $side = new ] && N=$NEW
    note "$side: $("$N" --version)"
    for p in fib words primes; do
        "$N" build --opt-level 2 "$HERE/nazm/$p.nz" -o "$OUT/bin/$side-$p" >/dev/null
    done
    "$N" build --opt-level 2 "$OUT/tight.nz" -o "$OUT/bin/$side-tight" >/dev/null
done
quiet() { [ -z "$(pgrep -x cargo; pgrep -x cargo-nextest; pgrep -x rustc)" ] && awk -v l="$(load)" 'BEGIN { exit !(l < 2) }'; }
if [ "$MODE" = "--wait" ]; then
    tries=0
    while ! quiet && [ $tries -lt 240 ]; do sleep 15; tries=$((tries + 1)); done
fi
if [ "$MODE" = "--force" ]; then
    note "FORCED: numbers below were taken on a host that may be loaded; do not record them"
elif ! quiet; then
    note "REFUSED: the host is not quiet (load $(load)); nothing timed"
    exit 2
fi
cd "$OUT"
pair() { # name args
    note "== $1 $2, load $(load)"
    hyperfine -N --warmup 2 --runs 15 "./bin/base-$1 $2" "./bin/new-$1 $2" 2>&1 \
        | grep -E "Time|±|faster" | sed 's/^ *//' | tee -a "$LOG"
}
pair tight ""
pair fib ""
pair words words.txt
pair primes 1
pair primes "$(getconf _NPROCESSORS_ONLN)"
note "== end, load $(load)"
