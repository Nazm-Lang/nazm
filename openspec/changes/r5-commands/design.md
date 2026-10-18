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
port, after the checker's facts. Until 1–7 are public, `doc` is not started. (2026-10-15: R2
publishes 1–7 as `compiler/facts.nz`, its design D24, with the package resolution and the root's
rule in `compiler/package.nz`; `doc` is D13.)

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
difference in `tests/args/nazm.txt`'s lines. `fmt`, `init`, `check`, `doc`, `fix`, `lsp` and `test` are carried; any other
command, or `help` of one, is `nazm: not carried: nazm CMD`, status 6.

**D13 — `doc` is `apidoc.rs` over R2's facts; `check` runs on the same interface.**
`toolchain/commands/doc.nz` documents what `apidoc::document` documents, read from
`compiler/facts.nz` alone: each compilation checked as a module (`fx_check_module`), its exported
non-closure functions, `pub` records and enums and `pub use` items of the program's own modules
with a durable key, keyed by module and identity in `BTreeMap` order (bytes), a later item of one
identity replacing an earlier. Each item is a JSON object written with sorted keys, as serde_json's
default map writes it, and its Markdown block is made beside it; `-o DIR` writes the pretty form
(`{:#}`, two spaces) and `api.md` after `create_dir_all`, `nazm: cannot write DIR: …` (4) on a
failure. The `description` is the comment lines above the declaration (`str::lines`, `trim` by
`char::is_whitespace`), the anchor the identity with every character but an ASCII letter or digit a
`-`, a source's path relative to the base (`canonicalize` of both, through `os_realpath`). A
directory is `nazm_package::resolve`'s: no manifest is `nazm: no manifest \`DIR/nazm.toml\`` (1), a
manifest that is not one is its reason after the manifest's path (1), and every `.nz` file under the
canonical source root — not through a link to a directory — sorted as `PathBuf` sorts (component by
component), each the root of its own compilation; the `package` object is the root package's name
and version and every package's profile, from `compiler/package.nz`, and `null` for a file outside
a package. A compilation that does not load is `cannot load PATH: MESSAGES` (1); one that fails is its
errors rendered (`dg_render`), then `N errors — a program that does not check has no API to
document` (1). Not carried, refused with 6: where the front end stops (`fx_why`), a manifest
`package.nz` does not read, a package it does not resolve, a source root that cannot be read.

`fx_check` takes an absolute path, and the reference names every module by the path the user gave.
So `front.nz` (shared by `check` and `doc`) prefixes a relative path with the working directory and
takes that prefix off every module's name and off every path a diagnostic's words name after a
backquote — except a dependency package's module, which the reference names under its canonical
source root. `check` now runs `fx_check` instead of driving the stages itself (main's loader took the
source root and the packages, 2026-10-15); its refusals and outputs are unchanged.

The port is built without `--heap-limit`: under one, 0.10.0's `os_realpath` traps (status 133, no
message) — a runtime defect, handed back. `run.sh doc` runs `doc FILE` and, where the reference
succeeds, `doc -o DIR FILE` for every input of `check` plus the repository's package directories and
a project of every template, comparing status, output, error and both files; `tests/args/doc.txt`
covers the options, packages, re-exports, deprecation and the failures.

**D14 — `fix` is `fix.rs` over the same facts.** `toolchain/commands/fix.nz` checks the program
as `check_map` does (`fx_check`: the root must have `main`; warnings carry no fixes it reads) and
turns every fix of every error into an edit of the file its diagnostic is in — the file the stage
named, else the last whose text begins at or before the fix's offset; an offset that is also where
a later file begins is refused, since the reference's span knows its file and the port's does not.
The edit's bytes are local to that file and the text it expects there is taken from the check's
own copy. `--apply` writes automatic edits file by file in name order, right to left by start
(stable for equal starts), and skips with the reference's words an overlap, a span off a character
boundary or past the end, bytes changed since the check, a result that does not parse
(`sx_parse_source`), a file that cannot be read or written. The report is the reference's — three
lines an edit, or one `nazm.fix/1` object with the struct's field order — then, without `--json`,
the dry run's note and `N edits applied, M diagnostics remaining` from a fresh check after
`--apply`; status 1 while errors remain. Loading is `opened`'s: a manifest the reference refuses is
its words (3 for `could not read…`, else 1), a load failure its diagnostics, text or JSON, and 3 when
every one is `N0205`. Not carried, with 6: where the front end stops, and a root that is a
directory or not UTF-8, which `compiler/load.nz` traps on rather than reporting `N0205` (a defect of
`compiler/`, handed back). `run.sh fix` runs `fix` and `fix --json` over every input from the
repository's root and, where the reference would apply an edit, `fix --apply` on a copy, the file
compared too; `tests/args/fix.txt` covers both applicabilities, several edits in one file, an edit
in an imported module, the failures and the options.

**D15 — `check` carries packages (task 5.2, 2026-10-16), and says what still blocks the rest.**
Since `fx_check` decides the source root and resolves the packages (R2's `compiler/package.nz`),
`check` follows `prepare.rs` and `package.rs` for them instead of refusing every manifest at or
above the file (D11's walk is gone): a directory with a manifest whose package has no `main` is a
library, checked as `checking::library` checks it — every `.nz` file under the canonical source
root, sorted as `PathBuf` sorts, each through `fx_check_module`, its errors (not its warnings) each
reported once by file, local span and code, `--deny-warnings` and the other options not consulted,
as the reference consults none; a package with a `main` is that file, named by its canonical path;
a file in a package is checked whole with its dependencies, and an import of a package
(`N0508`) is the loader's diagnostic like any other. A manifest that is not one is `resolve`'s
`error[N0500]` for a directory and `root::decide`'s `nazm: N0500: …` for a file in it (status 1;
`fx_refusal`, whose words R2 keeps the reference's). `--locked` in a package without a `nazm.lock`
is `N0505` with its help; `--features` in a package — whose manifest declares none, since the
front end reads no `[features]` — is `Graph::enable`'s `N0500` for each name asked, sorted and each
once (status 1). `--profile` naming no profile is `N0511` with the list; `--profile general` holds
no rule and is carried where no module asks `@cfg`. `--cache-report --no-cache` prints the tally a
run without the cache has (every module checked, nothing reused, missed, rejected, declined or
stored) and the root's kind, `derived` or `package`.

Still refused, each with what blocks it: `--locked` where the lockfile exists (its digests —
`nazm_package::digest`, BLAKE3 over the manifest and the source tree — are not ported), a profile
other than `general` and `--profile-report` (the profile rules evaluate MIR facts, R3's),
`--cache-report` with the cache (the port keeps no cache, so its tally would be invented), and
`--summary-json` (its references carry the ids of `nazm diagnostics` — N30's per-module language
service — and that service's `state.source` digest; neither is published by `compiler/facts.nz`).

`--source-root` (2026-10-16, R2's `fx_check_in`): the root as given, made absolute as the reference
makes it, decided by `pk_given` — a directory that is an ancestor of the root module as written; a
path that is not a directory is `nazm: \`DIR\` is not a directory` in the words the user wrote it in
(3); a root the module is not under as written, or reached through a link, is refused (R2's
`pk_given` gives up rather than decide what the reference's `canonicalize` might accept), and so is
`--source-root` for a package's module, whose packages the reference resolves under one root and
loads under the other. `--cache-report` names the root `given`.

Epoch 88 (main, 2026-10-16) holds a pattern `let`'s names to the scope the `let` is written in
(`N0202` for a name that scope binds already). Until R2's checker held it too, `check`, `doc` and
`fix` refused a compilation whose own modules wrote a pattern `let`; `compiler/sema.nz` holds it
since `ff1dce6a`, and the refusal is gone.

**D16 — `lsp` is `lsp.rs` over the same facts, and refuses wherever they do not answer
(2026-10-16).** `toolchain/commands/lsp.nz` speaks the protocol as `lsp-server` does over
standard input and output: `Content-Length` framing read with `os_read_line` and `os_read` (one
buffer), each message parsed by a JSON reader of its own (the port must know exactly what it
accepted; a message it reads differently from serde — a duplicate field, an `id` that is neither
an `i32` nor a string, a header other than `Content-Length` — is refused), and every answer written
as serde writes it: the outer message's fields in `lsp-server`'s order, every `Value` inside with
its keys sorted (`serde_json::Map` is a `BTreeMap`). The handshake is `initialize_start` and
`initialize_finish`: the first message must be `initialize` (input that ends first is `nazm lsp:
initialize: disconnected channel`, status 4), answered with the reference's capabilities, then
`initialized`. `shutdown` is answered `null` and must be followed by `exit`; `exit` at any other
time ends the reader, and the server with it, status 0, as does the end of input. A request the
reference does not implement is `MethodNotFound` (−32601) in its words — `textDocument/formatting`
among them: the reference offers no formatting. `--source-root` is `check`'s (D15), given to every
document's compilation.

Documents: `didOpen`, `didChange` (each change a UTF-16 range of the text the changes before it
left, `LineIndex::offset`, or the whole text), `didClose` (the others published again, then the
closed one cleared, without a version). Each publication is `refresh`'s: the document's
compilation checked with its text standing for its file, saved or not and whether or not the file
exists (`fx_check_over`), as a module — the program's `N0204` left out, as `Entry::Absent` leaves
it — its own file's errors, then the checker's warnings — kept beside the errors in `Ck.w` and
published even where there are errors, as the service chains `nazm_core::warnings` — positions in
UTF-16 (`LineIndex::position`) of the text the compilation holds for the file. Refused: two
documents open at once (the reference publishes them in its `HashMap`'s order), a document named by
another spelling of an open one's path, a compilation that does not load (the service analyses
what loaded) or that the front end stops at, and a warning beside a syntax error.

Hover and definition are answered from `name_at` (the lexer's identifier holding the position, or
ending there) and the checker's indexes: a local (`ck_binding_at`) is `NAME: T` (`mut ` first
where it is), `T` as `fx_show` shows any type, and its declaration's span; a call to a plain
function (`ck_site_at`, `ck_site_fn`) is `fn NAME(p: T, …) -> R` then ` ! {…}` or, where none is
declared, a line of `inferred effects: {…}` (`Signature::render`, `EffectSet`'s `Display`).
Definition of any other name is where `fx_declaration_at` says it is declared — a function called
or used as a value, a record built — in its module's file, or `null` in the standard library or the
prelude, which have no file. `null` where the reference answers nothing: no name there, a built-in
called, a function's name at its declaration, a field or variant after `.` that is not called.
Refused: hover of a method, closure, generic or foreign function or of a call through a value,
hover of a name the checker publishes no binding for that a binding or function of the compilation
is called by, and definition of what `fx_declaration_at` does not index (a constant, a field, an enum's
variant in an expression). Completion, references,
signature help, rename, symbols, semantic tokens, code actions and call hierarchy are refused.

`run.sh lsp` runs the scripted sessions of `tests/lsp/` in a fixture, both sides, every byte of
the output compared; then every input of `check`, opened as it is on disk (and closed), and again
with hover at seven positions — the sessions written by `tests/lsp-session.py`, the one part of the
comparison that needs `python3`.

**D17 — `test` is `test.rs`, its two legs another `nazm`'s (task 6.1, 2026-10-19).**
`toolchain/commands/test.nz` gathers, runs and reports as `test.rs` does: a directory's `.nz` files
in byte order (an entry that is itself a directory is walked), `NAME.nz` beside `NAME.expected` read
as `read_to_string` reads it (a file that is not UTF-8 is no case), then every top-level `@test` and
`@fuzz` function the file marks in source order, among the declarations the host builds (`@cfg`
by `compiler/configure.nz`; the file parsed by `sx_parse_source`, a function with two such
attributes a case for each), `@test(fails)` read from its arguments, `@bench` skipped; a file with
no case is counted and named on standard error. Each case is two programs run to their end with
standard input empty — `nazm run --interpret [--harness SPEC] FILE` and `nazm build [--harness SPEC]
FILE -o EXE` then the executable, each with `NAZM_RESULT_REPORT=1` — their output and errors
decoded as `String::from_utf8_lossy` decodes and trimmed by `char::is_whitespace`, compared as `met`
compares (output equal to the trimmed `.expected`, exit status 0 for `passes`, 2 for `fails`); the
text report, the `nazm.test/1` objects (`serde_json`'s escapes) and the `nazm.test-summary/1` line
are the reference's, with its statuses (`no_cases` and `unreadable` 3, `toolchain_failed` 4, a
failed case 1). The command line is `cli.nz`'s, which gained `--fuzz`'s `u32` (clap's `invalid value
… for '--fuzz <N>'` with the integer parser's reasons, an `i64` read first and then the range) and
`usize`, a required option and a required group.

The port carries neither `run` nor `build`, so the legs are started from the `nazm` that
`NAZM_TEST_DRIVER` names — the reference's own seam (`test.rs`, `DRIVER`), which names `current_exe`
where it is unset — and without it the command refuses with status 6 before anything else. The
comparison sets it to the reference for both sides, so what is compared is the command around the
legs: gathering, `@cfg`, the harness arguments, the lossy decoding, the report, the summary and
the exit statuses. An `@cfg` that names a feature, a package or a profile (it asks a manifest) is
refused. `run.sh test` runs every tracked recorded case (`--interpret-only`, text and `--json`),
the conformance corpus and a fixture whole (compiled legs included, `--summary-json`, `--filter`,
`--fuzz`), and `tests/args/test.txt` the command lines.

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
