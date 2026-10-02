//! The freestanding runtime and entry (N62, `docs/architecture.md` §7.64): `aarch64-unknown-none`
//! on QEMU's `virt` board — no OS, no C library, no heap, one thread.
//!
//! A failure is recorded in plain globals and reported on the board's PL011 UART; the machine is
//! stopped by ARM semihosting's `SYS_EXIT_EXTENDED`. `_start` sets the stack the linker script
//! reserves, calls `main` and prints its result. Nothing runs before `main` but that.

use std::fmt::Write as _;

use crate::preamble;

/// The PL011 UART's data register on QEMU's `virt` machine.
pub const UART: u64 = 0x0900_0000;

/// The board's one stack, in bytes: 64 KiB.
pub const STACK: u64 = 0x1_0000;

/// What `nz.stack_check` keeps in reserve below a recursive call (N49's quarter): 16 KiB.
pub const STACK_HEADROOM: u64 = STACK >> 2;

/// Where QEMU's `virt` machine loads a kernel image, and so where `link.ld` places it.
pub const LOAD_ADDRESS: u64 = 0x4008_0000;

/// The linker script a freestanding image is linked with (`ld.lld -T link.ld`): `_start` first at
/// the load address, then text, read-only data, data and zeroed data, then a 64 KiB stack whose
/// bounds are `__stack_bottom` and `__stack_top`.
#[must_use]
pub fn linker_script() -> String {
    format!(
        "/* Written by nazm build for aarch64-unknown-none (N62). */\nENTRY(_start)\nSECTIONS\n{{\n  \
         . = {LOAD_ADDRESS:#x};\n  .text : {{ *(.text.start) *(.text*) }}\n  .rodata : {{ *(.rodata*) }}\n  \
         .data : {{ *(.data*) }}\n  .bss : {{ *(.bss*) *(COMMON) }}\n  . = ALIGN(16);\n  __stack_bottom = .;\n  . = . + {STACK:#x};\n  \
         __stack_top = .;\n}}\n"
    )
}

/// The freestanding runtime: the failure state and its helpers, the UART, the semihosting exit,
/// the MMIO helpers and — where a function in a call cycle checks it — the stack check, against
/// the linker script's `__stack_bottom` rather than an operating system's limit.
#[must_use]
pub fn emit_runtime(stack: bool) -> String {
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

define hidden i32 @nz.failed_now() {{
entry:
  %f = load i1, ptr @nz.failed
  %r = zext i1 %f to i32
  ret i32 %r
}}

define hidden void @nz.uart_byte(i8 %c) {{
entry:
  store volatile i8 %c, ptr inttoptr (i64 {UART} to ptr)
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

; ARM semihosting SYS_EXIT_EXTENDED (0x20): {{ ADP_Stopped_ApplicationExit, code }}.
define hidden void @nz.board_exit(i64 %code) noreturn {{
entry:
  %blk = alloca [2 x i64], align 8
  store i64 131110, ptr %blk
  %c = getelementptr i64, ptr %blk, i64 1
  store i64 %code, ptr %c
  call void asm sideeffect "mov x0, #0x20\0Amov x1, $0\0Ahlt #0xf000", "r,~{{x0}},~{{x1}},~{{memory}}"(ptr %blk)
  br label %hang
hang:
  br label %hang
}}

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

/// The two device-register built-ins (N62): a volatile 32-bit load and store at an address. The
/// hosted runtime defines them too, unreachable there — a hosted build refuses a program using
/// them (`N0392`) — so one inventory describes both.
pub const MMIO_RUNTIME: &str = r"
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
pub fn emit_entry(entry: &str, roots: usize, watermark: bool) -> String {
    let mut out = String::new();
    preamble(&mut out, "<nazm freestanding entry>");
    for line in [
        "module asm \".section .text.start, \\22ax\\22\"",
        "module asm \".global _start\"",
        "module asm \"_start:\"",
        "module asm \"  ldr x0, =__stack_top\"",
        // Floating point and SIMD are trapped at reset; code generation may use them.
        "module asm \"  mov x1, #(3 << 20)\"",
        "module asm \"  msr cpacr_el1, x1\"",
        "module asm \"  isb\"",
        "module asm \"  mov sp, x0\"",
    ] {
        let _ = writeln!(out, "{line}");
    }
    if watermark {
        for line in [
            "module asm \"  ldr x0, =__stack_bottom\"",
            "module asm \"  ldr x1, =__stack_top\"",
            "module asm \"  ldr x2, =0x5a5a5a5a5a5a5a5a\"",
            "module asm \"2: cmp x0, x1\"",
            "module asm \"  b.hs 3f\"",
            "module asm \"  str x2, [x0], #8\"",
            "module asm \"  b 2b\"",
            "module asm \"3:\"",
        ] {
            let _ = writeln!(out, "{line}");
        }
    }
    for line in [
        "module asm \"  bl nz_board_main\"",
        "module asm \"1: b 1b\"",
    ] {
        let _ = writeln!(out, "{line}");
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
