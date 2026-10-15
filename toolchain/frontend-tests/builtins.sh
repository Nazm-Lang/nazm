#!/bin/sh
# Regenerates `compiler/builtins.nz`, the checker's built-in table, from `nazm_sema::Intrinsic`
# through the reference dumper's `builtins` mode (`openspec/changes/r2-front-end/design.md`, D14).
# With `--check`, says whether the file in the tree is the one this checkout generates.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
ref_target=${R2_REF_TARGET:-"$HOME/.cache/nazm-target-r2"}
(cd "$root/toolchain/frontend-tests/refdump" && CARGO_TARGET_DIR="$ref_target" cargo build --release --quiet)
if [ "${1:-}" = "--check" ]; then
  "$ref_target/release/nazm-r2-refdump" builtins | cmp -s - "$root/compiler/builtins.nz" \
    || { echo "builtins.sh: compiler/builtins.nz is stale; run toolchain/frontend-tests/builtins.sh" >&2; exit 1; }
else
  "$ref_target/release/nazm-r2-refdump" builtins > "$root/compiler/builtins.nz"
fi
