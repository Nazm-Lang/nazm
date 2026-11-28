#!/bin/bash
# linux-toolchain L2 — the aarch64 Linux self-build, release grade
# (`openspec/changes/linux-toolchain/`, tasks 2.1 and 2.2).
#
#     cargo xtask contained linux-selfbuild          (the only way to run it; it exports OUT)
#     toolchain/selfbuild/linux-selfbuild.sh OUT     (inside the contained runner)
#
# `host-selfbuild.sh` on Linux: the same chain, the same checks and the same `nazm.selfbuild-record/1`
# record, with the container in place of `host-run`. The container's cgroup (4 GiB, no network, a
# pid limit) is the ceiling every Nazm-built program runs under, as `host-run` is on macOS; each
# program built here also carries the 2 GiB counted heap. The script refuses to start anywhere else
# (`toolchain/lib/stage.sh`, contained mode). The chain:
#
#     stage 0   the seed (the Rust `nazm`, LLVM) builds the builder (`nazm`, `pl`, `cg`)
#     stage 1   stage 0's builder builds the nine programs of `programs.txt`
#     stage 2   stage 1's builder does the same
#     stage 3   stage 2's builder does the same
#
# each stage's programs built `--release --backend llvm --no-cache --heap-limit 2G --target
# aarch64-unknown-linux-gnu`. The checks, those of `host-selfbuild.sh`: stages 1, 2 and 3
# byte-identical; the last stage's front end writes the seed's Core IR; the corpus built by the last
# stage runs as the seed's builds do; the self-built interpreter and `nazm-check` agree with the
# seed's; the front end built with a 256 MiB heap stops with N0406. And two of Linux's own: every
# program of the last stage is a hardened aarch64 ELF (PIE, stripped, full RELRO, a stack that is not
# executable: what `--release` on Linux makes, L1), and the last stage's `nazm`, with
# `NAZM_CONTAINED` unset as a user's shell has it, builds and runs a program on its own triple, with
# and without `--release` (L1, tasks 1.1 and 1.2).
#
# What differs from the macOS script, and why:
#   - the Rust seed hardens only macOS (`crates/nazm-cli/src/build/preflight.rs`: `--release` is "a
#     macOS executable, built on macOS"), and the seed is frozen; so the seed's builds here (stage 0,
#     the corpus's references, the reference `nazm-check`) are its LLVM backend without `--release`.
#     Every Nazm stage is `--release`. The comparisons with the seed are of behaviour and of Core IR,
#     which `--release` does not change; the byte comparisons are between Nazm stages only;
#   - `shasum` is `sha256sum`; `otool`, `codesign` and `dwarfdump` (localising a difference) are
#     `readelf` and `cmp`; `xcrun`, `sw_vers` and `plutil` (the pinned SDK) have no counterpart: the
#     environment is the kernel, the image's OS release, `clang` and the C library, recorded below.
#     The image has no `file`; `readelf -h` says what `file` would (class, machine, type);
#   - no program is refused for lacking a counted heap (that is `host-run`'s rule): every corpus
#     program runs, under the container and a deadline;
#   - `git` is not in the image: the commit and whether the tree was clean are the supervisor's
#     (`NAZM_SOURCE_COMMIT`, `NAZM_SOURCE_TREE_CLEAN`, as the `release` workload passes them); the
#     build-input digest is taken here from the files, at the start and again at the end;
#   - each step's peak resident memory is the kernel's (`wait4`'s `ru_maxrss`, the reader
#     `selfbuild.sh` uses), appended to `OUT/peaks.txt`. Nothing is timed.
#
# `OUT/fixpoint.txt` is written last, and only when every check passed.

set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage_name=linux-selfbuild
. "$root/toolchain/lib/stage.sh"

say() { printf '%s\n' "$*"; }
# Descriptor 3 is the script's own standard error, kept whatever a step redirects.
exec 3>&2
# `fail` also stops the run from inside a subshell: it signals the script itself, which exits 1.
fail() { printf 'linux-selfbuild: FAILED: %s\n' "$*" >&3; kill -TERM $$; exit 1; }
trap 'exit 1' TERM

[ $# -eq 1 ] || stage_fail "usage: linux-selfbuild.sh OUT"
out=$1
backend=llvm

# --- the refusals: nothing has run when any of these stops the script ------------------------------
STAGE_HEAP=${STAGE_HEAP:-2G}
stage_init "$root" contained
[ "$(uname -s)" = Linux ] || stage_fail "refused [not-linux]: the Linux self-build runs in the contained runner's Linux; the macOS one is host-selfbuild.sh"
case $(uname -m) in
    aarch64 | arm64) arch=aarch64; target=aarch64-unknown-linux-gnu ;;
    *) stage_fail "refused [arch]: this is the aarch64 self-build (linux-toolchain L2); x86_64 Linux is cross-built from its stage 3 (L3), not $(uname -m)" ;;
esac
[ -n "$out" ] || stage_fail "usage: OUT is empty"
if [ -e "$out" ] && [ -n "$(ls -A "$out" 2> /dev/null)" ]; then
    stage_fail "refused [out-exists]: $out is not empty; give a new directory"
fi
[ -f "$root/toolchain/selfbuild/programs.txt" ] || stage_fail "toolchain/selfbuild/programs.txt is missing"
seed=${NAZM:-$(stage_checkout_nazm "$root")}
[ -x "$seed" ] || stage_fail "build the Rust seed first: cargo build -p nazm-cli"
for t in clang cc readelf sha256sum perl; do
    command -v "$t" > /dev/null 2>&1 || stage_fail "$t is required"
done
unset NAZM_PIPELINE NAZM_NZCG NAZM_LIBRARY NAZM_LIBEXEC

# What the run is built from: the commit and the tree's state (the supervisor's), and the build-input
# digest (`scripts/source-digest.sh`), taken now and again at the end.
src_commit=${NAZM_SOURCE_COMMIT:-$(git -C "$root" rev-parse HEAD 2> /dev/null || true)}
case $src_commit in
    *[!0-9a-f]* | '') stage_fail "refused [no-commit]: the source commit is not known (NAZM_SOURCE_COMMIT); run it through \`cargo xtask contained linux-selfbuild\`" ;;
esac
src_clean=${NAZM_SOURCE_TREE_CLEAN:-unknown}
case $src_clean in yes | no) ;; *) stage_fail "refused [no-tree-state]: NAZM_SOURCE_TREE_CLEAN is yes or no, not '$src_clean'" ;; esac
src_digest=$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null) || stage_fail "the build-input digest could not be taken"
sha256_of() { sha256sum "$1" 2> /dev/null | cut -d' ' -f1; }

heap=$STAGE_HEAP
mkdir -p "$out" || stage_fail "cannot create $out"
out=$(CDPATH= cd -- "$out" && pwd)
peaks="$out/peaks.txt"
: > "$peaks"

env_kernel="$(uname -sr) $(uname -m)"
env_clang=$(clang --version 2> /dev/null | head -1)
env_libc=$(getconf GNU_LIBC_VERSION 2> /dev/null || ldd --version 2> /dev/null | head -1)
# environment.txt: the toolchain the fixpoint claims (one container image, one toolchain).
{
    echo "source-commit: $src_commit"
    echo "source-tree-clean: $src_clean"
    echo "build-input-digest: $src_digest"
    echo "script: linux-selfbuild.sh (backend $backend, target $target)"
    echo "kernel: $env_kernel"
    echo "uname -a: $(uname -a)"
    echo "os-release: $(. /etc/os-release 2> /dev/null && echo "$PRETTY_NAME")"
    echo "clang --version:"; clang --version 2>&1
    echo "ld -v: $(ld -v 2>&1 | head -1)"
    echo "libc: $env_libc ($(ldd --version 2>&1 | head -1))"
    echo "readelf: $(readelf --version 2>&1 | head -1)"
    echo "cgroup memory.max: $(cat /sys/fs/cgroup/memory.max 2>&1)"
    echo "cgroup pids.max: $(cat /sys/fs/cgroup/pids.max 2>&1)"
    echo "PATH=$PATH"
    echo "seed: $("$seed" --version 2> /dev/null)"
    echo "seed sha256: $(sha256_of "$seed")"
    echo "heap limit: $heap"
} > "$out/environment.txt" 2>&1

# The peak reader (`selfbuild.sh`'s): `peak LABEL FILE DEADLINE CMD...` runs CMD under the deadline
# (SIGKILL past it), appends "LABEL peak-rss-KiB status" to FILE, and exits with CMD's status.
tools="$out/.tools"
mkdir -p "$tools"
cat > "$tools/peak.c" << 'EOF'
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
cc -O1 -o "$tools/peak" "$tools/peak.c" || stage_fail "the peak-memory reader did not compile"

# Core IR: the front end is given the container's own platform (the seed's `core-ir` has no --target).
core_os=linux
core_arch=$arch

# --- the steps ----------------------------------------------------------------------------------
# The release configuration, every Nazm stage's; the seed's is the same but for `--release` (above).
cfg=(--release --backend "$backend" --no-cache --target "$target")
cfg_seed=(--backend llvm --no-cache --target "$target")

programs=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$')
builder=$(echo "$programs" | awk '$1 == "builder" { printf "%s%s:%s", sep, $2, $3; sep = " " }')
others=$(echo "$programs" | awk '$1 == "other" { printf "%s%s:%s", sep, $2, $3; sep = " " }')
[ -n "$builder" ] && [ -n "$others" ] || stage_fail "programs.txt names no builder or no other program"
names=$(echo "$builder $others" | tr ' ' '\n' | cut -d: -f1)

# `step LABEL DEADLINE EXE ARGS...`: one Nazm-built program, its peak in peaks.txt, its standard error
# in `$errf`, standard output the caller's, standard input empty; its status is returned.
errf="$out/.step.err"
step() {
    label=$1; deadline=$2; shift 2
    "$tools/peak" "$label" "$peaks" "$deadline" "$@" 2> "$errf" < /dev/null
}

# `build_with BIN DEST NAME SOURCE [HEAP]`: BIN's builder builds SOURCE into DEST/NAME.
build_with() {
    B=$1; dest=$2; name=$3; src=$4; h=${5:-$heap}
    NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$B/pl" NAZM_NZCG="$B/cg" \
        step "$(basename "$dest") $name build" 1800 "$B/nazm" build "${cfg[@]}" --heap-limit "$h" "$root/$src" -o "$dest/$name" \
        > "$dest/$name.build.out" || {
            st=$?
            cp "$errf" "$dest/$name.build.err" 2> /dev/null
            say "  $name: \`nazm build\` failed (status $st), the end of what it said: $(tail -c 1200 "$dest/$name.build.err" 2> /dev/null)"
            return 1
        }
    return 0
}

say "linux-selfbuild: target $target, backend $backend, heap limit $heap, release configuration, $env_kernel, $env_clang, $env_libc"
say "linux-selfbuild: commit $src_commit (tree clean: $src_clean), build-input digest $src_digest"

# --- stage 0: the seed (Rust, run directly) builds the builder ---------------------------------------
mkdir -p "$out/stage0"
for p in $builder; do
    name=${p%%:*}; src=${p#*:}
    "$tools/peak" "stage0 $name seed-build" "$peaks" 1800 "$seed" build "${cfg_seed[@]}" --heap-limit "$heap" "$root/$src" -o "$out/stage0/$name" \
        > "$out/stage0/$name.log" 2>&1 < /dev/null \
        || fail "stage 0: the seed did not build $src: $(tail -5 "$out/stage0/$name.log")"
done
say "stage 0: the seed built the builder"

# --- stages 1..3 ------------------------------------------------------------------------------------
stages=3
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

# --- (1): the fixpoint, every executable whole --------------------------------------------------------
# A difference is localised with what Linux has: the ELF headers, sections, notes and the first bytes
# that differ (macOS's otool, codesign and dwarfdump have no part here: nothing is signed).
localise() {
    a=$1; b=$2; name=$3
    d="$out/localise/$(basename "$a")-$(basename "$b")-$name"
    mkdir -p "$d"
    cp "$a/$name" "$d/a"; cp "$b/$name" "$d/b"
    { echo "== readelf -hlW (diff) =="; diff <(readelf -hlW "$d/a") <(readelf -hlW "$d/b") | head -60
      echo "== readelf -SW (diff) =="; diff <(readelf -SW "$d/a") <(readelf -SW "$d/b") | head -60
      echo "== readelf -nW (build ids) =="; readelf -nW "$d/a" 2>&1 | head -20; echo "--"; readelf -nW "$d/b" 2>&1 | head -20
      echo "== cmp -l (first 20) =="; cmp -l "$d/a" "$d/b" 2>&1 | head -20
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
compare_stages "$out/stage1" "$out/stage2" || fix=1
compare_stages "$out/stage2" "$out/stage3" || fix=1

# --- (1b): what --release made on Linux: a hardened aarch64 ELF ---------------------------------------
# `elf_is FILE`: "" when FILE is a 64-bit aarch64 ELF executable (EXEC or DYN), else why not.
elf_is() {
    h=$(readelf -hW "$1" 2> /dev/null) || { echo "not an ELF file"; return; }
    echo "$h" | grep -q 'Class:[[:space:]]*ELF64' || { echo "not ELF64"; return; }
    echo "$h" | grep -q 'Machine:[[:space:]]*AArch64' || { echo "not AArch64: $(echo "$h" | grep Machine:)"; return; }
    echo "$h" | grep -Eq 'Type:[[:space:]]*(EXEC|DYN)' || { echo "not an executable: $(echo "$h" | grep Type:)"; return; }
}
# `elf_hardened FILE`: "" when FILE is also a PIE (DYN, FLAGS_1 PIE), stripped (no .symtab), bound now
# (BIND_NOW) with a GNU_RELRO segment (full RELRO), and its GNU_STACK is not executable; else why not.
elf_hardened() {
    why=$(elf_is "$1"); [ -z "$why" ] || { echo "$why"; return; }
    readelf -hW "$1" | grep -q 'Type:[[:space:]]*DYN' || { echo "not a PIE (type is not DYN)"; return; }
    dyn=$(readelf -dW "$1" 2> /dev/null)
    echo "$dyn" | grep -Eq '\(FLAGS_1\).*PIE' || { echo "not a PIE (FLAGS_1 has no PIE)"; return; }
    echo "$dyn" | grep -Eq '\(FLAGS\).*BIND_NOW|\(FLAGS_1\).*NOW|\(BIND_NOW\)' || { echo "not bound now (no BIND_NOW)"; return; }
    seg=$(readelf -lW "$1" 2> /dev/null)
    echo "$seg" | grep -q 'GNU_RELRO' || { echo "no GNU_RELRO segment"; return; }
    stack=$(echo "$seg" | awk '$1 == "GNU_STACK" { print $(NF - 1) }')
    [ -n "$stack" ] || { echo "no GNU_STACK segment"; return; }
    case $stack in *E*) echo "the stack is executable (GNU_STACK $stack)"; return ;; esac
    if readelf -SW "$1" 2> /dev/null | grep -q '[[:space:]]\.symtab[[:space:]]'; then echo "not stripped (.symtab)"; return; fi
}
hardened=0
for name in $names; do
    why=$(elf_hardened "$last/$name")
    if [ -z "$why" ]; then hardened=$((hardened + 1)); else bad=$((bad + 1)); say "  stage $stages's $name is not a hardened release executable: $why"; fi
done
say "release: $hardened of $(echo "$names" | wc -l | tr -d ' ') stage $stages executables are aarch64 PIEs, stripped, full RELRO, stack not executable"

# --- (2): Core IR against the seed ----------------------------------------------------------------------
coreagree=0; corediffer=0
mkdir -p "$out/core"
for p in $builder $others; do
    name=${p%%:*}; src=${p#*:}
    step "core $name front-end" 900 "$last/fe" --coredoc-sources "$root/$src" "$root/library" "$core_os" "$core_arch" > "$out/core/$name.json"
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

# --- (3): behaviour against the seed, the corpus at --heap-limit 256M ----------------------------------
corpus="$out/corpus"
mkdir -p "$corpus"
agree=0; disagree=0; unbuilt=0
for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
    [ -f "$f" ] || continue
    rel=${f#"$root"/}
    n=$(echo "$rel" | tr '/.' '__')
    if ! "$seed" build "${cfg_seed[@]}" --heap-limit 256M "$f" -o "$corpus/$n.ref" > "$corpus/$n.ref.build" 2>&1 < /dev/null; then
        continue  # the seed does not build it: not an input
    fi
    if ! build_with "$last" "$corpus" "$n" "$rel" 256M > "$corpus/$n.build" 2>&1; then
        unbuilt=$((unbuilt + 1)); say "  not built by stage $stages: $rel: $(head -c 300 "$corpus/$n.build")"; continue
    fi
    (cd "$corpus" && step "corpus $n ref" 60 "./$n.ref" > "$n.ref.stdout"; echo $? > "$n.ref.status"; cp "$errf" "$n.ref.stderr")
    (cd "$corpus" && step "corpus $n" 60 "./$n" > "$n.stdout"; echo $? > "$n.status"; cp "$errf" "$n.stderr")
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
    (cd "$dir" && step "interp $n" 60 "$last/interp" --std "$root/library" "$corpus/$n.ir.json" "$file" > "$corpus/$n.i.stdout"; echo $? > "$corpus/$n.i.status"; cp "$errf" "$corpus/$n.i.stderr")
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
if "$seed" build "${cfg_seed[@]}" --heap-limit "$heap" "$root/toolchain/harness/check/main.nz" -o "$out/check.ref" > "$out/check.ref.build" 2>&1 < /dev/null; then
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

# --- (5): the bound: the front end built with a 256 MiB heap stops with N0406 ----------------------------
mkdir -p "$out/probe"
if build_with "$last" "$out/probe" fe compiler/diagnose.nz 256M > "$out/probe/build.log" 2>&1; then
    step "probe fe-under-256MiB" 900 "$out/probe/fe" --coredoc-sources "$root/toolchain/commands/nazm.nz" "$root/library" "$core_os" "$core_arch" > /dev/null
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

# --- (6): L1, a user's build: stage 3's `nazm` with NAZM_CONTAINED unset ----------------------------------
# What a shipped Linux `nazm` does in a user's shell: build for its own triple (no --target), normally and
# with --release, and `nazm run`. The cgroup still holds every program; only the declaration is gone.
l1="$out/l1"
mkdir -p "$l1"
printf 'fn main() {\n    println("Hello from Nazm on Linux")\n}\n' > "$l1/hello.nz"
printf 'Hello from Nazm on Linux\n' > "$l1/expected.stdout"
user() {
    label=$1; deadline=$2; shift 2
    (cd "$l1" && env -u NAZM_CONTAINED NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$last/pl" NAZM_NZCG="$last/cg" \
        "$tools/peak" "$label" "$peaks" "$deadline" "$@" 2> "$errf" < /dev/null)
}
l1ok=0
for kind in plain release; do
    flag=; [ $kind = release ] && flag=--release
    if ! user "l1 $kind build" 900 "$last/nazm" build $flag hello.nz -o "hello-$kind" > "$l1/$kind.build.out"; then
        cp "$errf" "$l1/$kind.build.err"; say "  L1: \`nazm build $flag\` of hello.nz failed without NAZM_CONTAINED: $(tail -c 1200 "$l1/$kind.build.err")"; continue
    fi
    if [ $kind = release ]; then why=$(elf_hardened "$l1/hello-$kind"); else why=$(elf_is "$l1/hello-$kind"); fi
    [ -z "$why" ] || { say "  L1: the $kind build is not what it should be: $why"; continue; }
    user "l1 $kind run" 60 "$l1/hello-$kind" > "$l1/$kind.stdout"; st=$?
    if [ "$st" -eq 0 ] && cmp -s "$l1/expected.stdout" "$l1/$kind.stdout"; then
        l1ok=$((l1ok + 1))
    else
        say "  L1: the $kind build ran with status $st and printed: $(head -c 200 "$l1/$kind.stdout"; head -c 300 "$errf")"
    fi
done
if user "l1 run" 900 "$last/nazm" run hello.nz > "$l1/run.stdout" && cmp -s "$l1/expected.stdout" "$l1/run.stdout"; then
    l1ok=$((l1ok + 1))
else
    say "  L1: \`nazm run hello.nz\` without NAZM_CONTAINED: $(head -c 200 "$l1/run.stdout"; head -c 300 "$errf")"
fi
release_type=$(readelf -hW "$l1/hello-release" 2> /dev/null | sed -n 's/^ *Type:[[:space:]]*\([A-Z]*\).*/\1/p')
if [ "$l1ok" -eq 3 ]; then
    say "L1: without NAZM_CONTAINED, stage $stages's nazm built hello.nz for $target (aarch64 ELF) and with --release ($release_type, PIE, stripped, BIND_NOW, GNU_RELRO, GNU_STACK not executable); both and \`nazm run\` printed the expected line"
else
    bad=$((bad + 1)); say "L1: FAILED: $l1ok of 3 user builds and runs as expected"
fi
# The driver's Linux answers (task 1.3), the test that lives with build.nz's: the admission of the Linux
# triples, --release on Linux and the refusals around them, NAZM_CONTAINED unset.
if bash "$root/toolchain/commands/tests/linux-build.sh" "$last" "$l1/driver" > "$l1/driver.txt" 2>&1; then
    say "L1, the driver: $(tail -1 "$l1/driver.txt") (toolchain/commands/tests/linux-build.sh)"
else
    bad=$((bad + 1)); say "L1, the driver: FAILED (toolchain/commands/tests/linux-build.sh):"; sed 's/^/  /' "$l1/driver.txt"
fi

# --- (7): memory, from peaks.txt (KiB), per stage and program, MiB (sum, largest step) --------------------
say "memory (the kernel's ru_maxrss, peaks.txt, KiB): per stage and kind, MiB (sum, largest step):"
awk '{ p = $(NF - 1); k = $1 " " $2; sum[k] += p; if (p > mx[k]) mx[k] = p }
     END { for (k in sum) printf "  %s %.1f MiB, largest %.1f MiB\n", k, sum[k] / 1024, mx[k] / 1024 }' "$peaks" | sort
say "memory: the largest step of the run: $(awk '{ p = $(NF - 1); if (p > m) { m = p; l = $0 } } END { printf "%.1f MiB (%s)", m / 1024, l }' "$peaks")"

[ "$bad" -eq 0 ] || fail "$bad checks failed"
[ "$fix" -eq 0 ] || fail "the stages are not a fixpoint"
[ "$corediffer" -eq 0 ] || fail "the self-built front end and the reference disagree on Core IR"
[ "$disagree" -eq 0 ] && [ "$unbuilt" -eq 0 ] || fail "the last stage's builds do not all run as the seed's do"
[ "$idisagree" -eq 0 ] || fail "the self-built interpreter does not run the corpus as the seed's does"
[ "$cagree" -eq 1 ] || fail "the self-built nazm-check does not report the tree as the seed's build does"
[ "$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null || echo unknown)" = "$src_digest" ] \
    || fail "the tree changed during the run (the build-input digest); nothing it built is a record of one tree"
summary="stages 1 to $stages byte-identical; $hardened release executables hardened; $agree corpus programs agree; $iagree run by the self-built interpreter as by the seed's; nazm-check agrees; the 256 MiB heap stops with N0406; L1 user builds pass"
# The record: written last, only here, after every check above passed.
{
    echo "schema nazm.selfbuild-record/1"
    echo "script linux-selfbuild.sh"
    echo "arch $arch"
    echo "target $target"
    echo "backend $backend"
    echo "heap-limit $heap"
    echo "stages $stages"
    echo "source-commit $src_commit"
    echo "source-tree-clean $src_clean"
    echo "build-input-digest $src_digest"
    echo "seed-sha256 $(sha256_of "$seed")"
    echo "environment-sha256 $(sha256_of "$out/environment.txt")"
    echo "environment-kernel $env_kernel"
    echo "environment-clang $env_clang"
    echo "environment-libc $env_libc"
    for s in $(seq 0 "$stages"); do
        for name in $names; do
            [ -f "$out/stage$s/$name" ] && echo "sha256 stage$s $name $(sha256_of "$out/stage$s/$name")"
        done
    done
    echo "summary $summary"
    echo "result passed"
} > "$out/fixpoint.txt.part" && mv "$out/fixpoint.txt.part" "$out/fixpoint.txt" || fail "could not write $out/fixpoint.txt"
say "linux-selfbuild complete ($target, $backend backend): $summary"
