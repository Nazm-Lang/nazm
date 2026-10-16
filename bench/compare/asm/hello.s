// Hand-written aarch64 assembly for macOS: the ceiling a compiler is measured against.
// One write(2) and exit through libSystem (macOS has no stable raw system calls).
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x0, #1
        adrp    x1, msg@PAGE
        add     x1, x1, msg@PAGEOFF
        mov     x2, #14
        bl      _write
        mov     w0, #0
        ldp     x29, x30, [sp], #16
        ret

        .section __TEXT,__cstring
msg:    .ascii  "Hello, world!\n"
