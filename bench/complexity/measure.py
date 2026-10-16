#!/usr/bin/env python3
"""The compiler complexity audit (performance leadership 7.1): every phase of both compilers on
generated programs of n, 2n, 4n and 8n (`gen.py`), timed and its peak memory read, each fitted to a
growth class.

    python3 bench/complexity/measure.py OUT [--runs R] [--only SHAPE,...] [--side rust,nazm]

Environment: NAZM (the release `nazm`), REFDUMP (`toolchain/frontend-tests/refdump`, release), and
the Nazm-written drivers in BIN: `tokens`, `tree`, `diagnose` (built as
`toolchain/frontend-tests/run.sh` builds them), `pipeline` (as `toolchain/pipeline/tests/run.sh`
does) and `nzcg` (as `toolchain/codegen/tests/run.sh` does). Missing tools are skipped by name.

Each measurement waits until the 1-minute load is below 2 and no cargo, rustc or cargo-nextest
runs, and records that load beside the number. Time is the median of R runs (mean and σ kept);
peak memory is the largest `maximum resident set size` of `/usr/bin/time -l` over the runs.
Writes OUT/results.json and prints a table; the growth exponent is log2 of t(8n)/t(4n) after the
empty program's time is subtracted, and a phase whose exponent passes 1.25 is flagged.
"""
import json
import os
import re
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SIZES = {"lines": 2000, "nest": 25, "chain": 60, "modules": 10}


def load():
    return float(os.getloadavg()[0])


def busy():
    for name in ("cargo", "rustc", "cargo-nextest"):
        if subprocess.run(["pgrep", "-x", name], capture_output=True).returncode == 0:
            return True
    return False


def wait_quiet():
    while load() >= 2 or busy():
        time.sleep(15)
    return load()


def run_once(cmd, cwd, env=None):
    t0 = time.perf_counter()
    p = subprocess.run(["/usr/bin/time", "-l"] + cmd, cwd=cwd, env=env,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    dt = time.perf_counter() - t0
    err = p.stderr.decode("utf-8", "replace")
    m = re.search(r"(\d+)\s+maximum resident set size", err)
    rss = int(m.group(1)) if m else 0
    return dt, rss, p.returncode, err, p.stdout


def measure(cmd, cwd, runs, setup=None, env=None):
    """Median, mean, σ of wall seconds and peak RSS bytes; the stderr of the last run."""
    ts, rss, err, rc = [], 0, "", 0
    run_once(cmd, cwd, env)  # warm-up (file cache)
    for _ in range(runs):
        if setup:
            setup()
        dt, r, rc, err, _ = run_once(cmd, cwd, env)
        ts.append(dt)
        rss = max(rss, r)
    return {"median": statistics.median(ts), "mean": statistics.mean(ts),
            "sd": statistics.stdev(ts) if len(ts) > 1 else 0.0, "rss": rss, "rc": rc,
            "err": err[-400:] if rc else ""}


def timings(cmd, cwd, runs, setup):
    """`nazm build --timings`: each phase's median milliseconds over the runs."""
    per = {}
    for _ in range(runs):
        setup()
        _, _, rc, err, _ = run_once(cmd, cwd)
        for line in err.splitlines():
            if '"nazm.timings/1"' in line:
                obj = json.loads(line[line.index("{"):])
                for ph in obj["phases"]:
                    per.setdefault(ph["phase"], []).append(ph["ms"])
                per.setdefault("total", []).append(obj["total_ms"])
    return {k: statistics.median(v) for k, v in per.items()}


def phases(nazm, refdump, bin_, d, out):
    """(side, phase, command, needs) for the program in directory d."""
    f = os.path.join(d, "main.nz")
    lib = os.path.join(ROOT, "library")
    cir = os.path.join(d, "core.json")
    lir = os.path.join(d, "ops.lir")
    ph = []
    if refdump:
        ph += [("rust", "lex (refdump tokens)", [refdump, "tokens", f]),
               ("rust", "parse (refdump tree)", [refdump, "tree", f])]
    ph += [("rust", "check --no-cache", [nazm, "check", "--no-cache", f]),
           ("rust", "check (warm cache)", [nazm, "check", f]),
           ("rust", "core-ir", [nazm, "core-ir", f]),
           ("rust", "mir", [nazm, "mir", f]),
           ("rust", "lir", [nazm, "lir", f])]
    b = lambda n: os.path.join(bin_, n)
    if os.path.exists(b("tokens")):
        ph.append(("nazm", "lex (tokens.nz)", [b("tokens"), f]))
    if os.path.exists(b("tree")):
        ph.append(("nazm", "parse (tree.nz)", [b("tree"), f]))
    if os.path.exists(b("diagnose")):
        ph += [("nazm", "syntax (diagnose --syntax)", [b("diagnose"), "--syntax", f]),
               ("nazm", "check (diagnose)", [b("diagnose"), f, lib, "macos", "aarch64"]),
               ("nazm", "core ir (diagnose --core)", [b("diagnose"), "--core", f, lib, "macos", "aarch64"])]
    if os.path.exists(b("pipeline")):
        ph += [("nazm", "mir (pipeline mir)", [b("pipeline"), "mir", cir]),
               ("nazm", "lir (pipeline lir)", [b("pipeline"), "lir", cir])]
    if os.path.exists(b("nzcg")):
        ph.append(("nazm", "codegen (nzcg)", [b("nzcg"), lir, os.path.join(out, "nzcg-objs")]))
    return ph


def main():
    out = os.path.abspath(sys.argv[1])
    args = sys.argv[2:]
    runs = int(args[args.index("--runs") + 1]) if "--runs" in args else 5
    only = args[args.index("--only") + 1].split(",") if "--only" in args else list(SIZES)
    sides = args[args.index("--side") + 1].split(",") if "--side" in args else ["rust", "nazm"]
    nazm = os.environ["NAZM"]
    refdump = os.environ.get("REFDUMP")
    bin_ = os.environ.get("BIN", "")
    os.makedirs(os.path.join(out, "nzcg-objs"), exist_ok=True)
    res_path = os.path.join(out, "results.json")
    results = json.load(open(res_path)) if os.path.exists(res_path) else []
    done = {(r["shape"], r["mult"], r["side"], r["phase"]) for r in results}
    for shape in only:
        for mult in (0, 1, 2, 4, 8):  # 0: the empty program's cost, subtracted in the fit
            d = os.path.join(out, "src", f"{shape}-{mult}")
            size = max(1, SIZES[shape] * mult) if mult else 1
            lines = int(subprocess.check_output([sys.executable, os.path.join(HERE, "gen.py"), shape, str(size), d]))
            f = os.path.join(d, "main.nz")
            # Inputs for the pipeline and the generator, from the reference.
            with open(os.path.join(d, "core.json"), "wb") as o:
                subprocess.run([nazm, "core-ir", "--json", "--with-sources", f], stdout=o, stderr=subprocess.DEVNULL)
            with open(os.path.join(d, "ops.lir"), "wb") as o:
                subprocess.run([nazm, "lir", "--ops", f], stdout=o, stderr=subprocess.DEVNULL)
            todo = [p for p in phases(nazm, refdump, bin_, d, out) if p[0] in sides]
            if "rust" in sides:
                todo += [("rust", "build --timings --no-cache", None), ("rust", "build (warm)", None),
                         ("rust", "build -O2 --no-cache", None)]
            for side, phase, cmd in todo:
                if (shape, mult, side, phase) in done:
                    continue
                l = wait_quiet()
                exe = os.path.join(d, "a.out")
                if phase == "build --timings --no-cache":
                    bcmd = [nazm, "build", "--no-cache", "--timings", f, "-o", exe]
                    r = measure(bcmd, d, runs)
                    r["phases"] = timings(bcmd, d, runs, lambda: None)
                elif phase == "build (warm)":
                    bcmd = [nazm, "build", "--timings", f, "-o", exe]
                    run_once(bcmd, d)
                    r = measure(bcmd, d, runs)
                    r["phases"] = timings(bcmd, d, runs, lambda: None)
                elif phase == "build -O2 --no-cache":
                    r = measure([nazm, "build", "--no-cache", "--opt-level", "2", f, "-o", exe], d, runs)
                else:
                    if phase.startswith("check (warm"):
                        run_once(cmd, d)
                    r = measure(cmd, d, runs)
                r.update({"shape": shape, "mult": mult, "size": size, "lines": lines, "side": side,
                          "phase": phase, "load": round(l, 2), "load_after": round(load(), 2)})
                results.append(r)
                json.dump(results, open(res_path, "w"), indent=1)
                print(f"{shape:8} x{mult} {lines:6}L {side:4} {phase:28} {r['median']*1000:9.1f} ms "
                      f"±{r['sd']*1000:6.1f} {r['rss']/1048576:7.1f} MiB rc={r['rc']} load {l:.2f}", flush=True)
    report(results, out)


def report(results, out):
    key = {}
    for r in results:
        key[(r["shape"], r["side"], r["phase"], r["mult"])] = r
    rows = []
    for (shape, side, phase) in sorted({(k[0], k[1], k[2]) for k in key}):
        get = lambda m: key.get((shape, side, phase, m))
        base = get(0)
        pts = [get(m) for m in (1, 2, 4, 8)]
        if any(p is None for p in pts):
            continue
        b = base["median"] if base else 0.0
        t = [max(p["median"] - b, 1e-6) for p in pts]
        rss = [p["rss"] for p in pts]
        import math
        exp_t = math.log2(t[3] / t[2])
        exp_all = math.log(t[3] / t[0]) / math.log(8)
        brss = base["rss"] if base else 0
        drss = [max(x - brss, 1) for x in rss]
        exp_m = math.log2(drss[3] / drss[2]) if drss[2] > 1 else 0.0
        cls = ("constant" if exp_t < 0.3 else "linear" if exp_t < 1.15 else
               "n log n" if exp_t < 1.25 else "quadratic" if exp_t > 1.75 else "superlinear")
        flag = "FLAG" if exp_t >= 1.25 or exp_m >= 1.25 else ""
        rc = max(p["rc"] for p in pts)
        rows.append({"shape": shape, "side": side, "phase": phase,
                     "ms": [round(p["median"] * 1000, 1) for p in pts],
                     "sd": [round(p["sd"] * 1000, 1) for p in pts],
                     "mib": [round(x / 1048576, 1) for x in rss], "lines": [p["lines"] for p in pts],
                     "base_ms": round(b * 1000, 1), "exp_time": round(exp_t, 2),
                     "exp_time_1_8": round(exp_all, 2), "exp_mem": round(exp_m, 2),
                     "class": cls, "flag": flag, "rc": rc,
                     "load": [p["load"] for p in pts]})
    json.dump(rows, open(os.path.join(out, "fit.json"), "w"), indent=1)
    for r in rows:
        print(f"{r['shape']:8} {r['side']:4} {r['phase']:28} {r['ms']} ms {r['mib']} MiB "
              f"exp {r['exp_time']} ({r['exp_time_1_8']}) mem {r['exp_mem']} {r['class']} {r['flag']} rc={r['rc']}")


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[2] == "--report":
        out = os.path.abspath(sys.argv[1])
        report(json.load(open(os.path.join(out, "results.json"))), out)
    else:
        main()
