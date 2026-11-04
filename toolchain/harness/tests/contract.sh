#!/bin/bash
# R8 slice 14: the black-box suite against the Nazm-built `nazm` (`rewrite-in-nazm` 3.1, 3.2, 3.4).
#
#   cargo xtask contained --profile p4t4 run 'bash toolchain/harness/tests/contract.sh [FILE ...]'
#
# The ported CLI tests (`toolchain/harness/cli/*.nz`, every `@test` of the Rust suite that has a port)
# are run by the checkout's Rust `nazm test` (the seed, the harness's driver, as `run.sh` does) with
# `NAZM` = the Nazm-built `nazm` (built here by the seed, with the pipeline beside it in `libexec/`),
# so each test's children are the Nazm `nazm`. A test that fails is run again with `NAZM` = the seed:
# a test that fails on both is not the port's fault (`both-fail`); one that passes on the seed and
# fails on the port is a `PORT-FAIL`, a defect to fix or to record. A file the port could not run at
# all (the driver's own timeout, a build that did not finish) is counted `unrun`.
#
# Prints `port-pass`, `PORT-FAIL`, `both-fail` and `unrun` lines, and ends with the counts. Logs
# (one JSON file per run, the build logs) go under `$CONTRACT_OUT` (default `$TMPDIR/contract`).
#
#   FILE ...           the test files by name (`cli`, `exit_status`), default every file in cli/
#   CONTRACT_OUT       where the logs go
#   CONTRACT_DEADLINE  seconds one file may take (default 1800)
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root" || exit 1
export NAZM_HARNESS_ROOT=$root
export NAZM_USER_CACHE_DIR=
. "$root/toolchain/lib/stage.sh"
stage_name=contract
stage_init "$root" contained

OUT=${CONTRACT_OUT:-${TMPDIR:-/tmp}/contract}
DEADLINE=${CONTRACT_DEADLINE:-1800}
mkdir -p "$OUT/bin" "$OUT/libexec/nazm" "$OUT/logs"

cargo build --locked -q -p nazm-cli --bin nazm || stage_fail "the seed did not build"
SEED=${CARGO_TARGET_DIR:-$root/target}/debug/nazm
[ -x "$SEED" ] || stage_fail "no seed at $SEED"

build() { # SOURCE OUT
    stage_build "$SEED" --stack-size 256M "$1" -o "$2" > "$2.log" 2>&1 || { echo "contract: $1 did not build:"; tail -5 "$2.log"; exit 2; }
}
if [ ! -x "$OUT/bin/nazm" ]; then build "$root/toolchain/commands/nazm.nz" "$OUT/bin/nazm"; fi
if [ ! -x "$OUT/libexec/nazm/nazm-pipeline" ]; then build "$root/toolchain/pipeline/main.nz" "$OUT/libexec/nazm/nazm-pipeline"; fi
PORT=$OUT/bin/nazm
export NAZM_LIBEXEC=$OUT/libexec/nazm
# Tools the ported tests use (as run.sh builds them): the JSON tool for documents too large for the interpreter.
if [ ! -x "$OUT/jtool" ]; then build "$here/tools/jtool.nz" "$OUT/jtool"; fi
export NAZM_HARNESS_JTOOL=$OUT/jtool
export NAZM_HARNESS_BUILDER=$SEED
# The Nazm-built nazm carries no Cranelift (`rewrite-in-nazm` decision 3): the legs that build with it are left out.
export NAZM_HARNESS_NO_CRANELIFT=1

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
    for f in "$here"/cli/*.nz; do files+=("$(basename "$f" .nz)"); done
fi

verdicts() { # JSONFILE -> "name pass|fail"
    grep '"schema":"nazm.test/1"' "$1" | sed -E 's/.*"name":"([^"]*)".*"passed":(true|false).*/\1 \2/' \
        | sed 's/ true$/ pass/; s/ false$/ fail/' | sort
}

pass=0 portfail=0 both=0 unrun=0 files_run=0
for f in "${files[@]}"; do
    src=$here/cli/$f.nz
    [ -f "$src" ] || { echo "UNRUN   $f: no such file"; unrun=$((unrun + 1)); continue; }
    total=$(grep -c '^@test' "$src")
    perl -e 'alarm shift; exec @ARGV or exit 127' "$DEADLINE" env NAZM="$PORT" "$SEED" test --json "$src" > "$OUT/logs/$f.port.json" 2> "$OUT/logs/$f.port.err" < /dev/null
    verdicts "$OUT/logs/$f.port.json" > "$OUT/logs/$f.port.v"
    got=$(wc -l < "$OUT/logs/$f.port.v" | tr -d ' ')
    if [ "$got" -lt "$total" ]; then
        echo "UNRUN   $f: $got of $total tests reported ($(tail -1 "$OUT/logs/$f.port.err" | cut -c1-160))"
        unrun=$((unrun + total - got))
    fi
    files_run=$((files_run + 1))
    pass_here=$(grep -c ' pass$' "$OUT/logs/$f.port.v")
    pass=$((pass + pass_here))
    if grep -q ' fail$' "$OUT/logs/$f.port.v"; then
        perl -e 'alarm shift; exec @ARGV or exit 127' "$DEADLINE" env NAZM="$SEED" "$SEED" test --json "$src" > "$OUT/logs/$f.seed.json" 2> "$OUT/logs/$f.seed.err" < /dev/null
        verdicts "$OUT/logs/$f.seed.json" > "$OUT/logs/$f.seed.v"
        while read -r name v; do
            [ "$v" = fail ] || continue
            s=$(awk -v n="$name" '$1 == n { print $2 }' "$OUT/logs/$f.seed.v")
            if [ "$s" = pass ]; then
                portfail=$((portfail + 1)); echo "PORT-FAIL $name"
                grep -F "\"name\":\"$name\"" "$OUT/logs/$f.port.json" | head -1 | sed -E 's/.*"interpreted":\{"output":"//' | cut -c1-${CONTRACT_SHOW:-400} | sed 's/^/    /' 
            else
                both=$((both + 1)); echo "both-fail $name (seed: ${s:-missing})"
            fi
        done < "$OUT/logs/$f.port.v"
    fi
    echo "file    $f: $pass_here of $total pass on the port"
done
echo "contract: files $files_run, port-pass $pass, PORT-FAIL $portfail, both-fail $both, unrun $unrun"
[ "$portfail" -eq 0 ] && [ "$unrun" -eq 0 ]
