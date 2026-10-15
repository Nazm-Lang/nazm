#!/bin/sh
# R2's host check (`openspec/changes/r2-front-end/design.md`, D6): only what the runbook allows on
# a host. The reference checks the compiler's source and the R2 drivers; no generated compiler
# stage (C1, C2, C3) runs here — every stage comparison, the bootstrap's fixpoint and the selfhost
# suite are the coordinator's, contained (runbook, *Mandatory containment*; compiler/AGENTS.md).
# The front end's comparisons with the Rust `nazm` are `run.sh`.

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
nazm=${NAZM:-"$HOME/.local/nazm-0.10.0/bin/nazm"}

fail() { printf 'host-check: %s\n' "$*" >&2; exit 1; }

"$nazm" check --no-cache "$root/compiler/emit.nz" >/dev/null || fail "the compiler source does not check under the reference"
for driver in tokens tree diagnose; do
  "$nazm" check --no-cache "$root/compiler/$driver.nz" >/dev/null || fail "compiler/$driver.nz does not check under the reference"
done
"$nazm" fmt --check "$root"/compiler/*.nz >/dev/null || fail "compiler/*.nz is not formatted"
echo "host-check: passed — the reference checks the compiler and the R2 drivers, and they are formatted (stages run contained)"
