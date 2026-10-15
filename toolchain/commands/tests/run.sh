#!/bin/sh
# R5's comparisons (`openspec/changes/r5-commands/design.md`): each command written in Nazm against
# the Rust `nazm` built from this checkout, on every input.
#
#   toolchain/commands/tests/run.sh [-j N] [PART...] [FILE.nz ...]     PART: fmt, args, init, check (all by default)
#
# Each command (toolchain/commands/CMD.nz) is built by the 0.10.0 release ($NAZM). The reference is
# `nazm` built from this checkout into $CARGO_TARGET_DIR (default ~/.cache/nazm-target-track5), or
# $NAZM_REF when set. Every comparison is of the whole observable behaviour — standard output,
# standard error, the exit status and every file afterwards — byte for byte. A refusal (exit 6:
# something the port does not carry) is counted apart and is never an agreement.
#
#   fmt   For every input — the repository's tracked `.nz` files, which include
#         toolchain/frontend-tests/inputs, or the FILEs named — a copy is formatted by each side
#         twice: `fmt --check FILE`, then `fmt FILE`, which rewrites it. Then one `fmt --check`
#         over every input at once, and a missing file named after one that is rewritten.
#   args  Every command line in tests/args/CMD.txt, run by both sides in the same fixture
#         directory: clap's usage errors and help, and what the command then does (design, D8).
#   init  `nazm init` of every template, the trees written compared file by file (design, D9).
#   check For every input (as for fmt, or the FILEs named), `check --no-cache FILE` and
#         `check --json --no-cache FILE` from the repository's root, and `--deny-warnings` too where
#         the reference warns (design, D11). The standard library is the checkout's (NAZM_LIBRARY);
#         the reference's check cache goes to a scratch directory (NAZM_CACHE_DIR).
#
# Prints one summary line per part, after the names of the first differences. Exits 1 when
# anything differs or is refused. Leaves nothing behind unless KEEP=DIR is given, where every
# difference's two records are kept.

set -eu

jobs=4
if [ "${1:-}" = "-j" ]; then jobs=$2; shift 2; fi
parts=""
while [ $# -gt 0 ]; do
  case $1 in
    fmt|args|init|check) parts="$parts $1"; shift ;;
    *) break ;;
  esac
done
[ -n "$parts" ] || parts="fmt args init check"

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

mkdir -p "$work/bin" "$work/cache"
# The checker recurses with the program's nesting, as the reference's does on its 24 MiB stack.
export NAZM_LIBRARY="$root/library" NAZM_CACHE_DIR="$work/cache"
for cmd in fmt init check; do
  "$nazm" build --heap-limit 2G --stack-size 256M "$root/toolchain/commands/$cmd.nz" -o "$work/bin/$cmd" > "$work/build.log" 2>&1 \
    || { cat "$work/build.log" >&2; echo "run.sh: toolchain/commands/$cmd.nz did not build" >&2; exit 2; }
done

status=0

# --- fmt: every input -----------------------------------------------------------------------------

part_fmt() {
  list="$work/inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')

  mkdir -p "$work/res" "$work/batch/ref" "$work/batch/got"
  export R5_ROOT="$root" R5_REF="$ref" R5_FMT="$work/bin/fmt" R5_WORK="$work"

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
  (cd "$work/batch/got" && { rc=0; "$work/bin/fmt" --check $args > ../got.out 2> ../got.err || rc=$?; echo "exit $rc" >> ../got.out; })
  if cmp -s "$work/batch/ref.out" "$work/batch/got.out" && cmp -s "$work/batch/ref.err" "$work/batch/got.err"; then batch=agree; else batch=differ; fi

  # A file that is not there, after one that is rewritten: the command stops at it, as the
  # reference does, with the first file already written.
  for side in ref got; do
    mkdir -p "$work/missing/$side"
    cp "$root/toolchain/commands/tests/inputs/unicode-space.nz" "$work/missing/$side/a.nz"
  done
  (cd "$work/missing/ref" && { rc=0; "$ref" fmt a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
  (cd "$work/missing/got" && { rc=0; "$work/bin/fmt" a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
  if cmp -s "$work/missing/ref/out" "$work/missing/got/out" && cmp -s "$work/missing/ref/err" "$work/missing/got/err"; then :; else batch=differ; fi

  echo "fmt: $agree agree, $differ differ, $refused refused (of $total inputs); batch $batch"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] && [ "$batch" = agree ] || status=1
}

# --- one run of a command, recorded ---------------------------------------------------------------

# The fixture a command line runs in: what each command's lines name.
fixture() {
  cmd=$1 dir=$2
  mkdir -p "$dir"
  case $cmd in
    fmt)
      printf 'fn main() -> Int {\n    0\n}\n' > "$dir/a.nz"
      printf 'fn main() -> Int { 1+2 }\n' > "$dir/b.nz"
      mkdir -p "$dir/sub"
      ;;
    check)
      printf 'fn main() -> Int {\n    0\n}\n' > "$dir/a.nz"
      printf 'fn main() -> Int {\n    let x: Bool = 1;\n    x\n}\n' > "$dir/b.nz"
      printf 'enum E { A, B, C }\nfn main() -> Int {\n    match E.A {\n        E.A => 0,\n        _ => 1,\n    }\n}\n' > "$dir/w.nz"
      printf 'use "gone.nz";\nfn main() -> Int { 0 }\n' > "$dir/m.nz"
      printf 'fn main() -> Int { 1 + }\n' > "$dir/s.nz"
      mkdir -p "$dir/sub"
      ;;
    init)
      mkdir -p "$dir/empty" "$dir/full/sub"
      printf 'kept\n' > "$dir/full/keep.txt"
      printf 'a file\n' > "$dir/file"
      ;;
  esac
}

# Everything a run left in its directory: each path, and each file's bytes.
tree_record() {
  (cd "$1" && find . -mindepth 1 | LC_ALL=C sort | while IFS= read -r p; do
    if [ -d "$p" ]; then echo "dir $p"; else echo "file $p"; cat "$p"; echo "[end]"; fi
  done)
}

# `record DIR CMD -- ARGS...`: run in DIR, then the status, output, error and tree, to DIR.record.
record() {
  dir=$1; shift
  (cd "$dir" && { rc=0; "$@" > "$dir.out" 2> "$dir.err" || rc=$?; echo "exit $rc" > "$dir.record"; })
  { echo "[stdout]"; cat "$dir.out"; echo "[stderr]"; cat "$dir.err"; echo "[tree]"; tree_record "$dir"; } >> "$dir.record"
}

# --- args: every command line of every ported command ---------------------------------------------

part_args() {
  n=0 agree=0 differ=0 refused=0
  for cmd in fmt init check; do
    file="$root/toolchain/commands/tests/args/$cmd.txt"
    [ -f "$file" ] || continue
    while IFS= read -r line; do
      case $line in '#'*|'') continue ;; esac
      n=$((n + 1))
      d="$work/args/$n"
      fixture "$cmd" "$d/ref"
      fixture "$cmd" "$d/got"
      eval "set -- $line"
      record "$d/ref" "$ref" "$cmd" "$@"
      record "$d/got" "$work/bin/$cmd" "$@"
      if grep -q '^exit 6$' "$d/got.record"; then
        refused=$((refused + 1)); echo "  refused: $cmd $line"
      elif cmp -s "$d/ref.record" "$d/got.record"; then
        agree=$((agree + 1))
      else
        differ=$((differ + 1)); echo "  differ: $cmd $line"
        if [ -n "${KEEP:-}" ]; then
          mkdir -p "$KEEP/args-$n" && cp "$d/ref.record" "$KEEP/args-$n/want" && cp "$d/got.record" "$KEEP/args-$n/got"
        fi
      fi
      rm -rf "$d"
    done < "$file"
  done
  echo "args: $agree agree, $differ differ, $refused refused (of $n command lines)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- init: every template ---------------------------------------------------------------------------

part_init() {
  n=0 agree=0 differ=0 refused=0
  # The templates compiled into init.nz are the reference's.
  "$root/toolchain/commands/tests/gen-templates.sh" > "$work/init_templates.nz"
  if cmp -s "$work/init_templates.nz" "$root/toolchain/commands/init_templates.nz"; then templates=current; else
    templates=stale
    status=1
    echo "  differ: toolchain/commands/init_templates.nz is not tests/gen-templates.sh's output"
  fi
  for t in cli library server embedded wasm contract; do
    for where in "app" "nested/deeper/app" "empty" "./app2/"; do
      n=$((n + 1))
      d="$work/init/$n"
      fixture init "$d/ref"
      fixture init "$d/got"
      record "$d/ref" "$ref" init "$where" --template "$t"
      record "$d/got" "$work/bin/init" "$where" --template "$t"
      if grep -q '^exit 6$' "$d/got.record"; then
        refused=$((refused + 1)); echo "  refused: init $where --template $t"
      elif cmp -s "$d/ref.record" "$d/got.record"; then
        agree=$((agree + 1))
      else
        differ=$((differ + 1)); echo "  differ: init $where --template $t"
        if [ -n "${KEEP:-}" ]; then
          mkdir -p "$KEEP/init-$n" && cp "$d/ref.record" "$KEEP/init-$n/want" && cp "$d/got.record" "$KEEP/init-$n/got"
        fi
      fi
      rm -rf "$d"
    done
  done
  echo "init: $agree agree, $differ differ, $refused refused (of $n projects); templates $templates"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- check: every input ------------------------------------------------------------------------------

part_check() {
  list="$work/check-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/cres"
  export R5_ROOT="$root" R5_REF="$ref" R5_CHECK="$work/bin/check" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # One input: each side's runs recorded — status, output, error — and compared byte for byte.
  # Writes `agree`, `differ`, `refused` or `timeout`, then the refusal's reason, to cres/NAME.
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/c-$name"
    mkdir -p "$d"
    run() {
      out=$1; shift
      (cd "$R5_ROOT" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; cat "$out.o" "$out.e" >> "$out"; })
    }
    run "$d/ref.text" "$R5_REF" check --no-cache "$f"
    run "$d/ref.json" "$R5_REF" check --json --no-cache "$f"
    run "$d/got.text" timeout "$R5_DEADLINE" "$R5_CHECK" --no-cache "$f"
    run "$d/got.json" timeout "$R5_DEADLINE" "$R5_CHECK" --json --no-cache "$f"
    kinds="text json"
    if grep -q "^warning\[" "$d/ref.text.e"; then
      run "$d/ref.deny" "$R5_REF" check --no-cache --deny-warnings "$f"
      run "$d/got.deny" timeout "$R5_DEADLINE" "$R5_CHECK" --no-cache --deny-warnings "$f"
      kinds="$kinds deny"
    fi
    verdict=agree
    why=""
    for k in $kinds; do
      if grep -q "^exit 6$" "$d/got.$k"; then verdict=refused; why=$(cat "$d/got.$k.e"); break; fi
      if grep -q "^exit 124$" "$d/got.$k"; then verdict=timeout; break; fi
      cmp -s "$d/ref.$k" "$d/got.$k" || verdict=differ
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/cres/$name"
    if [ "$verdict" = differ ] || [ "$verdict" = timeout ]; then
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/check-$name" && for k in $kinds; do cp "$d/ref.$k" "$KEEP/check-$name/want.$k"; cp "$d/got.$k" "$KEEP/check-$name/got.$k"; done
      fi
    fi
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/cres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/cres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/cres/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/cres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/cres/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  # Why the port refused, most frequent first.
  for r in "$work/cres/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^check: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "check: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total inputs)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

for p in $parts; do
  case $p in
    fmt) part_fmt "$@" ;;
    args) part_args ;;
    init) part_init ;;
    check) part_check "$@" ;;
  esac
done
exit $status
