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

**D1 — One module per command, one dispatcher.** `toolchain/commands/<command>.nz` declares the
command's arguments (`<command>_cli`, read by the shared parser, D8, which prints `nazm <command>`
in its usage and help as the reference does) and carries it out (`<command>_command`).
`toolchain/commands/nazm.nz` is the one program: the top level (D12), then the command, its own
arguments read from the one after its name (`cli_from`). The comparison runs both sides the same
way, `nazm CMD ARGS`. (Until 2026-10-15 each command was a program of its own, compared as
`fmt …` against `nazm fmt …`.)

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

**D4 — Reading a file is the reference's `read_to_string`.** A missing file, one that does not
open, a directory (whose read fails with `IsADirectory`), or a file that is not UTF-8 is
`nazm: cannot read PATH` and exit 3, at once, after any earlier file was already written. `fmt`
asks `os_stat` and `os_open_read` first, since `read_file` traps (`N0407`) where `read_to_string`
returns an error, and validates UTF-8 itself (no overlong forms, no surrogates, nothing past
U+10FFFF), since `read_file` returns any bytes.

**D5 — What is not carried refuses with exit status 6.** A command meeting what its port does not
carry says `CMD: not carried: …` on standard error and exits 6 — never an output the reference
would not give — and the comparison counts it apart. Since the shared parser (D8), `fmt` and `init`
carry every path of `crates/nazm-cli/src/fmt.rs` and `init.rs`, so neither refuses anything today.

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
`--json`; `doc` its `nazm.api-doc/2` line and `-o` files; `init` the tree written (D9); `test` and
the commands that build or run programs compare after R3/R4/R9 are integrated; `lsp` and the MCP
server by a scripted session of requests and the responses, byte for byte. Every ported command's
command lines are compared too (D8): `run.sh`'s `args` part runs each line of
`tests/args/CMD.txt` on both sides in the same fixture directory — the output, the error, the status
and every file left afterwards — and prints `args: N agree, M differ, R refused (of T command
lines)`.

**D8 — The command line is clap's, ported once (`toolchain/commands/cli.nz`).** The reference reads
its arguments with clap 4.6 (`crates/nazm-cli/src/cli.rs`, derive), and a usage error or `--help`
is part of what a command prints. `cli.nz` ports the part of `clap_builder` the commands use: the
parser (long options with and without `=`, short clusters with attached values, `--`, values pending
until the next argument and resolved as clap resolves them — including the errors clap drops with
`let _ =` — and `PathBuf`'s refusal of an empty value); the validator's required arguments; the
usage strings, plain for help and "smart" — the arguments used — for errors; the error messages
with their tips, the similar long option chosen by `strsim::jaro` above 0.7 computed in `Float64` as
the reference does; and the help template, short and long (`-h` and `--help` differ only when some
help text has a long form), aligned as clap aligns it at the 100 columns it assumes without a
terminal, unwrapped (the reference is built without `wrap_help`). Help is standard output and status
0, an error standard error and status 5 (`exit::USAGE`; `main.rs`, `parse_cli`). A command declares
its arguments in `cli.rs`'s order with the text clap derives from their doc comments — the first
paragraph, its final period dropped, as `about`, and the whole as `long_about` when there is more
than one paragraph. Subcommands, aliases, possible values, groups, conflicts, environment variables,
other `num_args` and hyphen values are not carried yet: no command ported so far declares them.

**D9 — `init` writes the templates the Rust binary compiles in.** `init.rs` holds its templates with
`include_str!`; the Nazm command holds the same files as string literals in
`toolchain/commands/init_templates.nz`, generated from `crates/nazm-cli/templates` by
`tests/gen-templates.sh` (one literal per line, so the build stays fast), and `run.sh` fails when the
committed file is not the generator's output. The refusals come in `init.rs`'s order and words: an
unknown template (5), a path whose last component is not a name — Rust's `Path::file_name` — (5),
a name that is not a package's (`nazm_package::valid_name`, 1), a directory that exists and is not
empty (1). Directories are made as `create_dir_all` makes them (`NotFound` climbs to the parent,
another failure stands unless the path is a directory) and files written as `fs::write` (created
or truncated), so a failure is `nazm: cannot write PATH: MESSAGE (os error N)`, status 4 — the
system's message from `os_error_text`, as `io::Error` displays it. The comparison runs every template
into a fresh directory, a nested one, an empty one and one written `./NAME/`, and `args/init.txt`
covers the refusals, a regular file in the way and every option form.

**D10 — `doc` waits for what R2's checker publishes.** `nazm doc` (`apidoc.rs`) documents a
program only after it checks completely, from the checker's own facts; a port must refuse wherever
the checker stops, and it needs from `compiler/` what R2 does not expose yet:

1. A type as `Types::show` writes it (`params`, `ret`, `capabilities`), generics and capabilities
   included.
2. Each function's required effects (`inferred_effects`), beside the declared set `fneff` holds, and
   both as `inspect::effects` names them.
3. Durable identities: a function's as `cir::Names::function` writes it, a record's and an enum's as
   `Names::ty`, a trait's as `Names::module` and its name, and each module's key (`graph.key`).
4. Per function: whether it is a closure, its foreign symbol (`foreign`) and C export (`export`),
   whether it has type parameters, its visibility, and its declaration's spans — `pub`, the whole
   declaration, and where the body starts — with its attributes (`@deprecated` and its reason).
5. Per record and enum: the `pub` span, the whole span and the attributes; per enum variant its
   field names (records' field type spans are held already).
6. Each `pub use` (N103): its path, its list with aliases, and what each item resolves to — a
   function (`res.callee`), a record or an enum (`res.type_name`), or a trait (`traits().mentions()`).
7. Whether the whole compilation is carried (`ck_stopped` empty) — `doc` refuses otherwise — and
   its diagnostics rendered for several files (`crate::emit`), for a program that does not check.

Outside `compiler/` it also needs a package's resolution (`nazm_package::resolve`: the manifest, the
dependencies, the source root) and the source root's rule (`root::decide`); those are R5's own to
port, after the checker's facts. Until 1–7 are public, `doc` is not started.

**D11 — `check` is `checking.rs` over R2's front end, and refuses where that stops.**
`toolchain/commands/check.nz` drives the loader, the parser, the attributes and the checker as
`compiler/diagnose.nz` does — reading them with `use`, changing none — and then writes what
`checking::run` writes: the warnings first (`crate::emit`), then `ok` on standard output (0), or
`nazm: N warnings, refused by --deny-warnings` (1), or the errors and `nazm: N errors` (1); a
compilation that does not load is its loader's diagnostics alone, 3 when every one is `N0205`,
else 1. The human form is `Diagnostic::render` over the whole compilation's source map: a span is
published at its offset in the concatenation of the files, so its file is the one whose text
holds it — except a syntax diagnostic, whose file is known — and where an offset is also the end
of the file before (a span the reference could have meant there), the command refuses rather than
pick. `prepare.rs` comes first: a directory without a manifest is `nazm: \`DIR\` is a directory
with no \`nazm.toml\`, so it is not a package` (3), `--locked` outside a package is 5, and
`--features` outside one is 5 after the load. Not carried, each refused with status 6 before
anything else is written: whatever the checker stops at (its own reason, `ck_stopped`), a package
or a `nazm.root` marker at or above the file (found by walking `..` from its directory — the root
decision, the package loader and the profiles a manifest names are not ported), an import of a
package (`N0508`), `--source-root`, `--profile`, `--profile-report`, `--cache-report` (the cache's
tally) and `--summary-json` (it needs the language service's diagnostic identities and the BLAKE3
source digest of the durable paths, which need the source root decided), a root module that is
not a readable UTF-8 file, and an empty `--features` value. The check cache (`.nazm/check/`) is
derived data — deleting it changes no program's meaning, which the reference's equivalence tests
hold — so the port neither reads nor writes it: a run is `--no-cache`'s, with the same output.
The standard library is read from the directory `NAZM_LIBRARY` names (the reference compiles the
same files in); without it the command refuses. `cli.nz` gains what `check`'s arguments need:
`conflicts_with_all` (clap's `validate_conflicts`, before the required arguments, its usage the
arguments used but the conflicting ones) and an option given again with its values split at a
delimiter (`--features`, `ArgAction::Append` with `value_delimiter`). The comparison runs every
input from the repository's root with `--no-cache`, text and `--json`, and `--deny-warnings` where
the reference warns, and every command line of `tests/args/check.txt`; the reference's cache, where
a command line uses it, goes to a scratch `NAZM_CACHE_DIR`.

**D12 — The top level is clap's, for a command that requires a subcommand.** `nazm.nz` ports what
clap does with `Cli` before a command sees its arguments, as `nazm` 0.10.0 prints it: `--help` the
long help (`long_about`, then `Usage: nazm <COMMAND>`, every command with its `about` aligned at
the longest name, and the options with their help on the next line), `-h` the short one (`about`,
the options aligned), `-V` and `--version` `nazm 0.10.0`, each the moment it is read — so
`--help --bogus` is help and `--bogus --help` an error — and `--help=VALUE` a usage error whose
usage names `--help`. An unknown option is `unexpected argument`, with the similar one of `--help`
and `--version` (`strsim::jaro` above 0.7) as a tip and in the usage; an argument that names no
command is `unrecognized subcommand`, with the similar command as a tip; after `--` a command's
exact name is refused as a value with the tip to remove the `--`; no command at all prints the
short help on standard error, status 5 (`arg_required_else_help`). `help` takes names, not
options: none is the long help, a command its `--help`, `help help` its own text, and a name after
a command that has no commands of its own is `unrecognized subcommand` under that command's usage
(under `help`'s own, without the closing line). The commands and their `about`s are a table in
`nazm.nz`, in `cli.rs`'s order with `help` last; a doc comment changed in `cli.rs` shows as a
difference in `tests/args/nazm.txt`'s lines. `fmt`, `init` and `check` are carried; any other
command, or `help` of one, is `nazm: not carried: nazm CMD`, status 6.

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
