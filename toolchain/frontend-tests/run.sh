#!/bin/sh
# R2's comparisons (`openspec/changes/r2-front-end/design.md`): the front end written in Nazm
# against the Rust toolchain, on every input.
#
#   toolchain/frontend-tests/run.sh [MODE...]      MODE: tokens, tree, syntax, check (default: the first three)
#
# Each driver is built by the reference `nazm` ($NAZM) with a heap limit (design D6) and run
# under a deadline; the reference side is `toolchain/frontend-tests/refdump`, built from this checkout's
# `nazm-syntax`. Prints one line per mode, `MODE: N agree, M differ`, and writes every
# difference to $OUT/MODE/NAME.diff (R2_KEEP=1 keeps both sides of every agreement too). Exits 1
# when anything differs.
#
# Inputs (design D8): tests/compat/v1, corpus, examples, the compiler's own sources, the
# conformance and parity corpora, the hand-written cases in toolchain/frontend-tests/inputs/, and the
# programs extracted from crates/nazm-cli/tests (toolchain/frontend-tests/inputs/rs).

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
nazm=${NAZM:-"$HOME/.local/nazm-0.10.0/bin/nazm"}
out=${OUT:-"$root/target/r2"}
ref_target=${R2_REF_TARGET:-"$HOME/.cache/nazm-target-r2"}
heap=${R2_HEAP_LIMIT:-2G}
deadline=${R2_DEADLINE:-20}
jobs=${R2_JOBS:-6}

[ -x "$nazm" ] || { echo "run.sh: no reference nazm at $nazm (set NAZM)" >&2; exit 2; }
modes=${*:-tokens tree syntax}

mkdir -p "$out/bin"

# The reference dumper, built from this checkout.
(cd "$root/toolchain/frontend-tests/refdump" && CARGO_TARGET_DIR="$ref_target" cargo build --release --quiet) \
  || { echo "run.sh: the reference dumper did not build" >&2; exit 2; }
ref="$ref_target/release/nazm-r2-refdump"

# The inputs, one path per line, sorted, relative to the root.
list="$out/inputs.txt"
(
  cd "$root"
  find tests/compat/v1 corpus examples compiler/conformance compiler/parity toolchain/frontend-tests/inputs \
    crates/nazm-cli/tests crates/nazm-cli/templates \
    -name '*.nz' -type f 2>/dev/null
  ls compiler/*.nz
) | LC_ALL=C sort > "$list"

for mode in $modes; do
  case $mode in
    tokens) driver=tokens ;;
    tree) driver=tree ;;
    # Diagnostics as `nazm check --json` objects (design D4): `syntax` compares the lexer's and the
    # parser's alone, `check` every one — which needs the checker, slice by slice.
    syntax|check) driver=diagnose ;;
    *) echo "run.sh: unknown mode $mode" >&2; exit 2 ;;
  esac
  exe="$out/bin/$driver"
  # The parser recurses with the program's nesting, as the reference's does on its 24 MiB stack.
  "$nazm" build --heap-limit "$heap" --stack-size 256M "$root/compiler/$driver.nz" -o "$exe" >/dev/null \
    || { echo "run.sh: compiler/$driver.nz did not build" >&2; exit 2; }
  rm -rf "$out/$mode"
  mkdir -p "$out/$mode"
  # One worker per input: compare, and leave a `.diff` behind when the two disagree.
  export R2_ROOT="$root" R2_REF="$ref" R2_EXE="$exe" R2_MODE="$mode" R2_DIR="$out/$mode" R2_DEADLINE="$deadline" R2_NAZM="$nazm"
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    want="$R2_DIR/$name.want"; got="$R2_DIR/$name.got"
    case $R2_MODE in
      tokens|tree)
        "$R2_REF" "$R2_MODE" "$R2_ROOT/$f" > "$want" 2>&1 || true
        timeout "$R2_DEADLINE" "$R2_EXE" "$R2_ROOT/$f" > "$got" 2>&1 || true ;;
      check)
        { "$R2_NAZM" check --json --no-cache "$R2_ROOT/$f" 2>&1 >/dev/null || true; } | grep "^{" > "$want" || true
        # Standard error says where the checker stopped (design D13), kept beside the result.
        timeout "$R2_DEADLINE" "$R2_EXE" "$R2_ROOT/$f" "$R2_ROOT/library" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      syntax)
        "$R2_REF" syntax "$R2_ROOT/$f" > "$want" 2>&1 || true
        timeout "$R2_DEADLINE" "$R2_EXE" --syntax "$R2_ROOT/$f" > "$got" 2>&1 || true ;;
    esac
    if cmp -s "$want" "$got"; then
      [ -n "${R2_KEEP:-}" ] || rm -f "$want" "$got"
      : > "$R2_DIR/$name.ok"
    else
      diff "$want" "$got" > "$R2_DIR/$name.diff" 2>&1 || true
    fi
  ' sh
  agree=$(find "$out/$mode" -name '*.ok' | wc -l | tr -d ' ')
  differ=$(find "$out/$mode" -name '*.diff' | wc -l | tr -d ' ')
  if [ "$mode" = check ]; then
    # The inputs the checker carries whole: no `.stop` left behind; among them, a difference is a
    # defect rather than a gap.
    carried=0; carried_ok=0
    for ok in "$out/$mode"/*.ok "$out/$mode"/*.diff; do
      [ -e "$ok" ] || continue
      base=${ok%.*}
      [ -e "$base.stop" ] && continue
      carried=$((carried + 1))
      case $ok in *.ok) carried_ok=$((carried_ok + 1)) ;; esac
    done
    echo "$mode: $agree agree, $differ differ; the checker carries $carried, of which $carried_ok agree"
  else
    echo "$mode: $agree agree, $differ differ"
  fi
  [ "$differ" -eq 0 ] || status=1
done
exit ${status:-0}
