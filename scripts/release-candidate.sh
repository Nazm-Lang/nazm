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

# `shasum` on macOS, `sha256sum` on GNU userland. Resolved once rather than at each call,
# and a missing digest tool is a failure: a manifest without digests is a list of names.
if command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
elif command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
    fail "neither shasum nor sha256sum is available; the manifest would have no digests"
fi

# Where build products go. The checkout may be mounted read-only — it is inside the
# contained runner, which is the only place the heavy gates below are allowed to run — so
# the candidate is assembled under the target directory, wherever that is.
target=${CARGO_TARGET_DIR:-"$root/target"}

# A candidate has to be attributable to a commit, or the manifest's commit line is a
# guess about which of the working tree's states was built.
# Who this candidate says it is.
#
# `git` is not in the contained image — deliberately, it has no business there — so inside
# the runner `git rev-parse` returns nothing and the honest answer would be "unknown". The
# supervisor has git and knows exactly which revision it mounted, so it passes both facts
# in. When they are absent this falls back to asking git directly, which is what happens
# when the script is run by hand on a host.
#
# A candidate that cannot name its revision is refused. The whole point of the manifest and
# the provenance record is to let someone else check this artefact against a source tree,
# and neither is worth anything if the tree cannot be identified.
commit=${NAZM_SOURCE_COMMIT:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}
if [ -n "${NAZM_SOURCE_TREE_CLEAN+set}" ]; then
    if [ "$NAZM_SOURCE_TREE_CLEAN" = yes ]; then dirty=; else dirty="the supervisor reported uncommitted changes"; fi
else
    dirty=$(git status --porcelain 2>/dev/null || true)
fi
if [ "$commit" = unknown ]; then
    fail "the source revision could not be established, so the candidate could not be attributed to one"
fi
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
    # The benchmark reads its baseline and writes its record in the same directory, and
    # the checkout is read-only wherever the heavy gates are allowed to run. So it is
    # pointed at a writable copy: the baselines go in, the record comes out, and the
    # source tree is neither written to nor needed for the comparison.
    bench_dir="$target/release-bench"
    rm -rf "$bench_dir"
    mkdir -p "$bench_dir"
    cp bench/baseline*.json "$bench_dir/" 2>/dev/null || true
    NAZM_BENCH_DIR="$bench_dir" cargo run -q -p xtask -- bench --runs 5 --check
fi

# --- assemble -----------------------------------------------------------------------
out="$target/release-candidate"
rm -rf "$out"
mkdir -p "$out/bin" "$out/docs" "$out/schema" "$out/examples" "$out/scripts" \
         "$out/compiler/conformance" "$out/bench/programs" "$out/bench/reference"

# Two different executables, named so they cannot be mistaken for one another.
#
# `nazm` is the Rust implementation: the interpreter, the reference front end, and the
# compiler that builds C1. `nazmc-selfhosted` is the compiler *written in Nazm* — stage 2
# of the bootstrap, built by C1 from `compiler/emit.nz`, and byte-identical to stage 3.
# A package carrying only the first would be claiming a self-hosted compiler and shipping
# the thing that produced it.
cp "$target/release/nazm" "$out/bin/"
if [ -x "$target/bootstrap/stage2/nazmc" ]; then
    cp "$target/bootstrap/stage2/nazmc" "$out/bin/nazmc-selfhosted"
else
    fail "the bootstrap produced no stage-2 compiler to package"
fi
cp README.md LICENSE* "$out/" 2>/dev/null || cp README.md "$out/"
cp docs/*.md docs/*.ebnf "$out/docs/"
cp schema/*.json "$out/schema/"
cp examples/*.nz "$out/examples/"

# The bootstrap's own inputs. A candidate that claims a byte-identical C2 and C3 and
# does not carry the source they were built from is asking to be taken on trust.
cp compiler/*.nz compiler/bootstrap.sh "$out/compiler/"
cp compiler/conformance/* "$out/compiler/conformance/"
cp scripts/*.sh "$out/scripts/"
cp bench/programs/* "$out/bench/programs/"
# The external reference programs, so the one cross-language number in
# docs/performance.md can be re-measured rather than taken on faith.
cp bench/reference/* "$out/bench/reference/"

# Every recorded baseline, not just one: the tool refuses to compare across platforms, so
# a candidate carrying only the machine it happened to be built on would leave the other
# unverifiable. Named after their files so a reader can tell which is which.
for b in bench/baseline*.json; do
    [ -e "$b" ] || continue
    cp "$b" "$out/docs/performance-$(basename "$b")"
done
cp "$target/bootstrap/record.txt" "$out/docs/bootstrap-record.txt"


# --- provenance ---------------------------------------------------------------------
#
# Everything needed to say what this candidate is and what was actually checked against
# it. Separate from MANIFEST.txt because the manifest describes the *contents* and this
# describes the *build* — and because a reader asking "which revision is this, and did
# the tests run against it?" should not have to infer the answer from a file list.
{
    printf 'nazm release candidate — provenance\n\n'
    printf 'source-commit:      %s\n' "$commit"
    printf 'source-tree-clean:  %s\n' "$([ -z "$dirty" ] && echo yes || echo 'no (--allow-dirty)')"
    # The tree as built, independent of git: every tracked-by-this-package source, in a
    # fixed order, digested together. A commit identifies a revision only if the working
    # tree matches it, and this says what was on disk regardless.
    printf 'source-tree-digest: %s\n' \
        "$(find compiler crates xtask docs schema examples scripts bench/programs -type f \
            \( -name '*.rs' -o -name '*.nz' -o -name '*.md' -o -name '*.json' -o -name '*.sh' -o -name '*.ebnf' \) \
            2>/dev/null | LC_ALL=C sort | while read -r f; do sha256 "$f"; done | \
            { if command -v shasum >/dev/null 2>&1; then shasum -a 256; else sha256sum; fi; } | cut -d' ' -f1)"
    printf '\n'
    printf 'target:             %s\n' "$(uname -sm)"
    printf 'rustc:              %s\n' "$(rustc --version)"
    printf 'clang:              %s\n' "$(clang --version | head -1)"
    printf 'container-image:    %s\n' "${NAZM_IMAGE:-nazm-contained:1.98.1}"
    printf 'contained:          %s\n' "${NAZM_CONTAINED:-0}"
    printf '\n'
    printf 'bootstrap:\n'
    sed 's/^/  /' "$target/bootstrap/record.txt"
    printf '\n'
    printf 'verification (this run, this revision):\n'
    printf '  cargo fmt --all -- --check           passed\n'
    printf '  nazm fmt --check (every .nz)         passed\n'
    printf '  cargo lint                           passed\n'
    printf '  cargo xtask check                    passed\n'
    printf '  cargo test --workspace               %s\n' "${suite_line:-passed}"
    printf '  compiler/bootstrap.sh                passed, C2 and C3 byte-identical\n'
    if [ "$skip_bench" = no ]; then
        printf '  cargo xtask bench --check            passed\n'
    else
        printf '  cargo xtask bench --check            SKIPPED (--skip-bench)\n'
    fi
    printf '\n'
    printf 'binaries:\n'
    printf '  bin/nazm                             the Rust implementation, release build,\n'
    printf '                                       for %s. This is what builds C1.\n' "$(uname -sm)"
    printf '  bin/nazmc-selfhosted                 the compiler written in Nazm, as C2 —\n'
    printf '                                       built by C1 from compiler/emit.nz, for %s.\n' "$(uname -sm)"
    printf '                                       Byte-identical to C3.\n'
    printf '\n'
    printf 'Neither binary is portable off %s. The sources are; see docs/bootstrap.md.\n' "$(uname -sm)"
} > "$out/PROVENANCE.txt"

{
    printf 'nazm release candidate\n\n'
    printf 'date-utc:    %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'commit:      %s\n' "$commit"
    printf 'tree-clean:  %s\n' "$([ -z "$dirty" ] && echo yes || echo 'no (--allow-dirty)')"
    printf 'host:        %s\n' "$(uname -sm)"
    printf 'rustc:       %s\n' "$(rustc --version)"
    printf 'clang:       %s\n' "$(clang --version | head -1)"
    printf 'version:     %s\n' "$("$out/bin/nazm" --version 2>/dev/null || echo unknown)"
    printf 'binary-sha:  %s\n' "$(sha256 "$out/bin/nazm")"
    printf '\n'
    printf 'This is an EXPERIMENTAL candidate built and verified on one machine.\n'
    printf 'It has been checked on %s and nowhere else; other targets are\n' "$(uname -sm)"
    printf 'unverified rather than unsupported. See docs/performance.md for what has\n'
    printf 'been measured, and README.md for what is not built.\n'
    printf '\n'
    printf 'contains: the binary, the documentation, the JSON schemas, the examples,\n'
    printf 'the compiler written in Nazm with its bootstrap script and conformance\n'
    printf 'corpus, the benchmark programs, and the scripts the documentation names.\n'
    printf 'Not the Rust source: this repository is that, and the commit above names it.\n'
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
    printf '  %s  %s\n' "$(sha256 "$f")" "${f#./}"
done) >> "$out/MANIFEST.txt"



# --- validate -----------------------------------------------------------------------
#
# Read the manifest back and check every digest against the file it names. A manifest is
# a claim about an artefact; unverified, it is a claim about the script that wrote it.
# This also catches the case that matters most — a file copied in but never listed —
# because the count is compared both ways.
say "validating the candidate against its own manifest"
listed=0
while read -r want name; do
    [ -n "${name:-}" ] || continue
    [ -f "$out/$name" ] || fail "MANIFEST.txt lists $name, which is not in the candidate"
    got=$(sha256 "$out/$name")
    [ "$got" = "$want" ] || fail "$name does not match its digest: manifest $want, file $got"
    listed=$((listed + 1))
done <<EOT
$(sed -n 's/^  \([0-9a-f]\{64\}\)  \(.*\)$/\1 \2/p' "$out/MANIFEST.txt")
EOT
present=$(cd "$out" && find . -type f ! -name MANIFEST.txt | wc -l | tr -d ' ')
[ "$listed" -eq "$present" ] \
    || fail "the candidate holds $present files and the manifest lists $listed"
say "manifest verified: $listed files, every digest matching"
say "release candidate assembled"
cat "$out/MANIFEST.txt"
printf '\nat: %s\n' "$out"
printf 'Nothing has been published. Tagging and distribution are yours to do.\n'
