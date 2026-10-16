#!/usr/bin/env bash
# Run the toolchain itself on a real Windows host (W1): `nazm.exe`, built here for
# x86_64-pc-windows-gnu, interprets, checks and formats programs there, and each answer is compared
# with the same command's answer from the `nazm` built for this machine — standard output,
# standard error and exit status, each on its own.
#
#   scripts/windows-toolchain.sh [HOST] [FILE.nz ...]
#
# HOST is an ssh destination (default `win-dell`) whose shell is cmd.exe and whose account may
# write to its home directory; FILES default to bench/compare/nazm/*.nz and examples/*.nz. Per file:
# `nazm run --interpret`, `nazm check` and `nazm fmt --check`, standard input empty. The files and
# nazm.exe are copied to one directory there, every command runs from one batch file, the answers
# come back, and the directory is deleted. Output: one line per command and file, and a summary.
#
# TOOLS (W2) is a directory on the host holding clang and its linker — llvm-mingw's `bin`, e.g.
# `C:\Users\nazmci\nazm-tools\llvm-mingw-20261006-ucrt-x86_64\bin` — put on PATH for the batch
# file alone. With it, each file is also run natively (`nazm run`) and built (`nazm build`, the
# executable then run: its output, error output, and the build's and the program's exit status),
# `nazm test` runs over TESTS (default: examples/requirements/heater.nz and examples/apps/nbody,
# both legs), and `nazm doctor`'s answer there is printed. Without it, `nazm build` is tried once
# and what it lacks is printed, not counted.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${1:-win-dell}"; [ $# -gt 0 ] && shift
NAZM="${NAZM:-$ROOT/target/release/nazm}"
NAZM_EXE="${NAZM_EXE:-$ROOT/target/x86_64-pc-windows-gnu/release/nazm.exe}"
[ -x "$NAZM" ] || { echo "need a nazm for this machine at $NAZM (or set NAZM)" >&2; exit 2; }
[ -f "$NAZM_EXE" ] || { echo "need nazm.exe at $NAZM_EXE (cargo build --release -p nazm-cli --target x86_64-pc-windows-gnu, or set NAZM_EXE)" >&2; exit 2; }
ssh -o BatchMode=yes "$HOST" "ver" >/dev/null 2>&1 || { echo "no Windows host reachable as $HOST" >&2; exit 3; }

inputs=("$@")
if [ ${#inputs[@]} -eq 0 ]; then
  for f in "$ROOT"/bench/compare/nazm/*.nz "$ROOT"/examples/*.nz; do [ -f "$f" ] && inputs+=("$f"); done
fi

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/p" "$work/want" "$work/got"
remote="nz-toolchain-$$"
TOOLS="${TOOLS:-}"
commands=("run --interpret" "check" "fmt --check")
tags=(run check fmt)
[ -n "$TOOLS" ] && { commands+=("run"); tags+=(native); }
bat="$work/all.cmd"
printf '@echo off\r\ncd /d %%~dp0\r\nmkdir got\r\nmkdir b\r\n' >"$bat"
[ -n "$TOOLS" ] && printf 'set PATH=%s;%%PATH%%\r\n' "$TOOLS" >>"$bat"
mkdir -p "$work/b"
names=()
for f in "${inputs[@]}"; do
  name=$(basename "$f" .nz)
  [ -e "$work/p/$name.nz" ] && name="$name-$(echo "$f" | cksum | cut -c1-6)"
  cp "$f" "$work/p/$name.nz"; names+=("$name")
  for i in "${!commands[@]}"; do
    t=${tags[$i]}
    # The reference: the same command, the same relative path, here.
    (cd "$work" && "$NAZM" ${commands[$i]} "p/$name.nz" </dev/null >"want/$name.$t.out" 2>"want/$name.$t.err"
     echo "exit=$?" >>"want/$name.$t.out")
    printf 'nazm.exe %s p/%s.nz <NUL >got\\%s.%s.out 2>got\\%s.%s.err\r\n>>got\\%s.%s.out echo exit=%%ERRORLEVEL%%\r\n' \
      "${commands[$i]}" "$name" "$name" "$t" "$name" "$t" "$name" "$t" >>"$bat"
  done
  [ -n "$TOOLS" ] || continue
  # Built, then run: the build's own words on standard error, the program's after them.
  (cd "$work" && "$NAZM" build "p/$name.nz" -o "b/$name" </dev/null >/dev/null 2>"want/$name.built.err"
   b=$?; "b/$name" </dev/null >"want/$name.built.out" 2>>"want/$name.built.err"
   echo "build=$b exit=$?" >>"want/$name.built.out")
  printf 'nazm.exe build p/%s.nz -o b\\%s.exe <NUL >NUL 2>got\\%s.built.err\r\nset B=%%ERRORLEVEL%%\r\nb\\%s.exe <NUL >got\\%s.built.out 2>>got\\%s.built.err\r\n>>got\\%s.built.out echo build=%%B%% exit=%%ERRORLEVEL%%\r\n' \
    "$name" "$name" "$name" "$name" "$name" "$name" "$name" >>"$bat"
done
[ -n "$TOOLS" ] && tags+=(built)
if [ -n "$TOOLS" ]; then
  # `nazm test` over a directory of cases, both legs, here and there.
  mkdir -p "$work/t"
  if [ -n "${TESTS:-}" ]; then read -ra tests <<<"$TESTS"; else
    tests=("$ROOT/examples/requirements/heater.nz" "$ROOT/examples/apps/nbody/nbody.nz" "$ROOT/examples/apps/nbody/nbody.expected")
  fi
  cp "${tests[@]}" "$work/t/"
  (cd "$work" && "$NAZM" test t </dev/null >want/test.out 2>want/test.err; echo "exit=$?" >>want/test.out)
  printf 'nazm.exe test t <NUL >got\\test.out 2>got\\test.err\r\n>>got\\test.out echo exit=%%ERRORLEVEL%%\r\nnazm.exe doctor >got\\doctor.out 2>&1\r\n' >>"$bat"
else
  # W2's question, asked once: what a native build needs that this host has not got.
  printf 'nazm.exe build p/%s.nz -o hello.exe <NUL >got\\build.out 2>got\\build.err\r\n>>got\\build.out echo exit=%%ERRORLEVEL%%\r\n' "${names[0]}" >>"$bat"
fi

ssh "$HOST" "mkdir $remote\\p" >/dev/null || { echo "could not make $remote on $HOST" >&2; exit 3; }
scp -q "$NAZM_EXE" "$bat" "$HOST:$remote/" && scp -q "$work"/p/*.nz "$HOST:$remote/p/" \
  && { [ -z "$TOOLS" ] || scp -q -r "$work/t" "$HOST:$remote/"; } \
  || { ssh "$HOST" "rmdir /s /q $remote" >/dev/null 2>&1; echo "copy to $HOST failed" >&2; exit 3; }
ssh "$HOST" "$remote\\all.cmd" >/dev/null 2>&1
scp -q "$HOST:$remote/got/*" "$work/got/"
ssh "$HOST" "rmdir /s /q $remote" >/dev/null 2>&1

agree=0; differ=0
for name in "${names[@]}"; do
  for t in "${tags[@]}"; do
    for x in want got; do for s in out err; do
      # Line endings, trailing blanks and the path separator in what is echoed back are not answers.
      [ -f "$work/$x/$name.$t.$s" ] || : >"$work/$x/$name.$t.$s"
      tr -d '\r' <"$work/$x/$name.$t.$s" | sed -e 's/[[:space:]]*$//' -e 's|p\\|p/|g' >"$work/$x/$name.$t.$s.n"
    done; done
    if cmp -s "$work/want/$name.$t.out.n" "$work/got/$name.$t.out.n" && cmp -s "$work/want/$name.$t.err.n" "$work/got/$name.$t.err.n"; then
      agree=$((agree + 1)); echo "agree  $t $name"
    else
      differ=$((differ + 1)); echo "DIFFER $t $name"
      diff "$work/want/$name.$t.out.n" "$work/got/$name.$t.out.n" | head -6
      diff "$work/want/$name.$t.err.n" "$work/got/$name.$t.err.n" | head -6
    fi
  done
done
if [ -n "$TOOLS" ]; then
  for x in want got; do for s in out err; do
    tr -d '\r' <"$work/$x/test.$s" 2>/dev/null | sed -e 's/[[:space:]]*$//' -e 's|t\\|t/|g' >"$work/$x/test.$s.n"
  done; done
  if cmp -s "$work/want/test.out.n" "$work/got/test.out.n" && cmp -s "$work/want/test.err.n" "$work/got/test.err.n"; then
    agree=$((agree + 1)); echo "agree  test t ($(grep -c '^ok' "$work/got/test.out.n") cases passed)"
  else
    differ=$((differ + 1)); echo "DIFFER test t"; diff "$work/want/test.out.n" "$work/got/test.out.n" | head -8
  fi
  echo "nazm doctor there:"; tr -d '\r' <"$work/got/doctor.out" | sed 's/^/  /'
else
  echo "nazm build there (W2): $(tr -d '\r' <"$work/got/build.out" 2>/dev/null | tail -1); $(tr -d '\r' <"$work/got/build.err" 2>/dev/null | head -2 | tr '\n' ' ')"
fi
ver=$(ssh "$HOST" "ver" | tr -d '\r' | grep -o 'Version [0-9.]*')
echo "windows-toolchain ($HOST, $ver): $agree agree, $differ differ (${#names[@]} files x ${#tags[@]} commands${TOOLS:+, and nazm test})"
[ "$differ" -eq 0 ]
