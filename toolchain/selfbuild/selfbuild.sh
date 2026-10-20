#!/bin/bash
# R6 — the Nazm toolchain builds itself (`openspec/changes/rewrite-in-nazm/`, task 7.1).
#
#     cargo xtask contained selfbuild [stages=N] [backend=llvm]   (the only way to run it)
#     toolchain/selfbuild/selfbuild.sh [OUT] [stages=N] [backend=llvm]   (inside the contained runner)
#
# The builder is three programs written in Nazm: `nazm` (`toolchain/commands/nazm.nz`), whose
# `build` is the driver with R2's front end in it (R8 slice 4, `openspec/changes/r8-cut-over/design.md`,
# *Slice 4*), the pipeline (`toolchain/pipeline/main.nz`: MIR, LIR and the runtime generator, R3)
# and the code generator (`toolchain/codegen/main.nz`, R9), the driver's two stages. A program is
# built as
#
#     NAZM_PIPELINE=pl NAZM_NZCG=cg nazm build --backend nazm --no-cache --target T --heap-limit H FILE -o NAME
#
# which runs the front end in process (Core IR, `nazm core-ir --with-sources`'s document), the
# pipeline (LIR, every unit, and the runtime's units, LLVM IR text), the code generator (one
# object per unit), the platform's `clang -c` for the runtime's units (as R3 leaves them) and its
# link — with no Rust between source and object. The front end (`compiler/diagnose.nz`) is built
# too, for the Core IR check below.
#
# `backend=llvm` (R8 slice 5) builds every stage's programs on the default LLVM backend instead:
#
#     NAZM_PIPELINE=pl nazm build --backend llvm --no-cache --target T --heap-limit H FILE -o NAME
#
# the pipeline writing each unit as LLVM text (`toolchain/pipeline/llvm.nz`), as the Rust build does,
# and `clang -c` making every object. The seed's own builds of the builder are the LLVM backend's
# (its default), so stage 1 must also equal stage 0: the same sources, the same text, the same objects.
# The chain (`rewrite-in-nazm` design, decision 7):
#
#     stage 0   the Rust `nazm` (the seed) builds the builder
#     stage 1   stage 0's builder builds the builder and every other toolchain program
#     stage 2   stage 1's builder does the same
#     stage 3   stage 2's builder does the same
#
# and stages 2 and 3 must be byte-identical: every executable. Stage 1 is compared with stage 2
# too: a builder built by Rust and one built by Nazm from the same source must agree, or one of
# them is wrong.
#
# Every program the builder builds is given the bounded heap (`--heap-limit`, R1d) the seed gives
# stage 0, so no stage of the chain allocates without a ceiling of its own. The container is the
# other ceiling: this script refuses to start outside it, as `compiler/bootstrap.sh` does. Each
# build's peak resident memory — the largest of the driver and the stages it waited for — is read
# from the kernel (`getrusage` of the waited child) and written to OUT/memory.txt; no time is
# taken.
#
# The target is the container's own (aarch64-unknown-linux-gnu on an Apple-silicon host): the
# containment rule needs a Linux kernel's cgroup, so the self-build is a Linux build of the
# toolchain. Nothing here makes Linux a supported target (`docs/user/support.md`).

set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${1:-/work/selfbuild}
# `stages=N` after OUT (what `cargo xtask contained selfbuild stages=N` passes) runs fewer stages.
for a in "${@:2}"; do
    case $a in
        stages=*) SELFBUILD_STAGES=${a#stages=} ;;
        backend=nazm | backend=llvm) SELFBUILD_BACKEND=${a#backend=} ;;
        *) printf 'selfbuild: unknown setting %s\n' "$a" >&2; exit 2 ;;
    esac
done
backend=${SELFBUILD_BACKEND:-nazm}
nazm=${NAZM:-"${CARGO_TARGET_DIR:-$root/target}/debug/nazm"}
# The heap each built program may hold: 2 GiB, the limit the R6 proposal would let a stage run
# under on a host (`openspec/changes/r6-host-qualification/design.md`), so the self-build is the
# evidence for exactly that number. The container's 4 GiB is the ceiling above it.
heap=${SELFBUILD_HEAP:-2G}
stages=${SELFBUILD_STAGES:-3}
deadline=${SELFBUILD_DEADLINE:-900}

fail() { printf 'selfbuild: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

if [ "${NAZM_CONTAINED:-}" != "1" ]; then
    fail "refusing to run compiler stages with no resource ceiling: run \`cargo xtask contained selfbuild\`."
fi
case $(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo none) in
    ''|*[!0-9]*|0) fail "NAZM_CONTAINED=1 is set, but no memory ceiling applies here. Run this through \`cargo xtask contained selfbuild\`; never set NAZM_CONTAINED on a host." ;;
esac
[ -x "$nazm" ] || fail "build the Rust implementation first: cargo build -p nazm-cli"
command -v clang >/dev/null 2>&1 || fail "clang is required: \`nazm build\` assembles the runtime's units and links with it"

case $(uname -m) in
    aarch64|arm64) arch=aarch64 ;;
    x86_64) arch=x86_64 ;;
    *) fail "no target for $(uname -m)" ;;
esac
case $(uname -s) in
    Linux) os=linux; target="$arch-unknown-linux-gnu" ;;
    *) fail "the self-build runs in the contained runner's Linux; $(uname -s) is not it" ;;
esac

heap_bytes() {
    case $1 in
        *G) echo $(( ${1%G} * 1024 * 1024 * 1024 )) ;;
        *M) echo $(( ${1%M} * 1024 * 1024 )) ;;
        *K) echo $(( ${1%K} * 1024 )) ;;
        *) echo "$1" ;;
    esac
}
heapb=$(heap_bytes "$heap")

# The builder's programs, then every other program of the toolchain, as NAME SOURCE. Since R8 slice 4
# the builder is `nazm build` (`nazm`) and its two stages; the front end (`fe`) is one of the others.
builder="nazm:toolchain/commands/nazm.nz pl:toolchain/pipeline/main.nz cg:toolchain/codegen/main.nz"
# The interpreter (`toolchain/interp/main.nz`) and `nazm-check` (`toolchain/harness/check/main.nz`)
# joined at R8 slice 1, moved from 0.10.0's `List[T]` to the checkout's `Chain[T]` (R1.5 D7;
# `openspec/changes/r6-host-qualification/design.md`, *Gaps*; `openspec/changes/r8-cut-over/`).
others="fe:compiler/diagnose.nz mcp:toolchain/commands/mcp.nz mutate:toolchain/harness/mutate/main.nz jtool:toolchain/harness/tools/jtool.nz interp:toolchain/interp/main.nz check:toolchain/harness/check/main.nz"

rm -rf "$out"
mkdir -p "$out"
mem="$out/memory.txt"
: > "$mem"

# `peak LABEL CMD...`: run CMD under the deadline, its standard streams as given; append
# "LABEL peak-rss-KiB status" to the memory record. The peak is the kernel's ru_maxrss of the
# one child waited for (`wait4`), read by a few lines of C: the image has no other tool that
# reports it.
cat > "$out/peak.c" <<'EOF'
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>
static pid_t child;
static void expire(int s) { (void)s; kill(child, SIGKILL); }
int main(int argc, char **argv) {
    if (argc < 5) return 2;
    child = fork();
    if (child == 0) { execvp(argv[4], argv + 4); _exit(127); }
    signal(SIGALRM, expire);
    alarm((unsigned)atoi(argv[3]));
    int st = 0;
    struct rusage ru;
    while (wait4(child, &st, 0, &ru) < 0) {}
    int code = WIFEXITED(st) ? WEXITSTATUS(st) : 128 + WTERMSIG(st);
    FILE *f = fopen(argv[2], "a");
    if (f) { fprintf(f, "%s %ld %d\n", argv[1], ru.ru_maxrss, code); fclose(f); }
    return code;
}
EOF
cc -O1 -o "$out/peak" "$out/peak.c" || fail "the peak-memory reader did not compile"
peak() {
    label=$1; shift
    "$out/peak" "$label" "$mem" "$deadline" "$@"
}

# `build_with BIN DEST NAME SOURCE`: the builder in BIN builds SOURCE into DEST/NAME — its `nazm
# build`, with its pipeline and code generator as the stages (R8 slice 4) — under the heap limit
# `$heapb`. What the build said is kept in DEST/NAME.build.out and .err.
build_with() {
    B=$1; dest=$2; name=$3; src=$4
    st=$(basename "$dest")
    NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$B/pl" NAZM_NZCG="$B/cg" \
        peak "$st $name build" "$B/nazm" build --backend "$backend" --no-cache --target "$target" --heap-limit "$heapb" \
        "$root/$src" -o "$dest/$name" > "$dest/$name.build.out" 2> "$dest/$name.build.err" < /dev/null \
        || { say "  $name: \`nazm build\` failed: $(head -c 600 "$dest/$name.build.err")"; return 1; }
    return 0
}

say "selfbuild: target $target, backend $backend, heap limit $heap ($heapb bytes) per built program, $stages stages"
say "selfbuild: seed $("$nazm" --version 2>/dev/null)"

# --- stage 0: the seed builds the builder -------------------------------------------------------
mkdir -p "$out/stage0"
for p in $builder; do
    name=${p%%:*}; src=${p#*:}
    peak "stage0 $name seed-build" "$nazm" build --no-cache --heap-limit "$heap" "$root/$src" -o "$out/stage0/$name" > "$out/stage0/$name.log" 2>&1 \
        || fail "stage 0: the seed did not build $src: $(tail -5 "$out/stage0/$name.log")"
done
say "stage 0: the seed built the builder"

# --- stages 1..N ------------------------------------------------------------------------------
bad=0
prev="$out/stage0"
for s in $(seq 1 "$stages"); do
    dest="$out/stage$s"
    mkdir -p "$dest"
    built=0
    for p in $builder $others; do
        name=${p%%:*}; src=${p#*:}
        if build_with "$prev" "$dest" "$name" "$src"; then built=$((built + 1)); else bad=$((bad + 1)); fi
    done
    say "stage $s: stage $((s - 1))'s builder built $built programs"
    for b in nazm pl cg; do
        [ -x "$dest/$b" ] || fail "stage $s has no $b: the chain stops here"
    done
    prev="$dest"
done

# --- the fixpoint -------------------------------------------------------------------------------
compare() {
    a=$1; b=$2; differ=0; same=0
    for f in $(cd "$a" && find . -type f ! -name '*.err' ! -name '*.out' | sort); do
        if cmp -s "$a/$f" "$b/$f"; then same=$((same + 1)); else differ=$((differ + 1)); say "  differs: $f"; fi
    done
    say "$(basename "$a") vs $(basename "$b"): $same files identical, $differ differ"
    [ "$differ" -eq 0 ]
}
fix=0
# On the LLVM backend the seed's builds of the builder are the same backend's: Nazm's emitter and the
# Rust one must write the same text, so stage 1's builder is the seed's, byte for byte.
if [ "$backend" = llvm ] && [ "$stages" -ge 1 ]; then
    seeddiff=0
    for b in $(echo "$builder" | tr ' ' '\n' | cut -d: -f1); do
        if cmp -s "$out/stage0/$b" "$out/stage1/$b"; then say "stage 0 vs stage 1: $b identical"; else seeddiff=$((seeddiff + 1)); say "  differs: $b (the seed's LLVM build and Nazm's)"; fi
    done
    [ "$seeddiff" -eq 0 ] || fix=1
fi
if [ "$stages" -ge 2 ]; then compare "$out/stage1" "$out/stage2" || fix=1; fi
if [ "$stages" -ge 3 ]; then compare "$out/stage2" "$out/stage3" || fix=1; fi

# --- the last builder's Core IR is the reference's ----------------------------------------------
# For every toolchain program, the document the last stage's front end (`fe --coredoc-sources`)
# writes must be the one the seed's `nazm core-ir --with-sources` prints (the indented form), byte
# for byte. (`nazm build` writes the same document from the same front end, in process; its
# objects agreeing with the Rust build's is `toolchain/commands/tests/build.sh`'s.)
coreagree=0; corediffer=0
last="$out/stage$stages"
mkdir -p "$out/core"
for p in $builder $others; do
    name=${p%%:*}; src=${p#*:}
    [ -x "$last/fe" ] || break
    peak "core $name front-end" "$last/fe" --coredoc-sources "$root/$src" "$root/library" "$os" "$arch" > "$out/core/$name.json" 2> /dev/null
    [ -s "$out/core/$name.json" ] || { corediffer=$((corediffer + 1)); say "  Core IR of $src: the front end wrote none"; continue; }
    # Whole: every member, a call's `callee_at` and a payload binding's `at` included (R6 gap 2).
    "$nazm" core-ir --with-sources "$root/$src" 2> /dev/null > "$out/core/$name.ref.json"
    if cmp -s "$out/core/$name.ref.json" "$out/core/$name.json"; then
        coreagree=$((coreagree + 1))
    else
        corediffer=$((corediffer + 1))
        say "  Core IR of $src differs from the reference's: $(diff "$out/core/$name.ref.json" "$out/core/$name.json" | head -6 | cut -c1-160)"
    fi
done
rm -rf "$out/core"
say "core-ir: $coreagree documents identical to the reference's, $corediffer differ"

# --- what the last builder builds runs as the seed's build does ---------------------------------
# A fixpoint proves reproduction, not correctness (`rewrite-in-nazm` design, decision 7). The last
# stage's builder builds programs the seed builds too, and both run: standard output, standard
# error and exit status must agree byte for byte.
last="$out/stage$stages"
corpus="$out/corpus"
mkdir -p "$corpus"
agree=0; disagree=0; unbuilt=0
for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
    [ -f "$f" ] || continue
    rel=${f#"$root"/}
    n=$(echo "$rel" | tr '/.' '__')
    if ! "$nazm" build --no-cache --heap-limit "$heap" "$f" -o "$corpus/$n.ref" > "$corpus/$n.ref.build" 2>&1; then
        continue  # the seed does not build it: not an input
    fi
    if ! build_with "$last" "$corpus" "$n" "$rel" > "$corpus/$n.build" 2>&1; then
        unbuilt=$((unbuilt + 1)); say "  not built by stage $stages: $rel: $(head -c 300 "$corpus/$n.build")"; continue
    fi
    (cd "$corpus" && timeout 60 "./$n.ref" > "$n.ref.stdout" 2> "$n.ref.stderr" < /dev/null; echo $? > "$n.ref.status")
    (cd "$corpus" && timeout 60 "./$n" > "$n.stdout" 2> "$n.stderr" < /dev/null; echo $? > "$n.status")
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
say "corpus: $agree programs built by stage $stages run as the seed's builds do, $disagree disagree, $unbuilt not built"

# --- the self-built interpreter runs the corpus as the seed's does --------------------------------
# The last stage's `interp` is given the seed's `nazm core-ir --json` of each corpus program (as
# `toolchain/interp/tests/run.sh` gives it) and must print, fail and exit as the seed's `nazm run
# --interpret` does. A program that does not check, or has no `main`, is not an input.
iagree=0; idisagree=0
if [ -x "$last/interp" ]; then
    for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
        [ -f "$f" ] || continue
        rel=${f#"$root"/}
        n=$(echo "$rel" | tr '/.' '__')
        dir=$(dirname "$f"); file=$(basename "$f")
        if ! (cd "$dir" && "$nazm" core-ir --json "$file" > "$corpus/$n.ir.json" 2> /dev/null) \
            || grep -q '^{"schema":"nazm.core-ir/[23]","entry":null' "$corpus/$n.ir.json"; then
            continue
        fi
        (cd "$dir" && timeout 60 "$nazm" run --interpret "$file" > "$corpus/$n.iref.stdout" 2> "$corpus/$n.iref.stderr" < /dev/null; echo $? > "$corpus/$n.iref.status")
        (cd "$dir" && timeout 60 "$last/interp" --std "$root/library" "$corpus/$n.ir.json" "$file" > "$corpus/$n.i.stdout" 2> "$corpus/$n.i.stderr" < /dev/null; echo $? > "$corpus/$n.i.status")
        # A runaway recursion is the one thing the two interpreters say differently by design: the
        # seed's stops where its own native stack gives out ("recursed deeper than the interpreter's
        # stack holds, at N calls", N a property of the host), the Nazm one at its fixed depth of
        # 512. Both must refuse it with N0403, with the same output and status; the text is not compared.
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
else
    say "interp: stage $stages has no interpreter"; idisagree=1
fi

# --- the self-built nazm-check reports the tree as the seed's build of it does ---------------------
cagree=0
if [ -x "$last/check" ] && "$nazm" build --no-cache --heap-limit "$heap" "$root/toolchain/harness/check/main.nz" -o "$out/check.ref" > "$out/check.ref.build" 2>&1; then
    (cd "$root" && timeout 600 "$out/check.ref" "$root" > "$out/check.ref.out" 2>&1 < /dev/null; echo $? >> "$out/check.ref.out")
    (cd "$root" && timeout 600 "$last/check" "$root" > "$out/check.out" 2>&1 < /dev/null; echo $? >> "$out/check.out")
    if cmp -s "$out/check.ref.out" "$out/check.out"; then
        cagree=1; say "nazm-check: stage $stages's report of the tree is the seed's build's, byte for byte ($(grep -c '^ok ' "$out/check.out") gates ok)"
    else
        say "nazm-check: stage $stages's report differs from the seed's build's: $(diff "$out/check.ref.out" "$out/check.out" | head -6 | cut -c1-200)"
    fi
else
    say "nazm-check: not compared: $( [ -x "$last/check" ] || echo "stage $stages has none"; head -c 300 "$out/check.ref.build" 2>/dev/null)"
fi

# --- the bounded heap holds in a self-built stage -----------------------------------------------
# The last builder builds the front end again with a heap of 256 MiB, and it is run on the largest
# toolchain program: it must stop with N0406 (status 2), its resident peak near the limit rather
# than at the 1.3 GB it needs.
probe_heap=$((256 * 1024 * 1024))
saved=$heapb; heapb=$probe_heap
mkdir -p "$out/probe"
if build_with "$last" "$out/probe" fe compiler/diagnose.nz > "$out/probe/build.log" 2>&1; then
    heapb=$saved
    "$out/peak" "probe fe-under-256MiB" "$mem" "$deadline" "$out/probe/fe" --coredoc-sources "$root/toolchain/commands/nazm.nz" "$root/library" "$os" "$arch" > /dev/null 2> "$out/probe/err"
    pst=$?
    if [ "$pst" -eq 2 ] && grep -q 'error\[N0406\]' "$out/probe/err"; then
        say "heap probe: the front end built with a 256 MiB heap stopped with $(head -1 "$out/probe/err")"
    else
        say "heap probe: FAILED — status $pst, $(head -c 300 "$out/probe/err")"; bad=$((bad + 1))
    fi
else
    heapb=$saved
    say "heap probe: the probe did not build: $(head -c 300 "$out/probe/build.log")"; bad=$((bad + 1))
fi

say "memory (peak resident KiB, status), per step:"
cat "$mem"
# Per stage, the largest step's peak, and which step it was.
say "memory, the largest step of each stage (peak resident MiB):"
awk '{ k = $NF; v = $(NF - 1); if (!($1 in m) || v > m[$1]) { m[$1] = v; w[$1] = $2 " " $3 } }
     END { for (s in m) printf "  %s %.1f (%s)\n", s, m[s] / 1024, w[s] }' "$mem" | sort
[ "$bad" -eq 0 ] || fail "$bad program builds failed"
[ "$fix" -eq 0 ] || fail "the stages are not a fixpoint"
[ "$corediffer" -eq 0 ] || fail "the self-built front end and the reference disagree on Core IR"
[ "$disagree" -eq 0 ] && [ "$unbuilt" -eq 0 ] || fail "the last stage's builds do not all run as the seed's do"
[ "$idisagree" -eq 0 ] || fail "the self-built interpreter does not run the corpus as the seed's does"
[ "$cagree" -eq 1 ] || fail "the self-built nazm-check does not report the tree as the seed's build does"
say "selfbuild complete ($backend backend): stages 1 to $stages byte-identical, every program built by \`nazm build\`; $agree programs agree with the seed's builds; $iagree run by the self-built interpreter as by the seed's; nazm-check agrees; the bounded heap holds"
