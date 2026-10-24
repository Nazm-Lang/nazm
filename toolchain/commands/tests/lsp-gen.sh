#!/bin/bash
# R8 slice 11: the scripted LSP sessions `refusals.sh lsp-MODE` runs, written on the host (they need Python; a container has none).
#
#     toolchain/commands/tests/lsp-gen.sh ROOT OUTDIR MODE...
#     MODE: plain hover nav flat syms symsflat (and the modes `lsp-session.py` has added: see its header)
#
# For every input of the corpus (as refusals.sh finds them, under the checkout at ROOT: `/src` for a container that mounts the clone there,
# so the sessions name the file the way the container will see it) one session per mode, OUTDIR/MODE/PATH.bin with PATH's slashes as
# underscores. A file that is not UTF-8 gets none (no editor can send it). With REFUSALS_RS=1 the Rust suites' programs are included, every LSP_EVERY-th.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$1
out=$2
shift 2
cd "$here/../../.."
find examples tests/compat/v1 compiler/conformance toolchain/codegen/tests/programs crates/nazm-cli/tests/refusals \
    crates/nazm-cli/tests/transfers toolchain/frontend-tests/inputs/check toolchain/frontend-tests/inputs/modules -name '*.nz' | LC_ALL=C sort -u |
    {
        cat
        # The Rust suites' own programs, every LSP_EVERY-th (REFUSALS_RS=1, as refusals.sh reads them).
        [ -z "${REFUSALS_RS:-}" ] || find toolchain/frontend-tests/inputs/rs -name '*.nz' | LC_ALL=C sort -u | awk -v n="${LSP_EVERY:-1}" 'NR % n == 0'
    } |
    while IFS= read -r f; do
        for m in "$@"; do
            mkdir -p "$out/$m"
            arg=$m
            [ "$m" = plain ] && arg=
            LSP_AS="$PWD=$root" python3 -I "$here/lsp-session.py" "$PWD/$f" $arg > "$out/$m/$(echo "$f" | tr '/' '_').bin" 2> /dev/null || rm -f "$out/$m/$(echo "$f" | tr '/' '_').bin"
        done
    done
