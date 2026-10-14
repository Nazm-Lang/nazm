# Getting started

From nothing to a checked, tested, formatted, natively compiled program and a small package,
in about ten minutes. Every program on this page is real Nazm (the 1.0 draft, toolchain 0.9.0), and every command shown
with its output is run by `crates/nazm-cli/tests/release_examples.rs`, which fails if this page
stops being true.

## 1. Build the toolchain

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it on first use)
and `clang`, which `nazm build` uses to compile and link. On macOS, the Xcode command-line tools
provide `clang`; on Linux, install your distribution's `clang` package.

```sh
git clone https://github.com/Nazm-Lang/nazm
cd nazm
cargo build --release -p nazm-cli
export PATH="$PWD/target/release:$PATH"
nazm --version
```

`cargo install --path crates/nazm-cli --locked` puts `nazm` in `~/.cargo/bin` instead. The binary
carries the standard library inside it, so it needs no installation directory.

## 2. A first program

Create `hello.nz`:

<!-- file: hello.nz -->
```nazm
fn main(out: OutCap) -> Int {
    print("Hello, Nazm!\n");
    0
}
```

`main` is the entry point, and the `Int` it returns is the program's exit status. The commands
on this page run with `NAZM_RESULT_REPORT` set, which also prints that result after the
program's own output — the `0` below; without it, `nazm run hello.nz` prints only the greeting.
`out: OutCap` is the authority to write to the standard streams. Nothing in Nazm reaches the
outside world without such a value, and the only place one comes from is `main`'s parameters. A
`main` that writes neither an `OutCap` nor an `IoCap` is given one named `out`, so `fn main()`
may print too; every other authority — files, the network, processes, the clock — is written.

<!-- run -->
```console
$ nazm run hello.nz
Hello, Nazm!
0
```

## 3. Check it

`nazm check` type-checks without running anything. Write a file without the capability to
see what it catches — printing comes with `main`, and files do not:

<!-- file: nocap.nz -->
```nazm
fn main() -> Int {
    write_file("hello.txt", "Hello, Nazm!\n");
    0
}
```

<!-- run -->
```console
$ nazm check hello.nz
ok
$ nazm check nocap.nz  # fails
error[N0369]: the built-in `write_file` requires `IoCap` authority, and `main` holds no `IoCap` here
  --> nocap.nz:2:5
  |
2 |     write_file("hello.txt", "Hello, Nazm!\n");
  |     ^^^^^^^^^^ `write_file` is authorised by an `IoCap`
  = help: authority is a value: give `main` an `IoCap` parameter and have each caller pass one. The only source of one is `main`'s parameters
nazm: 1 error
```

Every diagnostic has a stable code; `nazm check --json` prints the same thing as
`nazm.diagnostic/1` for tools.

## 4. Build a native executable

`nazm build` compiles the same program to a native executable — no interpreter and no `nazm`
process at run time. It produces the same output:

<!-- run -->
```console
$ nazm build hello.nz -o hello
hello
$ ./hello
Hello, Nazm!
0
```

There are two code generators, held to the same results. LLVM is the default; Cranelift
generates code in process instead of through clang (`performance.md` has a dated build-time
comparison):

<!-- run -->
```console
$ nazm build hello.nz -o hello-fast --backend cranelift
hello-fast
$ ./hello-fast
Hello, Nazm!
0
```

## 5. Test it

`nazm test DIR` runs every `NAME.nz` that has a `NAME.expected` beside it, interpreted **and**
compiled, and requires both to print exactly what the `.expected` file says.

<!-- file: checks/double.nz -->
```nazm
fn double(n: Int) -> Int {
    n * 2
}

fn main() -> Int {
    double(21)
}
```

<!-- file: checks/double.expected -->
```text
42
```

<!-- run -->
```console
$ nazm test checks
ok    double
nazm: 1 of 1 passed
```

## 6. Format it

There is one layout. `nazm fmt --check` reports a file that differs from it; `nazm fmt` rewrites
the file.

<!-- file: messy.nz -->
```nazm
fn  add(a:Int,b:Int)->Int{a+b}
fn main()->Int{add(2,   3)}
```

<!-- run -->
```console
$ nazm fmt --check messy.nz  # fails
messy.nz
nazm: 1 file would change; run `nazm fmt` without --check
$ nazm fmt messy.nz
messy.nz
nazm: 1 of 1 file rewritten
$ cat messy.nz
fn add(a: Int, b: Int) -> Int { a + b }
fn main() -> Int { add(2, 3) }
```

## 7. More than one file

A file is a module. `use "FILE";` makes the definitions that file marks `pub` nameable here;
everything else in it stays private.

<!-- file: geometry.nz -->
```nazm
pub struct Rect {
    w: Int,
    h: Int,
}

pub fn area(r: Rect) -> Int {
    r.w * r.h
}
```

<!-- file: main.nz -->
```nazm
use "geometry.nz";

fn main() -> Int {
    area(Rect(w: 6, h: 7))
}
```

<!-- run -->
```console
$ nazm run main.nz
42
```

## 8. A package

A program that grows past a few files becomes a package: a `nazm.toml` beside `src/`. `nazm init`
writes one from a template — `cli`, `library`, `server`, `embedded`, `wasm` or `contract` — with a
module, a test and a README:

<!-- run -->
```console
$ nazm init demo --template cli
nazm: created `demo` from the `cli` template (6 files); its README.md says what to run
$ nazm run demo
hello, world
0
$ nazm test demo/tests
ok    greet
nazm: 1 of 1 passed
$ nazm lock demo
nazm: wrote demo/nazm.lock (1 package)
$ nazm build demo -o demo/demo --locked
demo/demo
$ ./demo/demo
hello, world
0
```

`nazm lock` pins every package the program uses, by version and BLAKE3 digest, and `--locked`
refuses to build against anything else. Dependencies are written `use "NAME:path.nz";` and
declared in `nazm.toml`; see `docs/spec.md`, *Packages*.

## Where next

- [`examples/tour/`](../examples/tour/) — records and enums, `Result` and `?`, generics,
  closures and function values, traits, and tasks with declared effects and a `SpawnCap`, each
  checked, run and built under both backends by the test suite;
  [`examples/worker-pool.nz`](../examples/worker-pool.nz) for a larger concurrent program.
- [`guide.md`](guide.md) — the language, a page per idea.
- [`spec.md`](spec.md) — what each construct means, exactly.
- [`support.md`](support.md) — targets and optional features, and what is verified on each.
- [`limitations.md`](limitations.md) — what Nazm does not do.
