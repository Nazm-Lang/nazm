# The QEMU image for the freestanding targets: `docker build -t nazm-qemu:n86 -f docker/qemu.Dockerfile docker`.
# N62: ld.lld links a board image from `nazm build --target aarch64-unknown-none --objects DIR`;
# qemu-system-aarch64 boots it. N86: qemu-system-riscv64 (in qemu-system-misc) boots the RISC-V
# board's, and this image's clang is the one that compiles RISC-V (the macOS host's has no such
# code generator).
FROM nazm-contained:1.98.1
RUN apt-get update && apt-get install -y --no-install-recommends qemu-system-arm qemu-system-misc lld && rm -rf /var/lib/apt/lists/*
