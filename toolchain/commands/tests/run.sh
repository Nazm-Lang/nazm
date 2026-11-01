#!/bin/sh
# R5's comparisons (`openspec/changes/r5-commands/design.md`): each command written in Nazm against
# the Rust `nazm` built from this checkout, on every input.
#
#   toolchain/commands/tests/run.sh [-j N] [PART...] [FILE.nz ...]     PART: fmt, args, init, check, doc, fix, lsp, test, interface, docs, bindgen, reload, agent, mcp (all by default)
#
# The Nazm `nazm` (toolchain/commands/nazm.nz, which dispatches to CMD.nz) is built by the checkout's
# `nazm` ($NAZM, with a heap limit of at most 2 GiB) and run as the reference is run, `nazm CMD
# ARGS`. It runs thousands of times, in parallel, so this is contained work (R6, `docs/internal/runbook.md`):
#
#   cargo xtask contained run 'cargo build --locked -p nazm-cli && NAZM=target/debug/nazm sh toolchain/commands/tests/run.sh'
#
# and refuses to start anywhere else. The reference is
# `nazm` built from this checkout into $CARGO_TARGET_DIR (default ~/.cache/nazm-target-track5), or
# $NAZM_REF when set. Every comparison is of the whole observable behaviour — standard output,
# standard error, the exit status and every file afterwards — byte for byte. A refusal (exit 6:
# something the port does not carry) is counted apart and is never an agreement.
#
#   fmt   For every input — the repository's tracked `.nz` files, which include
#         toolchain/frontend-tests/inputs, or the FILEs named — a copy is formatted by each side
#         twice: `fmt --check FILE`, then `fmt FILE`, which rewrites it. Then one `fmt --check`
#         over every input at once, and a missing file named after one that is rewritten.
#   args  Every command line in tests/args/CMD.txt (R5_ARGS="lock yank" for those commands only), run by both sides as `nazm CMD LINE` in the same
#         fixture directory: clap's usage errors and help, and what the command then does (design,
#         D8); and every line of tests/args/nazm.txt as `nazm LINE`: the top level (design, D12).
#   init  `nazm init` of every template, the trees written compared file by file (design, D9).
#   check For every input (as for fmt, or the FILEs named) and the repository's package
#         directories (a package's `main`, or a library's modules), `check --no-cache FILE` and
#         `check --json --no-cache FILE` from the repository's root, and `--deny-warnings` too where
#         the reference warns (design, D11). The standard library is the checkout's (NAZM_LIBRARY);
#         the reference's check cache goes to a scratch directory (NAZM_CACHE_DIR).
#   doc   For every input of check, the repository's package directories and a project of every
#         template, `doc PATH` from the repository's root, and `doc -o DIR PATH` where the reference
#         succeeds: status, output, error, `api.json` and `api.md` (design, D13).
#   lsp   Every scripted session of tests/lsp — each message framed, `@ROOT@` the fixture's
#         directory — run by both sides as `nazm lsp` in the same fixture: every byte of the output,
#         the error and the status (design, D16). Then every input of check opened as it is on
#         disk and closed, and again with hover at a few positions (tests/lsp-session.py, which
#         needs python3 to write the sessions).
#
#   test  Every recorded case of the repository (a tracked `NAME.nz` beside `NAME.expected`),
#         `test --interpret-only` text and `--json` from the repository's root; then the
#         conformance corpus and a fixture whole, compiled legs too, with `--summary-json`,
#         `--filter` and `--fuzz`. Both sides start the reference as the legs' driver
#         (NAZM_TEST_DRIVER; design D17).
#
#   (`patch` has no part of its own: its command lines, with the plans it applies, are tests/args/patch.txt.)
#
#   interface  For every input of check, `interface FILE` and `interface --hash FILE` from the
#         repository's root (design D19).
#
#   docs  `docs --index` in each form and every section of the corpus by its id (`--section ID
#         --json`, the text of every tenth), then corpora with one thing wrong (a missing, non-UTF-8
#         or duplicated document, a broken link, an unclosed fence, a setext heading, a misplaced
#         anchor, a grammar with a detached comment ...): the answers and the problems named (design D20).
#
#   bindgen  Every header of tests/bindgen and bench/compare, with and without `-o`: the module, the refusals,
#         the report and the file written (design D22).
#
#   reload  `reload-check OLD NEW` for every pair of the programs of tests/reload, itself included (design D22).
#
#   agent  For every input of check, `snapshot` and `diagnostics` (compact and indented), `resolve` and `references` of the first three
#         definitions (each side with a cold check cache of its own: the answer is the cache's), `diagnostics --detail`, `delta`
#         against the reference's snapshot and two baselines made wrong, `patch rename` at six names and `patch fix` at the
#         diagnostics that carry a fix (design D24).
#
#   mcp   `nazm-mcp` (toolchain/commands/mcp.nz, built unless NAZM_MCP_PORT names one; the reference is NAZM_MCP_REF, by default beside
#         the reference's `nazm`) in sessions over stdio (tests/mcp-compare.py; design D25): the scripted session of tests/mcp/protocol.txt
#         and the documentation's sections, then for every input of check the snapshot, diagnostics, check summary, deltas, details and
#         patches, each answer compared byte for byte.
#
# NAZM_PORT names a port already built from this checkout, instead of building one.
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
    fmt|args|init|check|doc|fix|lsp|test|interface|docs|bindgen|reload|agent|mcp) parts="$parts $1"; shift ;;
    *) break ;;
  esac
done
[ -n "$parts" ] || parts="fmt args init check doc fix lsp test interface docs bindgen reload agent mcp"

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
. "$root/toolchain/lib/stage.sh"
stage_name=commands
stage_init "$root" contained
nazm=${NAZM:-$(stage_checkout_nazm "$root")}
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

export NAZM_TEST_DRIVER="$ref"
mkdir -p "$work/bin" "$work/cache"
# The checker recurses with the program's nesting, as the reference's does on its 24 MiB stack.
# NAZM_NO_LIBRARY=1 leaves NAZM_LIBRARY unset: the Nazm `nazm` then reads the library it carries (R8 slice 12).
if [ -n "${NAZM_NO_LIBRARY:-}" ]; then unset NAZM_LIBRARY; else export NAZM_LIBRARY="$root/library"; fi
export NAZM_CACHE_DIR="$work/cache"
if [ -n "${NAZM_PORT:-}" ]; then
  # A port already built from this checkout's toolchain/commands/nazm.nz.
  port=$NAZM_PORT
else
  stage_build "$nazm" --stack-size 256M "$root/toolchain/commands/nazm.nz" -o "$work/bin/nazm" > "$work/build.log" 2>&1 \
    || { cat "$work/build.log" >&2; echo "run.sh: toolchain/commands/nazm.nz did not build" >&2; exit 2; }
  port="$work/bin/nazm"
fi

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
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work"

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
    side "$d/got" "$R5_PORT" fmt
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
  (cd "$work/batch/got" && { rc=0; "$port" fmt --check $args > ../got.out 2> ../got.err || rc=$?; echo "exit $rc" >> ../got.out; })
  if cmp -s "$work/batch/ref.out" "$work/batch/got.out" && cmp -s "$work/batch/ref.err" "$work/batch/got.err"; then batch=agree; else batch=differ; fi

  # A file that is not there, after one that is rewritten: the command stops at it, as the
  # reference does, with the first file already written.
  for side in ref got; do
    mkdir -p "$work/missing/$side"
    cp "$root/toolchain/commands/tests/inputs/unicode-space.nz" "$work/missing/$side/a.nz"
  done
  (cd "$work/missing/ref" && { rc=0; "$ref" fmt a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
  (cd "$work/missing/got" && { rc=0; "$port" fmt a.nz absent.nz > out 2> err || rc=$?; echo "exit $rc" >> out; cat a.nz >> out; })
  if cmp -s "$work/missing/ref/out" "$work/missing/got/out" && cmp -s "$work/missing/ref/err" "$work/missing/got/err"; then :; else batch=differ; fi

  echo "fmt: $agree agree, $differ differ, $refused refused (of $total inputs); batch $batch"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] && [ "$batch" = agree ] || status=1
}

# --- one run of a command, recorded ---------------------------------------------------------------

# The fixture a command line runs in: what each command's lines name.
# The packages `lock`, `update`, `publish` and `yank` run on, in `DIR`: path packages (a library, an
# application, a diamond, a cycle, a version that is not the one declared, a name that is not the
# directory's, features, a manifest that is not one), a registry of published versions made by the
# reference's own `publish`, applications that depend on it (one that cannot be solved), indexes that are
# wrong, a stale version directory, and a package holding a link.
pkg_fixture() {
  dir=$1
  mkdir -p "$dir"
  mk() {
    mkdir -p "$1/src"
    printf 'schema = "nazm.package/1"\n\n[package]\nname = "%s"\nversion = "%s"\n%s' "$2" "$3" "${4:-}" > "$1/nazm.toml"
    printf 'pub fn f() -> Int { %s }\n' "$(echo "$3" | tr -d .)" > "$1/src/lib.nz"
  }
  (
    cd "$dir"
    mk one one 1.0.0
    mk lib lib 1.0.0
    printf '\n[features]\nx = []\n' >> lib/nazm.toml
    mk app app 0.1.0 '
[dependencies]
lib = { path = "../lib", version = "1.0.0" }
'
    mk mid mid 1.0.0 '
[dependencies]
lib = { path = "../lib", version = "1.0.0" }
'
    mk dia dia 1.0.0 '
[dependencies]
lib = { path = "../lib", version = "1.0.0" }
mid = { path = "../mid", version = "1.0.0" }
'
    mk wrong wrong 1.0.0 '
[dependencies]
lib = { path = "../lib", version = "2.0.0" }
'
    mk cyc1 cyc1 1.0.0 '
[dependencies]
cyc2 = { path = "../cyc2", version = "1.0.0" }
'
    mk cyc2 cyc2 1.0.0 '
[dependencies]
cyc1 = { path = "../cyc1", version = "1.0.0" }
'
    mk badname badname 1.0.0 '
[dependencies]
lib = { path = "../one", version = "1.0.0" }
'
    mk feat feat 1.0.0 '
[dependencies]
lib = { path = "../lib", version = "1.0.0", features = ["x"] }

[features]
default = ["extra"]
extra = []
'
    mk nofeat nofeat 1.0.0 '
[dependencies]
lib = { path = "../lib", version = "1.0.0", features = ["nope"] }
'
    mk bad bad 1.0.0 'mystery = 1
'
    mk nosrc nosrc 1.0.0
    rm -rf nosrc/src
    mkdir -p reg sub
    for v in 1.0.0 1.1.0 1.2.0 2.0.0; do mk "pub/util-$v" util "$v"; "$ref" publish "pub/util-$v" --registry reg > /dev/null 2>&1; done
    mk pub/util-3.0.0 util 3.0.0
    mk pub/deep deep 1.0.0 '
[dependencies]
util = { version = "^1.0.0" }
'
    "$ref" publish pub/deep --registry reg > /dev/null 2>&1
    mk pub/deep2 deep 1.1.0 '
[dependencies]
util = { version = "~1.1.0" }
'
    mk pub/odd odd 1.0.0 '
[dependencies]
util = { version = "2.0.0" }
'
    "$ref" publish pub/odd --registry reg > /dev/null 2>&1
    mk app1 app1 0.1.0 '
[registry]
path = "../reg"

[dependencies]
deep = { version = "^1.0.0" }
'
    mk app2 app2 0.1.0 '
[registry]
path = "../reg"

[dependencies]
deep = { version = "^1.0.0" }
odd = { version = "1.0.0" }
'
    mk app3 app3 0.1.0 '
[registry]
path = "../reg"

[dependencies]
util = { version = "^1.0.0" }
'
    mk app4 app4 0.1.0 '
[dependencies]
util = { version = "^1.0.0" }
'
    mk app5 app5 0.1.0 '
[registry]
path = "../reg"

[dependencies]
nothere = { version = "^1.0.0" }
'
    mk app6 app6 0.1.0 '
[registry]
path = "../reg"

[dependencies]
util = { version = "^9.0.0" }
'
    mk link link 1.0.0
    ln -s ../../one/src/lib.nz link/src/linked
    mk stale stale 1.0.0
    mkdir -p reg/packages/stale/1.0.0
    printf 'schema = "nazm.registry-index/9"\nname = "broken"\n' > reg/index/broken.toml
    printf 'schema = "nazm.registry-index/1"\nname = "dup"\n\n[[version]]\nversion = "1.0.0"\ndigest = "blake3:%064d"\n\n[[version]]\nversion = "1.0.0"\ndigest = "blake3:%064d"\n' 0 0 > reg/index/dup.toml
    printf 'schema = "nazm.registry-index/1"\nname = "badidx"\n\n[[version]]\nversion = "1.0.0"\ndigest = "sha256:abc"\n' > reg/index/badidx.toml
    # m1, m2, … : one package each, written with the manifest of the same line of tests/manifests.txt.
    n=0
    while IFS= read -r body; do
      n=$((n + 1))
      mkdir -p "m$n/src"
      printf 'pub fn f() -> Int { 1 }\n' > "m$n/src/lib.nz"
      # shellcheck disable=SC2059
      printf "$body" > "m$n/nazm.toml"
    done < "$root/toolchain/commands/tests/manifests.txt"
  )
}

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
      # app/ is a package whose `main` imports the library package lib/; lib/'s two modules share
      # an error; bad/ imports a package it does not declare; nopkg.nz imports one outside a package;
      # broken/'s manifest is not one.
      mkdir -p "$dir/app/src" "$dir/lib/src/inner" "$dir/bad/src" "$dir/broken"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "app"\nversion = "0.1.0"\nmain = "main.nz"\n\n[dependencies]\nlib = { path = "../lib", version = "1.0.0" }\n' > "$dir/app/nazm.toml"
      printf 'use "lib:a.nz";\nfn main() -> Int { twice(2) }\n' > "$dir/app/src/main.nz"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "lib"\nversion = "1.0.0"\n' > "$dir/lib/nazm.toml"
      printf 'use "inner/b.nz";\npub fn twice(n: Int) -> Int { n * k() }\n' > "$dir/lib/src/a.nz"
      printf 'pub fn k() -> Int {\n    let s: Str = 2;\n    2\n}\n' > "$dir/lib/src/inner/b.nz"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "bad"\nversion = "0.1.0"\nmain = "main.nz"\n' > "$dir/bad/nazm.toml"
      printf 'use "lib:a.nz";\nfn main() -> Int { 0 }\n' > "$dir/bad/src/main.nz"
      printf 'use "lib:a.nz";\nfn main() -> Int { 0 }\n' > "$dir/nopkg.nz"
      printf 'schema = "nazm.package/9"\n' > "$dir/broken/nazm.toml"
      # lk/ is app/ with a lockfile (written by the reference); lk1/ ... lk12/ hold it wrong in a way each:
      # a digest, a version, a package it does not use, a package missing, a package twice, another
      # schema, TOML that is not, a field it does not know, no schema, an empty file, a directory.
      mkdir -p "$dir/lk/src"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "lk"\nversion = "0.1.0"\nmain = "main.nz"\n\n[dependencies]\nlib = { path = "../lib", version = "1.0.0" }\n' > "$dir/lk/nazm.toml"
      cp "$dir/app/src/main.nz" "$dir/lk/src/main.nz"
      (cd "$dir" && "$ref" lock lk > /dev/null 2>&1)
      for n in 1 2 3 4 5 6 7 8 9 10 11 12; do cp -R "$dir/lk" "$dir/lk$n"; done
      (
        cd "$dir"
        awk 'BEGIN{n=0} /^digest/ && n==0 {sub(/"blake3:./,"\"blake3:x"); n=1} {print}' lk/nazm.lock > lk1/nazm.lock
        sed 's/^version = "1.0.0"/version = "1.0.1"/' lk/nazm.lock > lk2/nazm.lock
        { cat lk/nazm.lock; printf '\n[[package]]\nname = "zzz"\nversion = "1.0.0"\nsource = "path+../zzz"\ndigest = "blake3:00"\ndependencies = []\n'; } > lk3/nazm.lock
        awk 'BEGIN{RS="";ORS="\n\n"} !/name = "lib"/' lk/nazm.lock > lk4/nazm.lock
        awk 'BEGIN{RS="";ORS="\n\n"} {print} /name = "lib"/{print}' lk/nazm.lock > lk5/nazm.lock
        sed 's|nazm.lock/1|nazm.lock/9|' lk/nazm.lock > lk6/nazm.lock
        printf 'not toml' > lk7/nazm.lock
        awk '/^dependencies/ {print "extra = 1"} {print}' lk/nazm.lock > lk8/nazm.lock
        grep -v '^schema' lk/nazm.lock > lk9/nazm.lock
        : > lk10/nazm.lock
        sed 's|nazm.lock/1|nazm.lock/2|' lk/nazm.lock > lk11/nazm.lock
        rm lk12/nazm.lock; mkdir lk12/nazm.lock
      )
      ;;
    doc)
      printf '// Adds.\n//   Two lines.\npub fn add(a: Int, b: Int) -> Int { a + b }\n\n@deprecated("use add")\npub fn plus(a: Int) -> Int { a }\n\n/// Kept.\npub struct P { x: Int, y: Str }\n\npub enum E { A, B(n: Int) }\n' > "$dir/a.nz"
      printf 'pub fn f() -> Int {\n    let x: Bool = 1;\n    0\n}\n' > "$dir/b.nz"
      printf 'use "gone.nz";\npub fn f() -> Int { 0 }\n' > "$dir/m.nz"
      mkdir -p "$dir/sub" "$dir/pkg/src/inner" "$dir/ro"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "pkg"\nversion = "0.2.0"\n' > "$dir/pkg/nazm.toml"
      printf 'use "inner/b.nz";\npub use "inner/b.nz" { g as h };\npub fn f() -> Int { g() }\n' > "$dir/pkg/src/a.nz"
      printf 'pub fn g() -> Int { 1 }\n' > "$dir/pkg/src/inner/b.nz"
      printf 'x\n' > "$dir/ro/api.json"
      ;;
    fix)
      printf 'fn main() -> Int {\n    0\n}\n' > "$dir/ok.nz"
      printf 'fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n' > "$dir/mut.nz"
      printf 'fn main() -> Int {\n    let x = 1;\n    let y = 2;\n    x = 3;\n    y = 4;\n    x + y\n}\n' > "$dir/two.nz"
      printf 'fn main() -> Int {\n    let s = "abc;\n    0\n}\n' > "$dir/lex.nz"
      printf 'fn f() -> Int { 1 }\nfn main() -> Int {\n    f()\n    0\n}\n' > "$dir/semi.nz"
      printf 'use "helper.nz";\nfn main() -> Int { h() }\n' > "$dir/imp.nz"
      printf 'pub fn h() -> Int {\n    let v = 1;\n    v = 2;\n    v\n}\n' > "$dir/helper.nz"
      printf 'use "gone.nz";\nfn main() -> Int { 0 }\n' > "$dir/m.nz"
      mkdir -p "$dir/sub"
      ;;
    lsp)
      # What tests/lsp's sessions open: a type error (a.nz), CRLF lines after non-ASCII text (u.nz),
      # warnings (w.nz), warnings beside an error (we.nz), an import with an error (i.nz, helper.nz),
      # a syntax error (s.nz), functions and calls (f.nz, c.nz, h2.nz), a path with a space, and a
      # package importing a dependency whose module has an error (app/, lib/).
      printf 'fn main() -> Int {\n    let x: Bool = 1;\n    0\n}\n' > "$dir/a.nz"
      printf 'fn main() -> Int {\r\n    let s: Str = "é𝄞"; let mut n = 1;\r\n    n = 2;\r\n    let t = s;\r\n    n\r\n}\r\n' > "$dir/u.nz"
      printf 'enum E { A, B, C }\nfn main() -> Int {\n    match E.A {\n        E.A => 0,\n        _ => 1,\n    }\n}\n' > "$dir/w.nz"
      printf 'enum E { A, B, C }\nfn f() -> Int {\n    match E.B {\n        E.B => 0,\n        _ => 1,\n    }\n}\nfn main() -> Int {\n    let x: Bool = 1;\n    match E.A {\n        E.A => 0,\n        _ => f(),\n    }\n}\n' > "$dir/we.nz"
      printf 'use "helper.nz";\nfn main() -> Int { h() }\n' > "$dir/i.nz"
      printf 'pub fn h() -> Int {\n    let v: Str = 1;\n    0\n}\n' > "$dir/helper.nz"
      printf 'fn main() -> Int { 1 + }\n' > "$dir/s.nz"
      printf 'fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int {\n    let total = add(1, 2);\n    total\n}\n' > "$dir/f.nz"
      printf 'use "h2.nz";\nfn add(a: Int, b: Int) -> Int { a + b }\nfn quiet(n: Int) -> Int ! {} { n }\nfn main() -> Int {\n    let total = add(1, 2);\n    let q = quiet(total);\n    twice(q)\n}\n' > "$dir/c.nz"
      printf 'pub fn twice(n: Int) -> Int { n * 2 }\n' > "$dir/h2.nz"
      printf 'fn main() -> Int {\n    let q = 1;\n    q\n}\n' > "$dir/with space.nz"
      mkdir -p "$dir/app/src" "$dir/lib/src/inner"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "app"\nversion = "0.1.0"\nmain = "main.nz"\n\n[dependencies]\nlib = { path = "../lib", version = "1.0.0" }\n' > "$dir/app/nazm.toml"
      printf 'use "lib:a.nz";\nfn main() -> Int { twice(2) }\n' > "$dir/app/src/main.nz"
      printf 'schema = "nazm.package/1"\n\n[package]\nname = "lib"\nversion = "1.0.0"\n' > "$dir/lib/nazm.toml"
      printf 'use "inner/b.nz";\npub fn twice(n: Int) -> Int { n * k() }\n' > "$dir/lib/src/a.nz"
      printf 'pub fn k() -> Int {\n    let s: Str = 2;\n    2\n}\n' > "$dir/lib/src/inner/b.nz"
      ;;
    test)
      # ok.nz passes; m.nz's output is not its expectation; units.nz holds test functions (an Ok, an
      # Err, a trap, two `fails`, a fuzz target and a `@cfg` that does not hold); bad.nz does not check;
      # lone.nz has no expectation; sub/ and empty/ are directories.
      printf 'fn main() -> Int {\n    print("hi\\n");\n    0\n}\n' > "$dir/ok.nz"
      printf 'hi\n0\n' > "$dir/ok.expected"
      printf 'fn main() -> Int {\n    print("x\\n");\n    0\n}\n' > "$dir/m.nz"
      printf 'y\n0\n' > "$dir/m.expected"
      printf 'fn main() -> Int {\n    let x: Bool = 1;\n    0\n}\n' > "$dir/bad.nz"
      printf '0\n' > "$dir/bad.expected"
      printf 'fn main() -> Int { 3 }\n' > "$dir/lone.nz"
      printf 'use "@std/test";\n\nfn main() -> Int { 0 }\n\n@test\nfn passes() -> Result[Int, Str] { assert_true("t", true) }\n\n@test\nfn errs() -> Result[Int, Str] { Result[Int, Str].Err(error: "boom") }\n\n@test\nfn traps() -> Result[Int, Str] {\n    let z = 0;\n    Result[Int, Str].Ok(value: 5 / z)\n}\n\n@test(fails)\nfn expected_err() -> Result[Int, Str] { Result[Int, Str].Err(error: "no") }\n\n@test(fails)\nfn wrongly_ok() -> Result[Int, Str] { Result[Int, Str].Ok(value: 0) }\n\n@fuzz\nfn short(input: Bytes) -> Result[Int, Str] {\n    if bytes_len(input) < 40 { Result[Int, Str].Ok(value: 0) } else { Result[Int, Str].Err(error: int_to_str(bytes_len(input))) }\n}\n\n@cfg(os = "nowhere")\n@test\nfn never() -> Result[Int, Str] { Result[Int, Str].Err(error: "ran") }\n\n@bench\nfn timed() -> Int { 1 }\n' > "$dir/units.nz"
      printf 'units\n' > "$dir/units.expected"
      printf '\377\376\n' > "$dir/binary.expected"
      printf 'fn main() -> Int { 0 }\n' > "$dir/binary.nz"
      printf 'fn main() -> Int { 0 }\n' > "$dir/.nz"
      printf 'fn main() -> Int { 0 }\n' > "$dir/odd.name.nz"
      printf '0\n' > "$dir/odd.name.expected"
      printf 'x\n' > "$dir/note.txt"
      mkdir -p "$dir/sub" "$dir/empty"
      printf 'fn main() -> Int { 0 }\n' > "$dir/sub/inner.nz"
      printf '0\n' > "$dir/sub/inner.expected"
      ;;
    patch)
      # mut.nz has one fix; plan.json is the reference's plan for it, and the others are variants of
      # it that `apply` must refuse (or, for plan.json, twice.json and dot.json, carry out).
      printf 'fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n' > "$dir/mut.nz"
      mkdir -p "$dir/sub"
      printf 'fn main() -> Int { 0 }\n' > "$dir/sub/b.nz"
      (cd "$dir" && "$ref" patch fix --root mut.nz --file mut.nz --byte 38 --json > plan.json)
      (cd "$dir" && {
        sed 's/"status":"available"/"status":"unavailable"/' plan.json > bad.json
        sed 's/"digest":"./"digest":"z/' plan.json > tamper.json
        sed 's|"path":"mut.nz"|"path":"../mut.nz"|' plan.json > escape.json
        sed 's|"path":"mut.nz"|"path":"/etc/hosts"|' plan.json > abs.json
        sed 's|"path":"mut.nz"|"path":"./mut.nz"|' plan.json > dot.json
        sed 's/"expected":"x"/"expected":"y"/' plan.json > stale.json
        sed 's/"replacement"/"replacemen"/' plan.json > malformed.json
        sed 's/"start":27/"start":-1/' plan.json > negative.json
        sed 's/"start":27/"start":27.0/' plan.json > float.json
        sed 's/"end":28/"end":20/' plan.json > backwards.json
        sed 's/"files":\[\(.*\)\],"validation"/"files":[\1,\1],"validation"/' plan.json > twice.json
        printf '{}\n' > empty.json
        printf '[1,2]\n' > array.json
      })
      ;;
    interface)
      printf 'pub struct P { x: Int }\npub fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { 0 }\n' > "$dir/i.nz"
      printf 'fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n' > "$dir/mut.nz"
      printf 'fn main() -> Int { 1 + }\n' > "$dir/s.nz"
      printf 'enum E { A, B, C }\nfn main() -> Int {\n    match E.A {\n        E.A => 0,\n        _ => 1,\n    }\n}\n' > "$dir/w.nz"
      printf 'use "helper.nz";\npub fn g() -> Int { h() }\n' > "$dir/imp.nz"
      printf 'pub fn h() -> Int { 1 }\n' > "$dir/helper.nz"
      printf 'use "../outside.nz";\npub fn g() -> Int { 0 }\n' > "$dir/out.nz"
      printf 'x\n' > "$dir/outside.nz"
      mkdir -p "$dir/sub"
      printf 'pub fn q() -> Int { 0 }\n' > "$dir/sub/q.nz"
      ;;
    docs)
      # The repository's documentation, linked: the corpus every line reads with `--root .`.
      ln -s "$root/docs" "$dir/docs"
      ;;
    attest)
      # A key pair made once per run and copied into every fixture (so both sides sign with one key), an
      # artefact, the provenance record that names it, and variants of the record.
      if [ ! -f "$work/akey/key" ]; then
        mkdir -p "$work/akey"
        ssh-keygen -q -t ed25519 -N '' -C test -f "$work/akey/key"
        printf 'tester %s\n' "$(cat "$work/akey/key.pub")" > "$work/akey/allowed"
      fi
      cp "$work/akey/key" "$work/akey/key.pub" "$work/akey/allowed" "$dir/"
      printf 'hello\n' > "$dir/art.bin"
      printf 'other\n' > "$dir/other.bin"
      printf '{"schema":"nazm.provenance/1","output":{"files":[]}}' > "$dir/none.json"
      h=$(cd "$dir" && "$ref" attest sign --key key none.json art.bin 2>&1 | sed 's/.*blake3:\([0-9a-f]*\)).*/\1/')
      printf '{"schema":"nazm.provenance/1","output":{"files":[{"path":"art.bin","digest":"blake3:%s"}]},"n":7,"s":"x\\u00e9y","nested":{"b":[true,null,-3],"a":"z"}}\n' "$h" > "$dir/prov.json"
      printf '{"schema":"nazm.provenance/2","output":{"files":[{"digest":"blake3:%s"}]}}\n' "$h" > "$dir/schema.json"
      printf '{"schema":"nazm.provenance/1","output":{"files":[{"digest":"blake3:%s"}]},"f":1.5}\n' "$h" > "$dir/frac.json"
      printf '[]\n' > "$dir/array.json"
      printf '{"schema":"nazm.provenance/1","output":{"files":[{"digest":"sha256:%s"}]}}\n' "$h" > "$dir/sha.json"
      cp "$dir/prov.json" "$dir/prov2.json"
      mkdir -p "$dir/sub"
      # a statement signed once, to verify
      (cd "$dir" && "$ref" attest sign --key key --out signed.json prov.json art.bin > /dev/null 2>&1)
      printf 'not json' > "$dir/notjson.json"
      printf '{"_type":"x","predicateType":"y"}' > "$dir/badtype.json"
      printf '{"_type":"https://in-toto.io/Statement/v1","predicateType":"urn:nazm:provenance:1","subject":[{"name":"x","digest":{"blake3":"00"}}]}' > "$dir/nosubject.json"
      for f in notjson badtype nosubject; do (cd "$dir" && ssh-keygen -q -Y sign -n nazm-attestation -f key "$f.json" 2>/dev/null); done
      ;;
    lock|update|publish|yank) pkg_fixture "$dir" ;;
    snapshot|delta|diagnostics|resolve|references)
      # check's files and packages, mut.nz (an error with a fix), and the snapshots delta compares with: base.json is the
      # reference's of a.nz, and each other one is it made wrong in a way of its own.
      fixture check "$dir"
      printf 'fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n' > "$dir/mut.nz"
      (
        cd "$dir"
        "$ref" snapshot --json --root a.nz > base.json
        "$ref" snapshot --json --root b.nz > other.json
        "$ref" snapshot --json --root mut.nz > mutbase.json
        sed 's/"shape":"[0-9a-f]/"shape":"f/' base.json > tweak.json
        sed 's/"identity":"a.nz::fn main"/"identity":"a.nz::fn mainz"/' base.json > renamed.json
        sed 's/"source":"[0-9a-f]/"source":"X/' base.json > invalid_digest.json
        sed 's|nazm.snapshot/3","status":"available"|nazm.snapshot/2","status":"available"|' base.json > wrongschema.json
        printf '{"schema":"nazm.snapshot/3","status":"unavailable","reason":"incomplete_compilation"}\n' > unavail.json
        printf 'not json' > notjson.json
        printf '{}' > empty.json
        sed 's/nazm 0.10.0/nazm 0.9.0/' base.json > wrongcompiler.json
        sed 's/"untracked"/"extra":1,"untracked"/' base.json > extra.json
        sed 's/"kind":"function"/"kind":"functio"/' base.json > kind.json
        sed 's/"identity":"a.nz::fn main"/"identity":"a.nz::fn  main"/' base.json > badid.json
      )
      ;;
    bindgen)
      printf '\r\nint64_t crlf(int64_t a);\r\nstruct H;\r\n' > "$dir/crlf.h"
      cp "$root"/toolchain/commands/tests/bindgen/*.h "$dir/"
      mkdir -p "$dir/sub" "$dir/out"
      printf 'x\n' > "$dir/out/taken.txt"
      ;;
    reload-check)
      printf 'fn add(a: Int, b: Int) -> Int {\r\n    a + b\r\n}\r\nfn main() -> Int { add(2, 2) }\r\n' > "$dir/crlf.nz"
      cp "$root"/toolchain/commands/tests/reload/*.nz "$dir/"
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
  (cd "$dir" && { rc=0; NAZM_CACHE_DIR="$dir.cache" "$@" < /dev/null > "$dir.out" 2> "$dir.err" || rc=$?; echo "exit $rc" > "$dir.record"; })
  # A package's modules are named by their canonical paths, which hold the side's own directory.
  canon=$(cd "$dir" && pwd -P)
  { echo "[stdout]"; sed "s|$canon|DIR|g" "$dir.out"; echo "[stderr]"; sed "s|$canon|DIR|g" "$dir.err"; echo "[tree]"; tree_record "$dir"; } >> "$dir.record"
}

# --- args: every command line of every ported command ---------------------------------------------

part_args() {
  n=0 agree=0 differ=0 refused=0
  for cmd in fmt init check doc fix lsp test patch interface docs attest lock update publish yank bindgen reload-check doctor snapshot delta diagnostics resolve references nazm; do
    # R5_ARGS names the commands whose lines to run (all by default).
    case " ${R5_ARGS:-$cmd} " in *" $cmd "*) ;; *) continue ;; esac
    file="$root/toolchain/commands/tests/args/$cmd.txt"
    [ -f "$file" ] || continue
    while IFS= read -r line; do
      case $line in '#'*|'') continue ;; esac
      n=$((n + 1))
      d="$work/args/$n"
      # The top level's lines run where `check`'s would, so a carried command they name has files.
      if [ "$cmd" = nazm ]; then fx=check; else fx=$cmd; fi
      # In a subshell: `fixture` sets `cmd`, which would make a top-level line a `check` one.
      (fixture "$fx" "$d/ref")
      (fixture "$fx" "$d/got")
      if [ "$cmd" = nazm ]; then eval "set -- $line"; else eval "set -- $cmd $line"; fi
      record "$d/ref" "$ref" "$@"
      record "$d/got" "$port" "$@"
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
      record "$d/got" "$port" init "$where" --template "$t"
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
    # The packages, by their directories: a package's `main`, or a library's modules (N74).
    (cd "$root" && { git ls-files '*nazm.toml'; find toolchain/frontend-tests/inputs -name nazm.toml -type f; } \
      | grep -v '^crates/nazm-cli/templates/' | sed 's|/nazm.toml$||' | LC_ALL=C sort -u) >> "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/cres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
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
    run "$d/got.text" timeout "$R5_DEADLINE" "$R5_PORT" check --no-cache "$f"
    run "$d/got.json" timeout "$R5_DEADLINE" "$R5_PORT" check --json --no-cache "$f"
    run "$d/ref.sum" "$R5_REF" check --summary-json --no-cache "$f"
    run "$d/got.sum" timeout "$R5_DEADLINE" "$R5_PORT" check --summary-json --no-cache "$f"
    kinds="text json sum"
    if grep -q "^warning\[" "$d/ref.text.e"; then
      run "$d/ref.deny" "$R5_REF" check --no-cache --deny-warnings "$f"
      run "$d/got.deny" timeout "$R5_DEADLINE" "$R5_PORT" check --no-cache --deny-warnings "$f"
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

# --- doc: every input, and every package --------------------------------------------------------------

part_doc() {
  list="$work/doc-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
    # The repository's packages, by their directories, and a project of every template.
    (cd "$root" && git ls-files '*nazm.toml' | grep -v '^crates/nazm-cli/templates/' | sed 's|/nazm.toml$||') >> "$list"
    for t in cli library server embedded wasm contract; do
      (cd "$work" && "$ref" init "doc-$t" --template "$t" > /dev/null 2>&1) && echo "$work/doc-$t" >> "$list"
    done
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/dres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # One input: `doc PATH` and `doc -o OUT PATH` from the repository's root, each side's status,
  # output, error and the files written, compared byte for byte.
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/d-$name"
    mkdir -p "$d"
    run() {
      out=$1; shift
      (cd "$R5_ROOT" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; })
      sed "s|$d/[a-z]*-out|OUT|g" "$out.o" >> "$out"
      sed "s|$d/[a-z]*-out|OUT|g" "$out.e" >> "$out"
    }
    files() {
      for x in api.json api.md; do [ -f "$1/$x" ] && { echo "[$x]"; cat "$1/$x"; }; done
    }
    run "$d/ref.line" "$R5_REF" doc "$f"
    run "$d/got.line" timeout "$R5_DEADLINE" "$R5_PORT" doc "$f"
    kinds=line
    if grep -q "^exit 0$" "$d/ref.line"; then
      run "$d/ref.dir" "$R5_REF" doc -o "$d/ref-out" "$f"
      files "$d/ref-out" >> "$d/ref.dir"
      run "$d/got.dir" timeout "$R5_DEADLINE" "$R5_PORT" doc -o "$d/got-out" "$f"
      files "$d/got-out" >> "$d/got.dir"
      kinds="line dir"
    fi
    verdict=agree
    why=""
    for k in $kinds; do
      if grep -q "^exit 6$" "$d/got.$k"; then verdict=refused; why=$(cat "$d/got.$k.e"); break; fi
      if grep -q "^exit 124$" "$d/got.$k"; then verdict=timeout; break; fi
      cmp -s "$d/ref.$k" "$d/got.$k" || verdict=differ
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/dres/$name"
    if [ "$verdict" = differ ] || [ "$verdict" = timeout ]; then
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/doc-$name" && for k in $kinds; do cp "$d/ref.$k" "$KEEP/doc-$name/want.$k"; cp "$d/got.$k" "$KEEP/doc-$name/got.$k"; done
      fi
    fi
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/dres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/dres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/dres/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/dres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/dres/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/dres/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^doc: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "doc: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total inputs)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

# --- fix: every input ----------------------------------------------------------------------------------

part_fix() {
  list="$work/fix-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/xres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # One input: `fix FILE` and `fix --json FILE` from the repository's root; where the reference
  # would apply an edit, `fix --apply case.nz` on a copy, each side its own, the file compared too.
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/x-$name"
    mkdir -p "$d/ref" "$d/got"
    run() {
      out=$1; at=$2; shift 2
      (cd "$at" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; cat "$out.o" "$out.e" >> "$out"; })
    }
    run "$d/ref.text" "$R5_ROOT" "$R5_REF" fix "$f"
    run "$d/ref.json" "$R5_ROOT" "$R5_REF" fix --json "$f"
    run "$d/got.text" "$R5_ROOT" timeout "$R5_DEADLINE" "$R5_PORT" fix "$f"
    run "$d/got.json" "$R5_ROOT" timeout "$R5_DEADLINE" "$R5_PORT" fix --json "$f"
    kinds="text json"
    if grep -q "\[would apply\]$" "$d/ref.text.o"; then
      cp "$R5_ROOT/$f" "$d/ref/case.nz"
      cp "$R5_ROOT/$f" "$d/got/case.nz"
      run "$d/ref.apply" "$d/ref" "$R5_REF" fix --apply case.nz
      run "$d/got.apply" "$d/got" timeout "$R5_DEADLINE" "$R5_PORT" fix --apply case.nz
      cat "$d/ref/case.nz" >> "$d/ref.apply"
      cat "$d/got/case.nz" >> "$d/got.apply"
      kinds="$kinds apply"
    fi
    verdict=agree
    why=""
    for k in $kinds; do
      if grep -q "^exit 6$" "$d/got.$k"; then verdict=refused; why=$(cat "$d/got.$k.e"); break; fi
      if grep -q "^exit 124$" "$d/got.$k"; then verdict=timeout; break; fi
      cmp -s "$d/ref.$k" "$d/got.$k" || verdict=differ
    done
    printf "%s\n%s\n%s\n" "$verdict" "$why" "$kinds" > "$R5_WORK/xres/$name"
    if [ "$verdict" = differ ] || [ "$verdict" = timeout ]; then
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/fix-$name" && for k in $kinds; do cp "$d/ref.$k" "$KEEP/fix-$name/want.$k"; cp "$d/got.$k" "$KEEP/fix-$name/got.$k"; done
      fi
    fi
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/xres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/xres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/xres/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/xres/"* 2>/dev/null | wc -l | tr -d ' ')
  applied=$(grep -l 'apply$' "$work/xres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/xres/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/xres/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^fix: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "fix: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total inputs; $applied applied)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

# --- lsp: scripted sessions, and every input opened ----------------------------------------------------

# `session SESSION DIR BIN`: SESSION's messages, `@ROOT@` made DIR's canonical path, each framed as an
# editor frames it, given to `BIN lsp` in DIR; its status, output, error and the tree, to DIR.record.
session() {
  canon=$(cd "$2" && pwd -P)
  while IFS= read -r line; do
    case $line in '#'*|'') continue ;; esac
    body=$(printf '%s' "$line" | sed "s|@ROOT@|$canon|g")
    printf 'Content-Length: %s\r\n\r\n%s' "$(printf '%s' "$body" | wc -c | tr -d ' ')" "$body"
  done < "$1" > "$2.in"
  (cd "$2" && { rc=0; "$3" lsp < "$2.in" > "$2.out" 2> "$2.err" || rc=$?; echo "exit $rc" > "$2.record"; })
  { echo "[stdout]"; sed "s|$canon|DIR|g" "$2.out"; echo; echo "[stderr]"; cat "$2.err"; echo "[tree]"; tree_record "$2"; } >> "$2.record"
}

part_lsp() {
  n=0 agree=0 differ=0 refused=0
  for s in "$root"/toolchain/commands/tests/lsp/*.txt; do
    name=$(basename "$s" .txt)
    n=$((n + 1))
    d="$work/lsp/$name"
    (fixture lsp "$d/ref")
    (fixture lsp "$d/got")
    session "$s" "$d/ref" "$ref"
    session "$s" "$d/got" "$port"
    if grep -q '^exit 6$' "$d/got.record"; then
      refused=$((refused + 1)); echo "  refused: $name: $(grep 'not carried' "$d/got.err" | cut -c1-100)"
    elif cmp -s "$d/ref.record" "$d/got.record"; then
      agree=$((agree + 1))
    else
      differ=$((differ + 1)); echo "  differ: $name"
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/lsp-$name" && cp "$d/ref.record" "$KEEP/lsp-$name/want" && cp "$d/got.record" "$KEEP/lsp-$name/got"
      fi
    fi
    rm -rf "$d" "$d".*
  done
  echo "lsp: $agree agree, $differ differ, $refused refused (of $n sessions)"
  [ "$differ" -eq 0 ] || status=1

  # Every input of check opened as it is on disk — its diagnostics published, then closed — and
  # again with hover at a few positions, and with what a client asks of forty of its names: definition, hover, references,
  # rename, symbols (tests/lsp-session.py writes the sessions).
  command -v python3 > /dev/null || { echo "lsp corpus: skipped (no python3 to write the sessions)"; return; }
  list="$work/lsp-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/lres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/l-$name"
    mkdir -p "$d"
    verdict=agree
    why=""
    for mode in open hover nav flat syms symsflat; do
      if [ $mode = open ]; then m=""; else m=$mode; fi
      if ! python3 -I "$R5_ROOT/toolchain/commands/tests/lsp-session.py" "$R5_ROOT/$f" $m > "$d/$mode.in"; then verdict=skipped; break; fi
      (cd "$R5_ROOT" && { rc=0; "$R5_REF" lsp < "$d/$mode.in" > "$d/ref.$mode" 2>&1 || rc=$?; echo "exit $rc" >> "$d/ref.$mode"; })
      (cd "$R5_ROOT" && { rc=0; timeout "$R5_DEADLINE" "$R5_PORT" lsp < "$d/$mode.in" > "$d/got.$mode" 2>&1 || rc=$?; echo "exit $rc" >> "$d/got.$mode"; })
      if grep -q "^exit 6$" "$d/got.$mode"; then verdict=refused; why=$(grep -o "lsp: not carried.*" "$d/got.$mode"); break; fi
      if grep -q "^exit 124$" "$d/got.$mode"; then verdict=timeout; break; fi
      if ! cmp -s "$d/ref.$mode" "$d/got.$mode"; then
        verdict=differ
        if [ -n "${KEEP:-}" ]; then
          mkdir -p "$KEEP/lsp-$name" && cp "$d/ref.$mode" "$KEEP/lsp-$name/want.$mode" && cp "$d/got.$mode" "$KEEP/lsp-$name/got.$mode"
        fi
        break
      fi
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/lres/$name"
    rm -rf "$d"
  ' sh
  agree=$(grep -l '^agree$' "$work/lres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/lres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/lres/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/lres/"* 2>/dev/null | wc -l | tr -d ' ')
  skipped=$(grep -l '^skipped$' "$work/lres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/lres/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/lres/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^lsp: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "lsp corpus: $agree agree, $differ differ, $refused refused, $timeout timed out, $skipped not UTF-8 (of $total inputs)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

# --- test: every recorded case, the corpora, the fixture ----------------------------------------------

part_test() {
  list="$work/test-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && git ls-files '*.expected' | sed 's/\.expected$/.nz/' | while IFS= read -r f; do [ -f "$f" ] && echo "$f"; done | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/tres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-300}"
  # One input: `test --interpret-only FILE` and with `--json`, from the repository's root.
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/t-$name"
    mkdir -p "$d"
    run() {
      out=$1; shift
      (cd "$R5_ROOT" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; cat "$out.o" "$out.e" >> "$out"; })
    }
    run "$d/ref.text" "$R5_REF" test --interpret-only "$f"
    run "$d/ref.json" "$R5_REF" test --interpret-only --json "$f"
    run "$d/got.text" timeout "$R5_DEADLINE" "$R5_PORT" test --interpret-only "$f"
    run "$d/got.json" timeout "$R5_DEADLINE" "$R5_PORT" test --interpret-only --json "$f"
    verdict=agree
    why=""
    for k in text json; do
      if grep -q "^exit 6$" "$d/got.$k"; then verdict=refused; why=$(cat "$d/got.$k.e"); break; fi
      if grep -q "^exit 124$" "$d/got.$k"; then verdict=timeout; break; fi
      cmp -s "$d/ref.$k" "$d/got.$k" || verdict=differ
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/tres/$name"
    if [ "$verdict" = differ ] || [ "$verdict" = timeout ]; then
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/test-$name" && for k in text json; do cp "$d/ref.$k" "$KEEP/test-$name/want.$k"; cp "$d/got.$k" "$KEEP/test-$name/got.$k"; done
      fi
    fi
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/tres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/tres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/tres/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/tres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/tres/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/tres/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^test: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "test: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total recorded cases)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1

  # The corpora whole and the fixture, compiled legs included, and the other forms of the command.
  n=0 agree=0 differ=0 refused=0
  for spec in "compiler/conformance|" "compiler/conformance|--summary-json" "compiler/conformance|--json" "FIXTURE|" "FIXTURE|--json" \
      "FIXTURE|--summary-json" "FIXTURE|--interpret-only --filter units::" "FIXTURE|--interpret-only --summary-json --fuzz 7" \
      "FIXTURE|--filter ok" "FIXTURE|--interpret-only --filter zzz" "FIXTURE|--interpret-only --json --fuzz 3"; do
    n=$((n + 1))
    target=${spec%%|*} opts=${spec#*|}
    d="$work/tw/$n"
    (fixture test "$d/ref"); (fixture test "$d/got")
    if [ "$target" = FIXTURE ]; then at_ref="$d/ref" at_got="$d/got" arg=.; else at_ref=$root at_got=$root arg=$target; fi
    # shellcheck disable=SC2086
    (cd "$at_ref" && { rc=0; "$ref" test $opts $arg > "$d/ref.o" 2> "$d/ref.e" || rc=$?; echo "exit $rc" > "$d/ref.r"; })
    # shellcheck disable=SC2086
    (cd "$at_got" && { rc=0; "$port" test $opts $arg > "$d/got.o" 2> "$d/got.e" || rc=$?; echo "exit $rc" > "$d/got.r"; })
    if grep -q '^exit 6$' "$d/got.r"; then
      refused=$((refused + 1)); echo "  refused: test $opts $target"
    elif cmp -s "$d/ref.o" "$d/got.o" && cmp -s "$d/ref.e" "$d/got.e" && cmp -s "$d/ref.r" "$d/got.r"; then
      agree=$((agree + 1))
    else
      differ=$((differ + 1)); echo "  differ: test $opts $target"
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/testwhole-$n" && cat "$d/ref.o" "$d/ref.e" "$d/ref.r" > "$KEEP/testwhole-$n/want" && cat "$d/got.o" "$d/got.e" "$d/got.r" > "$KEEP/testwhole-$n/got"
      fi
    fi
    rm -rf "$d"
  done
  echo "test corpora: $agree agree, $differ differ, $refused refused (of $n runs)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- interface: every input -----------------------------------------------------------------------------

part_interface() {
  list="$work/interface-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/ires"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # One input: `interface FILE` and `interface --hash FILE` from the repository's root.
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/i-$name"
    mkdir -p "$d"
    run() {
      out=$1; shift
      (cd "$R5_ROOT" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; cat "$out.o" "$out.e" >> "$out"; })
    }
    run "$d/ref.text" "$R5_REF" interface "$f"
    run "$d/ref.hash" "$R5_REF" interface --hash "$f"
    run "$d/got.text" timeout "$R5_DEADLINE" "$R5_PORT" interface "$f"
    run "$d/got.hash" timeout "$R5_DEADLINE" "$R5_PORT" interface --hash "$f"
    verdict=agree
    why=""
    for k in text hash; do
      if grep -q "^exit 6$" "$d/got.$k"; then verdict=refused; why=$(cat "$d/got.$k.e"); break; fi
      if grep -q "^exit 124$" "$d/got.$k"; then verdict=timeout; break; fi
      cmp -s "$d/ref.$k" "$d/got.$k" || verdict=differ
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/ires/$name"
    if [ "$verdict" = differ ] || [ "$verdict" = timeout ]; then
      if [ -n "${KEEP:-}" ]; then
        mkdir -p "$KEEP/interface-$name" && for k in text hash; do cp "$d/ref.$k" "$KEEP/interface-$name/want.$k"; cp "$d/got.$k" "$KEEP/interface-$name/got.$k"; done
      fi
    fi
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/ires/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/ires/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/ires/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/ires/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/ires/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/ires/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^interface: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "interface: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total inputs)"
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

# --- docs: every section, and corpora with something wrong ---------------------------------------------

part_docs() {
  n=0 agree=0 differ=0 refused=0
  ids="$work/docs-ids.txt"
  "$ref" docs --index --root "$root" | cut -f1 | LC_ALL=C sort -u > "$ids"
  total=$(wc -l < "$ids" | tr -d ' ')
  mkdir -p "$work/docres"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-120}"
  # The whole index in each form, then every section of the corpus by its id: `--section ID --json`,
  # and the text form of every tenth.
  # shellcheck disable=SC2016
  { printf '%s\n' "--index --json" "--index" "--index --document spec" "--index --document nope" "--index --document grammar --json"; \
    i=0; while IFS= read -r id; do i=$((i + 1)); printf '%s\n' "--section $id --json"; [ $((i % 10)) -eq 0 ] && printf '%s\n' "--section $id"; done < "$ids"; } \
    | tr '\n' '\0' | xargs -0 -n 1 -P "$jobs" sh -c '
    line=$1
    name=$(printf "%s" "$line" | tr " /:" "___")
    d="$R5_WORK/docres/$name"
    mkdir -p "$d"
    # shellcheck disable=SC2086
    (cd "$R5_ROOT" && { rc=0; "$R5_REF" docs $line --root "$R5_ROOT" > "$d/ref.o" 2> "$d/ref.e" || rc=$?; echo "exit $rc" > "$d/ref.r"; })
    # shellcheck disable=SC2086
    (cd "$R5_ROOT" && { rc=0; timeout "$R5_DEADLINE" "$R5_PORT" docs $line --root "$R5_ROOT" > "$d/got.o" 2> "$d/got.e" || rc=$?; echo "exit $rc" > "$d/got.r"; })
    if grep -q "^exit 6$" "$d/got.r"; then v=refused
    elif cmp -s "$d/ref.o" "$d/got.o" && cmp -s "$d/ref.e" "$d/got.e" && cmp -s "$d/ref.r" "$d/got.r"; then v=agree
    else
      v=differ
      if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP/docs-$name" && cp "$d/ref.o" "$KEEP/docs-$name/want" && cp "$d/got.o" "$KEEP/docs-$name/got"; fi
    fi
    echo "$v" > "$R5_WORK/docres/$name.v"
    rm -rf "$d"
  ' sh
  agree=$(grep -l '^agree$' "$work/docres/"*.v 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/docres/"*.v 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/docres/"*.v 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l '^differ$' "$work/docres/"*.v 2>/dev/null | head -20); do echo "  differ: $(basename "$r" .v)"; done
  echo "docs: $agree agree, $differ differ, $refused refused (of $((agree + differ + refused)) runs over $total sections)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1

  # Corpora with one thing wrong: each document's problem, as the reference names it.
  n=0 agree=0 differ=0 refused=0
  for variant in missing nonutf8 duplicate broken-link fence setext anchor no-title grammar generated empty-identity detached lettered; do
    n=$((n + 1))
    d="$work/dv/$variant"
    mkdir -p "$d/docs/user" "$d/docs/internal"
    cp "$root"/docs/user/*.md "$root/docs/user/grammar.ebnf" "$d/docs/user/"
    cp "$root"/docs/internal/*.md "$d/docs/internal/"
    case $variant in
      missing) rm "$d/docs/user/mcp.md" ;;
      nonutf8) printf '\377\n' >> "$d/docs/internal/runbook.md" ;;
      duplicate) printf '\n## Settled\n\ntext\n' >> "$d/docs/user/spec.md" ;;
      broken-link) printf '\n[x](spec.md#nope-nothing)\n' >> "$d/docs/internal/runbook.md" ;;
      fence) printf '\n```\ncode\n' >> "$d/docs/user/mcp.md" ;;
      setext) printf '\nsome text\nTitle\n=====\n' >> "$d/docs/internal/bootstrap.md" ;;
      anchor) printf '\n<!-- id: foo -->\n' >> "$d/docs/user/mcp.md" ;;
      no-title) { printf 'x\n'; cat "$root/docs/internal/runbook.md"; } > "$d/docs/internal/runbook.md" ;;
      grammar) printf '\nbad line\n' >> "$d/docs/user/grammar.ebnf" ;;
      generated) sed 's/generated from docs\/user\/grammar.ebnf: begin/generated: begin/' "$root/docs/user/guide.md" > "$d/docs/user/guide.md" ;;
      empty-identity) printf '\n## —\n\ntext\n' >> "$d/docs/user/mcp.md" ;;
      detached) printf '\n(* a comment\n   alone *)\n\n(* another *)\nzz = "a" ;\n' >> "$d/docs/user/grammar.ebnf" ;;
      lettered) printf '\n## A1 — a lettered label\n\ntext\n\n## A1 — the same again\n\ntext\n' >> "$d/docs/user/mcp.md" ;;
    esac
    for form in "--index --json" "--section spec" "--section mcp:x --json"; do
      # shellcheck disable=SC2086
      (cd "$d" && { rc=0; "$ref" docs $form --root . > ref.o 2> ref.e || rc=$?; echo "exit $rc" > ref.r; })
      # shellcheck disable=SC2086
      (cd "$d" && { rc=0; "$port" docs $form --root . > got.o 2> got.e || rc=$?; echo "exit $rc" > got.r; })
      if grep -q '^exit 6$' "$d/got.r"; then
        refused=$((refused + 1)); echo "  refused: $variant $form: $(head -c 120 "$d/got.e")"
      elif cmp -s "$d/ref.o" "$d/got.o" && cmp -s "$d/ref.e" "$d/got.e" && cmp -s "$d/ref.r" "$d/got.r"; then
        agree=$((agree + 1))
      else
        differ=$((differ + 1)); echo "  differ: $variant $form"
        if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP/docsv-$variant" && cp "$d/ref.o" "$KEEP/docsv-$variant/want" && cp "$d/got.o" "$KEEP/docsv-$variant/got"; fi
      fi
    done
    rm -rf "$d"
  done
  echo "docs corpora: $agree agree, $differ differ, $refused refused (of $((n * 3)) runs over $n variants)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- bindgen: every header ---------------------------------------------------------------------------------

part_bindgen() {
  n=0 agree=0 differ=0 refused=0
  # The headers, and one with CRLF line ends, written here: a tracked file would not keep its CRs.
  mkdir -p "$work/hdr"
  cp "$root"/toolchain/commands/tests/bindgen/*.h "$work/hdr/"
  printf '\r\nint64_t crlf(int64_t a);\r\nstruct H;\r\n' > "$work/hdr/crlf.h"
  for h in "$work"/hdr/*.h "$root"/bench/compare/c/chan.h "$root"/bench/compare/cpp/chan.h "$@"; do
    [ -f "$h" ] || continue
    for form in "" "-o OUT"; do
      n=$((n + 1))
      d="$work/bg/$n"
      mkdir -p "$d/ref" "$d/got"
      cp "$h" "$d/ref/h.h"; cp "$h" "$d/got/h.h"
      # shellcheck disable=SC2086
      (cd "$d/ref" && { rc=0; "$ref" bindgen h.h ${form%OUT}${form:+out.nz} > ../ref.o 2> ../ref.e || rc=$?; echo "exit $rc" > ../ref.r; })
      # shellcheck disable=SC2086
      (cd "$d/got" && { rc=0; "$port" bindgen h.h ${form%OUT}${form:+out.nz} > ../got.o 2> ../got.e || rc=$?; echo "exit $rc" > ../got.r; })
      if grep -q '^exit 6$' "$d/got.r"; then
        refused=$((refused + 1)); echo "  refused: $(basename "$h") $form: $(head -c 100 "$d/got.e")"
      elif cmp -s "$d/ref.o" "$d/got.o" && cmp -s "$d/ref.e" "$d/got.e" && cmp -s "$d/ref.r" "$d/got.r" && diff -r "$d/ref" "$d/got" > /dev/null; then
        agree=$((agree + 1))
      else
        differ=$((differ + 1)); echo "  differ: $(basename "$h") $form"
        if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP/bindgen-$n" && cp "$d/ref.o" "$KEEP/bindgen-$n/want" && cp "$d/got.o" "$KEEP/bindgen-$n/got"; fi
      fi
      rm -rf "$d"
    done
  done
  echo "bindgen: $agree agree, $differ differ, $refused refused (of $n runs)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- reload-check: every pair of programs -----------------------------------------------------------------

part_reload() {
  n=0 agree=0 differ=0 refused=0
  # The programs, and one with CRLF line ends, written here: a tracked file would not keep its CRs.
  dir="$work/rlsrc"
  mkdir -p "$dir"
  cp "$root"/toolchain/commands/tests/reload/*.nz "$dir/"
  printf 'fn add(a: Int, b: Int) -> Int {\r\n    a + b\r\n}\r\nfn main() -> Int { add(2, 2) }\r\n' > "$dir/crlf.nz"
  for x in "$dir"/*.nz; do
    for y in "$dir"/*.nz; do
      n=$((n + 1))
      d="$work/rl"
      mkdir -p "$d"
      (cd "$dir" && { rc=0; "$ref" reload-check "$(basename "$x")" "$(basename "$y")" > "$d/ref.o" 2> "$d/ref.e" || rc=$?; echo "exit $rc" > "$d/ref.r"; })
      (cd "$dir" && { rc=0; "$port" reload-check "$(basename "$x")" "$(basename "$y")" > "$d/got.o" 2> "$d/got.e" || rc=$?; echo "exit $rc" > "$d/got.r"; })
      if grep -q '^exit 6$' "$d/got.r"; then
        refused=$((refused + 1)); echo "  refused: $(basename "$x") $(basename "$y"): $(head -c 100 "$d/got.e")"
      elif cmp -s "$d/ref.o" "$d/got.o" && cmp -s "$d/ref.e" "$d/got.e" && cmp -s "$d/ref.r" "$d/got.r"; then
        agree=$((agree + 1))
      else
        differ=$((differ + 1)); echo "  differ: $(basename "$x") $(basename "$y")"
        if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP/reload-$n" && cp "$d/ref.o" "$KEEP/reload-$n/want" && cp "$d/got.o" "$KEEP/reload-$n/got"; fi
      fi
      rm -rf "$d"
    done
  done
  # Records and enums are compared by their layout too; a generic one is refused (the reference lists it for each instance).
  echo "reload-check: $agree agree, $differ differ, $refused refused (of $n pairs)"
  [ "$differ" -eq 0 ] && [ "$refused" -eq 0 ] || status=1
}

# --- agent: the commands that answer from the language service's packets -----------------------------------------

# For every input of check (or the FILEs named), from the repository's root: `snapshot` and `diagnostics` (compact and indented),
# `diagnostics --detail` of up to three of the index's ids, `delta` against the snapshot the reference wrote and against two
# baselines made wrong (a digest changed, a definition renamed), and — `patch.nz` — `patch rename` at six names and `patch fix`
# at up to three diagnostics that carry a fix (design D24).
part_agent() {
  list="$work/agent-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/ares" "$work/akeys"
  export R5_ROOT="$root" R5_REF="$ref" R5_PORT="$port" R5_WORK="$work" R5_DEADLINE="${R5_DEADLINE:-60}"
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    d="$R5_WORK/a-$name"
    mkdir -p "$d"
    keys=""
    run() {
      out=$1; shift
      (cd "$R5_ROOT" && { rc=0; "$@" > "$out.o" 2> "$out.e" || rc=$?; echo "exit $rc" > "$out"; cat "$out.o" "$out.e" >> "$out"; })
    }
    # `ask KEY ARGS...`: the command on each side, to be compared.
    ask() {
      key=$1; shift
      run "$d/ref.$key" "$R5_REF" "$@"
      run "$d/got.$key" timeout "$R5_DEADLINE" "$R5_PORT" "$@"
      keys="$keys $key"
    }
    ask snap snapshot --json --root "$f"
    ask snapi snapshot --root "$f"
    ask diag diagnostics --json --root "$f"
    ask diagi diagnostics --root "$f"
    # `askc`: as `ask`, each side with a cold check cache of its own (`resolve` and `references` run the cached check).
    askc() {
      key=$1; shift
      rm -rf "$d/cr" "$d/cg"
      run "$d/ref.$key" env NAZM_CACHE_DIR="$d/cr" "$R5_REF" "$@"
      run "$d/got.$key" env NAZM_CACHE_DIR="$d/cg" timeout "$R5_DEADLINE" "$R5_PORT" "$@"
      keys="$keys $key"
    }
    askc resolve resolve "$f"
    (cd "$R5_ROOT" && "$R5_REF" snapshot --json --root "$f" > "$d/base.json" 2> /dev/null)
    if [ -s "$d/base.json" ]; then
      # `references` of the first three definitions the reference lists (and of a variant of one that is not there).
      grep -o "\"identity\":\"[^\"]*\"" "$d/base.json" | head -3 | sed "s/\"identity\":\"\(.*\)\"/\1/" > "$d/ids"
      n=0
      while IFS= read -r id; do
        n=$((n + 1))
        askc "refs$n" references "$id" "$f"
      done < "$d/ids"
      # A baseline with the first digest changed, and one whose first definition has another name.
      sed "s/\"source\":\"\([0-9a-e]\)/\"source\":\"f/" "$d/base.json" > "$d/tweak.json"
      sed "s/\"identity\":\"\([^\"]*\)::fn \([a-z_]*\)\"/\"identity\":\"\1::fn zz_\2\"/" "$d/base.json" > "$d/renamed.json"
      ask delta delta --json --root "$f" --baseline "$d/base.json"
      ask deltat delta --root "$f" --baseline "$d/tweak.json"
      ask deltar delta --json --root "$f" --baseline "$d/renamed.json"
    fi
    (cd "$R5_ROOT" && "$R5_REF" diagnostics --json --root "$f" > "$d/index.json" 2> /dev/null)
    state=$(grep -o "\"source\":\"[0-9a-f]*\"" "$d/index.json" | head -1 | sed "s/\"source\":\"\(.*\)\"/\1/")
    if [ -n "$state" ]; then
      n=0
      for id in $(grep -o "\"id\":\"[^\"]*\"" "$d/index.json" | head -3 | sed "s/\"id\":\"\(.*\)\"/\1/"); do
        n=$((n + 1))
        ask "detail$n" diagnostics --json --root "$f" --detail "$id" --state "$state"
      done
    fi
    # The positions: names to rename, diagnostics with a fix.
    if command -v python3 > /dev/null; then
      (cd "$R5_ROOT" && "$R5_REF" check --json --no-cache "$f" 2>&1 >/dev/null | python3 -I "$R5_ROOT/toolchain/commands/tests/patch-positions.py" "$R5_ROOT/$f") > "$d/positions" 2> /dev/null || true
      n=0
      while read -r kind byte new; do
        [ -n "$kind" ] || continue
        n=$((n + 1))
        if [ "$kind" = rename ]; then ask "rename$n" patch rename --json --root "$f" --file "$f" --byte "$byte" --new-name "$new"; fi
        if [ "$kind" = fix ]; then ask "fix$n" patch fix --json --root "$f" --file "$f" --byte "$byte"; fi
      done < "$d/positions"
    fi
    # Each command compared on its own: differ over timeout over refused over agree is the verdict on the input.
    verdict=agree
    why=""
    : > "$R5_WORK/akeys/$name"
    for k in $keys; do
      v=agree
      if grep -q "^exit 6$" "$d/got.$k"; then
        v=refused
        [ -n "$why" ] || why=$(head -1 "$d/got.$k.e")
      elif grep -q "^exit 124$" "$d/got.$k"; then
        v=timeout
      elif ! cmp -s "$d/ref.$k" "$d/got.$k"; then
        v=differ
        if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP/agent-$name" && cp "$d/ref.$k" "$KEEP/agent-$name/want.$k" && cp "$d/got.$k" "$KEEP/agent-$name/got.$k"; fi
      fi
      echo "$(printf "%s" "$k" | sed "s/[0-9]*$//") $v" >> "$R5_WORK/akeys/$name"
      case $v in
        differ) verdict=differ ;;
        timeout) [ "$verdict" = differ ] || verdict=timeout ;;
        refused) [ "$verdict" = agree ] && verdict=refused ;;
      esac
    done
    printf "%s\n%s\n" "$verdict" "$why" > "$R5_WORK/ares/$name"
    rm -rf "$d"
  ' sh

  agree=$(grep -l '^agree$' "$work/ares/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ$' "$work/ares/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused$' "$work/ares/"* 2>/dev/null | wc -l | tr -d ' ')
  timeout=$(grep -l '^timeout$' "$work/ares/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l -E '^(differ|timeout)$' "$work/ares/"* 2>/dev/null | head -20); do
    echo "  $(head -1 "$r"): $(basename "$r")"
  done
  for r in "$work/ares/"*; do [ "$(head -1 "$r")" = refused ] && sed -n 2p "$r"; done \
    | sed 's/^[a-z]*: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  echo "agent: $agree agree, $differ differ, $refused refused, $timeout timed out (of $total inputs)"
  # And by command: each run of a command on an input.
  cat "$work/akeys/"* | awk '{ n[$1] ++; v[$1 " " $2] ++ } END { for (k in n) printf "  %-10s %6d runs: %d agree, %d differ, %d refused, %d timed out\n", k, n[k], v[k " agree"], v[k " differ"], v[k " refused"], v[k " timeout"] }' | sort
  [ "$differ" -eq 0 ] && [ "$timeout" -eq 0 ] || status=1
}

# --- mcp: the server over stdio ---------------------------------------------------------------------------------

part_mcp() {
  mref=${NAZM_MCP_REF:-"$(dirname "$ref")/nazm-mcp"}
  [ -x "$mref" ] || { echo "run.sh mcp: no nazm-mcp at $mref (set NAZM_MCP_REF)" >&2; status=1; return; }
  if [ -n "${NAZM_MCP_PORT:-}" ]; then
    mport=$NAZM_MCP_PORT
  else
    stage_build "$nazm" --stack-size 256M "$root/toolchain/commands/mcp.nz" -o "$work/bin/nazm-mcp" > "$work/build-mcp.log" 2>&1 \
      || { cat "$work/build-mcp.log" >&2; echo "run.sh: toolchain/commands/mcp.nz did not build" >&2; exit 2; }
    mport="$work/bin/nazm-mcp"
  fi
  command -v python3 > /dev/null || { echo "run.sh mcp: python3 is needed" >&2; status=1; return; }
  list="$work/mcp-inputs.txt"
  if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$list"
  else
    (cd "$root" && { git ls-files '*.nz'; find toolchain/frontend-tests/inputs toolchain/commands -name '*.nz' -type f; } | LC_ALL=C sort -u) > "$list"
  fi
  total=$(wc -l < "$list" | tr -d ' ')
  mkdir -p "$work/mres"
  export NAZM_REF="$ref" NAZM_PORT="$port" R5_ROOT="$root" R5_MREF="$mref" R5_MPORT="$mport" R5_WORK="$work"
  (cd "$root" && python3 -I "$root/toolchain/commands/tests/mcp-compare.py" protocol "$mref" "$mport" "$root") > "$work/mres-protocol" 2>&1 || true
  echo "  protocol: $(head -1 "$work/mres-protocol")"
  (cd "$root" && python3 -I "$root/toolchain/commands/tests/mcp-compare.py" nodocs "$mref" "$mport" "$root" examples/max.nz) > "$work/mres-nodocs" 2>&1 || true
  echo "  no --docs: $(head -1 "$work/mres-nodocs")"
  [ "$(head -1 "$work/mres-nodocs")" = agree ] || status=1
  # The command line: every line of tests/args/mcp.txt, both servers started with it and an empty input.
  cn=0 ca=0
  while IFS= read -r line; do
    case $line in '#'*|'') continue ;; esac
    cn=$((cn + 1))
    set -f
    if [ "$line" = "(none)" ]; then set --; else set -- $line; fi
    set +f
    (cd "$root" && { rc=0; "$mref" "$@" < /dev/null > "$work/c.ref" 2> "$work/c.ref.e" || rc=$?; echo "exit $rc" >> "$work/c.ref"; })
    (cd "$root" && { rc=0; "$mport" "$@" < /dev/null > "$work/c.got" 2> "$work/c.got.e" || rc=$?; echo "exit $rc" >> "$work/c.got"; })
    if cmp -s "$work/c.ref" "$work/c.got" && cmp -s "$work/c.ref.e" "$work/c.got.e"; then ca=$((ca + 1)); else echo "  differ: nazm-mcp $line"; fi
  done < "$root/toolchain/commands/tests/args/mcp.txt"
  echo "  command lines: $ca of $cn agree"
  [ "$ca" -eq "$cn" ] || status=1
  # shellcheck disable=SC2016
  tr '\n' '\0' < "$list" | xargs -0 -n 1 -P "$jobs" sh -c '
    f=$1
    name=$(printf "%s" "$f" | tr "/" "_")
    (cd "$R5_ROOT" && python3 -I "$R5_ROOT/toolchain/commands/tests/mcp-compare.py" file "$R5_MREF" "$R5_MPORT" "$R5_ROOT" "$f") > "$R5_WORK/mres/$name" 2>&1 || true
  ' sh
  agree=$(grep -l '^agree$' "$work/mres/"* 2>/dev/null | wc -l | tr -d ' ')
  differ=$(grep -l '^differ' "$work/mres/"* 2>/dev/null | wc -l | tr -d ' ')
  refused=$(grep -l '^refused' "$work/mres/"* 2>/dev/null | wc -l | tr -d ' ')
  for r in $(grep -l '^differ' "$work/mres/"* 2>/dev/null | head -20); do
    echo "  differ: $(basename "$r")"; head -3 "$r" | cut -c1-300
  done
  for r in "$work/mres/"*; do case $(head -1 "$r") in refused*) head -1 "$r" ;; esac; done \
    | sed 's/^refused: //; s/^nazm[.a-z_]* answers with an error rather than a result: //; s/^[a-z]*: not carried: //' | cut -c1-90 | sort | uniq -c | sort -rn | head -12 | sed 's/^/  refused:/'
  other=$((total - agree - differ - refused))
  echo "mcp: $agree agree, $differ differ, $refused refused, $other other (of $total inputs); the protocol session: $(head -1 "$work/mres-protocol" | cut -c1-20)"
  [ "$differ" -eq 0 ] && [ "$other" -eq 0 ] && [ "$(head -1 "$work/mres-protocol")" = agree ] || status=1
}

for p in $parts; do
  case $p in
    fix) part_fix "$@" ;;
    fmt) part_fmt "$@" ;;
    args) part_args ;;
    init) part_init ;;
    check) part_check "$@" ;;
    doc) part_doc "$@" ;;
    lsp) part_lsp "$@" ;;
    test) part_test "$@" ;;
    docs) part_docs ;;
    reload) part_reload ;;
    bindgen) part_bindgen "$@" ;;
    interface) part_interface "$@" ;;
    agent) part_agent "$@" ;;
    mcp) part_mcp "$@" ;;
  esac
done
exit $status
