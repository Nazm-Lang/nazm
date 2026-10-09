# A clean Linux host for installing a prebuilt Nazm archive (Gate 3, docs/systems-domains.md
# Part A): Debian with a C toolchain and nothing else — no Rust, no cargo, no Nazm. The test mounts
# the archive and scripts/clean-install-test.sh and runs it with no network.
#
#   docker build [--platform linux/amd64] -f docker/clean-install.Dockerfile -t nazm-clean-install:<arch> docker
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends clang \
 && rm -rf /var/lib/apt/lists/*
RUN ! command -v cargo && ! command -v rustc
WORKDIR /work
