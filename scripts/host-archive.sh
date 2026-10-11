#!/bin/sh
# Package a prebuilt Nazm toolchain for one host (Gate 3, docs/systems-domains.md Part A).
#
#   scripts/host-archive.sh TRIPLE BINARY OUTDIR
#
# Writes OUTDIR/nazm-TRIPLE.tar.gz: bin/nazm (the standard library and runtime are inside it),
# the licence, README, the getting-started and support documents, and INSTALL.txt saying what the
# host must provide — a C toolchain (clang) for native builds, never Rust. Local only: nothing is
# uploaded or published.
set -eu
[ $# -eq 3 ] || { printf 'usage: %s TRIPLE BINARY OUTDIR\n' "$0" >&2; exit 2; }
triple=$1 binary=$2 outdir=$3
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ -x "$binary" ] || { printf 'host-archive: %s is not an executable\n' "$binary" >&2; exit 2; }
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir="$stage/nazm-$triple"
mkdir -p "$dir/bin" "$dir/docs" "$outdir"
cp "$binary" "$dir/bin/nazm"
cp "$root/README.md" "$root"/LICENSE* "$root/NOTICE" "$dir/" 2>/dev/null || cp "$root/README.md" "$dir/"
cp "$root/docs/getting-started.md" "$root/docs/support.md" "$dir/docs/"
version=$("$dir/bin/nazm" --version 2>/dev/null || echo unknown)
cat > "$dir/INSTALL.txt" <<TXT
$version, prebuilt for $triple.

Unpack anywhere and put bin/ on PATH. Nothing else is installed: the standard library and the
runtime are inside bin/nazm.

\`nazm run\`, \`nazm check\`, \`nazm fmt\` and the language service need nothing more. \`nazm build\`
writes native code through clang, which must be on PATH (Xcode's command-line tools on macOS, the
clang package on Linux). Rust is never needed.

What this host has shown is in docs/support.md.
TXT
tar -C "$stage" -czf "$outdir/nazm-$triple.tar.gz" "nazm-$triple"
printf 'host-archive: %s/nazm-%s.tar.gz\n' "$outdir" "$triple"
