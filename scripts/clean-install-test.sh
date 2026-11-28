#!/bin/sh
# Install a prebuilt archive where no Rust exists, and use it (Gate 3, Part A).
#
#   scripts/clean-install-test.sh ARCHIVE
#   scripts/clean-install-test.sh --container ARCHIVE
#
# Unpacks ARCHIVE into a fresh directory, refuses to proceed if cargo or rustc is reachable on the
# PATH it runs with, then checks, interprets, builds and runs a program with the unpacked nazm —
# and, where the archive carries libexec/nazm/nzcg, builds and runs it with --backend nazm. On Linux
# (linux-toolchain L4) also `nazm --version`, `nazm fmt --check`, and a `--release` build that runs.
# Prints `clean-install: ok TRIPLE` and exits 0 only if every step did what it should.
#
# With --container (on the host; the Linux archives, nazm-*-unknown-linux-gnu.tar.gz): the same test in
# a fresh Debian 13 container of the archive's architecture (docker/clean-install.Dockerfile, which
# installs only what the archive's INSTALL.txt asks for: clang and libc6-dev), with no network, the
# archive and this script mounted read-only, under a 4 GiB memory limit; the container is removed
# after. An x86_64 archive on an aarch64 host runs under Docker's amd64 emulation, and the report says so.
set -eu
if [ "${1:-}" = --container ]; then
    [ $# -eq 2 ] || { printf 'usage: %s --container ARCHIVE\n' "$0" >&2; exit 2; }
    archive=$(CDPATH= cd -- "$(dirname -- "$2")" && pwd)/$(basename -- "$2")
    root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
    case $(basename -- "$archive") in
        nazm-aarch64-unknown-linux-gnu.tar.gz) platform=linux/arm64 arch=aarch64 ;;
        nazm-x86_64-unknown-linux-gnu.tar.gz) platform=linux/amd64 arch=x86_64 ;;
        *) printf 'clean-install: --container takes a Linux archive (nazm-aarch64-unknown-linux-gnu.tar.gz or nazm-x86_64-unknown-linux-gnu.tar.gz), not %s\n' "$archive" >&2; exit 2 ;;
    esac
    [ -f "$archive" ] || { printf 'clean-install: %s is not a file\n' "$archive" >&2; exit 2; }
    here=$(uname -m); [ "$here" = arm64 ] && here=aarch64
    if [ "$here" = "$arch" ]; then how=native; else how="emulated ($platform on an $here host, Docker's emulation)"; fi
    image=nazm-clean-install:$arch
    docker build -q --platform "$platform" -f "$root/docker/clean-install.Dockerfile" -t "$image" "$root/docker" > /dev/null
    printf 'clean-install: %s in a fresh %s container of %s, %s\n' "$(basename -- "$archive")" "$platform" "$(docker run --rm --platform "$platform" "$image" sh -c '. /etc/os-release; echo "$PRETTY_NAME"')" "$how"
    exec docker run --rm --network none --platform "$platform" --memory 4g --memory-swap 4g --pids-limit 512 \
        -v "$archive:/in/$(basename -- "$archive"):ro" -v "$root/scripts/clean-install-test.sh:/in/clean-install-test.sh:ro" \
        "$image" sh /in/clean-install-test.sh "/in/$(basename -- "$archive")"
fi
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
# Nazm's own code generator, where the archive carries it (the macOS targets): found beside `nazm`,
# where the backend looks, never through NAZM_NZCG.
if [ -e "$work/$dir/libexec/nazm/nzcg" ]; then
    unset NAZM_NZCG
    nazm build --backend nazm hello.nz -o hello-nzcg
    [ "$(./hello-nzcg)" = "HELLO FROM A PREBUILT NAZM" ] || { echo 'clean-install: the program nzcg built printed something else' >&2; exit 1; }
fi
# On Linux, what INSTALL.txt promises there: the version, the formatter, and a release build that runs.
if [ "$(uname -s)" = Linux ]; then
    v=$(nazm --version) || { echo 'clean-install: nazm --version failed' >&2; exit 1; }
    printf 'clean-install: %s\n' "$v"
    printf 'use "@std/text"\n\nfn main(out: OutCap) -> Int {\n    print(str_concat(text_to_upper("hello from a prebuilt nazm"), "\\n"))\n    0\n}\n' > tidy.nz
    nazm fmt --check tidy.nz || { echo 'clean-install: nazm fmt --check refused a formatted file' >&2; exit 1; }
    printf 'fn main() {\nprintln("x")\n}\n' > untidy.nz
    if nazm fmt --check untidy.nz > /dev/null 2>&1; then echo 'clean-install: nazm fmt --check passed an unformatted file' >&2; exit 1; fi
    nazm build --release hello.nz -o hello-release
    [ "$(./hello-release)" = "HELLO FROM A PREBUILT NAZM" ] || { echo 'clean-install: the release build printed something else' >&2; exit 1; }
    printf 'clean-install: --version, check, run, build, build --release, --backend nazm and fmt --check as INSTALL.txt says, with %s added to Debian'"'"'s base image\n' \
        "$(dpkg-query -W -f '${binary:Package} ' clang libc6-dev 2> /dev/null | sed 's/ $//')"
fi
printf 'clean-install: ok %s\n' "$triple"
