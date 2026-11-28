#!/bin/bash
# linux-toolchain L3 — x86_64 Linux, cross-built on aarch64 and run under emulation
# (`openspec/changes/linux-toolchain/`, tasks 3.2 and 3.3).
#
#     cargo xtask contained linux-x86-build [smoke]          phase 1, the aarch64 image
#     cargo xtask contained --platform amd64 linux-x86-run   phase 2, the linux/amd64 image
#
#     toolchain/selfbuild/linux-x86-selfbuild.sh build RECORD OUT   (inside the aarch64 contained runner)
#     toolchain/selfbuild/linux-x86-selfbuild.sh run BUILD OUT      (inside the amd64 contained runner)
#
# What Windows was in 0.11.0 (`windows-selfbuild.sh`), for x86_64 Linux. There is no x86_64 Linux host
# here, so the x86_64 programs are built on aarch64 and run in a `linux/amd64` container, which this
# aarch64 machine emulates: the emulation is named in the record, and nothing here is timed.
#
# Phase 1, `build` (aarch64, natively). The builder is the stage 3 of a passed aarch64 Linux self-build
# (`linux-selfbuild.sh`, L2): RECORD is the directory its contained workload exported
# (`linux-selfbuild.tar.gz` beside its `fixpoint.txt`). The builder is taken from that record rather
# than rebuilt here, and is accepted only when the record passed, every stage 3 file has the digest
# the record gives, stage 3 = stage 2 by the record, and the record's build-input digest is this
# tree's: so the builder is the fixpoint of exactly the sources the x86_64 programs are built from,
# and the record names it by digest (`builder-record-sha256`), as the Windows record does. (Rebuilding
# it here would hide the one thing the record exists to say: which passed self-build made the
# release's x86_64 programs.) Then:
#   - the builder builds the nine programs of `programs.txt` for x86_64-unknown-linux-gnu, as L2
#     builds its stages (`--release --backend llvm --no-cache --heap-limit 2G`), twice; the two builds
#     must be identical byte for byte, and each program a hardened x86-64 ELF (PIE, stripped, BIND_NOW,
#     GNU_RELRO, a stack that is not executable);
#   - the front end is built for x86_64 with a 256 MiB heap (the heap probe phase 2 runs);
#   - the builder's own nine, run here, answer every question phase 2 asks of the x86_64 nine (the
#     answers, `want/`): the reference each x86_64 answer is held to. The builder already agreed with
#     the Rust seed in its own run (L2), so the comparison here is aarch64 against x86_64.
# Phase 1 exports one archive and its manifest (`build.txt`, the digest of every file in it).
#
# Phase 2, `run` (x86_64, emulated). BUILD is phase 1's export: the manifest must name this tree (its
# commit and build-input digest) and every file must have its digest. The x86_64 nine then answer the
# same questions (`got/`), in the same paths, so an answer may name a path and still compare:
#   - per file of the corpus (bench/compare, examples, conformance, compat, the code generator's
#     programs): `nazm run --interpret`, `nazm check`, `nazm fmt --check`, and `nazm build --release`
#     of it for the machine `nazm` runs on (but where C is beside it), the built program then run;
#   - the self-built interpreter over the corpus's Core IR (the seed's, made in phase 1);
#   - the front end's Core IR of the nine sources; `nazm-check` over the tree; `jtool` on small
#     documents; `nazm-mutate` reading the catalogue (nothing applied); `nazm-mcp` answering a session;
#   - the front end built with a 256 MiB heap must stop with N0406;
#   - L1 on x86_64: `nazm`, with `NAZM_CONTAINED` unset as a user's shell has it, builds and runs
#     hello, normally and with `--release` (a hardened x86-64 ELF), and `nazm run` runs it.
# Every answer (standard output, standard error, status) must be the aarch64 answer byte for byte.
# Only then is `OUT/fixpoint.txt` (`nazm.selfbuild-record/1`) written, last.
#
# Each program runs under the container's cgroup (4 GiB, no network, a pid limit) and a deadline, and
# carries the counted heap it was built with; its peak resident memory, the kernel's, is appended to
# OUT/peaks.txt. Under emulation the deadlines are ten times the native ones: a deadline here stops a
# hung program, it measures nothing.
set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage_name=linux-x86-selfbuild
. "$root/toolchain/lib/stage.sh"

say() { printf '%s\n' "$*"; }
exec 3>&2
fail() { printf 'linux-x86-selfbuild: FAILED: %s\n' "$*" >&3; kill -TERM $$; exit 1; }
trap 'exit 1' TERM

[ $# -eq 3 ] || stage_fail "usage: linux-x86-selfbuild.sh build RECORD OUT | run BUILD OUT"
phase=$1
input=$2
out=$3
target=x86_64-unknown-linux-gnu
backend=llvm

# --- the refusals: nothing has run when any of these stops the script ------------------------------
STAGE_HEAP=${STAGE_HEAP:-2G}
stage_init "$root" contained
heap=$STAGE_HEAP
[ "$heap" = 2G ] || stage_fail "refused [heap]: the release is built with a 2 GiB counted heap, not $heap"
[ "$(uname -s)" = Linux ] || stage_fail "refused [not-linux]: this runs in the contained runner's Linux"
case $phase:$(uname -m) in
    build:aarch64 | build:arm64) slow=1 ;;
    run:x86_64) slow=10 ;;
    build:*) stage_fail "refused [arch]: phase 1 (build) runs in the aarch64 image, where the builder runs natively, not on $(uname -m)" ;;
    run:*) stage_fail "refused [arch]: phase 2 (run) runs in the linux/amd64 image (\`--platform amd64\`), not on $(uname -m)" ;;
    *) stage_fail "usage: the phase is build or run, not '$phase'" ;;
esac
[ -d "$input" ] || stage_fail "refused [no-input]: $input is not a directory"
[ -n "$out" ] || stage_fail "usage: OUT is empty"
if [ -e "$out" ] && [ -n "$(ls -A "$out" 2> /dev/null)" ]; then
    stage_fail "refused [out-exists]: $out is not empty; give a new directory"
fi
for t in clang cc readelf sha256sum perl tar; do
    command -v "$t" > /dev/null 2>&1 || stage_fail "$t is required"
done
unset NAZM_PIPELINE NAZM_NZCG NAZM_LIBRARY NAZM_LIBEXEC NAZM_EXE

src_commit=${NAZM_SOURCE_COMMIT:-}
case $src_commit in
    *[!0-9a-f]* | '') stage_fail "refused [no-commit]: the source commit is not known (NAZM_SOURCE_COMMIT); run it through \`cargo xtask contained\`" ;;
esac
src_clean=${NAZM_SOURCE_TREE_CLEAN:-unknown}
case $src_clean in yes | no) ;; *) stage_fail "refused [no-tree-state]: NAZM_SOURCE_TREE_CLEAN is yes or no, not '$src_clean'" ;; esac
src_digest=$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null) || stage_fail "the build-input digest could not be taken"
sha256_of() { sha256sum "$1" 2> /dev/null | cut -d' ' -f1; }

mkdir -p "$out" || stage_fail "cannot create $out"
out=$(CDPATH= cd -- "$out" && pwd)
input=$(CDPATH= cd -- "$input" && pwd)
peaks="$out/peaks-$phase.txt"
: > "$peaks"

# The peak reader (`linux-selfbuild.sh`'s): `peak LABEL FILE DEADLINE CMD...`.
tools="$out/.tools-$phase"
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

names=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$' | awk '{ print $2 }')
programs=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$' | awk '{ print $2 ":" $3 }')
[ "$(echo "$names" | wc -l | tr -d ' ')" -eq 9 ] || stage_fail "programs.txt does not name nine programs"

# `elf_is FILE MACHINE`: "" when FILE is a 64-bit ELF executable for MACHINE (readelf's word), else why not.
elf_is() {
    h=$(readelf -hW "$1" 2> /dev/null) || { echo "not an ELF file"; return; }
    echo "$h" | grep -q 'Class:[[:space:]]*ELF64' || { echo "not ELF64"; return; }
    echo "$h" | grep -q "Machine:.*$2" || { echo "not $2: $(echo "$h" | grep Machine:)"; return; }
    echo "$h" | grep -Eq 'Type:[[:space:]]*(EXEC|DYN)' || { echo "not an executable: $(echo "$h" | grep Type:)"; return; }
}
# `elf_hardened FILE MACHINE`: "" when FILE is also a PIE, stripped, BIND_NOW with GNU_RELRO, and its
# stack not executable (what `--release` makes on Linux; `linux-selfbuild.sh`'s check), else why not.
elf_hardened() {
    why=$(elf_is "$1" "$2"); [ -z "$why" ] || { echo "$why"; return; }
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
heap_of() {
    LC_ALL=C grep -ao 'nazm: error\[N0406\]: out of memory: the heap limit of [0-9][0-9]* bytes is reached' "$1" 2> /dev/null \
        | sed 's/.*limit of \([0-9]*\) bytes.*/\1/' | sort -u | tr '\n' ' ' | sed 's/ $//'
}

errf="$out/.step.err"
# `step LABEL DEADLINE EXE ARGS...`: one program, under the deadline (times `slow`), its peak in peaks,
# its standard error in `$errf`, standard input empty; its status is returned.
step() {
    label=$1; deadline=$2; shift 2
    "$tools/peak" "$label" "$peaks" $((deadline * slow)) "$@" 2> "$errf" < /dev/null
}

# The nine whose answers are asked for are at $bin on both phases (the builder's on aarch64, the x86_64
# ones under emulation): the same path, so an answer that names one compares.
bin="$out/bin"
nenv=(NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$bin/pl" NAZM_NZCG="$bin/cg" NAZM_USER_CACHE_DIR="$out/cache")

# `ans DEST LABEL DEADLINE DIR STDIN EXE ARGS...`: one answer, DEST/LABEL.{out,err,status}.
ans() {
    dst=$1; label=$2; dl=$3; dir=$4; in=$5; shift 5
    (cd "$dir" && env "${nenv[@]}" NAZM_EXE="$bin/nazm" "$tools/peak" "$label" "$peaks" $((dl * slow)) "$@" \
        > "$dst/$label.out" 2> "$dst/$label.err" < "$in"; echo $? > "$dst/$label.status")
}

# The corpus files whose answers are compared, in one order on both phases.
corpus_files() {
    for f in "$root"/bench/compare/nazm/*.nz "$root"/examples/*.nz "$root"/compiler/conformance/*.nz \
        "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
        [ -f "$f" ] && echo "${f#"$root"/}"
    done
}

# `answers DEST`: every question both phases ask of the nine at $bin.
answers() {
    dst=$1
    mkdir -p "$dst" "$out/cb" "$out/cache"
    rm -f "$out"/cb/*
    for rel in $(corpus_files); do
        n=$(echo "$rel" | tr '/.' '__')
        ans "$dst" "$n.interp" 120 "$root" /dev/null "$bin/nazm" run --interpret "$rel"
        ans "$dst" "$n.check" 120 "$root" /dev/null "$bin/nazm" check "$rel"
        ans "$dst" "$n.fmt" 120 "$root" /dev/null "$bin/nazm" fmt --check "$rel"
        # A program with C beside it (`NAME.c`) links that C: not built, as windows-selfbuild.sh skips it.
        [ -f "$root/${rel%.nz}.c" ] && continue
        ans "$dst" "$n.build" 900 "$root" /dev/null "$bin/nazm" build --release --backend "$backend" --no-cache --heap-limit 256M "$rel" -o "$out/cb/$n"
        if [ -x "$out/cb/$n" ]; then
            ans "$dst" "$n.run" 60 "$root" /dev/null "$out/cb/$n"
        fi
    done
    # The interpreter over the Core IR the seed made in phase 1 (`ir/list.txt`: NAME DIR FILE).
    while read -r n dir file; do
        ans "$dst" "$n.i" 60 "$root/$dir" /dev/null "$bin/interp" --std "$root/library" "$out/ir/$n.ir.json" "$file"
    done < "$out/ir/list.txt"
    # Core IR of the nine sources by the front end; the platform is an argument, the same on both.
    for p in $programs; do
        ans "$dst" "core-${p%%:*}" 900 "$root" /dev/null "$bin/fe" --coredoc-sources "$root/${p#*:}" "$root/library" linux x86_64
    done
    # nazm-check over the tree, every gate.
    ans "$dst" check-tree 1800 "$root" /dev/null "$bin/check" "$root"
    # jtool on small documents.
    for t in "same a.json b.json" "same a.json c.json" "same a.json d.json" "lines a.json" "get a.json a b/2/c b/#"; do
        # shellcheck disable=SC2086
        ans "$dst" "jtool-$(echo "$t" | tr ' /#.' '____')" 60 "$out/fx" /dev/null "$bin/jtool" $t
    done
    # nazm-mutate reading the tree (nothing applied).
    for t in "list $root/xtask/mutations/mutations.toml" "check $root" "plan $root"; do
        # shellcheck disable=SC2086
        ans "$dst" "mutate-$(echo "${t%% *}")" 600 "$root" /dev/null "$bin/mutate" $t
    done
    # nazm-mcp: initialize, ping, tools/list and one call, which runs `nazm` (NAZM_EXE).
    ans "$dst" mcp 300 "$root" "$out/fx/mcp.in" "$bin/mcp" --root examples/max.nz
}

# --- phase 1 --------------------------------------------------------------------------------------
if [ "$phase" = build ]; then
    smoke=${LXS_SMOKE:-}
    seed=${NAZM:-$(stage_checkout_nazm "$root")}
    [ -x "$seed" ] || stage_fail "build the Rust seed first: cargo build -p nazm-cli"
    # The builder's record, checked before anything is built with it.
    [ -f "$input/fixpoint.txt" ] && [ -f "$input/linux-selfbuild.tar.gz" ] \
        || stage_fail "refused [no-builder-record]: $input holds no linux-selfbuild.tar.gz and fixpoint.txt (\`cargo xtask contained linux-selfbuild\` exports them)"
    rec="$input/fixpoint.txt"
    rfield() { sed -n "s/^$1 //p" "$rec" | head -1; }
    [ "$(head -1 "$rec")" = "schema nazm.selfbuild-record/1" ] && [ "$(tail -1 "$rec")" = "result passed" ] \
        || stage_fail "refused [builder-record]: $rec is not a passed nazm.selfbuild-record/1"
    [ "$(rfield script)" = linux-selfbuild.sh ] && [ "$(rfield target)" = aarch64-unknown-linux-gnu ] && [ "$(rfield heap-limit)" = 2G ] \
        || stage_fail "refused [builder-record]: $rec is not an aarch64 linux-selfbuild.sh record at 2G"
    mkdir -p "$out/builder"
    tar -C "$out/builder" -xzf "$input/linux-selfbuild.tar.gz" || stage_fail "refused [builder-record]: the record's archive does not unpack"
    brec="$out/builder/linux-selfbuild"
    cmp -s "$rec" "$brec/fixpoint.txt" || stage_fail "refused [builder-record]: the archive's fixpoint.txt is not the one beside it"
    for name in $names; do
        want=$(awk -v n="$name" '$1 == "sha256" && $2 == "stage3" && $3 == n { print $4 }' "$rec")
        prev=$(awk -v n="$name" '$1 == "sha256" && $2 == "stage2" && $3 == n { print $4 }' "$rec")
        [ -n "$want" ] && [ "$want" = "$prev" ] || stage_fail "refused [builder-record]: the record has no stage 3 $name equal to its stage 2"
        [ "$(sha256_of "$brec/stage3/$name")" = "$want" ] || stage_fail "refused [builder-record]: stage3/$name is not the file the record names"
    done
    if [ "$(rfield build-input-digest)" != "$src_digest" ]; then
        [ -n "$smoke" ] || stage_fail "refused [builder-tree]: the builder was built from the tree $(rfield build-input-digest) ($(rfield source-commit)), this is $src_digest: run \`cargo xtask contained linux-selfbuild\` on this tree first"
        say "LXS_SMOKE: the builder's tree ($(rfield build-input-digest)) is not this one: a smoke run, never the evidence"
    fi
    builder="$brec/stage3"
    builder_record_sha=$(sha256_of "$rec")

    {
        echo "source-commit: $src_commit"
        echo "source-tree-clean: $src_clean"
        echo "build-input-digest: $src_digest"
        echo "script: linux-x86-selfbuild.sh build (backend $backend, target $target, on aarch64 natively)"
        echo "kernel: $(uname -sr) $(uname -m)"
        echo "os-release: $(. /etc/os-release 2> /dev/null && echo "$PRETTY_NAME")"
        echo "clang --version:"; clang --version 2>&1
        echo "x86_64 linker: $(x86_64-linux-gnu-ld -v 2>&1 | head -1)"
        echo "x86_64 C library: $(dpkg-query -W -f '${Package} ${Version}' libc6-dev-amd64-cross 2>&1), $(dpkg-query -W -f '${Package} ${Version}' libgcc-14-dev-amd64-cross 2>&1)"
        echo "libc: $(getconf GNU_LIBC_VERSION 2> /dev/null)"
        echo "cgroup memory.max: $(cat /sys/fs/cgroup/memory.max 2>&1)"
        echo "seed: $("$seed" --version 2> /dev/null)"
        echo "seed sha256: $(sha256_of "$seed")"
        echo "builder: stage 3 of the record $builder_record_sha ($(rfield source-commit), digest $(rfield build-input-digest))"
        echo "builder sha256: $(sha256_of "$builder/nazm")"
        echo "heap limit: $heap"
    } > "$out/environment-build.txt" 2>&1
    say "linux-x86-selfbuild build: target $target, builder $builder_record_sha, commit $src_commit (clean: $src_clean), digest $src_digest"

    # The nine for x86_64, twice, by the builder.
    xbuild() { # DEST NAME SOURCE HEAP
        NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$builder/pl" NAZM_NZCG="$builder/cg" \
            step "$(basename "$1") $2 build" 1800 "$builder/nazm" build --release --backend "$backend" --no-cache \
            --target "$target" --heap-limit "$4" "$root/$3" -o "$1/$2" > "$1/$2.build.out" \
            || { cp "$errf" "$1/$2.build.err"; fail "the x86_64 build of $3 did not finish: $(tail -c 1200 "$errf")"; }
        cp "$errf" "$1/$2.build.err"
    }
    mkdir -p "$out/x86" "$out/x86b" "$out/probe"
    for d in x86 x86b; do
        for p in $programs; do xbuild "$out/$d" "${p%%:*}" "${p#*:}" "$heap"; done
    done
    same=0; differ=0; hardened=0
    for name in $names; do
        if cmp -s "$out/x86/$name" "$out/x86b/$name"; then same=$((same + 1)); else differ=$((differ + 1)); say "  differs between the two builds: $name"; fi
        why=$(elf_hardened "$out/x86/$name" X86-64)
        if [ -z "$why" ]; then hardened=$((hardened + 1)); else say "  $name is not a hardened x86-64 release executable: $why"; fi
        [ "$(heap_of "$out/x86/$name")" = "$(stage_heap_bytes "$heap")" ] || fail "$name does not carry the counted heap at $heap: '$(heap_of "$out/x86/$name")'"
    done
    say "3.2: the nine built for $target twice by the builder: $same identical, $differ differ; $hardened of 9 x86-64 PIEs, stripped, BIND_NOW, GNU_RELRO, stack not executable"
    [ "$differ" -eq 0 ] || fail "the x86_64 builds are not reproducible"
    [ "$hardened" -eq 9 ] || fail "not every x86_64 program is a hardened release executable"
    xbuild "$out/probe" fe compiler/diagnose.nz 256M

    # The Core IR the interpreter is given, from the seed; the fixtures.
    mkdir -p "$out/ir" "$out/fx"
    : > "$out/ir/list.txt"
    for f in "$root"/compiler/conformance/*.nz "$root"/tests/compat/v1/run/*.nz "$root"/toolchain/codegen/tests/programs/*.nz; do
        [ -f "$f" ] || continue
        rel=${f#"$root"/}; n=$(echo "$rel" | tr '/.' '__'); dir=$(dirname "$rel"); file=$(basename "$f")
        if ! (cd "$root/$dir" && "$seed" core-ir --json "$file" > "$out/ir/$n.ir.json" 2> /dev/null) \
            || grep -q '^{"schema":"nazm.core-ir/[23]","entry":null' "$out/ir/$n.ir.json"; then
            rm -f "$out/ir/$n.ir.json"; continue
        fi
        echo "$n $dir $file" >> "$out/ir/list.txt"
    done
    printf '{"a":1,"b":[1,2,{"c":"x"}]}\n' > "$out/fx/a.json"
    printf '{"b":[1,2,{"c":"x"}],"a":1}\n' > "$out/fx/b.json"
    printf '{"a":2}\n' > "$out/fx/c.json"
    printf 'not json\n' > "$out/fx/d.json"
    {
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"l3","version":"1"}}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"ping"}'
        printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/list"}'
        printf '%s\n' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nazm.diagnostics","arguments":{}}}'
    } > "$out/fx/mcp.in"

    # The builder's answers: the reference.
    mkdir -p "$bin"
    for name in $names; do cp "$builder/$name" "$bin/$name"; done
    answers "$out/want"
    say "3.3 references: $(find "$out/want" -name '*.status' | wc -l | tr -d ' ') answers by the builder's nine (aarch64), $(grep -c . "$out/ir/list.txt") corpus programs with Core IR"
    rm -rf "$bin" "$out/cb" "$out/cache"

    [ "$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null || echo unknown)" = "$src_digest" ] \
        || fail "the tree changed during the run (the build-input digest)"
    # The manifest phase 2 checks: what the run is, and the digest of every file it hands over.
    {
        echo "schema nazm.linux-x86-build/1"
        echo "source-commit $src_commit"
        echo "source-tree-clean $src_clean"
        echo "build-input-digest $src_digest"
        if [ -n "$smoke" ]; then echo "smoke yes"; else echo "smoke no"; fi
        echo "seed-sha256 $(sha256_of "$seed")"
        echo "builder-record-sha256 $builder_record_sha"
        echo "builder-record-commit $(rfield source-commit)"
        echo "builder-sha256 $(sha256_of "$builder/nazm")"
        (cd "$out" && find environment-build.txt peaks-build.txt x86 x86b probe want ir fx -type f | LC_ALL=C sort | while read -r f; do echo "file $(sha256_of "$f") $f"; done)
        echo "result built"
    } > "$out/build.txt"
    say "linux-x86-selfbuild build complete: the nine for $target, identical twice, and the aarch64 answers"
    exit 0
fi

# --- phase 2 --------------------------------------------------------------------------------------
[ -f "$input/build.txt" ] && [ -f "$input/linux-x86-build.tar.gz" ] \
    || stage_fail "refused [no-build]: $input holds no linux-x86-build.tar.gz and build.txt (\`cargo xtask contained linux-x86-build\` exports them)"
tar -C "$out" -xzf "$input/linux-x86-build.tar.gz" || stage_fail "refused [build]: the archive does not unpack"
man="$out/build.txt"
cmp -s "$input/build.txt" "$man" || stage_fail "refused [build]: the archive's build.txt is not the one beside it"
mfield() { sed -n "s/^$1 //p" "$man" | head -1; }
[ "$(head -1 "$man")" = "schema nazm.linux-x86-build/1" ] && [ "$(tail -1 "$man")" = "result built" ] || stage_fail "refused [build]: $man is not a whole phase 1 manifest"
smoke=; [ "$(mfield smoke)" = yes ] && smoke=yes
if [ "$(mfield source-commit)" != "$src_commit" ] || [ "$(mfield build-input-digest)" != "$src_digest" ]; then
    [ -n "$smoke" ] || stage_fail "refused [build]: phase 1 built $(mfield source-commit) (digest $(mfield build-input-digest)), this tree is $src_commit ($src_digest)"
    say "LXS_SMOKE: phase 1 ran on another tree ($(mfield build-input-digest)); a smoke run writes no record"
fi
files=0
while read -r _ want f; do
    [ "$(cd "$out" && sha256_of "$f")" = "$want" ] || stage_fail "refused [build]: $f is not the file phase 1 named"
    files=$((files + 1))
done < <(grep '^file ' "$man")
[ "$(cd "$out" && find environment-build.txt peaks-build.txt x86 x86b probe want ir fx -type f | wc -l | tr -d ' ')" -eq "$files" ] \
    || stage_fail "refused [build]: the archive holds files the manifest does not name"

# The emulation, as this container sees it: the supervisor's machine against this one's.
case ${NAZM_SUPERVISOR_ARCH:-unknown} in
    x86_64) emulation="none (an x86_64 host)" ;;
    aarch64)
        if grep -q '/run/rosetta/rosetta' /proc/self/maps 2> /dev/null; then
            emulation="rosetta (an aarch64 host; Docker's Rosetta runs the x86_64 programs: /run/rosetta/rosetta is mapped into them)"
        else
            emulation="qemu (an aarch64 host; the x86_64 programs run under the VM's binfmt emulator)"
        fi
        ;;
    *) stage_fail "refused [no-supervisor-arch]: NAZM_SUPERVISOR_ARCH names the host's machine (aarch64 or x86_64), not '${NAZM_SUPERVISOR_ARCH:-}'" ;;
esac
env_clang=$(clang --version 2> /dev/null | head -1)
env_libc=$(getconf GNU_LIBC_VERSION 2> /dev/null)
{
    cat "$out/environment-build.txt"
    echo "--- phase 2"
    echo "script: linux-x86-selfbuild.sh run (target $target)"
    echo "emulation: $emulation"
    echo "kernel: $(uname -sr) $(uname -m)"
    echo "os-release: $(. /etc/os-release 2> /dev/null && echo "$PRETTY_NAME")"
    echo "cpu: $(grep -m1 'model name' /proc/cpuinfo 2> /dev/null | sed 's/.*: //')"
    echo "clang --version:"; clang --version 2>&1
    echo "ld -v: $(ld -v 2>&1 | head -1)"
    echo "libc: $env_libc ($(ldd --version 2>&1 | head -1))"
    echo "cgroup memory.max: $(cat /sys/fs/cgroup/memory.max 2>&1)"
    echo "cgroup pids.max: $(cat /sys/fs/cgroup/pids.max 2>&1)"
    echo "heap limit: $heap"
} > "$out/environment.txt" 2>&1
say "linux-x86-selfbuild run: $target under emulation: $emulation; $env_clang, $env_libc"
[ -z "$smoke" ] || say "LXS_SMOKE: phase 1 was a smoke run; no record is written"

bad=0
mkdir -p "$bin"
for name in $names; do
    cp "$out/x86/$name" "$bin/$name"
    why=$(elf_hardened "$bin/$name" X86-64)
    [ -z "$why" ] || { bad=$((bad + 1)); say "  $name is not a hardened x86-64 release executable: $why"; }
done
answers "$out/got"

# The comparison: every answer byte for byte.
exact=0; zero=0; diff_n=0
: > "$out/differences.txt"; : > "$out/agreements.txt"
for w in "$out"/want/*.status; do
    label=$(basename "$w" .status)
    g="$out/got/$label"
    if [ ! -f "$g.status" ]; then diff_n=$((diff_n + 1)); echo "MISSING $label" >> "$out/differences.txt"; continue; fi
    ws=$(cat "$w"); gs=$(cat "$g.status")
    if [ "$ws" = "$gs" ] && cmp -s "$out/want/$label.out" "$g.out" && cmp -s "$out/want/$label.err" "$g.err"; then
        exact=$((exact + 1)); [ "$ws" = 0 ] && zero=$((zero + 1)); echo "exact $label status $ws" >> "$out/agreements.txt"; continue
    fi
    diff_n=$((diff_n + 1))
    {
        echo "DIFFER $label (status $ws on aarch64, $gs on x86_64)"
        diff "$out/want/$label.out" "$g.out" | head -8 | cut -c1-240
        diff "$out/want/$label.err" "$g.err" | head -8 | cut -c1-240
    } >> "$out/differences.txt"
done
for g in "$out"/got/*.status; do
    [ -f "$out/want/$(basename "$g")" ] || { diff_n=$((diff_n + 1)); echo "EXTRA $(basename "$g" .status)" >> "$out/differences.txt"; }
done
total=$(ls "$out"/want/*.status | wc -l | tr -d ' ')
say "3.3: $exact of $total answers of the x86_64 nine identical to the aarch64 ones byte for byte ($zero with status 0), $diff_n differ"
grep -A16 '^DIFFER\|^MISSING\|^EXTRA' "$out/differences.txt" | head -80

# The bound: the front end built with a 256 MiB heap stops with N0406.
step "probe fe-under-256MiB" 900 "$out/probe/fe" --coredoc-sources "$root/toolchain/commands/nazm.nz" "$root/library" linux x86_64 > /dev/null
pst=$?
cp "$errf" "$out/probe/err"
if [ "$pst" -eq 2 ] && grep -q 'error\[N0406\]' "$out/probe/err"; then
    say "heap probe: the x86_64 front end built with a 256 MiB heap stopped with $(head -1 "$out/probe/err")"
else
    bad=$((bad + 1)); say "heap probe: FAILED: status $pst, $(head -c 300 "$out/probe/err")"
fi

# L1 on x86_64: the x86_64 `nazm` with NAZM_CONTAINED unset, as a user's shell has it.
l1="$out/l1"
mkdir -p "$l1"
printf 'fn main() {\n    println("Hello from Nazm on Linux")\n}\n' > "$l1/hello.nz"
printf 'Hello from Nazm on Linux\n' > "$l1/expected.stdout"
user() {
    label=$1; deadline=$2; shift 2
    (cd "$l1" && env -u NAZM_CONTAINED NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$bin/pl" NAZM_NZCG="$bin/cg" \
        "$tools/peak" "$label" "$peaks" $((deadline * slow)) "$@" 2> "$errf" < /dev/null)
}
l1ok=0
for kind in plain release; do
    flag=; [ $kind = release ] && flag=--release
    if ! user "l1 $kind build" 900 "$bin/nazm" build $flag hello.nz -o "hello-$kind" > "$l1/$kind.build.out"; then
        cp "$errf" "$l1/$kind.build.err"; say "  L1: \`nazm build $flag\` of hello.nz failed without NAZM_CONTAINED: $(tail -c 1200 "$l1/$kind.build.err")"; continue
    fi
    if [ $kind = release ]; then why=$(elf_hardened "$l1/hello-$kind" X86-64); else why=$(elf_is "$l1/hello-$kind" X86-64); fi
    [ -z "$why" ] || { say "  L1: the $kind build is not what it should be: $why"; continue; }
    user "l1 $kind run" 60 "$l1/hello-$kind" > "$l1/$kind.stdout"; st=$?
    if [ "$st" -eq 0 ] && cmp -s "$l1/expected.stdout" "$l1/$kind.stdout"; then
        l1ok=$((l1ok + 1))
    else
        say "  L1: the $kind build ran with status $st and printed: $(head -c 200 "$l1/$kind.stdout"; head -c 300 "$errf")"
    fi
done
if user "l1 run" 900 "$bin/nazm" run hello.nz > "$l1/run.stdout" && cmp -s "$l1/expected.stdout" "$l1/run.stdout"; then
    l1ok=$((l1ok + 1))
else
    say "  L1: \`nazm run hello.nz\` without NAZM_CONTAINED: $(head -c 200 "$l1/run.stdout"; head -c 300 "$errf")"
fi
if [ "$l1ok" -eq 3 ]; then
    say "L1 on x86_64: without NAZM_CONTAINED, the x86_64 nazm built hello.nz (x86-64 ELF) and with --release (PIE, stripped, BIND_NOW, GNU_RELRO, stack not executable); both and \`nazm run\` printed the expected line"
else
    bad=$((bad + 1)); say "L1 on x86_64: FAILED: $l1ok of 3 user builds and runs as expected"
fi
# The driver's Linux answers (task 1.3), the test that lives with build.nz's: the admission of the Linux
# triples, --release on Linux and the refusals around them, NAZM_CONTAINED unset.
if bash "$root/toolchain/commands/tests/linux-build.sh" "$bin" "$l1/driver" > "$l1/driver.txt" 2>&1; then
    say "L1 on x86_64, the driver: $(tail -1 "$l1/driver.txt") (toolchain/commands/tests/linux-build.sh)"
else
    bad=$((bad + 1)); say "L1 on x86_64, the driver: FAILED (toolchain/commands/tests/linux-build.sh):"; sed 's/^/  /' "$l1/driver.txt"
fi

say "memory (the kernel's ru_maxrss under emulation, KiB in peaks-run.txt): the largest step: $(awk '{ p = $(NF - 1); if (p > m) { m = p; l = $0 } } END { printf "%.1f MiB (%s)", m / 1024, l }' "$peaks")"

[ "$bad" -eq 0 ] || fail "$bad checks failed"
[ "$diff_n" -eq 0 ] || fail "$diff_n answers differ from the aarch64 ones"
[ "$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null || echo unknown)" = "$src_digest" ] \
    || fail "the tree changed during the run (the build-input digest)"
summary="the nine built for $target twice by the aarch64 stage 3 of record $(mfield builder-record-sha256), identical, hardened; under emulation ($(echo "$emulation" | cut -d' ' -f1)) $exact of $total answers identical to the aarch64 ones; the 256 MiB heap stops with N0406; L1 user builds pass"
if [ -n "$smoke" ]; then
    say "linux-x86-selfbuild run complete (smoke, no record): $summary"
    exit 0
fi
# The record: written last, only here, after every check above passed.
{
    echo "schema nazm.selfbuild-record/1"
    echo "script linux-x86-selfbuild.sh"
    echo "arch x86_64"
    echo "target $target"
    echo "backend $backend"
    echo "heap-limit $heap"
    echo "emulation $emulation"
    echo "source-commit $src_commit"
    echo "source-tree-clean $src_clean"
    echo "build-input-digest $src_digest"
    echo "seed-sha256 $(mfield seed-sha256)"
    echo "builder-sha256 $(mfield builder-sha256)"
    echo "builder-record-sha256 $(mfield builder-record-sha256)"
    echo "builder-record-commit $(mfield builder-record-commit)"
    echo "environment-sha256 $(sha256_of "$out/environment.txt")"
    echo "environment-clang $env_clang"
    echo "environment-libc $env_libc"
    for d in x86 x86b; do
        for name in $names; do echo "sha256 $d $name $(sha256_of "$out/$d/$name")"; done
    done
    echo "summary $summary"
    echo "result passed"
} > "$out/fixpoint.txt.part" && mv "$out/fixpoint.txt.part" "$out/fixpoint.txt" || fail "could not write $out/fixpoint.txt"
say "linux-x86-selfbuild complete ($target): $summary"
