# The bootstrap, and what it establishes

`compiler/bootstrap.sh` runs the chain Milestone D of `docs/NAZM_AUTONOMOUS_GOAL.md`
describes, from one compiler source **S**:

1. the retained Rust implementation compiles S into **C1**;
2. C1 compiles S into **C2**;
3. C2 compiles S into **C3**;
4. C2 and C3 are compared, and both compile the conformance corpus.

Each stage writes to its own directory, so no stage can read another's artefacts and a
stale binary cannot stand in for one that was never built. Every stage is checked and
failure propagates — a script that prints success after a failed stage is worse than no
script.

## The result, on the verified configuration

| | |
| --- | --- |
| Host | aarch64-apple-darwin (Darwin arm64) |
| Toolchain | rustc 1.98.1 (pinned), Apple clang 21.0.0 |
| S | `compiler/{lex,parse,analyse,emit}.nz`, 3524 lines |
| **C2 vs C3, emitted IR** | **byte-identical** |
| **C2 vs C3, executables** | **byte-identical**, including the runnable form |
| Conformance | 5 cases, each agreeing across C2, C3 **and the interpreter** |

The IR being identical is the fixpoint: compiling S with a compiler built by Rust and
with a compiler built by itself produces the same output. That is what self-hosting
means, and it is checked rather than asserted.

## What is normalised, and why it is narrow

The macOS linker stamps an `LC_UUID` and embeds the output *filename* in the ad-hoc code
signature. Neither is anything the compiler produced. The script therefore assembles each
stage **twice**: once runnable, and once with `-Wl,-no_uuid` purely to compare — a binary
without an `LC_UUID` is one macOS refuses to load, so the normalised copy is never
executed. Each stage links to the same filename in its own directory, which removes the
second difference.

Nothing about code or data is touched. On this configuration the runnable binaries turn
out to be byte-identical too (`runnable-bytes-differing: 0`), because the UUID clang
computes is a function of the content — so the normalisation removes a difference that
did not arise. It is kept because it *can* arise, and finding out by having the check
fail would be worse.

## What this does **not** establish

- **It is not evidence that the compiler is correct.** A compiler that miscompiles S in a
  way that reproduces the same miscompilation is a fixpoint too. Reproducibility and
  correctness are different claims; the conformance corpus and the differential tests in
  `crates/nazm-cli/tests/selfhost.rs` are what speak to the second, and they compare
  against the Rust implementation and against explicit expectations rather than against
  the Nazm compiler itself.
- **It is one target.** aarch64-apple-darwin with Apple clang. Nothing here says what
  another target would do.
- **S is concatenated, not imported.** The compiler written in Nazm does not resolve
  `use` — that is the driver's job in the Rust implementation and is not yet ported. The
  script concatenates the four files and strips their `use` lines. The same text compiles
  either way, and `nazm check` accepts both, but a reader should know the compiler is not
  yet reading a multi-file program the way `nazm build` does.
- **The Rust implementation is still required**, to build C1 and to run `nazm check`
  inside the script. `docs/NAZM_AUTONOMOUS_GOAL.md` §8 keeps it deliberately: it is the
  bootstrap toolchain and the semantic oracle, and deleting it after the first successful
  build is exactly what the contract forbids.
- **External dependencies remain**: `clang` assembles the IR and links, and libc supplies
  `malloc`, `realloc`, `memcmp`, `memcpy`, `snprintf`, `write`, `exit` and the `stdio`
  file functions. Emitting LLVM and invoking an external assembler is permitted and
  documented; it is not hidden.

## Reproducing it

```sh
cargo build -p nazm-cli      # the Rust bootstrap
./compiler/bootstrap.sh      # writes target/bootstrap/
cat target/bootstrap/record.txt
```

The record names the date, host, toolchain versions, commit, whether the tree was clean,
the source digest and line count, both compiler digests, and the conformance count.
