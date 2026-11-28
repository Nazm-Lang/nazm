# A clean Linux host for installing a prebuilt Nazm archive (Gate 3, docs/internal/systems-domains.md
# Part A): Debian with a C toolchain and nothing else — no Rust, no cargo, no Nazm. The test mounts
# the archive and scripts/clean-install-test.sh and runs it with no network
# (`scripts/clean-install-test.sh --container ARCHIVE` builds this image and runs it).
# What it installs is exactly what a Linux archive's INSTALL.txt asks for (linux-toolchain L4):
# clang and the C library's headers, libc6-dev.
#
#   docker build [--platform linux/amd64] -f docker/clean-install.Dockerfile -t nazm-clean-install:<arch> docker
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends clang libc6-dev \
 && rm -rf /var/lib/apt/lists/*
RUN ! command -v cargo && ! command -v rustc
WORKDIR /work
