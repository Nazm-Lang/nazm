# The Nazm 1.0 compatibility corpus

`docs/user/stability.md` promises: **a program valid under stable Nazm 1.0, using only stable public
surfaces, does not silently change meaning under a later 1.x toolchain.** This directory is the
part of that promise a machine holds a toolchain to. `crates/nazm-cli/tests/compat_v1.rs` runs it
in the ordinary workspace suite, on every 1.x toolchain.

| directory | a case | what every 1.x toolchain must do |
| --- | --- | --- |
| `run/` | `NAME.nz`, or `NAME/` holding `main.nz` or a package's `nazm.toml` | check it, and print exactly `NAME.stdout` under `nazm run` and from executables built by the LLVM and Cranelift backends |
| `trap/` | `NAME.nz`, whose first line is `// expect: CODE` | check it, and stop with exit status 2 and `CODE` interpreted and compiled |
| `refuse/` | `NAME.nz`, whose first line is `// expect: CODE` | refuse it — `nazm check` exits 1 with `CODE` as its first error — and build nothing |
| `check/` | `NAME.nz` | check it |

The cases cover the stable language surface: integers and their boundaries, shadowing, evaluation
order, records, enums and `match`, generics and `Vec[T]`, equality, `Result`, `Option` and `?`,
function values and closures, traits and methods, control flow, strings as bytes, the memory
contract (values, handles, capture by copy), effects, capabilities and attenuation, effect
parameters, provenance and declassification, structured concurrency with generic channels and
selection, modules and re-exports, packages, the standard library, the C boundary, and the
refusals that keep absent constructs absent. Gate 2 added numbers, arrays and constants, bytes
and text, the collections and binary encoding, the seeded generator, attributes and conditional
compilation, the shape of a test, and exports of every number. R1 added its sections when they
became stable at 0.10.0, in the `run/r1-*` cases: lines without `;`, `else if`, compound
assignment, match guards, short variants, methods and indexes by name, maps by index, one list
type, patterns, tuples, string forms and interpolation, `for`, `Box`, `Cell`, `defer`, derived
ordering, hashing and JSON, constant tables, and R1e's libraries.

## The rules

- **Append-only within 1.x.** A case may be added; none is removed or edited. Every file is
  pinned by its BLAKE3 digest in `BLAKE3SUMS`, so an edit fails the test until the pin is
  changed too, and a changed pin is a reviewed change.
- **A refusal a later 1.x supersedes is kept, and held to the opposite.** `docs/user/stability.md`
  makes adding a construct 1.0 refused by name a minor addition, because no valid program
  changes meaning. Such a case is listed in `SUPERSEDED` with the addition that superseded it,
  stays in the corpus unedited and pinned, and must then check. `refuse/float-literal.nz` is
  the first: Gate 2 added floating point before the first 1.0.0 release.
- **An edited expectation is a changed meaning.** That is a 2.0 change, or the fix of a defect
  in the 1.0 toolchain, recorded in the release record that makes it with the case named. A
  change of diagnostic *text* is not one: cases name codes, never messages.
- **Only stable surfaces.** A case uses nothing `docs/user/stability.md` classifies as experimental
  or internal; an experimental facility is tested where it lives, not here.
- **Expectations come from the specification.** Each case says which section of `docs/user/spec.md`
  it holds the toolchain to, and its expected output was worked out from that section, not
  copied from whatever the toolchain printed.

A new case is added with its pin: run the corpus test, which names the unpinned file, and add
`DIGEST  PATH` — the file's BLAKE3 digest, two spaces, its path from this directory.

**Edited before 1.0.** R1.5's D6 (2026-10-15) made `[a, b, c]` a list wherever no `[T; n]` is
expected, a changed meaning recorded as a breaking change in
`openspec/changes/r1-5-performance-and-dx/design.md`. `run/arrays.nz`,
`refuse/constant-index-out-of-range.nz` and `refuse/array-of-strings.nz` now write the array type
where they bind an array; their expectations are unchanged, and their pins were changed with them.
