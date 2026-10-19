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
tree=$(cd "${1:?usage: $0 SCRATCH_TREE}" && pwd -P)
if [ "$tree" = "$(cd "$here/../.." && pwd)" ]; then
    echo "refusing to seed defects into the working tree; give a scratch copy" >&2
    exit 2
fi
# `nazm-check` and `nazm-mutate` are written in the checkout's language (`Chain[T]`, R1.5 D7; R8
# slice 1), so the copy's own `nazm` builds them (HARNESS_NAZM names another), with a heap limit of
# 2 GiB, and they run contained or, on a macOS host, one at a time through `cargo xtask host-run`
# (`toolchain/lib/stage.sh`; the runbook's R6 rule). The runner is built before the copy's target
# directory is chosen, so the copy's own xtask never replaces it.
. "$here/../lib/stage.sh"
stage_name=seeded
stage_init "$(cd "$here/../.." && pwd)"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/nazm-target-R7-mut}
seeds=$here/tests/seeds.toml

log=$(mktemp -d "${TMPDIR:-/tmp}/r7-seeded.XXXXXX")
trap 'rm -rf "$log"' EXIT
(cd "$tree" && cargo build -q -j "${CARGO_JOBS:-4}" -p xtask -p nazm-cli) || exit 1
xtask=$CARGO_TARGET_DIR/debug/xtask
HARNESS_NAZM=${HARNESS_NAZM:-$CARGO_TARGET_DIR/debug/nazm}
for prog in check mutate; do
    stage_build "$HARNESS_NAZM" "$here/$prog/main.nz" -o "$log/nazm-$prog" >"$log/$prog.build" 2>&1 || {
        cat "$log/$prog.build" >&2; exit 1; }
done

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
    "$STAGE_RUN" 900 "$log/nazm-check" "$tree" >"$log/nazm.out" 2>&1
    local failed=""
    while IFS= read -r gate; do
        local r n
        r=$(block "$gate" "$log/rust.out")
        n=$(block "$gate" "$log/nazm.out")
        if [ "$r" != "$n" ]; then
            failed="$failed$gate: rust [$(echo "$r" | tr '\n' '|')] nazm [$(echo "$n" | tr '\n' '|')]; "
        fi
    done < <(sed -nE 's/^(ok|FAIL)  +//p' "$log/nazm.out")
    # `cargo xtask mutate --plan` and `nazm-mutate plan`: the whole catalogue's plan, or the same
    # problems, on the same tree.
    (cd "$tree" && "$xtask" mutate --plan) >"$log/rust-plan.out" 2>&1
    local rp=$?
    "$STAGE_RUN" 900 "$log/nazm-mutate" plan "$tree" >"$log/nazm-plan.out" 2>&1
    local np=$?
    if [ $rp -ne $np ] || ! cmp -s "$log/rust-plan.out" "$log/nazm-plan.out"; then
        failed="${failed}mutate --plan: exit $rp/$np, $(diff "$log/rust-plan.out" "$log/nazm-plan.out" | head -6 | tr '\n' '|'); "
    fi
    local verdicts
    verdicts=$(grep -E '^FAIL' "$log/nazm.out" | sed 's/^FAIL  //' | tr '\n' ',' | sed 's/,$//')
    [ $np -ne 0 ] && verdicts="${verdicts:+$verdicts,}mutate --plan refused"
    if [ -z "$failed" ]; then
        agree=$((agree + 1)); echo "agree   $label: ${verdicts:-every ported gate ok}"
    else
        differ=$((differ + 1)); echo "DIFFER  $label: $failed"
    fi
}

compare "the pristine copy"

"$STAGE_RUN" 300 "$log/nazm-mutate" list "$seeds" | cut -f1,2 >"$log/names"
# The Rust gates compile `xtask/src/rules.rs` in: a seed there rebuilds them, and so does its
# restore. (A seed elsewhere in `xtask/src/` is a module-boundaries input, read, not built.)
rebuild() { (cd "$tree" && cargo build -q -j "${CARGO_JOBS:-4}" -p xtask) || exit 1; }
while IFS=$'\t' read -r name file; do
    out=$("$STAGE_RUN" 300 "$log/nazm-mutate" apply "$tree" "$seeds" "$name")
    if [ $? -ne 0 ]; then differ=$((differ + 1)); echo "DIFFER  $name: $out"; continue; fi
    made=""
    if [ "$name" = readme-says-not-pre-registered ]; then
        made=$tree/docs/research/evaluation-preregistration.md
        printf '# Pre-registration\n' >"$made"
    fi
    case $file in xtask/src/rules.rs) rebuild ;; esac
    compare "$name"
    [ -n "$made" ] && rm -f "$made"
    "$STAGE_RUN" 300 "$log/nazm-mutate" restore "$tree" >/dev/null || { echo "could not restore $name" >&2; exit 1; }
    case $file in xtask/src/rules.rs) rebuild ;; esac
done <"$log/names"

# A file a gate reads, gone: the failure is worded as the Rust gate words it.
for gone in docs/research/evaluation.md docs/capability-matrix.md docs/guide.md Cargo.lock \
    crates/nazm-cli/src/build/mod.rs docs/mcp.md THIRD-PARTY-NOTICES.md bench/claims.toml \
    docs/performance.md AGENTS.md CLAUDE.md compiler/AGENTS.md .agent/templates/state.md \
    docs/agent-evals.md Cargo.toml xtask/mutations/mutations.toml crates/nazm-bench/src/selftest.rs; do
    mv "$tree/$gone" "$log/gone"
    compare "without $gone"
    mv "$log/gone" "$tree/$gone"
done

# OpenSpec holds changes, each with its proposal, and only process capabilities.
mkdir -p "$tree/openspec/changes/seeded-change" "$tree/openspec/specs/seeded-language-capability"
compare "an OpenSpec change without a proposal, and a language capability in openspec/specs"
rmdir "$tree/openspec/changes/seeded-change" "$tree/openspec/specs/seeded-language-capability"

# A corpus document that is not UTF-8.
cp "$tree/docs/roadmap.md" "$log/kept"
printf '\377\n' >>"$tree/docs/roadmap.md"
compare "docs/roadmap.md not UTF-8"
cp "$log/kept" "$tree/docs/roadmap.md"

compare "the copy restored"
echo "$agree agree, $differ differ"
[ "$differ" -eq 0 ]
