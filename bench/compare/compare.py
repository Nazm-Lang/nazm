#!/usr/bin/env python3
"""R1, 5b.10: three small programs in fifteen languages, timed on the host (docs/performance.md).

Each workload is written once per language under bench/compare/<lang>/, idiomatically and as
short as is natural there:

    hello  prints `Hello, world!`
    fib    recursive fib(35), printing 9227465
    words  counts the words of a generated file with the language's standard hash map and prints
           the number of distinct words, then the ten most frequent as `word count`, most frequent
           first, ties by word ascending (words.expected; the input is gen_words.py's)
    nbody  the Computer Language Benchmarks Game's n-body (floating point): five bodies, 5,000,000
           steps of 0.01, printing the energy before and after to 9 decimals
    bintrees  the benchmarks game's binary-trees (allocation): depth 18, a stretch tree, a
           long-lived tree and the checks, as tab-separated lines. Both are sized for C -O2 at
           about 100-300 ms; `--tiny` runs them at 1,000 steps and depth 8, to check every
           language's output without measuring anything
    primes the parallel workload (performance leadership, multicore; Nazm, C, Go and Rust):
           counts the primes below 3,000,000 by trial division, printing 216816, on W workers —
           blocks of 1,000 numbers dealt round robin, block b to worker b mod W, each worker
           sending its count back — run at W = 1, 2, 4, 8 and the host's processor count, each
           with its speed-up over the same build's W=1. Blocks rather than single numbers: with
           n ≡ w mod W and W even, every odd number, so every prime but 2, falls to the odd
           workers, and half the workers would have nothing to do in every language.
           Interpreters are not timed on it. Each run records the load average before and after

Every program is built with its language's usual release flags (recorded in the output) and must
print the expected output before it is timed; one that does not is recorded as not measured. A
toolchain that is absent is recorded as absent, never skipped silently.

    python3 bench/compare/compare.py NAZM OUT_DIR [--runs N] [--only NAME,...]

`--only` measures the named programs alone (say `--only primes`); the default is every one.

NAZM is the `nazm` binary to measure. Results go to OUT_DIR/compare.json and OUT_DIR/compare.md;
builds (run in OUT_DIR/work, since gnatmake writes objects to its working directory), caches
and the generated input go to OUT_DIR/work. Nothing is written outside OUT_DIR.

Measured for each program: run time (`hyperfine --warmup 2 --runs N --shell=none` when hyperfine
is on PATH, else a wall clock around the process, as bench/cross.py), peak resident memory of one
run (`/usr/bin/time`), the size of the executable where there is one, build time (the median of
BUILD_RUNS builds after one unmeasured build, so a toolchain's caches of its own standard library
are warm; Nazm's twice, as two rows — uncached with `--no-cache`, and with its user cache warm
and its project store empty, R1.5 1.2), and lines of code (non-blank lines that are not wholly a comment).
Since 2026-10-17 also: the run time's σ, the program's characters (of those lines, without
indentation), and the cold build — the first of the run, its work directory's caches empty — beside
the warm median; and Fortran, COBOL and hand-written aarch64 assembly. Each variant also states what
it checks at run or compile time (R1.5 5.1, the `checks` column, SAFETY below), and the compiled
languages that can are built a second time with the checks Nazm always has — Rust with overflow
checks, Zig ReleaseSafe, C and C++ with AddressSanitizer and UndefinedBehaviorSanitizer — so a row
compares like with like. The host is not
controlled: thermal state, other processes and frequency scaling all reach these numbers, which
is why each run time carries its spread.
"""

import glob
import json
import os
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
WARMUP = 2
RUNS = 10
SLOW_RUNS = 3  # for a program whose checking run took longer than SLOW_SECONDS
SLOW_SECONDS = 2.0
BUILD_RUNS = 3
WORKLOADS = ["hello", "fib", "words", "nbody", "bintrees"]
# Sizes of the two newer programs, as the argument each is run with. The default is sized for C
# -O2 at roughly 100-300 ms (the Computer Language Benchmarks Game's figures, scaled; not timed
# here); `--tiny` runs a small size, for checking every language's output without a benchmark.
NBODY_STEPS = 5_000_000
BINTREES_DEPTH = 18
TINY = {"nbody": 1000, "bintrees": 8}
# The parallel workload (performance leadership, multicore): run at each of worker_counts().
PARALLEL = ["primes"]
# Programs Nazm's interpreter is not timed on: minutes a run, and its row is never ranked.
INTERPRETER_UNTIMED = ["nbody", "bintrees"]
DASH = "—"

# R1.5 5.1: what each variant checks — integer overflow, bounds, memory (use after free, double
# free), data races — and when: at compile time, or at run time. Nazm's are checked in every build
# (overflow and bounds at run time; memory by the language's ownership and reclamation; races and
# leaked tasks refused at compile time, N0321 and N0320).
SAFETY = {
    "nazm build --opt-level 2": "overflow, bounds, memory; races refused at compile time",
    "nazm build --opt-level 2 (user cache warm)": "as above",
    "nazm run (interpreter)": "as above",
    "clang -O2": "none",
    "clang -O2 -fsanitize=address,undefined": "overflow, bounds, memory (sanitizers, run time)",
    "clang++ -O2": "none",
    "clang++ -O2 -fsanitize=address,undefined": "overflow, bounds, memory (sanitizers, run time)",
    "go build": "bounds, memory; overflow wraps; races only under -race",
    "rustc -C opt-level=3": "bounds, memory, races (compile time); overflow wraps",
    "rustc -C opt-level=3 -C overflow-checks=on": "overflow, bounds, memory, races (compile time)",
    "zig build-exe -O ReleaseFast": "none",
    "zig build-exe -O ReleaseSafe": "overflow, bounds",
    "javac + java": "bounds, memory; overflow wraps",
    "dotnet build -c Release": "bounds, memory; overflow wraps",
    "python3": "bounds, memory; integers do not overflow",
    "ruby": "bounds, memory; integers do not overflow",
    "node": "bounds, memory; numbers are doubles",
    "bun": "bounds, memory; numbers are doubles",
    "swiftc -O": "overflow, bounds, memory",
    "fpc -O3": "none (range and overflow checks off by default)",
    "gnatmake -O2": "overflow, bounds (GNAT's default checks)",
    "gfortran -O2": "none",
    "gfortran -O2 -fcheck=all -ftrapv": "overflow, bounds (run time)",
    "cobc -x -O2": "none (size errors only where ON SIZE ERROR is written)",
    "clang (hand-written aarch64 assembly)": "none",
}


def nbody_expected(steps):
    """Energy before and after `steps` steps of 0.01: the benchmarks game's own figures."""
    after = {1000: "-0.169087605", 5_000_000: "-0.169083134"}[steps]
    return "-0.169075164\n" + after


def bintrees_expected(max_depth, min_depth=4):
    """The binary-trees output, which is arithmetic: a tree of depth d has 2^(d+1)-1 nodes."""
    max_depth = max(max_depth, min_depth + 2)

    def nodes(d):
        return 2 ** (d + 1) - 1

    out = [f"stretch tree of depth {max_depth + 1}\t check: {nodes(max_depth + 1)}"]
    for d in range(min_depth, max_depth + 1, 2):
        n = 1 << (max_depth - d + min_depth)
        out.append(f"{n}\t trees of depth {d}\t check: {n * nodes(d)}")
    out.append(f"long lived tree of depth {max_depth}\t check: {nodes(max_depth)}")
    return "\n".join(out)


def worker_counts():
    """W = 1, 2, 4 and 8, and the host's processors when that is another count."""
    counts = [1, 2, 4, 8]
    cpus = os.cpu_count() or 1
    return counts if cpus in counts else counts + [cpus]


def plan_first(plan, name):
    """The first record key of a program's runs."""
    return next(key for key, n, _ in plan if n == name)


def speedups(record):
    """Each parallel row's speed-up: its own W=1 median over its median, by language and variant."""
    base = {}
    for key, w in record["workloads"].items():
        for r in w["rows"]:
            if key.endswith(" W=1") and r.get("run"):
                base[(key.split(" ")[0], r["language"], r["variant"])] = r["run"]["median_ms"]
    for key, w in record["workloads"].items():
        if " W=" not in key:
            continue
        for r in w["rows"]:
            one = base.get((key.split(" ")[0], r["language"], r["variant"]))
            if one and r.get("run"):
                r["speedup"] = round(one / r["run"]["median_ms"], 2)


def version(cmd, env=None):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=60, env=env)
        text = (out.stdout or out.stderr).strip().splitlines()
        return text[0].strip() if text and out.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def timed(cmd, env=None, cwd=None, timeout=1800):
    start = time.perf_counter()
    out = subprocess.run(cmd, capture_output=True, text=True, env=env, cwd=cwd, timeout=timeout)
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


def peak_rss(cmd, env):
    """Peak resident memory of one run, in KiB, from the system's `time`."""
    if platform.system() == "Darwin":
        out = subprocess.run(["/usr/bin/time", "-l", *cmd], capture_output=True, text=True,
                             env=env)
        for line in out.stderr.splitlines():
            if "maximum resident set size" in line:
                return int(line.split()[0]) // 1024
    else:
        out = subprocess.run(["/usr/bin/time", "-v", *cmd], capture_output=True, text=True,
                             env=env)
        for line in out.stderr.splitlines():
            if "Maximum resident set size" in line:
                return int(line.split()[-1])
    return None


def run_times(cmd, env, runs, warmup, work):
    """Run-time spread: hyperfine when it is installed, else the wall clock of cross.py."""
    if shutil.which("hyperfine"):
        export = work / "hyperfine.json"
        out = subprocess.run(["hyperfine", "--warmup", str(warmup), "--runs", str(runs),
                              "--shell=none", "--style", "none", "--export-json", str(export),
                              shlex.join(cmd)], capture_output=True, text=True, env=env)
        if out.returncode != 0:
            raise RuntimeError(out.stderr.strip()[:300])
        result = json.loads(export.read_text())["results"][0]
        if "times" in result:  # hyperfine 1
            times = result["times"]
        else:  # hyperfine 2
            times = [m["time_wall_clock"]["value"] for m in result["measurements"]]
        export.unlink()
        return spread(times), "hyperfine"
    for _ in range(warmup):
        timed(cmd, env=env)
    return spread([timed(cmd, env=env)[0] for _ in range(runs)]), "wall clock"


def loc_lines(path):
    """Non-blank lines that are not wholly a comment, each without its indentation."""
    ext = path.suffix
    markers = {".py": ("#",), ".rb": ("#",), ".adb": ("--",), ".pas": ("//", "(*"),
               ".f90": ("!",), ".cob": ("*>",)}.get(ext, ("//",))
    out = []
    for line in path.read_text().splitlines():
        s = line.strip()
        if not s or s.startswith(markers):
            continue
        if ext == ".pas" and s.startswith("{") and not s.startswith("{$"):
            continue
        out.append(s)
    return out


def loc(path):
    """Non-blank lines that are not wholly a comment."""
    return len(loc_lines(path))


def chars(path):
    """Characters of the lines `loc` counts, without indentation or trailing space."""
    return sum(len(s) for s in loc_lines(path))


def first_glob(pattern):
    found = sorted(glob.glob(os.path.expanduser(pattern)))
    return found[-1] if found else None


def find_tools():
    """Each language's tools, or None where they are absent."""
    tools = {name: shutil.which(name) for name in
             ["clang", "clang++", "go", "rustc", "zig", "swiftc", "fpc", "dotnet", "python3",
              "ruby", "node", "bun", "gfortran", "cobc"]}
    # The plain `java` on a Mac's PATH may be a stub that only says Java is not installed.
    java = None
    for d in ["/opt/homebrew/opt/openjdk/bin", os.path.dirname(shutil.which("javac") or "")]:
        if d and os.path.exists(os.path.join(d, "javac")) and \
                version([os.path.join(d, "java"), "-version"]) is not None:
            java = d
            break
    tools["java"] = os.path.join(java, "java") if java else None
    tools["javac"] = os.path.join(java, "javac") if java else None
    # The .NET SDK as `dotnet-install.sh` puts it, where nothing adds it to PATH.
    if tools["dotnet"] is None and os.path.exists(os.path.expanduser("~/.dotnet/dotnet")):
        tools["dotnet"] = os.path.expanduser("~/.dotnet/dotnet")
        # An app built there looks for its runtime here, and the SDK sends nothing home.
        os.environ["DOTNET_ROOT"] = os.path.expanduser("~/.dotnet")
        os.environ["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1"
    gnat = shutil.which("gnatmake") or first_glob(
        "~/.local/share/alire/toolchains/gnat_native_*/bin/gnatmake")
    tools["gnatmake"] = gnat
    return tools


def languages(nazm, tools, work, env, goenv, zigenv, gnatenv):
    """Language label -> (directory, file extension, file naming, variants).

    A variant is (label, tool needed, build command or None, run command, environment, flags),
    the commands as functions of the source path and the output path, and optionally a function
    run before every build, outside its time.
    """
    # R1.5, 1.2: Nazm's build measured twice. Uncached, `--no-cache`: every unit and the runtime
    # compiled each build. Cached: a warm user cache (D12), which the unmeasured first build fills
    # and every timed build keeps, beside a project store emptied before each build — so the
    # program's own units are compiled every time and only the runtime's and `@std`'s are reused.
    # Both caches are under OUT_DIR/work, never the user's own.
    user_cache = work / "nazm-user-cache"
    project_cache = work / "nazm-project-cache"
    cached_env = dict(env, NAZM_USER_CACHE_DIR=str(user_cache),
                      NAZM_CACHE_DIR=str(project_cache))

    def cold_project():
        shutil.rmtree(project_cache, ignore_errors=True)

    def exe(name, tag):
        return str(work / f"{name}.{tag}")

    def unit_dir(name):
        # fpc writes its units and assembler files here and does not create it.
        (work / name).mkdir(parents=True, exist_ok=True)
        return work / name

    def dotnet_build(src, out):
        # `dotnet` needs a project: one beside a copy of the source, in the work directory.
        proj = Path(out + ".proj")
        proj.mkdir(parents=True, exist_ok=True)
        shutil.copy(src, proj / Path(src).name)
        major = (version([tools["dotnet"], "--version"]) or "8").split(".")[0]
        (proj / "p.csproj").write_text(
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType>"
            f"<TargetFramework>net{major}.0</TargetFramework><ImplicitUsings>enable"
            "</ImplicitUsings><AssemblyName>p</AssemblyName></PropertyGroup></Project>\n")
        return [tools["dotnet"], "build", str(proj / "p.csproj"), "-c", "Release", "-o",
                out + ".dir", "-nologo", "-v", "q"]

    return [
        ("Nazm", "nazm", "{n}.nz", [
            ("nazm build --opt-level 2", nazm,
             lambda s, n: [nazm, "build", "--no-cache", "--opt-level", "2", s, "-o",
                           exe(n, "nazm")],
             lambda s, n: [exe(n, "nazm")], env,
             "LLVM backend, clang -O2; checked arithmetic and bounds; --no-cache: every unit "
             "and the runtime compiled each build"),
            ("nazm build --opt-level 2 (user cache warm)", nazm,
             lambda s, n: [nazm, "build", "--opt-level", "2", s, "-o", exe(n, "nazmc")],
             lambda s, n: [exe(n, "nazmc")], cached_env,
             "as above, with the user cache (R1.5 D12) warm and the project store empty: the "
             "runtime's and @std's objects reused, the program's own compiled each build",
             cold_project),
            ("nazm run (interpreter)", nazm, None,
             lambda s, n: [nazm, "run", "--interpret", "--max-iterations", "1000000000000", s, "--"], env,
             "tree-walking interpreter"),
        ]),
        ("C", "c", "{n}.c", [
            ("clang -O2", "clang", lambda s, n: ["clang", "-O2", "-o", exe(n, "c"), s],
             lambda s, n: [exe(n, "c")], env, "-O2"),
            ("clang -O2 -fsanitize=address,undefined", "clang",
             lambda s, n: ["clang", "-O2", "-fsanitize=address,undefined", "-o", exe(n, "csan"),
                           s],
             lambda s, n: [exe(n, "csan")], env, "-O2, AddressSanitizer and UBSan: Nazm's checks"),
        ]),
        ("C++", "cpp", "{n}.cpp", [
            ("clang++ -O2", "clang++",
             lambda s, n: ["clang++", "-O2", "-std=c++20", "-o", exe(n, "cpp"), s],
             lambda s, n: [exe(n, "cpp")], env, "-O2 -std=c++20"),
            ("clang++ -O2 -fsanitize=address,undefined", "clang++",
             lambda s, n: ["clang++", "-O2", "-std=c++20", "-fsanitize=address,undefined", "-o",
                           exe(n, "cppsan"), s],
             lambda s, n: [exe(n, "cppsan")], env,
             "-O2 -std=c++20, AddressSanitizer and UBSan: Nazm's checks"),
        ]),
        ("Go", "go", "{n}.go", [
            ("go build", "go", lambda s, n: ["go", "build", "-o", exe(n, "go"), s],
             lambda s, n: [exe(n, "go")], goenv,
             "default (optimised); build cache warm, so a rebuild may reuse the package"),
        ]),
        ("Rust", "rust", "{n}.rs", [
            ("rustc -C opt-level=3", "rustc",
             lambda s, n: ["rustc", "-C", "opt-level=3", "--edition", "2021", "-o",
                           exe(n, "rs"), s],
             lambda s, n: [exe(n, "rs")], env, "-C opt-level=3 (cargo --release)"),
            ("rustc -C opt-level=3 -C overflow-checks=on", "rustc",
             lambda s, n: ["rustc", "-C", "opt-level=3", "-C", "overflow-checks=on", "--edition",
                           "2021", "-o", exe(n, "rss"), s],
             lambda s, n: [exe(n, "rss")], env, "-C opt-level=3, overflow checks on: Nazm's checks"),
        ]),
        ("Zig", "zig", "{n}.zig", [
            ("zig build-exe -O ReleaseFast", "zig",
             lambda s, n: ["zig", "build-exe", "-O", "ReleaseFast", f"-femit-bin={exe(n, 'zig')}",
                           s],
             lambda s, n: [exe(n, "zig")], zigenv, "-O ReleaseFast"),
            ("zig build-exe -O ReleaseSafe", "zig",
             lambda s, n: ["zig", "build-exe", "-O", "ReleaseSafe",
                           f"-femit-bin={exe(n, 'zigs')}", s],
             lambda s, n: [exe(n, "zigs")], zigenv, "-O ReleaseSafe: overflow and bounds checked"),
        ]),
        ("Java", "java", "{N}.java", [
            ("javac + java", "javac",
             lambda s, n: [tools["javac"], "-d", exe(n, "classes"), s],
             lambda s, n: [tools["java"], "-cp", exe(n, "classes"), n.capitalize()], env,
             "javac default; JVM default flags"),
        ]),
        ("C#", "csharp", "{n}.cs", [
            ("dotnet build -c Release", "dotnet", lambda s, n: dotnet_build(s, exe(n, "cs")),
             lambda s, n: [exe(n, "cs") + ".dir/p"], env, "-c Release"),
        ]),
        ("Python", "python", "{n}.py", [
            ("python3", "python3", None, lambda s, n: ["python3", s], env, "CPython, no flags"),
        ]),
        ("Ruby", "ruby", "{n}.rb", [
            ("ruby", "ruby", None, lambda s, n: ["ruby", s], env, "no flags"),
        ]),
        ("JavaScript", "javascript", "{n}.js", [
            ("node", "node", None, lambda s, n: ["node", s], env, "no flags"),
        ]),
        ("TypeScript", "typescript", "{n}.ts", [
            ("bun", "bun", None, lambda s, n: ["bun", s], env, "no flags"),
        ]),
        ("Swift", "swift", "{n}.swift", [
            ("swiftc -O", "swiftc", lambda s, n: ["swiftc", "-O", "-o", exe(n, "swift"), s],
             lambda s, n: [exe(n, "swift")], env, "-O"),
        ]),
        ("Pascal", "pascal", "{n}.pas", [
            ("fpc -O3", "fpc",
             lambda s, n: ["fpc", "-O3", "-v0", f"-FU{unit_dir('fpc-' + n)}",
                           f"-o{exe(n, 'pas')}", s],
             lambda s, n: [exe(n, "pas")], env, "-O3 (Free Pascal)"),
        ]),
        ("Fortran", "fortran", "{n}.f90", [
            ("gfortran -O2", "gfortran",
             lambda s, n: [tools["gfortran"] or "gfortran", "-O2", "-o", exe(n, "f90"), s],
             lambda s, n: [exe(n, "f90")], env, "-O2 (GNU Fortran)"),
            ("gfortran -O2 -fcheck=all -ftrapv", "gfortran",
             lambda s, n: [tools["gfortran"] or "gfortran", "-O2", "-fcheck=all", "-ftrapv", "-o",
                           exe(n, "f90c"), s],
             lambda s, n: [exe(n, "f90c")], env, "-O2, bounds and overflow checked: Nazm's checks"),
        ]),
        ("COBOL", "cobol", "{n}.cob", [
            ("cobc -x -O2", "cobc",
             lambda s, n: [tools["cobc"] or "cobc", "-x", "-free", "-O2", "-o", exe(n, "cob"), s],
             lambda s, n: [exe(n, "cob")], env, "-x -free -O2 (GnuCOBOL, through the C compiler)"),
        ]),
        ("Assembly", "asm", "{n}.s", [
            ("clang (hand-written aarch64 assembly)", "clang",
             lambda s, n: ["clang", "-o", exe(n, "asm"), s],
             lambda s, n: [exe(n, "asm")], env, "hand-written for aarch64 macOS; assembled, linked"),
        ]),
        ("Ada", "ada", "{n}.adb", [
            ("gnatmake -O2", "gnatmake",
             lambda s, n: [tools["gnatmake"] or "gnatmake", "-q", "-f", "-O2",
                           f"-D{work / ('gnat-' + n)}", s, "-o", exe(n, "ada")],
             lambda s, n: [exe(n, "ada")], gnatenv, "-O2 (GNAT), -f: every unit compiled"),
        ]),
    ]


def main():
    args = sys.argv[1:]
    runs = RUNS
    if "--runs" in args:
        i = args.index("--runs")
        runs = int(args[i + 1])
        del args[i:i + 2]
    only = None
    if "--only" in args:
        i = args.index("--only")
        only = set(args[i + 1].split(","))
        del args[i:i + 2]
    # A quick development pass (`--runs 1` or 2) skips the warm-up runs.
    warmup = WARMUP if runs >= 3 else 0
    tiny = "--tiny" in args
    if tiny:
        args.remove("--tiny")
    if len(args) != 2:
        print(__doc__)
        return 2
    nazm = str(Path(args[0]).resolve())
    out_dir = Path(args[1]).resolve()
    work = out_dir / "work"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    tools = find_tools()
    env = {k: v for k, v in os.environ.items() if not k.startswith("NAZM_")}
    goenv = dict(env, GOCACHE=str(work / "gocache"), GOPATH=str(work / "gopath"),
                 GOTOOLCHAIN="local", GOFLAGS="-mod=mod")
    zigenv = dict(env, ZIG_GLOBAL_CACHE_DIR=str(work / "zig-global"),
                  ZIG_LOCAL_CACHE_DIR=str(work / "zig-local"))
    gnatenv = dict(env)
    if tools["gnatmake"]:
        gnatenv["PATH"] = os.path.dirname(tools["gnatmake"]) + os.pathsep + env["PATH"]

    words_txt = work / "words.txt"
    subprocess.run([sys.executable, str(HERE / "gen_words.py"), str(words_txt)], check=True)
    expected = {
        "hello": "Hello, world!",
        "fib": "9227465",
        "words": (HERE / "words.expected").read_text().strip(),
        "primes": "216816",
    }
    steps = TINY["nbody"] if tiny else NBODY_STEPS
    depth = TINY["bintrees"] if tiny else BINTREES_DEPTH
    expected["nbody"] = nbody_expected(steps)
    expected["bintrees"] = bintrees_expected(depth)
    sized = {"nbody": [str(steps)], "bintrees": [str(depth)]}

    def tool_version(name, cmd, venv=env):
        return version(cmd, venv) if tools.get(name) or name == "nazm" else None

    record = {
        "host": {
            "machine": platform.machine(),
            "system": f"{platform.system()} {platform.release()}",
            "os": version(["sw_vers", "-productVersion"]),
            "cpus": os.cpu_count(),
            "processor": version(["sysctl", "-n", "machdep.cpu.brand_string"]),
            "memory_bytes": int(version(["sysctl", "-n", "hw.memsize"]) or 0) or None,
        },
        "date": time.strftime("%Y-%m-%d"),
        "toolchains": {
            "nazm": version([nazm, "--version"]),
            "clang": tool_version("clang", ["clang", "--version"]),
            "go": tool_version("go", ["go", "version"]),
            "rustc": tool_version("rustc", ["rustc", "--version"]),
            "zig": tool_version("zig", ["zig", "version"]),
            "java": tool_version("java", [tools["java"] or "java", "-version"]),
            "dotnet": tool_version("dotnet", [tools["dotnet"] or "dotnet", "--version"]),
            "python3": tool_version("python3", ["python3", "--version"]),
            "ruby": tool_version("ruby", ["ruby", "--version"]),
            "node": tool_version("node", ["node", "--version"]),
            "bun": tool_version("bun", ["bun", "--version"]),
            "swiftc": tool_version("swiftc", ["swiftc", "--version"]),
            "fpc": tool_version("fpc", ["fpc", "-iV"]),
            "gfortran": tool_version("gfortran", ["gfortran", "--version"]),
            "cobc": tool_version("cobc", ["cobc", "--version"]),
            "gnat": tool_version("gnatmake", [tools["gnatmake"] or "gnatmake", "--version"],
                                 gnatenv),
            "hyperfine": version(["hyperfine", "--version"]),
        },
        "method": {
            "warmup": warmup,
            "runs": runs,
            "slow_runs": f"{SLOW_RUNS} when the checking run took over {SLOW_SECONDS} s",
            "build_runs": BUILD_RUNS,
            "clock": "hyperfine --shell=none (stdout discarded) when installed, else "
                     "time.perf_counter around the process with stdout captured",
            "memory": "maximum resident set size of one run, from /usr/bin/time",
            "build": "median of the timed builds after one unmeasured build",
            "loc": "non-blank lines that are not wholly a comment",
            "words_input": "gen_words.py: 2,000,000 words, 5,000-word vocabulary, fixed seed",
        },
        "workloads": {},
    }

    langs = languages(nazm, tools, work, env, goenv, zigenv, gnatenv)
    # Each timed run: (the record's key, the program, its arguments). The parallel workload is one
    # program run at each worker count, built once.
    plan = [(name, name, [str(words_txt)] if name == "words" else sized.get(name, []))
            for name in WORKLOADS]
    plan += [(f"primes W={w}", "primes", [str(w)]) for w in worker_counts()]
    if only is not None:
        plan = [p for p in plan if p[1] in only]
    record["method"]["primes"] = (
        "primes below 3,000,000 by trial division, in blocks of 1,000 dealt round robin to W "
        f"workers; W = {', '.join(str(w) for w in worker_counts())} (the last the host's "
        "processors); the speed-up is each row's median over its own W=1 median")
    builds = {}
    for key, name, extra in plan:
        rows = []
        load_before = os.getloadavg()
        for lang, d, pattern, variants in langs:
            src = HERE / d / pattern.format(n=name, N=name.capitalize())
            for variant in variants:
                label, tool, build, run, venv, flags = variant[:6]
                prepare = variant[6] if len(variant) > 6 else None
                row = {"language": lang, "variant": label, "flags": flags,
                       "safety": SAFETY.get(label, DASH),
                       "source": str(src.relative_to(HERE.parent.parent))}
                print(f"{key}: {lang}, {label}", file=sys.stderr, flush=True)
                if not src.exists():
                    # A language with no parallel program is said once, not at every W.
                    if key == name or key == plan_first(plan, name):
                        row["status"] = "unmeasured: no source"
                        rows.append(row)
                    continue
                row["loc"] = loc(src)
                row["chars"] = chars(src)
                if tool != nazm and not tools.get(tool):
                    row["status"] = f"absent: {tool} is not installed"
                    rows.append(row)
                    continue
                if name in PARALLEL and build is None:
                    if key == plan_first(plan, name):
                        row["status"] = ("unmeasured: the parallel workload is timed on "
                                         "compiled builds only (an interpreter would take "
                                         "minutes a run)")
                        rows.append(row)
                    continue
                if name in INTERPRETER_UNTIMED and label == "nazm run (interpreter)":
                    row["status"] = ("unmeasured: Nazm's tree-walking interpreter takes minutes "
                                     "a run here, and the interpreter row is not ranked")
                    rows.append(row)
                    continue
                srcs = str(src)
                if (lang, label, name) in builds:
                    row.update(builds[(lang, label, name)])
                    if "status" in row:
                        rows.append(row)
                        continue
                elif build is not None:
                    cmd = build(srcs, name)
                    failed = None
                    times = []
                    for i in range(BUILD_RUNS + 1):
                        if prepare is not None:
                            prepare()
                        t, out = timed(cmd, env=venv, cwd=work)
                        if out.returncode != 0:
                            text = (out.stderr.strip() or out.stdout.strip()).splitlines()
                            failed = " / ".join(text[-3:])[:400].replace("|", "/")
                            break
                        if i > 0:
                            times.append(t)
                        else:
                            # The first build of the run, its work directory's caches (Go's,
                            # Zig's, Nazm's user cache) empty: the cold build.
                            cold = t
                    if failed is not None:
                        row["status"] = f"unmeasured: the build failed: {failed}"
                        builds[(lang, label, name)] = {"status": row["status"]}
                        rows.append(row)
                        continue
                    row["build"] = spread(times)
                    row["build_cold_ms"] = round(cold * 1000, 1)
                    row["build_command"] = " ".join(
                        Path(a).name if "/" in a else a for a in cmd)
                    builds[(lang, label, name)] = {"build": row["build"],
                                                   "build_cold_ms": row["build_cold_ms"],
                                                   "build_command": row["build_command"]}
                runc = run(srcs, name) + extra
                exe_path = Path(runc[0])
                if build is not None and exe_path.is_file() and str(exe_path).startswith(
                        str(work)):
                    row["binary_bytes"] = exe_path.stat().st_size
                try:
                    t, out = timed(runc, env=venv)
                except subprocess.TimeoutExpired:
                    row["status"] = "unmeasured: the checking run timed out"
                    rows.append(row)
                    continue
                got = out.stdout.strip()
                if out.returncode != 0 or got != expected[name]:
                    first = got.splitlines()[0] if got else " / ".join(
                        out.stderr.strip().splitlines()[-2:])[:200]
                    row["status"] = (f"unmeasured: exit {out.returncode}, it printed "
                                     f"{first!r}, not the expected output")
                    rows.append(row)
                    continue
                n = min(SLOW_RUNS, runs) if t > SLOW_SECONDS else runs
                try:
                    row["run"], row["clock"] = run_times(runc, venv, n, warmup, work)
                except RuntimeError as e:
                    row["status"] = f"unmeasured: the timer failed: {e}"
                    rows.append(row)
                    continue
                row["peak_rss_kib"] = peak_rss(runc, venv)
                row["status"] = "measured; output checked"
                rows.append(row)
        record["workloads"][key] = {"expected": expected[name], "rows": rows,
                                    "load_before": [round(x, 2) for x in load_before],
                                    "load_after": [round(x, 2) for x in os.getloadavg()]}
    speedups(record)

    (out_dir / "compare.json").write_text(json.dumps(record, indent=2) + "\n")
    md = render(record)
    (out_dir / "compare.md").write_text(md)
    print(md)
    return 0


def render(record):
    h = record["host"]
    mem = f", {h['memory_bytes'] // 2**30} GiB" if h.get("memory_bytes") else ""
    lines = [f"Host: {h['processor']}, {h['cpus']} CPUs{mem}, macOS {h['os']} "
             f"({h['system']}, {h['machine']}); {record['date']}. Not controlled.", ""]
    for name, w in record["workloads"].items():
        parallel = " W=" in name
        lines += [f"### {name}", ""]
        if "load_before" in w:
            lines += [f"Load average (1, 5, 15 min): {w['load_before']} before, "
                      f"{w['load_after']} after.", ""]
        lines += ["| language | variant | checks | LOC | chars | run median (ms) | σ (ms) | "
                  "run min–max (ms) | " + ("speed-up over W=1 | " if parallel else "")
                  + "peak RSS (KiB) | binary (KiB) | build cold (ms) | build warm, median (ms) | "
                  "status |",
                  "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | "
                  + ("---: | " if parallel else "") + "---: | ---: | ---: | ---: | --- |"]
        for r in w["rows"]:
            run = r.get("run")
            build = r.get("build")
            span = f"{run['min_ms']:.1f}–{run['max_ms']:.1f}" if run else DASH
            size = f"{r['binary_bytes'] / 1024:.0f}" if r.get("binary_bytes") else DASH
            median = f"{run['median_ms']:.1f}" if run else DASH
            built = f"{build['median_ms']:.0f}" if build else DASH
            gain = (f"{r['speedup']:.2f}× | " if r.get("speedup") else f"{DASH} | ") \
                if parallel else ""
            sd = f"{run['stdev_ms']:.1f}" if run else DASH
            cold = f"{r['build_cold_ms']:.0f}" if r.get("build_cold_ms") else DASH
            lines.append(
                f"| {r['language']} | {r['variant']} | {r.get('safety', DASH)} | "
                f"{r.get('loc', DASH)} | {r.get('chars', DASH)} | {median} | {sd} | "
                f"{span} | {gain}{r.get('peak_rss_kib') or DASH} | {size} | {cold} | {built} | "
                f"{r['status']} |")
        lines.append("")
    lines += ["Toolchains:", ""]
    for k, v in record["toolchains"].items():
        lines.append(f"- {k}: {v or 'absent'}")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
