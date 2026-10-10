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
}

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
}

impl Uart {
    /// Its name in a manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pl011 => "pl011",
            Self::Ns16550a => "ns16550a",
        }
    }
}

/// A board's interrupts (Gate 3, `docs/spec.md`, *Boards*): a GICv2 and what it delivers. AArch64
/// only in 1.0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interrupts {
    /// The GICv2 distributor's registers.
    pub distributor: u64,
    /// The GICv2 CPU interface's registers.
    pub cpu: u64,
    /// The board timer's period, when the program wants it: its interrupt is named `timer`.
    pub timer_period_us: Option<u64>,
    /// Every further interrupt by name, with the number (`INTID`) it is.
    pub lines: Vec<(String, u32)>,
}

/// The AArch64 virtual timer's interrupt on a GICv2: private peripheral interrupt 11, `INTID` 27.
pub const TIMER_INTID: u32 = 27;

/// One interrupt handler the entry dispatches to (Gate 3): its `INTID`, the Nazm function's
/// symbol, and whether it takes the device's authority (an `MmioCap`, a word that is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    pub intid: u32,
    pub symbol: String,
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
    /// Its interrupts (Gate 3), or none: with none, interrupts stay masked.
    pub interrupts: Option<Interrupts>,
    /// How it is booted: the emulator command line, the image last.
    pub emulator: Cow<'static, str>,
    /// Whether it has been run on hardware. No board has.
    pub hardware: bool,
}

impl Board {
    /// The board as a manifest (Gate 3): the text `nazm build --board` reads back as this board.
    #[must_use]
    pub fn manifest(&self) -> String {
        let isa = match self.isa {
            Isa::Aarch64 => "aarch64",
            Isa::Riscv64 => "riscv64",
        };
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
        if let Some(h) = self.heap {
            let _ = write!(out, "\n[heap]\nsize = {h:#x}\n");
        }
        if let Some(i) = &self.interrupts {
            let _ = write!(
                out,
                "\n[interrupts]\ncontroller = \"gicv2\"\ndistributor = {:#x}\ncpu = {:#x}\n",
                i.distributor, i.cpu
            );
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
pub const BOARDS: [Board; 2] = [
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
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel",
        ),
        hardware: false,
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
        interrupts: None,
        emulator: Cow::Borrowed(
            "qemu-system-riscv64 -M virt -bios none -nographic -nic none -kernel",
        ),
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
    let triple = &board.triple;
    let load = board.load_address;
    let stack = board.stack;
    let keep =
        |names: &[String]| -> String { names.iter().map(|n| format!(" KEEP(*({n}))")).collect() };
    let (code, rodata, data) = (keep(&placed.code), keep(&placed.rodata), keep(&placed.data));
    // A heap (Gate 3): the arena after the stack, its bounds `__heap_start` and `__heap_end`.
    let heap = board.heap.map_or_else(String::new, |h| {
        format!("  . = ALIGN(16);\n  __heap_start = .;\n  . = . + {h:#x};\n  __heap_end = .;\n")
    });
    format!(
        "/* Written by nazm build for {triple} (N62, N86). */\nENTRY(_start)\nSECTIONS\n{{\n  \
         . = {load:#x};\n  .text : {{ *(.text.start) *(.text*){code} }}\n  .rodata : {{ *(.rodata*) *(.srodata*){rodata} }}\n  \
         .data : {{ *(.data*) *(.sdata*){data} }}\n  .bss : {{ *(.bss*) *(.sbss*) *(COMMON) }}\n  . = ALIGN(16);\n  __stack_bottom = .;\n  . = . + {stack:#x};\n  \
         __stack_top = .;\n{heap}  /DISCARD/ : {{ *(.eh_frame*) }}\n}}\n"
    )
}

/// [`emit_runtime`] for a program that reaches `needs` (Gate 3): with a heap in the board's
/// manifest, its arena and the runtime parts allocation admits.
#[must_use]
pub fn emit_runtime_for(board: &Board, needs: &crate::Needs) -> String {
    let mut out = emit_runtime(board, needs.stack);
    if board.heap.is_some() {
        out.push_str(HEAP_RUNTIME);
        out.push_str(&crate::core::board_heap_parts(needs));
    }
    out
}

/// A board's arena (Gate 3, `docs/spec.md`, *Boards*): a bump allocator over the linker
/// script's `__heap_start` to `__heap_end`, never returned to — `free` does nothing. Each block
/// is 16-aligned behind a 16-byte header holding its size, so `realloc` copies what there was.
/// The bump is a compare-and-swap, so an interrupt handler allocating while `main` does is safe.
/// Exhausted, `malloc` answers null, which every caller reports as `N0406`. And the C library's
/// four memory functions, as byte loops LLVM may not turn back into calls to themselves.
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
    if stack {
        // The quarter of the board's stack kept in reserve below a recursive call (N49's).
        let headroom = board.stack >> 2;
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
  %limit = add i64 %bottom, {headroom}
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
    emit_entry_with(board, entry, roots, watermark, &[], None)
}

/// [`emit_entry`], with the board's interrupts taken by `handlers` (Gate 3): the vector table,
/// the GICv2 and the timer set up before `main`, and every interrupt dispatched to its handler.
#[must_use]
pub fn emit_entry_with(
    board: &Board,
    entry: &str,
    roots: usize,
    watermark: bool,
    handlers: &[Handler],
    on_failure: Option<(&str, bool)>,
) -> String {
    let irq = board
        .interrupts
        .as_ref()
        .filter(|_| board.isa == Isa::Aarch64);
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
    } else {
        &[]
    };
    for line in [
        ".section .text.start, \\22ax\\22",
        ".global _start",
        "_start:",
    ]
    .iter()
    .chain(start)
    .chain(if watermark { paint } else { &[] })
    .chain(arm)
    .chain(call)
    {
        let _ = writeln!(out, "module asm \"{line}\"");
    }
    if let Some(i) = irq {
        vectors(&mut out);
        irq_runtime(&mut out, i, handlers);
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
    let (d, c) = (i.distributor, i.cpu);
    let _ = write!(
        out,
        r#"
@.nz.unexpected = private constant [48 x i8] c"nazm: an exception the image has no handler for\0A"
declare void @nz.uart_write(ptr, i64)

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
        let _ = writeln!(out, "declare i64 @{}({param})", h.symbol);
    }
}
