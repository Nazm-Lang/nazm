#!/bin/bash
# R7: each ported CLI test file against its Rust file under the catalogued mutations they kill.
#
#   toolchain/harness/tests/mutants.sh SCRATCH_TREE [FILE…]
#
# SCRATCH_TREE is a copy of the repository (`git archive HEAD | tar -x -C …`) — never the working
# tree: every mutation is applied there by `nazm-mutate` (built from `mutate/main.nz`), the tree's
# `nazm` and the Rust test binary rebuilt, the Rust killer and the Nazm test of the same name run,
# and the mutation restored. FILEs are ported test files (`line_breaks`, …); default every one in
# `cli/`. One line per mutation and a summary `N agree, M differ`: a mutation agrees when the Rust
# killer and the Nazm test both fail it.
#
# Environment: HARNESS_NAZM (builds and runs the harness; default the 0.10.0 release) and
# CARGO_TARGET_DIR (the scratch tree's own; default ~/.cache/nazm-target-R7-mut).
set -u

here=$(cd "$(dirname "$0")/.." && pwd)
tree=$(cd "${1:?usage: $0 SCRATCH_TREE [FILE…]}" && pwd)
shift
if [ "$tree" = "$(cd "$here/../.." && pwd)" ]; then
    echo "refusing to mutate the working tree; give a scratch copy" >&2
    exit 2
fi
HARNESS_NAZM=${HARNESS_NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/nazm-target-R7-mut}
export NAZM=$CARGO_TARGET_DIR/debug/nazm
export NAZM_HARNESS_ROOT=$tree
catalogue=$tree/xtask/mutations/mutations.toml

log=$(mktemp -d "${TMPDIR:-/tmp}/r7-mutants.XXXXXX")
trap 'rm -rf "$log"' EXIT
mutate=$log/nazm-mutate
"$HARNESS_NAZM" build "$here/mutate/main.nz" -o "$mutate" >"$log/build.out" 2>&1 || {
    cat "$log/build.out" >&2; exit 1; }

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
    for f in "$here"/cli/*.nz; do files+=("$(basename "$f" .nz)"); done
fi

agree=0
differ=0
"$mutate" list "$catalogue" >"$log/list" || exit 1
for file in "${files[@]}"; do
    # name <TAB> file <TAB> killers, for the mutations a test of FILE kills.
    grep -E "	nazm-cli:test=cli_[a-z_]+:$file::" "$log/list" >"$log/$file.mutants"
    while IFS=$'\t' read -r name _ killers; do
        for k in $killers; do
            case $k in nazm-cli:test=cli_*:"$file"::*) ;; *) continue ;; esac
            bin=${k#nazm-cli:test=}; bin=${bin%%:*}
            test=${k#nazm-cli:test=$bin:}
            out=$("$mutate" apply "$tree" "$catalogue" "$name")
            if [ $? -ne 0 ]; then
                differ=$((differ + 1)); echo "DIFFER  $name: $out"; continue
            fi
            (cd "$tree" && cargo build -q -p nazm-cli --bin nazm && \
                cargo test -q -p nazm-cli --test "$bin" --no-run) >"$log/b.out" 2>&1
            built=$?
            rust=pass; nazm=pass
            if [ $built -ne 0 ]; then
                rust="fail (the mutant does not build)"; nazm=$rust
            else
                (cd "$tree" && cargo test -p nazm-cli --test "$bin" -- "$test" --exact) \
                    >"$log/r.out" 2>&1 || rust=fail
                "$HARNESS_NAZM" test --filter "$test" "$here/cli/$file.nz" >"$log/n.out" 2>&1 || nazm=fail
                grep -q "^ok    \|^FAIL  " "$log/n.out" || nazm="fail (no test named $test)"
            fi
            "$mutate" restore "$tree" >/dev/null || { echo "could not restore $name" >&2; exit 1; }
            if [ "$rust" = fail ] && [ "$nazm" = fail ]; then
                agree=$((agree + 1)); echo "agree   $name: $test caught by both"
            else
                differ=$((differ + 1)); echo "DIFFER  $name: $test rust $rust, nazm $nazm"
            fi
        done
    done <"$log/$file.mutants"
done
# The restored tree passes again: rebuild once, so the next run starts from the original.
(cd "$tree" && cargo build -q -p nazm-cli --bin nazm) >/dev/null 2>&1
echo "$agree agree, $differ differ"
[ "$differ" -eq 0 ]
