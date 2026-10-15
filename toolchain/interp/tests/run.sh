#!/bin/sh
# R4's comparison (openspec/changes/r4-interpreter): build the interpreter written in Nazm with one
# prebuilt `nazm`, run it over every program of its inputs, and compare standard output, standard
# error and exit status with the Rust toolchain's `nazm run` — once plainly, once with the memory
# and result reports asked for. Prints each difference, then `N agree, M differ` (and how many
# inputs did not check, which are not the interpreter's to run).
#
#   toolchain/interp/tests/run.sh [-v] [FILE.nz ...]
#
# NAZM names the Rust toolchain compared with (default: the 0.10.0 release); NAZM_BUILD the one that
# builds the interpreter, which is written in the 0.10.0 language (default: the 0.10.0 release).
# Where NAZM has `run --interpret` (R1.5 D11), that is the oracle; before, plain `run` was.
# -v shows each difference's diff.
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
NAZM=${NAZM:-$HOME/.local/nazm-0.10.0/bin/nazm}
NAZM_BUILD=${NAZM_BUILD:-$HOME/.local/nazm-0.10.0/bin/nazm}
interpret=""
if "$NAZM" run --help 2>/dev/null | grep -q -- '--interpret'; then interpret="--interpret"; fi
verbose=0
if [ "${1:-}" = "-v" ]; then verbose=1; shift; fi
work=$(mktemp -d "${TMPDIR:-/tmp}/r4-interp.XXXXXX")
# A program may write files beside itself (examples/std/pkgtool.nz, examples/provenance/tally.nz):
# what the runs leave in the repository that was not there before is removed at the end.
untracked() { (cd "$repo" && git ls-files -o --exclude-standard examples tests toolchain/interp) 2>/dev/null; }
untracked >"$work/before" 2>/dev/null || true
tidy() {
    untracked | while read -r f; do
        grep -qxF "$f" "$work/before" || rm -f "$repo/$f"
    done
    rm -rf "$work"
}
trap tidy EXIT

if ! "$NAZM_BUILD" build --opt-level 2 "$repo/toolchain/interp/main.nz" -o "$work/interp" >"$work/build.log" 2>&1; then
    grep -v '^warning' "$work/build.log" | grep -A8 '^error' | head -40
    echo "interp: the build failed"
    exit 1
fi

# Every input: the compatibility corpus's run and trap cases, the examples and this track's own.
# `--rust-tests` instead takes the programs the Rust CLI tests embed: each string literal of
# `crates/nazm-cli/tests/*.rs` with a `fn main` in it — raw, or plain with Rust's escapes and line
# continuations undone — written out as a file of its own. A format string (one with `{{`) is left
# out: its text is not the program. Character literals are stepped over, so a `'"'` starts nothing.
if [ "${1:-}" = "--rust-tests" ]; then
    mkdir -p "$work/rs"
    for t in "$repo"/crates/nazm-cli/tests/*.rs; do
        perl -0777 -ne '
            my $base = $ARGV; $base =~ s{.*/}{}; $base =~ s{\.rs$}{};
            my $n = 0;
            while (/\br(#*)"(.*?)"\1|(?<![\w])\x27(?:[^\x27\\\n]|\\.[^\x27\n]*)\x27|"((?:[^"\\]|\\.)*)"/sg) {
                my $src;
                if (defined $2) {
                    $src = $2;
                } elsif (defined $3) {
                    $src = $3;
                    next if $src =~ /\{\{|\}\}/;
                    $src =~ s/\\\n\s*//g;
                    $src =~ s/\\(u\{[0-9a-fA-F]+\}|.)/
                        my $e = $1;
                        $e eq "n" ? "\n" : $e eq "t" ? "\t" : $e eq "r" ? "\r" : $e eq "0" ? "\0" :
                        $e =~ m{^u\{(.*)\}$} ? chr(hex($1)) : $e/ge;
                } else {
                    next;
                }
                next unless $src =~ /fn main\b/;
                $n++;
                open(my $f, ">", "'"$work"'/rs/$base-$n.nz") or die;
                print $f $src;
                close($f);
            }' "$t"
    done
    cases=$(ls "$work"/rs/*.nz)
    R4_BOUND=${R4_BOUND:-300}
elif [ $# -gt 0 ]; then
    cases="$*"
else
    cases=$( (ls "$repo"/tests/compat/v1/run/*.nz "$repo"/tests/compat/v1/run/*/main.nz \
        "$repo"/tests/compat/v1/trap/*.nz "$repo"/toolchain/interp/tests/cases/*.nz;
        find "$repo/examples" -name '*.nz' | sort) 2>/dev/null)
fi

# A run is bounded, so a hang in either interpreter is a difference rather than a stuck suite.
# `R4_BOUND` seconds each (default 60; 300 for `--rust-tests`, some of which run long).
bound=${R4_BOUND:-60}
bounded() { perl -e 'alarm shift; exec @ARGV or exit 127' "$bound" "$@"; }

agree=0
differ=0
unchecked=0
for path in $cases; do
    dir=$(dirname "$path")
    file=$(basename "$path")
    name=${path#"$repo"/}
    # A program that does not check, or a library with no `main`, is the checker's to refuse.
    if ! (cd "$dir" && "$NAZM" core-ir --json "$file" >"$work/ir.json" 2>/dev/null) ||
        grep -q '^{"schema":"nazm.core-ir/[23]","entry":null' "$work/ir.json"; then
        unchecked=$((unchecked + 1))
        continue
    fi
    # Plainly; with the reports; and, for a program that fails at run time, with `--json`.
    modes="plain reports"
    while [ -n "$modes" ]; do
        mode=${modes%% *}
        if [ "$mode" = "$modes" ]; then modes=""; else modes=${modes#* }; fi
        # The interpreter takes the reports as flags: as a compiled Nazm program, its own runtime
        # would answer the variables about itself (main.nz).
        json=""
        if [ "$mode" = reports ]; then
            set -- env NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 NAZM_RESULT_REPORT=1
            flags="--memory-report --sched-report --result-report"
        else
            set -- env
            flags=""
            [ "$mode" = json ] && json="--json"
        fi
        (cd "$dir" && bounded "$@" "$NAZM" run $interpret $json "$file" </dev/null >"$work/rust.out" 2>"$work/rust.err")
        rust=$?
        (cd "$dir" && bounded "$work/interp" $json $flags --std "$repo/library" "$work/ir.json" "$file" \
            </dev/null >"$work/nazm.out" 2>"$work/nazm.err")
        nazm=$?
        [ "$mode" = plain ] && [ "$rust" = 2 ] && modes="$modes json"
        what=""
        cmp -s "$work/rust.out" "$work/nazm.out" || what="$what stdout"
        cmp -s "$work/rust.err" "$work/nazm.err" || what="$what stderr"
        [ "$rust" = "$nazm" ] || what="$what status($rust/$nazm)"
        if [ -z "$what" ]; then
            agree=$((agree + 1))
        else
            differ=$((differ + 1))
            echo "differ: $name [$mode]$what"
            if [ $verbose = 1 ]; then
                diff "$work/rust.out" "$work/nazm.out" | head -10
                diff "$work/rust.err" "$work/nazm.err" | head -10
            fi
        fi
    done
done
echo "$agree agree, $differ differ ($unchecked inputs do not check)"
[ $differ = 0 ]
