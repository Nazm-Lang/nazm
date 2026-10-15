#!/bin/sh
# R5's comparisons (`openspec/changes/r5-commands/design.md`): each command written in Nazm against
# the Rust `nazm` built from this checkout, on every input.
#
#   toolchain/commands/tests/run.sh [-j N] [FILE.nz ...]
#
# `fmt` (toolchain/commands/fmt.nz) is built by the 0.10.0 release ($NAZM). The reference is
# `nazm` built from this checkout into $CARGO_TARGET_DIR (default ~/.cache/nazm-target-track5), or
# $NAZM_REF when set. For every input — the repository's tracked `.nz` files, which include
# toolchain/frontend-tests/inputs — a copy is formatted by each side, twice: `fmt --check FILE`, and
# `fmt FILE`, which rewrites it. Standard output, standard error, the exit status and the file's
# bytes afterwards must agree byte for byte; then one `fmt --check` over every input at once. A
# refusal (exit 6: something the port does not carry) is counted apart and is never an agreement.
#
# Prints one line, `fmt: N agree, M differ, R refused (of T inputs); batch agree|differ`, after the
# names of the first differences. Exits 1 when anything differs or is refused. Leaves nothing
# behind unless KEEP=DIR is given, where every difference's two records are kept.

set -eu

jobs=4
if [ "${1:-}" = "-j" ]; then jobs=$2; shift 2; fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
nazm=${NAZM:-"$HOME/.local/nazm-0.10.0/bin/nazm"}
[ -x "$nazm" ] || { echo "run.sh: no nazm at $nazm (set NAZM)" >&2; exit 2; }

work=$(mktemp -d "${TMPDIR:-/tmp}/r5-commands.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

# The reference, built from this checkout.
if [ -n "${NAZM_REF:-}" ]; then
  ref=$NAZM_REF
else
  target=${CARGO_TARGET_DIR:-"$HOME/.cache/nazm-target-track5"}
  (cd "$root" && CARGO_TARGET_DIR="$target" cargo build --release --quiet -p nazm-cli -j 4) \
    || { echo "run.sh: the reference nazm did not build" >&2; exit 2; }
  ref="$target/release/nazm"
fi

"$nazm" build "$root/toolchain/commands/fmt.nz" -o "$work/fmt" > "$work/build.log" 2>&1 \
  || { cat "$work/build.log" >&2; echo "run.sh: toolchain/commands/fmt.nz did not build" >&2; exit 2; }

list="$work/inputs.txt"
if [ $# -gt 0 ]; then
  printf '%s\n' "$@" > "$list"
else
  (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
fi
total=$(wc -l < "$list" | tr -d ' ')

mkdir -p "$work/res" "$work/batch/ref" "$work/batch/got"
export R5_ROOT="$root" R5_REF="$ref" R5_FMT="$work/fmt" R5_WORK="$work"

# One input: a record per side — each run's exit status, output and error, and the file after the
# rewrite — compared byte for byte. Writes `agree`, `differ` or `refused` to res/NAME.
# shellcheck disable=SC2016
tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
  f=$1
  name=$(printf "%s" "$f" | tr "/" "_")
  d="$R5_WORK/$name"
  mkdir -p "$d/ref" "$d/got"
  side() {
    dir=$1; shift
    cp "$R5_ROOT/$f" "$dir/case.nz"
    (cd "$dir" && { "$@" --check case.nz > check.out 2> check.err; echo "check exit $?"; } > record)
    (cd "$dir" && { "$@" case.nz > fmt.out 2> fmt.err; echo "fmt exit $?"; } >> record)
    (cd "$dir" && cat check.out check.err fmt.out fmt.err case.nz >> record)
  }
  side "$d/ref" "$R5_REF" fmt
  side "$d/got" "$R5_FMT"
  if grep -q "exit 6$" "$d/got/record"; then
    verdict=refused
  elif cmp -s "$d/ref/record" "$d/got/record"; then
    verdict=agree
  else
    verdict=differ
  fi
  echo "$verdict" > "$R5_WORK/res/$name"
  if [ "$verdict" != agree ] && [ -n "${KEEP:-}" ]; then
    mkdir -p "$KEEP/$name" && cp "$d/ref/record" "$KEEP/$name/want" && cp "$d/got/record" "$KEEP/$name/got"
  fi
  if [ "$verdict" = agree ]; then rm -rf "$d"; else cp "$R5_ROOT/$f" "$d/source.nz"; fi
' sh

agree=$(grep -l '^agree$' "$work/res/"* 2>/dev/null | wc -l | tr -d ' ')
differ=$(grep -l '^differ$' "$work/res/"* 2>/dev/null | wc -l | tr -d ' ')
refused=$(grep -l '^refused$' "$work/res/"* 2>/dev/null | wc -l | tr -d ' ')
for r in $(grep -l -v '^agree$' "$work/res/"* 2>/dev/null | head -20); do
  echo "  $(cat "$r"): $(basename "$r")"
done

# Every input at once: the multi-file command's listing, its summary and its exit status.
i=0
while IFS= read -r f; do
  i=$((i + 1))
  cp "$root/$f" "$work/batch/ref/$i.nz"
  cp "$root/$f" "$work/batch/got/$i.nz"
done < "$list"
args=$(i=0; while [ $i -lt "$total" ]; do i=$((i + 1)); printf '%s.nz ' "$i"; done)
# shellcheck disable=SC2086
(cd "$work/batch/ref" && { rc=0; "$ref" fmt --check $args > ../ref.out 2> ../ref.err || rc=$?; echo "exit $rc" >> ../ref.out; })
# shellcheck disable=SC2086
(cd "$work/batch/got" && { rc=0; "$work/fmt" --check $args > ../got.out 2> ../got.err || rc=$?; echo "exit $rc" >> ../got.out; })
if cmp -s "$work/batch/ref.out" "$work/batch/got.out" && cmp -s "$work/batch/ref.err" "$work/batch/got.err"; then batch=agree; else batch=differ; fi

# A file that is not there, after one that is rewritten: the command stops at it, as the
# reference does, with the first file already written.
for side in ref got; do
  mkdir -p "$work/missing/$side"
  cp "$root/toolchain/commands/tests/inputs/unicode-space.nz" "$work/missing/$side/a.nz"
done
(cd "$work/missing/ref" && { rc=0; "$ref" fmt a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
(cd "$work/missing/got" && { rc=0; "$work/fmt" a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
if cmp -s "$work/missing/ref/out" "$work/missing/got/out" && cmp -s "$work/missing/ref/err" "$work/missing/got/err"; then :; else batch=differ; fi

echo "fmt: $agree agree, $differ differ, $refused refused (of $total inputs); batch $batch"
[ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] && [ "$batch" = agree ]
