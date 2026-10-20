#!/bin/bash
# R8 slices 4, 5 and 6: `nazm build` written in Nazm against the Rust one, on Nazm's own code
# generator (`--backend nazm`, slice 4), on the default LLVM backend (slice 5) and in the rest of its
# flags and forms (slice 6) (`openspec/changes/r8-cut-over/design.md`, *Slice 4*, S4.6, *Slice 5* and *Slice 6*).
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
# A `host-run` that is refused because another checkout's is running (`another-stage-running`) is asked again.
#
# For every input (by default `tests/compat/v1/{run,trap,refuse,check}/*.nz`,
# `compiler/conformance/*.nz` and `toolchain/codegen/tests/programs/*.nz`) and every mode:
#
#   exe       `build --backend nazm --no-cache FILE -o prog`
#   heap      the same with `--heap-limit 64M`
#   objects   `--objects objs` for the host's target
#   x86       `--objects objs --target x86_64-apple-darwin`
#   windows   `--objects objs --target x86_64-pc-windows-gnu`
#   cached    `-o prog` without `--no-cache`, twice on each side over a fresh object store of its own
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
#   (BUILD_TOOLCHAIN=1 adds the toolchain's own programs as inputs)
#   llvm-soa      `--layout soa --emit-ir --objects objs` (soa: the same on `--backend nazm`)
#
# and, slice 6's (each side with a fresh object store of its own, NAZM_CACHE_DIR):
#
#   report        `--build-report --no-cache -o prog`: `nazm.build-report/1` on standard output
#   report-objects `--build-report --no-cache --objects objs` (`link.performed` false); report-nazm the same
#                 on `--backend nazm`
#   report-cached `--build-report -o prog` twice on each side, over the store of its own: the first build files
#                 every unit and the second reuses them (`reused`, `stored`); report-cached-nazm the same
#   summary       `--summary-json -o prog`: `nazm.command-summary/1` alone on standard output
#   timings       `--timings -o prog`: `nazm.timings/1` on standard error, whose times are the machine's: the
#                 line must have the format and the phases' names are not compared; everything else must agree
#   lib, lib-shared  `--lib -o libx.a` and `--lib --shared -o libx.dylib`: the archive (or the library, which
#                 holds the build's directory, so is compared through `nm`) and its header
#   lib-objects   `--lib --objects objs`
#   release-lib   `--release --lib`: the usage refusal
#   rtlink        `--runtime rt -o prog` against the runtime artifact `nazm runtime build` made (by the
#                 reference, then by the Nazm side: the two must be the same bytes)
#   prov          `--provenance p.json --sbom s.json -o prog`: both documents, and the build
#   prov-sign     `--provenance p.json --attest-with CMD -o prog`: the signer's `p.json.sig`
#   link          `--link foreign.o -o prog` for the inputs that call C (the corpus's ffi programs)
#
# both sides run in directories of their own, the file named by its absolute path, and agree when
# standard output, standard error, the exit status and every file written (the executable, or every
# object and `link.txt`) are the same bytes. The Nazm side's refusal (status 6: what it does not
# carry) is counted apart, with its reason, never as agreeing. No program built here is run: they
# are built without a heap limit, which the host rule does not admit; identical bytes are the
# agreement. Prints a line per input and mode that does not agree, then
# `build: N agree, M differ, K refused`. Exit status 0 only when nothing differs.
#
# The command lines (`BUILD_ARGS=1`, or the part `args`): every line of tests/args/build.txt, run by both
# sides as `nazm LINE` in the same fixture directory (a.nz builds, b.nz does not, w.nz is warned of, m.nz
# imports a file that is not there, lib.nz is a library, the packages of the repository's examples as
# pkg/…): clap's usage errors and help, the refusals for the targets the cut-over removes, the package builds,
# `runtime build` and `runtime verify`; the same agreement. `BUILD_PART=args` runs only these.
set -u
stage_name=build-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${BUILD_OUT:-${TMPDIR:-/tmp}/nazm-build-compare}
PART=${BUILD_PART:-all}
MODES=${BUILD_MODES:-exe heap objects x86 windows cached llvm llvm-heap llvm-objects llvm-x86 llvm-windows llvm-x86exe llvm-ir llvm-irexe llvm-O1 llvm-O2 llvm-O3 llvm-debug llvm-debugexe llvm-release soa llvm-soa report report-objects report-nazm report-cached report-cached-nazm summary timings lib lib-shared lib-objects rtlink prov link}
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
    tries=0
    while :; do
        "$STAGE_RUN" 300 "$OUT/bin/nzcg" "\$@" > "$OUT/bin/nzcg.out" 2> "$OUT/bin/nzcg.err"
        status=\$?
        if [ "\$status" -ne 0 ] && [ "\$tries" -lt 900 ] && grep -q 'another-stage-running' "$OUT/bin/nzcg.err"; then
            tries=\$((tries + 1))
            sleep 2
            continue
        fi
        break
    done
    cat "$OUT/bin/nzcg.out"
    cat "$OUT/bin/nzcg.err" >&2
    exit \$status
fi
exec "$OUT/bin/nzcg" "\$@"
EOF
chmod +x "$OUT/bin/nzcg-ref"
rmdir "$OUT/bin/nzcg.lock" 2> /dev/null

# `run_gen DIR OUTFILE ERRFILE STATUSFILE ARGS...`: the Nazm `nazm` in DIR through one `host-run`, asked again
# while another checkout's `host-run` is the one running.
run_gen() {
    rg_dir=$1
    rg_out=$2
    rg_err=$3
    rg_status=$4
    shift 4
    rg_tries=0
    while :; do
        (cd "$rg_dir" && NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline" NAZM_NZCG="$OUT/bin/nzcg" \
            NAZM_CACHE_DIR="$rg_dir/../gen-store" "$STAGE_RUN" 900 "$OUT/bin/nazm" "$@" > "$rg_out" 2> "$rg_err"
            echo $? > "$rg_status")
        if [ "$(cat "$rg_status")" -ne 0 ] && [ "$rg_tries" -lt 900 ] && grep -q 'another-stage-running' "$rg_err"; then
            rg_tries=$((rg_tries + 1))
            sleep 2
            continue
        fi
        break
    done
}

# `run_ref DIR OUTFILE ERRFILE STATUSFILE ARGS...`: the Rust `nazm` in DIR.
run_ref() {
    rr_dir=$1
    rr_out=$2
    rr_err=$3
    rr_status=$4
    shift 4
    (cd "$rr_dir" && NAZM_NZCG="$OUT/bin/nzcg-ref" NAZM_CACHE_DIR="$rr_dir/../ref-store" "$REF" "$@" > "$rr_out" 2> "$rr_err" < /dev/null
        echo $? > "$rr_status")
}

# `norm_timings FILE`: the `nazm.timings/1` line of FILE, when it has the format, replaced by `TIMINGS`.
norm_timings() {
    sed -E 's/^\{"schema":"nazm\.timings\/1","phases":\[(\{"phase":"[a-z_]+","ms":[0-9]+\.[0-9]{3}\},?)+\],"total_ms":[0-9]+\.[0-9]{3}\}$/TIMINGS/' "$1" > "$1.n"
    mv "$1.n" "$1"
}

agree=0
differ=0
refused=0

# `settle LABEL W [ROUNDS]`: compare what the two sides did in W (ref.*, gen.* per round and the files left in
# W/ref and W/gen); count it and say so when it differs.
settle() {
    label=$1
    W=$2
    rounds=${3:-1}
    if [ "$(cat "$W/gen.status.1")" = 6 ]; then
        refused=$((refused + 1))
        echo "refused  $label: $(head -c 300 "$W/gen.err.1")"
        return
    fi
    why=""
    r=1
    while [ "$r" -le "$rounds" ]; do
        # The two sides work in directories of their own: a path in what they say is the same where it names its own.
        for x in out err; do
            sed "s#$W/ref#@DIR@#g" "$W/ref.$x.$r" > "$W/ref.$x.$r.n"
            mv "$W/ref.$x.$r.n" "$W/ref.$x.$r"
            sed "s#$W/gen#@DIR@#g" "$W/gen.$x.$r" > "$W/gen.$x.$r.n"
            mv "$W/gen.$x.$r.n" "$W/gen.$x.$r"
        done
        for x in out err status; do
            cmp -s "$W/ref.$x.$r" "$W/gen.$x.$r" || why="$why $x.$r"
        done
        r=$((r + 1))
    done
    diff -r "$W/ref" "$W/gen" > "$W/files.diff" 2>&1 || why="$why files"
    if [ -z "$why" ]; then
        agree=$((agree + 1))
        rm -rf "$W"
    else
        differ=$((differ + 1))
        echo "differ   $label:$why (status $(cat "$W/ref.status.1") vs $(cat "$W/gen.status.1"))"
        r=1
        while [ "$r" -le "$rounds" ]; do
            for x in out err; do
                cmp -s "$W/ref.$x.$r" "$W/gen.$x.$r" || diff "$W/ref.$x.$r" "$W/gen.$x.$r" | head -6 | cut -c1-200 | sed 's/^/    /'
            done
            r=$((r + 1))
        done
        head -4 "$W/files.diff" | cut -c1-200 | sed 's/^/    /'
    fi
}

if [ "$PART" != args ]; then
if [ $# -gt 0 ]; then
    inputs=("$@")
else
    inputs=()
    for f in "$ROOT"/tests/compat/v1/run/*.nz "$ROOT"/tests/compat/v1/trap/*.nz "$ROOT"/tests/compat/v1/refuse/*.nz \
        "$ROOT"/tests/compat/v1/check/*.nz "$ROOT"/compiler/conformance/*.nz "$ROOT"/toolchain/codegen/tests/programs/*.nz; do
        [ -f "$f" ] && inputs+=("$f")
    done
    # BUILD_TOOLCHAIN=1 adds the toolchain's own programs (the builder and the others of the self-build):
    # large, several modules each, one of them reached through `..`.
    if [ "${BUILD_TOOLCHAIN:-}" = 1 ]; then
        for f in toolchain/commands/nazm.nz toolchain/pipeline/main.nz toolchain/codegen/main.nz compiler/diagnose.nz \
            toolchain/commands/mcp.nz toolchain/harness/mutate/main.nz toolchain/harness/tools/jtool.nz \
            toolchain/interp/main.nz toolchain/harness/check/main.nz; do
            inputs+=("$ROOT/$f")
        done
    fi
fi

# The foreign object the `link` mode hands both sides: C's `ffi_add`, whatever the corpus's ffi programs call.
mkdir -p "$OUT/foreign"
if [ ! -f "$OUT/foreign/foreign.o" ]; then
    printf 'long ffi_add(long a, long b) { return a + b; }\nlong nz_c_add(long a, long b) { return a + b; }\n' > "$OUT/foreign/foreign.c"
    (cd "$OUT/foreign" && clang -c -o foreign.o foreign.c 2> /dev/null)
fi

# The runtime artifact the `rtlink` mode links: built once by the reference, and once by the Nazm side.
rt_ready=0

for src in "${inputs[@]}"; do
    case $src in /*) ;; *) src=$PWD/$src ;; esac
    rel=${src#"$ROOT"/}
    n=$(echo "$rel" | tr '/.' '__')
    for mode in $MODES; do
        W=$OUT/work/$n.$mode
        rm -rf "$W"
        mkdir -p "$W/ref" "$W/gen"
        backend=(--backend nazm)
        rounds=1
        nocache=(--no-cache)
        pre=()
        case $mode in
            exe) args=(-o prog) ;;
            cached) args=(-o prog); nocache=(); rounds=2 ;;
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
            report) args=(--build-report -o prog); backend=() ;;
            report-objects) args=(--build-report --objects objs); backend=() ;;
            report-nazm) args=(--build-report -o prog) ;;
            report-cached) args=(--build-report -o prog); backend=(); nocache=(); rounds=2 ;;
            report-cached-nazm) args=(--build-report -o prog); nocache=(); rounds=2 ;;
            llvm-O2-cached) args=(--opt-level 2 --build-report -o prog); backend=(); nocache=(); rounds=2 ;;
            summary) args=(--summary-json -o prog); backend=() ;;
            timings) args=(--timings -o prog); backend=() ;;
            lib) args=(--lib -o libx.a); backend=() ;;
            lib-shared) args=(--lib --shared -o libx.dylib); backend=() ;;
            lib-objects) args=(--lib --objects objs); backend=() ;;
            lib-ir) args=(--lib --emit-ir -o libx.a); backend=() ;;
            lib-windows) args=(--lib --target x86_64-pc-windows-gnu -o libx.a); backend=() ;;
            lib-x86) args=(--lib --target x86_64-apple-darwin -o libx.a); backend=() ;;
            lib-nazm) args=(--lib -o libx.a) ;;
            rtlink) args=(--runtime "$OUT/rt" -o prog); backend=() ;;
            prov) args=(--provenance p.json --sbom s.json -o prog); backend=() ;;
            prov-sign) args=(--provenance p.json --attest-with cat -o prog); backend=() ;;
            prov-lib) args=(--lib --provenance p.json --sbom s.json -o libx.a); backend=() ;;
            prov-objects) args=(--objects objs --provenance p.json --sbom s.json); backend=() ;;
            link) args=(--link "$OUT/foreign/foreign.o" -o prog); backend=() ;;
            *) echo "build-compare: no mode $mode"; exit 2 ;;
        esac
        case $mode in
            rtlink)
                if [ "$rt_ready" = 0 ]; then
                    rm -rf "$OUT/rt" "$OUT/rt-gen"
                    (cd "$OUT" && "$REF" runtime build --out rt > /dev/null 2>&1)
                    mkdir -p "$OUT/rt-gen"
                    (cd "$OUT/rt-gen" && NAZM_LIBRARY="$ROOT/library" NAZM_PIPELINE="$OUT/bin/nazm-pipeline" "$STAGE_RUN" 900 "$OUT/bin/nazm" runtime build --out rt > /dev/null 2>&1)
                    if ! diff -r "$OUT/rt" "$OUT/rt-gen/rt" > "$OUT/rt.diff" 2>&1; then
                        echo "differ   runtime build [rtlink]: $(head -c 300 "$OUT/rt.diff")"
                        differ=$((differ + 1))
                    fi
                    rt_ready=1
                fi
                ;;
        esac
        r=1
        while [ "$r" -le "$rounds" ]; do
            [ "$r" -gt 1 ] && rm -f "$W/ref/prog" "$W/gen/prog"
            run_ref "$W/ref" "$W/ref.out.$r" "$W/ref.err.$r" "$W/ref.status.$r" build ${backend[@]+"${backend[@]}"} ${nocache[@]+"${nocache[@]}"} "${args[@]}" "$src"
            r=$((r + 1))
        done
        # A cached build's generated side runs one round more when asked (the second must be the reference's too).
        r=1
        while [ "$r" -le "$rounds" ]; do
            [ "$r" -gt 1 ] && rm -f "$W/gen/prog"
            run_gen "$W/gen" "$W/gen.out.$r" "$W/gen.err.$r" "$W/gen.status.$r" build ${backend[@]+"${backend[@]}"} ${nocache[@]+"${nocache[@]}"} "${args[@]}" "$src"
            r=$((r + 1))
        done
        if [ "$mode" = timings ]; then
            for side in ref gen; do norm_timings "$W/$side.err.1"; done
        fi
        label="$rel [$mode]"
        # A debug executable's bytes hold its build's own directory (its pid): the DWARF is compared.
        if [ "$mode" = llvm-debugexe ] && [ -f "$W/ref/prog" ] && [ -f "$W/gen/prog" ]; then
            for side in ref gen; do
                dwarfdump --debug-info --debug-line "$W/$side/prog.dSYM" 2>&1 | grep -v 'UUID' | tail -n +2 > "$W/$side.dwarf"
                rm -rf "$W/$side/prog" "$W/$side/prog.dSYM"
            done
            cmp -s "$W/ref.dwarf" "$W/gen.dwarf" || echo "differ   $label: dwarf" > "$W/dwarf.differs"
        fi
        # A shared library names the build's directory (`@rpath` and the debug map): its exported symbols, then.
        if [ "$mode" = lib-shared ] && [ -f "$W/ref/libx.dylib" ] && [ -f "$W/gen/libx.dylib" ]; then
            for side in ref gen; do
                nm -gU "$W/$side/libx.dylib" > "$W/$side.nm" 2>&1
                rm -f "$W/$side/libx.dylib"
            done
            cmp -s "$W/ref.nm" "$W/gen.nm" || echo "differ   $label: nm" > "$W/dwarf.differs"
        fi
        if [ -f "$W/dwarf.differs" ]; then
            cat "$W/dwarf.differs"
            differ=$((differ + 1))
            rm -rf "$W"
            continue
        fi
        settle "$label" "$W" "$rounds"
    done
done
fi

# The command lines of tests/args/build.txt, in a fixture both sides share.
if [ "$PART" != all ] && [ "$PART" != args ]; then
    PART=none
fi
if [ "$PART" = all ] || [ "$PART" = args ]; then
    ARGS=$ROOT/toolchain/commands/tests/args/build.txt
    FX=$OUT/fixture
    mkdir -p "$FX"
    fixture() {
        fx_dir=$1
        mkdir -p "$fx_dir"
        cp "$ROOT/tests/compat/v1/run/enums.nz" "$fx_dir/a.nz"
        printf 'fn main() -> Int {\n    let x: Bool = 1;\n    x\n}\n' > "$fx_dir/b.nz"
        printf 'enum E { A, B, C }\nfn main() -> Int {\n    match E.A {\n        E.A => 0,\n        _ => 1,\n    }\n}\n' > "$fx_dir/w.nz"
        printf 'use "gone.nz";\nfn main() -> Int { 0 }\n' > "$fx_dir/m.nz"
        printf 'fn square(x: Int) -> Int ! {} { x * x }\n\npub extern "C" fn sum_squares(a: Int, b: Int) -> Int ! {} = "nazm_sum_squares" {\n    square(a) + square(b)\n}\n\npub extern "C" fn is_even(n: Int) -> Bool ! {} = "nazm_is_even" { n %% 2 == 0 }\n' > "$fx_dir/lib.nz"
        mkdir -p "$fx_dir/pkg"
        cp -R "$ROOT/examples/packages/app" "$fx_dir/pkg/app"
        cp -R "$ROOT/examples/packages/geometry" "$fx_dir/pkg/geometry"
        mkdir -p "$fx_dir/sub"
        printf 'fn main() -> Int { 0 }\n' > "$fx_dir/sub/x.nz"
        # A runtime artifact for the host, built once by the reference, and three that are not whole.
        if [ ! -d "$OUT/rt-fixture" ]; then
            (cd "$OUT" && "$REF" runtime build --out rt-fixture > /dev/null 2>&1)
        fi
        cp -R "$OUT/rt-fixture" "$fx_dir/rt"
        mkdir -p "$fx_dir/rt-bad"
        printf 'not json\n' > "$fx_dir/rt-bad/nazm-runtime.json"
        cp -R "$fx_dir/rt" "$fx_dir/rt-abi"
        sed 's/"abi":[0-9]*/"abi":9999/' "$fx_dir/rt/nazm-runtime.json" > "$fx_dir/rt-abi/nazm-runtime.json"
        cp -R "$fx_dir/rt" "$fx_dir/rt-archive"
        printf 'x' >> "$fx_dir/rt-archive/libnazmrt.a"
    }
    if [ -f "$ARGS" ]; then
        k=0
        while IFS= read -r line; do
            case $line in '#'* | '') continue ;; esac
            # BUILD_FILTER=REGEX runs only the lines it matches.
            if [ -n "${BUILD_FILTER:-}" ] && ! echo "$line" | grep -Eq "$BUILD_FILTER"; then continue; fi
            k=$((k + 1))
            W=$OUT/work/args.$k
            rm -rf "$W"
            mkdir -p "$W"
            fixture "$W/ref"
            fixture "$W/gen"
            # shellcheck disable=SC2086
            run_ref "$W/ref" "$W/ref.out.1" "$W/ref.err.1" "$W/ref.status.1" $line
            # shellcheck disable=SC2086
            run_gen "$W/gen" "$W/gen.out.1" "$W/gen.err.1" "$W/gen.status.1" $line
            for side in ref gen; do norm_timings "$W/$side.err.1"; done
            settle "nazm $line" "$W" 1
        done < "$ARGS"
    fi
fi
echo "build: $agree agree, $differ differ, $refused refused"
[ "$differ" -eq 0 ]
