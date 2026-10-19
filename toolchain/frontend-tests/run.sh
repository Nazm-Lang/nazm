#!/bin/sh
# R2's comparisons (`openspec/changes/r2-front-end/design.md`): the front end written in Nazm
# against the Rust toolchain, on every input.
#
#   toolchain/frontend-tests/run.sh [MODE...]      MODE: tokens, tree, syntax, check, render, facts,
#                                                  core, keys, units, refs, calls, provenance, coredigests, mir (default: the first three)
#
# `render` compares the diagnostics' text, as `nazm check` writes them (design D4, task 4.2), and
# `facts` what `nazm doc` reads of each compilation with what `compiler/facts.nz` answers (D24),
# and `core` each function's Core IR with `nazm core-ir`'s, digests included (D25): an input agrees
# when every function the Nazm lowering carries agrees, and the summary counts functions too.
#
# Each driver is built by the reference `nazm` ($NAZM) with a heap limit (design D6) and run
# under a deadline; the reference side is `toolchain/frontend-tests/refdump`, built from this checkout's
# `nazm-syntax`. Prints one line per mode, `MODE: N agree, M differ`, and writes every
# difference to $OUT/MODE/NAME.diff (R2_KEEP=1 keeps both sides of every agreement too). Exits 1
# when anything differs.
#
# Inputs (design D8): tests/compat/v1, corpus, examples, the compiler's own sources, the
# conformance and parity corpora, the hand-written cases in toolchain/frontend-tests/inputs/,
# the programs extracted from crates/nazm-cli/tests (toolchain/frontend-tests/inputs/rs), and
# every other track's Nazm sources under toolchain/.

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

# The host, as Rust's `std::env::consts` names it: what `@cfg` asks of (design D19).
case $(uname -s) in Darwin) host_os=macos ;; Linux) host_os=linux ;; *) host_os=$(uname -s | tr 'A-Z' 'a-z') ;; esac
case $(uname -m) in arm64|aarch64) host_arch=aarch64 ;; *) host_arch=$(uname -m) ;; esac

# The reference dumper, built from this checkout.
(cd "$root/toolchain/frontend-tests/refdump" && CARGO_TARGET_DIR="$ref_target" cargo build --release --quiet) \
  || { echo "run.sh: the reference dumper did not build" >&2; exit 2; }
ref="$ref_target/release/nazm-r2-refdump"

# The inputs, one path per line, sorted, relative to the root.
list="$out/inputs.txt"
(
  cd "$root"
  find tests/compat/v1 corpus examples compiler/conformance compiler/parity toolchain \
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
    syntax|check|render|facts|core|keys|units|refs|calls|provenance|coredigests|mir) driver=diagnose ;;
    *) echo "run.sh: unknown mode $mode" >&2; exit 2 ;;
  esac
  exe="$out/bin/$driver"
  # The parser recurses with the program's nesting, as the reference's does on its 24 MiB stack.
  "$nazm" build --heap-limit "$heap" --stack-size 256M "$root/compiler/$driver.nz" -o "$exe" >/dev/null \
    || { echo "run.sh: compiler/$driver.nz did not build" >&2; exit 2; }
  rm -rf "$out/$mode"
  mkdir -p "$out/$mode"
  # The R3 pipeline, which lowers MIR from the Core IR document (the R5 hand-back, fact 4).
  if [ "$mode" = mir ]; then
    "$nazm" build --heap-limit "$heap" --stack-size 256M "$root/toolchain/pipeline/main.nz" -o "$out/bin/pipeline" >/dev/null 2>&1 \
      || { echo "run.sh: toolchain/pipeline/main.nz did not build" >&2; exit 2; }
  fi
  # One worker per input: compare, and leave a `.diff` behind when the two disagree.
  export R2_ROOT="$root" R2_REF="$ref" R2_EXE="$exe" R2_MODE="$mode" R2_DIR="$out/$mode" R2_DEADLINE="$deadline" R2_NAZM="$nazm" \
    R2_OS="$host_os" R2_ARCH="$host_arch" R2_PIPELINE="$out/bin/pipeline"
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    want="$R2_DIR/$name.want"; got="$R2_DIR/$name.got"
    # The reference lexes text (`&str`): a file that is not UTF-8 has no tokens or tree to compare,
    # only its refusal by the loader, which `check` compares.
    case $R2_MODE in
      tokens|tree|syntax)
        if ! iconv -f UTF-8 -t UTF-8 "$R2_ROOT/$f" >/dev/null 2>&1; then : > "$R2_DIR/$name.skip"; exit 0; fi ;;
    esac
    case $R2_MODE in
      tokens|tree)
        "$R2_REF" "$R2_MODE" "$R2_ROOT/$f" > "$want" 2>&1 || true
        timeout "$R2_DEADLINE" "$R2_EXE" "$R2_ROOT/$f" > "$got" 2>&1 || true ;;
      check)
        { "$R2_NAZM" check --json --no-cache "$R2_ROOT/$f" 2>&1 >/dev/null || true; } | grep "^{" > "$want" || true
        # Standard error says where the checker stopped (design D13), kept beside the result.
        timeout "$R2_DEADLINE" "$R2_EXE" "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      render)
        # The standard error of `nazm check`, without its summary line.
        { "$R2_NAZM" check --no-cache "$R2_ROOT/$f" 2>&1 >/dev/null || true; } | grep -Ev "^nazm: [0-9]+ (error|warning)" > "$want" || true
        timeout "$R2_DEADLINE" "$R2_EXE" --text "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      facts)
        status=0
        "$R2_NAZM" doc "$R2_ROOT/$f" > "$want.json" 2>/dev/null || status=$?
        python3 "$R2_ROOT/toolchain/frontend-tests/doc-facts.py" "$status" < "$want.json" > "$want"
        rm -f "$want.json"
        timeout "$R2_DEADLINE" "$R2_EXE" --facts "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      core)
        status=0
        "$R2_NAZM" core-ir "$R2_ROOT/$f" > "$want.json" 2>/dev/null || status=$?
        python3 "$R2_ROOT/toolchain/frontend-tests/core-ir.py" want "$status" < "$want.json" > "$want"
        timeout "$R2_DEADLINE" "$R2_EXE" --core "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop"
        # Function by function and part by part: an input agrees when every one carried does,
        # and a document printed whole is what the reference printed, byte for byte.
        if python3 "$R2_ROOT/toolchain/frontend-tests/core-ir.py" compare "$want" "$got" "$want.json" > "$R2_DIR/$name.count"; then
          [ -n "${R2_KEEP:-}" ] || rm -f "$want" "$got" "$want.json"
          : > "$R2_DIR/$name.ok"
        else
          diff "$want" "$got" > "$R2_DIR/$name.diff" 2>&1 || true
        fi
        exit 0 ;;
      units)
        # Core IR from units filed per module under its check key (design D28): lowered and
        # filed on a first run, read back on a second, which is the run compared with the
        # reference as `core` compares; then the units are removed.
        status=0
        "$R2_NAZM" core-ir "$R2_ROOT/$f" > "$want.json" 2>/dev/null || status=$?
        python3 "$R2_ROOT/toolchain/frontend-tests/core-ir.py" want "$status" < "$want.json" > "$want"
        units="$R2_DIR/$name.units"
        rm -rf "$units"
        mkdir -p "$units"
        timeout "$R2_DEADLINE" "$R2_EXE" --units "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" "$units" > /dev/null 2>&1 || true
        timeout "$R2_DEADLINE" "$R2_EXE" --units "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" "$units" > "$got" 2> "$R2_DIR/$name.reuse" || true
        rm -rf "$units"
        if python3 "$R2_ROOT/toolchain/frontend-tests/core-ir.py" compare "$want" "$got" "$want.json" > "$R2_DIR/$name.count"; then
          [ -n "${R2_KEEP:-}" ] || rm -f "$want" "$got" "$want.json"
          : > "$R2_DIR/$name.ok"
        else
          diff "$want" "$got" > "$R2_DIR/$name.diff" 2>&1 || true
        fi
        exit 0 ;;
      keys)
        # The keys of the cache (design D28): for each module its key, source fingerprint,
        # published interface, interface hash and check key, as the reference computes them.
        "$R2_REF" keys "$R2_ROOT/$f" > "$want" 2>/dev/null || true
        timeout "$R2_DEADLINE" "$R2_EXE" --keys "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      refs)
        # The reference index (R5 hand-back): every entity with its declaration and its uses, as
        # the reference index of the Rust side holds them.
        "$R2_REF" refs "$R2_ROOT/$f" > "$want" 2>/dev/null || true
        timeout "$R2_DEADLINE" "$R2_EXE" --refs "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      calls)
        # The call graph (R5 hand-back): the calls of each function, as the Rust side recorded them.
        "$R2_REF" calls "$R2_ROOT/$f" > "$want" 2>/dev/null || true
        timeout "$R2_DEADLINE" "$R2_EXE" --calls "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      provenance)
        # The provenance summaries (R5 hand-back): what each function result carries, passes on and
        # reaches, and what its parameters are handed, as the Rust side solved them.
        "$R2_REF" provenance "$R2_ROOT/$f" > "$want" 2>/dev/null || true
        timeout "$R2_DEADLINE" "$R2_EXE" --provenance "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      coredigests)
        # The Core IR digests of each function (R5 hand-back), as inspect reports them.
        "$R2_REF" coredigests "$R2_ROOT/$f" > "$want" 2>/dev/null || true
        timeout "$R2_DEADLINE" "$R2_EXE" --coredigests "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$got" 2> "$R2_DIR/$name.stop" || true
        [ -s "$R2_DIR/$name.stop" ] || rm -f "$R2_DIR/$name.stop" ;;
      mir)
        # The native digests (R5 hand-back): the MIR the R3 pipeline lowers, with the digest of each
        # function, from the Core IR document the Rust toolchain prints and from the one the Nazm front
        # end publishes: the front end is the difference. `.exact` marks an input whose MIR is also
        # `nazm mir` itself, byte for byte (the pipeline is R3, and not everything it lowers is).
        if ! "$R2_NAZM" core-ir --json "$R2_ROOT/$f" > "$R2_DIR/$name.rust.json" 2>/dev/null; then : > "$R2_DIR/$name.skip"; rm -f "$R2_DIR/$name.rust.json"; exit 0; fi
        timeout "$R2_DEADLINE" "$R2_EXE" --coredoc "$R2_ROOT/$f" "$R2_ROOT/library" "$R2_OS" "$R2_ARCH" > "$R2_DIR/$name.json" 2> /dev/null || true
        if [ ! -s "$R2_DIR/$name.json" ]; then : > "$R2_DIR/$name.skip"; rm -f "$R2_DIR/$name.rust.json" "$R2_DIR/$name.json"; exit 0; fi
        timeout 60 "$R2_PIPELINE" mir "$R2_DIR/$name.rust.json" > "$want" 2> /dev/null || true
        timeout 60 "$R2_PIPELINE" mir "$R2_DIR/$name.json" > "$got" 2> "$R2_DIR/$name.stop" || true
        if "$R2_NAZM" mir "$R2_ROOT/$f" 2> /dev/null | cmp -s - "$want"; then : > "$R2_DIR/$name.exact"; fi
        rm -f "$R2_DIR/$name.json" "$R2_DIR/$name.rust.json"
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
  if [ "$mode" = units ]; then
    # How many units the second runs reused and wrote: every one reused where nothing changed.
    reuse=$(cat "$out/$mode"/*.reuse 2>/dev/null | awk '/^units:/ { r += $2; w += $4 } END { printf "%d reused, %d written", r, w }')
    counts=$(cat "$out/$mode"/*.count 2>/dev/null | awk '{ t += $1; c += $2; a += $3; d += $6; e += $7 } END { printf "%d %d %d %d %d", t, c, a, d, e }')
    set -- $counts
    echo "$mode: $agree agree, $differ differ; functions: $1, carried $2, of which $3 agree; whole documents: $4, of which $5 agree byte for byte; on the second run, units $reuse"
  elif [ "$mode" = core ]; then
    # Functions: the reference's, those the lowering carried, those of them that agree.
    counts=$(cat "$out/$mode"/*.count 2>/dev/null | awk '{ t += $1; c += $2; a += $3; p += $4; q += $5; d += $6; e += $7 } END { printf "%d %d %d %d %d %d %d", t, c, a, p, q, d, e }')
    set -- $counts
    echo "$mode: $agree agree, $differ differ; functions: $1, the lowering carries $2, of which $3 agree; the document's other parts: $4 carried, of which $5 agree; whole documents: $6 carried, of which $7 agree byte for byte"
  elif [ "$mode" = mir ]; then
    # Inputs with no MIR to compare (a program that does not check, one the Core IR document does
    # not carry whole) are not this comparison's.
    skipped=$(find "$out/$mode" -name '*.skip' | wc -l | tr -d ' ')
    exact=$(find "$out/$mode" -name '*.exact' | wc -l | tr -d ' ')
    echo "$mode: $agree agree, $differ differ ($skipped without MIR or a whole Core IR document); of the agreements $exact are also nazm mir itself"
  elif [ "$mode" = check ] || [ "$mode" = render ] || [ "$mode" = facts ] || [ "$mode" = refs ] || [ "$mode" = calls ] || [ "$mode" = provenance ] || [ "$mode" = coredigests ]; then
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
    skipped=$(find "$out/$mode" -name '*.skip' | wc -l | tr -d ' ')
    if [ "$skipped" -gt 0 ]; then
      echo "$mode: $agree agree, $differ differ ($skipped not UTF-8, skipped)"
    else
      echo "$mode: $agree agree, $differ differ"
    fi
  fi
  [ "$differ" -eq 0 ] || status=1
  # Growth (performance leadership 7.4): the document printed for a function nested 200 deep is
  # its size times its depth, but the work of printing it is no longer a part per indentation.
  if [ "$mode" = core ]; then
    python3 "$root/bench/complexity/growth.py" "Core IR (diagnose --core)" nest 25 5 src "$nazm" "$out/growth" -- \
      "$exe" --core "{in}" "$root/library" "$host_os" "$host_arch" || status=1
  fi
done
exit ${status:-0}
