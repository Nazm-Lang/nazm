# Write STAGE/TOP as the gzip-compressed tar DEST, the same bytes for the same files (R1.5 7.2):
# entries sorted by name, every time WHEN (SOURCE_DATE_EPOCH, 0 when unset), owner 0 with no names,
# permissions as staged, no PAX records, and a gzip header with no name and no time - which neither
# BSD nor GNU tar gives alone. Used by scripts/host-archive.sh (the seed's archive) and
# scripts/release-archive.sh (the release archive, R8 slice 18).
#
#   python3 -I scripts/reproducible-tar.py STAGE TOP DEST WHEN
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
