#!/bin/sh
# Assemble a local release candidate, after every gate has passed.
#
#   scripts/release-candidate.sh [--allow-dirty] [--skip-bench]
#
# "Local" is the whole of it: this builds an artefact under `target/` and prints where.
# It does not tag, push, publish or upload anything, and it never will — that is a
# decision a person makes, and a script that made it would be making it for them.
#
# The gates run in cheapest-first order so a failure costs the least time it can, and
# **every one of them must pass**. A release candidate assembled past a failing gate is
# a tarball with a reassuring name.

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

allow_dirty=no
skip_bench=no
for arg do
    case $arg in
        --allow-dirty) allow_dirty=yes ;;
        --skip-bench) skip_bench=yes ;;
        *) printf 'release: unknown option %s\n' "$arg" >&2; exit 2 ;;
    esac
done

say() { printf '\n=== %s\n' "$*"; }
fail() { printf 'release: %s\n' "$*" >&2; exit 1; }

# A candidate has to be attributable to a commit, or the manifest's commit line is a
# guess about which of the working tree's states was built.
dirty=$(git status --porcelain 2>/dev/null || true)
if [ -n "$dirty" ] && [ "$allow_dirty" = no ]; then
    printf '%s\n' "$dirty" >&2
    fail "the tree is not clean. Commit first, or pass --allow-dirty and accept that the manifest's commit will not describe what was built."
fi

say "formatting"
cargo fmt --all -- --check
./target/debug/nazm fmt --check compiler/*.nz examples/*.nz bench/programs/*.nz 2>/dev/null \
    || cargo run -q -p nazm-cli -- fmt --check compiler/*.nz examples/*.nz bench/programs/*.nz

say "lints"
cargo lint

say "architectural gates"
cargo xtask check

say "tests"
cargo test --workspace

say "release build"
cargo build --release -p nazm-cli

say "the bootstrap chain"
sh compiler/bootstrap.sh

if [ "$skip_bench" = no ]; then
    say "benchmarks against the recorded baseline"
    cargo run -q -p xtask -- bench --runs 5 --check
fi

# --- assemble -----------------------------------------------------------------------
out="$root/target/release-candidate"
rm -rf "$out"
mkdir -p "$out/bin" "$out/docs" "$out/schema" "$out/examples"

cp target/release/nazm "$out/bin/"
cp README.md LICENSE* "$out/" 2>/dev/null || cp README.md "$out/"
cp docs/*.md docs/*.ebnf "$out/docs/"
cp schema/*.json "$out/schema/"
cp examples/*.nz "$out/examples/"
cp bench/baseline.json "$out/docs/performance-baseline.json"
cp target/bootstrap/record.txt "$out/docs/bootstrap-record.txt"

digest() { shasum -a 256 "$1" | cut -d' ' -f1; }

{
    printf 'nazm release candidate\n\n'
    printf 'date-utc:    %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'commit:      %s\n' "$(git rev-parse HEAD 2>/dev/null || echo unknown)"
    printf 'tree-clean:  %s\n' "$([ -z "$dirty" ] && echo yes || echo 'no (--allow-dirty)')"
    printf 'host:        %s\n' "$(uname -sm)"
    printf 'rustc:       %s\n' "$(rustc --version)"
    printf 'clang:       %s\n' "$(clang --version | head -1)"
    printf 'version:     %s\n' "$("$out/bin/nazm" --version 2>/dev/null || echo unknown)"
    printf 'binary-sha:  %s\n' "$(digest "$out/bin/nazm")"
    printf '\n'
    printf 'This is an EXPERIMENTAL candidate built and verified on one machine.\n'
    printf 'It has been checked on %s and nowhere else; other targets are\n' "$(uname -sm)"
    printf 'unverified rather than unsupported. See docs/performance.md for what has\n'
    printf 'been measured, and README.md for what is not built.\n'
    printf '\n'
    printf 'gates passed:\n'
    printf '  cargo fmt --all -- --check\n'
    printf '  nazm fmt --check (every .nz in the tree)\n'
    printf '  cargo lint\n'
    printf '  cargo xtask check\n'
    printf '  cargo test --workspace\n'
    printf '  compiler/bootstrap.sh (C1 -> C2 -> C3, byte-identical)\n'
    [ "$skip_bench" = no ] && printf '  cargo xtask bench --check\n'
    printf '\n'
    printf 'contents:\n'
} > "$out/MANIFEST.txt"

# Every file, with its digest, so the candidate can be checked rather than trusted.
(cd "$out" && find . -type f ! -name MANIFEST.txt | sort | while read -r f; do
    printf '  %s  %s\n' "$(shasum -a 256 "$f" | cut -d' ' -f1)" "${f#./}"
done) >> "$out/MANIFEST.txt"

say "release candidate assembled"
cat "$out/MANIFEST.txt"
printf '\nat: %s\n' "$out"
printf 'Nothing has been published. Tagging and distribution are yours to do.\n'
