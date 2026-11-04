#!/bin/bash
# R8 slice 16 — the self-build on a macOS host, one serial step at a time
# (`openspec/changes/r8-cut-over/design.md`, *Slice 16*, S16.1 and S16.2).
#
#     toolchain/selfbuild/host-selfbuild.sh aarch64|x86_64 OUT
#
# The script is a shell loop. It starts **no Nazm-built program itself**: every such step (a
# `nazm build` by a stage's builder, a front end, the interpreter, `nazm-check`, a corpus
# program) is one `host-run` (`toolchain/lib/stage-run.sh`: 2 GiB counted heap at most, a deadline,
# the peak resident memory appended to `OUT/peaks.txt`), strictly one after another. Stage 0 is
# the seed, the Rust `nazm`, which is not a Nazm program and is run directly. A refusal, or status
# 70 (the rule withdrawn), from any step stops the loop and the run has failed; a withdrawal marker
# is left for a person. `SELFBUILD_BACKEND=nazm` runs the loop on `--backend nazm`; the default is
# the released LLVM backend. `SELFBUILD_DRY_RUN=1` prints what would run and runs nothing.
#
# The Linux self-build (`selfbuild.sh`, contained) is a different script; the two share only
# `programs.txt`. Nothing is timed.

set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage_name=host-selfbuild
. "$root/toolchain/lib/stage.sh"

say() { printf '%s\n' "$*"; }
# Descriptor 3 is the script's own standard error, kept whatever a step redirects: a refusal or a
# failure is written there, so a step's `2> /dev/null` can never swallow it (the first host run
# lost a `no-counted-heap` refusal that way).
exec 3>&2
# `fail` also stops the run from inside a subshell: it signals the script itself, which exits 1.
fail() { printf 'host-selfbuild: FAILED: %s\n' "$*" >&3; kill -TERM $$; exit 1; }
trap 'exit 1' TERM

[ $# -eq 2 ] || stage_fail "usage: host-selfbuild.sh aarch64|x86_64 OUT"
arch=$1
out=$2
backend=${SELFBUILD_BACKEND:-llvm}
dry=${SELFBUILD_DRY_RUN:-}

# --- the refusals (S16.1): nothing has run when any of these stops the script ------------------
[ "$(uname -s)" = Darwin ] || stage_fail "refused [not-macos]: the host self-build is for macOS; the Linux one is \`cargo xtask contained selfbuild\`"
[ -z "${NAZM_CONTAINED:-}" ] || stage_fail "refused [host-environment]: NAZM_CONTAINED is set in the host's environment; unset it"
[ -z "${NAZM_ALLOW_UNCONTAINED:-}" ] || stage_fail "refused [host-environment]: NAZM_ALLOW_UNCONTAINED is set; it is never accepted"
if [ -n "$(docker ps -q --filter label=nazm.owner=nazm-contained-runner 2> /dev/null)" ]; then
    stage_fail "refused [contained-workload-running]: a contained workload is running; no host stage beside it"
fi
marker=${NAZM_HOST_RUN_DIR:-$HOME/.cache/nazm-host-run}/withdrawn
[ ! -e "$marker" ] || stage_fail "refused [withdrawn]: the host rule is withdrawn ($(head -c 300 "$marker")); marker: $marker; remove it only after a person has investigated"
case $arch in
    aarch64) target=aarch64-apple-darwin ;;
    x86_64) target=x86_64-apple-darwin ;;
    *) stage_fail "refused [arch]: ARCH is aarch64 or x86_64, not '$arch'" ;;
esac
case $backend in
    llvm | nazm) ;;
    *) stage_fail "refused [backend]: SELFBUILD_BACKEND is llvm or nazm, not '$backend'" ;;
esac
if [ "$arch" = x86_64 ]; then
    arch -x86_64 /usr/bin/true > /dev/null 2>&1 || stage_fail "refused [no-rosetta]: Rosetta does not answer (arch -x86_64 /usr/bin/true)"
fi
[ -n "$out" ] || stage_fail "usage: OUT is empty"
if [ -e "$out" ] && [ -n "$(ls -A "$out" 2> /dev/null)" ]; then
    stage_fail "refused [out-exists]: $out is not empty; give a new directory"
fi
[ -f "$root/toolchain/selfbuild/programs.txt" ] || stage_fail "toolchain/selfbuild/programs.txt is missing"

# --- the pinned environment (S16.3) -----------------------------------------------------------------
SDKROOT=$(xcrun --show-sdk-path 2> /dev/null) || stage_fail "xcrun --show-sdk-path failed"
[ -d "$SDKROOT" ] || stage_fail "refused [no-sdk]: the SDK xcrun names ($SDKROOT) is not a directory"
MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
export SDKROOT MACOSX_DEPLOYMENT_TARGET
if [ "$arch" = x86_64 ]; then
    # A translated process that starts the /usr/bin/clang shim cannot use libxcrun (arm64 only):
    # the real compiler first, and the answer probed once from an x86_64 process (S16.4).
    # cargo (which builds xtask, below) stays reachable, last: its directory holds no clang to shadow the real one.
    cargo_dir=$(dirname "$(command -v cargo)")
    PATH=$(dirname "$(xcrun --find clang)"):/usr/bin:/bin:$cargo_dir
    export PATH
    arch -x86_64 sh -c 'clang -target x86_64-apple-darwin -print-target-triple' > /dev/null 2>&1 \
        || stage_fail "refused [rosetta-clang]: clang does not answer from an x86_64 process"
fi
unset NAZM_PIPELINE NAZM_NZCG NAZM_LIBRARY

heap=${STAGE_HEAP:-2G}
mkdir -p "$out" || stage_fail "cannot create $out"
out=$(CDPATH= cd -- "$out" && pwd)
peaks="$out/peaks.txt"
if [ -n "$dry" ]; then
    STAGE_HEAP=$heap
    stage_bytes=$(stage_heap_bytes "$STAGE_HEAP") || stage_fail "STAGE_HEAP=$STAGE_HEAP is not a size"
    [ "$stage_bytes" -gt 0 ] && [ "$stage_bytes" -le 2147483648 ] || stage_fail "STAGE_HEAP=$STAGE_HEAP: at most 2 GiB"
    STAGE_RUN="$root/toolchain/lib/stage-run.sh"
    : > "$peaks"
    export STAGE_HEAP STAGE_RUN
    seed=${NAZM:-$(stage_checkout_nazm "$root")}
else
    STAGE_PEAKS=$peaks
    export STAGE_PEAKS
    stage_init "$root"
    [ "$STAGE_MODE" = host ] || stage_fail "not in host mode"
    seed=${NAZM:-$(stage_checkout_nazm "$root")}
    [ -x "$seed" ] || stage_fail "build the Rust seed first: cargo build -p nazm-cli"
    command -v clang > /dev/null 2>&1 || stage_fail "clang is required"
fi
heap=$STAGE_HEAP
STAGE_MAX_OUTPUT_MIB=${STAGE_MAX_OUTPUT_MIB:-192}
export STAGE_MAX_OUTPUT_MIB

# environment.txt: the toolchain the fixpoint claims (one machine, one toolchain).
{
    echo "script: host-selfbuild.sh $arch (backend $backend, target $target)"
    echo "SDKROOT=$SDKROOT"
    echo "MACOSX_DEPLOYMENT_TARGET=$MACOSX_DEPLOYMENT_TARGET"
    echo "PATH=$PATH"
    echo "uname -m: $(uname -m)"
    echo "sw_vers:"; sw_vers 2>&1
    echo "xcrun --show-sdk-version: $(xcrun --show-sdk-version 2>&1 | tail -1)"
    echo "SDKSettings.json Version: $(plutil -extract Version raw "$SDKROOT/SDKSettings.json" 2>&1 | head -1)"
    echo "SDK realpath: $(realpath "$SDKROOT" 2>&1)"
    echo "clang --version:"; clang --version 2>&1
    echo "ld -v: $(ld -v 2>&1 | head -1)"
    if [ "$arch" = x86_64 ]; then echo "rosetta: arch -x86_64 /usr/bin/true answers; clang target $(arch -x86_64 sh -c 'clang -target x86_64-apple-darwin -print-target-triple' 2>&1)"; else echo "rosetta: not used"; fi
    echo "seed: $("$seed" --version 2> /dev/null)"
    echo "heap limit: $heap"
} > "$out/environment.txt" 2>&1

# The Core IR check gives the front end the seed machine's own platform (the seed's `core-ir` has no --target).
case $(uname -m) in arm64) core_arch=aarch64 ;; *) core_arch=x86_64 ;; esac

# --- the steps ----------------------------------------------------------------------------------
# release configuration: what slice 18's released binaries are built with.
cfg=(--release --backend "$backend" --no-cache --target "$target")
[ "$backend" = nazm ] && say "host-selfbuild: backend nazm (the Linux self-build's configuration), not the released one"

programs=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$')
builder=$(echo "$programs" | awk '$1 == "builder" { printf "%s%s:%s", sep, $2, $3; sep = " " }')
others=$(echo "$programs" | awk '$1 == "other" { printf "%s%s:%s", sep, $2, $3; sep = " " }')
names=$(echo "$builder $others" | tr ' ' '\n' | cut -d: -f1)

# `step LABEL DEADLINE EXE ARGS...`: ONE host-run, labelled in peaks.txt. Standard streams are
# the caller's. A refusal or a withdrawal (status 70) ends the whole run.
errf="$out/.step.err"
step() {
    label=$1; deadline=$2; shift 2
    if [ -n "$dry" ]; then
        printf 'would host-run (%ss): %s\n' "$deadline" "$*" >&2
        return 0
    fi
    printf '# %s\n' "$label" >> "$peaks"
    "$STAGE_RUN" "$deadline" "$@" 2> "$errf" < /dev/null
    st=$?
    if [ "$st" -eq 70 ] || { grep -q '^host-run: refused \[' "$errf" && ! { [ -n "${STEP_ALLOW_NOHEAP:-}" ] && grep -q '^host-run: refused \[no-counted-heap\]' "$errf"; }; }; then
        cat "$errf" >&3
        fail "host-run stopped the run at '$label' (status $st): $(grep -m1 'refused\|WITHDRAWAL' "$errf")"
    fi
    return "$st"
}
refused_name() { sed -n 's/^host-run: refused \[\([a-z0-9-]*\)\].*/\1/p' "$1" | head -1; }

# `build_with BIN DEST NAME SOURCE [HEAP]`: BIN's builder builds SOURCE into DEST/NAME.
build_with() {
    B=$1; dest=$2; name=$3; src=$4; h=${5:-$heap}
    NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$B/pl" NAZM_NZCG="$B/cg" \
        step "$(basename "$dest") $name build" 1800 "$B/nazm" build "${cfg[@]}" --heap-limit "$h" "$root/$src" -o "$dest/$name" \
        > "$dest/$name.build.out" || {
            st=$?
            cp "$errf" "$dest/$name.build.err" 2> /dev/null
            say "  $name: \`nazm build\` failed (status $st): $(head -c 600 "$dest/$name.build.err" 2> /dev/null)"
            return 1
        }
    return 0
}
# (`step` keeps the program's standard error in `$errf`; callers copy it where they need it.)

say "host-selfbuild: target $target, backend $backend, heap limit $heap, release configuration, SDK $SDKROOT, deployment target $MACOSX_DEPLOYMENT_TARGET"

# The seed is the checkout's own `target/debug/nazm`: bring it up to date first (the script built only xtask, so a
# stale seed built stage 0). Not for a seed named by NAZM, and not in a dry run.
if [ -z "${NAZM:-}" ] && [ "${SELFBUILD_DRY_RUN:-}" != 1 ]; then
    (cd "$root" && cargo build --locked -q -p nazm-cli) || fail "stage 0: the seed (cargo build -p nazm-cli) did not build"
fi

# --- stage 0: the seed (Rust, run directly) builds the builder -------------------------------------
# On the seed's default (LLVM) backend whatever SELFBUILD_BACKEND is: the seed is not asked to run `--backend nazm`.
cfg0=(--release --backend llvm --no-cache --target "$target")
mkdir -p "$out/stage0"
for p in $builder; do
    name=${p%%:*}; src=${p#*:}
    if [ -n "$dry" ]; then
        printf 'would run directly (the Rust seed): %s build %s --heap-limit %s %s -o %s\n' "$seed" "${cfg0[*]}" "$heap" "$root/$src" "$out/stage0/$name"
        continue
    fi
    "$seed" build "${cfg0[@]}" --heap-limit "$heap" "$root/$src" -o "$out/stage0/$name" > "$out/stage0/$name.log" 2>&1 \
        || fail "stage 0: the seed did not build $src: $(tail -5 "$out/stage0/$name.log")"
done
say "stage 0: the seed built the builder"

# --- stages 1..3: one host-run per program ------------------------------------------------------------
stages=${SELFBUILD_STAGES:-3}
prev="$out/stage0"
for s in $(seq 1 "$stages"); do
    dest="$out/stage$s"
    mkdir -p "$dest"
    for p in $builder $others; do
        name=${p%%:*}; src=${p#*:}
        build_with "$prev" "$dest" "$name" "$src" || fail "stage $s: $name did not build; the loop stops here"
    done
    say "stage $s: stage $((s - 1))'s builder built $(echo "$names" | wc -l | tr -d ' ') programs"
    prev="$dest"
done
last="$out/stage$stages"
bad=0

if [ -n "$dry" ]; then
    # Show the shape of the later phases on the first corpus program only, then stop.
    f=$(ls "$root"/compiler/conformance/*.nz | head -1)
    rel=${f#"$root"/}
    n=$(echo "$rel" | tr '/.' '__')
    mkdir -p "$out/corpus" "$out/probe"
    printf 'would run directly (the Rust seed): %s build %s --heap-limit 256M %s -o %s\n' "$seed" "${cfg[*]}" "$f" "$out/corpus/$n.ref"
    build_with "$last" "$out/corpus" "$n" "$rel" 256M
    step "corpus $n ref" 60 "$out/corpus/$n.ref"
    step "corpus $n" 60 "$out/corpus/$n"
    step "core fe" 900 "$last/fe" --coredoc-sources "$root/$rel" "$root/library" macos "$core_arch"
    step "interp $n" 60 "$last/interp" --std "$root/library" "$out/corpus/$n.ir.json" "$(basename "$f")"
    step "check" 900 "$last/check" "$root"
    build_with "$last" "$out/probe" fe compiler/diagnose.nz 256M
    step "probe fe-under-256MiB" 900 "$out/probe/fe" --coredoc-sources "$root/toolchain/commands/nazm.nz" "$root/library" macos "$core_arch"
    say "dry run: nothing was run; the comparisons were not made"
    exit 0
fi

# --- S16.2 (1): the fixpoint, every executable whole ------------------------------------------------------
localise() {
    a=$1; b=$2; name=$3
    d="$out/localise/$(basename "$a")-$(basename "$b")-$name"
    mkdir -p "$d"
    cp "$a/$name" "$d/a"; cp "$b/$name" "$d/b"
    { echo "== otool -l (diff) =="; diff <(otool -l "$d/a") <(otool -l "$d/b") | head -60
      echo "== codesign -d -vvv =="; codesign -d -vvv "$d/a" 2>&1 | head -30; echo "--"; codesign -d -vvv "$d/b" 2>&1 | head -30
      echo "== dwarfdump --uuid =="; dwarfdump --uuid "$d/a" 2>&1; dwarfdump --uuid "$d/b" 2>&1
      codesign --remove-signature "$d/a" 2> /dev/null; codesign --remove-signature "$d/b" 2> /dev/null
      if cmp -s "$d/a" "$d/b"; then echo "== without the signature: IDENTICAL (the signature differs) =="; else echo "== without the signature: still differ: $(cmp "$d/a" "$d/b" 2>&1 | head -1) =="; fi
    } > "$d/report.txt" 2>&1
    say "    localised in $d/report.txt"
}
compare_stages() {
    a=$1; b=$2; same=0; differ=0
    for name in $names; do
        if cmp -s "$a/$name" "$b/$name"; then same=$((same + 1)); else differ=$((differ + 1)); say "  differs: $name"; localise "$a" "$b" "$name"; fi
    done
    say "$(basename "$a") vs $(basename "$b"): $same executables identical, $differ differ"
    [ "$differ" -eq 0 ]
}
fix=0
if [ "$stages" -ge 2 ]; then compare_stages "$out/stage1" "$out/stage2" || fix=1; fi
if [ "$stages" -ge 3 ]; then compare_stages "$out/stage2" "$out/stage3" || fix=1; fi

# --- (2): Core IR against the seed ------------------------------------------------------------------------
coreagree=0; corediffer=0
mkdir -p "$out/core"
for p in $builder $others; do
    name=${p%%:*}; src=${p#*:}
    step "core $name front-end" 900 "$last/fe" --coredoc-sources "$root/$src" "$root/library" macos "$core_arch" > "$out/core/$name.json" 2> /dev/null
    [ -s "$out/core/$name.json" ] || { corediffer=$((corediffer + 1)); say "  Core IR of $src: the front end wrote none: $(head -c 600 "$errf" 2> /dev/null)"; continue; }
    "$seed" core-ir --with-sources "$root/$src" 2> /dev/null > "$out/core/$name.ref.json"
    if cmp -s "$out/core/$name.ref.json" "$out/core/$name.json"; then
        coreagree=$((coreagree + 1))
    else
        corediffer=$((corediffer + 1))
        say "  Core IR of $src differs from the reference's: $(diff "$out/core/$name.ref.json" "$out/core/$name.json" | head -6 | cut -c1-160)"
    fi
done
rm -rf "$out/core"
say "core-ir: $coreagree documents identical to the reference's, $corediffer differ"

# --- (3): behaviour against the seed, the corpus at --heap-limit 256M --------------------------------------
corpus="$out/corpus"
mkdir -p "$corpus"
agree=0; disagree=0; unbuilt=0; noheap=0; noheap_names=""
for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
    [ -f "$f" ] || continue
    rel=${f#"$root"/}
    n=$(echo "$rel" | tr '/.' '__')
    if ! "$seed" build "${cfg[@]}" --heap-limit 256M "$f" -o "$corpus/$n.ref" > "$corpus/$n.ref.build" 2>&1; then
        continue  # the seed does not build it: not an input
    fi
    if ! build_with "$last" "$corpus" "$n" "$rel" 256M > "$corpus/$n.build" 2>&1; then
        unbuilt=$((unbuilt + 1)); say "  not built by stage $stages: $rel: $(head -c 300 "$corpus/$n.build")"; continue
    fi
    (cd "$corpus" && STEP_ALLOW_NOHEAP=1 step "corpus $n ref" 60 "./$n.ref" > "$n.ref.stdout" 2> /dev/null; echo $? > "$n.ref.status"; cp "$errf" "$n.ref.stderr")
    (cd "$corpus" && STEP_ALLOW_NOHEAP=1 step "corpus $n" 60 "./$n" > "$n.stdout" 2> /dev/null; echo $? > "$n.status"; cp "$errf" "$n.stderr")
    rn=$(refused_name "$corpus/$n.ref.stderr"); sn=$(refused_name "$corpus/$n.stderr")
    if [ "$rn" = no-counted-heap ] || [ "$sn" = no-counted-heap ]; then
        noheap=$((noheap + 1)); noheap_names="$noheap_names $rel"
        continue
    fi
    if cmp -s "$corpus/$n.ref.stdout" "$corpus/$n.stdout" && cmp -s "$corpus/$n.ref.stderr" "$corpus/$n.stderr" \
        && cmp -s "$corpus/$n.ref.status" "$corpus/$n.status"; then
        agree=$((agree + 1))
    else
        disagree=$((disagree + 1))
        say "  disagrees with the seed's build: $rel (status $(cat "$corpus/$n.ref.status") vs $(cat "$corpus/$n.status"))"
        for x in stdout stderr; do
            cmp -s "$corpus/$n.ref.$x" "$corpus/$n.$x" || say "    $x: $(diff "$corpus/$n.ref.$x" "$corpus/$n.$x" | head -6 | cut -c1-200)"
        done
    fi
done
for r in $noheap_names; do say "  not run on the host (no counted heap): $r"; done
say "corpus: $agree programs built by stage $stages run as the seed's builds do, $disagree disagree, $unbuilt not built, $noheap not run on the host (no counted heap; counted apart, never as agreeing)"

# --- (4): the interpreter and nazm-check -------------------------------------------------------------------
iagree=0; idisagree=0
for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
    [ -f "$f" ] || continue
    rel=${f#"$root"/}
    n=$(echo "$rel" | tr '/.' '__')
    dir=$(dirname "$f"); file=$(basename "$f")
    if ! (cd "$dir" && "$seed" core-ir --json "$file" > "$corpus/$n.ir.json" 2> /dev/null) \
        || grep -q '^{"schema":"nazm.core-ir/[23]","entry":null' "$corpus/$n.ir.json"; then
        continue
    fi
    (cd "$dir" && perl -e 'alarm shift; exec @ARGV or exit 127' 60 "$seed" run --interpret "$file" > "$corpus/$n.iref.stdout" 2> "$corpus/$n.iref.stderr" < /dev/null; echo $? > "$corpus/$n.iref.status")
    (cd "$dir" && step "interp $n" 60 "$last/interp" --std "$root/library" "$corpus/$n.ir.json" "$file" > "$corpus/$n.i.stdout" 2> /dev/null; echo $? > "$corpus/$n.i.status"; cp "$errf" "$corpus/$n.i.stderr")
    # A runaway recursion is said differently by design (S1 N0403): both refuse it, text not compared.
    if grep -q "^error\[N0403\]: .* recursed deeper than the interpreter's stack holds" "$corpus/$n.iref.stderr" \
        && grep -q "^error\[N0403\]: .* recursed deeper than 512" "$corpus/$n.i.stderr"; then
        : > "$corpus/$n.iref.stderr"; : > "$corpus/$n.i.stderr"
    fi
    if cmp -s "$corpus/$n.iref.stdout" "$corpus/$n.i.stdout" && cmp -s "$corpus/$n.iref.stderr" "$corpus/$n.i.stderr" \
        && cmp -s "$corpus/$n.iref.status" "$corpus/$n.i.status"; then
        iagree=$((iagree + 1))
    else
        idisagree=$((idisagree + 1))
        say "  the self-built interpreter disagrees with the seed's: $rel (status $(cat "$corpus/$n.iref.status") vs $(cat "$corpus/$n.i.status"))"
        for x in stdout stderr; do
            cmp -s "$corpus/$n.iref.$x" "$corpus/$n.i.$x" || say "    $x: $(diff "$corpus/$n.iref.$x" "$corpus/$n.i.$x" | head -6 | cut -c1-200)"
        done
    fi
done
say "interp: $iagree programs run by stage $stages's interpreter as by the seed's \`run --interpret\`, $idisagree disagree"

cagree=0
if "$seed" build "${cfg[@]}" --heap-limit "$heap" "$root/toolchain/harness/check/main.nz" -o "$out/check.ref" > "$out/check.ref.build" 2>&1; then
    (cd "$root" && step "check ref" 900 "$out/check.ref" "$root" > "$out/check.ref.out" 2>&1; echo $? >> "$out/check.ref.out")
    (cd "$root" && step "check stage$stages" 900 "$last/check" "$root" > "$out/check.out" 2>&1; echo $? >> "$out/check.out")
    if cmp -s "$out/check.ref.out" "$out/check.out"; then
        cagree=1; say "nazm-check: stage $stages's report of the tree is the seed's build's, byte for byte ($(grep -c '^ok ' "$out/check.out") gates ok)"
    else
        say "nazm-check: stage $stages's report differs from the seed's build's: $(diff "$out/check.ref.out" "$out/check.out" | head -6 | cut -c1-200)"
    fi
else
    say "nazm-check: the seed did not build the reference: $(head -c 300 "$out/check.ref.build")"
fi

# --- (5): the bound on macOS: the first counted heap under this allocator ----------------------------------
mkdir -p "$out/probe"
if build_with "$last" "$out/probe" fe compiler/diagnose.nz 256M > "$out/probe/build.log" 2>&1; then
    step "probe fe-under-256MiB" 900 "$out/probe/fe" --coredoc-sources "$root/toolchain/commands/nazm.nz" "$root/library" macos "$core_arch" > /dev/null 2> /dev/null
    pst=$?
    cp "$errf" "$out/probe/err"
    if [ "$pst" -eq 2 ] && grep -q 'error\[N0406\]' "$out/probe/err"; then
        say "heap probe: the front end built with a 256 MiB heap stopped with $(head -1 "$out/probe/err")"
    else
        say "heap probe: FAILED: status $pst, $(head -c 300 "$out/probe/err")"; bad=$((bad + 1))
    fi
else
    say "heap probe: the probe did not build: $(head -c 300 "$out/probe/build.log")"; bad=$((bad + 1))
fi

# --- (6): memory, from peaks.txt (bytes), summed per program and stage in MiB ------------------------------
say "memory (host-run's peaks.txt, bytes): per step, then per stage and program, MiB (sum, largest step):"
awk '/^# / { label = substr($0, 3); next }
     /peak resident [0-9]+ bytes/ {
         match($0, /peak resident [0-9]+/); p = substr($0, RSTART + 14, RLENGTH - 14) + 0
         split(label, w, " "); k = w[1] " " w[2]
         sum[k] += p; if (p > mx[k]) mx[k] = p
     }
     END { for (k in sum) printf "  %s %.1f MiB, largest %.1f MiB\n", k, sum[k] / 1048576, mx[k] / 1048576 }' "$peaks" | sort

[ "$bad" -eq 0 ] || fail "$bad checks failed"
[ "$fix" -eq 0 ] || fail "the stages are not a fixpoint"
[ "$corediffer" -eq 0 ] || fail "the self-built front end and the reference disagree on Core IR"
[ "$disagree" -eq 0 ] && [ "$unbuilt" -eq 0 ] || fail "the last stage's builds do not all run as the seed's do"
[ "$idisagree" -eq 0 ] || fail "the self-built interpreter does not run the corpus as the seed's does"
[ "$cagree" -eq 1 ] || fail "the self-built nazm-check does not report the tree as the seed's build does"
say "host-selfbuild complete ($arch, $backend backend): stages 1 to $stages byte-identical; $agree corpus programs agree ($noheap not run on the host); $iagree run by the self-built interpreter as by the seed's; nazm-check agrees; the 256 MiB heap stops with N0406"
