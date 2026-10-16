#!/bin/sh
# Package a prebuilt Nazm toolchain for one host (Gate 3, docs/systems-domains.md Part A).
#
#   scripts/host-archive.sh TRIPLE BINARY OUTDIR
#
# Writes OUTDIR/nazm-TRIPLE.tar.gz: bin/nazm (the standard library and runtime are inside it),
# on the two macOS targets libexec/nazm/nzcg (Nazm's own code generator, `--backend nazm`, built
# from toolchain/codegen/ by the BINARY being packaged: R9, D15), the licence, README, the
# getting-started and support documents, and INSTALL.txt saying what the host must provide — a C
# toolchain (clang) for native builds, never Rust. Local only: nothing is uploaded or published.
set -eu
[ $# -eq 3 ] || { printf 'usage: %s TRIPLE BINARY OUTDIR\n' "$0" >&2; exit 2; }
triple=$1 binary=$2 outdir=$3
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ -x "$binary" ] || { printf 'host-archive: %s is not an executable\n' "$binary" >&2; exit 2; }
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir="$stage/nazm-$triple"
mkdir -p "$dir/bin" "$dir/docs" "$outdir"
cp "$binary" "$dir/bin/nazm"
# The MCP server, where it was built beside it (0.9.0): THIRD-PARTY-NOTICES.md covers both.
mcp="$(dirname -- "$binary")/nazm-mcp"
[ -x "$mcp" ] && cp "$mcp" "$dir/bin/nazm-mcp"
# Nazm's own code generator, where `--backend nazm` builds before 1.0 (its D13: macOS), installed
# where the backend looks for it, `../libexec/nazm/nzcg` from `nazm`. Built by the packaged binary
# itself, without a cache and from the stage directory, so the source tree is never written.
nzcg_note=
case $triple in
    aarch64-apple-darwin|x86_64-apple-darwin)
        mkdir -p "$dir/libexec/nazm"
        (cd "$stage" && "$dir/bin/nazm" build --no-cache "$root/toolchain/codegen/main.nz" \
            -o "$dir/libexec/nazm/nzcg" >/dev/null) \
            || { printf 'host-archive: nzcg did not build\n' >&2; exit 1; }
        nzcg_note="
\`nazm build --backend nazm\` (EXPERIMENTAL) compiles with Nazm's own code generator,
libexec/nazm/nzcg, which must stay where it is beside bin/."
        ;;
esac
cp "$root/README.md" "$root"/LICENSE* "$root/NOTICE" "$root/THIRD-PARTY-NOTICES.md" "$dir/"
cp "$root/docs/getting-started.md" "$root/docs/support.md" "$dir/docs/"
version=$("$dir/bin/nazm" --version 2>/dev/null || echo unknown)
cat > "$dir/INSTALL.txt" <<TXT
$version, prebuilt for $triple.

Unpack anywhere and put bin/ on PATH. Nothing else is installed: the standard library and the
runtime are inside bin/nazm.

\`nazm run\`, \`nazm check\`, \`nazm fmt\` and the language service need nothing more. \`nazm build\`
writes native code through clang, which must be on PATH (Xcode's command-line tools on macOS, the
clang package on Linux). Rust is never needed.$nzcg_note

What this host has shown is in docs/support.md.
TXT
# The archive is the same bytes for the same files (R1.5 7.2, scripts/reproducible-check.sh):
# entries sorted by name, every time SOURCE_DATE_EPOCH (0 when unset), owner 0, permissions as
# staged, and a gzip header with no name and no time — which neither BSD nor GNU tar gives alone.
python3 - "$stage" "nazm-$triple" "$outdir/nazm-$triple.tar.gz" "${SOURCE_DATE_EPOCH:-0}" <<'PY'
import gzip, io, os, sys, tarfile
stage, top, dest, when = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
paths = []
for d, dirs, files in os.walk(os.path.join(stage, top)):
    dirs.sort()
    paths.append(d)
    paths.extend(os.path.join(d, f) for f in sorted(files))
raw = io.BytesIO()
with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as t:
    for p in paths:
        info = t.gettarinfo(p, os.path.relpath(p, stage))
        info.mtime, info.uid, info.gid, info.uname, info.gname = when, 0, 0, "", ""
        info.pax_headers = {}
        if info.isfile():
            with open(p, "rb") as f:
                t.addfile(info, f)
        else:
            t.addfile(info)
with open(dest, "wb") as out:
    with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0) as z:
        z.write(raw.getvalue())
PY
printf 'host-archive: %s/nazm-%s.tar.gz\n' "$outdir" "$triple"
