# The environment the contained runner executes workloads in.
#
# Why a container at all: macOS cannot impose a hard memory ceiling on a child.
# `RLIMIT_AS` is an alias of `RLIMIT_RSS` there, and `setrlimit(2)` documents it as
# advisory — the system *prefers* to reclaim from a process that exceeds its declared
# set rather than refusing the allocation. On 2026-09-21 a stage that allocated without
# bound therefore reached the machine instead of the process, three times. A cgroup
# refuses the allocation, so the failure stays inside the workload.
#
# The Rust version tracks `rust-toolchain.toml` exactly. A floating tag would reintroduce
# the drift that pin exists to prevent.
#
# Debian trixie carries clang 19. clang 17 rejects at least one module this compiler
# emits (`review/m1e/control.ll:44`: "instruction expected to be numbered '%3'"), which is
# an open defect tracked separately — pinning a version known to accept the IR keeps that
# defect from masquerading as a bootstrap failure.
FROM rust:1.98.1-slim-trixie

# `procps` supplies `ps`, which `xtask`'s mutation lock uses to tell "no such process"
# apart from "exists but is not yours" — `kill -0` fails for both and offers no errno to
# separate them, so a live lock would look dead and be stolen. Without `ps` the probe
# correctly reports Unknown and four tests fail; the slim image simply does not ship it.
# `libclang-rt-19-dev` is libFuzzer's archive (`-fsanitize=fuzzer`) and SanitizerCoverage's runtime: what
# `nazm-fuzz` links the instrumented Nazm targets with (R8 slice 13, S13.5). Debian's package, nothing bundled.
RUN apt-get update \
 && apt-get install -y --no-install-recommends clang libclang-rt-19-dev ca-certificates procps time \
 && rm -rf /var/lib/apt/lists/*

# Cargo writes here, never into the read-only source mount. The host's `target/` holds
# aarch64-apple-darwin artefacts and must not be mixed with Linux ones.
ENV CARGO_TARGET_DIR=/work/target
# On the writable layer, which `--storage-opt size` caps.
RUN mkdir -p /work/target /work/cargo /work/bootstrap
WORKDIR /src

# Warm the toolchain components and the build cache into the image.
#
# The copy lands at /src, the same path the live source is later mounted at. Cargo's
# fingerprints record absolute paths, so warming at any other path would invalidate every
# one of them and the cache would be dead weight. At run time the read-only bind mount
# covers this copy; only what actually changed is rebuilt.
#
# Without this the registry and the three rust-toolchain components are fetched on every
# run, inside the deadline, over the network.
COPY . /src
RUN cargo fetch --locked \
 && cargo build --locked --tests --jobs 1 \
 && cargo build --locked -p xtask --jobs 1

# `ssh-keygen` signs and checks the in-toto attestations (`nazm attest`), so the attestation tests and
# their mutation killers run here instead of being ignored (N94, killer repoint 2026-11-08). Debian's
# package, nothing bundled; its own layer, so the warm build above stays cached. `python3` writes the
# reproducible release archive (`scripts/reproducible-tar.py`), so the packaging tests run here too.
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-client python3 \
 && rm -rf /var/lib/apt/lists/*
