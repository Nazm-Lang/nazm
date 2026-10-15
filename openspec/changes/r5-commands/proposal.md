## Why

`rewrite-in-nazm` (R5, task 6.1) asks for every `nazm` command written in Nazm, so that the toolchain
the project publishes at 1.0.0 is Nazm all the way down and the Rust workspace stays only as the
frozen seed. R2's parser (`compiler/syntax.nz`, with `compiler/lex.nz`) now agrees with the Rust
front end on every input — tokens, tree and syntax diagnostics — and its checker is growing group
by group, so the commands that read source can be written now, each against the Rust command it
replaces.

## What Changes

The commands are ported one at a time, each a program under `toolchain/commands/` compared with
the Rust `nazm` built from the same checkout, byte for byte, over every input
(`toolchain/commands/tests/run.sh`). The order is by what each needs of the front end:

1. **`fmt`** — needs the lexer only (the reference formats tokens and comments, not a tree).
2. **`doc`** — the parser and the checker's public definitions (`nazm.api-doc/2`).
3. **`init`** — needs nothing of the front end: templates written out, then `nazm check` of them.
4. **`check`** (text and `--json`) — R2's checker, as it grows; the command is its driver.
5. **`test`** — `check`, then running the cases: the interpreter (R4) and the build (R3, R9).
6. **`fix`** — `check`'s fixes applied to the files.
7. **`lsp`** — the Language Server over stdin/stdout: `check`'s diagnostics, `fmt`, positions.
8. **The agent commands and the MCP server** — `context`, `snapshot`, `delta`, `diagnostics`,
   `docs`, `repo`, `patch`, `resolve`, `references`, `inspect`, `interface`.
9. **Packages** — `lock`, `update`, `publish`, `yank`, `attest`.
10. **The rest** — `bindgen`, `contract`, `accel`, `bench`, `profile`, `explain-cost`,
    `explain-flow`, `obligations`, `trace`, `reload-check`, `repl`, `comptime`, `capabilities`, and
    the command-line grammar itself (one `nazm` dispatching to them, `--help` included).

Each command is carried whole or refuses: where the port meets something it does not carry yet, it
says so on standard error and exits with status 6, never with an output the reference would not
give. A refusal is counted apart from an agreement in the comparison.

**Recorded for later, not done now:** the copyright holder decided that `nazm fmt` will drop a
statement's `;` once the repository's own sources may use the new syntax (semicolon-free lines,
R1 5b.19, in the code the toolchain compiles itself from). Until then `fmt` keeps every token, as
the reference does; the change lands in both formatters together, with its own comparison.

## Capabilities

### New Capabilities
- `commands-in-nazm`: what each command written in Nazm must agree with the Rust command on.
  Archived with `--skip-specs`; the normative text lands in the shared documents when R5 closes.

### Modified Capabilities
None.

## Impact

`toolchain/commands/` (new: one program per command and `tests/run.sh`). It reads
`compiler/lex.nz`, `compiler/diag.nz` and later `compiler/syntax.nz` and the checker, and never
changes them (R2 owns `compiler/`). Nothing in `crates/`, `library/` or `docs/` changes here; the
coordinator folds the comparisons into Rust tests and the decisions into the shared documents at
integration.
