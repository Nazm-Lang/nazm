//! The freestanding runtime and entry (N62, `docs/architecture.md` §7.64), for each board of the
//! table (N86, §7.87): no OS, no C library, no heap, one thread.
//!
//! A board is a description — where its image loads, its UART, how its machine is stopped, its
//! architecture's startup — and the linker script, the runtime and the entry are generated from one
//! and from nothing else. A failure is recorded in plain globals and reported on the board's UART,
//! and the machine stopped with a status. `_start` sets the stack the linker script reserves,
//! calls `main` and prints its result. Nothing runs before `main` but that.

use std::borrow::Cow;
use std::fmt::Write as _;

use crate::preamble;

/// A board's instruction set, which decides its startup and how its machine is stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isa {
    Aarch64,
    Riscv64,
    /// ARMv7-M, Thumb-2 (Gate 3): a Cortex-M microcontroller.
    Thumbv7m,
    /// ARMv8-M Mainline with a single-precision FPU (M1): a Cortex-M33, the RP2350's.
    Thumbv8m,
    /// RV32IMC with Zicsr (M1): a RISC-V microcontroller with no atomic instructions, the
    /// ESP32-C3's.
    Riscv32,
}

impl Isa {
    /// Its name in a manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::Riscv64 => "riscv64",
            Self::Thumbv7m => "thumbv7m",
            Self::Thumbv8m => "thumbv8m.main",
            Self::Riscv32 => "riscv32",
        }
    }

    /// An Arm M profile (Gate 3, M1): a vector table first in the image, an NVIC, SysTick, and
    /// AArch32 semihosting.
    #[must_use]
    pub const fn m_profile(self) -> bool {
        matches!(self, Self::Thumbv7m | Self::Thumbv8m)
    }

    /// A microcontroller (Gate 3, M1): 32-bit pointers and a 64-bit `Int`, whose 64-bit
    /// division and atomics are the board runtime's, and which has no heap in 1.0.
    #[must_use]
    pub const fn microcontroller(self) -> bool {
        matches!(self, Self::Thumbv7m | Self::Thumbv8m | Self::Riscv32)
    }
}

/// One region of a microcontroller's memory (M1, `docs/spec.md`, *Boards*): where it
/// starts and how many bytes it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub origin: u64,
    pub length: u64,
}

impl Region {
    /// The first address past it.
    #[must_use]
    pub const fn end(self) -> u64 {
        self.origin.saturating_add(self.length)
    }
}

/// A board whose image runs from flash (M1): the code, read-only data and the initial values of
/// writable data are in `flash`; writable data, zeroed data and the stack in `ram`. `_start`
/// copies the data from its flash load address and zeroes the rest before anything else runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub flash: Region,
    pub ram: Region,
}

/// What a chip's boot ROM expects of an image beyond its code (M1, `docs/spec.md`,
/// *Boards*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Image {
    /// The RP2350's: an `IMAGE_DEF` block — a secure Arm executable for the RP2350 — within the
    /// first 4 KiB of flash, after the vector table (RP2350 datasheet, *Minimum viable image
    /// metadata*). Without it the boot ROM boots nothing.
    Rp2350,
}

impl Image {
    /// Its name in a manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rp2350 => "rp2350",
        }
    }
}

/// The RP2350 boot ROM's minimum `IMAGE_DEF` (M1): the block's start marker; `IMAGE_TYPE`, one
/// word, an executable for Arm in the secure state on the RP2350 (`0x1021`); the last item, one
/// word of items before it; the link to the next block, 0 for this one alone; the end marker.
pub const RP2350_IMAGE_DEF: [u32; 5] = [0xffff_ded3, 0x1021_0142, 0x0000_01ff, 0, 0xab12_3579];

/// How a board's machine is stopped with a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// ARM semihosting's `SYS_EXIT_EXTENDED`, which QEMU honours with `-semihosting`.
    Semihosting,
    /// SiFive's test device: `0x5555` for success, `code << 16 | 0x3333` for a failure.
    SifiveTest { address: u64 },
}

/// A board's UART (Gate 3): both are written a byte at a time at their data register, which is
/// the manifest's address; the kind says which device that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uart {
    /// ARM's PL011, QEMU `virt`'s on AArch64.
    Pl011,
    /// The NS16550A, QEMU `virt`'s on RISC-V.
    Ns16550a,
    /// Arm's CMSDK APB UART, the MPS2 boards' (Gate 3): enabled for transmission before `main`.
    Cmsdk,
}

impl Uart {
    /// Its name in a manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pl011 => "pl011",
            Self::Ns16550a => "ns16550a",
            Self::Cmsdk => "cmsdk",
        }
    }
}

/// A board's interrupt controller (Gate 3, `docs/spec.md`, *Boards*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    /// An AArch64 board's GICv2: its distributor and CPU interface. The timer is the virtual
    /// timer's private interrupt.
    Gicv2 { distributor: u64, cpu: u64 },
    /// A Cortex-M's NVIC, at the architecture's fixed addresses; the timer is the core's SysTick,
    /// counting at `clock_hz`.
    Nvic { clock_hz: u64 },
}

/// A board's interrupts (Gate 3, `docs/spec.md`, *Boards*): a controller and what it delivers.
/// AArch64's and Cortex-M's in 1.0; a RISC-V board has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interrupts {
    pub controller: Controller,
    /// The board timer's period, when the program wants it: its interrupt is named `timer`.
    pub timer_period_us: Option<u64>,
    /// Every further interrupt by name, with its number — a GICv2's `INTID`, or an NVIC's
    /// external interrupt, numbered from 0.
    pub lines: Vec<(String, u32)>,
}

/// The AArch64 virtual timer's interrupt on a GICv2: private peripheral interrupt 11, `INTID` 27.
pub const TIMER_INTID: u32 = 27;

/// SysTick's exception on an M profile: vector 15. An NVIC line `n` is vector `16 + n`.
pub const SYSTICK_VECTOR: u32 = 15;

impl Interrupts {
    /// The number a handler for `timer` is dispatched by: an `INTID`, or an M profile's vector.
    #[must_use]
    pub const fn timer_number(&self) -> u32 {
        match self.controller {
            Controller::Gicv2 { .. } => TIMER_INTID,
            Controller::Nvic { .. } => SYSTICK_VECTOR,
        }
    }

    /// The number a handler for line `id` is dispatched by.
    #[must_use]
    pub const fn line_number(&self, id: u32) -> u32 {
        match self.controller {
            Controller::Gicv2 { .. } => id,
            Controller::Nvic { .. } => 16 + id,
        }
    }
}

/// One interrupt handler the entry dispatches to (Gate 3): its `INTID`, the Nazm function's
/// symbol, and whether it takes the device's authority (an `MmioCap`, a word that is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    pub intid: u32,
    pub symbol: String,
    pub takes_cap: bool,
    /// Defined by the entry itself — the scheduler's tick — rather than by the program.
    pub defined_here: bool,
}

/// One task of the board's fixed-priority scheduler (Gate 3, `docs/spec.md`, *Boards*): its name,
/// the Nazm function's symbol, its priority (higher runs first) and period in ticks, and whether it
/// takes the device's authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub symbol: String,
    pub priority: u32,
    pub period: u64,
    pub takes_cap: bool,
}

/// One board: everything about it that is not the language's (N86). Since Gate 3 a board is a
/// manifest (`docs/spec.md`, *Boards*): the built-in ones below print as one ([`Board::manifest`]),
/// and `nazm build --board FILE` reads one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// The target triple a build names it by.
    pub triple: Cow<'static, str>,
    /// What it is, for a person.
    pub name: Cow<'static, str>,
    pub isa: Isa,
    /// Where the emulator loads the image, and so where `link.ld` places `_start`.
    pub load_address: u64,
    /// The UART's data register, written a byte at a time.
    pub uart: u64,
    pub uart_kind: Uart,
    pub stop: Stop,
    /// The image's one stack, in bytes: [`STACK`] unless a manifest says otherwise.
    pub stack: u64,
    /// A heap of this many bytes after the image (Gate 3), or none.
    pub heap: Option<u64>,
    /// A watchdog of this many scheduler ticks (Gate 3), or none: `watchdog_feed()` re-arms it,
    /// and expiring fails the image (`N0415`).
    pub watchdog: Option<u64>,
    /// Its interrupts (Gate 3), or none: with none, interrupts stay masked.
    pub interrupts: Option<Interrupts>,
    /// How it is booted: the emulator command line, the image last.
    pub emulator: Cow<'static, str>,
    /// Whether it has been run on hardware. No board has.
    pub hardware: bool,
    /// Flash and RAM (M1), for an image that runs from flash: `None` for an image loaded whole
    /// into memory, as an emulator loads one.
    pub memory: Option<Memory>,
    /// The Nazm function `_start` calls after memory and before the interrupts and `main` (M1):
    /// the chip's clocks, pads and console. A function of the program, by its name.
    pub init: Option<Cow<'static, str>>,
    /// What the boot ROM expects of the image (M1), or nothing beyond ELF.
    pub image: Option<Image>,
}

impl Board {
    /// The board as a manifest (Gate 3): the text `nazm build --board` reads back as this board.
    #[must_use]
    pub fn manifest(&self) -> String {
        let isa = self.isa.name();
        // A TOML basic string: only a quote and a backslash need escaping in a board's name.
        let name = self.name.replace('\\', "\\\\").replace('"', "\\\"");
        let mut out = format!(
            "name = \"{name}\"\nisa = \"{isa}\"\nload_address = {:#x}\nstack = {:#x}\n\n[uart]\nkind = \"{}\"\naddress = {:#x}\n\n[stop]\n",
            self.load_address,
            self.stack,
            self.uart_kind.name(),
            self.uart
        );
        match self.stop {
            Stop::Semihosting => out.push_str("kind = \"semihosting\"\n"),
            Stop::SifiveTest { address } => {
                let _ = writeln!(out, "kind = \"sifive-test\"\naddress = {address:#x}");
            }
        }
        // M1: a top-level key, so written before the tables, as TOML reads it back.
        if let Some(init) = &self.init {
            out = out.replacen("\n[uart]\n", &format!("init = \"{init}\"\n\n[uart]\n"), 1);
        }
        if let Some(m) = self.memory {
            let _ = write!(
                out,
                "\n[memory]\nflash = {{ origin = {:#x}, length = {:#x} }}\nram = {{ origin = {:#x}, length = {:#x} }}\n",
                m.flash.origin, m.flash.length, m.ram.origin, m.ram.length
            );
        }
        if let Some(i) = self.image {
            let _ = write!(out, "\n[image]\nformat = \"{}\"\n", i.name());
        }
        if let Some(h) = self.heap {
            let _ = write!(out, "\n[heap]\nsize = {h:#x}\n");
        }
        if let Some(t) = self.watchdog {
            let _ = write!(out, "\n[watchdog]\nticks = {t}\n");
        }
        if let Some(i) = &self.interrupts {
            match i.controller {
                Controller::Gicv2 { distributor, cpu } => {
                    let _ = write!(
                        out,
                        "\n[interrupts]\ncontroller = \"gicv2\"\ndistributor = {distributor:#x}\ncpu = {cpu:#x}\n"
                    );
                }
                Controller::Nvic { clock_hz } => {
                    let _ = write!(
                        out,
                        "\n[interrupts]\ncontroller = \"nvic\"\nclock_hz = {clock_hz}\n"
                    );
                }
            }
            if let Some(p) = i.timer_period_us {
                let _ = writeln!(out, "timer_period_us = {p}");
            }
            if !i.lines.is_empty() {
                out.push_str("\n[interrupts.lines]\n");
                for (name, id) in &i.lines {
                    let _ = writeln!(out, "{name} = {id}");
                }
            }
        }
        out
    }
}

/// Every board, in a fixed order.
pub const BOARDS: [Board; 5] = [
    Board {
        triple: Cow::Borrowed("aarch64-unknown-none"),
        name: Cow::Borrowed("QEMU virt, AArch64 (Cortex-A53)"),
        isa: Isa::Aarch64,
        load_address: 0x4008_0000,
        uart: 0x0900_0000,
        uart_kind: Uart::Pl011,
        stop: Stop::Semihosting,
        stack: STACK,
        heap: None,
        watchdog: None,
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel",
        ),
        hardware: false,
        memory: None,
        init: None,
        image: None,
    },
    Board {
        triple: Cow::Borrowed("riscv64gc-unknown-none-elf"),
        name: Cow::Borrowed("QEMU virt, RISC-V RV64GC, machine mode"),
        isa: Isa::Riscv64,
        load_address: 0x8000_0000,
        uart: 0x1000_0000,
        uart_kind: Uart::Ns16550a,
        stop: Stop::SifiveTest { address: 0x10_0000 },
        stack: STACK,
        heap: None,
        watchdog: None,
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-riscv64 -M virt -bios none -nographic -nic none -kernel",
        ),
        hardware: false,
        memory: None,
        init: None,
        image: None,
    },
    // Gate 3: an MCU-class board. The AN385 image's SSRAM1 is at 0, where the vector table must
    // be; its UART0 is a CMSDK APB UART.
    Board {
        triple: Cow::Borrowed("thumbv7m-none-eabi"),
        name: Cow::Borrowed("QEMU MPS2 AN385, Cortex-M3"),
        isa: Isa::Thumbv7m,
        load_address: 0,
        uart: 0x4000_4000,
        uart_kind: Uart::Cmsdk,
        stop: Stop::Semihosting,
        stack: STACK,
        heap: None,
        watchdog: None,
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-arm -M mps2-an385 -cpu cortex-m3 -nographic -nic none -semihosting -kernel",
        ),
        hardware: false,
        memory: None,
        init: None,
        image: None,
    },
    // M1: the Cortex-M33, on the AN505 image of the MPS2+ (an SSE-200 subsystem). The core starts
    // in the secure state from the secure alias of SSRAM1, which stands for flash; the image's
    // data and stack are in the secure alias of SSRAM2, and its UART0 is the secure alias of a
    // CMSDK APB UART.
    Board {
        triple: Cow::Borrowed("thumbv8m.main-none-eabihf"),
        name: Cow::Borrowed("QEMU MPS2 AN505, Cortex-M33"),
        isa: Isa::Thumbv8m,
        load_address: 0x1000_0000,
        uart: 0x5020_0000,
        uart_kind: Uart::Cmsdk,
        stop: Stop::Semihosting,
        stack: STACK,
        heap: None,
        watchdog: None,
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-arm -M mps2-an505 -cpu cortex-m33 -nographic -nic none -semihosting -kernel",
        ),
        hardware: false,
        memory: Some(Memory {
            flash: Region {
                origin: 0x1000_0000,
                length: 0x40_0000,
            },
            ram: Region {
                origin: 0x3800_0000,
                length: 0x20_0000,
            },
        }),
        init: None,
        image: None,
    },
    // M1: RV32IMC on QEMU's RISC-V `virt`, machine mode, no firmware. Its memory is all RAM, so the
    // board's first MiB stands for flash and the second for RAM: the image's data is copied
    // between them as a chip's is.
    Board {
        triple: Cow::Borrowed("riscv32imc-unknown-none-elf"),
        name: Cow::Borrowed("QEMU virt, RISC-V RV32IMC, machine mode"),
        isa: Isa::Riscv32,
        load_address: 0x8000_0000,
        uart: 0x1000_0000,
        uart_kind: Uart::Ns16550a,
        stop: Stop::SifiveTest { address: 0x10_0000 },
        stack: STACK,
        heap: None,
        watchdog: None,
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-riscv32 -M virt -bios none -nographic -nic none -kernel",
        ),
        hardware: false,
        memory: Some(Memory {
            flash: Region {
                origin: 0x8000_0000,
                length: 0x10_0000,
            },
            ram: Region {
                origin: 0x8010_0000,
                length: 0x10_0000,
            },
        }),
        init: None,
        image: None,
    },
];

/// The board a triple names, if it names one.
#[must_use]
pub fn board(triple: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|b| b.triple == triple)
}

/// The first board, N62's: what a caller that names no board means.
pub const AARCH64: &Board = &BOARDS[0];

/// Every board's one stack, in bytes: 64 KiB.
pub const STACK: u64 = 0x1_0000;

/// What `nz.stack_check` keeps in reserve below a recursive call (N49's quarter): 16 KiB.
pub const STACK_HEADROOM: u64 = STACK >> 2;

/// The linker script a freestanding image is linked with (`ld.lld -T link.ld`): `_start` first at
/// the board's load address, then text, read-only data, data and zeroed data — each with RISC-V's
/// small-data sections beside it (N86), which the linker would otherwise place as orphans ahead of
/// `_start` — then a 64 KiB stack whose bounds are `__stack_bottom` and `__stack_top`. Unwind
/// tables are discarded: a board has no unwinder, and an orphaned `.eh_frame` was placed at the
/// load address ahead of `_start` (N86).
#[must_use]
pub fn linker_script(board: &Board) -> String {
    linker_script_placing(board, &Placed::default())
}

/// The sections a program's `@section` names (Gate 3, `docs/spec.md`, *Boards*), by kind: each
/// is kept and placed after the board's own sections of that kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placed {
    pub code: Vec<String>,
    pub rodata: Vec<String>,
    pub data: Vec<String>,
}

impl Placed {
    /// The sections the objects' LLVM text places definitions in: a function's code, a
    /// constant's read-only data, a writable global's data. Read from what was compiled, so the
    /// script cannot disagree with the objects it links.
    #[must_use]
    pub fn of(texts: &[&str]) -> Self {
        let mut p = Self::default();
        for line in texts.iter().flat_map(|t| t.lines()) {
            let Some(at) = line.find(" section \"") else {
                continue;
            };
            let rest = &line[at + 10..];
            let Some(end) = rest.find('"') else { continue };
            let name = rest[..end].to_owned();
            let into = if line.starts_with("define ") {
                &mut p.code
            } else if line.contains(" global ") {
                &mut p.data
            } else {
                &mut p.rodata
            };
            if !into.contains(&name) {
                into.push(name);
            }
        }
        p.code.sort();
        p.rodata.sort();
        p.data.sort();
        p
    }
}

/// [`linker_script`], keeping and placing `placed`'s sections (Gate 3).
#[must_use]
pub fn linker_script_placing(board: &Board, placed: &Placed) -> String {
    let triple = &board.triple;
    let load = board.load_address;
    let stack = board.stack;
    let keep =
        |names: &[String]| -> String { names.iter().map(|n| format!(" KEEP(*({n}))")).collect() };
    let (code, rodata, data) = (keep(&placed.code), keep(&placed.rodata), keep(&placed.data));
    // An M profile's vector table first, at the load address (Gate 3).
    let vectors = if board.isa.m_profile() {
        "KEEP(*(.vectors)) "
    } else {
        ""
    };
    // An image that runs from flash (M1): its own script.
    if let Some(m) = board.memory {
        return flash_script(board, m, vectors, &code, &rodata, &data);
    }
    // A heap (Gate 3): the arena after the stack, its bounds `__heap_start` and `__heap_end`.
    let heap = board.heap.map_or_else(String::new, |h| {
        format!("  . = ALIGN(16);\n  __heap_start = .;\n  . = . + {h:#x};\n  __heap_end = .;\n")
    });
    format!(
        "/* Written by nazm build for {triple} (N62, N86). */\nENTRY(_start)\nSECTIONS\n{{\n  \
         . = {load:#x};\n  .text : {{ {vectors}*(.text.start) *(.text*){code} }}\n  .rodata : {{ *(.rodata*) *(.srodata*){rodata} }}\n  \
         .data : {{ *(.data*) *(.sdata*){data} }}\n  .bss : {{ *(.bss*) *(.sbss*) *(COMMON) }}\n  . = ALIGN(16);\n  __stack_bottom = .;\n  . = . + {stack:#x};\n  \
         __stack_top = .;\n{heap}  /DISCARD/ : {{ *(.eh_frame*) }}\n}}\n"
    )
}

/// The linker script of an image that runs from flash (M1, `docs/spec.md`, *Boards*):
/// the manifest's flash and RAM as `MEMORY`; the vector table, the boot ROM's block when the
/// manifest's `[image]` asks for one, code and read-only data in flash; writable data in RAM with
/// its initial values loaded in flash after the read-only data — `__data_start` to `__data_end`,
/// loaded at `__data_load` — then zeroed data (`__bss_start` to `__bss_end`) and the stack. The
/// linker refuses an image whose sections overflow a region, naming the region; the two
/// assertions name `N0850`, as the build does when it can see the overflow first.
fn flash_script(
    board: &Board,
    m: Memory,
    vectors: &str,
    code: &str,
    rodata: &str,
    data: &str,
) -> String {
    let triple = &board.triple;
    let stack = board.stack;
    let image = match board.image {
        Some(Image::Rp2350) => "KEEP(*(.image_def)) ",
        None => "",
    };
    format!(
        "/* Written by nazm build for {triple} (N62, N86, M1). */\nMEMORY\n{{\n  \
         FLASH (rx) : ORIGIN = {fo:#x}, LENGTH = {fl:#x}\n  RAM (rwx) : ORIGIN = {ro:#x}, LENGTH = {rl:#x}\n}}\n\
         ENTRY(_start)\nSECTIONS\n{{\n  \
         .text : {{ {vectors}{image}*(.text.start) *(.text*){code} }} > FLASH\n  \
         .rodata : {{ *(.rodata*) *(.srodata*){rodata} }} > FLASH\n  \
         .data : ALIGN(8) {{ __data_start = .; *(.data*) *(.sdata*){data} . = ALIGN(8); __data_end = .; }} > RAM AT > FLASH\n  \
         __data_load = LOADADDR(.data);\n  \
         .bss (NOLOAD) : ALIGN(8) {{ __bss_start = .; *(.bss*) *(.sbss*) *(COMMON) . = ALIGN(8); __bss_end = .; }} > RAM\n  \
         .stack (NOLOAD) : ALIGN(16) {{ __stack_bottom = .; . = . + {stack:#x}; __stack_top = .; }} > RAM\n  \
         /DISCARD/ : {{ *(.eh_frame*) }}\n}}\n\
         ASSERT(__data_load + SIZEOF(.data) <= {fe:#x}, \"nazm: error[N0850]: the image does not fit the board's flash\")\n\
         ASSERT(__stack_top <= {re:#x}, \"nazm: error[N0850]: the data and the stack do not fit the board's RAM\")\n",
        fo = m.flash.origin,
        fl = m.flash.length,
        ro = m.ram.origin,
        rl = m.ram.length,
        fe = m.flash.end(),
        re = m.ram.end(),
    )
}

/// [`emit_runtime`] for a program that reaches `needs` (Gate 3): on a 64-bit board the memory
/// functions; with a heap in the board's manifest, its arena and the runtime parts allocation
/// admits.
#[must_use]
pub fn emit_runtime_for(board: &Board, needs: &crate::Needs) -> String {
    let mut out = emit_runtime(board, needs.stack);
    if !board.isa.microcontroller() {
        out.push_str(MEMORY_RUNTIME);
    }
    if board.heap.is_some() {
        out.push_str(HEAP_RUNTIME);
        out.push_str(&crate::core::board_heap_parts(needs));
    } else if needs.strings {
        // Without a heap nothing is allocated, so every owner a value can hold is null — a
        // static atomic's, or a literal's — and counting it is nothing (Gate 3).
        out.push_str(
            "define hidden void @nz.str_retain(ptr %o) {\nentry:\n  ret void\n}\n\
             define hidden void @nz.str_release(ptr %o) {\nentry:\n  ret void\n}\n",
        );
    }
    out
}

/// A board's arena (Gate 3, `docs/spec.md`, *Boards*): a bump allocator over the linker
/// script's `__heap_start` to `__heap_end`, never returned to — `free` does nothing. Each block
/// is 16-aligned behind a 16-byte header holding its size, so `realloc` copies what there was.
/// The bump is a compare-and-swap, so an interrupt handler allocating while `main` does is safe.
/// Exhausted, `malloc` answers null, which every caller reports as `N0406`. `realloc` copies with
/// [`MEMORY_RUNTIME`]'s `memcpy`.
pub const HEAP_RUNTIME: &str = r#"
@__heap_start = external global i8
@__heap_end = external global i8
@nz.heap_next = internal global i64 0

define ptr @malloc(i64 %n) {
entry:
  %padded = add i64 %n, 31
  %need = and i64 %padded, -16
  %start = ptrtoint ptr @__heap_start to i64
  %end = ptrtoint ptr @__heap_end to i64
  %huge = icmp ugt i64 %n, %end
  br i1 %huge, label %none, label %retry
retry:
  %seen = load atomic i64, ptr @nz.heap_next seq_cst, align 8
  %fresh = icmp eq i64 %seen, 0
  %cur = select i1 %fresh, i64 %start, i64 %seen
  %next = add i64 %cur, %need
  %over = icmp ugt i64 %next, %end
  br i1 %over, label %none, label %claim
claim:
  %r = cmpxchg ptr @nz.heap_next, i64 %seen, i64 %next seq_cst seq_cst
  %ok = extractvalue { i64, i1 } %r, 1
  br i1 %ok, label %got, label %retry
got:
  %h = inttoptr i64 %cur to ptr
  store i64 %n, ptr %h
  %p = getelementptr i8, ptr %h, i64 16
  ret ptr %p
none:
  ret ptr null
}

define void @free(ptr %p) {
entry:
  ret void
}

define ptr @realloc(ptr %p, i64 %n) {
entry:
  %q = call ptr @malloc(i64 %n)
  %absent = icmp eq ptr %p, null
  %failed = icmp eq ptr %q, null
  %skip = or i1 %absent, %failed
  br i1 %skip, label %done, label %copy
copy:
  %h = getelementptr i8, ptr %p, i64 -16
  %old = load i64, ptr %h
  %less = icmp ult i64 %old, %n
  %m = select i1 %less, i64 %old, i64 %n
  %c = call ptr @memcpy(ptr %q, ptr %p, i64 %m)
  br label %done
done:
  ret ptr %q
}
"#;

/// The C library's memory functions for a 64-bit board, heap or none (Gate 3): code generation
/// calls `memcpy` and `memset` for a large value's copy or initialisation — a `@std/linalg`
/// matrix's — whatever allocates. Byte loops, which LLVM may not turn back into calls to
/// themselves.
pub const MEMORY_RUNTIME: &str = r#"
define ptr @memcpy(ptr %d, ptr %s, i64 %n) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %sp = getelementptr i8, ptr %s, i64 %i
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i64 %i
  store i8 %b, ptr %dp
  %j = add i64 %i, 1
  br label %loop
done:
  ret ptr %d
}

define ptr @memmove(ptr %d, ptr %s, i64 %n) "no-builtins" {
entry:
  %di = ptrtoint ptr %d to i64
  %si = ptrtoint ptr %s to i64
  %forward = icmp ule i64 %di, %si
  br i1 %forward, label %fwd, label %back
fwd:
  %c = call ptr @memcpy(ptr %d, ptr %s, i64 %n)
  ret ptr %d
back:
  br label %loop
loop:
  %i = phi i64 [ %n, %back ], [ %k, %body ]
  %more = icmp ugt i64 %i, 0
  br i1 %more, label %body, label %done
body:
  %k = sub i64 %i, 1
  %sp = getelementptr i8, ptr %s, i64 %k
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i64 %k
  store i8 %b, ptr %dp
  br label %loop
done:
  ret ptr %d
}

define ptr @memset(ptr %d, i32 %v, i64 %n) "no-builtins" {
entry:
  %b = trunc i32 %v to i8
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %dp = getelementptr i8, ptr %d, i64 %i
  store i8 %b, ptr %dp
  %j = add i64 %i, 1
  br label %loop
done:
  ret ptr %d
}

define i32 @memcmp(ptr %a, ptr %b, i64 %n) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %j, %same ]
  %more = icmp ult i64 %i, %n
  br i1 %more, label %body, label %equal
body:
  %ap = getelementptr i8, ptr %a, i64 %i
  %x = load i8, ptr %ap
  %bp = getelementptr i8, ptr %b, i64 %i
  %y = load i8, ptr %bp
  %eq = icmp eq i8 %x, %y
  br i1 %eq, label %same, label %differ
same:
  %j = add i64 %i, 1
  br label %loop
differ:
  %xw = zext i8 %x to i32
  %yw = zext i8 %y to i32
  %d = sub i32 %xw, %yw
  ret i32 %d
equal:
  ret i32 0
}

define i32 @bcmp(ptr %a, ptr %b, i64 %n) "no-builtins" {
entry:
  %r = call i32 @memcmp(ptr %a, ptr %b, i64 %n)
  ret i32 %r
}
"#;

/// The freestanding runtime: the failure state and its helpers, the UART, the semihosting exit,
/// the MMIO helpers and — where a function in a call cycle checks it — the stack check, against
/// the linker script's `__stack_bottom` rather than an operating system's limit.
#[must_use]
pub fn emit_runtime(board: &Board, stack: bool) -> String {
    let uart = board.uart;
    let exit = board_exit(board);
    // A CMSDK UART transmits only once enabled (Gate 3): its control register's TX-enable bit,
    // and a baud divider of 16, the least it accepts. The PL011 and the NS16550A need nothing.
    let uart_init = match board.uart_kind {
        Uart::Cmsdk => format!(
            "define hidden void @nz.uart_init() {{\nentry:\n  store volatile i32 16, ptr inttoptr (i64 {} to ptr)\n  store volatile i32 1, ptr inttoptr (i64 {} to ptr)\n  ret void\n}}\n",
            board.uart + 0x10,
            board.uart + 0x8
        ),
        Uart::Pl011 | Uart::Ns16550a => {
            "define hidden void @nz.uart_init() {\nentry:\n  ret void\n}\n".to_owned()
        }
    };
    let mut out = String::new();
    preamble(&mut out, "<nazm freestanding runtime>");
    out.push_str(&format!(
        r#"declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

@nz.failed = hidden global i1 false
@nz.errmsg = hidden global ptr null
@nz.errlen = hidden global i64 0

define hidden void @nz.fail(ptr %m, i64 %n) {{
entry:
  %had = load i1, ptr @nz.failed
  br i1 %had, label %done, label %record
record:
  store i1 true, ptr @nz.failed
  store ptr %m, ptr @nz.errmsg
  store i64 %n, ptr @nz.errlen
  br label %done
done:
  ret void
}}

; No C library keeps an `errno` here (N85): a foreign call's capture of it does nothing, and
; `c_errno()`, which would read it, is refused (`N0613`).
define hidden void @nz.errno_save() {{
entry:
  ret void
}}

define hidden i32 @nz.failed_now() {{
entry:
  %f = load i1, ptr @nz.failed
  %r = zext i1 %f to i32
  ret i32 %r
}}

define hidden void @nz.uart_byte(i8 %c) {{
entry:
  store volatile i8 %c, ptr inttoptr (i64 {uart} to ptr)
  ret void
}}

{uart_init}

define hidden void @nz.uart_write(ptr %p, i64 %n) {{
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %at = getelementptr i8, ptr %p, i64 %i
  %c = load i8, ptr %at
  call void @nz.uart_byte(i8 %c)
  %next = add i64 %i, 1
  br label %loop
done:
  ret void
}}

; An Int in decimal: its magnitude as an unsigned number, so the smallest Int is printed too.
define hidden void @nz.uart_int(i64 %v) {{
entry:
  %buf = alloca [20 x i8], align 1
  %neg = icmp slt i64 %v, 0
  %flip = sub i64 0, %v
  %mag0 = select i1 %neg, i64 %flip, i64 %v
  br label %digit
digit:
  %mag = phi i64 [ %mag0, %entry ], [ %rest, %digit ]
  %pos = phi i64 [ 20, %entry ], [ %at, %digit ]
  %at = sub i64 %pos, 1
  %d = urem i64 %mag, 10
  %d8 = trunc i64 %d to i8
  %ch = add i8 %d8, 48
  %slot = getelementptr i8, ptr %buf, i64 %at
  store i8 %ch, ptr %slot
  %rest = udiv i64 %mag, 10
  %again = icmp ne i64 %rest, 0
  br i1 %again, label %digit, label %sign
sign:
  br i1 %neg, label %minus, label %emit
minus:
  call void @nz.uart_byte(i8 45)
  br label %emit
emit:
  %first = getelementptr i8, ptr %buf, i64 %at
  %count = sub i64 20, %at
  call void @nz.uart_write(ptr %first, i64 %count)
  ret void
}}

{exit}
; After the diagnostic, the program's failure hook (Gate 3), handed the failure's number — the
; four digits after `nazm: error[N` — and then the machine stopped. An image's entry defines the
; hook's caller, doing nothing without an `@on_failure`; a library has no entry, so it is weak.
declare extern_weak void @nz_board_on_failure(i64)

define hidden void @nz.report() noreturn {{
entry:
  %m = load ptr, ptr @nz.errmsg
  %n = load i64, ptr @nz.errlen
  call void @nz.uart_write(ptr %m, i64 %n)
  %long = icmp uge i64 %n, 17
  br i1 %long, label %digits, label %hook
digits:
  %p0 = getelementptr i8, ptr %m, i64 13
  %c0 = load i8, ptr %p0
  %p1 = getelementptr i8, ptr %m, i64 14
  %c1 = load i8, ptr %p1
  %p2 = getelementptr i8, ptr %m, i64 15
  %c2 = load i8, ptr %p2
  %p3 = getelementptr i8, ptr %m, i64 16
  %c3 = load i8, ptr %p3
  %d0 = zext i8 %c0 to i64
  %d1 = zext i8 %c1 to i64
  %d2 = zext i8 %c2 to i64
  %d3 = zext i8 %c3 to i64
  %v0 = sub i64 %d0, 48
  %v1 = sub i64 %d1, 48
  %v2 = sub i64 %d2, 48
  %v3 = sub i64 %d3, 48
  %a0 = mul i64 %v0, 1000
  %a1 = mul i64 %v1, 100
  %a2 = mul i64 %v2, 10
  %s0 = add i64 %a0, %a1
  %s1 = add i64 %s0, %a2
  %code = add i64 %s1, %v3
  br label %hook
hook:
  %number = phi i64 [ 0, %entry ], [ %code, %digits ]
  %present = icmp ne ptr @nz_board_on_failure, null
  br i1 %present, label %call, label %stop
call:
  ; The program's failure is reported; the hook runs as code that has not failed, or its own
  ; first call would look like one. A failure inside it is recorded and the machine stopped.
  store i1 false, ptr @nz.failed
  call void @nz_board_on_failure(i64 %number)
  br label %stop
stop:
  call void @nz.board_exit(i64 2)
  unreachable
}}
"#
    ));
    out.push_str(MMIO_RUNTIME);
    out.push_str(ATOMIC_RUNTIME);
    // What an ARMv7-M has no instruction for, and LLVM calls instead (Gate 3); an ARMv8-M
    // Mainline has none of it either, nor an instruction between a 64-bit integer and a float
    // (M1).
    if board.isa.m_profile() {
        out.push_str(THUMB_RUNTIME);
    }
    if board.isa == Isa::Thumbv8m {
        out.push_str(THUMBV8M_FLOAT_RUNTIME);
    }
    // What an RV32IMC has no instruction for (M1): atomics, 64-bit division, and the C library's
    // memory functions that code generation calls.
    if board.isa == Isa::Riscv32 {
        out.push_str(RISCV32_RUNTIME);
    }
    if stack {
        // The quarter of the board's stack kept in reserve below a recursive call (N49's).
        let headroom = board.stack >> 2;
        let _ = write!(
            out,
            r"
@__stack_bottom = external global i8
declare ptr @llvm.frameaddress.p0(i32)

; The limit a unit's inlined check compares its frame with (the speed milestone), unset until the
; first check sets it (M1: a board's runtime had not defined it, so no image that recurses linked).
@nz.stack_limit = hidden global i64 0

define hidden i32 @nz.stack_check() {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %bottom = ptrtoint ptr @__stack_bottom to i64
  %limit = add i64 %bottom, {headroom}
  store i64 %limit, ptr @nz.stack_limit
  %past = icmp ult i64 %sp, %limit
  %r = zext i1 %past to i32
  ret i32 %r
}}

; A function whose own frame is large asks this instead (Q1-A1-01): past the limit, or with less
; than `need` bytes between this frame and the limit.
define hidden i32 @nz.stack_check_frame(i64 %need) {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %bottom = ptrtoint ptr @__stack_bottom to i64
  %limit = add i64 %bottom, {headroom}
  %past = icmp ult i64 %sp, %limit
  %room = sub i64 %sp, %limit
  %short = icmp ult i64 %room, %need
  %either = or i1 %past, %short
  %r = zext i1 %either to i32
  ret i32 %r
}}
"
        );
    }
    out
}

/// How the machine is stopped with `%code` (N62, N86): ARM semihosting's `SYS_EXIT_EXTENDED`
/// (0x20, `{ ADP_Stopped_ApplicationExit, code }`), or SiFive's test device.
fn board_exit(board: &Board) -> String {
    match board.stop {
        // AArch32's semihosting (Gate 3): `bkpt 0xab` on an M profile, and a block of words.
        Stop::Semihosting if board.isa.m_profile() => "; ARM semihosting SYS_EXIT_EXTENDED (0x20) on an M profile: { ADP_Stopped_ApplicationExit, code }.
define hidden void @nz.board_exit(i64 %code) noreturn {
entry:
  %blk = alloca [2 x i32], align 4
  store i32 131110, ptr %blk
  %c = getelementptr i32, ptr %blk, i32 1
  %c32 = trunc i64 %code to i32
  store i32 %c32, ptr %c
  call void asm sideeffect \"mov r0, #0x20\\0Amov r1, $0\\0Abkpt 0xab\", \"r,~{r0},~{r1},~{memory}\"(ptr %blk)
  br label %hang
hang:
  br label %hang
}
"
        .to_owned(),
        Stop::Semihosting => "; ARM semihosting SYS_EXIT_EXTENDED (0x20): { ADP_Stopped_ApplicationExit, code }.
define hidden void @nz.board_exit(i64 %code) noreturn {
entry:
  %blk = alloca [2 x i64], align 8
  store i64 131110, ptr %blk
  %c = getelementptr i64, ptr %blk, i64 1
  store i64 %code, ptr %c
  call void asm sideeffect \"mov x0, #0x20\\0Amov x1, $0\\0Ahlt #0xf000\", \"r,~{x0},~{x1},~{memory}\"(ptr %blk)
  br label %hang
hang:
  br label %hang
}
"
        .to_owned(),
        Stop::SifiveTest { address } => format!(
            "; SiFive's test device: 0x5555 stops the machine with success, `code << 16 | 0x3333` with
; `code` (N86).
define hidden void @nz.board_exit(i64 %code) noreturn {{
entry:
  %ok = icmp eq i64 %code, 0
  %c32 = trunc i64 %code to i32
  %shifted = shl i32 %c32, 16
  %fail = or i32 %shifted, 13107
  %word = select i1 %ok, i32 21845, i32 %fail
  store volatile i32 %word, ptr inttoptr (i64 {address} to ptr)
  br label %hang
hang:
  br label %hang
}}
"
        ),
    }
}

/// The device-register built-ins (N62, N86): a volatile 32-bit load and store at an address; and
/// (Gate 3) the watchdog's feed and its counters, which a board's scheduler arms. The hosted runtime
/// defines them too, unreachable there — a hosted build refuses a program using
/// them (`N0392`) — so one inventory describes both.
pub const MMIO_RUNTIME: &str = r"
define hidden i64 @nz.mmio_read8(i64 %addr) {
entry:
  %p = inttoptr i64 %addr to ptr
  %v = load volatile i8, ptr %p
  %r = zext i8 %v to i64
  ret i64 %r
}

define hidden i64 @nz.mmio_write8(i64 %addr, i64 %value) {
entry:
  %p = inttoptr i64 %addr to ptr
  %t = trunc i64 %value to i8
  store volatile i8 %t, ptr %p
  ret i64 0
}

define hidden i64 @nz.mmio_read16(i64 %addr) {
entry:
  %p = inttoptr i64 %addr to ptr
  %v = load volatile i16, ptr %p
  %r = zext i16 %v to i64
  ret i64 %r
}

define hidden i64 @nz.mmio_write16(i64 %addr, i64 %value) {
entry:
  %p = inttoptr i64 %addr to ptr
  %t = trunc i64 %value to i16
  store volatile i16 %t, ptr %p
  ret i64 0
}

define hidden i64 @nz.mmio_read32(i64 %addr) {
entry:
  %p = inttoptr i64 %addr to ptr
  %v = load volatile i32, ptr %p
  %r = zext i32 %v to i64
  ret i64 %r
}

define hidden i64 @nz.mmio_write32(i64 %addr, i64 %value) {
entry:
  %p = inttoptr i64 %addr to ptr
  %t = trunc i64 %value to i32
  store volatile i32 %t, ptr %p
  ret i64 0
}

define hidden i64 @nz.mmio_read64(i64 %addr) {
entry:
  %p = inttoptr i64 %addr to ptr
  %v = load volatile i64, ptr %p
  ret i64 %v
}

define hidden i64 @nz.mmio_write64(i64 %addr, i64 %value) {
entry:
  %p = inttoptr i64 %addr to ptr
  store volatile i64 %value, ptr %p
  ret i64 0
}

@nz.wd_reload = hidden global i64 0
@nz.wd_left = hidden global i64 0

define hidden i64 @nz.watchdog_feed() {
entry:
  %n = load atomic i64, ptr @nz.wd_reload seq_cst, align 8
  store atomic i64 %n, ptr @nz.wd_left seq_cst, align 8
  ret i64 0
}
";

/// What LLVM calls on an ARMv7-M for what it has no instruction for (Gate 3): 64-bit atomics and
/// 64-bit division. A board has no C library or compiler runtime, so the board's runtime defines
/// them. Each atomic is the plain access with interrupts masked — PRIMASK saved, `cpsid i`,
/// restored — which on one core no interrupt can tear; division is long division by shifts and
/// subtractions, which ARMv7-M has. The quotient and remainder come back in r0–r3, the AEABI's
/// convention, as a two-`i64` aggregate does. Nazm never divides by zero: it stops first (`N0401`).
pub const THUMB_RUNTIME: &str = r#"
define internal i32 @nz.mask() {
entry:
  %m = call i32 asm sideeffect "mrs $0, primask\0Acpsid i", "=r,~{memory}"()
  ret i32 %m
}

define internal void @nz.unmask(i32 %m) {
entry:
  call void asm sideeffect "msr primask, $0", "r,~{memory}"(i32 %m)
  ret void
}

define i64 @__atomic_load_8(ptr %p, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %v = load volatile i64, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %v
}

define void @__atomic_store_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  store volatile i64 %v, ptr %p
  call void @nz.unmask(i32 %m)
  ret void
}

define i64 @__atomic_exchange_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  store volatile i64 %v, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

define zeroext i1 @__atomic_compare_exchange_8(ptr %p, ptr %e, i64 %d, i32 %s, i32 %f) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %want = load i64, ptr %e
  %same = icmp eq i64 %old, %want
  br i1 %same, label %swap, label %differ
swap:
  store volatile i64 %d, ptr %p
  br label %done
differ:
  store i64 %old, ptr %e
  br label %done
done:
  call void @nz.unmask(i32 %m)
  ret i1 %same
}

define i64 @__atomic_fetch_add_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %new = add i64 %old, %v
  store volatile i64 %new, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

define i64 @__atomic_fetch_sub_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %new = sub i64 %old, %v
  store volatile i64 %new, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

; The EABI's memory functions (sizes are 32 bits here), which LLVM calls to copy and clear what
; a value occupies: byte loops LLVM may not turn back into calls to themselves. `memset`'s value
; comes after its length, as the EABI orders them.
define void @__aeabi_memcpy(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i32 %i, %n
  br i1 %more, label %body, label %done
body:
  %sp = getelementptr i8, ptr %s, i32 %i
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i32 %i
  store i8 %b, ptr %dp
  %j = add i32 %i, 1
  br label %loop
done:
  ret void
}

define void @__aeabi_memcpy4(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memcpy(ptr %d, ptr %s, i32 %n)
  ret void
}

define void @__aeabi_memcpy8(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memcpy(ptr %d, ptr %s, i32 %n)
  ret void
}

define void @__aeabi_memmove(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  %di = ptrtoint ptr %d to i32
  %si = ptrtoint ptr %s to i32
  %forward = icmp ule i32 %di, %si
  br i1 %forward, label %fwd, label %loop
fwd:
  call void @__aeabi_memcpy(ptr %d, ptr %s, i32 %n)
  ret void
loop:
  %i = phi i32 [ %n, %entry ], [ %k, %body ]
  %more = icmp ugt i32 %i, 0
  br i1 %more, label %body, label %done
body:
  %k = sub i32 %i, 1
  %sp = getelementptr i8, ptr %s, i32 %k
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i32 %k
  store i8 %b, ptr %dp
  br label %loop
done:
  ret void
}

define void @__aeabi_memmove4(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memmove(ptr %d, ptr %s, i32 %n)
  ret void
}

define void @__aeabi_memmove8(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memmove(ptr %d, ptr %s, i32 %n)
  ret void
}

define void @__aeabi_memset(ptr %d, i32 %n, i32 %c) "no-builtins" {
entry:
  %b = trunc i32 %c to i8
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i32 %i, %n
  br i1 %more, label %body, label %done
body:
  %dp = getelementptr i8, ptr %d, i32 %i
  store i8 %b, ptr %dp
  %j = add i32 %i, 1
  br label %loop
done:
  ret void
}

define void @__aeabi_memset4(ptr %d, i32 %n, i32 %c) "no-builtins" {
entry:
  call void @__aeabi_memset(ptr %d, i32 %n, i32 %c)
  ret void
}

define void @__aeabi_memset8(ptr %d, i32 %n, i32 %c) "no-builtins" {
entry:
  call void @__aeabi_memset(ptr %d, i32 %n, i32 %c)
  ret void
}

define void @__aeabi_memclr(ptr %d, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memset(ptr %d, i32 %n, i32 0)
  ret void
}

define void @__aeabi_memclr4(ptr %d, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memset(ptr %d, i32 %n, i32 0)
  ret void
}

define void @__aeabi_memclr8(ptr %d, i32 %n) "no-builtins" {
entry:
  call void @__aeabi_memset(ptr %d, i32 %n, i32 0)
  ret void
}

define { i64, i64 } @__aeabi_uldivmod(i64 %n, i64 %d) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 64, %entry ], [ %i1, %next ]
  %q = phi i64 [ 0, %entry ], [ %q2, %next ]
  %r = phi i64 [ 0, %entry ], [ %r2, %next ]
  %more = icmp ugt i32 %i, 0
  br i1 %more, label %step, label %done
step:
  %i1 = sub i32 %i, 1
  %sh = zext i32 %i1 to i64
  %bit = lshr i64 %n, %sh
  %b1 = and i64 %bit, 1
  %rs = shl i64 %r, 1
  %r1 = or i64 %rs, %b1
  %qs = shl i64 %q, 1
  %fits = icmp uge i64 %r1, %d
  br i1 %fits, label %take, label %keep
take:
  %rt = sub i64 %r1, %d
  %qt = or i64 %qs, 1
  br label %next
keep:
  br label %next
next:
  %r2 = phi i64 [ %rt, %take ], [ %r1, %keep ]
  %q2 = phi i64 [ %qt, %take ], [ %qs, %keep ]
  br label %loop
done:
  %a = insertvalue { i64, i64 } undef, i64 %q, 0
  %b = insertvalue { i64, i64 } %a, i64 %r, 1
  ret { i64, i64 } %b
}

define { i64, i64 } @__aeabi_ldivmod(i64 %n, i64 %d) {
entry:
  %nneg = icmp slt i64 %n, 0
  %dneg = icmp slt i64 %d, 0
  %nm = sub i64 0, %n
  %dm = sub i64 0, %d
  %na = select i1 %nneg, i64 %nm, i64 %n
  %da = select i1 %dneg, i64 %dm, i64 %d
  %u = call { i64, i64 } @__aeabi_uldivmod(i64 %na, i64 %da)
  %uq = extractvalue { i64, i64 } %u, 0
  %ur = extractvalue { i64, i64 } %u, 1
  %flip = xor i1 %nneg, %dneg
  %qn = sub i64 0, %uq
  %q = select i1 %flip, i64 %qn, i64 %uq
  %rn = sub i64 0, %ur
  %r = select i1 %nneg, i64 %rn, i64 %ur
  %a = insertvalue { i64, i64 } undef, i64 %q, 0
  %b = insertvalue { i64, i64 } %a, i64 %r, 1
  ret { i64, i64 } %b
}
"#;

/// What LLVM calls on an ARMv8-M Mainline with a single-precision FPU for what it has no
/// instruction for beyond [`THUMB_RUNTIME`] (M1): conversions between a 64-bit integer and a
/// `Float32`, and the widening of a `Float32` to a double and the comparison of two, which the
/// lowering's exact range check of a conversion from a float to an integer computes in (folded to
/// single precision when optimised, called unoptimised). Each is the run-time ABI's helper, with
/// the base procedure call standard's soft-float convention (`arm_aapcscc`) as the ABI requires
/// even of a hard-float image, and computes only in integers, so none calls itself. An integer is
/// rounded to the nearest float, ties to even; a float is truncated toward zero, a NaN is 0 and a
/// value out of range saturates — what LLVM's saturating conversion, which Nazm's lowering uses,
/// asks of them; a widening is exact; a comparison is IEEE 754's, false with a NaN. A program's
/// own `Float64` is refused (`N0854`): these are not its arithmetic.
pub const THUMBV8M_FLOAT_RUNTIME: &str = r#"
declare i64 @llvm.ctlz.i64(i64, i1)

define arm_aapcscc float @__aeabi_ul2f(i64 %x) "no-builtins" {
entry:
  %zero = icmp eq i64 %x, 0
  br i1 %zero, label %none, label %some
none:
  ret float 0.0
some:
  %lz = call i64 @llvm.ctlz.i64(i64 %x, i1 true)
  %n = shl i64 %x, %lz
  %top = lshr i64 %n, 40
  %rest = shl i64 %n, 24
  %above = icmp ugt i64 %rest, -9223372036854775808
  %tie = icmp eq i64 %rest, -9223372036854775808
  %low = and i64 %top, 1
  %odd = icmp ne i64 %low, 0
  %even = and i1 %tie, %odd
  %up = or i1 %above, %even
  %inc = zext i1 %up to i64
  %m = add i64 %top, %inc
  %e = sub i64 189, %lz
  %eb = shl i64 %e, 23
  %bits = add i64 %eb, %m
  %b32 = trunc i64 %bits to i32
  %f = bitcast i32 %b32 to float
  ret float %f
}

define arm_aapcscc float @__aeabi_l2f(i64 %x) "no-builtins" {
entry:
  %neg = icmp slt i64 %x, 0
  %flip = sub i64 0, %x
  %mag = select i1 %neg, i64 %flip, i64 %x
  %f = call arm_aapcscc float @__aeabi_ul2f(i64 %mag)
  %b = bitcast float %f to i32
  %sign = select i1 %neg, i32 -2147483648, i32 0
  %o = or i32 %b, %sign
  %r = bitcast i32 %o to float
  ret float %r
}

; The magnitude of a float that is at least 1 and below 2^64, truncated: its biased exponent and
; its bits.
define internal i64 @nz.f2u64(i32 %ex, i32 %b) {
entry:
  %frac = and i32 %b, 8388607
  %m32 = or i32 %frac, 8388608
  %m = zext i32 %m32 to i64
  %up = icmp uge i32 %ex, 150
  br i1 %up, label %left, label %right
left:
  %l = sub i32 %ex, 150
  %l64 = zext i32 %l to i64
  %v = shl i64 %m, %l64
  ret i64 %v
right:
  %r = sub i32 150, %ex
  %r64 = zext i32 %r to i64
  %w = lshr i64 %m, %r64
  ret i64 %w
}

define arm_aapcscc i64 @__aeabi_f2ulz(float %f) "no-builtins" {
entry:
  %b = bitcast float %f to i32
  %sh = lshr i32 %b, 23
  %ex = and i32 %sh, 255
  %frac = and i32 %b, 8388607
  %special = icmp eq i32 %ex, 255
  %some = icmp ne i32 %frac, 0
  %nan = and i1 %special, %some
  %neg = icmp slt i32 %b, 0
  %small = icmp ult i32 %ex, 127
  %n0 = or i1 %nan, %neg
  %nothing = or i1 %n0, %small
  br i1 %nothing, label %zero, label %sized
zero:
  ret i64 0
sized:
  %huge = icmp uge i32 %ex, 191
  br i1 %huge, label %max, label %fits
max:
  ret i64 -1
fits:
  %v = call i64 @nz.f2u64(i32 %ex, i32 %b)
  ret i64 %v
}

define arm_aapcscc i64 @__aeabi_f2lz(float %f) "no-builtins" {
entry:
  %b = bitcast float %f to i32
  %sh = lshr i32 %b, 23
  %ex = and i32 %sh, 255
  %frac = and i32 %b, 8388607
  %special = icmp eq i32 %ex, 255
  %some = icmp ne i32 %frac, 0
  %nan = and i1 %special, %some
  %small = icmp ult i32 %ex, 127
  %nothing = or i1 %nan, %small
  %neg = icmp slt i32 %b, 0
  br i1 %nothing, label %zero, label %sized
zero:
  ret i64 0
sized:
  %huge = icmp uge i32 %ex, 190
  br i1 %huge, label %bound, label %fits
bound:
  %edge = select i1 %neg, i64 -9223372036854775808, i64 9223372036854775807
  ret i64 %edge
fits:
  %v = call i64 @nz.f2u64(i32 %ex, i32 %b)
  %nv = sub i64 0, %v
  %r = select i1 %neg, i64 %nv, i64 %v
  ret i64 %r
}

declare i32 @llvm.ctlz.i32(i32, i1)

define arm_aapcscc double @__aeabi_f2d(float %f) "no-builtins" {
entry:
  %b = bitcast float %f to i32
  %s32 = and i32 %b, -2147483648
  %s64 = zext i32 %s32 to i64
  %sign = shl i64 %s64, 32
  %sh = lshr i32 %b, 23
  %ex = and i32 %sh, 255
  %frac = and i32 %b, 8388607
  %f64 = zext i32 %frac to i64
  %special = icmp eq i32 %ex, 255
  br i1 %special, label %inf, label %finite
inf:
  %im = shl i64 %f64, 29
  %ib = or i64 %im, 9218868437227405312
  %ir = or i64 %ib, %sign
  %id = bitcast i64 %ir to double
  ret double %id
finite:
  %sub = icmp eq i32 %ex, 0
  br i1 %sub, label %small, label %normal
normal:
  %e32 = add i32 %ex, 896
  %e64 = zext i32 %e32 to i64
  %eb = shl i64 %e64, 52
  %nm = shl i64 %f64, 29
  %nb = or i64 %eb, %nm
  %nr = or i64 %nb, %sign
  %nd = bitcast i64 %nr to double
  ret double %nd
small:
  %zero = icmp eq i32 %frac, 0
  br i1 %zero, label %signed_zero, label %subnormal
signed_zero:
  %zd = bitcast i64 %sign to double
  ret double %zd
subnormal:
  ; The value is frac * 2^-149: its top bit, at p, is 2^(p - 149), a double's exponent p + 874.
  %lz = call i32 @llvm.ctlz.i32(i32 %frac, i1 true)
  %p = sub i32 31, %lz
  %se = add i32 %p, 874
  %se64 = zext i32 %se to i64
  %seb = shl i64 %se64, 52
  %up = sub i32 52, %p
  %up64 = zext i32 %up to i64
  %sm0 = shl i64 %f64, %up64
  %sm = and i64 %sm0, 4503599627370495
  %sb = or i64 %seb, %sm
  %sr = or i64 %sb, %sign
  %sd = bitcast i64 %sr to double
  ret double %sd
}

; Two doubles' order, IEEE 754's: 2 when either is a NaN, else -1, 0 or 1. Each is mapped to an
; integer that orders as it does — its magnitude, negated when its sign is set — so both zeros are 0.
define internal i32 @nz.dorder(double %a, double %b) "no-builtins" {
entry:
  %x = bitcast double %a to i64
  %y = bitcast double %b to i64
  %xm = and i64 %x, 9223372036854775807
  %ym = and i64 %y, 9223372036854775807
  %xn = icmp ugt i64 %xm, 9218868437227405312
  %yn = icmp ugt i64 %ym, 9218868437227405312
  %nan = or i1 %xn, %yn
  br i1 %nan, label %unordered, label %ordered
unordered:
  ret i32 2
ordered:
  %xs = icmp slt i64 %x, 0
  %ys = icmp slt i64 %y, 0
  %xneg = sub i64 0, %xm
  %yneg = sub i64 0, %ym
  %xk = select i1 %xs, i64 %xneg, i64 %xm
  %yk = select i1 %ys, i64 %yneg, i64 %ym
  %lt = icmp slt i64 %xk, %yk
  %gt = icmp sgt i64 %xk, %yk
  %l = select i1 %lt, i32 -1, i32 0
  %r = select i1 %gt, i32 1, i32 %l
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmpeq(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %t = icmp eq i32 %o, 0
  %r = zext i1 %t to i32
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmplt(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %t = icmp eq i32 %o, -1
  %r = zext i1 %t to i32
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmple(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %t = icmp sle i32 %o, 0
  %r = zext i1 %t to i32
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmpge(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %ge = icmp sge i32 %o, 0
  %num = icmp ne i32 %o, 2
  %t = and i1 %ge, %num
  %r = zext i1 %t to i32
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmpgt(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %t = icmp eq i32 %o, 1
  %r = zext i1 %t to i32
  ret i32 %r
}

define arm_aapcscc i32 @__aeabi_dcmpun(double %a, double %b) "no-builtins" {
entry:
  %o = call i32 @nz.dorder(double %a, double %b)
  %t = icmp eq i32 %o, 2
  %r = zext i1 %t to i32
  ret i32 %r
}
"#;

/// What LLVM calls on an RV32IMC for what it has no instruction for (M1): the 64-bit atomics —
/// with no `A` extension, every atomic operation — 64-bit division, and the C library's `memcpy`,
/// `memmove` and `memset`, whose sizes are 32 bits here. Each atomic is the plain access with
/// machine interrupts masked — `mstatus.MIE` cleared and restored — which on one hart no
/// interrupt can tear, as [`THUMB_RUNTIME`]'s are on a Cortex-M; division is long division by
/// shifts and subtractions, as there, in libgcc's names. Nazm never divides by zero: it stops
/// first (`N0401`).
pub const RISCV32_RUNTIME: &str = r#"
define internal i32 @nz.mask() {
entry:
  %m = call i32 asm sideeffect "csrrci $0, mstatus, 8", "=r,~{memory}"()
  ret i32 %m
}

define internal void @nz.unmask(i32 %m) {
entry:
  %mie = and i32 %m, 8
  call void asm sideeffect "csrs mstatus, $0", "r,~{memory}"(i32 %mie)
  ret void
}

define i64 @__atomic_load_8(ptr %p, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %v = load volatile i64, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %v
}

define void @__atomic_store_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  store volatile i64 %v, ptr %p
  call void @nz.unmask(i32 %m)
  ret void
}

define i64 @__atomic_exchange_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  store volatile i64 %v, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

define zeroext i1 @__atomic_compare_exchange_8(ptr %p, ptr %e, i64 %d, i32 %s, i32 %f) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %want = load i64, ptr %e
  %same = icmp eq i64 %old, %want
  br i1 %same, label %swap, label %differ
swap:
  store volatile i64 %d, ptr %p
  br label %done
differ:
  store i64 %old, ptr %e
  br label %done
done:
  call void @nz.unmask(i32 %m)
  ret i1 %same
}

define i64 @__atomic_fetch_add_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %new = add i64 %old, %v
  store volatile i64 %new, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

define i64 @__atomic_fetch_sub_8(ptr %p, i64 %v, i32 %o) {
entry:
  %m = call i32 @nz.mask()
  %old = load volatile i64, ptr %p
  %new = sub i64 %old, %v
  store volatile i64 %new, ptr %p
  call void @nz.unmask(i32 %m)
  ret i64 %old
}

define ptr @memcpy(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i32 %i, %n
  br i1 %more, label %body, label %done
body:
  %sp = getelementptr i8, ptr %s, i32 %i
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i32 %i
  store i8 %b, ptr %dp
  %j = add i32 %i, 1
  br label %loop
done:
  ret ptr %d
}

define ptr @memmove(ptr %d, ptr %s, i32 %n) "no-builtins" {
entry:
  %di = ptrtoint ptr %d to i32
  %si = ptrtoint ptr %s to i32
  %forward = icmp ule i32 %di, %si
  br i1 %forward, label %fwd, label %loop
fwd:
  %c = call ptr @memcpy(ptr %d, ptr %s, i32 %n)
  ret ptr %d
loop:
  %i = phi i32 [ %n, %entry ], [ %k, %body ]
  %more = icmp ugt i32 %i, 0
  br i1 %more, label %body, label %done
body:
  %k = sub i32 %i, 1
  %sp = getelementptr i8, ptr %s, i32 %k
  %b = load i8, ptr %sp
  %dp = getelementptr i8, ptr %d, i32 %k
  store i8 %b, ptr %dp
  br label %loop
done:
  ret ptr %d
}

define ptr @memset(ptr %d, i32 %v, i32 %n) "no-builtins" {
entry:
  %b = trunc i32 %v to i8
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %j, %body ]
  %more = icmp ult i32 %i, %n
  br i1 %more, label %body, label %done
body:
  %dp = getelementptr i8, ptr %d, i32 %i
  store i8 %b, ptr %dp
  %j = add i32 %i, 1
  br label %loop
done:
  ret ptr %d
}

define internal { i64, i64 } @nz.udivmod64(i64 %n, i64 %d) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 64, %entry ], [ %i1, %next ]
  %q = phi i64 [ 0, %entry ], [ %q2, %next ]
  %r = phi i64 [ 0, %entry ], [ %r2, %next ]
  %more = icmp ugt i32 %i, 0
  br i1 %more, label %step, label %done
step:
  %i1 = sub i32 %i, 1
  %sh = zext i32 %i1 to i64
  %bit = lshr i64 %n, %sh
  %b1 = and i64 %bit, 1
  %rs = shl i64 %r, 1
  %r1 = or i64 %rs, %b1
  %qs = shl i64 %q, 1
  %fits = icmp uge i64 %r1, %d
  br i1 %fits, label %take, label %keep
take:
  %rt = sub i64 %r1, %d
  %qt = or i64 %qs, 1
  br label %next
keep:
  br label %next
next:
  %r2 = phi i64 [ %rt, %take ], [ %r1, %keep ]
  %q2 = phi i64 [ %qt, %take ], [ %qs, %keep ]
  br label %loop
done:
  %a = insertvalue { i64, i64 } undef, i64 %q, 0
  %b = insertvalue { i64, i64 } %a, i64 %r, 1
  ret { i64, i64 } %b
}

define i64 @__udivdi3(i64 %n, i64 %d) {
entry:
  %u = call { i64, i64 } @nz.udivmod64(i64 %n, i64 %d)
  %q = extractvalue { i64, i64 } %u, 0
  ret i64 %q
}

define i64 @__umoddi3(i64 %n, i64 %d) {
entry:
  %u = call { i64, i64 } @nz.udivmod64(i64 %n, i64 %d)
  %r = extractvalue { i64, i64 } %u, 1
  ret i64 %r
}

define i64 @__divdi3(i64 %n, i64 %d) {
entry:
  %nneg = icmp slt i64 %n, 0
  %dneg = icmp slt i64 %d, 0
  %nm = sub i64 0, %n
  %dm = sub i64 0, %d
  %na = select i1 %nneg, i64 %nm, i64 %n
  %da = select i1 %dneg, i64 %dm, i64 %d
  %u = call { i64, i64 } @nz.udivmod64(i64 %na, i64 %da)
  %uq = extractvalue { i64, i64 } %u, 0
  %flip = xor i1 %nneg, %dneg
  %qn = sub i64 0, %uq
  %q = select i1 %flip, i64 %qn, i64 %uq
  ret i64 %q
}

define i64 @__moddi3(i64 %n, i64 %d) {
entry:
  %nneg = icmp slt i64 %n, 0
  %dneg = icmp slt i64 %d, 0
  %nm = sub i64 0, %n
  %dm = sub i64 0, %d
  %na = select i1 %nneg, i64 %nm, i64 %n
  %da = select i1 %dneg, i64 %dm, i64 %d
  %u = call { i64, i64 } @nz.udivmod64(i64 %na, i64 %da)
  %ur = extractvalue { i64, i64 } %u, 1
  %rn = sub i64 0, %ur
  %r = select i1 %nneg, i64 %rn, i64 %ur
  ret i64 %r
}
"#;

/// The atomic built-ins' operations (Gate 3, `docs/spec.md`, *Atomics*): one sequentially
/// consistent access to an aligned 8-byte cell each. A board's runtime and the hosted one both
/// define them, as they do the device registers; on a target without threads LLVM makes each
/// the plain access it is with one task.
pub const ATOMIC_RUNTIME: &str = r"
define hidden i64 @nz.atomic_load(ptr %cell) {
entry:
  %v = load atomic i64, ptr %cell seq_cst, align 8
  ret i64 %v
}

define hidden void @nz.atomic_store(ptr %cell, i64 %v) {
entry:
  store atomic i64 %v, ptr %cell seq_cst, align 8
  ret void
}

define hidden i64 @nz.atomic_swap(ptr %cell, i64 %v) {
entry:
  %old = atomicrmw xchg ptr %cell, i64 %v seq_cst, align 8
  ret i64 %old
}

define hidden i1 @nz.atomic_cas(ptr %cell, i64 %expected, i64 %new) {
entry:
  %r = cmpxchg ptr %cell, i64 %expected, i64 %new seq_cst seq_cst, align 8
  %ok = extractvalue { i64, i1 } %r, 1
  ret i1 %ok
}
";

/// What `_start` paints the stack with when the run measures it (N63): a word no frame is
/// likely to hold, so the deepest overwritten word is the deepest the run reached.
pub const STACK_PAINT: u64 = 0x5a5a_5a5a_5a5a_5a5a;

/// The freestanding entry: `_start`, then `main`, its result on the UART, and the machine stopped.
///
/// With `watermark` (N63), `_start` first paints the whole stack with [`STACK_PAINT`], and after
/// `main`'s result the entry writes `nazm-stack: used=N`: the bytes between the stack's top and
/// the lowest word no longer painted — a measurement of that run, not a bound.
#[must_use]
pub fn emit_entry(board: &Board, entry: &str, roots: usize, watermark: bool) -> String {
    emit_entry_with(board, entry, roots, watermark, &[], None, &[], None)
}

/// [`emit_entry`], with the board's interrupts taken by `handlers` (Gate 3): the vector table,
/// the GICv2 and the timer set up before `main`, and every interrupt dispatched to its handler;
/// the failure hook, the scheduler's tasks, and (M1) the manifest's `init`, each by its symbol and
/// whether it takes the device's authority.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn emit_entry_with(
    board: &Board,
    entry: &str,
    roots: usize,
    watermark: bool,
    handlers: &[Handler],
    on_failure: Option<(&str, bool)>,
    tasks: &[Task],
    init: Option<(&str, bool)>,
) -> String {
    let irq = board
        .interrupts
        .as_ref()
        .filter(|_| board.isa == Isa::Aarch64);
    // A Cortex-M's interrupts (Gate 3): its vector table and an NVIC.
    let nvic = board.interrupts.as_ref().filter(|_| board.isa.m_profile());
    let mut out = String::new();
    preamble(&mut out, "<nazm freestanding entry>");
    // The UART, which the interrupt path, a fault and the watermark each write to: one declaration.
    out.push_str("declare void @nz.uart_write(ptr, i64)\n");
    let (start, paint, call): (&[&str], &[&str], &[&str]) = match board.isa {
        Isa::Aarch64 => (
            &[
                "  ldr x0, =__stack_top",
                // Floating point and SIMD are trapped at reset; code generation may use them.
                "  mov x1, #(3 << 20)",
                "  msr cpacr_el1, x1",
                "  isb",
                "  mov sp, x0",
            ],
            &[
                "  ldr x0, =__stack_bottom",
                "  ldr x1, =__stack_top",
                "  ldr x2, =0x5a5a5a5a5a5a5a5a",
                "2: cmp x0, x1",
                "  b.hs 3f",
                "  str x2, [x0], #8",
                "  b 2b",
                "3:",
            ],
            &["  bl nz_board_main", "1: b 1b"],
        ),
        // Machine mode with no firmware (N86): the floating-point unit is off at reset
        // (`mstatus.FS`), and code generation may use it.
        Isa::Riscv64 => (
            &[
                "  la sp, __stack_top",
                "  li t0, 0x6000",
                "  csrs mstatus, t0",
            ],
            &[
                "  la t0, __stack_bottom",
                "  la t1, __stack_top",
                "  li t2, 0x5a5a5a5a5a5a5a5a",
                "2: bgeu t0, t1, 3f",
                "  sd t2, 0(t0)",
                "  addi t0, t0, 8",
                "  j 2b",
                "3:",
            ],
            &["  call nz_board_main", "1: j 1b"],
        ),
        // ARMv7-M (Gate 3): the core loads the stack pointer from the vector table, so `_start`
        // sets it again only so that a debugger's restart finds it; there is no FPU on an M3.
        Isa::Thumbv7m => (
            &["  ldr r0, =__stack_top", "  mov sp, r0"],
            &[
                "  ldr r0, =__stack_bottom",
                "  ldr r1, =__stack_top",
                "  ldr r2, =0x5a5a5a5a",
                "2: cmp r0, r1",
                "  bhs 3f",
                "  str r2, [r0], #4",
                "  b 2b",
                "3:",
            ],
            &["  bl nz_board_main", "1: b 1b", "  .ltorg"],
        ),
        // ARMv8-M Mainline (M1): as ARMv7-M, and the single-precision FPU, off at reset, given
        // full access in CPACR before anything can use it — the hard-float convention passes a
        // `Float32` in its registers.
        Isa::Thumbv8m => (
            &[
                "  ldr r0, =__stack_top",
                "  mov sp, r0",
                "  ldr r0, =0xE000ED88",
                "  ldr r1, [r0]",
                "  orr r1, r1, #0xF00000",
                "  str r1, [r0]",
                "  dsb",
                "  isb",
            ],
            &[
                "  ldr r0, =__stack_bottom",
                "  ldr r1, =__stack_top",
                "  ldr r2, =0x5a5a5a5a",
                "2: cmp r0, r1",
                "  bhs 3f",
                "  str r2, [r0], #4",
                "  b 2b",
                "3:",
            ],
            &["  bl nz_board_main", "1: b 1b", "  .ltorg"],
        ),
        // RV32IMC in machine mode (M1): no floating point to enable; a trap — an illegal
        // instruction, a misaligned or faulting access — goes to `nz_fault`, after `_start` and
        // four-byte aligned as `mtvec`'s direct mode needs, which takes the stack afresh and
        // reports and stops, rather than to whatever the reset value of `mtvec` points at.
        Isa::Riscv32 => (
            &[
                "  la sp, __stack_top",
                "  la t0, nz_fault",
                "  csrw mtvec, t0",
            ],
            &[
                "  la t0, __stack_bottom",
                "  la t1, __stack_top",
                "  li t2, 0x5a5a5a5a",
                "2: bgeu t0, t1, 3f",
                "  sw t2, 0(t0)",
                "  addi t0, t0, 4",
                "  j 2b",
                "3:",
            ],
            &[
                "  call nz_board_main",
                "1: j 1b",
                ".balign 4",
                "nz_fault:",
                "  la sp, __stack_top",
                "  call nz_fault_report",
            ],
        ),
    };
    // An image that runs from flash (M1): its data copied from where it is loaded in flash, and
    // its zeroed data zeroed, before anything reads either — the statics' initial values.
    let memory: &[&str] = match (board.memory.is_some(), board.isa) {
        (false, _) => &[],
        (true, Isa::Riscv32) => &[
            "  la t0, __data_start",
            "  la t1, __data_end",
            "  la t2, __data_load",
            "4: bgeu t0, t1, 5f",
            "  lw t3, 0(t2)",
            "  sw t3, 0(t0)",
            "  addi t0, t0, 4",
            "  addi t2, t2, 4",
            "  j 4b",
            "5: la t0, __bss_start",
            "  la t1, __bss_end",
            "6: bgeu t0, t1, 7f",
            "  sw zero, 0(t0)",
            "  addi t0, t0, 4",
            "  j 6b",
            "7:",
        ],
        (true, Isa::Thumbv7m | Isa::Thumbv8m) => &[
            "  ldr r0, =__data_start",
            "  ldr r1, =__data_end",
            "  ldr r2, =__data_load",
            "4: cmp r0, r1",
            "  bhs 5f",
            "  ldr r3, [r2], #4",
            "  str r3, [r0], #4",
            "  b 4b",
            "5: ldr r0, =__bss_start",
            "  ldr r1, =__bss_end",
            "  movs r2, #0",
            "6: cmp r0, r1",
            "  bhs 7f",
            "  str r2, [r0], #4",
            "  b 6b",
            "7:",
        ],
        // A 64-bit board is loaded whole: a manifest gives `[memory]` to a microcontroller only.
        (true, Isa::Aarch64 | Isa::Riscv64) => &[],
    };
    // The board's own start (M1): its `init`, after memory and before the interrupts and `main`.
    let board_init: &[&str] = match (init.is_some(), board.isa) {
        (false, _) => &[],
        (true, Isa::Riscv32 | Isa::Riscv64) => &["  call nz_board_init"],
        (true, _) => &["  bl nz_board_init"],
    };
    // With interrupts (Gate 3): the vector table installed and the controller and timer set up
    // before `main`, then IRQs unmasked.
    let arm: &[&str] = if irq.is_some() {
        &[
            "  adr x0, nz_vectors",
            "  msr vbar_el1, x0",
            "  isb",
            "  bl nz_irq_init",
            "  msr daifclr, #2",
        ]
    } else if nvic.is_some() {
        &["  cpsid i", "  bl nz_irq_init", "  cpsie i"]
    } else {
        &[]
    };
    // An M profile's vector table (Gate 3), first in the image at its load address: the initial
    // stack pointer, `_start` as the reset handler, and every other exception a fault — reported
    // and the machine stopped. The interrupts 3E adds take their own entries.
    let head: Vec<String> = if board.isa.m_profile() {
        // Every vector up to the highest one handled: a handled one names its handler's caller.
        let last = handlers.iter().map(|h| h.intid).max().unwrap_or(0).max(15);
        let mut v: Vec<String> = [
            ".syntax unified",
            ".thumb",
            ".section .vectors, \\22a\\22",
            ".p2align 2",
            ".global nz_vector_table",
            "nz_vector_table:",
            "  .word __stack_top",
            "  .word _start",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        for k in 2..=last {
            let handled = nvic.is_some() && handlers.iter().any(|h| h.intid == k);
            v.push(if handled {
                format!("  .word nz_vec_{k}")
            } else {
                "  .word nz_fault".to_owned()
            });
        }
        v.extend(
            [
                ".section .text.start, \\22ax\\22",
                ".thumb_func",
                "nz_fault:",
                "  bl nz_fault_report",
                ".global _start",
                ".thumb_func",
                "_start:",
            ]
            .iter()
            .map(|s| (*s).to_owned()),
        );
        v
    } else {
        [
            ".section .text.start, \\22ax\\22",
            ".global _start",
            "_start:",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    };
    // The RP2350 boot ROM's block (M1), placed after the vector table by the linker script.
    let image_def: Vec<String> = match board.image {
        Some(Image::Rp2350) => {
            let mut v = vec![
                ".section .image_def, \\22a\\22".to_owned(),
                ".p2align 2".to_owned(),
                ".global nz_image_def".to_owned(),
                "nz_image_def:".to_owned(),
            ];
            v.extend(
                RP2350_IMAGE_DEF
                    .iter()
                    .map(|w| format!("  .word {w:#010x}")),
            );
            v
        }
        None => Vec::new(),
    };
    for line in image_def
        .iter()
        .chain(head.iter())
        .map(String::as_str)
        .chain(start.iter().copied())
        .chain(memory.iter().copied())
        .chain(if watermark { paint } else { &[] }.iter().copied())
        .chain(board_init.iter().copied())
        .chain(arm.iter().copied())
        .chain(call.iter().copied())
    {
        let _ = writeln!(out, "module asm \"{line}\"");
    }
    if let Some(i) = irq {
        vectors(&mut out);
        irq_runtime(&mut out, i, handlers);
    }
    if let Some(i) = nvic {
        nvic_runtime(&mut out, i, handlers);
    }
    if !tasks.is_empty() {
        scheduler(&mut out, tasks, board.watchdog);
    }
    // The failure hook's caller (Gate 3), which the runtime's report calls: the program's
    // `@on_failure`, or nothing.
    match on_failure {
        Some((hook, takes_cap)) => {
            let (param, arg) = if takes_cap {
                (", i64", ", i64 0")
            } else {
                ("", "")
            };
            let _ = write!(
                out,
                "declare i64 @{hook}(i64{param})\ndefine void @nz_board_on_failure(i64 %code) {{\nentry:\n  %r = call i64 @{hook}(i64 %code{arg})\n  ret void\n}}\n"
            );
        }
        None => {
            out.push_str("define void @nz_board_on_failure(i64 %code) {\nentry:\n  ret void\n}\n")
        }
    }
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    let _ = writeln!(out, "@nz.failed = external global i1");
    let _ = writeln!(out, "declare void @nz.report() noreturn");
    let _ = writeln!(out, "declare void @nz.uart_int(i64)");
    let _ = writeln!(out, "declare void @nz.uart_byte(i8)");
    let _ = writeln!(out, "declare void @nz.board_exit(i64) noreturn");
    let _ = writeln!(out, "declare void @nz.uart_init()");
    if board.isa.m_profile() || board.isa == Isa::Riscv32 {
        let _ = write!(
            out,
            "@.nz.fault = private constant [27 x i8] c\"nazm: a fault of the image\\0A\"\n\
             define void @nz_fault_report() noreturn {{\nentry:\n  call void @nz.uart_write(ptr @.nz.fault, i64 27)\n  call void @nz.board_exit(i64 2)\n  unreachable\n}}\n"
        );
    }
    // The board's `init` (M1): the UART first, so a failure in it is reported, then the function;
    // a failure stops the machine as `main`'s does, and its answer is not the image's.
    if let Some((symbol, takes_cap)) = init {
        let (param, arg) = if takes_cap {
            ("i64", "i64 0")
        } else {
            ("", "")
        };
        let _ = write!(
            out,
            "declare i64 @{symbol}({param})\ndefine void @nz_board_init() {{\nentry:\n  call void @nz.uart_init()\n  %r = call i64 @{symbol}({arg})\n  %failed = load i1, ptr @nz.failed\n  br i1 %failed, label %reported, label %done\nreported:\n  call void @nz.report()\n  unreachable\ndone:\n  ret void\n}}\n"
        );
    }
    let _ = writeln!(out, "define void @nz_board_main() {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  call void @nz.uart_init()");
    let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
    let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
    let _ = writeln!(out, "  br i1 %failed, label %reported, label %finished");
    let _ = writeln!(out, "reported:");
    let _ = writeln!(out, "  call void @nz.report()");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "finished:");
    // With tasks (Gate 3), `main` was the initialisation: the scheduler runs until a task answers
    // something other than 0, which is the image's result, or one fails.
    let result = if tasks.is_empty() {
        "%result"
    } else {
        let _ = writeln!(out, "  %scheduled = call i64 @nz_sched_run()");
        let _ = writeln!(out, "  %fell = load i1, ptr @nz.failed");
        let _ = writeln!(out, "  br i1 %fell, label %reported, label %ended");
        let _ = writeln!(out, "ended:");
        "%scheduled"
    };
    let _ = writeln!(out, "  call void @nz.uart_int(i64 {result})");
    let _ = writeln!(out, "  call void @nz.uart_byte(i8 10)");
    if watermark {
        let _ = writeln!(out, "  %used = call i64 @nz.stack_used()");
        let _ = writeln!(
            out,
            "  call void @nz.uart_write(ptr @.nz.stack_label, i64 17)"
        );
        let _ = writeln!(out, "  call void @nz.uart_int(i64 %used)");
        let _ = writeln!(out, "  call void @nz.uart_byte(i8 10)");
    }
    let _ = writeln!(out, "  call void @nz.board_exit(i64 0)");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "}}");
    if watermark {
        let _ = write!(
            out,
            r#"
@__stack_bottom = external global i8
@__stack_top = external global i8
@.nz.stack_label = private constant [17 x i8] c"nazm-stack: used="

; The bytes between the stack's top and the lowest word no longer painted.
define internal i64 @nz.stack_used() {{
entry:
  %bottom = ptrtoint ptr @__stack_bottom to i64
  %top = ptrtoint ptr @__stack_top to i64
  br label %scan
scan:
  %a = phi i64 [ %bottom, %entry ], [ %n, %next ]
  %done = icmp uge i64 %a, %top
  br i1 %done, label %found, label %look
look:
  %p = inttoptr i64 %a to ptr
  %w = load volatile i64, ptr %p
  %painted = icmp eq i64 %w, {STACK_PAINT}
  br i1 %painted, label %next, label %found
next:
  %n = add i64 %a, 8
  br label %scan
found:
  %used = sub i64 %top, %a
  ret i64 %used
}}
"#
        );
    }
    out
}

/// The AArch64 vector table and the IRQ path (Gate 3): sixteen 128-byte entries, the IRQ from the
/// current level on `SP_ELx` taken to `nz_irq_entry`, every other exception to
/// `nz_irq_unexpected`. The IRQ path saves what a call may change — `x0`–`x18`, the frame and link
/// registers, every SIMD and floating-point register, and the exception's return state — calls
/// the dispatcher on the board's one stack, restores, and returns from the exception.
fn vectors(out: &mut String) {
    let mut asm: Vec<String> = vec![
        ".section .text.vectors, \\22ax\\22".to_owned(),
        ".balign 2048".to_owned(),
        "nz_vectors:".to_owned(),
    ];
    for i in 0..16 {
        asm.push(".balign 128".to_owned());
        asm.push(if i == 5 {
            "  b nz_irq_entry".to_owned()
        } else {
            "  b nz_irq_trap".to_owned()
        });
    }
    asm.extend(
        [
            "nz_irq_trap:",
            "  bl nz_irq_unexpected",
            "nz_irq_entry:",
            "  sub sp, sp, #704",
            "  stp x0, x1, [sp, #0]",
            "  stp x2, x3, [sp, #16]",
            "  stp x4, x5, [sp, #32]",
            "  stp x6, x7, [sp, #48]",
            "  stp x8, x9, [sp, #64]",
            "  stp x10, x11, [sp, #80]",
            "  stp x12, x13, [sp, #96]",
            "  stp x14, x15, [sp, #112]",
            "  stp x16, x17, [sp, #128]",
            "  stp x18, x29, [sp, #144]",
            "  str x30, [sp, #160]",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    for q in (0..32).step_by(2) {
        asm.push(format!("  stp q{q}, q{}, [sp, #{}]", q + 1, 192 + q * 16));
    }
    asm.extend(
        [
            "  mrs x0, elr_el1",
            "  mrs x1, spsr_el1",
            "  stp x0, x1, [sp, #168]",
            "  bl nz_irq_dispatch",
            "  ldp x0, x1, [sp, #168]",
            "  msr elr_el1, x0",
            "  msr spsr_el1, x1",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    for q in (0..32).step_by(2) {
        asm.push(format!("  ldp q{q}, q{}, [sp, #{}]", q + 1, 192 + q * 16));
    }
    asm.extend(
        [
            "  ldr x30, [sp, #160]",
            "  ldp x18, x29, [sp, #144]",
            "  ldp x16, x17, [sp, #128]",
            "  ldp x14, x15, [sp, #112]",
            "  ldp x12, x13, [sp, #96]",
            "  ldp x10, x11, [sp, #80]",
            "  ldp x8, x9, [sp, #64]",
            "  ldp x6, x7, [sp, #48]",
            "  ldp x4, x5, [sp, #32]",
            "  ldp x2, x3, [sp, #16]",
            "  ldp x0, x1, [sp, #0]",
            "  add sp, sp, #704",
            "  eret",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    for line in asm {
        let _ = writeln!(out, "module asm \"{line}\"");
    }
}

/// The GICv2 and timer set-up, and the dispatcher (Gate 3).
///
/// `nz_irq_init` enables each handled interrupt at the distributor (its priority 0x80, an SPI
/// targeted at CPU 0), opens the CPU interface to every priority, and starts the virtual timer
/// with the period's ticks when the timer is handled. `nz_irq_dispatch` acknowledges the pending
/// interrupt, re-arms the timer when it is the timer's, calls its handler, reports and stops the
/// machine if the handler failed, and signals the end of the interrupt. An exception that is not
/// an IRQ is a failure of the image, reported and stopped as one.
fn irq_runtime(out: &mut String, i: &Interrupts, handlers: &[Handler]) {
    let Controller::Gicv2 {
        distributor: d,
        cpu: c,
    } = i.controller
    else {
        return;
    };
    let _ = write!(
        out,
        r#"
@.nz.unexpected = private constant [48 x i8] c"nazm: an exception the image has no handler for\0A"

define void @nz_irq_unexpected() noreturn {{
entry:
  call void @nz.uart_write(ptr @.nz.unexpected, i64 48)
  call void @nz.board_exit(i64 2)
  unreachable
}}

define void @nz_irq_init() {{
entry:
  store volatile i32 0, ptr inttoptr (i64 {d} to ptr)
"#
    );
    for h in handlers {
        let n = u64::from(h.intid);
        let enable = d + 0x100 + 4 * (n >> 5);
        let bit = 1u64 << (n % 32);
        let _ = writeln!(
            out,
            "  store volatile i32 {bit}, ptr inttoptr (i64 {enable} to ptr)"
        );
        let _ = writeln!(
            out,
            "  store volatile i8 -128, ptr inttoptr (i64 {} to ptr)",
            d + 0x400 + n
        );
        if n >= 32 {
            let _ = writeln!(
                out,
                "  store volatile i8 1, ptr inttoptr (i64 {} to ptr)",
                d + 0x800 + n
            );
        }
    }
    let _ = writeln!(out, "  store volatile i32 1, ptr inttoptr (i64 {d} to ptr)");
    let _ = writeln!(
        out,
        "  store volatile i32 255, ptr inttoptr (i64 {} to ptr)",
        c + 4
    );
    let _ = writeln!(out, "  store volatile i32 1, ptr inttoptr (i64 {c} to ptr)");
    let timer = handlers.iter().any(|h| h.intid == TIMER_INTID);
    if let (true, Some(p)) = (timer, i.timer_period_us) {
        let _ = writeln!(out, "  call void @nz_timer_arm()");
        let _ = write!(
            out,
            r#"  ret void
}}

@nz.timer_ticks = internal global i64 0

define internal void @nz_timer_arm() {{
entry:
  %ticks = load i64, ptr @nz.timer_ticks
  %known = icmp ne i64 %ticks, 0
  br i1 %known, label %set, label %compute
compute:
  %f = call i64 asm sideeffect "mrs $0, cntfrq_el0", "=r"()
  %scaled = mul i64 %f, {p}
  %t = udiv i64 %scaled, 1000000
  %one = icmp eq i64 %t, 0
  %t1 = select i1 %one, i64 1, i64 %t
  store i64 %t1, ptr @nz.timer_ticks
  br label %set
set:
  %n = load i64, ptr @nz.timer_ticks
  call void asm sideeffect "msr cntv_tval_el0, $0", "r"(i64 %n)
  call void asm sideeffect "msr cntv_ctl_el0, $0", "r"(i64 1)
  call void asm sideeffect "isb", ""()
  ret void
}}
"#
        );
    } else {
        let _ = writeln!(out, "  ret void\n}}");
    }
    let _ = write!(
        out,
        r#"
define void @nz_irq_dispatch() {{
entry:
  %iar = load volatile i32, ptr inttoptr (i64 {iar} to ptr)
  %id = and i32 %iar, 1023
  switch i32 %id, label %done ["#,
        iar = c + 0xC
    );
    for (k, h) in handlers.iter().enumerate() {
        let _ = write!(out, " i32 {}, label %h{k}", h.intid);
    }
    let _ = writeln!(out, " ]");
    for (k, h) in handlers.iter().enumerate() {
        let _ = writeln!(out, "h{k}:");
        if h.intid == TIMER_INTID && i.timer_period_us.is_some() {
            let _ = writeln!(out, "  call void @nz_timer_arm()");
        }
        let arg = if h.takes_cap { "i64 0" } else { "" };
        let _ = writeln!(out, "  %r{k} = call i64 @{}({arg})", h.symbol);
        let _ = writeln!(out, "  br label %check");
    }
    let _ = write!(
        out,
        r#"check:
  %failed = load i1, ptr @nz.failed
  br i1 %failed, label %reported, label %done
reported:
  call void @nz.report()
  unreachable
done:
  %spurious = icmp eq i32 %id, 1023
  br i1 %spurious, label %out, label %eoi
eoi:
  store volatile i32 %iar, ptr inttoptr (i64 {eoir} to ptr)
  br label %out
out:
  ret void
}}
"#,
        eoir = c + 0x10
    );
    for h in handlers {
        let param = if h.takes_cap { "i64" } else { "" };
        if !h.defined_here {
            let _ = writeln!(out, "declare i64 @{}({param})", h.symbol);
        }
    }
}

/// A Cortex-M's interrupts (Gate 3): `nz_irq_init` starts SysTick with the period's ticks of the
/// core clock — reload, current value cleared, then enabled with its interrupt and the processor's
/// clock — and enables each handled line in the NVIC's set-enable registers. Each handled vector's
/// entry, `nz_vec_K`, calls its handler and reports and stops the machine if it failed: an M
/// profile saves the caller's registers itself and returns on the link register, so the entry is
/// an ordinary function and nothing acknowledges SysTick.
fn nvic_runtime(out: &mut String, i: &Interrupts, handlers: &[Handler]) {
    let Controller::Nvic { clock_hz } = i.controller else {
        return;
    };
    // `@nz.failed` and `@nz.report` are declared by the entry this is part of.
    out.push_str("define void @nz_irq_init() {\nentry:\n");
    if let (Some(p), true) = (
        i.timer_period_us,
        handlers.iter().any(|h| h.intid == SYSTICK_VECTOR),
    ) {
        let ticks = (u128::from(clock_hz) * u128::from(p))
            .checked_div(1_000_000)
            .unwrap_or(0);
        let reload = u32::try_from(ticks.saturating_sub(1)).unwrap_or(0x00FF_FFFF);
        let _ = writeln!(
            out,
            "  store volatile i32 {reload}, ptr inttoptr (i64 3758153748 to ptr)"
        );
        out.push_str("  store volatile i32 0, ptr inttoptr (i64 3758153752 to ptr)\n");
        out.push_str("  store volatile i32 7, ptr inttoptr (i64 3758153744 to ptr)\n");
    }
    for h in handlers.iter().filter(|h| h.intid >= 16) {
        let n = u64::from(h.intid - 16);
        let _ = writeln!(
            out,
            "  store volatile i32 {}, ptr inttoptr (i64 {} to ptr)",
            1u64 << (n % 32),
            0xE000_E100 + 4 * (n >> 5)
        );
    }
    out.push_str("  ret void\n}\n");
    for h in handlers {
        let (param, arg) = if h.takes_cap {
            ("i64", "i64 0")
        } else {
            ("", "")
        };
        if !h.defined_here {
            let _ = writeln!(out, "declare i64 @{}({param})", h.symbol);
        }
        let _ = write!(
            out,
            "define void @nz_vec_{k}() {{\nentry:\n  %r = call i64 @{sym}({arg})\n  %failed = load i1, ptr @nz.failed\n  br i1 %failed, label %reported, label %done\nreported:\n  call void @nz.report()\n  unreachable\ndone:\n  ret void\n}}\n",
            sym = h.symbol,
            k = h.intid
        );
    }
}

/// The board's fixed-priority scheduler (Gate 3, `docs/spec.md`, *Boards*). `nz_sched_tick` is the
/// board timer's handler: one more tick. `nz_sched_run` loops over the tasks, highest priority
/// first, the first written first among equals: a task whose release — its next multiple of its
/// period — has come runs to completion and is released again a period on; one whose release came
/// a whole period ago without its running has missed its deadline (`N0414`); a task that answers
/// anything but 0 ends the loop with that answer; and with nothing released the core waits for an
/// interrupt. No lock exists, so none can be held across a priority. With a watchdog, the scheduler
/// arms it as it starts — `main`, the initialisation, runs unwatched — each tick counts down what
/// the last `nz.watchdog_feed` set, and the tick that reaches nothing fails the image (`N0415`).
fn scheduler(out: &mut String, tasks: &[Task], watchdog: Option<u64>) {
    let mut order: Vec<(usize, &Task)> = tasks.iter().enumerate().collect();
    order.sort_by(|(i, a), (j, b)| b.priority.cmp(&a.priority).then(i.cmp(j)));
    let n = tasks.len();
    let init: Vec<String> = tasks.iter().map(|t| format!("i64 {}", t.period)).collect();
    let _ = write!(
        out,
        "@nz.sched_ticks = internal global i64 0\n@nz.sched_next = internal global [{n} x i64] [{}]\n\
         declare void @nz.fail(ptr, i64)\n\
         define i64 @nz_sched_tick() {{\nentry:\n  %old = atomicrmw add ptr @nz.sched_ticks, i64 1 seq_cst\n",
        init.join(", ")
    );
    // The watchdog (Gate 3): the runtime's count of ticks left, which each feed sets to the reload
    // the scheduler arms as it starts; a tick counts it down while armed, and the tick that
    // reaches nothing is a failure the interrupt path reports.
    let reload = watchdog.unwrap_or(0);
    if watchdog.is_some() {
        let msg = "nazm: error[N0415]: the watchdog expired: no task fed it in time\n";
        let _ = write!(
            out,
            "  %armed = load atomic i64, ptr @nz.wd_reload seq_cst, align 8\n  \
             %on = icmp ne i64 %armed, 0\n  br i1 %on, label %count, label %fed\n\
             count:\n  %left = atomicrmw sub ptr @nz.wd_left, i64 1 seq_cst\n  \
             %bit = icmp eq i64 %left, 1\n  br i1 %bit, label %bite, label %fed\n\
             bite:\n  call void @nz.fail(ptr @.nz.wd_msg, i64 {len})\n  ret i64 0\n\
             fed:\n  ret i64 0\n}}\n\
             @nz.wd_reload = external hidden global i64\n@nz.wd_left = external hidden global i64\n\
             @.nz.wd_msg = private constant [{len} x i8] c\"{escaped}\"\n",
            len = msg.len(),
            escaped = msg.replace('\n', "\\0A"),
        );
    } else {
        out.push_str("  ret i64 0\n}\n");
    }
    for (i, t) in tasks.iter().enumerate() {
        let msg = format!(
            "nazm: error[N0414]: the task `{}` missed its deadline\n",
            t.name
        );
        let escaped = msg.replace('\n', "\\0A");
        let _ = writeln!(
            out,
            "@.nz.missed{i} = private constant [{} x i8] c\"{escaped}\"",
            msg.len()
        );
        let param = if t.takes_cap { "i64" } else { "" };
        let _ = writeln!(out, "declare i64 @{}({param})", t.symbol);
    }
    out.push_str("define i64 @nz_sched_run() {\nentry:\n");
    if reload != 0 {
        let _ = write!(
            out,
            "  store atomic i64 {reload}, ptr @nz.wd_left seq_cst, align 8\n  \
             store atomic i64 {reload}, ptr @nz.wd_reload seq_cst, align 8\n"
        );
    }
    out.push_str(
        "  br label %scan\nscan:\n  %now = load atomic i64, ptr @nz.sched_ticks seq_cst, align 8\n",
    );
    let _ = writeln!(out, "  br label %t{}", order.first().map_or(0, |(i, _)| *i));
    for (k, (i, t)) in order.iter().enumerate() {
        let next_label = order
            .get(k + 1)
            .map_or_else(|| "idle".to_owned(), |(j, _)| format!("t{j}"));
        let arg = if t.takes_cap { "i64 0" } else { "" };
        let _ = write!(
            out,
            "t{i}:\n  %p{i} = getelementptr [{n} x i64], ptr @nz.sched_next, i64 0, i64 {i}\n  \
             %r{i} = load i64, ptr %p{i}\n  %due{i} = icmp uge i64 %now, %r{i}\n  \
             br i1 %due{i}, label %c{i}, label %{next_label}\n\
             c{i}:\n  %late{i} = add i64 %r{i}, {period}\n  %missed{i} = icmp uge i64 %now, %late{i}\n  \
             br i1 %missed{i}, label %m{i}, label %go{i}\n\
             m{i}:\n  call void @nz.fail(ptr @.nz.missed{i}, i64 {len})\n  ret i64 0\n\
             go{i}:\n  store i64 %late{i}, ptr %p{i}\n  %a{i} = call i64 @{sym}({arg})\n  \
             %f{i} = load i1, ptr @nz.failed\n  br i1 %f{i}, label %out{i}, label %k{i}\n\
             out{i}:\n  ret i64 0\n\
             k{i}:\n  %done{i} = icmp ne i64 %a{i}, 0\n  br i1 %done{i}, label %end{i}, label %scan\n\
             end{i}:\n  ret i64 %a{i}\n",
            period = t.period,
            len = format!("nazm: error[N0414]: the task `{}` missed its deadline\n", t.name).len(),
            sym = t.symbol,
        );
    }
    out.push_str(
        "idle:\n  call void asm sideeffect \"wfi\", \"~{memory}\"()\n  br label %scan\n}\n",
    );
}
