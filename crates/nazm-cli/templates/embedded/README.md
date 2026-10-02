# {{name}}

A freestanding program for `aarch64-unknown-none` (QEMU's `virt` board): no heap, no C library.
`nazm check .`; `nazm test tests` checks the pure part on this machine; `nazm build --target
aarch64-unknown-none . --objects build` writes the objects, `link.ld` and `link.txt`, and
`bounds.json` (the stack bound). Run under `qemu-system-aarch64 -M virt -cpu cortex-a53
-semihosting`.
