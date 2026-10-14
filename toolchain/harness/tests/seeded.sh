#!/bin/bash
# R7: every ported gate beside `cargo xtask check` on a scratch copy of the tree, pristine and with
# each defect of `seeds.toml` seeded — the same report lines, or a difference printed.
#
#   toolchain/harness/tests/seeded.sh SCRATCH_TREE
#
# SCRATCH_TREE is a copy of the repository (`git archive HEAD | tar -x -C …`), never the working
# tree. The Rust gates bake their root in at compile time, so `xtask` is built from the copy, with
# CARGO_TARGET_DIR (default ~/.cache/nazm-target-R7-mut). Seeds are applied and restored by
# `nazm-mutate`; a few that remove or add a whole file are done here.
set -u

here=$(cd "$(dirname "$0")/.." && pwd)
tree=$(cd "${1:?usage: $0 SCRATCH_TREE}" && pwd)
if [ "$tree" = "$(cd "$here/../.." && pwd)" ]; then
    echo "refusing to seed defects into the working tree; give a scratch copy" >&2
    exit 2
fi
HARNESS_NAZM=${HARNESS_NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/nazm-target-R7-mut}
seeds=$here/tests/seeds.toml

log=$(mktemp -d "${TMPDIR:-/tmp}/r7-seeded.XXXXXX")
trap 'rm -rf "$log"' EXIT
for prog in check mutate; do
    "$HARNESS_NAZM" build "$here/$prog/main.nz" -o "$log/nazm-$prog" >"$log/$prog.build" 2>&1 || {
        cat "$log/$prog.build" >&2; exit 1; }
done
(cd "$tree" && cargo build -q -p xtask) || exit 1
xtask=$CARGO_TARGET_DIR/debug/xtask

agree=0
differ=0
# The report lines of `report` for exactly the gates the Nazm report names.
block() {
    awk -v g="$1" '
        /^(ok|FAIL)  / { cur = $0; sub(/^(ok|FAIL) +/, "", cur); keep = (cur == g) }
        keep { print }' "$2"
}
compare() {
    local label=$1
    (cd "$tree" && "$xtask" check) >"$log/rust.out" 2>&1
    "$log/nazm-check" "$tree" >"$log/nazm.out" 2>&1
    local failed=""
    while IFS= read -r gate; do
        local r n
        r=$(block "$gate" "$log/rust.out")
        n=$(block "$gate" "$log/nazm.out")
        if [ "$r" != "$n" ]; then
            failed="$failed$gate: rust [$(echo "$r" | tr '\n' '|')] nazm [$(echo "$n" | tr '\n' '|')]; "
        fi
    done < <(sed -nE 's/^(ok|FAIL)  +//p' "$log/nazm.out")
    # The mutations-apply gate's exactly-once half (`nazm-mutate check`): its lines against the
    # Rust gate's lines of the same kinds, in order. The other half (`plan::validate`) is not
    # ported, so its lines are left out of the Rust side here — and named as such.
    (cd "$tree" && "$log/nazm-mutate" check "$tree") >"$log/nazm-mut.out" 2>&1
    block "mutations apply" "$log/rust.out" | sed -n 's/^        //p' \
        | grep -E ': its pattern is not in |: its pattern appears |: cannot read |has an entry missing' \
        >"$log/rust-mut.out"
    if ! cmp -s "$log/rust-mut.out" "$log/nazm-mut.out"; then
        failed="${failed}mutations apply (exactly once): rust [$(tr '\n' '|' <"$log/rust-mut.out")] nazm [$(tr '\n' '|' <"$log/nazm-mut.out")]; "
    fi
    local verdicts
    verdicts=$(grep -E '^FAIL' "$log/nazm.out" | sed 's/^FAIL  //' | tr '\n' ',' | sed 's/,$//')
    if [ -s "$log/nazm-mut.out" ]; then verdicts="${verdicts:+$verdicts,}mutations apply (exactly once)"; fi
    if [ -z "$failed" ]; then
        agree=$((agree + 1)); echo "agree   $label: ${verdicts:-every ported gate ok}"
    else
        differ=$((differ + 1)); echo "DIFFER  $label: $failed"
    fi
}

compare "the pristine copy"

"$log/nazm-mutate" list "$seeds" | cut -f1 >"$log/names"
while read -r name; do
    out=$("$log/nazm-mutate" apply "$tree" "$seeds" "$name")
    if [ $? -ne 0 ]; then differ=$((differ + 1)); echo "DIFFER  $name: $out"; continue; fi
    made=""
    if [ "$name" = readme-says-not-pre-registered ]; then
        made=$tree/docs/research/evaluation-preregistration.md
        printf '# Pre-registration\n' >"$made"
    fi
    compare "$name"
    [ -n "$made" ] && rm -f "$made"
    "$log/nazm-mutate" restore "$tree" >/dev/null || { echo "could not restore $name" >&2; exit 1; }
done <"$log/names"

# A file a gate reads, gone: the failure is worded as the Rust gate words it.
for gone in docs/research/evaluation.md docs/capability-matrix.md docs/guide.md Cargo.lock \
    crates/nazm-cli/src/build/mod.rs; do
    mv "$tree/$gone" "$log/gone"
    compare "without $gone"
    mv "$log/gone" "$tree/$gone"
done

compare "the copy restored"
echo "$agree agree, $differ differ"
[ "$differ" -eq 0 ]
