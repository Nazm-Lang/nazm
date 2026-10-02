# The QEMU image for N62's freestanding target: `docker build -t nazm-qemu:n62 -f docker/qemu.Dockerfile docker`.
# ld.lld links a board image from `nazm build --target aarch64-unknown-none --objects DIR`; qemu-system-aarch64 boots it.
FROM nazm-contained:1.98.1
RUN apt-get update && apt-get install -y --no-install-recommends qemu-system-arm lld && rm -rf /var/lib/apt/lists/*
