#!/bin/bash
# `nazm build` on a Linux host, as a user runs it (linux-toolchain L1, task 1.3): the admission of the
# Linux triples and `--release` on Linux, which the Rust seed does not answer the same way (it hardens
# only macOS, and it built Linux inside the contained runner alone), so they are held to what they must
# be here, not compared with the seed as build.sh compares the rest.
#
#     toolchain/commands/tests/linux-build.sh BIN WORK
#
# BIN holds a Nazm-built `nazm`, `pl` (its pipeline) and `cg` (its code generator) for this host, the
# names toolchain/selfbuild/programs.txt gives them; WORK is a new directory. Every command runs in WORK
# with NAZM_CONTAINED unset, as a user's shell has it. Runs on Linux only: toolchain/selfbuild/
# linux-selfbuild.sh (aarch64) and linux-x86-selfbuild.sh (x86_64, emulated) run it on their last stage,
# inside the contained runner. The macOS and Windows answers of the same driver stay build.sh's
# (`BUILD_PART=args`), compared with the seed.
#
# The cases:
#   host          `build hello.nz` and `build --target HOST hello.nz`: an ELF of this machine, which runs
#   release       `build --release hello.nz`: a PIE (DYN, FLAGS_1 PIE), stripped (no .symtab), BIND_NOW,
#                 GNU_RELRO, GNU_STACK not executable; it runs
#   run           `run hello.nz` prints the line
#   other-linux   `build --target OTHER --objects o hello.nz` for the other Linux architecture: admitted
#   release-macos `build --release --target aarch64-apple-darwin`: refused (5), a Linux host hardens Linux
#   release-nazm  `build --release --backend nazm`: refused (5), no stack canary
#   windows       `build --target x86_64-pc-windows-gnu`: no linker here (4), `--objects` named
#   wasm          `build --target wasm32-wasip1 --objects o`: not carried (6), the list naming both Linux triples
#
# Prints `linux-build: ok CASE` or `linux-build: FAILED CASE: why` for each, then
# `linux-build: N passed, M failed`; exit 0 only when none failed.
set -u
[ $# -eq 2 ] || { printf 'usage: %s BIN WORK\n' "$0" >&2; exit 2; }
bin=$1 work=$2
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
[ "$(uname -s)" = Linux ] || { echo "linux-build: refused: this is the Linux test of nazm build; on macOS see build.sh" >&2; exit 2; }
case $(uname -m) in
    aarch64 | arm64) host=aarch64-unknown-linux-gnu other=x86_64-unknown-linux-gnu machine=AArch64 ;;
    x86_64 | amd64) host=x86_64-unknown-linux-gnu other=aarch64-unknown-linux-gnu machine=X86-64 ;;
    *) echo "linux-build: refused: no Linux triple for $(uname -m)" >&2; exit 2 ;;
esac
for p in nazm pl cg; do [ -x "$bin/$p" ] || { echo "linux-build: $bin/$p is missing" >&2; exit 2; }; done
mkdir -p "$work" && work=$(CDPATH= cd -- "$work" && pwd)
printf 'fn main() {\n    println("Hello from Nazm on Linux")\n}\n' > "$work/hello.nz"
line='Hello from Nazm on Linux'

passed=0 failed=0
ok() { passed=$((passed + 1)); printf 'linux-build: ok %s\n' "$1"; }
no() { failed=$((failed + 1)); printf 'linux-build: FAILED %s: %s\n' "$1" "$2"; }
# `nz CASE ARGS...`: the Nazm `nazm` in WORK, NAZM_CONTAINED unset; CASE.out, CASE.err, and the status in $st.
nz() {
    c=$1; shift
    (cd "$work" && env -u NAZM_CONTAINED NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$bin/pl" NAZM_NZCG="$bin/cg" \
        timeout 900 "$bin/nazm" "$@" > "$work/$c.out" 2> "$work/$c.err" < /dev/null)
    st=$?
}
err() { tr '\n' ' ' < "$work/$1.err" | head -c 400; }
# `elf FILE`: "" when FILE is a 64-bit ELF executable of this machine, else why not.
elf() {
    h=$(readelf -hW "$1" 2> /dev/null) || { echo "not an ELF file"; return; }
    echo "$h" | grep -q 'Class:[[:space:]]*ELF64' || { echo "not ELF64"; return; }
    # readelf names x86-64 "Advanced Micro Devices X86-64": the machine ends the line.
    echo "$h" | grep -Eq "Machine:.*[[:space:]]$machine\$" || { echo "not $machine: $(echo "$h" | grep Machine:)"; return; }
    echo "$h" | grep -Eq 'Type:[[:space:]]*(EXEC|DYN)' || { echo "not an executable: $(echo "$h" | grep Type:)"; return; }
}
hardened() {
    why=$(elf "$1"); [ -z "$why" ] || { echo "$why"; return; }
    readelf -hW "$1" | grep -q 'Type:[[:space:]]*DYN' || { echo "not a PIE (type is not DYN)"; return; }
    dyn=$(readelf -dW "$1" 2> /dev/null)
    echo "$dyn" | grep -Eq '\(FLAGS_1\).*PIE' || { echo "not a PIE (FLAGS_1 has no PIE)"; return; }
    echo "$dyn" | grep -Eq '\(FLAGS\).*BIND_NOW|\(FLAGS_1\).*NOW|\(BIND_NOW\)' || { echo "not bound now"; return; }
    seg=$(readelf -lW "$1" 2> /dev/null)
    echo "$seg" | grep -q 'GNU_RELRO' || { echo "no GNU_RELRO segment"; return; }
    stack=$(echo "$seg" | awk '$1 == "GNU_STACK" { print $(NF - 1) }')
    [ -n "$stack" ] || { echo "no GNU_STACK segment"; return; }
    case $stack in *E*) echo "the stack is executable (GNU_STACK $stack)"; return ;; esac
    if readelf -SW "$1" 2> /dev/null | grep -q '[[:space:]]\.symtab[[:space:]]'; then echo "not stripped (.symtab)"; fi
}
# `runs CASE EXE`: "" when EXE prints the line and exits 0.
runs() {
    got=$(cd "$work" && timeout 60 "$2" 2> "$work/$1.run.err" < /dev/null); s=$?
    [ "$s" -eq 0 ] && [ "$got" = "$line" ] || echo "it ran with status $s and printed '$(printf '%s' "$got" | head -c 200)'"
}
# `refused CASE STATUS TEXT`: "" when the last command stopped with STATUS, wrote nothing to standard
# output, and said TEXT on standard error.
refused() {
    [ "$st" -eq "$2" ] || { echo "status $st, not $2: $(err "$1")"; return; }
    [ ! -s "$work/$1.out" ] || { echo "it wrote to standard output"; return; }
    grep -qF -- "$3" "$work/$1.err" || echo "it did not say '$3': $(err "$1")"
}

# host
why=
for form in plain target; do
    t=(); [ $form = target ] && t=(--target "$host")
    nz "host-$form" build "${t[@]}" hello.nz -o "hello-$form"
    if [ "$st" -ne 0 ]; then why="$why${form}: status $st: $(err "host-$form"); "; continue; fi
    w=$(elf "$work/hello-$form"); [ -z "$w" ] || { why="$why${form}: $w; "; continue; }
    w=$(runs "host-$form" "$work/hello-$form"); [ -z "$w" ] || why="$why${form}: $w; "
done
if [ -z "$why" ]; then ok "host ($host, built with and without --target)"; else no host "$why"; fi

# release
nz release build --release hello.nz -o hello-release
if [ "$st" -ne 0 ]; then no release "status $st: $(err release)"
else
    why=$(hardened "$work/hello-release"); [ -n "$why" ] || why=$(runs release "$work/hello-release")
    if [ -z "$why" ]; then ok "release (PIE, stripped, BIND_NOW, GNU_RELRO, stack not executable; it runs)"; else no release "$why"; fi
fi

# run
nz run run hello.nz
if [ "$st" -eq 0 ] && [ "$(cat "$work/run.out")" = "$line" ]; then ok run; else no run "status $st, printed '$(head -c 200 "$work/run.out")': $(err run)"; fi

# other-linux
nz other-linux build --target "$other" --objects o-other hello.nz
if [ "$st" -eq 0 ] && [ -f "$work/o-other/link.txt" ] && ls "$work"/o-other/*.o > /dev/null 2>&1; then ok "other-linux ($other, objects)"
else no other-linux "status $st, link.txt $([ -f "$work/o-other/link.txt" ] && echo present || echo missing): $(err other-linux)"; fi

# release-macos
nz release-macos build --release --target aarch64-apple-darwin hello.nz
why=$(refused release-macos 5 'a macOS executable built on macOS, or a Linux one built on Linux')
if [ -z "$why" ]; then ok release-macos; else no release-macos "$why"; fi

# release-nazm
nz release-nazm build --release --backend nazm hello.nz
why=$(refused release-nazm 5 'Nazm'"'"'s own code generator write no stack canary')
if [ -z "$why" ]; then ok release-nazm; else no release-nazm "$why"; fi

# windows
nz windows build --target x86_64-pc-windows-gnu hello.nz
# shellcheck disable=SC2016 # the backquotes are the message's own
why=$(refused windows 4 'this host has no linker for `x86_64-pc-windows-gnu`; build it with `--objects DIR`')
if [ -z "$why" ]; then ok windows; else no windows "$why"; fi

# wasm
nz wasm build --target wasm32-wasip1 --objects o-wasm hello.nz
why=$(refused wasm 6 'aarch64-unknown-linux-gnu and x86_64-unknown-linux-gnu; WebAssembly, WASI')
if [ -z "$why" ]; then ok wasm; else no wasm "$why"; fi

printf 'linux-build: %s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
