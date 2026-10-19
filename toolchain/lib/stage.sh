# Sourced by every script under toolchain/ that builds a program written in Nazm and runs it
# (R8 slice 2; `docs/runbook.md`, *What may run on the host*, and *Mandatory containment*).
#
#     . "$root/toolchain/lib/stage.sh"
#     stage_init "$root"                  # refuses to go on where the rule would be broken
#     nazm=${NAZM:-$(stage_checkout_nazm "$root")}
#     stage_build "$nazm" "$root/compiler/diagnose.nz" -o "$exe"
#     "$STAGE_RUN" 60 "$exe" ARGS...      # standard streams and exit status are the program's
#     jobs=$(stage_jobs 6)
#
# The rule these scripts keep:
#
#   - a Nazm-built program is built by the checkout's `nazm` (never by a release found on the
#     machine) with a heap limit of at most 2 GiB (`STAGE_HEAP`, default 2G: the counted heap the
#     runtime enforces on itself, `docs/spec.md`, *A bounded heap*);
#   - it runs inside `cargo xtask contained ...` (`NAZM_CONTAINED=1` and a cgroup memory ceiling,
#     both of which the runner provides and this file only reads), or on a macOS host through
#     `cargo xtask host-run`, one program at a time (so `stage_jobs` answers 1 there), under a
#     deadline, with its peak resident memory appended to `STAGE_PEAKS` and the program's own
#     status passed through (124 past the deadline, 126 for output over the runner's cap);
#   - anywhere else the script refuses to start.
#
# `NAZM_CONTAINED=1` is never set here, and `NAZM_ALLOW_UNCONTAINED=1` is never accepted.

# `stage_fail MESSAGE`: refuse, as the scripts' own refusals do (status 2).
stage_fail() {
    printf '%s: %s\n' "${stage_name:-stage}" "$*" >&2
    exit 2
}

# `stage_heap_bytes SIZE`: SIZE (`2G`, `512M`, `64K` or bytes) in bytes, or nothing.
stage_heap_bytes() {
    case $1 in
        '' | *[!0-9GMK]* | *[GMK]?*) return 1 ;;
        *G) echo $((${1%G} * 1073741824)) ;;
        *M) echo $((${1%M} * 1048576)) ;;
        *K) echo $((${1%K} * 1024)) ;;
        *) echo "$1" ;;
    esac
}

# `stage_checkout_nazm ROOT`: the `nazm` this checkout's Cargo builds, where Cargo puts it.
stage_checkout_nazm() {
    printf '%s\n' "${CARGO_TARGET_DIR:-$1/target}/debug/nazm"
}

# `stage_init ROOT [contained]`: choose the mode, check the heap limit, find the runner. With
# `contained` the script is one the host rule does not cover (it runs programs built without a heap
# limit, in parallel, or is a mutation campaign): it refuses on a host.
stage_init() {
    stage_root=$1
    STAGE_HEAP=${STAGE_HEAP:-2G}
    if [ "${NAZM_ALLOW_UNCONTAINED:-}" = 1 ]; then
        stage_fail "NAZM_ALLOW_UNCONTAINED=1 is never accepted (docs/runbook.md)"
    fi
    stage_bytes=$(stage_heap_bytes "$STAGE_HEAP") || stage_fail "STAGE_HEAP=$STAGE_HEAP is not a size"
    [ "$stage_bytes" -gt 0 ] && [ "$stage_bytes" -le 2147483648 ] \
        || stage_fail "STAGE_HEAP=$STAGE_HEAP: a program built here gets a heap limit of at most 2 GiB"
    STAGE_RUN="$stage_root/toolchain/lib/stage-run.sh"
    if [ "${NAZM_CONTAINED:-}" = 1 ]; then
        case $(cat /sys/fs/cgroup/memory.max 2> /dev/null || echo none) in
            '' | *[!0-9]* | 0)
                stage_fail "NAZM_CONTAINED=1 is set, but no memory ceiling applies here. Never set it on a host; run this through \`cargo xtask contained run '...'\`" ;;
        esac
        STAGE_MODE=contained
    elif [ "$(uname -s)" = Darwin ]; then
        STAGE_MODE=host
        STAGE_XTASK="${CARGO_TARGET_DIR:-$stage_root/target}/debug/xtask"
        (cd "$stage_root" && cargo build --locked -q -p xtask) || stage_fail "cargo xtask did not build"
        STAGE_PEAKS=${STAGE_PEAKS:-"${TMPDIR:-/tmp}/nazm-stage-peaks.$$"}
        : > "$STAGE_PEAKS"
        export STAGE_XTASK STAGE_PEAKS
    else
        stage_fail "a Nazm-built program runs contained (\`cargo xtask contained run '...'\`) or on macOS through \`cargo xtask host-run\`; neither applies here"
    fi
    if [ "${2:-}" = contained ] && [ "$STAGE_MODE" != contained ]; then
        stage_fail "this runs contained only (\`cargo xtask contained run '...'\`, docs/runbook.md): it is not one bounded-heap stage"
    fi
    export STAGE_HEAP STAGE_MODE STAGE_RUN
}

# `stage_jobs N`: how many programs may run at once: N contained, 1 on a host (one at a time).
stage_jobs() {
    if [ "$STAGE_MODE" = host ]; then echo 1; else echo "$1"; fi
}

# `stage_build NAZM SOURCE ARGS...`: `nazm build` with the heap limit.
stage_build() {
    stage_nazm=$1
    shift
    "$stage_nazm" build --heap-limit "$STAGE_HEAP" "$@"
}
