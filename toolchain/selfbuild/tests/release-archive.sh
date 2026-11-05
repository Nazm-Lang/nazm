#!/bin/bash
# scripts/release-archive.sh and scripts/verify-selfbuild.sh on a synthetic self-build record (R8 slice 18,
# S18.5). Runs no Nazm-built program: the "stages" are nine copies of one tiny C stub that carries the
# counted-heap message, so the release path's checks (record, digests, fixpoint, image format, ad hoc
# signature, no debug map, counted heap, reproducible archive, verification) are exercised without the host
# rule being touched. aarch64 macOS only (the stub is compiled here); prints `ok` or fails.
set -u
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../.." && pwd)
[ "$(uname -s)-$(uname -m)" = Darwin-arm64 ] || { echo "skipped: needs an arm64 Mac"; exit 0; }
work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }

cat > "$work/stub.c" <<'C'
const char *m = "nazm: error[N0406]: out of memory: the heap limit of 2147483648 bytes is reached";
int main(void) { return m[0] == 0; }
C
clang -O0 -o "$work/stub" "$work/stub.c" 2> /dev/null || fail "clang"
codesign -f -s - "$work/stub" 2> /dev/null || fail "codesign"

mkrec() { # DIR: a passed aarch64 record of this tree, stages 1-3 identical
    d=$1; mkdir -p "$d"
    for s in stage1 stage2 stage3; do mkdir -p "$d/$s"; done
    for n in $(grep -v '^[[:space:]]*#' "$root/toolchain/selfbuild/programs.txt" | awk 'NF { print $2 }'); do
        for s in stage1 stage2 stage3; do cp "$work/stub" "$d/$s/$n"; done
    done
    echo "script: host-selfbuild.sh aarch64 (synthetic)" > "$d/environment.txt"
    {
        echo "schema nazm.selfbuild-record/1"; echo "script host-selfbuild.sh"; echo "arch aarch64"
        echo "target aarch64-apple-darwin"; echo "backend llvm"; echo "heap-limit 2G"; echo "stages 3"
        echo "source-commit $(git -C "$root" rev-parse HEAD)"
        if [ -z "$(git -C "$root" status --porcelain)" ]; then echo "source-tree-clean yes"; else echo "source-tree-clean no"; fi
        echo "build-input-digest $(sh "$root/scripts/source-digest.sh" --root "$root")"
        echo "seed-sha256 $(printf seed | shasum -a 256 | cut -d' ' -f1)"
        echo "environment-sha256 $(sha "$d/environment.txt")"
        for f in "$d"/stage?/*; do echo "sha256 $(basename "$(dirname "$f")") $(basename "$f") $(sha "$f")"; done
        echo "summary synthetic"; echo "result passed"
    } > "$d/fixpoint.txt"
}
T=aarch64-apple-darwin
RA="$root/scripts/release-archive.sh --allow-dirty"
expect_refusal() { # WHY CMD... : the command must exit 2 naming [WHY]
    why=$1; shift
    err=$("$@" 2>&1 > /dev/null); rc=$?
    [ $rc -eq 2 ] && printf '%s' "$err" | grep -q "refused \[$why\]" || fail "expected refusal [$why], got rc=$rc: $err"
}

mkrec "$work/ok"
export SOURCE_DATE_EPOCH=1
bash $RA $T "$work/ok" "$work/out1" > /dev/null || fail "a good record was refused"
bash $RA $T "$work/ok" "$work/out2" > /dev/null || fail "second archive"
cmp -s "$work/out1/nazm-$T.tar.gz" "$work/out2/nazm-$T.tar.gz" || fail "the archive is not reproducible"
grep -q "nazm-$T.tar.gz" "$work/out1/SHA256SUMS" || fail "SHA256SUMS does not list the archive"
(cd "$work/out1" && shasum -a 256 -c SHA256SUMS > /dev/null) || fail "SHA256SUMS does not verify"
tar -tzf "$work/out1/nazm-$T.tar.gz" | grep -q "bin/nazm-mcp$" || fail "nazm-mcp is not shipped"
tar -tzf "$work/out1/nazm-$T.tar.gz" | grep -qE "libexec/nazm/(nazm-pipeline|nzcg)$" || fail "stages not shipped"
tar -tzf "$work/out1/nazm-$T.tar.gz" | grep -q "SELFBUILD.txt$" || fail "no SELFBUILD.txt"
if tar -tzf "$work/out1/nazm-$T.tar.gz" | grep -qE "/(fe|check|interp|jtool|mutate)$"; then fail "a harness program is shipped"; fi
bash "$root/scripts/verify-selfbuild.sh" "$work/out1/nazm-$T.tar.gz" > /dev/null || fail "verify-selfbuild refused a good archive"
bash "$root/scripts/verify-selfbuild.sh" "$work/out1/nazm-$T.tar.gz" "$work/ok" > /dev/null || fail "verify-selfbuild against OUT refused a good archive"

# A tampered archive is not verified.
mkdir "$work/un" && tar -C "$work/un" -xzf "$work/out1/nazm-$T.tar.gz"
printf 'x' >> "$work/un/nazm-$T/bin/nazm"
python3 -I "$root/scripts/reproducible-tar.py" "$work/un" "nazm-$T" "$work/bad.tar.gz" 1
bash "$root/scripts/verify-selfbuild.sh" "$work/bad.tar.gz" > /dev/null 2>&1 && fail "verify-selfbuild accepted a changed binary"

# Refusals.
expect_refusal not-a-release-target bash $RA x86_64-unknown-linux-gnu "$work/ok" "$work/o"
expect_refusal no-record bash $RA $T "$work/none" "$work/o"
mkrec "$work/b1"; printf 'x' >> "$work/b1/stage3/nazm"
expect_refusal tampered bash $RA $T "$work/b1" "$work/o"
mkrec "$work/b2"; printf 'x' >> "$work/b2/stage3/nazm"
sed -i '' "s/^sha256 stage3 nazm .*/sha256 stage3 nazm $(sha "$work/b2/stage3/nazm")/" "$work/b2/fixpoint.txt"
expect_refusal not-a-fixpoint bash $RA $T "$work/b2" "$work/o"
mkrec "$work/b3"; sed -i '' 's/^source-commit .*/source-commit 0000000/' "$work/b3/fixpoint.txt"
expect_refusal wrong-commit bash $RA $T "$work/b3" "$work/o"
mkrec "$work/b4"; sed -i '' 's/^heap-limit 2G/heap-limit 1G/' "$work/b4/fixpoint.txt"
expect_refusal wrong-heap-limit bash $RA $T "$work/b4" "$work/o"
mkrec "$work/b5"; sed -i '' '$d' "$work/b5/fixpoint.txt"
expect_refusal not-passed bash $RA $T "$work/b5" "$work/o"
mkrec "$work/b6"; sed -i '' 's/^target .*/target x86_64-apple-darwin/' "$work/b6/fixpoint.txt"
expect_refusal wrong-target bash $RA $T "$work/b6" "$work/o"
# A dirty tree is refused without --allow-dirty (this tree may be clean: then the refusal is not reachable, say so).
if [ -n "$(git -C "$root" status --porcelain)" ]; then expect_refusal dirty bash "$root/scripts/release-archive.sh" $T "$work/ok" "$work/o"; fi
# An image with a debug map is refused.
mkrec "$work/b7"; clang -g -O0 -o "$work/stubg" "$work/stub.c" 2> /dev/null && codesign -f -s - "$work/stubg" 2> /dev/null
cp "$work/stubg" "$work/b7/stage3/nazm"; cp "$work/stubg" "$work/b7/stage2/nazm"; cp "$work/stubg" "$work/b7/stage1/nazm"
sed -i '' "s/^\(sha256 stage[123] nazm \).*/\1$(sha "$work/stubg")/" "$work/b7/fixpoint.txt"
expect_refusal not-stripped bash $RA $T "$work/b7" "$work/o"
echo ok
