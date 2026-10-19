# Performance

This page gives Nazm's latest measured rank on each program and parameter, what the ranks mean,
and how to reproduce them. It is a summary: the full record, with every method, host, caveat and
earlier result, is [`docs/internal/performance-log.md`](../internal/performance-log.md) in the
repository.

**Nazm does not claim to be the fastest language.** Leading every compared language on every
measured parameter is a research target, and the table below shows where that is true and where it
is not. Every figure is a median over repeated runs on one machine (Apple M1 Pro, macOS), taken on
a quiet host, against the same programs written in each language's own idiom. Languages not
installed on that machine are not in the comparison.

## Latest rankings

Eighteen languages (C, C++, Rust, Zig, Go, Swift, Fortran, Ada, Pascal, COBOL, hand-written
assembly, Java, C#, TypeScript on Bun, JavaScript on Node, Python, Ruby, and Nazm). Rank is among
the languages that have a row for that program, each language's best row; lower is better on every
parameter. Nazm is built at `--opt-level 2`; its interpreter is never ranked.

| program | run time | peak memory | binary size | build, cold | build, warm | source lines |
| --- | --- | --- | --- | --- | --- | --- |
| `hello` (start-up) | 1.93 ms, #4 of 18, within 0.07 ms of C and assembly | #1 of 18 (tied) | #5 of 13 | #3 of 14 | #7 of 14 | #1 of 18 |
| `fib(35)` | 44.2 ms, #9 of 18 | #4 of 18 | #4 of 13 | #5 of 14 | #6 of 14 | #8 of 18 |
| word frequency | 103.8 ms, #6 of 16 | #3 of 16 | #5 of 11 | #6 of 12 | #6 of 12 | #6 of 16 |
| primes, 1 worker | 173.3 ms, #3 of 7 | #4 of 7 | #3 of 6 | #4 of 7 | #3 of 7 | #6 of 7 |
| primes, 8 workers | 23.9 ms (7.3x faster than 1 worker), #3 of 7 | #4 of 7 | | | | |
| n-body | 228.5 ms, #3 of 16 | #1 of 16 | #4 of 12 | #3 of 12 | #5 of 12 | #8 of 16 |
| binary-trees | 1,971.7 ms, #13 of 16 | #12 of 16 | | | | |

Among the rows that check integer overflow and array bounds, as Nazm does, Nazm is first on
start-up, second on `primes`, third on `fib`, word frequency and n-body, and fourth on
binary-trees.

## What the ranks mean

- **Where Nazm leads or is close:** process start-up, peak memory (first on `hello` and n-body,
  third on word frequency), source brevity on the small programs, floating-point work (n-body is
  ahead of C, C++, Go and Fortran and behind only Rust and Zig), and the parallel job, where it is
  third, within about 3% of Rust and 12% of Go.
- **Where it is behind:** `fib` against the unchecked C, C++, Rust and Zig rows (the gap is the
  overflow checks Nazm keeps on; checked Rust is within 1%), word frequency against Zig, Rust and
  C, build time against C, Go and Fortran, and binary-trees, the largest gap: allocation of many
  small boxed nodes is 1.7x C's time and 5.5x its memory.
- **Safety is part of the comparison.** A language that does not check overflow, bounds or memory
  runs fewer instructions. The last column of each comparison in the full record ranks only the
  rows that check as Nazm does, and the checks each row keeps are named beside it.
- **Builds** are measured cold (no cache) and warm (user cache filled). Nazm's cold build of the
  word-frequency program is about 4.6x C's; a warm build of an unchanged program takes tens of
  milliseconds.
- **Concurrency** is compared against the languages that have a concurrency model (spawn,
  ping-pong, fan-in, idle tasks, an HTTP server); fan-in with many producers is the known
  weak spot, about five times Go's time.

A rank is a statement about one program on one machine, not a ranking of languages. The programs
are small, and a different workload can order the languages differently.

## Reproducing the numbers

The programs, the other languages' sources and the harness are in the repository's `bench/`
directory. Timing needs a quiet machine (no builds, no other load), so run these yourself on a
host of your own; expect different absolute figures and, on another CPU, possibly different ranks.

```sh
# the cross-language comparison: every parameter in the tables above
python3 bench/compare/compare.py /path/to/nazm out --runs 10

# only some programs
python3 bench/compare/compare.py /path/to/nazm out --runs 10 --only nbody,bintrees

# concurrency against the other languages that have a model for it
bench/compare/concurrency_all.sh /path/to/nazm out --wait
```

`compare.py` refuses to time a row whose output differs from the Nazm program's, and times only the
languages whose toolchains are installed. The harness is
[`hyperfine`](https://github.com/sharkdp/hyperfine); the toolchain versions used are listed in the
full record.
