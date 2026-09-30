# The machine interface

Everything `nazm` says to a program, rather than to a person, is JSON under a **named,
versioned schema**. The schemas live in [`schema/`](../schema) as JSON Schema documents,
and `crates/nazm-cli/tests/schemas.rs` validates the real output of the real commands
against them — so these documents are tested rather than described.

| Schema | Command | Where | Shape |
| --- | --- | --- | --- |
| `nazm.diagnostic/1` | `nazm check --json`, `nazm run --json`, `nazm build --json` | stderr | one object per line |
| `nazm.fix/1` | `nazm fix --json` | stdout | one object per line |
| `nazm.capabilities/1` | `nazm capabilities --json` | stdout | one object |
| `nazm.test/1` | `nazm test --json` | stdout | one object per case |
| `nazm.check-report/1` | `nazm check --cache-report` | stdout | one object |
| `nazm.build-report/1` | `nazm build --build-report` | stdout | one object, before the executable's path |
| `nazm.context/3` | `nazm context --json`, and the `nazm-mcp` tool `nazm.semantic_context` | stdout; the tool result's structured content | one object |
| `nazm.snapshot/3` | `nazm snapshot --json`, and the `nazm-mcp` tool `nazm.semantic_snapshot` | stdout; the tool result's structured content | one object |
| `nazm.delta/3` | `nazm delta --json`, and the `nazm-mcp` tool `nazm.semantic_delta` | stdout; the tool result's structured content | one object |
| `nazm.patch/1` | `nazm patch rename --json`, `nazm patch fix --json`, and the `nazm-mcp` tool `nazm.semantic_patch` | stdout; the tool result's structured content | one object |
| `nazm.diagnostic-index/1` | `nazm diagnostics --json`, and the `nazm-mcp` tool `nazm.diagnostics` | stdout; the tool result's structured content | one object |
| `nazm.diagnostic-detail/1` | `nazm diagnostics --detail ID --state DIGEST --json`, and the `nazm-mcp` tool `nazm.diagnostic_detail` | stdout; the tool result's structured content | one object |
| `nazm.command-summary/1` | `nazm check --summary-json`, `nazm build --summary-json`, and the `nazm-mcp` tool `nazm.command_summary` (check only) | stdout; the tool result's structured content | one object |
| `nazm.test-summary/1` | `nazm test --summary-json` | stdout | one object |
| `nazm.docs-index/1` | `nazm docs --index --json`, and the `nazm-mcp` tool `nazm.docs_index` | stdout; the tool result's structured content | one object |
| `nazm.docs-section/1` | `nazm docs --section ID --json`, and the `nazm-mcp` tool `nazm.docs_section` | stdout; the tool result's structured content | one object |
| `nazm.repository-map/1` | `nazm repo --map --json`, and the `nazm-mcp` tool `nazm.repository_map` | stdout; the tool result's structured content | one object |
| `nazm.task-context/1` | `nazm repo --task --seed ID --json`, and the `nazm-mcp` tool `nazm.task_context` | stdout; the tool result's structured content | one object |
| `nazm.token-cost/1` | `nazm-tokens measure` (the text on standard input) and `nazm-tokens list`, a separate tool beside the compiler | stdout | one object |
| `nazm.agent-benchmark/1` | `nazm-agent-bench suite`, its journal records (`journal.jsonl` in the local evidence directory) and `nazm-agent-bench report`; evaluation tooling beside the compiler | stdout, and one record per journal line | one object, or one record per line |

Every object carries its own `schema` field. Not a header: the output is a stream of
lines and a consumer may read, log or replay any single one of them, and a version that
only the first reader sees is not a version.

## What compatibility means

The number after the `/` is a **major** version and the only one there is. It changes
when a consumer written against the previous version would now be **wrong** — not merely
incomplete.

**Compatible, and may happen in any release.** A consumer must tolerate all of these,
which in practice means *ignore what you do not recognise*:

- A new field on an existing object.
- A new diagnostic code. Codes are stable and never reused, but new ones appear.
- A new value in an existing enum — a new `applicability`, a new `severity`.
- A new element in an array: another fix on a diagnostic, another built-in in the
  inventory, another secondary label.
- A change to any `message`, `help`, `description` or `precondition`. **These are prose.**
  Match on `code`, never on text.

**Breaking, and requires a new major version.** None of these will happen inside a
version:

- Removing or renaming a field.
- Changing a field's type, or the meaning of its value.
- Changing what an existing diagnostic code means, or reusing a retired one.
- Changing `span` offsets from bytes to anything else.

**Applied to N37, capabilities.** No schema moved. Three diagnostic codes appeared —
`N0369` (missing authority), `N0370` (a capability constructed) and `N0371` (a `main`
parameter that is not a capability) — which is a new code, compatible. `IoCap` and
`SpawnCap` are two more built-in types, so `nazm.capabilities/1`'s type list has two more
elements, compatible. A function's authority is its parameter list, which
`nazm.context/2`, `nazm.snapshot/2` and `nazm.delta/2` already carry, so a capability-only
change is a `shape` change there and needed no new field; whether a function is ambient is
whether `effects.declared` is `null`, which already meant *declares none*. The persisted
module interface, which is not one of these schemas, stays `nazm.interface/6` for the same
reason: a capability parameter is written as every built-in type already is.

**Applied to N38, provenance.** Three schemas moved, each because an object valid under the old
major could not carry what the new one must say: `nazm.context/3` adds a function's
`provenance` — the origins its result carries, the parameters it passes on and those reaching
`write_file`'s path, and what each parameter is handed; `nazm.snapshot/3` adds a `provenance`
digest per function; `nazm.delta/3` reports it. A `/2` reader would not know to compare the new
section, and a function's provenance can change with no byte of it changing — when a callee's
body does — so reporting it under `/2` would call such a function unchanged. One code appeared,
`N0372`: in a function with a declared effect set, `write_file`'s path derives from a file's
contents or from a sequence or channel the checker does not follow. Its primary span is the
`write_file`, or the call handing such a value to a function whose parameter reaches one; its
secondary labels are one chain to the sink and to where the origin entered; it carries no fix.
The provenance facts themselves are machine-readable only in the packet; the persisted module
interface stays `nazm.interface/6`, and the cache's own entry format moved to `nazm.check/2`.

**Applied to N39, Core IR.** No schema moved. One code appeared, `N0900`: the compiler
contradicted itself — a program that checked with no diagnostic could not be lowered to Core IR,
or its Core IR broke an invariant (`architecture.md` §7.41). It describes the compiler, not the
program, carries no fix, and nothing is run or built after it. The printed Core IR,
`nazm.core-ir/1`, is a debugging form with its own version and is not one of these schemas.

**Applied to N40, MIR.** No schema moved and no code appeared. `N0900` gained one more way to be
raised: a program that checked whose MIR the validator rejects (`architecture.md` §7.42) — `nazm
build` then emits nothing. The native subset's own refusals (`N0101` for recursion, a `main` it
cannot call, a sequence handed to a task) are raised from MIR now and read exactly as before:
across all 205 sources in the tree, every build diagnostic is byte-identical. The printed MIR,
`nazm.mir/1`, is a debugging form with its own version and is not one of these schemas.

**Applied to N41, the backend boundary.** No schema moved and no code appeared. A program the
Cranelift backend cannot compile is refused under `N0101`, naming the construct and the
backend, and never falls back to LLVM; a target triple that is not the host is refused by name
before anything is compiled. `nazm.build-report/1` is unchanged: a Cranelift object reused by
its key counts as reused, exactly as a clang one does.

**Applied to N42, foreign functions.** Six codes appeared, all compatible: `N0380` (an ABI
other than `"C"`), `N0381` (a parameter or result type that cannot cross the C boundary — only
`Int` and `Bool` can), `N0382` (a symbol that is not a C identifier), `N0383` (two declarations
of one C symbol that disagree about its signature — in one module at check time, across modules
when the program is built — or a symbol the native runtime declares itself), `N0384` (type
parameters or a written effect set on a foreign declaration) and `N0385` (`nazm run` asked to
run a program that calls a foreign function, refused before any of it runs). A foreign call
without a `ForeignCap` held is the existing `N0369`, with or without an effect set; a declared
set that leaves `foreign` out is the existing `N0366`. One schema moved: the persisted module
interface is `nazm.interface/7`, because an export may now carry `foreign`, its C symbol, and a
`/6` reader would take such an export for an ordinary function with no body. `effects` lists
may contain `"foreign"` — a new value of an existing field, which is compatible. A link that
fails for a program with foreign calls names the foreign symbols and `--link` rather than
reporting a compiler bug.

**Applied to N43, the runtime constitution.** No schema moved and no code appeared. The runtime's
failures keep their codes and now have one written classification (`architecture.md` §7.45):
`N0400`, `N0401` and `N0405` are the language's; `N0406` and `N0404` are the host refusing memory
or a thread — `N0404` in a native build means the operating system would not start a task's
thread, as `nazm run` uses it when the interpreter could not start; `N0407` is I/O. Status 2 is a
reported failure, `exit_with`'s is the program's, and 0 a normal return.

A consumer that fails on an unrecognised field is not compatible with this contract, and
the `additionalProperties: false` in the schema documents is a statement about *this*
version's output, not a promise that the next one adds nothing.

## Spans are byte offsets

`start` is inclusive, `end` is exclusive, and both count **bytes** from the beginning of
the file — not characters, not UTF-16 units, not lines. `start == end` is an insertion
point rather than an empty range.

For a program assembled from several files with `use`, a diagnostic's offsets are into
the concatenation of every file in load order.

**This did not change when source identity became internal (N1, 2026-09-21.)** The compiler
now carries a real file with every span and offsets that are local to it, and the human
rendering names the file directly rather than searching for it. The published shape is
deliberately unmoved: `nazm.diagnostic/1` has always reported concatenation offsets with no
`file` field, and improving the compiler's internals is not a reason to move every existing
consumer's numbers. A future schema version could publish `{ file, start, end }` directly;
that is a version decision, not a side effect. `nazm fix --json` resolves them: its `file`,
`start` and `end` name a real file and an offset inside it, which is what an editing tool
needs.

**Where the conversion happens, corrected 2026-09-22.** N1 met the compatibility
requirement inside `Span` itself: a hand-written `Serialize` that dropped the file, and a
`Deserialize` that put `FileId::ROOT` back. The published bytes were right and the design
was wrong — *every* serialisation of a span was lossy, including future ones with no
connection to this schema, and a span read back named the wrong file with no error. The
conversion now belongs to `nazm-diag`, which owns this format: `Diagnostic::published`
converts through `SourceMap::global` at the moment of serialisation into `wire::Published*`
types that exist to be this schema and nothing else. `nazm-span` no longer depends on serde
at all, so a span cannot be serialised by accident; `cargo xtask check`'s **span identity**
gate fails if that changes. A test pins the exact bytes of a diagnostic in the second file
of a two-file program.

**An editor sees the same diagnostics, converted at its own boundary (N16, 2026-09-25).**
`nazm lsp` publishes diagnostics as the Language Server Protocol requires: per file, with
positions as lines and UTF-16 columns. That is the protocol's shape, not a new Nazm schema:
the code, message and severity are the diagnostic's own, the range is its primary span
converted from file-local byte offsets at the adapter, and nothing in `nazm.diagnostic/1`
changed. Standard LSP messages are not Nazm machine interfaces and carry no `schema` field.

## Fixes, and when to apply one

A fix is `replace these bytes with this text`. Two things make it usable unattended:

| | |
| --- | --- |
| `applicability: "automatic"` | Correct whenever the diagnostic is correct, and changes nothing else. `nazm fix --apply` applies these |
| `applicability: "needs_review"` | Resolves the diagnostic and may not be what was meant. Never applied automatically; presented with its precondition |

**`precondition` is present on every fix, including the automatic ones.** On an automatic
fix it states what makes the edit unconditional; on one needing review it states the
assumption a person is being asked to confirm. A fix with nothing written there would be
a fix whose author never asked when it could be wrong, and the field exists to force the
question at the point the fix is written.

`nazm fix --apply` also refuses to write a file that would no longer parse, applies edits
right-to-left so earlier offsets stay valid, and skips an edit that overlaps one it has
already made — reporting each skip and why. Overlapping repairs are two answers to what
may be one mistake, and applying both produces text neither suggested.

Every reason an edit is not applied is reported in `skipped`, and there are exactly seven:

| `skipped` | When |
| --- | --- |
| `it needs review` | the fix is `needs_review`, which is never applied unattended |
| `the file could not be read` | the file to edit could not be read back |
| `it overlaps an edit already applied in this run` | a later edit in the file touched these bytes first |
| `the span does not name a range of this file` | the span is past the end of the file or inside a character |
| `the file changed since it was checked` | the bytes at the span are no longer the ones the checker saw |
| `applying it produced a file that does not parse` | the file's edits together left it unparsable; nothing in it is written |
| `the file could not be written` | writing failed; the edit is not reported applied |

### In an editor

`nazm lsp` offers the same fixes as quick fixes (`textDocument/codeAction`), from the
diagnostics of the document's current analysis and never from the diagnostics a client
sends:

- an **automatic** fix is a quick fix, marked preferred when it is its diagnostic's only
  fix;
- a **needs-review** fix is a quick fix the user chooses, titled with its precondition —
  `Review fix N0300: discard the value — right only if …` — and never preferred.

The two execution models differ, and neither pretends to be the other. `nazm fix --apply`
writes files, and can roll a file back when the result would not parse. The language server
writes nothing: it returns each fix as an edit tied to the open document's exact version, with
the bytes it replaces checked against the current text, and the editor applies it — so there
is no rollback after the fact. What stands in for one is that every fix the compiler attaches
is tested to parse and to remove its diagnostic when applied alone, and that a client whose
buffer has moved on cannot apply an edit tied to the old version.

## The capability inventory

Not the language's capabilities. `IoCap` and `SpawnCap` are **authority**, values a program
holds (`docs/spec.md`, *Capabilities*); this inventory is a list of what the toolchain can
do, and the two share a word and nothing else.

`nazm capabilities` reports what **this build** supports, read from the tables the
compiler dispatches on: `Intrinsic::ALL` for the front end, the backend's own `Builtin`
for `nazm build`, `Type::ALL` for the types. A built-in cannot be listed as interpreted
and be missing from the interpreter, because the interpreter's table is where the name
came from.

Constructs are the exception and say so. Whether `scope` or recursion compiles is decided
by code, not by a table, so each of those rows carries an `evidence` field naming the
test file or the document that establishes it. A row claiming `compiled: false` means the
construct is **refused by name** — never silently missing, and never approximated.

## Running a corpus

`nazm test PATH…` runs every `NAME.nz` that has a `NAME.expected` beside it, under
`nazm run` **and** as a `nazm build` executable, and requires both to produce the
recorded output. That convention is not new — `compiler/bootstrap.sh` has compared the
conformance corpus that way since the bootstrap existed; the command makes it available
to anything that is not the bootstrap script.

The comparison is **stdout and stderr combined, trimmed**: a program's diagnostic is as
much of its behaviour as its value, and a trailing newline is a property of `printf`.
A `.nz` file with no `.expected` beside it is not a test case, and the number of them is
reported rather than passed over — a silently skipped file is how a renamed test stops
running and nobody notices.

**A case whose program *fails* cannot match both legs**, and that is a real difference
rather than an oversight. `nazm run` renders a failure with the source line, a caret and
a help line; a compiled program writes one line — `nazm: error[N0401]: … at file:line:col`
— because the executable carries the message and the position and not the source. Both
name the same code and the same place, which is the part that is specified. A corpus
case that is meant to fail should therefore test one leg, with `--interpret-only`, or
assert on the code rather than the rendering.

## The exit statuses

| | |
| --- | --- |
| 0 | Nothing to report |
| 1 | The program has errors |
| 2 | The program ran and failed at run time |
| 3 | A file named on the command line, or in a `use`, could not be read |
| 4 | The build toolchain was missing or rejected IR this compiler produced |

4 is separate from 1 on purpose: one means the program is wrong, the other means the
machine could not build a program that is fine.
