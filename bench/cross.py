#!/usr/bin/env python3
"""Gate 4 light: a small cross-language set, timed on the host (docs/internal/performance-log.md).

Each workload is a Nazm program in bench/programs/ and its statement-for-statement references in
bench/reference/. Every executable must print the program's .expected answer before it is timed;
one that does not is recorded as not measured. Build time and run time are separate measurements.
A toolchain that is absent is recorded as absent, never skipped silently.

    python3 bench/cross.py NAZM OUT_DIR

NAZM is the `nazm` binary to measure. Results go to OUT_DIR/cross.json and OUT_DIR/cross.md.
Nothing is written outside OUT_DIR. The host is not controlled: thermal state, other processes and
frequency scaling all reach these numbers, which is why each one carries its spread.
"""

import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WARMUP = 3
RUNS = 21
BUILD_RUNS = 5
WORKLOADS = ["floor", "sieve", "channels"]


def version(cmd):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        text = (out.stdout or out.stderr).strip().splitlines()
        return text[0] if text else "?"
    except (OSError, subprocess.TimeoutExpired):
        return None


def timed(cmd, env=None, cwd=None):
    start = time.perf_counter()
    out = subprocess.run(cmd, capture_output=True, text=True, env=env, cwd=cwd, timeout=600)
    return time.perf_counter() - start, out


def spread(xs):
    xs = sorted(xs)
    q = statistics.quantiles(xs, n=4) if len(xs) >= 4 else [xs[0], xs[len(xs) // 2], xs[-1]]
    return {
        "runs": len(xs),
        "median_ms": round(statistics.median(xs) * 1000, 3),
        "min_ms": round(xs[0] * 1000, 3),
        "max_ms": round(xs[-1] * 1000, 3),
        "iqr_ms": round((q[2] - q[0]) * 1000, 3),
        "stdev_ms": round(statistics.stdev(xs) * 1000, 3) if len(xs) > 1 else 0.0,
    }


def peak_rss(cmd):
    """Peak resident memory of one run, in KiB, from the system's `time`."""
    if platform.system() == "Darwin":
        out = subprocess.run(["/usr/bin/time", "-l", *cmd], capture_output=True, text=True)
        for line in out.stderr.splitlines():
            if "maximum resident set size" in line:
                return int(line.split()[0]) // 1024
    else:
        out = subprocess.run(["/usr/bin/time", "-v", *cmd], capture_output=True, text=True)
        for line in out.stderr.splitlines():
            if "Maximum resident set size" in line:
                return int(line.split()[-1])
    return None


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    nazm = str(Path(sys.argv[1]).resolve())
    out_dir = Path(sys.argv[2]).resolve()
    work = out_dir / "work"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    goenv = dict(os.environ, GOCACHE=str(work / "gocache"), GOPATH=str(work / "gopath"),
                 GOTOOLCHAIN="local", GOFLAGS="-mod=mod")
    env = {k: v for k, v in os.environ.items() if not k.startswith("NAZM_")}

    record = {
        "host": {
            "machine": platform.machine(),
            "system": f"{platform.system()} {platform.release()}",
            "cpus": os.cpu_count(),
            "processor": version(["sysctl", "-n", "machdep.cpu.brand_string"]),
        },
        "toolchains": {
            "nazm": version([nazm, "--version"]),
            "clang": version(["clang", "--version"]),
            "rustc": version(["rustc", "--version"]),
            "go": version(["go", "version"]),
            "python3": version(["python3", "--version"]),
            "zig": version(["zig", "version"]),
            "julia": version(["julia", "--version"]),
        },
        "method": {
            "warmup": WARMUP,
            "runs": RUNS,
            "build_runs": BUILD_RUNS,
            "clock": "wall clock around the whole process (time.perf_counter), stdout captured",
            "builds": "nazm --no-cache rows are cold builds of the program (the runtime included); "
                      "the warm rows and Go's reuse cached objects; clang and rustc build from "
                      "source every time and have no cache",
        },
        "workloads": {},
    }

    for name in WORKLOADS:
        expected = (ROOT / "bench/programs" / f"{name}.expected").read_text().strip()
        source = ROOT / "bench/programs" / f"{name}.nz"
        rows = []
        # Each variant: label, how it is built (or None for an interpreter), how it is run.
        variants = [
            ("nazm llvm -O2", [nazm, "build", "--no-cache", "--backend", "llvm", "--opt-level", "2",
                               str(source), "-o", str(work / f"{name}.llvm")],
             [str(work / f"{name}.llvm")], env, "checked arithmetic and bounds"),
            ("nazm llvm -O2, object cache warm", [nazm, "build", "--backend", "llvm",
                                                 "--opt-level", "2", str(source), "-o",
                                                 str(work / f"{name}.warm")],
             [str(work / f"{name}.warm")], env, "checked arithmetic and bounds"),
            ("nazm cranelift", [nazm, "build", "--no-cache", "--backend", "cranelift",
                                str(source), "-o", str(work / f"{name}.clif")],
             [str(work / f"{name}.clif")], env, "checked arithmetic and bounds"),
            ("nazm interpreter", None, [nazm, "run", "--interpret", str(source)], env,
             "checked arithmetic and bounds"),
        ]
        refs = {
            "c": ("clang -O2", lambda s, o: ["clang", "-O2", "-o", o, s], "unchecked"),
            "rs": ("rustc -O -C overflow-checks=on",
                   lambda s, o: ["rustc", "-O", "-C", "overflow-checks=on", "-o", o, s],
                   "checked arithmetic and bounds"),
            # Go's build cache keeps the standard library and, after the first build, this
            # package: its build time is a warm build, comparable to Nazm's warm row only.
            "go": ("go build (build cache warm)", lambda s, o: ["go", "build", "-o", o, s],
                   "wrapping arithmetic, checked bounds"),
        }
        for ext, (label, build, semantics) in refs.items():
            src = ROOT / "bench/reference" / f"{name}.{ext}"
            if src.exists():
                exe = str(work / f"{name}.{ext}.out")
                variants.append((label, build(str(src), exe), [exe],
                                 goenv if ext == "go" else env, semantics))
        py = ROOT / "bench/reference" / f"{name}.py"
        if py.exists():
            variants.append(("python3", None, ["python3", str(py)], env, "unbounded integers"))

        for label, build, run, venv, semantics in variants:
            row = {"variant": label, "semantics": semantics}
            if build is not None:
                tool = build[0]
                if tool != nazm and shutil.which(tool) is None:
                    row["status"] = f"unmeasured: {tool} is not installed"
                    rows.append(row)
                    continue
                times = []
                failed = None
                if "warm" in label:
                    # One build first, unmeasured, so every timed build finds its objects cached.
                    timed(build, env=venv)
                for _ in range(BUILD_RUNS):
                    t, out = timed(build, env=venv)
                    if out.returncode != 0:
                        failed = out.stderr.strip()[:300]
                        break
                    times.append(t)
                if failed is not None:
                    row["status"] = f"unmeasured: the build failed: {failed}"
                    rows.append(row)
                    continue
                row["build"] = spread(times)
                row["build_command"] = " ".join(Path(a).name if "/" in a else a for a in build)
            elif shutil.which(run[0]) is None and run[0] != nazm:
                row["status"] = f"unmeasured: {run[0]} is not installed"
                rows.append(row)
                continue
            _, out = timed(run, env=venv)
            got = out.stdout.strip().splitlines()
            answer = got[0] if got else ""
            if answer != expected:
                row["status"] = f"unmeasured: it printed {answer!r}, not {expected!r}"
                rows.append(row)
                continue
            for _ in range(WARMUP):
                timed(run, env=venv)
            row["run"] = spread([timed(run, env=venv)[0] for _ in range(RUNS)])
            row["peak_rss_kib"] = peak_rss(run)
            row["status"] = "measured; output checked"
            rows.append(row)
        record["workloads"][name] = {"expected": expected, "rows": rows}

    (out_dir / "cross.json").write_text(json.dumps(record, indent=2) + "\n")
    lines = ["| workload | variant | run median (ms) | run min–max (ms) | run IQR (ms) | "
             "peak RSS (KiB) | build median (ms) | status |",
             "| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |"]
    for name, w in record["workloads"].items():
        for r in w["rows"]:
            run = r.get("run")
            build = r.get("build")
            dash = "\u2014"
            span = f"{run['min_ms']}\u2013{run['max_ms']}" if run else dash
            lines.append(
                f"| {name} | {r['variant']} | {run['median_ms'] if run else dash} | {span} | "
                f"{run['iqr_ms'] if run else dash} | {r.get('peak_rss_kib') or dash} | "
                f"{build['median_ms'] if build else dash} | {r['status']} |")
    (out_dir / "cross.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    print(json.dumps(record["toolchains"], indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
