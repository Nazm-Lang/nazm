#!/bin/bash
# R8 slice 12: the Nazm `nazm` runs from its release archive alone (`openspec/changes/r8-cut-over/design.md`, *Slice 12*).
#
#     toolchain/commands/tests/archive.sh [FILE.nz ...]
#
# The Nazm `nazm` ($NAZM_PORT, or built by $NAZM, the checkout's Rust `nazm`, from toolchain/commands/nazm.nz) is copied into an
# archive layout (`nazm-TRIPLE/bin/nazm`, as `scripts/host-archive.sh` writes it) in a scratch directory with NO checkout beside it:
# (`bin/` on PATH, as INSTALL.txt says; `libexec/nazm/{nazm-pipeline,nzcg}` beside it) every command is run from a scratch work directory that holds only copies of the inputs, with an empty environment (no
# NAZM_LIBRARY, no HOME's cache, no repository). The reference is $NAZM_REF (default $NAZM), run the same way. A case agrees
# when standard output, standard error and the exit status are the same bytes; a status 6 from the Nazm side is a refusal,
# counted apart.
#
#   cases   for each input: check, check --json, interface, doc, and run (the top-level programs of examples/);
#           then `docs --index --json`, `docs --index`, and `docs --section` of a few ids, with no --root: the corpus the binary
#           carries against the reference's checkout (equal while the carried corpus is the checkout's).
#
# Nazm `nazm` runs are `host-run`s, one at a time (design, S12); the Rust side runs directly.
set -u
stage_name=archive-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${ARCHIVE_OUT:-${TMPDIR:-/tmp}/nazm-archive-compare}
rm -rf "$OUT/archive" "$OUT/work" "$OUT/home"
mkdir -p "$OUT/bin" "$OUT/archive/nazm-aarch64-apple-darwin/bin" "$OUT/work" "$OUT/home"
if [ -z "${NAZM_PORT:-}" ]; then
    stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/nazm.nz" -o "$OUT/bin/nazm" > "$OUT/bin/nazm.log" 2>&1 \
        || { echo "archive-compare: nazm did not build:"; cat "$OUT/bin/nazm.log"; exit 2; }
fi
if [ -z "${NAZM_PIPELINE:-}" ]; then
    stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/pipeline/main.nz" -o "$OUT/bin/nazm-pipeline" > "$OUT/bin/pipeline.log" 2>&1 \
        || { echo "archive-compare: nazm-pipeline did not build:"; cat "$OUT/bin/pipeline.log"; exit 2; }
fi
if [ -z "${NAZM_NZCG:-}" ]; then
    stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/codegen/main.nz" -o "$OUT/bin/nzcg" > "$OUT/bin/nzcg.log" 2>&1 \
        || { echo "archive-compare: nzcg did not build:"; cat "$OUT/bin/nzcg.log"; exit 2; }
fi
PORT=${NAZM_PORT:-$OUT/bin/nazm}
NZCG=${NAZM_NZCG:-$OUT/bin/nzcg}
PIPELINE=${NAZM_PIPELINE:-$OUT/bin/nazm-pipeline}
mkdir -p "$OUT/archive/nazm-aarch64-apple-darwin/libexec/nazm"
cp "$PORT" "$OUT/archive/nazm-aarch64-apple-darwin/bin/nazm"
cp "$PIPELINE" "$OUT/archive/nazm-aarch64-apple-darwin/libexec/nazm/nazm-pipeline"
cp "$NZCG" "$OUT/archive/nazm-aarch64-apple-darwin/libexec/nazm/nzcg"
ARCH_NAZM=$OUT/archive/nazm-aarch64-apple-darwin/bin/nazm
cp -R "$ROOT/examples" "$OUT/work/examples"
cp "$ROOT/README.md" "$OUT/work/README.md"
files=("$@")
if [ ${#files[@]} -eq 0 ]; then
    files=()
    while IFS= read -r f; do files+=("$f"); done < <(cd "$OUT/work" && find examples -name '*.nz' | sort)
fi
EMPTY_ENV=(env -i PATH=/usr/bin:/bin HOME="$OUT/home" TMPDIR="$OUT/home")
agree=0 differ=0 refused=0 total=0
run_case() { # LABEL ARGS...
    local label=$1; shift
    total=$((total + 1))
    (cd "$OUT/work" && "${EMPTY_ENV[@]}" "$REF" "$@" > "$OUT/ref.out" 2> "$OUT/ref.err"; echo $? > "$OUT/ref.st")
    (cd "$OUT/work" && env -i PATH="$OUT/archive/nazm-aarch64-apple-darwin/bin:/usr/bin:/bin" HOME="$OUT/home" TMPDIR="$OUT/home" STAGE_MODE="$STAGE_MODE" STAGE_XTASK="$STAGE_XTASK" STAGE_PEAKS="$STAGE_PEAKS" "$STAGE_RUN" 120 "$ARCH_NAZM" "$@" > "$OUT/got.out" 2> "$OUT/got.err"; echo $? > "$OUT/got.st")
    if [ "$(cat "$OUT/got.st")" = 6 ] && [ "$(cat "$OUT/ref.st")" != 6 ]; then
        refused=$((refused + 1)); echo "REFUSED $label: $(head -c 200 "$OUT/got.err")"
    elif cmp -s "$OUT/ref.out" "$OUT/got.out" && cmp -s "$OUT/ref.err" "$OUT/got.err" && cmp -s "$OUT/ref.st" "$OUT/got.st"; then
        agree=$((agree + 1))
    else
        differ=$((differ + 1)); echo "DIFFER $label (ref $(cat "$OUT/ref.st"), got $(cat "$OUT/got.st"))"
        if [ -n "${KEEP:-}" ]; then mkdir -p "$KEEP"; n=$(echo "$label" | tr -c 'A-Za-z0-9\n' _); cp "$OUT/ref.out" "$KEEP/$n.ref.out"; cp "$OUT/got.out" "$KEEP/$n.got.out"; cp "$OUT/ref.err" "$KEEP/$n.ref.err"; cp "$OUT/got.err" "$KEEP/$n.got.err"; fi
    fi
}
for f in "${files[@]}"; do
    run_case "check $f" check --no-cache "$f"
    run_case "check --json $f" check --json --no-cache "$f"
    run_case "interface $f" interface "$f"
    run_case "doc $f" doc "$f"
    case $f in examples/*/*) ;; *) run_case "run $f" run "$f" ;; esac
done
run_case "docs --index --json" docs --index --json
run_case "docs --index" docs --index
for id in spec guide spec:the-standard-library grammar:expr; do run_case "docs --section $id" docs --section "$id" --json; done
echo "archive-compare: agree $agree, differ $differ, refused $refused, of $total"
[ "$differ" -eq 0 ]
