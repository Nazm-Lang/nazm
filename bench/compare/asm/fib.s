// Hand-written aarch64 assembly for macOS: recursive fib(35), the same two calls a level as every
// other row (no memoisation, no loop), printed in decimal with one write(2) through libSystem.
        .globl  _main
        .p2align 2
fib:                                    // x0 = n -> x0 = fib(n)
        cmp     x0, #2
        b.lt    1f
        stp     x29, x30, [sp, #-32]!
        stp     x19, x20, [sp, #16]
        mov     x19, x0
        sub     x0, x0, #1
        bl      fib
        mov     x20, x0
        sub     x0, x19, #2
        bl      fib
        add     x0, x0, x20
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #32
1:      ret

_main:
        stp     x29, x30, [sp, #-48]!
        mov     x0, #35
        bl      fib
        add     x1, sp, #47             // digits right to left, then a newline
        mov     w2, #10
        strb    w2, [x1]
        mov     x3, #10
2:      udiv    x4, x0, x3
        msub    x5, x4, x3, x0
        add     w5, w5, #'0'
        sub     x1, x1, #1
        strb    w5, [x1]
        mov     x0, x4
        cbnz    x0, 2b
        add     x2, sp, #48
        sub     x2, x2, x1
        mov     x0, #1
        bl      _write
        mov     w0, #0
        ldp     x29, x30, [sp], #48
        ret
