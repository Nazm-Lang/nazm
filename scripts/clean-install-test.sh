#!/bin/sh
# Install a prebuilt archive where no Rust exists, and use it (Gate 3, Part A).
#
#   scripts/clean-install-test.sh ARCHIVE
#
# Unpacks ARCHIVE into a fresh directory, refuses to proceed if cargo or rustc is reachable on the
# PATH it runs with, then checks, interprets, builds and runs a program with the unpacked nazm.
# Prints `clean-install: ok TRIPLE` and exits 0 only if every step did what it should.
set -eu
[ $# -eq 1 ] || { printf 'usage: %s ARCHIVE\n' "$0" >&2; exit 2; }
archive=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
tar -xzf "$archive"
dir=$(ls -d nazm-*/)
triple=${dir#nazm-}; triple=${triple%/}
# The clean PATH: the system's directories and clang's, never a Rust toolchain. On macOS clang's
# real directory, not /usr/bin's xcrun shim: an x86_64 nazm under Rosetta that runs the shim makes
# it an x86_64 process, and Command Line Tools installed on Apple silicon carry only an arm64
# libxcrun. The real clang runs natively whichever architecture starts it.
real_clang=$(xcrun --find clang 2>/dev/null || command -v clang)
clang_dir=$(dirname "$real_clang")
# What the shim would have told clang: where the macOS SDK is.
if command -v xcrun >/dev/null 2>&1; then
    SDKROOT=$(xcrun --show-sdk-path)
    export SDKROOT
fi
PATH="$work/$dir/bin:$clang_dir:/usr/bin:/bin"
export PATH
unset CARGO_HOME RUSTUP_HOME
for tool in cargo rustc; do
    if command -v "$tool" >/dev/null 2>&1; then
        printf 'clean-install: %s is reachable at %s; this is not a clean install\n' "$tool" "$(command -v "$tool")" >&2
        exit 1
    fi
done
cat > hello.nz <<'NZ'
use "@std/text";

fn main(out: OutCap) -> Int {
    print(str_concat(text_to_upper("hello from a prebuilt nazm"), "\n"));
    0
}
NZ
nazm check hello.nz
# `main`'s result is the exit status (0.10.0), not printed.
[ "$(nazm run hello.nz)" = "HELLO FROM A PREBUILT NAZM" ] || { echo 'clean-install: nazm run printed something else' >&2; exit 1; }
nazm build hello.nz -o hello
[ "$(./hello)" = "HELLO FROM A PREBUILT NAZM" ] || { echo 'clean-install: the built program printed something else' >&2; exit 1; }
./hello >/dev/null || { echo 'clean-install: the built program did not exit 0' >&2; exit 1; }
printf 'clean-install: ok %s\n' "$triple"
