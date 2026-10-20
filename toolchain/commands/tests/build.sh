#!/bin/bash
# R8 slices 4 and 5: `nazm build` written in Nazm against the Rust one, on Nazm's own code
# generator (`--backend nazm`, slice 4) and on the default LLVM backend (slice 5)
# (`openspec/changes/r8-cut-over/design.md`, *Slice 4*, S4.6, and *Slice 5*).
#
#     toolchain/commands/tests/build.sh [FILE.nz ...]
#     BUILD_MODES="exe objects" toolchain/commands/tests/build.sh ...
#
# The Nazm `nazm` (toolchain/commands/nazm.nz, whose `build` is build.nz), the pipeline and the code
# generator are built by the checkout's `nazm` ($NAZM) with a heap limit of at most 2 GiB
# (`toolchain/lib/stage.sh`); the reference is the Rust `nazm` ($NAZM_REF, by default $NAZM). Each
# build runs one at a time, as the R6 rule allows on the macOS host: the Nazm one through
# `stage-run.sh` (one `host-run`, its stages its children: design S4.2), the Rust one directly,
# with its `nzcg` (`NAZM_NZCG`) a wrapper that runs the generator through `host-run`, one at a time.
#
# For every input (by default `tests/compat/v1/{run,trap,refuse,check}/*.nz`,
# `compiler/conformance/*.nz` and `toolchain/codegen/tests/programs/*.nz`) and every mode:
#
#   exe       `build --backend nazm --no-cache FILE -o prog`
#   heap      the same with `--heap-limit 64M`
#   objects   `--objects objs` for the host's target
#   x86       `--objects objs --target x86_64-apple-darwin`
#   windows   `--objects objs --target x86_64-pc-windows-gnu`
#   cached    `-o prog` without `--no-cache`, on the Nazm side twice over one fresh object store
#             (NAZM_CACHE_DIR): the second build, from the store, must still be the reference's
#
# and, on the default backend (LLVM text through clang; no `--backend` on either side), every mode
# a `llvm-` prefix names:
#
#   llvm          `-o prog`
#   llvm-heap     the same with `--heap-limit 64M`
#   llvm-objects  `--objects objs`; llvm-x86 and llvm-windows with `--target x86_64-apple-darwin`
#                 and `--target x86_64-pc-windows-gnu`
#   llvm-x86exe   `--target x86_64-apple-darwin -o prog` (linked, run under Rosetta only)
#   llvm-ir       `--emit-ir --objects objs`: every unit's LLVM text beside the output; llvm-irexe
#                 the same linked
#   llvm-O1, llvm-O2, llvm-O3
#                 `--opt-level N -o prog` (`-O2` and above link bitcode as one program)
#   llvm-debug    `--debug --emit-ir --objects objs`: the units' text and objects with DWARF
#   llvm-debugexe `--debug -o prog`, with `prog.dSYM`: the executable and the bundle name the build's
#                 working directory (`prog.nazm-PID/`, in the debug map), so they are compared
#                 through `dwarfdump --debug-info --debug-line` (UUIDs left out), not as bytes
#   llvm-release  `--release -o prog`: canaries, position independent, stripped, signed
#   llvm-soa      `--layout soa --emit-ir --objects objs` (soa: the same on `--backend nazm`)
#
# both sides run in directories of their own, the file named by its absolute path, and agree when
# standard output, standard error, the exit status and every file written (the executable, or every
# object and `link.txt`) are the same bytes. The Nazm side's refusal (status 6: what it does not
# carry) is counted apart, with its reason, never as agreeing. No program built here is run: they
# are built without a heap limit, which the host rule does not admit; identical bytes are the
# agreement. Prints a line per input and mode that does not agree, then
# `build: N agree, M differ, K refused`. Exit status 0 only when nothing differs.
set -u
stage_name=build-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${BUILD_OUT:-${TMPDIR:-/tmp}/nazm-build-compare}
MODES=${BUILD_MODES:-exe heap objects x86 windows cached llvm llvm-heap llvm-objects llvm-x86 llvm-windows llvm-x86exe llvm-ir llvm-irexe llvm-O1 llvm-O2 llvm-O3 llvm-debug llvm-debugexe llvm-release soa llvm-soa}
mkdir -p "$OUT/bin"
for p in nazm:toolchain/commands/nazm.nz nazm-pipeline:toolchain/pipeline/main.nz nzcg:toolchain/codegen/main.nz; do
    name=${p%%:*}
    if ! stage_build "$NAZM" "$ROOT/${p#*:}" -o "$OUT/bin/$name" > "$OUT/bin/$name.log" 2>&1; then
        echo "build-compare: $name did not build:"
        cat "$OUT/bin/$name.log"
        exit 2
    fi
done
# The reference's generator: `nzcg` through `host-run`, one at a time (the Rust build runs one per
# unit, side by side); contained, directly.
cat > "$OUT/bin/nzcg-ref" << EOF
#!/bin/sh
if [ "\${STAGE_MODE:-}" = host ]; then
    while ! mkdir "$OUT/bin/nzcg.lock" 2> /dev/null; do sleep 0.05; done
    trap 'rmdir "$OUT/bin/nzcg.lock"' EXIT
    "$STAGE_RUN" 300 "$OUT/bin/nzcg" "\$@"
    exit \$?
fi
exec "$OUT/bin/nzcg" "\$@"
EOF
chmod +x "$OUT/bin/nzcg-ref"
rmdir "$OUT/bin/nzcg.lock" 2> /dev/null

if [ $# -gt 0 ]; then
    inputs=("$@")
else
    inputs=()
    for f in "$ROOT"/tests/compat/v1/run/*.nz "$ROOT"/tests/compat/v1/trap/*.nz "$ROOT"/tests/compat/v1/refuse/*.nz \
        "$ROOT"/tests/compat/v1/check/*.nz "$ROOT"/compiler/conformance/*.nz "$ROOT"/toolchain/codegen/tests/programs/*.nz; do
        [ -f "$f" ] && inputs+=("$f")
    done
fi

agree=0
differ=0
refused=0
for src in "${inputs[@]}"; do
    case $src in /*) ;; *) src=$PWD/$src ;; esac
    rel=${src#"$ROOT"/}
    n=$(echo "$rel" | tr '/.' '__')
    for mode in $MODES; do
        W=$OUT/work/$n.$mode
        rm -rf "$W"
        mkdir -p "$W/ref" "$W/gen"
        backend=(--backend nazm)
        case $mode in
            exe | cached) args=(-o prog) ;;
            heap) args=(--heap-limit 64M -o prog) ;;
            objects) args=(--objects objs) ;;
            x86) args=(--objects objs --target x86_64-apple-darwin) ;;
            windows) args=(--objects objs --target x86_64-pc-windows-gnu) ;;
            soa) args=(--layout soa --objects objs) ;;
            llvm-soa) args=(--layout soa --emit-ir --objects objs); backend=() ;;
            llvm) args=(-o prog); backend=() ;;
            llvm-heap) args=(--heap-limit 64M -o prog); backend=() ;;
            llvm-objects) args=(--objects objs); backend=() ;;
            llvm-x86) args=(--objects objs --target x86_64-apple-darwin); backend=() ;;
            llvm-windows) args=(--objects objs --target x86_64-pc-windows-gnu); backend=() ;;
            llvm-x86exe) args=(--target x86_64-apple-darwin -o prog); backend=() ;;
            llvm-ir) args=(--emit-ir --objects objs); backend=() ;;
            llvm-irexe) args=(--emit-ir -o prog); backend=() ;;
            llvm-O1) args=(--opt-level 1 -o prog); backend=() ;;
            llvm-O2) args=(--opt-level 2 -o prog); backend=() ;;
            llvm-O3) args=(--opt-level 3 -o prog); backend=() ;;
            llvm-debug) args=(--debug --emit-ir --objects objs); backend=() ;;
            llvm-debugexe) args=(--debug -o prog); backend=() ;;
            llvm-release) args=(--release -o prog); backend=() ;;
            *) echo "build-compare: no mode $mode"; exit 2 ;;
        esac
        (cd "$W/ref" && NAZM_NZCG="$OUT/bin/nzcg-ref" "$REF" build ${backend[@]+"${backend[@]}"} --no-cache "${args[@]}" "$src" > ../ref.out 2> ../ref.err < /dev/null
            echo $? > ../ref.status)
        cache=--no-cache
        rounds=1
        if [ "$mode" = cached ]; then cache=""; rounds=2; fi
        for round in $(seq 1 $rounds); do
            rm -f "$W/gen/prog"
            (cd "$W/gen" && NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline" NAZM_NZCG="$OUT/bin/nzcg" \
                NAZM_CACHE_DIR="$W/store" "$STAGE_RUN" 900 "$OUT/bin/nazm" build ${backend[@]+"${backend[@]}"} $cache "${args[@]}" "$src" > ../gen.out 2> ../gen.err
                echo $? > ../gen.status)
        done
        if [ "$(cat "$W/gen.status")" = 6 ]; then
            refused=$((refused + 1))
            echo "refused  $rel [$mode]: $(head -c 300 "$W/gen.err")"
            continue
        fi
        why=""
        # A debug executable's bytes hold its build's own directory (its pid): the DWARF is compared.
        if [ "$mode" = llvm-debugexe ] && [ -f "$W/ref/prog" ] && [ -f "$W/gen/prog" ]; then
            for side in ref gen; do
                dwarfdump --debug-info --debug-line "$W/$side/prog.dSYM" 2>&1 | grep -v 'UUID' | tail -n +2 > "$W/$side.dwarf"
                rm -rf "$W/$side/prog" "$W/$side/prog.dSYM"
            done
            cmp -s "$W/ref.dwarf" "$W/gen.dwarf" || why="$why dwarf"
        fi
        for x in out err status; do
            cmp -s "$W/ref.$x" "$W/gen.$x" || why="$why $x"
        done
        diff -r "$W/ref" "$W/gen" > "$W/files.diff" 2>&1 || why="$why files"
        if [ -z "$why" ]; then
            agree=$((agree + 1))
            rm -rf "$W"
        else
            differ=$((differ + 1))
            echo "differ   $rel [$mode]:$why (status $(cat "$W/ref.status") vs $(cat "$W/gen.status"))"
            for x in out err; do
                cmp -s "$W/ref.$x" "$W/gen.$x" || diff "$W/ref.$x" "$W/gen.$x" | head -6 | cut -c1-200 | sed 's/^/    /'
            done
            head -4 "$W/files.diff" | cut -c1-200 | sed 's/^/    /'
        fi
    done
done
echo "build: $agree agree, $differ differ, $refused refused"
[ "$differ" -eq 0 ]
