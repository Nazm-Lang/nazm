#!/bin/bash
# The backend probe `nazm build` makes before it keys an object (`crates/nazm-cli/src/backend.rs`,
# `probe`): `clang -###` for compiling an empty `probe.ll` at -O0 for the host, or with
# `-target $TARGET` for another target (8.8), with only the variables the build passes to it.
# Writes the report to DIR/probe.report and prints what `pipeline backend` makes of it: the
# backend identity and configuration, as object keys carry them.
#
#     [TARGET=TRIPLE] toolchain/pipeline/tests/probe.sh PIPELINE DIR
set -u
P=$1
DIR=$(cd "$2" && pwd)
: > "$DIR/probe.ll"
pass=()
for v in PATH LD_LIBRARY_PATH TMPDIR DEVELOPER_DIR SDKROOT MACOSX_DEPLOYMENT_TARGET \
    IPHONEOS_DEPLOYMENT_TARGET TVOS_DEPLOYMENT_TARGET WATCHOS_DEPLOYMENT_TARGET XROS_DEPLOYMENT_TARGET; do
    if [ -n "${!v+x}" ]; then pass+=("$v=${!v}"); fi
done
extra=()
if [ -n "${TARGET:-}" ]; then extra=(-target "$TARGET"); fi
case "$(uname -s)" in Linux) [ -z "${TARGET:-}" ] && extra=(-ffunction-sections -fdata-sections) ;; esac
env -i ${pass[@]+"${pass[@]}"} clang -### -O0 -Wno-override-module -c ${extra[@]+"${extra[@]}"} -o "$DIR/probe.o" "$DIR/probe.ll" 2> "$DIR/probe.report"
rm -f "$DIR/probe.ll"
"$P" backend "$DIR/probe.report" "$DIR/probe.ll" "$DIR/probe.o"
