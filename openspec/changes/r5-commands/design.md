## Context

Every `nazm` command is today a Rust function in `crates/nazm-cli` over the Rust front end and
back ends. R5 writes each again in Nazm, as a program under `toolchain/commands/`, built by the
0.10.0 release (the shared rule for every rewrite track) and compared with the Rust command. R2's
lexer (`compiler/lex.nz`) and parser (`compiler/syntax.nz`) agree with the reference on every input;
R2's checker (`compiler/sema.nz`) carries a growing share of them. R2 owns `compiler/`: R5 reads
those files with `use "../../compiler/…"` and never edits them.

## Goals / Non-Goals

**Goals:** each command, once ported, behaves exactly as the Rust one — standard output, standard
error, exit status and every file it writes — on every input the comparison has; and where it does
not carry something, it refuses rather than guesses.

**Non-goals:** changing what any command does (a behaviour change lands in the Rust command and the
Nazm one together, with its comparison); the front end itself (R2); the pipeline, interpreter and
code generator (R3, R4, R9), which the commands that build or run programs will call.

## Decisions

**D1 — One program per command, then one dispatcher.** `toolchain/commands/<command>.nz` is a
program taking the command's own arguments (`fmt [--check] FILE...`, as after `nazm fmt`). The
command-line grammar — one `nazm` binary with `--help`, `-V` and clap's usage errors — is the last
item (proposal, 10); until then the comparison calls the Rust `nazm fmt …` and the Nazm `fmt …`.

**D2 — `fmt` is a port of `nazm_syntax::format` over `compiler/lex.nz`'s tokens.** The reference
formatter works on tokens and comments, not on the tree (`format.rs`, *It works on tokens, not on
the tree*): it merges the comments back in by position, keeps which blocks the author wrote on one
line, and decides spacing from the tokens on either side. The concrete syntax tree is not used, so
`fmt` needs the lexer only. `lex_all` does not report comments; in a file that lexes, the bytes
between two tokens are only ASCII whitespace and comments, so `fmt` scans each gap for `/* … */`
(nesting) and `//` (to the line's end, the newline excluded) — the spans the reference's lossless
lexer gives. Comment text is `trim_end`ed with Rust's `char::is_whitespace`, multi-byte spaces
included. The printer is the reference's state machine with its stacks as `Ints` and its output as
chunks, a line break always a chunk of its own, so "ends with a line break" and "take that back"
are the last chunk.

**D3 — A file that does not lex is rendered as `Diagnostic::render` renders it.** The reference
prints the lexer's diagnostics for that file (`SourceMap::single(path, text)`), formats the others,
and then fails with `nazm: some files could not be formatted because they do not lex` — without the
listing of changed files. `fmt` renders `compiler/diag.nz`'s records the same way: header, `-->`
with line and column (characters, not bytes), the source line, the caret clamped to it, secondary
labels, help, and each fix as `fix:` or `suggestion:` with its precondition.

**D4 — Reading a file is the reference's `read_to_string`.** A missing file, or one that is not
UTF-8, is `nazm: cannot read PATH` and exit 3, at once, after any earlier file was already
written. `fmt` validates UTF-8 itself (no overlong forms, no surrogates, nothing past U+10FFFF),
since `read_file` returns any bytes.

**D5 — What is not carried refuses with exit status 6.** An option other than `--check`, or no
file at all, is a usage error clap words and `nazm` exits 5 for; `fmt` does not reproduce clap's text
yet, so it says `fmt: not carried: …` and exits 6. A directory named as a file is not detected
(`read_file` fails at run time with `N0407`, where the reference says `cannot read`); the
comparison has none. Every other path of `crates/nazm-cli/src/fmt.rs` is ported.

**D6 — The comparison (`toolchain/commands/tests/run.sh`).** The reference is `nazm` built from
the same checkout (`CARGO_TARGET_DIR=$HOME/.cache/nazm-target-track5`, `-j 4`; `NAZM_REF` to name
another). The inputs are the repository's tracked `.nz` files — which include
`toolchain/frontend-tests/inputs`, R2's 2,388 programs — and `toolchain/commands/tests/inputs`
(edge cases: an empty file, a comment alone with CRLF, Unicode spaces ending comments, an
unterminated comment, a file that is not UTF-8). For each, a copy is run through each side twice —
`--check`, then a rewrite — and a record of both runs' exit status, standard output and standard
error and the file afterwards is compared byte for byte. Then one `--check` over every input at
once (the multi-file listing and summary), and a run naming a missing file after one that is
rewritten. One summary line: `fmt: N agree, M differ, R refused (of T inputs); batch agree|differ`.

**D7 — The comparisons of later commands** follow D6: a temporary copy per input, both sides' whole
observable behaviour recorded and compared, refusals counted apart. `check` compares text and
`--json`; `doc` its `nazm.api-doc/2` line and `-o` files; `init` the tree written; `test` and the
commands that build or run programs compare after R3/R4/R9 are integrated; `lsp` and the MCP server
by a scripted session of requests and the responses, byte for byte.

## Risks / Trade-offs

- **Two formatters until the Rust one retires.** A change to `format.rs` must be ported too; the
  comparison over every input catches a drift at once.
- **The gap scan relies on the lexer agreeing** with the reference (R2's `tokens` comparison). If
  `lex.nz` ever accepted what the reference refuses, `fmt` would format a file the reference
  refuses to; the comparison's lexing-error inputs guard it.

## Future work (recorded, not done)

- **`fmt` drops a statement's `;`** (decided by the copyright holder, 2026-10-15) once the
  repository's own sources may use the new syntax — semicolon-free lines (R1, 5b.19) in the code the
  toolchain compiles itself from. Then the formatter stops being "the same tokens" (`format.rs`,
  *What it guarantees*): a `;` that ends a statement at a line's end is removed, one between two
  statements on one line is kept, and the guarantee becomes "the same tokens but those `;`". It
  lands in both formatters together, with a comparison of its own.
