#!/usr/bin/env python3
"""R1, 5b.10: three small programs in fifteen languages, timed on the host (docs/performance.md).

Each workload is written once per language under bench/compare/<lang>/, idiomatically and as
short as is natural there:

    hello  prints `Hello, world!`
    fib    recursive fib(35), printing 9227465
    words  counts the words of a generated file with the language's standard hash map and prints
           the number of distinct words, then the ten most frequent as `word count`, most frequent
           first, ties by word ascending (words.expected; the input is gen_words.py's)

Every program is built with its language's usual release flags (recorded in the output) and must
print the expected output before it is timed; one that does not is recorded as not measured. A
toolchain that is absent is recorded as absent, never skipped silently.

    python3 bench/compare/compare.py NAZM OUT_DIR [--runs N]

NAZM is the `nazm` binary to measure. Results go to OUT_DIR/compare.json and OUT_DIR/compare.md;
builds (run in OUT_DIR/work, since gnatmake writes objects to its working directory), caches
and the generated input go to OUT_DIR/work. Nothing is written outside OUT_DIR.

Measured for each program: run time (`hyperfine --warmup 2 --runs N --shell=none` when hyperfine
is on PATH, else a wall clock around the process, as bench/cross.py), peak resident memory of one
run (`/usr/bin/time`), the size of the executable where there is one, build time (the median of
BUILD_RUNS builds after one unmeasured build, so a toolchain's caches of its own standard library
are warm), and lines of code (non-blank lines that are not wholly a comment). The host is not
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
WORKLOADS = ["hello", "fib", "words"]
DASH = "—"


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


def loc(path):
    """Non-blank lines that are not wholly a comment."""
    ext = path.suffix
    if ext in (".py", ".rb"):
        markers = ("#",)
    elif ext == ".adb":
        markers = ("--",)
    elif ext == ".pas":
        markers = ("//", "(*")
    else:
        markers = ("//",)
    n = 0
    for line in path.read_text().splitlines():
        s = line.strip()
        if not s or s.startswith(markers):
            continue
        if ext == ".pas" and s.startswith("{") and not s.startswith("{$"):
            continue
        n += 1
    return n


def first_glob(pattern):
    found = sorted(glob.glob(os.path.expanduser(pattern)))
    return found[-1] if found else None


def find_tools():
    """Each language's tools, or None where they are absent."""
    tools = {name: shutil.which(name) for name in
             ["clang", "clang++", "go", "rustc", "zig", "swiftc", "fpc", "dotnet", "python3",
              "ruby", "node", "bun"]}
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
    the commands as functions of the source path and the output path.
    """
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
            ("nazm run (interpreter)", nazm, None,
             lambda s, n: [nazm, "run", "--max-iterations", "1000000000000", s, "--"], env,
             "tree-walking interpreter"),
        ]),
        ("C", "c", "{n}.c", [
            ("clang -O2", "clang", lambda s, n: ["clang", "-O2", "-o", exe(n, "c"), s],
             lambda s, n: [exe(n, "c")], env, "-O2"),
        ]),
        ("C++", "cpp", "{n}.cpp", [
            ("clang++ -O2", "clang++",
             lambda s, n: ["clang++", "-O2", "-std=c++20", "-o", exe(n, "cpp"), s],
             lambda s, n: [exe(n, "cpp")], env, "-O2 -std=c++20"),
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
        ]),
        ("Zig", "zig", "{n}.zig", [
            ("zig build-exe -O ReleaseFast", "zig",
             lambda s, n: ["zig", "build-exe", "-O", "ReleaseFast", f"-femit-bin={exe(n, 'zig')}",
                           s],
             lambda s, n: [exe(n, "zig")], zigenv, "-O ReleaseFast"),
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
    # A quick development pass (`--runs 1` or 2) skips the warm-up runs.
    warmup = WARMUP if runs >= 3 else 0
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
    }

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
    for name in WORKLOADS:
        rows = []
        extra = [str(words_txt)] if name == "words" else []
        for lang, d, pattern, variants in langs:
            src = HERE / d / pattern.format(n=name, N=name.capitalize())
            for label, tool, build, run, venv, flags in variants:
                row = {"language": lang, "variant": label, "flags": flags,
                       "source": str(src.relative_to(HERE.parent.parent))}
                print(f"{name}: {lang}, {label}", file=sys.stderr, flush=True)
                if not src.exists():
                    row["status"] = "unmeasured: no source"
                    rows.append(row)
                    continue
                row["loc"] = loc(src)
                if tool != nazm and not tools.get(tool):
                    row["status"] = f"absent: {tool} is not installed"
                    rows.append(row)
                    continue
                srcs = str(src)
                if build is not None:
                    cmd = build(srcs, name)
                    failed = None
                    times = []
                    for i in range(BUILD_RUNS + 1):
                        t, out = timed(cmd, env=venv, cwd=work)
                        if out.returncode != 0:
                            text = (out.stderr.strip() or out.stdout.strip()).splitlines()
                            failed = " / ".join(text[-3:])[:400].replace("|", "/")
                            break
                        if i > 0:
                            times.append(t)
                    if failed is not None:
                        row["status"] = f"unmeasured: the build failed: {failed}"
                        rows.append(row)
                        continue
                    row["build"] = spread(times)
                    row["build_command"] = " ".join(
                        Path(a).name if "/" in a else a for a in cmd)
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
        record["workloads"][name] = {"expected": expected[name], "rows": rows}

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
        lines += [f"### {name}", "",
                  "| language | variant | LOC | run median (ms) | run min–max (ms) | "
                  "peak RSS (KiB) | binary (KiB) | build median (ms) | status |",
                  "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |"]
        for r in w["rows"]:
            run = r.get("run")
            build = r.get("build")
            span = f"{run['min_ms']:.1f}–{run['max_ms']:.1f}" if run else DASH
            size = f"{r['binary_bytes'] / 1024:.0f}" if r.get("binary_bytes") else DASH
            median = f"{run['median_ms']:.1f}" if run else DASH
            built = f"{build['median_ms']:.0f}" if build else DASH
            lines.append(
                f"| {r['language']} | {r['variant']} | {r.get('loc', DASH)} | {median} | "
                f"{span} | {r.get('peak_rss_kib') or DASH} | {size} | {built} | "
                f"{r['status']} |")
        lines.append("")
    lines += ["Toolchains:", ""]
    for k, v in record["toolchains"].items():
        lines.append(f"- {k}: {v or 'absent'}")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
