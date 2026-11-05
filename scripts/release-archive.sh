#!/bin/bash
# The release archive of a cut-over target, from the Nazm-built binaries (R8 slice 18, the cut-over;
# `openspec/changes/r8-cut-over/design.md`, S18.1-S18.5).
#
#   scripts/release-archive.sh [--allow-dirty] TRIPLE RECORD_DIR OUTDIR
#
# TRIPLE is one of the three targets 0.11.0 ships: aarch64-apple-darwin, x86_64-apple-darwin,
# x86_64-pc-windows-gnu. RECORD_DIR is the OUT of a passed self-build of this checkout's commit:
#
#   - macOS: `toolchain/selfbuild/host-selfbuild.sh aarch64|x86_64 OUT` (seed -> stage 1 -> 2 -> 3,
#     every Nazm-built step one `host-run`); what ships is its stage 3, which equals stage 2;
#   - Windows: `toolchain/selfbuild/windows-selfbuild.sh HOST OUT`, whose builder is the stage 3 of a
#     passed macOS self-build of the same commit; what ships is its first build, which equals its second.
#
# The Rust seed is never packaged here: it is stage 0, and it is named in the record by its digest only.
# Nothing here runs a Nazm-built program (the version is read from the source), and nothing is uploaded,
# tagged or signed: signing is the copyright holder's step (scripts/sign-release.sh), after the gates.
#
# Writes OUTDIR/nazm-TRIPLE.tar.gz (scripts/reproducible-tar.py: the same bytes for the same inputs) and
# rewrites OUTDIR/SHA256SUMS over every nazm-*.tar.gz there. Refuses, naming why, unless the record says
# `result passed`, names this commit and this tree's build-input digest, every digest in it is the file's
# on disk, the stages agree byte for byte, and each shipped image is what the target needs (format,
# architecture, the counted heap at 2 GiB, and on macOS an ad hoc signature that verifies and no debug map).
# --allow-dirty lets a tree with uncommitted changes through and says so in SELFBUILD.txt: never for a release.
set -euo pipefail

say() { printf 'release-archive: %s\n' "$*"; }
refuse() { printf 'release-archive: refused [%s]: %s\n' "$1" "$2" >&2; exit 2; }

allow_dirty=no
if [ "${1:-}" = --allow-dirty ]; then allow_dirty=yes; shift; fi
[ $# -eq 3 ] || { printf 'usage: %s [--allow-dirty] TRIPLE RECORD_DIR OUTDIR\n' "$0" >&2; exit 2; }
triple=$1 rec_dir=$2 outdir=$3
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

[ "$(uname -s)" = Darwin ] || refuse not-macos "the release archives are made on the macOS host the self-builds ran on (the signature is verified with codesign)"
case $triple in
    aarch64-apple-darwin) kind=macos arch=aarch64 file_arch=arm64 exe= ;;
    x86_64-apple-darwin) kind=macos arch=x86_64 file_arch=x86_64 exe= ;;
    x86_64-pc-windows-gnu) kind=windows arch=x86_64 file_arch= exe=.exe ;;
    *) refuse not-a-release-target "$triple is not one of the three cut-over targets (aarch64-apple-darwin, x86_64-apple-darwin, x86_64-pc-windows-gnu); the seed's archive of other targets is scripts/host-archive.sh, internal only" ;;
esac
[ -d "$rec_dir" ] || refuse no-record "$rec_dir is not a directory"
rec_dir=$(CDPATH= cd -- "$rec_dir" && pwd)
rec="$rec_dir/fixpoint.txt"
[ -f "$rec" ] || refuse no-record "$rec is missing: only a self-build that passed writes it"

sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
field() { sed -n "s/^$1 //p" "$2" | head -1; }

# --- the record -------------------------------------------------------------------------------------
[ "$(head -1 "$rec")" = "schema nazm.selfbuild-record/1" ] || refuse bad-record "$rec is not a nazm.selfbuild-record/1"
[ "$(tail -1 "$rec")" = "result passed" ] || refuse not-passed "$rec does not end with 'result passed'"
want_script=host-selfbuild.sh; [ $kind = windows ] && want_script=windows-selfbuild.sh
[ "$(field script "$rec")" = "$want_script" ] || refuse wrong-record "$rec is $(field script "$rec")'s, not $want_script's"
[ "$(field target "$rec")" = "$triple" ] || refuse wrong-target "$rec is for $(field target "$rec"), not $triple"
[ "$(field backend "$rec")" = llvm ] || refuse wrong-backend "$rec was built on the $(field backend "$rec") backend; the released one is llvm"
[ "$(field heap-limit "$rec")" = 2G ] || refuse wrong-heap-limit "$rec was built with heap limit $(field heap-limit "$rec"), not the released 2G"
if [ $kind = macos ]; then [ "$(field stages "$rec")" = 3 ] || refuse wrong-stages "$rec ran $(field stages "$rec") stages, not 3"; fi

commit=$(git -C "$root" rev-parse HEAD)
[ "$(field source-commit "$rec")" = "$commit" ] || refuse wrong-commit "$rec was built from $(field source-commit "$rec"); this checkout is $commit"
digest=$(sh "$root/scripts/source-digest.sh" --root "$root")
[ "$(field build-input-digest "$rec")" = "$digest" ] || refuse wrong-digest "the build-input digest in $rec is not this tree's ($digest)"
clean=yes
[ -z "$(git -C "$root" status --porcelain)" ] || clean=no
if [ "$clean" = no ] || [ "$(field source-tree-clean "$rec")" != yes ]; then
    [ "$allow_dirty" = yes ] || refuse dirty "the tree (now: clean=$clean; at the build: $(field source-tree-clean "$rec")) has uncommitted changes; commit, rebuild, or --allow-dirty for a dry run"
    clean="no (--allow-dirty)"
fi
[ "$(field environment-sha256 "$rec")" = "$(sha256_of "$rec_dir/environment.txt")" ] || refuse tampered "$rec_dir/environment.txt is not the file the record names"

# Every digest in the record is the file's on disk now.
while read -r _ where name want; do
    f="$rec_dir/$where/$name"
    [ -f "$f" ] || refuse tampered "$rec names $where/$name, which is missing"
    [ "$(sha256_of "$f")" = "$want" ] || refuse tampered "$where/$name is not the file the record names"
done < <(grep '^sha256 ' "$rec")

programs=$(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | grep -v '^[[:space:]]*$' | awk '{ print $2 }')
recsha() { awk -v w="$1" -v n="$2" '$1 == "sha256" && $2 == w && $3 == n { print $4 }' "$rec"; }

if [ $kind = macos ]; then
    from=stage3
    for n in $programs; do
        for s in 1 2 3; do [ -n "$(recsha stage$s "$n")" ] || refuse incomplete "$rec has no stage$s $n"; done
        cmp -s "$rec_dir/stage1/$n" "$rec_dir/stage2/$n" && cmp -s "$rec_dir/stage2/$n" "$rec_dir/stage3/$n" \
            || refuse not-a-fixpoint "$n: stages 1, 2 and 3 are not the same bytes"
    done
    builder_lines=
else
    from=win
    for n in $programs; do
        [ -n "$(recsha win "$n.exe")" ] && [ -n "$(recsha win2 "$n.exe")" ] || refuse incomplete "$rec has no win/win2 $n.exe"
        cmp -s "$rec_dir/win/$n.exe" "$rec_dir/win2/$n.exe" || refuse not-reproducible "$n.exe: the two Windows builds are not the same bytes"
    done
    # The builder is a macOS stage 3 of a passed self-build of the same commit.
    builder=$(field builder "$rec")
    brec="$(dirname -- "$(dirname -- "$builder")")/fixpoint.txt"
    [ -f "$brec" ] || refuse no-builder-record "the builder's record $brec is missing"
    [ "$(sha256_of "$brec")" = "$(field builder-record-sha256 "$rec")" ] || refuse tampered "$brec is not the builder record $rec names"
    [ "$(tail -1 "$brec")" = "result passed" ] && [ "$(field script "$brec")" = host-selfbuild.sh ] && [ "$(field backend "$brec")" = llvm ] \
        || refuse builder-not-passed "$brec is not a passed llvm host-selfbuild record"
    [ "$(field source-commit "$brec")" = "$commit" ] && [ "$(field build-input-digest "$brec")" = "$digest" ] \
        || refuse builder-wrong-commit "the builder was self-built from another tree than this one"
    bsha=$(awk '$1 == "sha256" && $2 == "stage3" && $3 == "nazm" { print $4 }' "$brec")
    [ -n "$bsha" ] && [ "$bsha" = "$(field builder-sha256 "$rec")" ] && [ -f "$builder" ] && [ "$(sha256_of "$builder")" = "$bsha" ] \
        || refuse builder-not-stage3 "the builder $builder is not the stage 3 nazm its record names"
    b2=$(awk '$1 == "sha256" && $2 == "stage2" && $3 == "nazm" { print $4 }' "$brec")
    [ "$b2" = "$bsha" ] || refuse builder-not-a-fixpoint "the builder's stage 2 and stage 3 nazm differ in its record"
    builder_lines=$(sed 's/^/  /' "$brec")
fi

# --- what ships, and the checks of each image ---------------------------------------------------------
# PATH-IN-ARCHIVE  PROGRAM (programs.txt's name). The other five of the nine are built and compared by the
# self-build but are the harness's and the front end's own programs, not the released toolchain.
shipped="bin/nazm$exe nazm
bin/nazm-mcp$exe mcp
libexec/nazm/nazm-pipeline$exe pl
libexec/nazm/nzcg$exe cg"
limit_of() {
    LC_ALL=C grep -ao 'nazm: error\[N0406\]: out of memory: the heap limit of [0-9][0-9]* bytes is reached' "$1" 2> /dev/null \
        | sed 's/.*limit of \([0-9]*\) bytes.*/\1/' | sort -u | tr '\n' ' ' | sed 's/ $//'
}
check_image() { # FILE NAME
    f=$1 n=$2
    desc=$(file -b "$f")
    if [ $kind = macos ]; then
        [ "$desc" = "Mach-O 64-bit executable $file_arch" ] || refuse wrong-image "$n is '$desc', not a Mach-O 64-bit $file_arch executable"
        codesign --verify --strict "$f" 2> /dev/null || refuse unsigned "$n: its signature does not verify (codesign --verify --strict)"
        sigdesc=$(codesign -dv "$f" 2>&1 || true)
        printf '%s\n' "$sigdesc" | grep -q '^Signature=adhoc$' || refuse not-adhoc "$n is not ad hoc signed, as a release build is"
        # A release build is stripped and carries no debug map (S16.3): no SO/OSO/FUN stab is left
        # (ld's own `OPT radr://5614542` marker stays in every image, and says nothing of debug information).
        [ "$(nm -a "$f" 2> /dev/null | grep -cE ' - [0-9a-f]+ [0-9a-f]+ +(SO|OSO|SOL|FUN|BNSYM|ENSYM|GSYM|STSYM) ' || true)" = 0 ] \
            || refuse not-stripped "$n carries a debug map (stab entries); the release build strips it"
    else
        case $desc in "PE32+ executable (console) x86-64"*) ;; *) refuse wrong-image "$n is '$desc', not a PE32+ x86-64 console executable" ;; esac
    fi
    [ "$(limit_of "$f")" = 2147483648 ] || refuse no-counted-heap "$n does not carry the counted heap at exactly 2 GiB ('$(limit_of "$f")')"
}

version=$(sed -n 's/^fn version() -> Str { "\(.*\)" }$/\1/p' "$root/toolchain/commands/nazm.nz")
[ -n "$version" ] || refuse no-version "the version is not readable from toolchain/commands/nazm.nz"
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}

mkdir -p "$outdir"
outdir=$(CDPATH= cd -- "$outdir" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
top="nazm-$triple"
dir="$stage/$top"
mkdir -p "$dir/bin" "$dir/libexec/nazm" "$dir/docs/user"
lines=
while read -r path n; do
    src="$rec_dir/$from/$n$exe"
    check_image "$src" "$n"
    cp "$src" "$dir/$path"
    chmod 0755 "$dir/$path"
    lines="${lines}shipped $path $from $n$exe $(sha256_of "$dir/$path")
"
done <<< "$shipped"
cp "$root/README.md" "$root"/LICENSE* "$root/NOTICE" "$root/THIRD-PARTY-NOTICES.md" "$dir/"
cp "$root/docs/user/getting-started.md" "$root/docs/user/support.md" "$dir/docs/user/"

if [ $kind = macos ]; then
    install_text="Unpack anywhere and put bin/ on PATH: \`nazm\` finds its stages, libexec/nazm/, beside the bin/
that PATH names (or set NAZM_LIBEXEC to that directory). The standard library and the runtime are inside
bin/nazm. The binaries are ad hoc signed and not notarized: clear the quarantine once
(xattr -dr com.apple.quarantine nazm-$triple).

\`nazm run --interpret\`, \`nazm check\`, \`nazm fmt\` and the language service need nothing more.
\`nazm build\` writes native code through clang, which must be on PATH (the Xcode Command Line Tools:
xcode-select --install). Rust is never needed.

\`nazm build --backend nazm\` (EXPERIMENTAL) compiles with Nazm's own code generator,
libexec/nazm/nzcg, which must stay where it is beside bin/."
else
    install_text="Unpack anywhere (Windows' own tar.exe reads this archive: tar -xf nazm-$triple.tar.gz) and put
bin\\ on PATH: \`nazm.exe\` finds its stages, libexec\\nazm\\, beside the bin\\ that PATH names (or set
NAZM_LIBEXEC to that directory). The standard library and the runtime are inside bin\\nazm.exe.

\`nazm run --interpret\`, \`nazm check\`, \`nazm fmt\` and the language service need nothing more.
\`nazm build\` writes native code through clang and a MinGW-w64 linker, which must be on PATH
(llvm-mingw). Rust is never needed. TLS is refused on Windows (N0830) until its engine has run on a
Windows host.

These executables are not hardened and not signed (docs/user/support.md)."
fi
cat > "$dir/INSTALL.txt" <<TXT
nazm $version, prebuilt for $triple.

$install_text

How these binaries were built, and how to check that they are what the self-build made, is in
SELFBUILD.txt. What this host has shown is in docs/user/support.md.
TXT

{
    cat <<TXT
nazm $version for $triple: how the binaries in this archive were built (R8 slice 18, the cut-over)

Every binary here was built by Nazm. The chain starts at the seed, the Rust implementation of the
same commit (\`cargo build -p nazm-cli\`): the seed builds the builder (stage 0: \`nazm\`, its pipeline
and its code generator); stage 0 builds the nine programs of toolchain/selfbuild/programs.txt
(stage 1); stage 1 builds them again (stage 2), and stage 2 again (stage 3). Stages 1, 2 and 3 are
the same bytes: the toolchain built by itself is itself. The seed is not in this archive.
TXT
    if [ $kind = macos ]; then
        cat <<TXT
What ships is stage 3, in the release configuration (--release: canaries, PIE, stripped, ad hoc
signed; --heap-limit 2G; the LLVM backend), each file renamed but its bytes unchanged.
TXT
    else
        cat <<TXT
What ships was built by the macOS stage 3 below (itself a fixpoint) for $triple (--objects,
linked by MinGW-w64), twice, the two builds the same bytes. It is not a fixpoint on Windows: no
Windows-native self-build has been made. These images are not hardened and not signed.
TXT
    fi
    cat <<TXT

To check it: in a checkout of source-commit, \`sh scripts/source-digest.sh\` prints build-input-digest;
\`scripts/verify-selfbuild.sh ARCHIVE\` checks every shipped file against the lines below, and
\`scripts/verify-selfbuild.sh ARCHIVE OUT\` also against your own self-build OUT of that commit
(toolchain/selfbuild/host-selfbuild.sh, the same SDK and clang as the environment below: nothing
is claimed across machines or toolchains).

schema nazm.release-selfbuild/1
triple $triple
version $version
source-commit $commit
source-tree-clean $clean
build-input-digest $digest
seed-sha256 $(field seed-sha256 "$rec")
${lines}
the self-build's record (fixpoint.txt):
$(sed 's/^/  /' "$rec")
TXT
    if [ -n "$builder_lines" ]; then printf '\nthe builder'"'"'s record (the macOS self-build that made the builder):\n%s\n' "$builder_lines"; fi
    # The environment, without PATH: a machine's own directories are not the release's business.
    printf '\nthe environment (environment.txt, PATH left out):\n'
    grep -v '^PATH=' "$rec_dir/environment.txt" | sed 's/^/  /'
} > "$dir/SELFBUILD.txt"
find "$dir" -type f ! -path '*/bin/*' ! -path '*/libexec/*' -exec chmod 0644 {} +
find "$dir" -type d -exec chmod 0755 {} +

python3 -I "$root/scripts/reproducible-tar.py" "$stage" "$top" "$outdir/$top.tar.gz" "$SOURCE_DATE_EPOCH"
(cd "$outdir" && for a in nazm-*.tar.gz; do [ -e "$a" ] && shasum -a 256 "$a"; done) > "$outdir/SHA256SUMS.part"
mv "$outdir/SHA256SUMS.part" "$outdir/SHA256SUMS"
say "$outdir/$top.tar.gz ($(sha256_of "$outdir/$top.tar.gz"))"
printf '%s' "$lines" | sed 's/^/  /'
say "SHA256SUMS lists $(grep -c . "$outdir/SHA256SUMS") archive(s). Nothing was uploaded, tagged or signed."
