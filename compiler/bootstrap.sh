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
out=${1:-"${CARGO_TARGET_DIR:-$root/target}/bootstrap"}
# The Rust implementation, C0. `CARGO_TARGET_DIR` is honoured because the contained
# runner puts Linux artefacts there: the checkout's own `target/` holds
# aarch64-apple-darwin binaries, and running one under a Linux kernel is an
# "Exec format error", not a bootstrap failure. `NAZM` overrides both.
nazm=${NAZM:-"${CARGO_TARGET_DIR:-$root/target}/debug/nazm"}

# S: the compiler's source, as one translation unit.
#
# The chain compiles `compiler/emit.nz` itself, imports and all. Every stage resolves
# `use` with the resolver written in Nazm (`compiler/module.nz`), so what is bootstrapped
# is the compiler as it is written rather than a concatenation of it.
#
# The list below is for the record only — the digest and line count of each source that
# ends up in the unit. Nothing reads it to decide what to compile.
root_source="emit.nz"
sources="emit.nz analyse.nz module.nz parse.nz lex.nz"
# The core prelude: the toolchain's own source, which every stage loads as the last module
# of every compilation — the compiler's own included. Not generated, not embedded in any
# stage: the same file, read by C1, C2 and C3, and recorded below like the compiler source.
core="$root/library/core/prelude.nz"

say() { printf '%s\n' "$*"; }
# `shasum` on macOS, `sha256sum` on GNU userland. A digest line that silently vanished
# would weaken the reproducibility record without failing anything, so this resolves once
# and fails loudly if neither exists.
if command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
  sha256() { printf 'no-sha256-tool\n'; }
fi
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
# The record's date. With SOURCE_DATE_EPOCH set — the release gate sets it to the commit's
# time — it is that instant, so two runs of one commit write the same record; otherwise it is
# the run's own. GNU `date -d @N` in the contained image, BSD `date -r N` on macOS.
if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
  stamp="$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null \
    || date -u -r "$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ) (SOURCE_DATE_EPOCH)"
else
  stamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
fi
{
  say "date-utc: $stamp"
  say "host: $(uname -sm)"
  say "rustc: $(rustc --version 2>/dev/null || echo 'not on PATH')"
  say "clang: $(clang --version | head -1)"
  say "commit: ${NAZM_SOURCE_COMMIT:-$(git -C "$root" rev-parse HEAD 2>/dev/null || echo "no git")}"
  say "tree-clean: ${NAZM_SOURCE_TREE_CLEAN:-$(test -z "$(git -C "$root" status --porcelain 2>/dev/null)" && echo yes || echo "unknown, no git here")}"
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
#
# ELF has the same problem with a different name: GNU ld and lld stamp a build-id, which
# `--build-id=none` removes. The flag has to be chosen per platform because each linker
# rejects the other's spelling, and the bootstrap now runs on Linux inside the contained
# runner as well as on macOS.
case "$(uname -s)" in
  Darwin) normalise_flag="-Wl,-no_uuid" ;;
  *)      normalise_flag="-Wl,--build-id=none" ;;
esac

link() {
  stage=$1
  clang -Wno-override-module -o "$stage/nazmc" "$stage/nazmc.ll" \
    || fail "$(basename "$stage") assembly failed"
  mkdir -p "$stage/normalised"
  clang -Wno-override-module "$normalise_flag" -o "$stage/normalised/nazmc" "$stage/nazmc.ll" \
    || fail "$(basename "$stage") normalised assembly failed"
}

S="$root/compiler/$root_source"

# One line per source the resolver will pull in, so the record still says exactly what was
# compiled. Each digest is of the file as it sits on disk: there is no merged file to take
# one of, because nothing merges them ahead of time any more.
total_lines=0
for f in $sources; do
  n=$(wc -l < "$root/compiler/$f" | tr -d ' ')
  say "source: $f $(sha256 "$root/compiler/$f") $n lines" >> "$out/record.txt"
  total_lines=$((total_lines + n))
done
say "source lines: $total_lines" >> "$out/record.txt"
say "core-prelude: library/core/prelude.nz $(sha256 "$core") $(wc -l < "$core" | tr -d ' ') lines" >> "$out/record.txt"

# A **reference** check, by the Rust front end, and not part of the chain. Nothing
# downstream depends on it; it is here so a bootstrap cannot succeed on a program the
# language does not accept. Stages 2 and 3 resolve and check for themselves, with the
# compiler written in Nazm.
"$nazm" check --no-cache "$S" >/dev/null || fail "the compiler source does not check under the reference front end"

# --- stage 1: Rust builds C1 --------------------------------------------------------
say "stage 1: the Rust implementation compiles the compiler source into C1"
# `--no-cache` on both: the bootstrap of record is the authority, and an
# authority may not depend on derived data that happened to be warm.
"$nazm" build --no-cache "$S" -o "$out/stage1/nazmc" >/dev/null || fail "stage 1 failed"
[ -x "$out/stage1/nazmc" ] || fail "stage 1 produced no executable"

# --- stage 2: C1 builds C2 ----------------------------------------------------------
say "stage 2: C1 compiles it into C2, resolving the imports itself"
"$out/stage1/nazmc" "$S" "$core" > "$out/stage2/nazmc.ll" || fail "stage 2 emission failed"
link "$out/stage2"

# --- stage 3: C2 builds C3 ----------------------------------------------------------
say "stage 3: C2 compiles it into C3, resolving the imports itself"
"$out/stage2/nazmc" "$S" "$core" > "$out/stage3/nazmc.ll" || fail "stage 3 emission failed"
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
say "c2-digest: $(sha256 "$out/stage2/normalised/nazmc")" >> "$out/record.txt"
say "c3-digest: $(sha256 "$out/stage3/normalised/nazmc")" >> "$out/record.txt"
say "c2-runnable-digest: $(sha256 "$out/stage2/nazmc")" >> "$out/record.txt"
say "runnable-bytes-differing: $(cmp -l "$out/stage2/nazmc" "$out/stage3/nazmc" | wc -l | tr -d ' ')" >> "$out/record.txt"

# --- conformance: both later stages compile the corpus and agree ---------------------
say "conformance: C2 and C3 compile the corpus"
corpus="$root/compiler/conformance"
checked=0
modules=0
for case in "$corpus"/*.nz; do
  [ -e "$case" ] || continue
  name=$(basename "$case" .nz)
  expected=$(cat "$corpus/$name.expected")
  for stage in stage2 stage3; do
    "$out/$stage/nazmc" "$case" "$core" > "$out/$stage/$name.ll" || fail "$stage refused $name"
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

# Multi-module cases, each a directory whose `main.nz` is the root. They exist because a
# single-file corpus cannot exercise the thing N2 settled: every compiler here resolves
# `use` for itself, so these compare **module semantics** — ownership, visibility,
# non-transitive imports, one identity for a diamond, and the root's `main` — across the
# reference, C2 and C3. That is a different claim from C2 = C3, which is a fixpoint and
# says nothing about whether either agrees with the reference about the language.
say "conformance: C2, C3 and the reference agree on module semantics"
for case in "$corpus"/modules/*/; do
  [ -d "$case" ] || continue
  name=$(basename "$case")
  expected=$(cat "$case/expected")
  for stage in stage2 stage3; do
    "$out/$stage/nazmc" "$case/main.nz" "$core" > "$out/$stage/$name.ll" \
      || fail "$stage refused module case $name"
    clang -Wno-override-module -o "$out/$stage/$name" "$out/$stage/$name.ll" \
      || fail "$stage emitted IR clang rejected for module case $name"
    got=$("$out/$stage/$name" 2>&1 || true)
    [ "$got" = "$expected" ] || fail "$stage: $name gave '$got', expected '$expected'"
  done
  # And the reference front end, which is what makes this a parity claim rather than a
  # self-consistency one.
  interp=$("$nazm" run "$case/main.nz" 2>&1 || true)
  [ "$interp" = "$expected" ] || fail "the reference disagrees about $name: '$interp'"
  checked=$((checked + 1))
  modules=$((modules + 1))
done

# Refusals, N11: programs every compiler must refuse, with the code the first line of
# each names — an ownership cycle through a `Vec`, an impossible inline layout through a
# generic record, conflicting inference, and an instance family that never ends. Agreement
# on what is accepted says nothing about agreement on what is not.
say "conformance: C2, C3 and the reference refuse the same programs"
refused=0
for case in "$corpus"/refused/*.nz; do
  [ -e "$case" ] || continue
  name=$(basename "$case" .nz)
  code=$(sed -n '1s#^// expect: ##p' "$case")
  [ -n "$code" ] || fail "refusal case $name names no code"
  for stage in stage2 stage3; do
    if "$out/$stage/nazmc" "$case" "$core" > "$out/$stage/$name.ll" 2> "$out/$stage/$name.err"; then
      fail "$stage accepted refusal case $name"
    fi
    grep -q "error\[$code\]" "$out/$stage/$name.err" || fail "$stage refused $name without $code"
  done
  if "$nazm" build "$case" -o "$out/$name.ref" > /dev/null 2> "$out/$name.ref.err"; then
    fail "the reference accepted refusal case $name"
  fi
  grep -q "error\[$code\]" "$out/$name.ref.err" || fail "the reference refused $name without $code"
  refused=$((refused + 1))
done

say "conformance-cases: $checked" >> "$out/record.txt"
say "module-conformance-cases: $modules" >> "$out/record.txt"
say "refusal-conformance-cases: $refused" >> "$out/record.txt"
[ "$refused" -gt 0 ] || fail "no refusal conformance cases were run"
[ "$checked" -gt 0 ] || fail "no conformance cases were run"
[ "$modules" -gt 0 ] || fail "no module conformance cases were run"

say ""
say "bootstrap complete: C2 and C3 are byte-identical, $checked conformance cases agree ($modules of them multi-module), $refused refusals agree"
say "record: $out/record.txt"
cat "$out/record.txt"
