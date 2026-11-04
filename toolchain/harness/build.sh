#!/bin/bash
# R8 slice 13 (S13.1): build the harness's Nazm programs with the checkout's `nazm` into
# `target/nzx/` (or `$NZX_OUT`):
#
#     toolchain/harness/build.sh [PROGRAM...]
#
#   nzx           toolchain/harness/nzx/main.nz     --heap-limit 256M (question 5: the runner is
#                                                   itself a Nazm-built process on the host, so
#                                                   its counted heap is at most 256 MiB, and it
#                                                   checks its own image at start)
#   nazm-mutate   toolchain/harness/mutate/main.nz  --heap-limit 2G
#   nazm-fuzz     toolchain/harness/fuzz/main.nz    --heap-limit 2G (S13.5; its targets are built by
#                                                   `nazm-fuzz build`, with SanitizerCoverage, in the container)
#
#   nazm-bench    toolchain/harness/bench/main.nz   --heap-limit 2G (S13.6; it measures only inside the container)
#
# The `nazm` is `$NAZM`, else the one this checkout's Cargo builds
# (`stage_checkout_nazm`: the seed before slice 18, the Nazm-built `nazm` after). Building runs no
# program of the harness's own; it is the compiler's work, as `toolchain/lib/stage.sh`'s scripts
# do it. Run a built `nzx` directly (it is a runner and holds its own lock); run anything else it
# builds only as `nzx host-run` or contained admit it.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
. "$root/toolchain/lib/stage.sh"
stage_name=harness-build

# NAME SOURCE HEAP
table="nzx nzx/main.nz 256M
nazm-mutate mutate/main.nz 2G
nazm-fuzz fuzz/main.nz 2G
nazm-bench bench/main.nz 2G"

nazm=${NAZM:-$(stage_checkout_nazm "$root")}
[ -x "$nazm" ] || stage_fail "no nazm at $nazm: build it (cargo build -p nazm-cli --bin nazm) or set NAZM"
out=${NZX_OUT:-$root/target/nzx}
mkdir -p "$out" || stage_fail "cannot create $out"

want=" $* "
built=0
failed=0
while read -r name source heap; do
    case $want in *" $name "*) ;; "  ") ;; *) continue ;; esac
    bytes=$(stage_heap_bytes "$heap") || stage_fail "$name: $heap is not a size"
    [ "$bytes" -gt 0 ] && [ "$bytes" -le 2147483648 ] || stage_fail "$name: a heap limit of at most 2 GiB"
    if STAGE_HEAP=$heap stage_build "$nazm" --no-cache "$here/$source" -o "$out/$name" > "$out/$name.build.log" 2>&1; then
        printf 'built   %s (heap limit %s)\n' "$name" "$heap"
        built=$((built + 1))
    else
        printf 'FAILED  %s: %s\n' "$name" "$(grep -m1 error "$out/$name.build.log")"
        failed=$((failed + 1))
    fi
done <<< "$table"
echo "$built built, $failed failed ($out)"
[ "$failed" -eq 0 ]
