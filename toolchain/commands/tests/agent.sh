#!/bin/bash
# R8 slice 9: the agent commands whole, written in Nazm, against the Rust ones
# (`openspec/changes/r8-cut-over/design.md`, *Slice 9*, task 9.1 and 9.2; R5's method, `openspec/changes/r5-commands/`).
#
#     toolchain/commands/tests/agent.sh [PART...] [-- FILE.nz ...]
#     PART: context args repo repo-args effects mcp-gen mcp-serve mcp-cmp   (the first four by default; AGENT_PARTS="context" does the same)
#
#   context     `context --root FILE --file FILE --byte N --json` at the offsets `context-positions.py` chooses in every input
#               (AGENT_POSITIONS=N of them per file, default 6), and `--root FILE --file FILE --byte N` (the indented form) at
#               the first three of them: the packet, or the reason there is none.
#   args        every command line of tests/args/context.txt, run in the repository's root.
#   repo        `repo --map --root .` in both forms and `repo --task` of seeds read from the reference's map (design, S9.9).
#   repo-args   every command line of tests/args/repo.txt.
#   mcp-*       the MCP tools `nazm.semantic_context`, `nazm.repository_map` and `nazm.task_context`, in three steps because a container has no
#               Python and a host-run no standard input: `mcp-gen` (host) writes MCP_DIR/N.in, the messages of `mcp-compare.py agent` for each
#               input, and N.ref, the reference `nazm-mcp`'s answers; `mcp-serve` (contained) answers each with the Nazm `nazm-mcp` asking the Nazm
#               `nazm`; `mcp-cmp` (host) compares N.got with N.ref, ordered by request id.
#   effects     R2's function effects (task 9.1): `compiler/diagnose.nz --effects` against `refdump effects` (REFDUMP, the reference dumper
#               `toolchain/frontend-tests/run.sh` builds) on every input; a refusal is a driver that stops where the reference does not.
#
# The Nazm `nazm` (toolchain/commands/nazm.nz) is built by the checkout's `nazm` ($NAZM) with a heap limit of at most 2 GiB
# (`toolchain/lib/stage.sh`, the R6 rule). Each Nazm run is one `host-run`, one at a time (a refusal because another stage is
# running is waited out, not counted); the Rust side runs directly. Both run from the repository's root with the file named
# by its path under it, so a diagnostic names the same file on both sides. A case agrees when standard output, standard error
# and the exit status are the same bytes. A status 6 from the Nazm side (what it does not carry) is counted apart, with its
# reason, and is never an agreement. Prints a line per case that does not agree, then one line per part:
# `PART: A agree, D differ, R refused`. Exit 0 only when nothing differs.
#
# The reference is $NAZM_REF (default $NAZM). The inputs are the tracked programs of the conformance, compatibility, example and
# front-end corpora; BIG=1 adds the toolchain's own sources. KEEP=DIR keeps each difference's two records.
set -u
stage_name=agent-compare
. "$(dirname "$0")/../../lib/stage.sh"
stage_init "$(cd "$(dirname "$0")/../../.." && pwd)"
ROOT=$stage_root
NAZM=${NAZM:-$(stage_checkout_nazm "$ROOT")}
REF=${NAZM_REF:-$NAZM}
OUT=${AGENT_OUT:-${TMPDIR:-/tmp}/nazm-agent-compare}
mkdir -p "$OUT/bin" "$OUT/work"

parts=()
files=()
while [ $# -gt 0 ]; do
    case $1 in
        --) shift; files=("$@"); break ;;
        *) parts+=("$1"); shift ;;
    esac
done
[ ${#parts[@]} -gt 0 ] || read -r -a parts <<< "${AGENT_PARTS:-context args repo repo-args}"

if [ -z "${NAZM_PORT:-}" ]; then
    if ! stage_build "$NAZM" --stack-size 256M "$ROOT/toolchain/commands/nazm.nz" -o "$OUT/bin/nazm" > "$OUT/bin/nazm.log" 2>&1; then
        echo "agent-compare: nazm did not build:"
        cat "$OUT/bin/nazm.log"
        exit 2
    fi
fi
PORT=${NAZM_PORT:-$OUT/bin/nazm}

if [ ${#files[@]} -eq 0 ]; then
    cd "$ROOT" || exit 2
    while IFS= read -r f; do files+=("$f"); done < <(
        {
            git ls-files 'examples/*.nz' 'examples/bad/*.nz' 'tests/compat/v1/*.nz' 'compiler/conformance/*.nz' \
                'toolchain/codegen/tests/programs/*.nz' 'crates/nazm-cli/tests/refusals/*.nz' 'crates/nazm-cli/tests/transfers/*.nz' \
                'toolchain/frontend-tests/inputs/check/*.nz' 'toolchain/frontend-tests/inputs/modules/*.nz'
            if [ "${BIG:-}" = 1 ]; then git ls-files 'compiler/*.nz' 'toolchain/commands/*.nz' 'toolchain/pipeline/*.nz'; fi
        } | LC_ALL=C sort -u
    )
fi

# NAZM_NO_LIBRARY=1 leaves NAZM_LIBRARY unset: the Nazm `nazm` then reads the library it carries (R8 slice 12).
if [ -n "${NAZM_NO_LIBRARY:-}" ]; then unset NAZM_LIBRARY; else export NAZM_LIBRARY="$ROOT/library"; fi
cd "$ROOT" || exit 2

agree=0
differ=0
refused=0
total_differ=0
n=0

# `case_run LABEL ARG...`: both sides run `nazm ARG...`; counts and reports.
case_run() {
    label=$1
    shift
    W=$OUT/work/case
    rm -rf "$W"
    mkdir -p "$W"
    "$REF" "$@" > "$W/ref.out" 2> "$W/ref.err" < /dev/null
    echo $? > "$W/ref.status"
    while :; do
        "$STAGE_RUN" "${AGENT_DEADLINE:-300}" "$PORT" "$@" > "$W/got.out" 2> "$W/got.err"
        rc=$?
        # Another stage or a contained workload has the host: wait for it, not counted.
        if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/got.err"; then sleep 10; continue; fi
        break
    done
    echo $rc > "$W/got.status"
    if [ "$rc" = 6 ]; then
        refused=$((refused + 1))
        echo "refused  $label: $(head -c 200 "$W/got.err" | tr '\n' ' ')"
        return
    fi
    if cmp -s "$W/ref.out" "$W/got.out" && cmp -s "$W/ref.err" "$W/got.err" && cmp -s "$W/ref.status" "$W/got.status"; then
        agree=$((agree + 1))
        return
    fi
    differ=$((differ + 1))
    echo "differ   $label (status $(cat "$W/ref.status") vs $(cat "$W/got.status"))"
    for x in out err; do
        cmp -s "$W/ref.$x" "$W/got.$x" || diff "$W/ref.$x" "$W/got.$x" | head -${DIFF_LINES:-4} | cut -c1-200 | sed 's/^/    /'
    done
    if [ -n "${KEEP:-}" ]; then
        k=$KEEP/$(echo "$label" | tr '/ ' '__')
        mkdir -p "$k" && cp "$W"/* "$k"/
    fi
}

# `part_args NAME`: every command line of tests/args/NAME.txt, run from the repository's root.
part_args() {
    name=$1
    cmd=$2
    agree=0
    differ=0
    refused=0
    list=$ROOT/toolchain/commands/tests/args/$name.txt
    [ -f "$list" ] || { echo "$name: no $list"; return; }
    while IFS= read -r line; do
        case $line in '' | '#'*) continue ;; esac
        [ "$line" = '(none)' ] && line=
        # The words of a line as the shell reads them (a seed with a space is quoted).
        eval "set -- $line"
        case_run "$cmd args: ${line:-(none)}" "$cmd" "$@"
    done < "$list"
    echo "$name: $agree agree, $differ differ, $refused refused"
    total_differ=$((total_differ + differ))
}

for p in "${parts[@]}"; do
    case $p in
        context)
            agree=0; differ=0; refused=0
            for f in "${files[@]}"; do
                k=0
                while IFS= read -r b; do
                    case_run "context $f @$b" context --root "$f" --file "$f" --byte "$b" --json
                    [ $k -lt 3 ] && case_run "context $f @$b (indented)" context --root "$f" --file "$f" --byte "$b"
                    k=$((k + 1))
                done < <(python3 "$ROOT/toolchain/commands/tests/context-positions.py" "$f" "${AGENT_POSITIONS:-6}")
            done
            echo "context: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        args) part_args context context ;;
        repo)
            agree=0; differ=0; refused=0
            # The repository as it is now, copied once, so that nothing edited meanwhile moves the state both sides are asked of.
            FROZEN=$OUT/repo
            rm -rf "$FROZEN"
            mkdir -p "$FROZEN"
            git ls-files -co --exclude-standard | grep -v '^docs/prompts-from-chatgpt/' | rsync -a --files-from=- ./ "$FROZEN/"
            case_run "repo --map --json" repo --map --json --root "$FROZEN"
            case_run "repo --map" repo --map --root "$FROZEN"
            while IFS= read -r line; do
                eval "set -- $line"
                case_run "repo --task $line" repo --task --json --root "$FROZEN" "$@"
                # The indented form of every tenth.
                n=$((n + 1))
                [ $((n % 10)) -eq 0 ] && case_run "repo --task $line (indented)" repo --task --root "$FROZEN" "$@"
            done < <(python3 "$ROOT/toolchain/commands/tests/repo-seeds.py" "$REF" "$FROZEN" "${AGENT_EVERY:-1}")
            echo "repo: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        repo-args) part_args repo repo ;;
        mcp-gen)
            # Host: the messages of `mcp-compare.py agent` for each input, and the reference's answers (MCP_DIR, NAZM_MCP_REF).
            : "${MCP_DIR:?mcp-gen needs MCP_DIR}"; mkdir -p "$MCP_DIR"
            MCP_REF=${NAZM_MCP_REF:-$(dirname "$REF")/nazm-mcp}
            k=0
            printf '%s\n' "$ROOT" > "$MCP_DIR/root"
            for f in "${files[@]}"; do
                NAZM_REF="$REF" python3 -I "$ROOT/toolchain/commands/tests/mcp-compare.py" agent-gen "$MCP_REF" x "$ROOT" "$f" "$MCP_DIR/$k"
                echo "$f" > "$MCP_DIR/$k.name"
                k=$((k + 1))
            done
            # One session of a server started with no `--docs`: the documentation and repository tools of the repository it was built from.
            NAZM_REF="$REF" python3 -I "$ROOT/toolchain/commands/tests/mcp-compare.py" nodocs-gen "$MCP_REF" x "$ROOT" examples/max.nz "$MCP_DIR/$k"
            echo examples/max.nz > "$MCP_DIR/$k.name" ;;
        mcp-serve)
            # Contained only (a host-run has no standard input): the Nazm `nazm-mcp`, asking the Nazm `nazm`, answers each `.in` of MCP_DIR into `.got`.
            [ "$STAGE_MODE" = contained ] || { echo "agent-compare: mcp-serve runs contained only (cargo xtask contained run '...')"; exit 2; }
            : "${MCP_DIR:?mcp-serve needs MCP_DIR}"
            if ! stage_build "$NAZM" --stack-size 128M "$ROOT/toolchain/commands/mcp.nz" -o "$OUT/bin/nazm-mcp" > "$OUT/bin/mcp.log" 2>&1; then
                echo "agent-compare: mcp.nz did not build:"; cat "$OUT/bin/mcp.log"; exit 2
            fi
            # The checkout is read-only in the container and nothing outlives it, so each answer goes to standard output as `GOT N BASE64`
            # (`mcp-cmp` reads them back from a log: MCP_LOG).
            mkdir -p "$OUT/mcp"
            for i in "$MCP_DIR"/*.in; do
                n=$(basename "${i%.in}")
                f=$(cat "${i%.in}.name")
                # The messages name the checkout the host generated them in once (the absolute-path call): here it is $ROOT. The runner gives
                # the program its standard input from STAGE_STDIN (`< "$i"` on the command would be replaced by /dev/null: the hang of 2026-10-22).
                sed "s#$(cat "$MCP_DIR/root")#$ROOT#g" "$i" > "$OUT/mcp/$n.in"
                NAZM_EXE="$PORT" STAGE_STDIN="$OUT/mcp/$n.in" "$STAGE_RUN" "${AGENT_DEADLINE:-600}" "$OUT/bin/nazm-mcp" --root "$f" $([ -f "${i%.in}.nodocs" ] || printf -- '--docs\n%s' "$ROOT") > "$OUT/mcp/$n.got" 2> "$OUT/mcp/$n.err" || true
                printf 'GOT %s %s\n' "$n" "$(base64 < "$OUT/mcp/$n.got" | tr -d '\n')"
            done ;;
        mcp-cmp)
            agree=0; differ=0; refused=0
            if [ -n "${MCP_LOG:-}" ]; then
                grep '^GOT ' "$MCP_LOG" | while read -r _ n b; do printf '%s' "$b" | base64 -d > "$MCP_DIR/$n.got"; done
            fi
            for i in "$MCP_DIR"/*.in; do
                v=$(python3 -I "$ROOT/toolchain/commands/tests/mcp-compare.py" agent-cmp "${i%.in}" x x)
                f=$(cat "${i%.in}.name")
                case $v in
                    agree*) agree=$((agree + 1)) ;;
                    refused*) refused=$((refused + 1)); echo "refused  mcp $f: ${v#refused: }" ;;
                    *) differ=$((differ + 1)); echo "differ   mcp $f: $v" ;;
                esac
            done
            echo "mcp: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        effects)
            agree=0; differ=0; refused=0
            : "${REFDUMP:?effects needs REFDUMP, the reference dumper (toolchain/frontend-tests/refdump, built by run.sh)}"
            if ! stage_build "$NAZM" --stack-size 256M "$ROOT/compiler/diagnose.nz" -o "$OUT/bin/diagnose" > "$OUT/bin/diagnose.log" 2>&1; then
                echo "agent-compare: diagnose did not build:"; cat "$OUT/bin/diagnose.log"; exit 2
            fi
            W=$OUT/work/effects; mkdir -p "$W"
            for f in "${files[@]}"; do
                "$REFDUMP" effects "$ROOT/$f" > "$W/want" 2> /dev/null
                while :; do
                    "$STAGE_RUN" "${AGENT_DEADLINE:-300}" "$OUT/bin/diagnose" --effects "$ROOT/$f" "$ROOT/library" macos aarch64 > "$W/got" 2> "$W/err"
                    if grep -q '^host-run: refused \[\(another-stage-running\|contained-workload-running\)\]' "$W/err"; then sleep 10; continue; fi
                    break
                done
                if [ "$(head -c 5 "$W/got")" = stops ] && [ "$(head -c 5 "$W/want")" != stops ]; then
                    refused=$((refused + 1)); echo "refused  effects $f: the checker stops"
                elif cmp -s "$W/want" "$W/got"; then
                    agree=$((agree + 1))
                else
                    differ=$((differ + 1)); echo "differ   effects $f"; diff "$W/want" "$W/got" | head -4 | cut -c1-200 | sed 's/^/    /'
                fi
            done
            echo "effects: $agree agree, $differ differ, $refused refused"
            total_differ=$((total_differ + differ)) ;;
        *) echo "agent-compare: no part $p"; exit 2 ;;
    esac
done
[ "$total_differ" -eq 0 ]
