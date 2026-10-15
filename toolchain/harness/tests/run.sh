#!/bin/bash
# R7: the harness in Nazm, compared with the Rust harness it ports.
#
#   toolchain/harness/tests/run.sh            the library's tests, every ported CLI test file
#                                             beside its Rust file, every ported gate beside
#                                             `cargo xtask check`
#   toolchain/harness/tests/run.sh tests      only the CLI test files
#   toolchain/harness/tests/run.sh tests F…   only the CLI test files named (`line_breaks` …)
#   toolchain/harness/tests/run.sh checks     only the gates
#
# Prints one line per comparison and ends with `N agree, M differ, stays Rust (K)`; exits 1 when
# anything differs or a Rust run could not be made. A Rust test named in `tests/stays-rust.txt`
# reaches a Rust API the CLI does not offer: it is counted as staying Rust, never as agreeing, and
# it must have no port (the list is revisited after the cut-over, R8).
#
# Environment:
#   HARNESS_NAZM      the `nazm` that builds and runs the harness (default: the 0.10.0 release)
#   CARGO_TARGET_DIR  where Cargo builds the Rust side (default: ~/.cache/nazm-target-track7)
#   NAZM              the `nazm` under test (default: the tree's, `$CARGO_TARGET_DIR/debug/nazm`,
#                     which is the binary the Rust tests run — so both sides test one binary)
set -u

here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root" || exit 1

HARNESS_NAZM=${HARNESS_NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/nazm-target-track7}
export NAZM=${NAZM:-$CARGO_TARGET_DIR/debug/nazm}
export NAZM_HARNESS_ROOT=$root
# What `.cargo/config.toml` gives every process Cargo starts: no ported test writes to the real user
# cache (R1.5, D12).
export NAZM_USER_CACHE_DIR=
what=${1:-all}
[ $# -gt 0 ] && shift
only=("$@")

agree=0
differ=0
stays=0
stays_list="$here/tests/stays-rust.txt"
log=$(mktemp -d "${TMPDIR:-/tmp}/r7-run.XXXXXX")
trap 'rm -rf "$log"' EXIT

note_agree() { agree=$((agree + 1)); printf 'agree   %s\n' "$1"; }
note_differ() { differ=$((differ + 1)); printf 'DIFFER  %s\n' "$1"; }
note_stays() { stays=$((stays + 1)); printf 'rust    %s\n' "$1"; }

# The reason `FILE::TEST` is listed as staying Rust, or nothing.
stays_reason() {
    [ -f "$stays_list" ] || return
    awk -v n="$1" '$1 == n { $1 = ""; sub(/^ +/, ""); print; exit }' "$stays_list"
}

# The files the list names that have no port at all (every test of theirs stays Rust).
stays_only_files() {
    [ -f "$stays_list" ] || return
    sed -nE 's/^([a-z0-9_]+)::.*/\1/p' "$stays_list" | sort -u | while read -r f; do
        [ -f "$here/cli/$f.nz" ] || echo "$f"
    done
}

# --- the CLI test files ----------------------------------------------------------------------

# The test binary a Rust test file is merged into: `mod NAME;` in one of `cli_*.rs`.
binary_of() {
    grep -l "^mod $1;" crates/nazm-cli/tests/cli_*.rs 2>/dev/null | head -1 | sed 's|.*/||; s|\.rs$||'
}

# `NAME pass|fail` for every `@test` of a ported file, from `nazm test --json` (nothing for a
# file with no port).
nazm_verdicts() {
    [ -f "$here/cli/$1.nz" ] || return
    "$HARNESS_NAZM" test --json "$here/cli/$1.nz" 2>/dev/null \
        | grep '"schema":"nazm.test/1"' \
        | sed -E 's/.*"name":"([^"]*)".*"passed":(true|false).*/\1 \2/' \
        | sed 's/ true$/ pass/; s/ false$/ fail/' | sort
}

# `NAME pass|fail` for every test of the Rust file, from libtest's own lines.
rust_verdicts() {
    local bin=$1 file=$2
    cargo test -p nazm-cli --test "$bin" -- "$file::" --test-threads 4 2>/dev/null \
        | sed -nE 's/^test ([^ ]+) \.\.\. (ok|FAILED)$/\1 \2/p' \
        | sed 's/ ok$/ pass/; s/ FAILED$/ fail/' | sort
}

compare_tests() {
    if ! cargo build -q -p nazm-cli --bin nazm 2>"$log/build.err"; then
        note_differ "the tree's nazm did not build: $(tail -1 "$log/build.err")"
        return
    fi
    # Nazm's own code generator (R9, D15), built once from `toolchain/codegen/` by the `nazm` under
    # test, as `differential::nzcg` builds it once per test process: what `--backend nazm` runs.
    if [ "$(uname -s)" = Darwin ]; then
        if "$NAZM" build --no-cache "$root/toolchain/codegen/main.nz" -o "$log/nzcg" >"$log/nzcg.out" 2>&1; then
            export NAZM_HARNESS_NZCG=$log/nzcg
        else
            note_differ "nzcg did not build: $(grep -m1 error "$log/nzcg.out")"
        fi
    fi
    # The harness's own tests: the library and the mutation core.
    for t in "$here"/tests/*_self.nz; do
        local own
        own=$(basename "$t" .nz)
        "$HARNESS_NAZM" test "$t" >"$log/$own.out" 2>&1
        if [ $? -eq 0 ]; then note_agree "$own ($(tail -1 "$log/$own.out"))"; else
            note_differ "$own: $(grep FAIL "$log/$own.out" | head -3)"; fi
    done
    local files=()
    if [ ${#only[@]} -gt 0 ]; then
        for f in "${only[@]}"; do files+=("$here/cli/$f.nz"); done
    else
        files=("$here"/cli/*.nz)
        while read -r f; do files+=("$here/cli/$f.nz"); done < <(stays_only_files)
    fi
    for f in "${files[@]}"; do
        local file bin
        file=$(basename "$f" .nz)
        bin=$(binary_of "$file")
        if [ -z "$bin" ]; then note_differ "$file: no Rust test binary holds it"; continue; fi
        rust_verdicts "$bin" "$file" >"$log/$file.rust"
        nazm_verdicts "$file" >"$log/$file.nazm"
        if [ ! -s "$log/$file.rust" ]; then note_differ "$file: the Rust tests did not run"; continue; fi
        while read -r name verdict; do
            local got why
            got=$(awk -v n="$name" '$1 == n { print $2 }' "$log/$file.nazm")
            why=$(stays_reason "$name")
            if [ -n "$why" ]; then
                if [ -n "$got" ]; then note_differ "$name: listed as staying Rust, and ported"
                else note_stays "$name ($verdict; $why)"; fi
            elif [ "$got" = "$verdict" ]; then note_agree "$name ($verdict)"; else
                note_differ "$name: rust $verdict, nazm ${got:-missing}"; fi
        done <"$log/$file.rust"
        # A test the list keeps in Rust that the Rust file does not have is a stale entry.
        if [ -f "$stays_list" ]; then
            while read -r name _; do
                grep -q "^$name " "$log/$file.rust" || note_differ "$name: listed as staying Rust, and no Rust test has this name"
            done < <(grep "^$file::" "$stays_list")
        fi
        # A Nazm test with no Rust test of its name is a port that invented something.
        while read -r name _; do
            grep -q "^$name " "$log/$file.rust" || note_differ "$name: no Rust test of this name"
        done <"$log/$file.nazm"
    done
}

# --- the gates -------------------------------------------------------------------------------

compare_checks() {
    [ -x "$here/check/main.nz" ] || [ -f "$here/check/main.nz" ] || return
    local exe="$log/nazm-check"
    if ! "$HARNESS_NAZM" build "$here/check/main.nz" -o "$exe" >"$log/check-build.out" 2>&1; then
        note_differ "check/main.nz did not build: $(grep -m1 error "$log/check-build.out")"
        return
    fi
    if ! cargo build -q -p xtask 2>"$log/xtask.err"; then
        note_differ "xtask did not build: $(tail -1 "$log/xtask.err")"
        return
    fi
    "$CARGO_TARGET_DIR/debug/xtask" check >"$log/rust-check.out" 2>&1
    "$exe" "$root" >"$log/nazm-check.out" 2>&1
    compare_reports "the tree" "$log/rust-check.out" "$log/nazm-check.out"
}

# The Rust report's lines for exactly the gates the Nazm report names, against the Nazm report.
compare_reports() {
    local label=$1 rust=$2 nazm=$3
    local names
    names=$(sed -nE 's/^(ok|FAIL)  +//p' "$nazm")
    while IFS= read -r gate; do
        [ -n "$gate" ] || continue
        local r n
        r=$(awk -v g="$gate" '
            /^(ok|FAIL)  / { cur = $0; sub(/^(ok|FAIL) +/, "", cur); keep = (cur == g) }
            keep { print }' "$rust")
        n=$(awk -v g="$gate" '
            /^(ok|FAIL)  / { cur = $0; sub(/^(ok|FAIL) +/, "", cur); keep = (cur == g) }
            keep { print }' "$nazm")
        if [ -z "$r" ]; then note_differ "$label: $gate: the Rust report has no such gate"
        elif [ "$r" = "$n" ]; then note_agree "$label: $(echo "$r" | head -1)"
        else note_differ "$label: $gate: rust [$(echo "$r" | tr '\n' '|')] nazm [$(echo "$n" | tr '\n' '|')]"; fi
    done <<<"$names"
}

case $what in
    all) compare_tests; compare_checks ;;
    tests) compare_tests ;;
    checks) compare_checks ;;
    *) echo "usage: $0 [all|tests|checks]" >&2; exit 2 ;;
esac

echo "$agree agree, $differ differ, stays Rust ($stays)"
[ "$differ" -eq 0 ]
