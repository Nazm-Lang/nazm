# Check that FILE is a hardened Linux release executable for MACHINE, reading the ELF headers itself,
# so that the release archives of the Linux targets are checked on the macOS host they are made on,
# which has no readelf (linux-toolchain L4, scripts/release-archive.sh). The same checks as
# toolchain/selfbuild/linux-selfbuild.sh's `elf_hardened`, which uses readelf inside the container:
#
#   - a 64-bit little-endian ELF of MACHINE (aarch64: EM_AARCH64; x86_64: EM_X86_64);
#   - a PIE: type ET_DYN, a PT_INTERP (an executable, not a library) and DF_1_PIE in DT_FLAGS_1;
#   - bound now (DT_BIND_NOW, DF_BIND_NOW in DT_FLAGS or DF_1_NOW in DT_FLAGS_1) with a PT_GNU_RELRO
#     segment: full RELRO;
#   - a PT_GNU_STACK segment that is not executable;
#   - stripped: no SHT_SYMTAB section and no .debug_ section.
#
#   python3 -I scripts/elf-image.py FILE aarch64|x86_64
#
# Prints `ok` and exits 0, or prints why not and exits 1. Nothing in FILE is run.
import struct, sys

MACHINES = {"aarch64": 183, "x86_64": 62}
PT_INTERP, PT_DYNAMIC, PT_GNU_STACK, PT_GNU_RELRO = 3, 2, 0x6474E551, 0x6474E552
DT_NULL, DT_BIND_NOW, DT_FLAGS, DT_FLAGS_1 = 0, 24, 30, 0x6FFFFFFB
DF_BIND_NOW, DF_1_NOW, DF_1_PIE = 0x8, 0x1, 0x08000000
SHT_SYMTAB = 2


def why_not(path, arch):
    with open(path, "rb") as f:
        b = f.read()
    if b[:4] != b"\x7fELF":
        return "not an ELF file"
    if b[4] != 2:
        return "not ELF64"
    if b[5] != 1:
        return "not little-endian"
    e_type, e_machine = struct.unpack_from("<HH", b, 16)
    if e_machine != MACHINES[arch]:
        return f"not {arch} (e_machine {e_machine})"
    if e_type != 3:
        return f"not a PIE (e_type {e_type}, not ET_DYN)"
    e_phoff, e_shoff = struct.unpack_from("<QQ", b, 32)
    e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHHHH", b, 54)
    segs = []
    for i in range(e_phnum):
        p_type, p_flags, p_offset, _vaddr, _paddr, p_filesz = struct.unpack_from("<IIQQQQ", b, e_phoff + i * e_phentsize)
        segs.append((p_type, p_flags, p_offset, p_filesz))
    types = [s[0] for s in segs]
    if PT_INTERP not in types:
        return "no PT_INTERP: a shared library, not an executable"
    dyn = [s for s in segs if s[0] == PT_DYNAMIC]
    if not dyn:
        return "no PT_DYNAMIC segment"
    tags = {}
    _, _, off, size = dyn[0]
    for i in range(size // 16):
        tag, val = struct.unpack_from("<qQ", b, off + i * 16)
        if tag == DT_NULL:
            break
        tags[tag] = val
    if not tags.get(DT_FLAGS_1, 0) & DF_1_PIE:
        return "not a PIE (DT_FLAGS_1 has no PIE)"
    if not (DT_BIND_NOW in tags or tags.get(DT_FLAGS, 0) & DF_BIND_NOW or tags.get(DT_FLAGS_1, 0) & DF_1_NOW):
        return "not bound now (no BIND_NOW)"
    if PT_GNU_RELRO not in types:
        return "no GNU_RELRO segment"
    stack = [s for s in segs if s[0] == PT_GNU_STACK]
    if not stack:
        return "no GNU_STACK segment"
    if stack[0][1] & 1:
        return "the stack is executable (GNU_STACK has PF_X)"
    if e_shnum == 0 or e_shstrndx >= e_shnum:
        return "no section headers to show it stripped"
    secs = [struct.unpack_from("<IIQQQQ", b, e_shoff + i * e_shentsize) for i in range(e_shnum)]
    str_off, str_size = secs[e_shstrndx][4], secs[e_shstrndx][5]
    names = b[str_off : str_off + str_size]
    for name_off, sh_type, *_ in secs:
        name = names[name_off : names.index(b"\0", name_off)].decode("latin-1")
        if sh_type == SHT_SYMTAB:
            return f"not stripped (a symbol table, {name})"
        if name.startswith(".debug_"):
            return f"not stripped (debug information, {name})"
    return ""


if len(sys.argv) != 3 or sys.argv[2] not in MACHINES:
    sys.exit("usage: python3 -I scripts/elf-image.py FILE aarch64|x86_64")
try:
    why = why_not(sys.argv[1], sys.argv[2])
except (OSError, struct.error, ValueError) as e:
    why = f"unreadable as ELF ({e})"
print(why or "ok")
sys.exit(1 if why else 0)
