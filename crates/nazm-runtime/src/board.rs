//! The freestanding runtime and entry (N62, `docs/architecture.md` §7.64), for each board of the
//! table (N86, §7.87): no OS, no C library, no heap, one thread.
//!
//! A board is a description — where its image loads, its UART, how its machine is stopped, its
//! architecture's startup — and the linker script, the runtime and the entry are generated from one
//! and from nothing else. A failure is recorded in plain globals and reported on the board's UART,
//! and the machine stopped with a status. `_start` sets the stack the linker script reserves,
//! calls `main` and prints its result. Nothing runs before `main` but that.

use std::fmt::Write as _;

use crate::preamble;

/// A board's instruction set, which decides its startup and how its machine is stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isa {
    Aarch64,
    Riscv64,
}

/// How a board's machine is stopped with a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// ARM semihosting's `SYS_EXIT_EXTENDED`, which QEMU honours with `-semihosting`.
    Semihosting,
    /// SiFive's test device: `0x5555` for success, `code << 16 | 0x3333` for a failure.
    SifiveTest { address: u64 },
}

/// One board: everything about it that is not the language's (N86).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board {
    /// The target triple a build names it by.
    pub triple: &'static str,
    /// What it is, for a person.
    pub name: &'static str,
    pub isa: Isa,
    /// Where the emulator loads the image, and so where `link.ld` places `_start`.
    pub load_address: u64,
    /// The UART's data register, written a byte at a time.
    pub uart: u64,
    pub stop: Stop,
    /// How it is booted: the emulator command line, the image last.
    pub emulator: &'static str,
    /// Whether it has been run on hardware. No board has.
    pub hardware: bool,
}

/// Every board, in a fixed order.
pub const BOARDS: [Board; 2] = [
    Board {
        triple: "aarch64-unknown-none",
        name: "QEMU virt, AArch64 (Cortex-A53)",
        isa: Isa::Aarch64,
        load_address: 0x4008_0000,
        uart: 0x0900_0000,
        stop: Stop::Semihosting,
        emulator:
            "qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel",
        hardware: false,
    },
    Board {
        triple: "riscv64gc-unknown-none-elf",
        name: "QEMU virt, RISC-V RV64GC, machine mode",
        isa: Isa::Riscv64,
        load_address: 0x8000_0000,
        uart: 0x1000_0000,
        stop: Stop::SifiveTest { address: 0x10_0000 },
        emulator: "qemu-system-riscv64 -M virt -bios none -nographic -nic none -kernel",
        hardware: false,
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
    let triple = board.triple;
    let load = board.load_address;
    let keep =
        |names: &[String]| -> String { names.iter().map(|n| format!(" KEEP(*({n}))")).collect() };
    let (code, rodata, data) = (keep(&placed.code), keep(&placed.rodata), keep(&placed.data));
    format!(
        "/* Written by nazm build for {triple} (N62, N86). */\nENTRY(_start)\nSECTIONS\n{{\n  \
         . = {load:#x};\n  .text : {{ *(.text.start) *(.text*){code} }}\n  .rodata : {{ *(.rodata*) *(.srodata*){rodata} }}\n  \
         .data : {{ *(.data*) *(.sdata*){data} }}\n  .bss : {{ *(.bss*) *(.sbss*) *(COMMON) }}\n  . = ALIGN(16);\n  __stack_bottom = .;\n  . = . + {STACK:#x};\n  \
         __stack_top = .;\n  /DISCARD/ : {{ *(.eh_frame*) }}\n}}\n"
    )
}

/// The freestanding runtime: the failure state and its helpers, the UART, the semihosting exit,
/// the MMIO helpers and — where a function in a call cycle checks it — the stack check, against
/// the linker script's `__stack_bottom` rather than an operating system's limit.
#[must_use]
pub fn emit_runtime(board: &Board, stack: bool) -> String {
    let uart = board.uart;
    let exit = board_exit(board);
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
define hidden void @nz.report() noreturn {{
entry:
  %m = load ptr, ptr @nz.errmsg
  %n = load i64, ptr @nz.errlen
  call void @nz.uart_write(ptr %m, i64 %n)
  call void @nz.board_exit(i64 2)
  unreachable
}}
"#
    ));
    out.push_str(MMIO_RUNTIME);
    out.push_str(ATOMIC_RUNTIME);
    if stack {
        let _ = write!(
            out,
            r"
@__stack_bottom = external global i8
declare ptr @llvm.frameaddress.p0(i32)

define hidden i32 @nz.stack_check() {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %bottom = ptrtoint ptr @__stack_bottom to i64
  %limit = add i64 %bottom, {STACK_HEADROOM}
  %past = icmp ult i64 %sp, %limit
  %r = zext i1 %past to i32
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

/// The device-register built-ins (N62, N86): a volatile 32-bit load and store at an address. The
/// hosted runtime defines them too, unreachable there — a hosted build refuses a program using
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
";

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
    let mut out = String::new();
    preamble(&mut out, "<nazm freestanding entry>");
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
    };
    for line in [
        ".section .text.start, \\22ax\\22",
        ".global _start",
        "_start:",
    ]
    .iter()
    .chain(start)
    .chain(if watermark { paint } else { &[] })
    .chain(call)
    {
        let _ = writeln!(out, "module asm \"{line}\"");
    }
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    let _ = writeln!(out, "@nz.failed = external global i1");
    let _ = writeln!(out, "declare void @nz.report() noreturn");
    let _ = writeln!(out, "declare void @nz.uart_int(i64)");
    let _ = writeln!(out, "declare void @nz.uart_byte(i8)");
    let _ = writeln!(out, "declare void @nz.board_exit(i64) noreturn");
    let _ = writeln!(out, "define void @nz_board_main() {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
    let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
    let _ = writeln!(out, "  br i1 %failed, label %reported, label %finished");
    let _ = writeln!(out, "reported:");
    let _ = writeln!(out, "  call void @nz.report()");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "finished:");
    let _ = writeln!(out, "  call void @nz.uart_int(i64 %result)");
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
declare void @nz.uart_write(ptr, i64)

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
