#!/usr/bin/env bash
# Run programs built for x86_64-pc-windows-gnu on a real Windows host, and compare each with the
# same program built natively here: standard output, standard error and exit status, each on its
# own. The evidence docs/support.md calls run-verified on a Windows host.
#
#   scripts/windows-host.sh [HOST] [FILE.nz ...]
#
# HOST is an ssh destination (default `win-dell`) whose shell is cmd.exe and whose account may
# write to its home directory; FILES default to the set toolchain/codegen/tests/run.sh runs. Each
# program is built to objects with `nazm build --target x86_64-pc-windows-gnu --objects`, linked
# here with MinGW-w64's gcc as its link.txt says, copied over, run once, and deleted there. A
# program that reads an argument, a file or standard input, or calls C, is skipped and counted,
# never passed. BACKEND (`llvm`, the default, `cranelift`, or `nazm` — Nazm's own code generator,
# found by NAZM_NZCG or installed beside `nazm`) builds both the Windows program and the native
# one. Output: one line per program and a summary.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${1:-win-dell}"; [ $# -gt 0 ] && shift
NAZM="${NAZM:-$ROOT/target/release/nazm}"
BACKEND="${BACKEND:-llvm}"
GCC="${GCC:-x86_64-w64-mingw32-gcc}"
command -v "$GCC" >/dev/null || { echo "need $GCC (brew install mingw-w64)" >&2; exit 2; }
[ -x "$NAZM" ] || { echo "need a nazm at $NAZM (or set NAZM)" >&2; exit 2; }
ssh -o BatchMode=yes "$HOST" "ver" >/dev/null 2>&1 || { echo "no Windows host reachable as $HOST" >&2; exit 3; }

inputs=("$@")
if [ ${#inputs[@]} -eq 0 ]; then
  for f in "$ROOT"/tests/compat/v1/run/*.nz "$ROOT"/toolchain/codegen/tests/programs/*.nz \
           "$ROOT"/bench/compare/nazm/*.nz "$ROOT"/bench/programs/*.nz "$ROOT"/examples/*.nz \
           "$ROOT"/examples/tour/*.nz "$ROOT"/examples/std/*.nz; do
    [ -f "$f" ] && inputs+=("$f")
  done
fi

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
agree=0; differ=0; skipped=0
for f in "${inputs[@]}"; do
  f="$(cd "$(dirname "$f")" && pwd)/$(basename "$f")"
  name=$(basename "$f" .nz)
  # Not for this comparison: programs that need C, input, arguments or a file of their own.
  if [ -f "${f%.nz}.c" ] || grep -qE 'extern "C"|arg\(|io_stdin|read_file|stdin|fs_|file_|os_open' "$f"; then
    skipped=$((skipped + 1)); echo "skip   $name (needs C, input or arguments)"; continue
  fi
  d="$work/$name"; mkdir -p "$d"
  if ! "$NAZM" build --backend "$BACKEND" --target x86_64-pc-windows-gnu --objects "$d/o" "$f" >"$d/build.log" 2>&1; then
    skipped=$((skipped + 1)); echo "skip   $name (not built for Windows: $(grep -m1 -oE 'N[0-9]{4}' "$d/build.log"))"; continue
  fi
  objs=$(sed -n 's/^objects //p' "$d/o/link.txt"); libs=$(sed -n 's/^libraries //p' "$d/o/link.txt")
  (cd "$d/o" && "$GCC" $objs $libs -o "$d/$name.exe") >"$d/link.log" 2>&1 \
    || { skipped=$((skipped + 1)); echo "skip   $name (link failed)"; continue; }
  # The same program built for this machine, the reference.
  if ! "$NAZM" build --backend "$BACKEND" "$f" -o "$d/host" >"$d/host.log" 2>&1; then
    skipped=$((skipped + 1)); echo "skip   $name (not built for this host)"; continue
  fi
  (cd "$d" && ./host >"$d/want.out" 2>"$d/want.err"); echo "exit=$?" >>"$d/want.out"
  scp -q "$d/$name.exe" "$HOST:nz-$name.exe" || { skipped=$((skipped + 1)); echo "skip   $name (copy failed)"; continue; }
  ssh "$HOST" "nz-$name.exe" >"$d/got.out" 2>"$d/got.err"; echo "exit=$?" >>"$d/got.out"
  ssh "$HOST" "del nz-$name.exe" >/dev/null 2>&1
  for x in want.out want.err got.out got.err; do tr -d '\r' <"$d/$x" | sed 's/[[:space:]]*$//' >"$d/$x.n"; done
  if cmp -s "$d/want.out.n" "$d/got.out.n" && cmp -s "$d/want.err.n" "$d/got.err.n"; then
    agree=$((agree + 1)); echo "agree  $name"
  else
    differ=$((differ + 1)); echo "DIFFER $name"
    diff "$d/want.out.n" "$d/got.out.n" | head -4; diff "$d/want.err.n" "$d/got.err.n" | head -4
  fi
done
ver=$(ssh "$HOST" "ver" | tr -d '\r' | grep -o 'Version [0-9.]*')
echo "windows-host ($HOST, $ver, $BACKEND): $agree agree, $differ differ, $skipped skipped"
[ "$differ" -eq 0 ]
