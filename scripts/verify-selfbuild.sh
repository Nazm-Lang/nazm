#!/bin/bash
# Check that a release archive's binaries are what its self-build made (R8 slice 18, S18.4).
#
#   scripts/verify-selfbuild.sh ARCHIVE [OUT]
#
# Unpacks ARCHIVE (nazm-TRIPLE.tar.gz, written by scripts/release-archive.sh) into a fresh directory
# and checks, running nothing it unpacked:
#
#   1. SELFBUILD.txt is a nazm.release-selfbuild/1 record whose embedded self-build record says
#      `result passed`, and every executable under bin/ and libexec/ is one of its `shipped` lines,
#      with that line's SHA-256;
#   2. each shipped file is, by that embedded record, the last stage and equal to the stage before it:
#      stage 3 = stage 2 (macOS: the fixpoint), or the first Windows build = the second;
#   3. with OUT, a self-build the reader ran (toolchain/selfbuild/host-selfbuild.sh or
#      windows-selfbuild.sh) of the same commit, that OUT's record passed for the same commit and
#      triple, and each shipped file is the same bytes as OUT's own (stage3/NAME or win/NAME.exe).
#
# Exit 0 and `verify-selfbuild: ok TRIPLE` when every check holds; 1 naming the first that does not.
# scripts/sign-release.sh runs it on every archive before anything is signed.
set -euo pipefail
[ $# -ge 1 ] && [ $# -le 2 ] || { printf 'usage: %s ARCHIVE [OUT]\n' "$0" >&2; exit 2; }
archive=$1 out=${2:-}
bad() { printf 'verify-selfbuild: NOT VERIFIED: %s\n' "$*" >&2; exit 1; }
if command -v shasum > /dev/null 2>&1; then sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else sha256_of() { sha256sum "$1" | cut -d' ' -f1; }; fi
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
