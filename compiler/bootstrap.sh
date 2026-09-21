#!/bin/sh
# The Nazm bootstrap: C1 -> C2 -> C3, from one compiler source S.
#
#   1. The retained Rust implementation compiles S into C1.
#   2. C1 compiles S into C2.
#   3. C2 compiles S into C3.
#   4. C2 and C3 are compared, and both compile the conformance corpus.
#
# Every stage runs in its own output directory, so no stage can read another's artefacts
# and a stale binary cannot stand in for one that was never built. Failure propagates:
# `set -e` plus an explicit check after every stage, because a script that prints success
# after a failed stage is worse than no script.
#
# What this does and does not establish is recorded in `docs/bootstrap.md`.

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-"$root/target/bootstrap"}
nazm="$root/target/debug/nazm"

# S: the compiler's source, as one translation unit.
#
# Concatenated rather than imported because `use` resolution is the driver's job and the
# compiler written in Nazm does not yet do it — a stated gap, not a trick: the same text
# compiles either way, and a reader can check that by running `nazm check` on both.
sources="lex.nz parse.nz analyse.nz emit.nz"

say() { printf '%s\n' "$*"; }
fail() { printf 'bootstrap: %s\n' "$*" >&2; exit 1; }

# Every stage below is a compiler written in Nazm, run natively with no ceiling. On
# 2026-09-21 that shape panicked the machine three times: the drivers accumulated their
# output by copying, and macOS cannot cap a child's memory (`RLIMIT_AS` is an alias of
# `RLIMIT_RSS`, and `setrlimit(2)` documents it as advisory). So the ceiling has to come
# from outside the process, and refusing has to be explicit rather than silent.
if [ "${NAZM_CONTAINED:-}" != "1" ] && [ "${NAZM_ALLOW_UNCONTAINED:-}" != "1" ]; then
    fail "refusing to bootstrap with no resource ceiling.

  This script runs compiler stages written in Nazm. A stage that allocates without
  bound reaches the machine rather than the process on this host.

  Run it inside the bounded environment, one workload at a time, and set
  NAZM_CONTAINED=1 only where a real ceiling applies (a container with --memory and
  --memory-swap set equal, or a cgroup that enforces one):

      NAZM_CONTAINED=1 $0 ${1:+\"$1\"}

  To override deliberately and accept the risk, set NAZM_ALLOW_UNCONTAINED=1."
fi
if [ "${NAZM_ALLOW_UNCONTAINED:-}" = "1" ]; then
    printf 'bootstrap: warning: NAZM_ALLOW_UNCONTAINED=1 — no memory ceiling. This is the configuration that panicked the machine on 2026-09-21.\n' >&2
fi

[ -x "$nazm" ] || fail "build the Rust implementation first: cargo build -p nazm-cli"
command -v clang >/dev/null 2>&1 || fail "clang is required to assemble the emitted IR"

rm -rf "$out"
mkdir -p "$out/stage1" "$out/stage2" "$out/stage3"

# --- the record -------------------------------------------------------------------
# Identities first, so a result can be attributed. Without these a reproducibility claim
# names no toolchain and is not checkable.
{
  say "date-utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  say "host: $(uname -sm)"
  say "rustc: $(rustc --version 2>/dev/null || echo 'not on PATH')"
  say "clang: $(clang --version | head -1)"
  say "commit: $(git -C "$root" rev-parse HEAD 2>/dev/null || echo 'no git')"
  say "tree-clean: $(test -z "$(git -C "$root" status --porcelain 2>/dev/null)" && echo yes || echo no)"
  say "sources: $sources"
} > "$out/record.txt"

# Assemble a stage twice: once to run, once to compare.
#
# The macOS linker stamps a UUID and embeds the output *filename* in the ad-hoc code
# signature. Neither is anything the compiler produced, so a byte comparison of the
# runnable binaries would fail on metadata. `-no_uuid` removes the first and a constant
# filename in a per-stage directory removes the second — but a binary without an
# `LC_UUID` is one macOS refuses to load, so the normalised copy exists **only** to be
# compared and is never executed. Nothing about code or data is touched by either.
link() {
  stage=$1
  clang -Wno-override-module -o "$stage/nazmc" "$stage/nazmc.ll" \
    || fail "$(basename "$stage") assembly failed"
  mkdir -p "$stage/normalised"
  clang -Wno-override-module -Wl,-no_uuid -o "$stage/normalised/nazmc" "$stage/nazmc.ll" \
    || fail "$(basename "$stage") normalised assembly failed"
}

cat_sources() {
  for f in $sources; do
    # `use` lines are dropped: every source is in this one unit already, and an import
    # of a file that is also concatenated would define its functions twice.
    grep -v '^use "' "$root/compiler/$f"
  done
}

cat_sources > "$out/S.nz"
say "source digest: $(shasum -a 256 "$out/S.nz" | cut -d' ' -f1)" >> "$out/record.txt"
say "source lines: $(wc -l < "$out/S.nz" | tr -d ' ')" >> "$out/record.txt"

# The same source must also pass the reference front end, so a bootstrap cannot succeed
# on a program the language does not accept.
"$nazm" check "$out/S.nz" >/dev/null || fail "S does not check"

# --- stage 1: Rust builds C1 --------------------------------------------------------
say "stage 1: the Rust implementation compiles S into C1"
"$nazm" build "$out/S.nz" -o "$out/stage1/nazmc" >/dev/null || fail "stage 1 failed"
[ -x "$out/stage1/nazmc" ] || fail "stage 1 produced no executable"

# --- stage 2: C1 builds C2 ----------------------------------------------------------
say "stage 2: C1 compiles S into C2"
"$out/stage1/nazmc" "$out/S.nz" > "$out/stage2/nazmc.ll" || fail "stage 2 emission failed"
link "$out/stage2"

# --- stage 3: C2 builds C3 ----------------------------------------------------------
say "stage 3: C2 compiles S into C3"
"$out/stage2/nazmc" "$out/S.nz" > "$out/stage3/nazmc.ll" || fail "stage 3 emission failed"
link "$out/stage3"

# --- the fixpoint -------------------------------------------------------------------
say "comparing C2 and C3"
if cmp -s "$out/stage2/nazmc.ll" "$out/stage3/nazmc.ll"; then
  say "ir: identical" >> "$out/record.txt"
else
  say "ir: DIFFERS" >> "$out/record.txt"
  fail "C2 and C3 emitted different IR — the compiler is not a fixpoint"
fi
if cmp -s "$out/stage2/normalised/nazmc" "$out/stage3/normalised/nazmc"; then
  say "executables: byte-identical once the linker UUID is removed" >> "$out/record.txt"
else
  say "executables: DIFFER" >> "$out/record.txt"
  fail "C2 and C3 differ as executables beyond the linker's UUID"
fi
# Both digests, and the difference between the runnable and normalised forms, so a reader
# can see exactly what was normalised away.
say "c2-digest: $(shasum -a 256 "$out/stage2/normalised/nazmc" | cut -d' ' -f1)" >> "$out/record.txt"
say "c3-digest: $(shasum -a 256 "$out/stage3/normalised/nazmc" | cut -d' ' -f1)" >> "$out/record.txt"
say "c2-runnable-digest: $(shasum -a 256 "$out/stage2/nazmc" | cut -d' ' -f1)" >> "$out/record.txt"
say "runnable-bytes-differing: $(cmp -l "$out/stage2/nazmc" "$out/stage3/nazmc" | wc -l | tr -d ' ')" >> "$out/record.txt"

# --- conformance: both later stages compile the corpus and agree ---------------------
say "conformance: C2 and C3 compile the corpus"
corpus="$root/compiler/conformance"
checked=0
for case in "$corpus"/*.nz; do
  [ -e "$case" ] || continue
  name=$(basename "$case" .nz)
  expected=$(cat "$corpus/$name.expected")
  for stage in stage2 stage3; do
    "$out/$stage/nazmc" "$case" > "$out/$stage/$name.ll" || fail "$stage refused $name"
    clang -Wno-override-module -o "$out/$stage/$name" "$out/$stage/$name.ll" \
      || fail "$stage emitted IR clang rejected for $name"
    got=$("$out/$stage/$name" 2>&1 || true)
    [ "$got" = "$expected" ] || fail "$stage: $name gave '$got', expected '$expected'"
  done
  # And the interpreter agrees, so the expectation is not merely what the compiler does.
  interp=$("$nazm" run "$case" 2>&1 || true)
  [ "$interp" = "$expected" ] || fail "the interpreter disagrees about $name: '$interp'"
  checked=$((checked + 1))
done
say "conformance-cases: $checked" >> "$out/record.txt"
[ "$checked" -gt 0 ] || fail "no conformance cases were run"

say ""
say "bootstrap complete: C2 and C3 are byte-identical, $checked conformance cases agree"
say "record: $out/record.txt"
cat "$out/record.txt"
