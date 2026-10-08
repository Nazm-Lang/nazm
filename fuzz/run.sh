#!/bin/sh
# The fuzz campaign, inside the contained runner (N93, docs/architecture.md §7.94).
#
#   sh fuzz/run.sh SECONDS [TARGET...]
#
# Builds `nazm-fuzz` with SanitizerCoverage — `--target` named, so the flags reach the code under
# test and not build scripts — and runs each target for SECONDS from its persistent corpus and
# the repository's programs, then minimises what it kept. Writes `/work/fuzzout/fuzz-min.tar`
# (each target's minimised corpus) and `/work/fuzzout/report.txt` (one `nazm.fuzz/1` line per
# target, and every crash). Exit 1 if any target found a crash.
set -eu
# Only inside the contained runner: its cgroup caps memory, so `memory.max` is a number. On a
# host it is absent or `max`, and fuzzing there is refused (Q1-A2-02).
case $(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo none) in
    ''|*[!0-9]*|0) echo "fuzz: refusing to run with no memory ceiling; use \`cargo xtask contained fuzz\`" >&2; exit 2 ;;
esac
secs=${1:-300}
[ $# -gt 0 ] && shift
targets=${*:-front lower run lir manifest}
work=/work/fuzz
rm -rf "$work" /work/fuzzout
mkdir -p "$work" /work/fuzzout
# The whole tree: the crates inherit their package fields from the root manifest.
tar -C /src --exclude=./target --exclude=./.git --exclude=./fuzz/target -cf - . | tar -C "$work" -xmf -
# The workspace's versions, offline.
cp "$work/Cargo.lock" "$work/fuzz/Cargo.lock"
host=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR=/work/fuzz-target
export RUSTFLAGS="-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3 -C llvm-args=-sanitizer-coverage-trace-pc-guard"
echo "fuzz: building the instrumented fuzzer" >&2
(cd "$work/fuzz" && cargo build --release --offline -q --target "$host")
echo "fuzz: built; $secs s per target: $targets" >&2
fuzzer="/work/fuzz-target/$host/release/nazm-fuzz"
status=0
for t in $targets; do
  case $t in
    manifest) extra="--corpus $work/examples" ;;
    *) extra="--corpus $work/examples --corpus $work/library --corpus $work/bench/programs --corpus $work/crates/nazm-cli/tests/fuzz-corpus" ;;
  esac
  mkdir -p "$work/fuzz/corpus/$t"
  echo "fuzz: $t" >&2
  # shellcheck disable=SC2086
  if ! "$fuzzer" "$t" --seconds "$secs" --corpus "$work/fuzz/corpus/$t" $extra \
      --out "/work/fuzzout/$t" --minimize > /dev/null 2>> /work/fuzzout/report.txt; then
    status=1
  fi
  for c in /work/fuzzout/$t/crashes/*.txt; do
    [ -f "$c" ] || continue
    echo "--- $t crash $(basename "$c")" >> /work/fuzzout/report.txt
    cat "$c" >> /work/fuzzout/report.txt
    echo >> /work/fuzzout/report.txt
  done
done
(cd /work/fuzzout && tar -cf fuzz-min.tar */min */crashes 2>/dev/null || true)
cat /work/fuzzout/report.txt
exit $status
