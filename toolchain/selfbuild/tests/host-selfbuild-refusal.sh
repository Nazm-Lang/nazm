#!/bin/bash
# A host-run refusal must reach the log of host-selfbuild.sh even when the step's own standard error
# is sent to /dev/null (the core step's was, and the first macOS self-build lost its
# `no-counted-heap` refusal that way). Runs no Nazm-built program: STAGE_RUN is a fake that only
# prints a refusal. Pure shell; prints `ok` or fails.
set -u
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
script=$here/../host-selfbuild.sh
work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT

# The script's own `exec 3>&2`, `fail`, and `step`, cut out of it.
{
    sed -n '/^exec 3>&2$/p' "$script"
    sed -n '/^fail() {/p' "$script"
    echo 'trap "exit 1" TERM'
    sed -n '/^step() {$/,/^}$/p' "$script"
} > "$work/funcs.sh"
grep -q '^step()' "$work/funcs.sh" && grep -q '^fail()' "$work/funcs.sh" && grep -q '^exec 3>&2' "$work/funcs.sh" || { echo "FAIL: could not cut the functions out of $script"; exit 1; }

cat > "$work/fake-run" <<'F'
#!/bin/sh
echo "host-run: refused [no-counted-heap]: the executable does not link the counted heap" >&2
exit 1
F
chmod +x "$work/fake-run"

cat > "$work/driver.sh" <<D
. "$work/funcs.sh"
STAGE_RUN="$work/fake-run"; peaks="$work/peaks"; errf="$work/.step.err"; dry=
# exactly how the core step is called: standard error to /dev/null
step "core fe" 60 /bin/true > "$work/out.json" 2> /dev/null
echo "NOT STOPPED"
D
bash "$work/driver.sh" > "$work/stdout" 2> "$work/stderr"
st=$?
fails=0
[ "$st" -eq 1 ] || { echo "FAIL: status $st, wanted 1"; fails=1; }
grep -q 'refused \[no-counted-heap\]' "$work/stderr" || { echo "FAIL: the refusal is not in the log: $(cat "$work/stderr")"; fails=1; }
grep -q 'FAILED: host-run stopped the run at' "$work/stderr" || { echo "FAIL: no FAILED line"; fails=1; }
grep -q 'NOT STOPPED' "$work/stdout" && { echo "FAIL: the loop went on"; fails=1; }
[ "$fails" -eq 0 ] && echo ok
exit "$fails"
