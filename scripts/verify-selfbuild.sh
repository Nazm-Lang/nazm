#!/bin/bash
# Check that a release archive's binaries are what its self-build made (R8 slice 18, S18.4), or that
# a self-build's record directory is whole (linux-toolchain L2).
#
#   scripts/verify-selfbuild.sh ARCHIVE [OUT]
#   scripts/verify-selfbuild.sh --record OUT
#
# Unpacks ARCHIVE (nazm-TRIPLE.tar.gz, written by scripts/release-archive.sh) into a fresh directory
# and checks, running nothing it unpacked:
#
#   1. SELFBUILD.txt is a nazm.release-selfbuild/1 record whose embedded self-build record says
#      `result passed`, and every executable under bin/ and libexec/ is one of its `shipped` lines,
#      with that line's SHA-256;
#   2. each shipped file is, by that embedded record, the last stage and equal to the stage before it:
#      stage 3 = stage 2 (macOS and Linux: the fixpoint), or the first Windows build = the second;
#   3. with OUT, a self-build the reader ran (toolchain/selfbuild/host-selfbuild.sh,
#      linux-selfbuild.sh or windows-selfbuild.sh) of the same commit, that OUT's record passed for
#      the same commit and triple, and each shipped file is the same bytes as OUT's own
#      (stage3/NAME or win/NAME.exe).
#
# With `--record OUT` there is no archive: OUT is a self-build's record directory (as the
# `linux-selfbuild` contained workload exports it, unpacked), and the checks are that OUT/fixpoint.txt
# is a nazm.selfbuild-record/1 ending `result passed`; that environment.txt and every file it names
# have the digests it gives; and that each program's last stage is its stage before (stage3 =
# stage2 = stage1 for host-selfbuild.sh and linux-selfbuild.sh, win = win2 for windows-selfbuild.sh,
# x86 = x86b for linux-x86-selfbuild.sh, whose record must also name its builder's record and the
# emulation it ran under), by the record and byte for byte. When this checkout is at the record's commit, its build-input
# digest must be the record's too. Nothing in OUT is run.
#
# Exit 0 and `verify-selfbuild: ok TRIPLE` when every check holds; 1 naming the first that does not.
# scripts/sign-release.sh runs it on every archive before anything is signed.
set -euo pipefail
[ $# -ge 1 ] && [ $# -le 2 ] || { printf 'usage: %s ARCHIVE [OUT] | --record OUT\n' "$0" >&2; exit 2; }
archive=$1 out=${2:-}
bad() { printf 'verify-selfbuild: NOT VERIFIED: %s\n' "$*" >&2; exit 1; }
if command -v shasum > /dev/null 2>&1; then sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else sha256_of() { sha256sum "$1" | cut -d' ' -f1; }; fi

if [ "$archive" = --record ]; then
    [ -n "$out" ] && [ -d "$out" ] || bad "--record needs a self-build's record directory"
    rec="$out/fixpoint.txt"
    [ -f "$rec" ] || bad "$rec is missing: only a self-build that passed writes it"
    field() { sed -n "s/^$1 //p" "$rec" | head -1; }
    [ "$(head -1 "$rec")" = "schema nazm.selfbuild-record/1" ] || bad "$rec is not a nazm.selfbuild-record/1"
    [ "$(tail -1 "$rec")" = "result passed" ] || bad "$rec does not end with 'result passed'"
    script=$(field script)
    case $script in
        host-selfbuild.sh | linux-selfbuild.sh) last=stage3 prev=stage2 first=stage1 ;;
        windows-selfbuild.sh) last=win prev=win2 first=win2 ;;
        # x86_64 Linux, cross-built by an aarch64 stage 3 and run under emulation (linux-toolchain L3):
        # the two builds are the pair; the record must name its builder's record and the emulation.
        linux-x86-selfbuild.sh)
            last=x86 prev=x86b first=x86b
            [ "$(field target)" = x86_64-unknown-linux-gnu ] || bad "$rec is a linux-x86-selfbuild.sh record for '$(field target)'"
            printf '%s\n' "$(field builder-record-sha256)" | grep -Eqx '[0-9a-f]{64}' || bad "$rec does not name its builder's record by digest"
            [ -n "$(field emulation)" ] || bad "$rec does not say whether the x86_64 programs ran emulated"
            ;;
        *) bad "$rec was written by '$script', which is no self-build this script knows" ;;
    esac
    [ "$(field environment-sha256)" = "$(sha256_of "$out/environment.txt")" ] || bad "$out/environment.txt is not the file the record names"
    files=0
    while read -r _ where name want; do
        [ -f "$out/$where/$name" ] || bad "$rec names $where/$name, which is missing"
        [ "$(sha256_of "$out/$where/$name")" = "$want" ] || bad "$where/$name is not the file the record names"
        files=$((files + 1))
    done < <(grep '^sha256 ' "$rec")
    recsha() { awk -v w="$1" -v n="$2" '$1 == "sha256" && $2 == w && $3 == n { print $4 }' "$rec"; }
    programs=0
    for name in $(awk -v w="$last" '$1 == "sha256" && $2 == w { print $3 }' "$rec"); do
        for s in "$prev" "$first"; do
            [ -n "$(recsha "$s" "$name")" ] || bad "$rec has no $s $name"
            [ "$(recsha "$s" "$name")" = "$(recsha "$last" "$name")" ] || bad "$name: $s and $last differ in the record (not a fixpoint)"
            cmp -s "$out/$s/$name" "$out/$last/$name" || bad "$name: $s and $last are not the same bytes"
        done
        programs=$((programs + 1))
    done
    [ "$programs" -gt 0 ] || bad "$rec names no $last program"
    commit=$(field source-commit)
    root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
    here=$(git -C "$root" rev-parse HEAD 2> /dev/null || echo unknown)
    if [ "$here" = "$commit" ]; then
        [ "$(sh "$root/scripts/source-digest.sh" --root "$root")" = "$(field build-input-digest)" ] \
            || bad "this checkout is at $commit, but its build-input digest is not the record's (the tree has changed)"
        tree="this checkout's tree, digest $(field build-input-digest)"
    else
        tree="not this checkout's commit ($here): its build-input digest was not compared"
    fi
    printf 'verify-selfbuild: ok %s record (%s, commit %s, %s): %s files have the digests the record gives; %s programs, %s = %s = %s byte for byte\n' \
        "$(field target)" "$script" "$commit" "$tree" "$files" "$programs" "$first" "$prev" "$last"
    exit 0
fi
[ -f "$archive" ] || bad "$archive is not a file"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$archive" || bad "$archive does not unpack"
set -- "$work"/nazm-*/
[ $# -eq 1 ] && [ -d "$1" ] || bad "$archive does not hold exactly one nazm-TRIPLE directory"
top=${1%/}
triple=${top##*/nazm-}
sb="$top/SELFBUILD.txt"
[ -f "$sb" ] || bad "$archive carries no SELFBUILD.txt: it is not a Nazm-built release archive (the seed's archives are never released)"
grep -qx 'schema nazm.release-selfbuild/1' "$sb" || bad "SELFBUILD.txt is not a nazm.release-selfbuild/1 record"
[ "$(sed -n 's/^triple //p' "$sb")" = "$triple" ] || bad "SELFBUILD.txt names another triple than the archive's directory ($triple)"
grep -qx '  result passed' "$sb" || bad "the embedded self-build record does not say 'result passed'"
commit=$(sed -n 's/^source-commit //p' "$sb")
# The embedded record: the lines of fixpoint.txt, indented, up to its `result` line.
embedded=$(awk '/^the self-build.s record \(fixpoint.txt\):$/ { on = 1; next } on && /^  / { sub(/^  /, ""); print; if ($0 == "result passed") exit; next } on { exit }' "$sb")
[ -n "$embedded" ] || bad "SELFBUILD.txt carries no self-build record"
[ "$(printf '%s\n' "$embedded" | sed -n 's/^source-commit //p')" = "$commit" ] || bad "the embedded record names another commit than SELFBUILD.txt"
recsha() { printf '%s\n' "$embedded" | awk -v w="$1" -v n="$2" '$1 == "sha256" && $2 == w && $3 == n { print $4 }'; }

listed=0
while read -r _ path from name want; do
    f="$top/$path"
    [ -f "$f" ] || bad "SELFBUILD.txt ships $path, which the archive does not hold"
    got=$(sha256_of "$f")
    [ "$got" = "$want" ] || bad "$path is not the file SELFBUILD.txt names ($got, not $want)"
    case $from in
        stage3) prev=stage2 ;;
        win) prev=win2 ;;
        *) bad "$path ships from '$from', which is neither the last macOS stage nor the Windows build" ;;
    esac
    [ "$(recsha "$from" "$name")" = "$want" ] || bad "$path is not $from/$name of the embedded record"
    [ "$(recsha "$prev" "$name")" = "$want" ] || bad "$path: $prev/$name of the embedded record is not the same bytes ($from is not a fixpoint)"
    if [ -n "$out" ]; then
        [ -f "$out/fixpoint.txt" ] && [ "$(tail -1 "$out/fixpoint.txt")" = "result passed" ] || bad "$out has no passed self-build record"
        [ "$(sed -n 's/^source-commit //p' "$out/fixpoint.txt")" = "$commit" ] || bad "$out was built from another commit than $commit"
        [ "$(sed -n 's/^target //p' "$out/fixpoint.txt")" = "$triple" ] || bad "$out is a self-build for another target than $triple"
        cmp -s "$f" "$out/$from/$name" || bad "$path is not the same bytes as $out/$from/$name: the archive is not what this self-build makes"
    fi
    listed=$((listed + 1))
done < <(grep '^shipped ' "$sb")
[ "$listed" -gt 0 ] || bad "SELFBUILD.txt ships nothing"
present=$(find "$top/bin" "$top/libexec" -type f 2> /dev/null | wc -l | tr -d ' ')
[ "$present" -eq "$listed" ] || bad "the archive holds $present files under bin/ and libexec/, SELFBUILD.txt ships $listed"
if [ -n "$out" ]; then
    printf 'verify-selfbuild: ok %s (commit %s): %s shipped files, each the self-build'"'"'s last stage, equal to the stage before it, and the same bytes as %s\n' "$triple" "$commit" "$listed" "$out"
else
    printf 'verify-selfbuild: ok %s (commit %s): %s shipped files, each the self-build'"'"'s last stage and equal to the stage before it, by the record\n' "$triple" "$commit" "$listed"
fi
