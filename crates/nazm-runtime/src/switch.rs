//! The task pool's context switch, per target (N83, `docs/architecture.md` §7.84).
//!
//! Three routines, written once per architecture and object format as module-level assembly in an
//! LLVM unit of their own, so the rest of the runtime stays target-independent text:
//!
//! - `nz_ctx_switch(save: *mut *mut u8, load: *mut u8)` saves every callee-saved register on the
//!   current stack, stores the stack pointer through `save`, switches to `load`, restores the
//!   registers saved there and returns into whatever that stack was doing.
//! - `nz_ctx_prepare(top: *mut u8, task: *mut u8) -> *mut u8` lays out the first frame of a fresh
//!   stack whose 16-byte-aligned end is `top`, so that the first switch to it "returns" into the
//!   entry stub with `task` in a callee-saved register; it returns the stack pointer to switch to.
//! - The entry stub calls `nz_task_main(task)`, which never returns.
//!
//! Callee-saved means the platform C ABI's: on AArch64 `x19`–`x28`, the frame pointer, the link
//! register and `d8`–`d15`; on x86-64 System V `rbx`, `rbp`, `r12`–`r15`, and the SSE and x87
//! control words. Darwin and ELF differ only in a symbol's leading underscore and the directive
//! that keeps it inside the executable.

/// Which switch to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchTarget {
    Aarch64Darwin,
    Aarch64Linux,
    X86_64Darwin,
    X86_64Linux,
    /// Windows x64 (Gate 3): its own calling convention and the thread information block.
    X86_64Windows,
}

impl SwitchTarget {
    /// Every target the pool has a switch for.
    pub const ALL: [Self; 5] = [
        Self::Aarch64Darwin,
        Self::Aarch64Linux,
        Self::X86_64Darwin,
        Self::X86_64Linux,
        Self::X86_64Windows,
    ];
}

const AARCH64: &str = r"
.text
.p2align 2
{GLOBAL} {P}nz_ctx_switch
{HIDDEN} {P}nz_ctx_switch
{TYPE_SWITCH}
{P}nz_ctx_switch:
  sub sp, sp, #176
  stp x19, x20, [sp, #0]
  stp x21, x22, [sp, #16]
  stp x23, x24, [sp, #32]
  stp x25, x26, [sp, #48]
  stp x27, x28, [sp, #64]
  stp x29, x30, [sp, #80]
  stp d8, d9, [sp, #96]
  stp d10, d11, [sp, #112]
  stp d12, d13, [sp, #128]
  stp d14, d15, [sp, #144]
  mov x9, sp
  str x9, [x0]
  mov sp, x1
  ldp x19, x20, [sp, #0]
  ldp x21, x22, [sp, #16]
  ldp x23, x24, [sp, #32]
  ldp x25, x26, [sp, #48]
  ldp x27, x28, [sp, #64]
  ldp x29, x30, [sp, #80]
  ldp d8, d9, [sp, #96]
  ldp d10, d11, [sp, #112]
  ldp d12, d13, [sp, #128]
  ldp d14, d15, [sp, #144]
  add sp, sp, #176
  ret

.p2align 2
{GLOBAL} {P}nz_ctx_prepare
{HIDDEN} {P}nz_ctx_prepare
{TYPE_PREPARE}
{P}nz_ctx_prepare:
  sub x0, x0, #176
  mov x9, #0
1:
  str xzr, [x0, x9]
  add x9, x9, #8
  cmp x9, #176
  b.lt 1b
  str x1, [x0, #0]
  adr x9, {L}nz_task_entry
  str x9, [x0, #88]
  ret

.p2align 2
{L}nz_task_entry:
  mov x0, x19
  mov x29, #0
  bl {P}nz_task_main
  brk #1
";

const X86_64: &str = r"
.text
.p2align 4
{GLOBAL} {P}nz_ctx_switch
{HIDDEN} {P}nz_ctx_switch
{TYPE_SWITCH}
{P}nz_ctx_switch:
  pushq %rbp
  pushq %rbx
  pushq %r12
  pushq %r13
  pushq %r14
  pushq %r15
  subq $8, %rsp
  stmxcsr (%rsp)
  fnstcw 4(%rsp)
  movq %rsp, (%rdi)
  movq %rsi, %rsp
  ldmxcsr (%rsp)
  fldcw 4(%rsp)
  addq $8, %rsp
  popq %r15
  popq %r14
  popq %r13
  popq %r12
  popq %rbx
  popq %rbp
  retq

.p2align 4
{GLOBAL} {P}nz_ctx_prepare
{HIDDEN} {P}nz_ctx_prepare
{TYPE_PREPARE}
{P}nz_ctx_prepare:
  leaq -80(%rdi), %rax
  movl $0x1F80, (%rax)
  movw $0x037F, 4(%rax)
  movq $0, 8(%rax)
  movq $0, 16(%rax)
  movq $0, 24(%rax)
  movq $0, 32(%rax)
  movq %rsi, 40(%rax)
  movq $0, 48(%rax)
  leaq {L}nz_task_entry(%rip), %rcx
  movq %rcx, 56(%rax)
  retq

.p2align 4
{L}nz_task_entry:
  movq %rbx, %rdi
  xorl %ebp, %ebp
  callq {P}nz_task_main
  ud2
";

/// The Windows x64 switch (Gate 3). The convention's callee-saved registers are `rbx`, `rbp`,
/// `rdi`, `rsi`, `r12`–`r15` and `xmm6`–`xmm15`, with the MXCSR and x87 control words; the two
/// arguments arrive in `rcx` and `rdx`. The thread information block's stack base (`gs:0x08`) and
/// limit (`gs:0x10`) are saved and restored with them, so the system's exception dispatch always
/// sees the stack it is on: a fresh task's base is its top and its limit 0, which every frame of it
/// lies within. Frame, from the saved stack pointer: `xmm6`–`xmm15` (160 bytes), MXCSR, x87 word,
/// the stack limit, the stack base, `r15` … `rbp`, the return address — 256 bytes in all.
const X86_64_WINDOWS: &str = r"
.text
.p2align 4
.globl nz_ctx_switch
nz_ctx_switch:
  pushq %rbp
  pushq %rbx
  pushq %rdi
  pushq %rsi
  pushq %r12
  pushq %r13
  pushq %r14
  pushq %r15
  movq %gs:8, %rax
  pushq %rax
  movq %gs:16, %rax
  pushq %rax
  subq $168, %rsp
  movdqu %xmm6, 0(%rsp)
  movdqu %xmm7, 16(%rsp)
  movdqu %xmm8, 32(%rsp)
  movdqu %xmm9, 48(%rsp)
  movdqu %xmm10, 64(%rsp)
  movdqu %xmm11, 80(%rsp)
  movdqu %xmm12, 96(%rsp)
  movdqu %xmm13, 112(%rsp)
  movdqu %xmm14, 128(%rsp)
  movdqu %xmm15, 144(%rsp)
  stmxcsr 160(%rsp)
  fnstcw 164(%rsp)
  movq %rsp, (%rcx)
  movq %rdx, %rsp
  ldmxcsr 160(%rsp)
  fldcw 164(%rsp)
  movdqu 0(%rsp), %xmm6
  movdqu 16(%rsp), %xmm7
  movdqu 32(%rsp), %xmm8
  movdqu 48(%rsp), %xmm9
  movdqu 64(%rsp), %xmm10
  movdqu 80(%rsp), %xmm11
  movdqu 96(%rsp), %xmm12
  movdqu 112(%rsp), %xmm13
  movdqu 128(%rsp), %xmm14
  movdqu 144(%rsp), %xmm15
  addq $168, %rsp
  popq %rax
  movq %rax, %gs:16
  popq %rax
  movq %rax, %gs:8
  popq %r15
  popq %r14
  popq %r13
  popq %r12
  popq %rsi
  popq %rdi
  popq %rbx
  popq %rbp
  retq

.p2align 4
.globl nz_ctx_prepare
nz_ctx_prepare:
  leaq -256(%rcx), %rax
  xorl %r8d, %r8d
1:
  movq $0, (%rax,%r8)
  addq $8, %r8
  cmpq $256, %r8
  jb 1b
  movl $0x1F80, 160(%rax)
  movw $0x037F, 164(%rax)
  movq %rcx, 176(%rax)
  movq %rdx, 232(%rax)
  leaq .Lnz_task_entry(%rip), %r8
  movq %r8, 248(%rax)
  retq

.p2align 4
.Lnz_task_entry:
  movq %rbx, %rcx
  xorl %ebp, %ebp
  subq $32, %rsp
  callq nz_task_main
  ud2
";

/// The switch for `target`, as an LLVM unit holding only module-level assembly.
#[must_use]
pub fn emit_switch(target: SwitchTarget) -> String {
    if target == SwitchTarget::X86_64Windows {
        let mut out = String::new();
        super::preamble(&mut out, "<nazm task switch>");
        for line in X86_64_WINDOWS.lines() {
            out.push_str("module asm \"");
            out.push_str(&super::escape(line));
            out.push_str("\"\n");
        }
        return out;
    }
    let (body, darwin) = match target {
        SwitchTarget::Aarch64Darwin => (AARCH64, true),
        SwitchTarget::Aarch64Linux => (AARCH64, false),
        SwitchTarget::X86_64Darwin => (X86_64, true),
        SwitchTarget::X86_64Linux => (X86_64, false),
        // Returned above, with its own text; never reaches here.
        SwitchTarget::X86_64Windows => (X86_64, false),
    };
    let elf_type = |name: &str| -> String {
        if darwin {
            String::new()
        } else if body == AARCH64 {
            format!(".type {name},%function")
        } else {
            format!(".type {name},@function")
        }
    };
    let asm = body
        .replace("{GLOBAL}", ".globl")
        .replace(
            "{HIDDEN}",
            if darwin { ".private_extern" } else { ".hidden" },
        )
        .replace("{TYPE_SWITCH}", &elf_type("nz_ctx_switch"))
        .replace("{TYPE_PREPARE}", &elf_type("nz_ctx_prepare"))
        .replace("{L}", if darwin { "L" } else { ".L" })
        .replace("{P}", if darwin { "_" } else { "" });
    let mut out = String::new();
    super::preamble(&mut out, "<nazm task switch>");
    for line in asm.lines() {
        out.push_str("module asm \"");
        out.push_str(&super::escape(line));
        out.push_str("\"\n");
    }
    out
}

/// The location of `errno` on `target` (N85, `docs/architecture.md` §7.86): `nz_errno_location`,
/// the one target-dependent fact the runtime's `errno` capture needs, in a unit of its own as the
/// switch is, so the runtime text stays the same on every target.
#[must_use]
pub fn emit_errno(target: SwitchTarget) -> String {
    let location = match target {
        SwitchTarget::Aarch64Darwin | SwitchTarget::X86_64Darwin => "__error",
        SwitchTarget::Aarch64Linux | SwitchTarget::X86_64Linux => "__errno_location",
        // MinGW-w64's C runtime (Gate 3).
        SwitchTarget::X86_64Windows => "_errno",
    };
    format!(
        "; Generated by nazm build. Do not edit.\n\
         source_filename = \"errno\"\n\n\
         declare ptr @{location}()\n\n\
         define hidden ptr @nz_errno_location() {{\n\
         entry:\n  %p = call ptr @{location}()\n  ret ptr %p\n}}\n"
    )
}
