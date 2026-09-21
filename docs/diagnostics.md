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
the merged buffer, because that is the text the compiler checked. `nazm fix --json`
resolves them: its `file`, `start` and `end` name a real file and an offset inside it,
which is what an editing tool needs.

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
