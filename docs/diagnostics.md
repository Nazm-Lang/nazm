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
