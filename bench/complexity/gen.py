#!/usr/bin/env python3
"""Generated programs for the compiler complexity audit (performance leadership 7.1).

    python3 bench/complexity/gen.py SHAPE SIZE OUTDIR

writes OUTDIR/main.nz (and, for `modules`, OUTDIR/mK.nz) and prints the line count. Every program
checks, builds and prints one number, so each phase of both compilers can be timed on it:

    lines N     about N lines: functions of ten lines each (a loop, a branch, a call to the
                function before), all called from `main`
    nest D      one function whose body nests D blocks (`if` inside `if`), D deep
    chain K     one expression of K terms joined by `+`, ten to a line
    modules M   M modules of ten functions each, `main` importing every one

Doubling SIZE doubles the input; `bench/complexity/run.sh` times n, 2n, 4n and 8n.
"""
import os
import sys


def fn(name, prev):
    call = f"{prev}(s, 2)" if prev else "s % 7"
    return [
        f"pub fn {name}(a: Int, b: Int): Int {{",
        "    let mut s = a",
        "    let mut i = 0",
        "    while i < b {",
        "        if s % 3 == 0 { s = s + i } else { s = s - 1 }",
        "        i += 1",
        "    }",
        f"    let t = {call}",
        "    (s + t) % 1000003",
        "}",
    ]


def lines(n):
    count = max(1, n // 10)
    out = []
    for k in range(count):
        out += fn(f"f{k}", f"f{k - 1}" if k else None)
    out += ["fn main() {", "    let mut acc = 0"]
    out += [f"    acc = (acc + f{k}({k}, 3)) % 1000003" for k in range(0, count, max(1, count // 100))]
    out += ["    println(acc)", "}"]
    return {"main.nz": out}


def nest(d):
    out = ["fn deep(x: Int): Int {", "    let mut r = 0"]
    for k in range(d):
        out.append("    " * (k + 1) + f"if x > {k} {{")
        out.append("    " * (k + 2) + f"r = r + {k % 10}")
    for k in reversed(range(d)):
        out.append("    " * (k + 1) + "}")
    out += ["    r", "}", "fn main() {", f"    println(deep({d // 2}))", "}"]
    return {"main.nz": out}


def chain(k):
    terms = [f"(x + {i % 97})" for i in range(k)]
    body = ["    let s = " + " + ".join(terms[0:10])]
    for i in range(10, k, 10):
        body[-1] += " +"
        body.append("        " + " + ".join(terms[i:i + 10]))
    out = ["fn sum(x: Int): Int {"] + body + ["    s % 1000003", "}"]
    out += ["fn main() {", "    println(sum(3))", "}"]
    return {"main.nz": out}


def modules(m):
    files = {}
    for j in range(m):
        body = []
        for k in range(10):
            body += fn(f"m{j}_f{k}", f"m{j}_f{k - 1}" if k else None)
        files[f"m{j}.nz"] = body
    out = [f'use "m{j}.nz"' for j in range(m)]
    out += ["fn main() {", "    let mut acc = 0"]
    out += [f"    acc = (acc + m{j}_f9({j}, 3)) % 1000003" for j in range(m)]
    out += ["    println(acc)", "}"]
    files["main.nz"] = out
    return files


def main():
    shape, size, outdir = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    files = {"lines": lines, "nest": nest, "chain": chain, "modules": modules}[shape](size)
    os.makedirs(outdir, exist_ok=True)
    total = 0
    for name, body in files.items():
        with open(os.path.join(outdir, name), "w") as f:
            f.write("\n".join(body) + "\n")
        total += len(body)
    print(total)


if __name__ == "__main__":
    main()
