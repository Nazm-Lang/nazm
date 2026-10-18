//! Static archives, written by nazm itself (N55, reproducible builds).
//!
//! The system's `ar` used to write them. Apple's Command Line Tools 27.0 write the symbol index
//! (`__.SYMDEF SORTED`) of `ar rcs` and `libtool -static` in a varying order, so the same objects
//! archived twice gave different bytes; without the index Apple's linker refuses the members. So
//! the archive is written here, from the objects' own symbol tables, and is a function of the
//! objects' bytes and names alone: no date, owner or mode of the machine, no tool's version.
//!
//! Two formats, chosen by the members:
//!
//! - **Mach-O** members: the BSD format Apple's `ld64` reads — every name `#1/N` with the name
//!   after the header, padded so each member's bytes start 8-byte aligned, and a first member
//!   `__.SYMDEF SORTED` whose index is sorted by symbol name, ties by member offset.
//! - **ELF, COFF and WebAssembly** members: the GNU/System V format GNU `ld`, MinGW's `ld` and
//!   LLVM's linkers read — a first member `/` indexing the symbols in member order, a `//` table
//!   of the names longer than 15 bytes.
//!
//! An index holds each member's defined global symbols, read from its object's symbol table
//! (Mach-O `nlist`, ELF `.symtab`, COFF's symbol table, WebAssembly's `linking` section).

use std::path::{Path, PathBuf};

/// The archive's magic.
const MAGIC: &[u8] = b"!<arch>\n";

/// The BSD index's name.
const SYMDEF: &str = "__.SYMDEF SORTED";

/// One member: its name in the archive, its bytes, its defined global symbols.
struct Member {
    name: String,
    data: Vec<u8>,
    symbols: Vec<Vec<u8>>,
}

/// Write `objects`, in order, as the static archive `output`.
pub(crate) fn write(output: &Path, objects: &[PathBuf]) -> Result<(), String> {
    let mut members = Vec::with_capacity(objects.len());
    for path in objects {
        let data =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        members.push((name, data));
    }
    let bytes = archive(members)?;
    std::fs::write(output, bytes).map_err(|e| format!("cannot write {}: {e}", output.display()))
}

/// The archive of `members` (name, bytes), in order.
pub(crate) fn archive(members: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let mut read = Vec::with_capacity(members.len());
    let mut mach_o = false;
    for (name, mut data) in members {
        mach_o |= Format::of(&data) == Some(Format::MachO);
        zero_coff_time(&mut data);
        let symbols = defined_globals(&data)
            .map_err(|e| format!("cannot read the symbols of `{name}`: {e}"))?;
        read.push(Member {
            name,
            data,
            symbols,
        });
    }
    if mach_o {
        bsd(&read)
    } else {
        gnu(&read)
    }
}

/// A COFF object's `TimeDateStamp` zeroed: LLVM 23's clang writes the build's time there for a
/// Windows target, so the same source archived twice differed (CI, macOS, LLVM 23). The stamp is
/// at byte 4 of a COFF header and at byte 8 of a big-object's; nothing reads it from a member.
fn zero_coff_time(data: &mut [u8]) {
    let at = match Format::of(data) {
        Some(Format::Coff) => 4,
        Some(Format::BigObj) => 8,
        _ => return,
    };
    if let Some(stamp) = data.get_mut(at..at + 4) {
        stamp.fill(0);
    }
}

/// A header field, left-aligned and padded with spaces to `width`.
fn field(out: &mut Vec<u8>, value: &str, width: usize) -> Result<(), String> {
    if value.len() > width {
        return Err(format!("`{value}` does not fit an archive header"));
    }
    out.extend_from_slice(value.as_bytes());
    out.resize(out.len() + width - value.len(), b' ');
    Ok(())
}

/// A member's 60-byte header.
fn header(out: &mut Vec<u8>, name: &str, mode: &str, size: usize) -> Result<(), String> {
    let (date, owner) = if mode.is_empty() {
        ("", "")
    } else {
        ("0", "0")
    };
    field(out, name, 16)?;
    field(out, date, 12)?;
    field(out, owner, 6)?;
    field(out, owner, 6)?;
    field(out, mode, 8)?;
    field(out, &size.to_string(), 10)?;
    out.extend_from_slice(b"`\n");
    Ok(())
}

fn round_up(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

fn u32_of(n: usize) -> Result<u32, String> {
    u32::try_from(n).map_err(|_| "the archive is larger than 4 GiB".to_string())
}

/// The BSD format, as Apple's `ld64` reads it.
fn bsd(members: &[Member]) -> Result<Vec<u8>, String> {
    // A `#1/N` name: N is the name padded with NULs so the member's bytes, after its 60-byte
    // header, start 8-byte aligned (as Apple's tools pad it).
    let long = |name: &str| round_up(name.len(), 8) + 4;
    let padded = |m: &Member| round_up(m.data.len(), 8);
    let count: usize = members.iter().map(|m| m.symbols.len()).sum();
    let strings: usize = members
        .iter()
        .flat_map(|m| &m.symbols)
        .map(|s| s.len() + 1)
        .sum();
    let strtab = round_up(strings, 8);
    let index = 4 + 8 * count + 4 + strtab;
    // Each member's offset, from the start of the archive.
    let mut offsets = Vec::with_capacity(members.len());
    let mut at = MAGIC.len() + 60 + long(SYMDEF) + index;
    for m in members {
        offsets.push(at);
        at += 60 + long(&m.name) + padded(m);
    }
    // Sorted by name, ties by member offset: a total order of the archive's own contents.
    let mut entries: Vec<(&[u8], usize)> = members
        .iter()
        .zip(&offsets)
        .flat_map(|(m, &off)| m.symbols.iter().map(move |s| (s.as_slice(), off)))
        .collect();
    entries.sort_unstable();
    let mut out = Vec::with_capacity(at);
    out.extend_from_slice(MAGIC);
    header(&mut out, "#1/20", "100644", long(SYMDEF) + index)?;
    named(&mut out, SYMDEF, long(SYMDEF));
    out.extend_from_slice(&u32_of(8 * count)?.to_le_bytes());
    let mut strx = 0;
    for (name, off) in &entries {
        out.extend_from_slice(&u32_of(strx)?.to_le_bytes());
        out.extend_from_slice(&u32_of(*off)?.to_le_bytes());
        strx += name.len() + 1;
    }
    out.extend_from_slice(&u32_of(strtab)?.to_le_bytes());
    let start = out.len();
    for (name, _) in &entries {
        out.extend_from_slice(name);
        out.push(0);
    }
    out.resize(start + strtab, 0);
    for m in members {
        let n = long(&m.name);
        header(&mut out, &format!("#1/{n}"), "644", n + padded(m))?;
        named(&mut out, &m.name, n);
        out.extend_from_slice(&m.data);
        out.resize(out.len() + padded(m) - m.data.len(), 0);
    }
    Ok(out)
}

/// A `#1/N` member's name, padded with NULs to `n` bytes.
fn named(out: &mut Vec<u8>, name: &str, n: usize) {
    out.extend_from_slice(name.as_bytes());
    out.resize(out.len() + n - name.len(), 0);
}

/// The GNU/System V format, as GNU `ld`, MinGW's `ld` and LLVM's linkers read it.
fn gnu(members: &[Member]) -> Result<Vec<u8>, String> {
    let even = |n: usize| round_up(n, 2);
    // The names a header cannot hold, in a `//` table; the others end in `/`.
    let mut table = Vec::new();
    let mut names = Vec::with_capacity(members.len());
    for m in members {
        if m.name.len() <= 15 && !m.name.contains('/') {
            names.push(format!("{}/", m.name));
        } else {
            names.push(format!("/{}", table.len()));
            table.extend_from_slice(m.name.as_bytes());
            table.extend_from_slice(b"/\n");
        }
    }
    let count: usize = members.iter().map(|m| m.symbols.len()).sum();
    let strings: usize = members
        .iter()
        .flat_map(|m| &m.symbols)
        .map(|s| s.len() + 1)
        .sum();
    let index = 4 + 4 * count + strings;
    let mut at = MAGIC.len() + 60 + even(index);
    if !table.is_empty() {
        at += 60 + even(table.len());
    }
    let mut offsets = Vec::with_capacity(members.len());
    for m in members {
        offsets.push(at);
        at += 60 + even(m.data.len());
    }
    let mut out = Vec::with_capacity(at);
    out.extend_from_slice(MAGIC);
    header(&mut out, "/", "0", index)?;
    out.extend_from_slice(&u32_of(count)?.to_be_bytes());
    for (m, &off) in members.iter().zip(&offsets) {
        for _ in &m.symbols {
            out.extend_from_slice(&u32_of(off)?.to_be_bytes());
        }
    }
    for s in members.iter().flat_map(|m| &m.symbols) {
        out.extend_from_slice(s);
        out.push(0);
    }
    pad_even(&mut out);
    if !table.is_empty() {
        header(&mut out, "//", "", table.len())?;
        out.extend_from_slice(&table);
        pad_even(&mut out);
    }
    for (m, name) in members.iter().zip(&names) {
        header(&mut out, name, "644", m.data.len())?;
        out.extend_from_slice(&m.data);
        pad_even(&mut out);
    }
    Ok(out)
}

fn pad_even(out: &mut Vec<u8>) {
    if out.len() % 2 == 1 {
        out.push(b'\n');
    }
}

/// An object's format, by its magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    MachO,
    Elf,
    Coff,
    BigObj,
    Wasm,
}

impl Format {
    fn of(data: &[u8]) -> Option<Self> {
        let magic: [u8; 4] = data.get(..4)?.try_into().ok()?;
        match magic {
            [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] => Some(Self::MachO),
            [0x7f, b'E', b'L', b'F'] => Some(Self::Elf),
            [0, b'a', b's', b'm'] => Some(Self::Wasm),
            [0, 0, 0xff, 0xff] => Some(Self::BigObj),
            // x86-64, AArch64, ARM64EC, i386, ARMv7 (Thumb).
            [0x64, 0x86, ..]
            | [0x64, 0xaa, ..]
            | [0x41, 0xa6, ..]
            | [0x4c, 0x01, ..]
            | [0xc4, 0x01, ..] => Some(Self::Coff),
            _ => None,
        }
    }
}

/// An object's bytes, read with bounds checked and in its byte order.
struct Bytes<'a> {
    data: &'a [u8],
    little: bool,
}

impl Bytes<'_> {
    fn take<const N: usize>(&self, at: usize) -> Result<[u8; N], String> {
        at.checked_add(N)
            .and_then(|end| self.data.get(at..end))
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| format!("truncated at byte {at}"))
    }
    fn u8(&self, at: usize) -> Result<u8, String> {
        Ok(self.take::<1>(at)?[0])
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        let b = self.take(at)?;
        Ok(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        let b = self.take(at)?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn u64(&self, at: usize) -> Result<u64, String> {
        let b = self.take(at)?;
        Ok(if self.little {
            u64::from_le_bytes(b)
        } else {
            u64::from_be_bytes(b)
        })
    }
    /// A 32-bit offset or count, as an index.
    fn at32(&self, at: usize) -> Result<usize, String> {
        usize::try_from(self.u32(at)?).map_err(|e| e.to_string())
    }
    fn at64(&self, at: usize) -> Result<usize, String> {
        usize::try_from(self.u64(at)?).map_err(|e| e.to_string())
    }
    /// The NUL-terminated string at `at`.
    fn string(&self, at: usize) -> Result<Vec<u8>, String> {
        let rest = self
            .data
            .get(at..)
            .ok_or_else(|| format!("a name outside the object at byte {at}"))?;
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| format!("an unterminated name at byte {at}"))?;
        Ok(rest.get(..end).unwrap_or_default().to_vec())
    }
}

/// The global symbols `data` defines, in its symbol table's order.
fn defined_globals(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    match Format::of(data) {
        Some(Format::MachO) => mach_o(data),
        Some(Format::Elf) => elf(data),
        Some(Format::Coff) => coff(data, false),
        Some(Format::BigObj) => coff(data, true),
        Some(Format::Wasm) => wasm(data),
        None => Err("not an object file nazm can index (Mach-O, ELF, COFF or WebAssembly)".into()),
    }
}

/// Mach-O: each `nlist` that is external, not a debugging entry, and not undefined — as Apple's
/// `ranlib` indexes (a common symbol is left out, as it does by default).
fn mach_o(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    const LC_SYMTAB: u32 = 0x2;
    const N_STAB: u8 = 0xe0;
    const N_TYPE: u8 = 0x0e;
    const N_EXT: u8 = 0x01;
    let b = Bytes { data, little: true };
    let wide = b.u32(0)? == 0xfeed_facf;
    let (header, entry) = if wide { (32, 16) } else { (28, 12) };
    let commands = b.at32(16)?;
    let mut at = header;
    let mut out = Vec::new();
    for _ in 0..commands {
        let size = b.at32(at + 4)?;
        if b.u32(at)? == LC_SYMTAB {
            let (symoff, nsyms, stroff) = (b.at32(at + 8)?, b.at32(at + 12)?, b.at32(at + 16)?);
            for i in 0..nsyms {
                let e = symoff + i * entry;
                let kind = b.u8(e + 4)?;
                if kind & N_STAB == 0 && kind & N_EXT != 0 && kind & N_TYPE != 0 {
                    out.push(b.string(stroff + b.at32(e)?)?);
                }
            }
        }
        if size == 0 {
            return Err("a load command of no size".into());
        }
        at += size;
    }
    Ok(out)
}

/// ELF: each `.symtab` symbol bound global, weak or unique and defined (a common one included,
/// as GNU `ar` indexes).
fn elf(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    const SHT_SYMTAB: u32 = 2;
    let wide = data.get(4) == Some(&2);
    let b = Bytes {
        data,
        little: data.get(5) != Some(&2),
    };
    let (shoff, shentsize, shnum) = if wide {
        (b.at64(0x28)?, b.u16(0x3a)?, b.u16(0x3c)?)
    } else {
        (b.at32(0x20)?, b.u16(0x2e)?, b.u16(0x30)?)
    };
    // (offset, size, link, entsize) of a section.
    let section = |i: usize| -> Result<(usize, usize, usize, usize), String> {
        let s = shoff + i * usize::from(shentsize);
        if wide {
            Ok((
                b.at64(s + 24)?,
                b.at64(s + 32)?,
                b.at32(s + 40)?,
                b.at64(s + 56)?,
            ))
        } else {
            Ok((
                b.at32(s + 16)?,
                b.at32(s + 20)?,
                b.at32(s + 24)?,
                b.at32(s + 36)?,
            ))
        }
    };
    let mut out = Vec::new();
    for i in 0..usize::from(shnum) {
        let s = shoff + i * usize::from(shentsize);
        if b.u32(s + 4)? != SHT_SYMTAB {
            continue;
        }
        let (offset, size, link, entsize) = section(i)?;
        let (strings, ..) = section(link)?;
        if entsize == 0 {
            return Err("a symbol table of entries of no size".into());
        }
        // The first entry is the null symbol.
        for k in 1..size.checked_div(entsize).unwrap_or(0) {
            let e = offset + k * entsize;
            let (name, info, shndx) = if wide {
                (b.at32(e)?, b.u8(e + 4)?, b.u16(e + 6)?)
            } else {
                (b.at32(e)?, b.u8(e + 12)?, b.u16(e + 14)?)
            };
            let binding = info >> 4;
            if matches!(binding, 1 | 2 | 10) && shndx != 0 {
                out.push(b.string(strings + name)?);
            }
        }
    }
    Ok(out)
}

/// COFF (and its big-object form): each external symbol that is defined or common, and each weak
/// external, as GNU and LLVM `ar` index.
fn coff(data: &[u8], big: bool) -> Result<Vec<Vec<u8>>, String> {
    const EXTERNAL: u8 = 2;
    const WEAK_EXTERNAL: u8 = 105;
    let b = Bytes { data, little: true };
    let (ptr, count, entry) = if big {
        (b.at32(48)?, b.at32(52)?, 20)
    } else {
        (b.at32(8)?, b.at32(12)?, 18)
    };
    let strings = ptr + count * entry;
    let mut out = Vec::new();
    let mut i = 0;
    while i < count {
        let e = ptr + i * entry;
        let value = b.u32(e + 8)?;
        let (section, class, aux) = if big {
            (b.u32(e + 12)?, b.u8(e + 18)?, b.u8(e + 19)?)
        } else {
            (u32::from(b.u16(e + 12)?), b.u8(e + 16)?, b.u8(e + 17)?)
        };
        if (class == EXTERNAL && (section != 0 || value != 0)) || class == WEAK_EXTERNAL {
            let short = b.take::<8>(e)?;
            let name = if short.get(..4) == Some(&[0, 0, 0, 0]) {
                b.string(strings + b.at32(e + 4)?)?
            } else {
                short.iter().copied().take_while(|&c| c != 0).collect()
            };
            out.push(name);
        }
        i += 1 + usize::from(aux);
    }
    Ok(out)
}

/// WebAssembly: each symbol of the `linking` section's symbol table that is neither local nor
/// undefined, and not a section's.
fn wasm(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    const SYMBOL_TABLE: u8 = 8;
    const LOCAL: u64 = 0x2;
    const UNDEFINED: u64 = 0x10;
    const EXPLICIT_NAME: u64 = 0x40;
    let mut r = Reader { data, at: 8 };
    let mut out = Vec::new();
    while r.at < data.len() {
        let id = r.byte()?;
        let size = r.leb()?;
        let end = r.at + size;
        if id == 0 && r.name()? == b"linking" {
            let _version = r.leb()?;
            while r.at < end {
                let kind = r.byte()?;
                let length = r.leb()?;
                let next = r.at + length;
                if kind == SYMBOL_TABLE {
                    for _ in 0..r.leb()? {
                        let symbol = r.byte()?;
                        let flags = u64::try_from(r.leb()?).map_err(|e| e.to_string())?;
                        let defined = flags & UNDEFINED == 0;
                        let name = match symbol {
                            // A function, global, tag or table: its index, then a name when it
                            // is defined or named explicitly.
                            0 | 2 | 4 | 5 => {
                                r.leb()?;
                                if defined || flags & EXPLICIT_NAME != 0 {
                                    Some(r.name()?.to_vec())
                                } else {
                                    None
                                }
                            }
                            // Data: a name, then where it is when defined.
                            1 => {
                                let name = r.name()?.to_vec();
                                if defined {
                                    r.leb()?;
                                    r.leb()?;
                                    r.leb()?;
                                }
                                Some(name)
                            }
                            // A section: its index only.
                            3 => {
                                r.leb()?;
                                None
                            }
                            other => return Err(format!("a symbol of unknown kind {other}")),
                        };
                        if let (Some(name), true) = (name, defined && flags & LOCAL == 0) {
                            out.push(name);
                        }
                    }
                }
                r.at = next;
            }
        }
        r.at = end;
    }
    Ok(out)
}

/// A WebAssembly reader: bytes, LEB128 numbers and names.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self
            .data
            .get(self.at)
            .ok_or_else(|| format!("truncated at byte {}", self.at))?;
        self.at += 1;
        Ok(b)
    }
    fn leb(&mut self) -> Result<usize, String> {
        let mut value: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return usize::try_from(value).map_err(|e| e.to_string());
            }
        }
        Err("a number longer than 64 bits".into())
    }
    fn name(&mut self) -> Result<&'a [u8], String> {
        let n = self.leb()?;
        let name = self
            .at
            .checked_add(n)
            .and_then(|end| self.data.get(self.at..end))
            .ok_or_else(|| format!("truncated name at byte {}", self.at))?;
        self.at += n;
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 64-bit Mach-O object defining `names` as external symbols in section 1, and an
    /// undefined `_missing`, a local `_hidden` and a debugging entry, none of them indexed.
    fn macho(names: &[&str]) -> Vec<u8> {
        let mut strtab = vec![0u8];
        let mut syms = Vec::new();
        let mut add = |name: &str, kind: u8, sect: u8| {
            let strx = u32::try_from(strtab.len()).unwrap_or_default();
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            syms.extend_from_slice(&strx.to_le_bytes());
            syms.extend_from_slice(&[kind, sect, 0, 0]);
            syms.extend_from_slice(&0u64.to_le_bytes());
        };
        add("_hidden", 0x0e, 1);
        add("_debug", 0x24, 1);
        for n in names {
            add(n, 0x0f, 1);
        }
        add("_missing", 0x01, 0);
        let count = u32::try_from(names.len() + 3).unwrap_or_default();
        let symoff = 32u32 + 24;
        let stroff = symoff + u32::try_from(syms.len()).unwrap_or_default();
        let mut o = Vec::new();
        for w in [0xfeed_facfu32, 0x0100_000c, 0, 1, 1, 24, 0, 0] {
            o.extend_from_slice(&w.to_le_bytes());
        }
        for w in [2u32, 24, symoff, count, stroff] {
            o.extend_from_slice(&w.to_le_bytes());
        }
        o.extend_from_slice(
            &u32::try_from(strtab.len())
                .unwrap_or_default()
                .to_le_bytes(),
        );
        o.extend_from_slice(&syms);
        o.extend_from_slice(&strtab);
        o
    }

    /// The BSD index: (name, offset) in its order.
    fn index(a: &[u8]) -> Vec<(String, u32)> {
        let b = Bytes {
            data: a,
            little: true,
        };
        let body = MAGIC.len() + 60 + 20;
        let size = b.at32(body).unwrap_or_default();
        let strings = body + 4 + size + 4;
        (0..size)
            .step_by(8)
            .map(|i| {
                let e = body + 4 + i;
                let name = b
                    .string(strings + b.at32(e).unwrap_or_default())
                    .unwrap_or_default();
                (
                    String::from_utf8_lossy(&name).into_owned(),
                    b.u32(e + 4).unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn a_mach_o_index_is_sorted_by_name_then_member_and_holds_only_defined_globals() {
        let a = archive(vec![
            ("b.o".into(), macho(&["_zeta", "_alpha", "_twice"])),
            ("a-long-member-name.o".into(), macho(&["_twice", "_beta"])),
        ])
        .unwrap_or_default();
        let idx = index(&a);
        let names: Vec<&str> = idx.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["_alpha", "_beta", "_twice", "_twice", "_zeta"]);
        let first = idx.first().map(|e| e.1).unwrap_or_default();
        let beta = idx.get(1).map(|e| e.1).unwrap_or_default();
        assert!(first < beta, "{idx:?}");
        assert_eq!(idx.get(2).map(|e| e.1), Some(first));
        assert_eq!(idx.get(3).map(|e| e.1), Some(beta));
        // Each offset is a member's header, its bytes 8-byte aligned after its padded name.
        for (_, off) in &idx {
            let off = usize::try_from(*off).unwrap_or_default();
            assert_eq!(a.get(off..off + 3), Some(&b"#1/"[..]));
            let n: usize = String::from_utf8_lossy(a.get(off + 3..off + 16).unwrap_or_default())
                .trim()
                .parse()
                .unwrap_or_default();
            assert_eq!((off + 60 + n) % 8, 0);
            assert_eq!(
                a.get(off + 60 + n..off + 64 + n),
                Some(&[0xcf, 0xfa, 0xed, 0xfe][..])
            );
        }
        assert_eq!(a.get(..8), Some(MAGIC));
        assert_eq!(a.len() % 8, 0);
    }

    #[test]
    fn the_same_members_are_the_same_bytes_in_any_order_of_symbols() {
        let one = archive(vec![("x.o".into(), macho(&["_a", "_b", "_c"]))]);
        let two = archive(vec![("x.o".into(), macho(&["_c", "_a", "_b"]))]);
        let (one, two) = (one.unwrap_or_default(), two.unwrap_or_default());
        // The objects differ, the index does not.
        assert_eq!(index(&one), index(&two));
    }

    #[test]
    fn a_coff_member_s_time_stamp_is_not_part_of_the_archive() {
        // x86-64 COFF: machine, sections, TimeDateStamp at byte 4; a big-object's is at byte 8.
        let mut plain = vec![0x64, 0x86, 0, 0, 0x7f, 0x12, 0x34, 0x56, 9, 9];
        let mut big = vec![
            0, 0, 0xff, 0xff, 2, 0, 0x64, 0x86, 0x7f, 0x12, 0x34, 0x56, 9,
        ];
        zero_coff_time(&mut plain);
        zero_coff_time(&mut big);
        assert_eq!(plain, [0x64, 0x86, 0, 0, 0, 0, 0, 0, 9, 9]);
        assert_eq!(big, [0, 0, 0xff, 0xff, 2, 0, 0x64, 0x86, 0, 0, 0, 0, 9]);
        // Any other format is left as it is.
        let mut elf = b"\x7fELF\x01\x02\x03\x04\x05".to_vec();
        zero_coff_time(&mut elf);
        assert_eq!(elf, b"\x7fELF\x01\x02\x03\x04\x05");
    }

    #[test]
    fn an_unknown_member_is_refused_by_name() {
        let e = archive(vec![("notes.txt".into(), b"hello, world".to_vec())]);
        assert!(matches!(&e, Err(m) if m.contains("`notes.txt`")), "{e:?}");
    }
}
