#!/bin/sh
# Run one Nazm-built program as the R6 rule lets it run (`toolchain/lib/stage.sh` chooses the mode):
#
#     stage-run.sh DEADLINE-SECONDS EXECUTABLE [ARGS...]
#
# Standard input is empty (contained: the file `$STAGE_STDIN` names, for the commands that read it; a host-run has none). Standard output, standard error and the exit status are the program's:
# contained, it is run directly under the deadline (SIGALRM, status 142); on a macOS host it is run
# by `cargo xtask host-run`, whose report goes to `$STAGE_PEAKS`, never to a standard stream, and
# whose own statuses are 124 (the deadline), 126 (output past its cap) and 70 (the rule withdrawn).
# `nzx host-run` (`toolchain/harness/build.sh` builds it into `target/nzx/`, or `$NZX`) is preferred when it exists,
# `cargo xtask host-run` otherwise, until slice 19 removes the second (R8 slice 13, S13.2).
if [ $# -lt 2 ]; then
    echo "stage-run: usage: stage-run.sh DEADLINE EXECUTABLE [ARGS...]" >&2
    exit 2
fi
deadline=$1
shift
case ${STAGE_MODE:-} in
    contained) exec perl -e 'alarm shift; exec @ARGV or exit 127' "$deadline" "$@" < "${STAGE_STDIN:-/dev/null}" ;;
    host)
        nzx=${NZX:-$(cd "$(dirname "$0")/../.." && pwd)/target/nzx/nzx}
        if [ -x "$nzx" ]; then
            exec "$nzx" host-run --deadline-secs "$deadline" --report "${STAGE_PEAKS:-/dev/null}" --max-output-mib "${STAGE_MAX_OUTPUT_MIB:-192}" -- "$@"
        fi
        exec "$STAGE_XTASK" host-run --deadline-secs "$deadline" --report "${STAGE_PEAKS:-/dev/null}" --max-output-mib "${STAGE_MAX_OUTPUT_MIB:-192}" -- "$@" ;;
    *)
        echo "stage-run: STAGE_MODE is not set: source toolchain/lib/stage.sh and call stage_init" >&2
        exit 2
        ;;
esac
