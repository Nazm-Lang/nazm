#!/bin/sh
# The build-input digest: every file a release candidate is built from, tested with or ships,
# as it is on disk, computed without git.
#
#   scripts/source-digest.sh [--root DIR]          print the digest
#   scripts/source-digest.sh [--root DIR] --list   print the files it covers, in digest order
#
# Why it exists. A candidate's provenance names its commit, and a commit names everything; but
# `git` is not in the contained image, so the build cannot ask it, and a commit is a claim
# about a repository the reader may not have. This digest is checkable from the files alone:
# run it in a checkout of the named commit and compare. Until R1 (2026-10-07) the provenance's
# `source-digest` covered six file extensions under nine directories, so a changed Cargo.lock,
# toolchain pin, Dockerfile, mutation catalogue, `.expected` fixture or linker script passed
# unseen; it said so, and this replaces it.
#
# What it covers: the tracked top-level entries listed below, every regular file under them,
# minus exactly what the repository's .gitignore files exclude there — except
# `review/m1c-review.tar.gz`, which is tracked although a later rule matches it. `xtask/tests/release.rs`
# holds that set equal to `git ls-files --cached --others --exclude-standard`, so a new
# top-level path, or a new ignore rule, cannot fall outside it unnoticed. Each line digested is
# a file's SHA-256 and its path, so a rename changes the digest as a content change does.

set -eu

root=.
list=no
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift 2 ;;
        --list) list=yes; shift ;;
        *) printf 'source-digest: unknown option %s\n' "$1" >&2; exit 2 ;;
    esac
done
cd "$root"

# `shasum` on macOS, `sha256sum` on GNU userland; both print `DIGEST  PATH`, the line digested.
if command -v shasum >/dev/null 2>&1; then
    tool="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
    tool=sha256sum
else
    printf 'source-digest: neither shasum nor sha256sum is available\n' >&2
    exit 1
fi

# Every top-level entry the repository tracks. A file here that is absent is skipped, so a
# fixture tree may hold a subset.
entries='.agent .cargo .dockerignore .github .gitignore AGENTS.md CLAUDE.md CONTRIBUTING.md
Cargo.lock Cargo.toml LICENSE LICENSE-CC-BY-4.0 NOTICE README.md SECURITY.md THIRD-PARTY-NOTICES.md _typos.toml bench compiler corpus crates
deny.toml docker docs editors examples fuzz library openspec review rust-toolchain.toml
rustfmt.toml schema scripts tests toolchain tools xtask'

files() {
    for e in $entries; do
        [ -e "$e" ] || continue
        find "$e" -type f \
            ! -name .DS_Store \
            ! -name '*.rs.bk' \
            ! -path '*/.nazm/*' \
            ! -path 'bench/record.json' \
            ! -path '.agent/state.md' \
            ! -path '.agent/plans/*' \
            ! -path 'fuzz/Cargo.lock' \
            ! -path 'fuzz/target/*' \
            ! -path 'review/m1d/*' \
            ! -path 'review/m1e/*' \
            ! \( -path 'review/*.tar.gz' ! -path 'review/m1c-review.tar.gz' \)
    done | LC_ALL=C sort
}

if [ "$list" = yes ]; then
    files
    exit 0
fi

# One digesting process per batch rather than per file; `xargs` keeps the sorted order.
files | tr '\n' '\0' | xargs -0 $tool | $tool | cut -d' ' -f1
