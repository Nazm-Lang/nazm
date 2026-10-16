#!/bin/sh
# R1.5 7.2 (design D16): the release archive is reproducible — built twice from this checkout, in
# two target directories, it is the same bytes.
#
#   scripts/reproducible-check.sh TRIPLE OUTDIR
#
# Builds `nazm` in release twice (`cargo build --release --locked -p nazm-cli`, each in its own
# OUTDIR/target-N, with the checkout's and the target directory's paths remapped so neither is
# written into the binary), packages each with scripts/host-archive.sh under one
# SOURCE_DATE_EPOCH (the last commit's time), and compares the two binaries and the two archives
# byte for byte. Exits 0 when both are identical; otherwise names every file of the archives that
# differs and exits 1. Signing is not done here: that is the copyright holder's step at release
# time (scripts/sign-release.sh), and it signs what this check showed reproducible.
set -eu
[ $# -eq 2 ] || { printf 'usage: %s TRIPLE OUTDIR\n' "$0" >&2; exit 2; }
triple=$1
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$2"
out=$(CDPATH= cd -- "$2" && pwd)
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}
export SOURCE_DATE_EPOCH

for n in 1 2; do
    t="$out/target-$n"
    rm -rf "$out/archive-$n"
    printf 'reproducible-check: build %s of 2\n' "$n"
    (cd "$root" && CARGO_TARGET_DIR="$t" \
        RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$root=/nazm --remap-path-prefix=$t=/target" \
        cargo build --release --locked -p nazm-cli)
    "$root/scripts/host-archive.sh" "$triple" "$t/release/nazm" "$out/archive-$n"
done

sum() { shasum -a 256 "$1" | cut -d' ' -f1; }
a="$out/archive-1/nazm-$triple.tar.gz"
b="$out/archive-2/nazm-$triple.tar.gz"
bin1=$(sum "$out/target-1/release/nazm")
bin2=$(sum "$out/target-2/release/nazm")
arc1=$(sum "$a")
arc2=$(sum "$b")
printf 'nazm      %s\n          %s\n' "$bin1" "$bin2"
printf 'archive   %s\n          %s\n' "$arc1" "$arc2"
if [ "$bin1" = "$bin2" ] && [ "$arc1" = "$arc2" ]; then
    printf 'reproducible-check: %s is reproducible\n' "nazm-$triple.tar.gz"
    exit 0
fi
# What differs, file by file.
for n in 1 2; do
    rm -rf "$out/unpacked-$n"
    mkdir -p "$out/unpacked-$n"
    tar -C "$out/unpacked-$n" -xzf "$out/archive-$n/nazm-$triple.tar.gz"
done
diff -rq "$out/unpacked-1" "$out/unpacked-2" || true
printf 'reproducible-check: NOT reproducible\n' >&2
exit 1
