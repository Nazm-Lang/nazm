#!/usr/bin/env python3
"""A growth guard for one phase of a Nazm-written compiler (performance leadership 7.4): the work
it does on a generated program of size n and of 8n (`gen.py`), refused when 8n costs more than
BOUND times n.

    python3 bench/complexity/growth.py NAME SHAPE N BOUND INPUT NAZM OUT -- COMMAND...

INPUT is what the phase reads, made from the generated `main.nz` by the reference NAZM: `src` (the
source itself), `core` (`nazm core-ir --json --with-sources`) or `ops` (`nazm lir --ops`). In
COMMAND, `{in}` is replaced by that input's path and `{dir}` by an empty directory of the run's.

Work is counted, not timed: the instructions the process retires, from `/usr/bin/time -l` (macOS),
which load does not change the way it changes wall time. A linear phase gives a little under 8 (its
fixed start-up cost does not grow); a quadratic one approaches 64. Prints one line — `passed`,
`failed` or `skipped` (no instruction count on this host) — and exits 1 only on `failed`.
"""
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def work(cmd):
    # A program built by Nazm runs on a macOS host only through `cargo xtask host-run`
    # (`toolchain/lib/stage.sh` sets STAGE_MODE=host and STAGE_XTASK), which records the
    # instructions retired in its report file; contained (Linux) there is no counter.
    if os.environ.get("STAGE_MODE") == "contained":
        r = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return r.returncode, None
    if os.environ.get("STAGE_MODE") == "host":
        report = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"nazm-growth-{os.getpid()}.txt")
        try:
            r = subprocess.run([os.environ["STAGE_XTASK"], "host-run", "--deadline-secs", "900",
                                "--report", report, "--max-output-mib", "256", "--"] + cmd,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            with open(report) as f:
                m = re.search(r"instructions retired (\d+)", f.read())
        finally:
            if os.path.exists(report):
                os.remove(report)
        return r.returncode, int(m.group(1)) if m else None
    r = subprocess.run(["/usr/bin/time", "-l"] + cmd, stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE, text=True)
    m = re.search(r"(\d+)\s+instructions retired", r.stderr)
    return r.returncode, int(m.group(1)) if m else None


def main():
    sep = sys.argv.index("--")
    name, shape, n, bound, kind, nazm, out = sys.argv[1:sep]
    command = sys.argv[sep + 1:]
    n, bound = int(n), float(bound)
    counts = []
    for size in (n, 8 * n):
        d = os.path.join(out, f"{shape}-{size}")
        subprocess.run([sys.executable, os.path.join(HERE, "gen.py"), shape, str(size), d],
                       check=True, stdout=subprocess.DEVNULL)
        src = os.path.join(d, "main.nz")
        if kind == "src":
            inp = src
        else:
            inp = os.path.join(d, "core.json" if kind == "core" else "ops.lir")
            args = ["core-ir", "--json", "--with-sources"] if kind == "core" else ["lir", "--ops"]
            with open(inp, "wb") as o:
                subprocess.run([nazm] + args + [src], stdout=o, stderr=subprocess.DEVNULL, check=True)
        scratch = os.path.join(d, "run")
        shutil.rmtree(scratch, ignore_errors=True)
        os.makedirs(scratch)
        rc, ins = work([a.replace("{in}", inp).replace("{dir}", scratch) for a in command])
        if rc != 0:
            print(f"growth {name} on {shape}: failed (exit {rc} at size {size})")
            sys.exit(1)
        if ins is None:
            print(f"growth {name} on {shape}: skipped (no instruction count on this host)")
            return
        counts.append(ins)
    ratio = counts[1] / counts[0]
    verdict = "passed" if ratio <= bound else "failed"
    print(f"growth {name} on {shape} {n} -> {8 * n}: work x{ratio:.1f} (bound x{bound:g}): {verdict}")
    sys.exit(0 if verdict == "passed" else 1)


if __name__ == "__main__":
    main()
