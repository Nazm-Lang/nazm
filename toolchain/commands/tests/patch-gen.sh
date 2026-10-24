#!/bin/bash
# R8 slice 11: the `patch` cases `refusals.sh --cases` runs, written on the host (they need Python and the reference's `check --json` for the diagnostics that carry a fix; a container has neither).
#
#     REF=target/debug/nazm toolchain/commands/tests/patch-gen.sh OUTFILE
#
# For every input of the corpus (as refusals.sh finds them): `patch rename` at up to six names and `patch fix` at up to three diagnostics with a fix, in the file itself;
# and, for the first file the input imports, `patch rename` at three of its names (a file of the compilation other than its root). One `LABEL||ARGS` line each.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../../.."
ref=${REF:-target/debug/nazm}
out=$1
: > "$out"
find examples tests/compat/v1 compiler/conformance toolchain/codegen/tests/programs crates/nazm-cli/tests/refusals \
    crates/nazm-cli/tests/transfers toolchain/frontend-tests/inputs/check toolchain/frontend-tests/inputs/modules -name '*.nz' | LC_ALL=C sort -u |
    while IFS= read -r f; do
        "$ref" check --json --no-cache "$f" 2>&1 > /dev/null | python3 -I "$here/patch-positions.py" "$PWD/$f" 2> /dev/null |
            while read -r kind byte new; do
                [ -n "$kind" ] || continue
                if [ "$kind" = rename ]; then echo "patch-rename $f $byte||patch rename --json --root $f --file $f --byte $byte --new-name $new" >> "$out"; fi
                if [ "$kind" = fix ]; then echo "patch-fix $f $byte||patch fix --json --root $f --file $f --byte $byte" >> "$out"; fi
            done || true
        python3 -I "$here/patch-imports.py" "$PWD/$f" 2> /dev/null |
            while read -r other byte new; do
                rel=${other#"$PWD/"}
                echo "patch-other $f $rel $byte||patch rename --json --root $f --file $rel --byte $byte --new-name $new" >> "$out"
            done || true
    done
