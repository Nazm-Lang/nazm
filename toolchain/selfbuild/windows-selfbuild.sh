#!/bin/bash
# R8 slice 17 — the released set built by Nazm for x86_64-pc-windows-gnu, and the Nazm-built
# toolchain's host-safe suite on a Windows host (`openspec/changes/r8-cut-over/design.md`,
# *Slice 17*, S17.1–S17.6).
#
#     toolchain/selfbuild/windows-selfbuild.sh HOST OUT
#
# STAGE (default ~/nazm-hostsb-aarch64-2/stage3) is a macOS stage a passed `host-selfbuild.sh` left:
# its `nazm` builds, through `host-run`, one program at a time:
#   - the nine programs of `programs.txt` for x86_64-pc-windows-gnu (`--objects`, linked here by
#     MinGW-w64's gcc as `link.txt` says), twice, and the two builds compared byte for byte;
#   - the same nine for this Mac from the same tree (the reference each Windows answer is held to);
#   - the heap probe (the front end at 256 MiB) and the runner's probes (`winprobe/`).
# The Windows programs are copied to HOST (an ssh destination whose shell is cmd.exe) under REMOTE
# (default C:\Users\nazmci\nazm-w17, which must not exist; any path under C:\Users is normalised alike) and run there only by `win-run.ps1`: one
# at a time, each in a job object with a memory ceiling, under a deadline, its output bounded, the
# run stopped at the first anomaly. Every Nazm-built program run here is one `host-run`
# (`toolchain/lib/stage-run.sh`), strictly one after another. REMOTE is removed at the end.
#
# TOOLS (default llvm-mingw's bin under C:\Users\nazmci\nazm-tools) is clang and its linker on
# HOST, for `nazm build` there. Nothing is timed. Exit 0 when every comparison agrees.
set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage_name=windows-selfbuild
. "$root/toolchain/lib/stage.sh"

say() { printf '%s\n' "$*"; }
exec 3>&2
fail() { printf 'windows-selfbuild: FAILED: %s\n' "$*" >&3; kill -TERM $$; exit 1; }
trap 'exit 1' TERM

[ $# -eq 2 ] || stage_fail "usage: windows-selfbuild.sh HOST OUT"
host=$1
out=$2
STAGE=${STAGE:-$HOME/nazm-hostsb-aarch64-2/stage3}
REMOTE=${REMOTE:-'C:\Users\nazmci\nazm-w17'}
TOOLS=${TOOLS:-'C:\Users\nazmci\nazm-tools\llvm-mingw-20261006-ucrt-x86_64\bin'}
GCC=${GCC:-x86_64-w64-mingw32-gcc}
target=x86_64-pc-windows-gnu

# --- the refusals: nothing has run when one of these stops the script -------------------------------
[ "$(uname -s)" = Darwin ] || stage_fail "refused [not-macos]: the Windows programs are built on the macOS host"
[ -z "${NAZM_CONTAINED:-}" ] || stage_fail "refused [host-environment]: NAZM_CONTAINED is set in the host's environment; unset it"
[ -z "${NAZM_ALLOW_UNCONTAINED:-}" ] || stage_fail "refused [host-environment]: NAZM_ALLOW_UNCONTAINED is set; it is never accepted"
if [ -n "$(docker ps -q --filter label=nazm.owner=nazm-contained-runner 2> /dev/null)" ]; then
    stage_fail "refused [contained-workload-running]: a contained workload is running; no host stage beside it"
fi
marker=${NAZM_HOST_RUN_DIR:-$HOME/.cache/nazm-host-run}/withdrawn
[ ! -e "$marker" ] || stage_fail "refused [withdrawn]: the host rule is withdrawn; marker: $marker"
for n in nazm pl cg; do [ -x "$STAGE/$n" ] || stage_fail "refused [no-stage]: $STAGE/$n is missing (STAGE names a passed host-selfbuild stage)"; done
command -v "$GCC" > /dev/null || stage_fail "refused [no-mingw]: $GCC is not on PATH (brew install mingw-w64)"
[ -n "$out" ] || stage_fail "usage: OUT is empty"
if [ -e "$out" ] && [ -n "$(ls -A "$out" 2> /dev/null)" ]; then stage_fail "refused [out-exists]: $out is not empty; give a new directory"; fi
ssh -o BatchMode=yes -o ConnectTimeout=15 "$host" "ver" > /dev/null 2>&1 || stage_fail "refused [no-host]: no Windows host answers as $host"
case $(ssh "$host" "if exist $REMOTE (echo yes) else (echo no)" | tr -d '\r') in
    no) ;;
    *) stage_fail "refused [remote-exists]: $REMOTE exists on $host; it is this script's scratch, and is removed only by the run that made it" ;;
esac

mkdir -p "$out" || stage_fail "cannot create $out"
out=$(CDPATH= cd -- "$out" && pwd)
peaks="$out/peaks.txt"
STAGE_PEAKS=$peaks
export STAGE_PEAKS
stage_init "$root"
[ "$STAGE_MODE" = host ] || stage_fail "not in host mode"
heap=$STAGE_HEAP
seed=${NAZM:-$(stage_checkout_nazm "$root")}
(cd "$root" && cargo build --locked -q -p nazm-cli) || fail "the seed (cargo build -p nazm-cli) did not build"
[ -x "$seed" ] || fail "no seed at $seed"
SDKROOT=$(xcrun --show-sdk-path 2> /dev/null) || stage_fail "xcrun --show-sdk-path failed"
MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
export SDKROOT MACOSX_DEPLOYMENT_TARGET
unset NAZM_PIPELINE NAZM_NZCG NAZM_LIBRARY
STAGE_MAX_OUTPUT_MIB=${STAGE_MAX_OUTPUT_MIB:-192}
export STAGE_MAX_OUTPUT_MIB

# `step LABEL DEADLINE EXE ARGS...`: ONE host-run (as host-selfbuild.sh's). A refusal or a
# withdrawal ends the run, but for `no-counted-heap` where STEP_ALLOW_NOHEAP is set.
errf="$out/.step.err"
step() {
    local label=$1 deadline=$2 st
    shift 2
    printf '# %s\n' "$label" >> "$peaks"
    "$STAGE_RUN" "$deadline" "$@" 2> "$errf" < "${STEP_STDIN:-/dev/null}"
    st=$?
    if [ "$st" -eq 70 ] || { grep -q '^host-run: refused \[' "$errf" && ! { [ -n "${STEP_ALLOW_NOHEAP:-}" ] && grep -q '^host-run: refused \[no-counted-heap\]' "$errf"; }; }; then
        cat "$errf" >&3
        fail "host-run stopped the run at '$label' (status $st): $(grep -m1 'refused\|WITHDRAWAL' "$errf")"
    fi
    return "$st"
}

# The image's counted heap and limit, read as host-run and win-run read them: the limit, or nothing.
heap_of() {
    LC_ALL=C grep -ao 'nazm: error\[N0406\]: out of memory: the heap limit of [0-9]* bytes is reached' "$1" 2> /dev/null \
        | sed 's/.*limit of \([0-9]*\) bytes.*/\1/' | sort -u | tr '\n' ' ' | sed 's/ $//'
}

programs=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$' | awk '{ print $2 ":" $3 }')

{
    echo "script: windows-selfbuild.sh $host (target $target)"
    echo "builder: $STAGE ($(file -b "$STAGE/nazm"))"
    echo "builder sha256: $(shasum -a 256 "$STAGE/nazm" | cut -c1-64)"
    echo "tree: $(cd "$root" && git rev-parse HEAD) $(cd "$root" && git status --porcelain | wc -l | tr -d ' ') paths changed"
    echo "gcc: $($GCC --version | head -1)"
    echo "clang: $(clang --version | head -1)"
    echo "seed: $("$seed" --version 2> /dev/null)"
    echo "host: $(ssh "$host" ver | tr -d '\r' | grep -o 'Version [0-9.]*')"
    echo "tools on host: $TOOLS"
    echo "heap limit: $heap"
} > "$out/environment.txt" 2>&1

# What the run is built from (R8 slice 18, S18.2), as host-selfbuild.sh records it; checked again at the end.
src_commit=$(git -C "$root" rev-parse HEAD 2> /dev/null || echo unknown)
if [ -z "$(git -C "$root" status --porcelain 2> /dev/null)" ]; then src_clean=yes; else src_clean=no; fi
src_digest=$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null) || src_digest=unknown
sha256_of() { shasum -a 256 "$1" 2> /dev/null | cut -d' ' -f1; }
{
    echo "source-commit: $src_commit"
    echo "source-tree-clean: $src_clean"
    echo "build-input-digest: $src_digest"
} >> "$out/environment.txt"

# --- S17.1: the nine programs for Windows, twice; the same nine for this Mac --------------------------
wcfg=(--backend llvm --no-cache --target "$target")
mcfg=(--release --backend llvm --no-cache --target aarch64-apple-darwin)
[ "$(uname -m)" = arm64 ] || mcfg=(--release --backend llvm --no-cache --target x86_64-apple-darwin)
lib_env=(NAZM_LIBRARY="$root/library" NAZM_PIPELINE="$STAGE/pl" NAZM_NZCG="$STAGE/cg")

# `wbuild DEST NAME SOURCE HEAP`: STAGE's nazm builds SOURCE for Windows into DEST/NAME.exe.
wbuild() {
    dest=$1; name=$2; src=$3; h=$4
    mkdir -p "$dest"
    (export "${lib_env[@]}"; step "win $(basename "$dest") $name build" 1800 "$STAGE/nazm" build "${wcfg[@]}" --heap-limit "$h" \
        "$src" --objects "$dest/$name.o" -o "$dest/$name" > "$dest/$name.build.out" 2> /dev/null) \
        || { cp "$errf" "$dest/$name.build.err"; fail "the Windows build of $src did not finish: $(head -c 400 "$errf")"; }
    cp "$errf" "$dest/$name.build.err"
    objs=$(sed -n 's/^objects //p' "$dest/$name.o/link.txt"); libs=$(sed -n 's/^libraries //p' "$dest/$name.o/link.txt")
    # shellcheck disable=SC2086
    (cd "$dest/$name.o" && "$GCC" $objs $libs -o "$dest/$name.exe") > "$dest/$name.link.log" 2>&1 \
        || fail "the link of $name.exe failed: $(head -c 400 "$dest/$name.link.log")"
    file -b "$dest/$name.exe" | grep -q 'PE32+ executable (console) x86-64' || fail "$name.exe is not a PE32+ x86-64 console program: $(file -b "$dest/$name.exe")"
    [ "$(heap_of "$dest/$name.exe")" = "$(stage_heap_bytes "$h")" ] || fail "$name.exe does not carry the counted heap at $h: '$(heap_of "$dest/$name.exe")'"
}
# WSB_REUSE=DIR takes the builds of an earlier run's OUT: a smoke run only, never the evidence.
if [ -n "${WSB_REUSE:-}" ]; then
    cp -R "$WSB_REUSE/win" "$WSB_REUSE/win2" "$WSB_REUSE/mac" "$out/" || fail "WSB_REUSE: no builds in $WSB_REUSE"
    say "WSB_REUSE=$WSB_REUSE: the builds are an earlier run's; a smoke run, not the evidence"
else
for d in win win2; do
    for p in $programs; do wbuild "$out/$d" "${p%%:*}" "$root/${p#*:}" "$heap"; done
done
same=0; differ=0
for p in $programs; do
    n=${p%%:*}
    if cmp -s "$out/win/$n.exe" "$out/win2/$n.exe" && diff -r "$out/win/$n.o" "$out/win2/$n.o" > /dev/null; then same=$((same + 1)); else differ=$((differ + 1)); say "  differs between the two builds: $n"; fi
done
say "S17.1 build: the nine programs built for $target twice by $STAGE/nazm: $same identical (objects and executables), $differ differ"
[ "$differ" -eq 0 ] || fail "the Windows builds are not reproducible"
wbuild "$out/win" probe-fe "$root/compiler/diagnose.nz" 256M
for p in spin flood; do wbuild "$out/win" "probe-$p" "$root/toolchain/selfbuild/winprobe/$p.nz" 256M; done
wbuild "$out/win" probe-fill "$root/toolchain/selfbuild/winprobe/fill.nz" 2G
wbuild "$out/win" probe-big "$root/toolchain/selfbuild/winprobe/fill.nz" 3G 2> /dev/null || true

mkdir -p "$out/mac"
for p in $programs; do
    n=${p%%:*}
    (export "${lib_env[@]}"; step "mac $n build" 1800 "$STAGE/nazm" build "${mcfg[@]}" --heap-limit "$heap" "$root/${p#*:}" -o "$out/mac/$n" \
        > "$out/mac/$n.build.out" 2> /dev/null) || fail "the reference build of $n did not finish: $(head -c 400 "$errf")"
done
say "S17.1 reference: the same nine built for this Mac from the same tree"
fi

# --- S17.4: the inputs ------------------------------------------------------------------------------
tree="$out/tree"
mkdir -p "$tree"
(cd "$root" && git ls-files -z | COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --null -cf "$out/tree.tar" -T -) || fail "the tree did not pack"
[ -f "$tree/../tree.tar" ] && [ "$(tar -tf "$out/tree.tar" | grep -c .)" -eq "$(cd "$root" && git ls-files | grep -c .)" ] || fail "the tree's tar does not hold every tracked file"
(cd "$tree" && tar -xf "$out/tree.tar") || fail "the tree did not unpack"
R=$REMOTE
RT="$R\\tree"
# The tree's absolute path as the Windows programs are given it: `C:/…` (S17.5).
WT="$(echo "$R" | tr '\\' '/')/tree"
RB="$R\\bin"
RG="$R\\got"
wenv="NAZM_LIBRARY=$RT\\library|NAZM_PIPELINE=$RB\\pl.exe|NAZM_NZCG=$RB\\cg.exe|NAZM_HOST_RUN_DIR=$R\\state|NAZM_USER_CACHE_DIR=$R\\cache|PATH=$TOOLS;C:\\Windows\\system32;C:\\Windows"
menv=(NAZM_LIBRARY="$tree/library" NAZM_PIPELINE="$out/mac/pl" NAZM_NZCG="$out/mac/cg" NAZM_USER_CACHE_DIR="$out/cache")
mkdir -p "$out/want" "$out/got" "$out/cb" "$out/ir" "$out/fx"
steps="$out/steps"
mkdir -p "$steps"
# `wstep FILE LABEL DEADLINE DIR STDIN ENV EXE ARGS`: one line of a win-run steps file; LABEL names got\LABEL.out/.err.
wstep() {
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\r\n' "$2" "$3" "$4" "$5" "$RG\\$2.out" "$RG\\$2.err" "$6" "$7" "${8:-}" >> "$1"
}
# `mref LABEL DEADLINE DIR EXE ARGS...`: the same on this Mac, one host-run, into want/LABEL.{out,err,status}.
mref() {
    label=$1; dl=$2; dir=$3; shift 3
    (cd "$dir" && export "${menv[@]}" && step "ref $label" "$dl" "$@" > "$out/want/$label.out"; echo $? > "$out/want/$label.out.status")
    cp "$errf" "$out/want/$label.err"
}

# The toolchain on its own files, as scripts/windows-toolchain.sh asks it, and the corpus: per file,
# run --interpret, check, fmt --check, build (the build's program run apart, in phase 2).
files=()
for f in "$tree"/bench/compare/nazm/*.nz "$tree"/examples/*.nz "$tree"/compiler/conformance/*.nz "$tree"/tests/compat/v1/run/*.nz "$tree"/toolchain/codegen/tests/programs/*.nz; do
    [ -f "$f" ] && files+=("${f#"$tree"/}")
done
# WSB_LIMIT=N takes the first N files and N corpus programs only: a smoke run, never the evidence.
if [ -n "${WSB_LIMIT:-}" ]; then files=("${files[@]:0:$WSB_LIMIT}"); say "WSB_LIMIT=$WSB_LIMIT: a smoke run, not the evidence"; fi
: > "$steps/a.txt"
cfiles=0
for rel in "${files[@]}"; do
    n=$(echo "$rel" | tr '/.' '__')
    wstep "$steps/a.txt" "$n.interp" 120 "$RT" NUL "$wenv" "$RB\\nazm.exe" "run --interpret $rel"
    wstep "$steps/a.txt" "$n.check" 120 "$RT" NUL "$wenv" "$RB\\nazm.exe" "check $rel"
    wstep "$steps/a.txt" "$n.fmt" 120 "$RT" NUL "$wenv" "$RB\\nazm.exe" "fmt --check $rel"
    mref "$n.interp" 120 "$tree" "$out/mac/nazm" run --interpret "$rel"
    mref "$n.check" 120 "$tree" "$out/mac/nazm" check "$rel"
    mref "$n.fmt" 120 "$tree" "$out/mac/nazm" fmt --check "$rel"
    # A program with C beside it (`NAME.c`) links that C: not built here, as scripts/windows-host.sh skips it.
    if [ -f "$tree/${rel%.nz}.c" ]; then cfiles=$((cfiles + 1)); continue; fi
    wstep "$steps/a.txt" "$n.build" 600 "$RT" NUL "$wenv" "$RB\\nazm.exe" "build --heap-limit 256M $rel -o $R\\cb\\$n.exe"
    mref "$n.build" 600 "$tree" "$out/mac/nazm" build --heap-limit 256M "$rel" -o "$out/cb/$n"
done
say "S17.4 references: ${#files[@]} files x 3 commands and $((${#files[@]} - cfiles)) builds ($cfiles with C beside them not built) run by the Mac build"
[ ${#files[@]} -gt 0 ] || fail "no files found in the tree"

# The interpreter over the corpus: Core IR from the seed, here, as host-selfbuild.sh makes it.
icorpus=()
for f in "$tree"/compiler/conformance/*.nz "$tree"/tests/compat/v1/run/*.nz "$tree"/toolchain/codegen/tests/programs/*.nz; do
    [ -n "${WSB_LIMIT:-}" ] && [ ${#icorpus[@]} -ge "$WSB_LIMIT" ] && break
    rel=${f#"$tree"/}; n=$(echo "$rel" | tr '/.' '__'); dir=$(dirname "$f"); file=$(basename "$f")
    if ! (cd "$dir" && "$seed" core-ir --json "$file" > "$out/ir/$n.ir.json" 2> /dev/null) \
        || grep -q '^{"schema":"nazm.core-ir/[23]","entry":null' "$out/ir/$n.ir.json"; then
        rm -f "$out/ir/$n.ir.json"; continue
    fi
    icorpus+=("$rel")
    wdir="$RT\\$(dirname "$rel" | tr '/' '\\')"
    wstep "$steps/a.txt" "$n.i" 60 "$wdir" NUL - "$RB\\interp.exe" "--std $RT\\library $R\\ir\\$n.ir.json $file"
    mref "$n.i" 60 "$dir" "$out/mac/interp" --std "$tree/library" "$out/ir/$n.ir.json" "$file"
done
say "S17.4 references: ${#icorpus[@]} corpus programs run by the Mac build's interpreter"

# Core IR of the nine sources by the front end, the same arguments on both hosts (the platform is an argument).
for p in $programs; do
    n=${p%%:*}; src=${p#*:}
    wstep "$steps/a.txt" "core-$n" 900 "$RT" NUL - "$RB\\fe.exe" "--coredoc-sources $WT/$src $WT/library macos aarch64"
    mref "core-$n" 900 "$tree" "$out/mac/fe" --coredoc-sources "$tree/$src" "$tree/library" macos aarch64
done
# nazm-check over the tree.
# The gates that run `cargo metadata` (layering, tokenizer reach, codegen reach, unsafe policy, third-party
# notices) are left out on both hosts: the Windows host has no cargo (S17.4).
gates=("network reach" "emission reach" "doc claims" "capability matrix" "mutations apply" "guide from grammar" "docs corpus" "performance claims" "agent governance" "dependency sources" "module boundaries")
wg=""; for g in "${gates[@]}"; do wg="$wg \"$g\""; done
wstep "$steps/a.txt" check-tree 900 "$RT" NUL - "$RB\\check.exe" "$WT$wg"
mref check-tree 900 "$tree" "$out/mac/check" "$tree" "${gates[@]}"
# jtool on small documents.
printf '{"a":1,"b":[1,2,{"c":"x"}]}\n' > "$out/fx/a.json"
printf '{"b":[1,2,{"c":"x"}],"a":1}\n' > "$out/fx/b.json"
printf '{"a":2}\n' > "$out/fx/c.json"
printf 'not json\n' > "$out/fx/d.json"
for t in "same a.json b.json" "same a.json c.json" "same a.json d.json" "lines a.json" "get a.json a b/2/c b/#"; do
    n=jtool-$(echo "$t" | tr ' /#.' '____')
    wstep "$steps/a.txt" "$n" 60 "$R\\fx" NUL - "$RB\\jtool.exe" "$t"
    # shellcheck disable=SC2086
    mref "$n" 60 "$out/fx" "$out/mac/jtool" $t
done
# nazm-mutate reading the tree (nothing applied).
for t in "list @/xtask/mutations/mutations.toml" "check @" "plan @"; do
    n=mutate-$(echo "$t" | tr ' /.@' '____')
    wstep "$steps/a.txt" "$n" 600 "$RT" NUL - "$RB\\mutate.exe" "${t//@/$WT}"
    # shellcheck disable=SC2086
    mref "$n" 600 "$tree" "$out/mac/mutate" ${t//@/$tree}
done
# nazm-mcp: initialize, ping, tools/list and one call, which runs `nazm` (NAZM_EXE) as the reference does.
{
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"w17","version":"1"}}}'
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"ping"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/list"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nazm.diagnostics","arguments":{}}}'
} > "$out/fx/mcp.in"
wstep "$steps/a.txt" mcp 300 "$RT" "$R\\fx\\mcp.in" "$wenv|NAZM_EXE=$RB\\nazm.exe" "$RB\\mcp.exe" "--root examples/max.nz"
# The reference is the Rust server (`crates/nazm-mcp`), which the port answers as, run directly with the
# seed as its `nazm`: `host-run` gives a Nazm-built program no standard input, so the Mac build cannot be asked.
(cd "$root" && cargo build --locked -q -p nazm-mcp) || fail "nazm-mcp (Rust) did not build"
(cd "$tree" && NAZM_EXE="$seed" NAZM_LIBRARY="$tree/library" perl -e 'alarm shift; exec @ARGV or exit 127' 300 "$(dirname "$seed")/nazm-mcp" --root examples/max.nz \
    < "$out/fx/mcp.in" > "$out/want/mcp.out" 2> "$out/want/mcp.err"; echo $? > "$out/want/mcp.out.status")
# The bound on Windows: the front end built with a 256 MiB heap must stop with N0406.
wstep "$steps/a.txt" probe-fe 900 "$RT" NUL - "$RB\\probe-fe.exe" "--coredoc-sources $WT/toolchain/commands/nazm.nz $WT/library macos aarch64"

# --- S17.3: the host's scratch, the runner's probes ---------------------------------------------------
cleanup() { ssh "$host" "rmdir /s /q $REMOTE" > /dev/null 2>&1; }
trap 'cleanup; exit 1' TERM
ssh "$host" "mkdir $R\\bin & mkdir $R\\got & mkdir $R\\state & mkdir $R\\cb & mkdir $R\\ir & mkdir $R\\fx & mkdir $R\\cache & mkdir $R\\tree" > /dev/null \
    || fail "could not make $REMOTE on $host"
scp -q "$out"/win/*.exe "$host:$(echo "$R" | tr '\\' '/')/bin/" || fail "copy of the executables failed"
scp -q "$root/toolchain/selfbuild/win-run.ps1" "$out/tree.tar" "$host:$(echo "$R" | tr '\\' '/')/" || fail "copy failed"
[ ${#icorpus[@]} -eq 0 ] || scp -q "$out"/ir/*.json "$host:$(echo "$R" | tr '\\' '/')/ir/" || fail "copy of the IR failed"
scp -q "$out"/fx/* "$host:$(echo "$R" | tr '\\' '/')/fx/" || fail "copy of the fixtures failed"
ssh "$host" "cd /d $RT && tar -xf $R\\tree.tar" > /dev/null || fail "the tree did not unpack on $host"
say "S17.4: executables, tree ($(wc -c < "$out/tree.tar" | tr -d ' ') bytes), IR and fixtures copied to $host:$REMOTE"

# `wrun NAME STEPS [win-run options]`: one win-run over a steps file; its own output and status kept.
wrun() {
    name=$1; s=$2; shift 2
    scp -q "$s" "$host:$(echo "$R" | tr '\\' '/')/$name.steps" || fail "copy of $name.steps failed"
    ssh "$host" "powershell -NoProfile -ExecutionPolicy Bypass -File $R\\win-run.ps1 -Steps $R\\$name.steps -Report $R\\$name.report -StateDir $R\\state $*" \
        2>&1 | tr -d '\r' | grep -v '^#< CLIXML' | grep -v '^<Objs' > "$out/$name.runner"
    ssh "$host" "type $R\\$name.report" 2> /dev/null | tr -d '\r' >> "$out/win-peaks.txt"
    tail -1 "$out/$name.runner"
}
# The probes: each must be stopped for its own reason, and nothing else.
printf 'probe-spin\t5\t%s\tNUL\t%s\t%s\t-\t%s\t\r\n' "$R" "$RG\\probe-spin.out" "$RG\\probe-spin.err" "$RB\\probe-spin.exe" > "$steps/p1.txt"
printf 'probe-flood\t120\t%s\tNUL\t%s\t%s\t-\t%s\t\r\n' "$R" "$RG\\probe-flood.out" "$RG\\probe-flood.err" "$RB\\probe-flood.exe" > "$steps/p2.txt"
printf 'probe-ceiling\t120\t%s\tNUL\t%s\t%s\t-\t%s\t\r\n' "$R" "$RG\\probe-ceiling.out" "$RG\\probe-ceiling.err" "$RB\\probe-fill.exe" > "$steps/p3.txt"
printf 'probe-big\t60\t%s\tNUL\t%s\t%s\t-\t%s\t\r\n' "$R" "$RG\\probe-big.out" "$RG\\probe-big.err" "$RB\\probe-big.exe" > "$steps/p4.txt"
probes_ok=0
wrun p1 "$steps/p1.txt" > /dev/null; grep -q "ANOMALY at 'probe-spin': deadline" "$out/p1.runner" && probes_ok=$((probes_ok + 1))
wrun p2 "$steps/p2.txt" -MaxOutMiB 8 > /dev/null; grep -q "ANOMALY at 'probe-flood': output" "$out/p2.runner" && probes_ok=$((probes_ok + 1))
wrun p3 "$steps/p3.txt" -CeilingMiB 300 > /dev/null; grep -q "ANOMALY at 'probe-ceiling': ceiling" "$out/p3.runner" && probes_ok=$((probes_ok + 1))
wrun p4 "$steps/p4.txt" > /dev/null; grep -q "refused \[heap-limit-too-large\]" "$out/p4.runner" && probes_ok=$((probes_ok + 1))
say "S17.3 runner probes: $probes_ok of 4 stopped for their own reason (deadline, output cap, memory ceiling, a 3 GiB limit refused)"
for p in p1 p2 p3 p4; do sed "s/^/  $p: /" "$out/$p.runner"; done
[ "$probes_ok" -eq 4 ] || { cleanup; fail "the runner's probes did not all hold; nothing else is run on $host"; }
ssh "$host" "del /q $R\\got\\probe-*" > /dev/null 2>&1

# --- S17.4: phase 1 on the host, then the built programs -------------------------------------------------
say "S17.4 phase 1: $(grep -c . "$steps/a.txt") steps on $host, one at a time"
last=$(wrun a "$steps/a.txt")
say "  $last"
case $last in "win-run: "*" steps run") ;; *) cleanup; fail "phase 1 stopped on $host: $last" ;; esac
fetch() {
    ssh "$host" "cd /d $R && tar -cf got.tar got cb" > /dev/null || fail "could not pack the results"
    scp -q "$host:$(echo "$R" | tr '\\' '/')/got.tar" "$out/got.tar" || fail "could not fetch the results"
    (cd "$out/got" && rm -rf got cb && tar -xf "$out/got.tar")
}
fetch
# Phase 2: each program the two builds wrote is run, where its executable carries the counted heap.
: > "$steps/b.txt"
noheap=0; noheap_names=""
for rel in "${files[@]}"; do
    n=$(echo "$rel" | tr '/.' '__')
    [ -f "$out/cb/$n" ] && [ -f "$out/got/cb/$n.exe" ] || continue
    if [ -z "$(heap_of "$out/cb/$n")" ] || [ -z "$(heap_of "$out/got/cb/$n.exe")" ]; then
        noheap=$((noheap + 1)); noheap_names="$noheap_names $rel"; continue
    fi
    wstep "$steps/b.txt" "$n.run" 60 "$RT" NUL - "$R\\cb\\$n.exe" ""
    mref "$n.run" 60 "$tree" "$out/cb/$n"
done
last=$(wrun b "$steps/b.txt")
say "S17.4 phase 2: $(grep -c . "$steps/b.txt") built programs run on $host: $last ($noheap with no counted heap not run, on either host)"
case $last in "win-run: "*" steps run") ;; *) cleanup; fail "phase 2 stopped on $host: $last" ;; esac
fetch
cleanup
say "S17.4: $REMOTE removed from $host"

# --- S17.6: the comparisons -----------------------------------------------------------------------------
# A Windows answer is held to the Mac's byte for byte; where it differs only in the line ending, the
# path of the tree, the path separator or the executable's `.exe`, it is counted apart, never as equal.
# Only a path under the scratch directory has its separators made `/` (a JSON `\n` is not a path).
norm() { # FILE SIDE [exe] -> normalised text on stdout
    if [ "$2" = mac ]; then sed -e "s|$tree|@R/tree|g" -e "s|$out/cb/|@R/cb/|g" "$1"
    else tr -d '\r' < "$1" | NORM_PARTS=$(printf "%s" "$REMOTE" | tr '\\' '|') perl -pe 'BEGIN { $re = join(q{(?:\\\\\\\\|\\\\|/)}, map { quotemeta } split /\|/, $ENV{NORM_PARTS}) } s{$re((?:(?:\\\\|\\|/)[A-Za-z0-9_.\-]+)*)}{my $p = $1; $p =~ s/\\\\|\\/\//g; "\@R$p"}ge; s/\.exe\b//g if $ENV{NORM_EXE}'; fi
}
exact=0; normal=0; diff_n=0; zero=0; tlsref=0
: > "$out/differences.txt"; : > "$out/agreements.txt"
for w in "$out"/want/*.out; do
    label=$(basename "$w" .out)
    g="$out/got/got/$label.out"
    if [ ! -f "$g" ]; then diff_n=$((diff_n + 1)); echo "MISSING $label" >> "$out/differences.txt"; continue; fi
    ws=$(cat "$w.status"); gs=$(tr -d '\r\n ' < "$g.status")
    case $label in *.build | *.run) export NORM_EXE=1 ;; *) unset NORM_EXE ;; esac
    # TLS on Windows is refused by decision (N0830, docs/user/support.md): counted apart, by name.
    if [ "$gs" = 1 ] && grep -q '^error\[N0830\]: TLS has no binding for `x86_64-pc-windows-gnu`' "$out/got/got/$label.err"; then
        tlsref=$((tlsref + 1)); echo "refused-by-decision $label (N0830 on Windows, status $ws on the Mac)" >> "$out/agreements.txt"; continue
    fi
    if [ "$ws" = "$gs" ] && cmp -s "$w" "$g" && cmp -s "$out/want/$label.err" "$out/got/got/$label.err"; then
        exact=$((exact + 1)); [ "$ws" = 0 ] && zero=$((zero + 1)); echo "exact $label status $ws" >> "$out/agreements.txt"; continue
    fi
    if [ "$ws" = "$gs" ] && [ "$(norm "$w" mac)" = "$(norm "$g" win)" ] && [ "$(norm "$out/want/$label.err" mac)" = "$(norm "$out/got/got/$label.err" win)" ]; then
        normal=$((normal + 1)); [ "$ws" = 0 ] && zero=$((zero + 1)); echo "normalised $label status $ws" >> "$out/agreements.txt"; continue
    fi
    diff_n=$((diff_n + 1))
    {
        echo "DIFFER $label (status $ws vs $gs)"
        diff <(norm "$w" mac) <(norm "$g" win) | head -8
        diff <(norm "$out/want/$label.err" mac) <(norm "$out/got/got/$label.err" win) | head -8
    } >> "$out/differences.txt"
done
pst=$(tr -d '\r\n ' < "$out/got/got/probe-fe.out.status" 2> /dev/null)
if [ "$pst" = 2 ] && grep -q 'error\[N0406\]' "$out/got/got/probe-fe.err"; then
    say "S17.4 heap probe on $host: the front end built with a 256 MiB heap stopped with $(tr -d '\r' < "$out/got/got/probe-fe.err" | head -1)"
else
    say "S17.4 heap probe on $host: FAILED: status $pst, $(head -c 300 "$out/got/got/probe-fe.err" 2> /dev/null)"; diff_n=$((diff_n + 1))
fi
for r in $noheap_names; do say "  not run (no counted heap): $r"; done
say "S17.6: $exact answers identical to the Mac's byte for byte, $normal equal after normalising line endings, paths and .exe, $diff_n differ, $tlsref refused on Windows by decision (TLS, N0830) ($zero of the agreeing answers have status 0; each listed in $out/agreements.txt, differences in $out/differences.txt)"
grep '^DIFFER\|^MISSING' "$out/differences.txt" | head -40
say "memory on $host (win-run, peak commit of one process, MiB): largest $(grep -o 'peak commit [0-9]*' "$out/win-peaks.txt" | awk '{ if ($3 > m) m = $3 } END { printf "%.1f", m / 1048576 }')"
[ "$diff_n" -eq 0 ] || fail "$diff_n answers differ"
[ "$(git -C "$root" rev-parse HEAD 2> /dev/null || echo unknown)" = "$src_commit" ] \
    && [ "$(sh "$root/scripts/source-digest.sh" --root "$root" 2> /dev/null || echo unknown)" = "$src_digest" ] \
    || fail "the tree changed during the run (commit or build-input digest); nothing it built is a record of one tree"
# The record the release path reads (S18.2): written last, only by a whole run that passed; a smoke run
# (WSB_REUSE, WSB_LIMIT) writes none.
if [ -z "${WSB_REUSE:-}" ] && [ -z "${WSB_LIMIT:-}" ]; then
    {
        echo "schema nazm.selfbuild-record/1"
        echo "script windows-selfbuild.sh"
        echo "target $target"
        echo "backend llvm"
        echo "heap-limit $heap"
        echo "source-commit $src_commit"
        echo "source-tree-clean $src_clean"
        echo "build-input-digest $src_digest"
        echo "seed-sha256 $(sha256_of "$seed")"
        echo "builder $STAGE/nazm"
        echo "builder-sha256 $(sha256_of "$STAGE/nazm")"
        echo "builder-record-sha256 $(sha256_of "$STAGE/../fixpoint.txt" || true)"
        echo "environment-sha256 $(sha256_of "$out/environment.txt")"
        for d in win win2; do
            for p in $programs; do n=${p%%:*}; echo "sha256 $d $n.exe $(sha256_of "$out/$d/$n.exe")"; done
        done
        echo "summary the nine built for $target twice, identical; $exact answers on $host identical to the Mac's, $normal equal after normalising, 0 differ, $tlsref refused by decision; the heap probe stopped with N0406"
        echo "result passed"
    } > "$out/fixpoint.txt.part" && mv "$out/fixpoint.txt.part" "$out/fixpoint.txt" || fail "could not write $out/fixpoint.txt"
fi
say "windows-selfbuild complete: the nine built for $target reproducibly; every answer agrees"
