//! Debug information for a Cranelift object (N91, `docs/architecture.md` §7.92).
//!
//! What N48 gives an LLVM `--debug` build, from the same facts: a compile unit, a subprogram per
//! MIR function with its file and line, and a line table mapping each instruction to the
//! statement MIR recorded it for. Cranelift carries a source location on every instruction it is
//! given one for (`FunctionBuilder::set_srcloc`) and reports, once a function is compiled, the
//! code ranges each location covers; this module turns those ranges into DWARF 4 with `gimli`
//! and writes it into the object beside the code, with a relocation wherever an address or a
//! section offset is not known until link time.
//!
//! The prologue — slots, parameters, zeroing — carries the function's own line and ends where
//! the first statement's code begins (`prologue_end`), so a breakpoint on a function lands after
//! its parameters are stored, as an LLVM build's does. Code Cranelift made for no statement — an
//! epilogue, a spill — is line 0, which a debugger steps through. Variables are not described:
//! Cranelift reports where a value lives only through value labels this backend does not track.

use std::collections::BTreeMap;

use cranelift_module::FuncId;
use cranelift_object::ObjectProduct;
use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Range, RangeList,
    RelocateWriter, Relocation, RelocationTarget, Sections, Writer,
};
use gimli::{constants, Encoding, Format, LineEncoding, RunTimeEndian, SectionId};
use object::write::{Object, SectionId as ObjectSection, SymbolId};
use object::{BinaryFormat, RelocationEncoding, RelocationFlags, RelocationKind, SectionKind};

/// One function's facts, gathered as it is compiled.
pub(crate) struct Function {
    pub func: FuncId,
    pub name: String,
    /// Its declaration: file index and line.
    pub file: u32,
    pub line: u32,
    /// Its code's length in bytes.
    pub size: u32,
    /// Each code range that has a location: start, end, and the location's index.
    pub ranges: Vec<(u32, u32, u32)>,
}

/// A unit's debug information, while its functions are compiled.
#[derive(Default)]
pub(crate) struct Debug {
    /// Each source location an instruction was given: file index, line, column. Cranelift's
    /// `SourceLoc` is an index into this.
    pub locations: Vec<(u32, u32, u32)>,
    known: BTreeMap<(u32, u32, u32), u32>,
    pub functions: Vec<Function>,
}

impl Debug {
    /// The index of a location, made on first use.
    pub(crate) fn location(&mut self, at: (u32, u32, u32)) -> u32 {
        if let Some(&i) = self.known.get(&at) {
            return i;
        }
        let i = u32::try_from(self.locations.len()).unwrap_or(u32::MAX - 1);
        self.locations.push(at);
        self.known.insert(at, i);
        i
    }
}

/// A section's bytes as they are written, with every relocation `gimli` asks for.
#[derive(Clone)]
struct Section {
    writer: EndianVec<RunTimeEndian>,
    relocations: Vec<Relocation>,
}

impl RelocateWriter for Section {
    type Writer = EndianVec<RunTimeEndian>;

    fn writer(&self) -> &Self::Writer {
        &self.writer
    }

    fn writer_mut(&mut self) -> &mut Self::Writer {
        &mut self.writer
    }

    fn relocate(&mut self, relocation: Relocation) {
        self.relocations.push(relocation);
    }
}

/// Write `debug` into the object as DWARF sections. `file` names a file by its index.
///
/// # Errors
///
/// `gimli`'s or `object`'s, as text: a defect, since every input is the compiler's own.
pub(crate) fn write(
    product: &mut ObjectProduct,
    debug: &Debug,
    file: &dyn Fn(u32) -> String,
) -> Result<(), String> {
    let Some(first) = debug.functions.first() else {
        return Ok(());
    };
    // Every target this backend builds — aarch64 and x86_64 — is little-endian.
    let endian = RunTimeEndian::Little;
    let encoding = Encoding {
        format: Format::Dwarf32,
        version: 4,
        address_size: 8,
    };
    let mut dwarf = DwarfUnit::new(encoding);
    let unit_name = nonempty(file(first.file));
    dwarf.unit.line_program = LineProgram::new(
        encoding,
        LineEncoding::default(),
        LineString::String(b".".to_vec()),
        None,
        LineString::String(unit_name.clone().into_bytes()),
        None,
    );
    let directory = dwarf.unit.line_program.default_directory();
    let mut files = BTreeMap::new();
    // The symbols addresses are relative to: a function's, by its position here.
    let symbols: Vec<FuncId> = debug.functions.iter().map(|f| f.func).collect();
    let mut ranges = Vec::new();
    let root = dwarf.unit.root();
    for (k, f) in debug.functions.iter().enumerate() {
        let start = Address::Symbol {
            symbol: k,
            addend: 0,
        };
        let mut file_id = |idx: u32, dwarf: &mut DwarfUnit| {
            *files.entry(idx).or_insert_with(|| {
                dwarf.unit.line_program.add_file(
                    LineString::String(nonempty(file(idx)).into_bytes()),
                    directory,
                    None,
                )
            })
        };
        let decl_file = file_id(f.file, &mut dwarf);
        // The line table: the prologue at the declaration, each range at its statement, a gap
        // at line 0.
        dwarf.unit.line_program.begin_sequence(Some(start));
        let row = |dwarf: &mut DwarfUnit, offset: u32, file, line: u32, column: u32, end: bool| {
            let r = dwarf.unit.line_program.row();
            r.address_offset = u64::from(offset);
            r.file = file;
            r.line = u64::from(line);
            r.column = u64::from(column);
            r.prologue_end = end;
            dwarf.unit.line_program.generate_row();
        };
        row(&mut dwarf, 0, decl_file, f.line, 0, false);
        let mut at = 0u32;
        let mut prologue = true;
        for &(from, to, loc) in &f.ranges {
            let Some(&(fi, line, column)) = debug.locations.get(loc as usize) else {
                continue;
            };
            if from > at && !prologue {
                row(&mut dwarf, at, decl_file, 0, 0, false);
            }
            let fid = file_id(fi, &mut dwarf);
            row(&mut dwarf, from, fid, line, column, prologue);
            prologue = false;
            at = to;
        }
        if at < f.size && !prologue {
            row(&mut dwarf, at, decl_file, 0, 0, false);
        }
        dwarf.unit.line_program.end_sequence(u64::from(f.size));
        // The subprogram.
        let sp = dwarf.unit.add(root, constants::DW_TAG_subprogram);
        let name = dwarf.strings.add(f.name.as_bytes());
        let die = dwarf.unit.get_mut(sp);
        die.set(constants::DW_AT_name, AttributeValue::StringRef(name));
        die.set(
            constants::DW_AT_decl_file,
            AttributeValue::FileIndex(Some(decl_file)),
        );
        die.set(
            constants::DW_AT_decl_line,
            AttributeValue::Udata(u64::from(f.line)),
        );
        die.set(constants::DW_AT_low_pc, AttributeValue::Address(start));
        die.set(
            constants::DW_AT_high_pc,
            AttributeValue::Udata(u64::from(f.size)),
        );
        ranges.push(Range::StartLength {
            begin: start,
            length: u64::from(f.size),
        });
    }
    let range_list = dwarf.unit.ranges.add(RangeList(ranges));
    let producer = dwarf.strings.add("nazm");
    let name = dwarf.strings.add(unit_name.as_bytes());
    let dir = dwarf.strings.add(".");
    let unit = dwarf.unit.get_mut(root);
    unit.set(
        constants::DW_AT_producer,
        AttributeValue::StringRef(producer),
    );
    unit.set(
        constants::DW_AT_language,
        AttributeValue::Language(constants::DW_LANG_C99),
    );
    unit.set(constants::DW_AT_name, AttributeValue::StringRef(name));
    unit.set(constants::DW_AT_comp_dir, AttributeValue::StringRef(dir));
    unit.set(
        constants::DW_AT_low_pc,
        AttributeValue::Address(Address::Constant(0)),
    );
    unit.set(
        constants::DW_AT_ranges,
        AttributeValue::RangeListRef(range_list),
    );
    let mut sections = Sections::new(Section {
        writer: EndianVec::new(endian),
        relocations: Vec::new(),
    });
    dwarf.write(&mut sections).map_err(|e| e.to_string())?;
    let symbols: Vec<SymbolId> = symbols
        .iter()
        .map(|f| {
            product
                .functions
                .get(*f)
                .and_then(|s| s.map(|(id, _)| id))
                .ok_or_else(|| format!("no symbol for function {f}"))
        })
        .collect::<Result<_, _>>()?;
    place(&mut product.object, &mut sections, &symbols)
}

/// A name `gimli` accepts for a file: never empty.
fn nonempty(name: String) -> String {
    if name.is_empty() {
        "<unknown>".to_owned()
    } else {
        name
    }
}

/// The object's name for a DWARF section: `.debug_info` in ELF, `__debug_info` in the `__DWARF`
/// segment of Mach-O.
fn names(format: BinaryFormat, id: SectionId) -> (Vec<u8>, Vec<u8>) {
    match format {
        BinaryFormat::MachO => (
            b"__DWARF".to_vec(),
            format!("__{}", id.name().trim_start_matches('.')).into_bytes(),
        ),
        _ => (Vec::new(), id.name().as_bytes().to_vec()),
    }
}

/// Add each written section to the object, then its relocations: to a function's symbol for an
/// address, and to another DWARF section for an offset into it — in ELF, where a linker
/// concatenates every object's sections; in Mach-O the offset is written as it is, since each
/// object's DWARF stays its own and `dsymutil` reads it there.
fn place(
    object: &mut Object<'static>,
    sections: &mut Sections<Section>,
    symbols: &[SymbolId],
) -> Result<(), String> {
    let format = object.format();
    let macho = format == BinaryFormat::MachO;
    let coff = format == BinaryFormat::Coff;
    let mut ids: BTreeMap<SectionId, ObjectSection> = BTreeMap::new();
    sections.for_each_mut(|id, s| -> Result<(), String> {
        if s.writer.len() == 0 {
            return Ok(());
        }
        let (segment, name) = names(format, id);
        let section = object.add_section(segment, name, SectionKind::Debug);
        ids.insert(id, section);
        Ok(())
    })?;
    sections.for_each_mut(|id, s| -> Result<(), String> {
        let Some(&section) = ids.get(&id) else {
            return Ok(());
        };
        let mut relocations = Vec::new();
        for r in &s.relocations {
            match r.target {
                RelocationTarget::Section(_) if macho => {
                    let offset = u64::try_from(r.addend).map_err(|e| e.to_string())?;
                    s.writer
                        .write_udata_at(r.offset, offset, r.size)
                        .map_err(|e| e.to_string())?;
                }
                RelocationTarget::Section(target) => {
                    let Some(&t) = ids.get(&target) else {
                        return Err(format!("a relocation to an empty section {target:?}"));
                    };
                    // An offset into another DWARF section: on COFF a section-relative one, since
                    // the sections are placed past 4 GiB in a 64-bit image and an absolute
                    // 32-bit address would not fit.
                    relocations.push((
                        r.offset,
                        object.section_symbol(t),
                        r.addend,
                        r.size,
                        coff && r.size == 4,
                    ));
                }
                RelocationTarget::Symbol(k) => {
                    let Some(&f) = symbols.get(k) else {
                        return Err(format!("a relocation to no function ({k})"));
                    };
                    if macho {
                        // As the compilers `dsymutil` was written for do: relative to the
                        // function's section, the function's place in it the addend.
                        let sym = object.symbol(f);
                        let (object::write::SymbolSection::Section(section), value) =
                            (sym.section, sym.value)
                        else {
                            return Err(format!("function {k} is in no section"));
                        };
                        let addend = r.addend + i64::try_from(value).map_err(|e| e.to_string())?;
                        relocations.push((
                            r.offset,
                            object.section_symbol(section),
                            addend,
                            r.size,
                            false,
                        ));
                    } else {
                        relocations.push((r.offset, f, r.addend, r.size, false));
                    }
                }
            }
        }
        object.set_section_data(section, s.writer.take(), 1);
        for (offset, symbol, addend, size, section_relative) in relocations {
            object
                .add_relocation(
                    section,
                    object::write::Relocation {
                        offset: offset as u64,
                        symbol,
                        addend,
                        flags: RelocationFlags::Generic {
                            kind: if section_relative {
                                RelocationKind::SectionOffset
                            } else {
                                RelocationKind::Absolute
                            },
                            encoding: RelocationEncoding::Generic,
                            size: size * 8,
                        },
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
}

/// Settle a Mach-O object's DWARF addresses (N91). `dsymutil` reads each object's DWARF itself
/// and maps every address in it through the executable's debug map, so an address must be the
/// code's address *in the object*: a relocation relative to a section, with that address in
/// place. The writer leaves only the offset into the section there, since a section's address is
/// fixed when the object is laid out; this adds it, in the emitted bytes. An ELF object's
/// relocations carry their addends and need nothing.
///
/// # Errors
///
/// The object's own, as text: a defect.
pub(crate) fn settle_macho(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    use object::read::{Object as _, ObjectSection as _};
    let mut patches: Vec<(usize, u8, u64)> = Vec::new();
    {
        let file = object::read::File::parse(&*bytes).map_err(|e| e.to_string())?;
        if file.format() != BinaryFormat::MachO {
            return Ok(bytes);
        }
        for section in file.sections() {
            if section.segment_name().ok().flatten() != Some("__DWARF") {
                continue;
            }
            let Some((start, _)) = section.file_range() else {
                continue;
            };
            for (offset, r) in section.relocations() {
                let object::read::RelocationTarget::Section(idx) = r.target() else {
                    continue;
                };
                let target = file.section_by_index(idx).map_err(|e| e.to_string())?;
                let at = usize::try_from(start + offset).map_err(|e| e.to_string())?;
                patches.push((at, r.size(), target.address()));
            }
        }
    }
    for (at, size, address) in patches {
        let width = usize::from(size >> 3);
        let Some(slot) = bytes.get_mut(at..at + width) else {
            return Err(format!("a relocation outside its section at {at}"));
        };
        let mut word = [0u8; 8];
        if let Some(w) = word.get_mut(..width) {
            w.copy_from_slice(slot);
        }
        let value = u64::from_le_bytes(word).wrapping_add(address);
        if let Some(v) = value.to_le_bytes().get(..width) {
            slot.copy_from_slice(v);
        }
    }
    Ok(bytes)
}
