# `nazm-mcp`

A read-only [Model Context Protocol](https://modelcontextprotocol.io) server over stdio, built on
the official Rust SDK. It offers eleven tools: the compiler's semantic context packet for a
definition (N27), a semantic snapshot of the root's compilation and the delta against one kept
earlier (N28), a planned rename or diagnostic fix (N29), the current diagnostics compactly or
one in full (N30), a compact summary of the root's check (N31), Nazm's own documentation by
section (N32), and the Nazm repository's context map and the minimum context for a task (N33).
It is local compiler tooling — it answers questions about one program, about the language and
about its repository, and changes nothing.

## Running it

```sh
cargo build --release -p nazm-mcp
nazm-mcp --root path/to/root.nz            # optionally --source-root DIR, --docs REPOSITORY
```

An MCP client starts it as a stdio server. A client configuration is usually a command and its
arguments:

```json
{ "command": "nazm-mcp", "args": ["--root", "/absolute/path/to/root.nz"] }
```

## The root

`--root` names the compilation every answer is about: that file and everything it imports, loaded
exactly as `nazm check` loads it. It is fixed for the server's lifetime; a tool call cannot name
another. Nothing outside that compilation is read — there is no directory scan — and nothing
claims to know every caller or importer in a project.

## The tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `nazm.semantic_context` | `file`, `byte_offset` | `nazm.context/1` |
| `nazm.semantic_snapshot` | none | `nazm.snapshot/1` |
| `nazm.semantic_delta` | `baseline`, a `nazm.snapshot/1` object | `nazm.delta/1` |
| `nazm.semantic_patch` | `operation` (`rename` or `diagnostic_fix`) and its fields | `nazm.patch/1` |
| `nazm.diagnostics` | none | `nazm.diagnostic-index/1` |
| `nazm.diagnostic_detail` | `id`, `state` | `nazm.diagnostic-detail/1` |
| `nazm.command_summary` | `operation`: `check` | `nazm.command-summary/1` |
| `nazm.docs_index` | optionally `document` | `nazm.docs-index/1` |
| `nazm.docs_section` | `id`, `state` | `nazm.docs-section/1` |
| `nazm.repository_map` | none | `nazm.repository-map/1` |
| `nazm.task_context` | `seeds`, `state`; optionally `intent`, `max_entities`, `max_source_bytes`, `max_doc_bytes` | `nazm.task-context/1` |

Each result is carried **once**, in the tool result's structured content, with an empty `content`,
and each tool's output schema is the published one in `schema/`.

### `nazm.semantic_context`

Two arguments:

| Argument | |
| --- | --- |
| `file` | a file of the root's compilation, relative to its source root (as packets name files) or absolute |
| `byte_offset` | a UTF-8 byte offset into that file |

The position must be on the declaration of a function, record or enum, or on a use of one. The
result is a `nazm.context/1` object (`schema/nazm.context-1.json`, `docs/architecture.md` §7.29) in
the tool result's structured content, **once**: `content` is an empty array, not a second copy of the
packet as text (which the specification only suggests, and which would double every response). A position with no such definition, a file the compilation did not load, or a
compilation that needed syntax recovery gives an `unavailable` object and `isError: true`; a
malformed call (an unknown tool, a missing or mistyped argument) is a protocol error.

A packet's links are **source-linked program dependencies only**: every link has a declaration in a
file of the compilation. Compiler-owned, built-in, interface-only or toolchain definitions with no
navigable declaration — the core prelude's `Option` and `Result`, `Int` — may affect checking but are
omitted from the link sections.

A packet holds the target's exact source and compact links — identity, kind, name, place — to what
it uses, the types it names, what uses it, what it calls and what calls it, and the diagnostics
inside it. To follow a link, call the tool again at the linked declaration.

### `nazm.semantic_snapshot` and `nazm.semantic_delta`

A snapshot (`schema/nazm.snapshot-1.json`, `docs/architecture.md` §7.30) lists every durable
function, record and enum of the root's compilation by its definition key, with one digest per
section the compiler records — source, shape, dependencies, related types, references, callers,
callees, diagnostics. **Keep it**: the server does not. To learn what changed, pass it back as
`baseline` to `nazm.semantic_delta`, which derives a snapshot from the disk at that call and
reports definitions added, removed, and changed, naming which sections changed. A rename is a
removal and an addition. A comment or a changed literal is `source` alone.

**N28 reports changes in the semantic surfaces Nazm currently records. It is not a proof of
behavioural equivalence.** Effects, capabilities and tests are unsupported, not unchanged — each
result lists them in `unsupported`.

The baseline is data, never a path: a string such as `"/etc/passwd"` is not a snapshot and is
refused as one, and no file is read for it. A baseline of another root module or another compiler
version, one with a field the schema does not define, a malformed identity, a duplicate or a bad
digest is refused with a reason and `isError: true` — never compared as well as it can be. A
compilation that needed syntax recovery has no snapshot and no delta.

### `nazm.semantic_patch`

**N29 plans edits but never applies them.** A patch (`schema/nazm.patch-1.json`,
`docs/architecture.md` §7.31) is a proposal bound to exact semantic and source state, not
permission to change a file. Two operations:

| `operation` | Fields | Plans |
| --- | --- | --- |
| `rename` | `file`, `byte_offset`, `new_name` | the compiler's validated rename of the entity there: locals and private definitions only |
| `diagnostic_fix` | `file`, `byte_offset`, `fix` | fix number `fix` among those the current diagnostics at that byte carry |

The result is minimal exact-byte edits — each renamed occurrence, or the compiler's one fix span —
with three freshness layers: `baseline.snapshot`, BLAKE3 of the root's `nazm.snapshot/1`; each
file's `digest`, BLAKE3 of its bytes; and each edit's `expected` bytes. A client that applies one
checks all three first. A fix's `applicability` (`automatic` or `needs_review`) and `precondition`
are structured fields: a `needs_review` patch is never safe to apply unread. An exported
definition's rename is refused, because one root compilation is not every module that may import
it; so is a fix at a position with no current diagnostic, whatever code a client expects there,
and any fix of a syntax diagnostic, whose compilation has no snapshot to bind to. The input is
typed: no root, no unknown argument, and no client-written replacement is accepted.

### `nazm.diagnostics` and `nazm.diagnostic_detail`

Progressive disclosure. `nazm.diagnostics` lists every current diagnostic of the root's
compilation compactly (`schema/nazm.diagnostic-index-1.json`, `docs/architecture.md` §7.32): each
one's `id`, code, severity, file and byte range, owning definition and fix counts — none of its
message, help or label text. Syntax errors are included. `nazm.diagnostic_detail`, with an `id`
and the index's `state.source`, returns that one diagnostic exactly as `nazm.diagnostic/1`
publishes it, with its places in their files, and for each fix the `nazm.semantic_patch` request
that plans it where N29 can — a compiler fix of a syntax diagnostic has none. If the sources have
changed since the index, the detail is refused as `stale_state`; ask for a fresh index. No fact is
read from prose, and `facts` is `unavailable`: the compiler records none.

### `nazm.command_summary`

What `nazm check` concludes for the root as it is on disk now (`schema/nazm.command-summary-1.json`,
`docs/architecture.md` §7.33): its status and exit status, and a reference to every diagnostic it
reports — code, place, fix counts and, where `nazm.diagnostics` indexes it, its `id` — including a
program's missing `main`, which the index does not hold. No prose; `nazm.diagnostic_detail` answers
an `id`. The only `operation` is `check`, and it runs without the check cache, so nothing is
written. Building writes an executable and testing runs programs that may write files, so neither
is offered here; `nazm build --summary-json` and `nazm test --summary-json` are the command line's.

### `nazm.docs_index` and `nazm.docs_section`

Nazm's own documentation by section, exactly as `nazm docs` gives it (`schema/nazm.docs-index-1.json`,
`schema/nazm.docs-section-1.json`, `docs/architecture.md` §7.34). The corpus is a fixed list of the
repository's documents — the specification, the grammar, the architecture, the capability matrix,
the roadmap, the goals and the rest — under the repository given as `--docs` at startup, or the one
the server was built from. It is not the root's project and not a directory scan.

`nazm.docs_index` takes an optional `document` id and returns every document with its authority
and every section's stable id, title, parent and digest, with no body, and the corpus `state`.
`nazm.docs_section` takes a section's exact `id` and that `state`, and returns the section's own
canonical text byte for byte — not its subsections', and never a paraphrase — with its document's
authority: a `goal` section is aspiration and never evidence of what exists, a `planning` section
is never a rule of the language. A `state` the documentation no longer has is `stale_state`; an id
no section has is `not_found`, never the nearest one. There is no path argument: `../README.md` or
`/etc/passwd` is only an id no section has, and no file is opened for it.

### `nazm.repository_map` and `nazm.task_context`

The repository's context map and the minimum context for a task, exactly as `nazm repo` gives
them (`schema/nazm.repository-map-1.json`, `schema/nazm.task-context-1.json`,
`docs/architecture.md` §7.35), over the same repository as the documentation tools — the one given
as `--docs`, or the one the server was built from — never the root's project.

`nazm.repository_map` takes nothing and returns every entity with a durable identity — packages
and targets, source roots, compilation roots, modules and durable definitions, documents, schemas,
mutation profiles — with its authority, parent, structural relationships and how to retrieve more,
and the repository `state`; no source body and no section text. `nazm.task_context` takes one to
eight `seeds`, ids from that map, and that `state`, and an optional `intent` — `understand`,
`edit` (the default, the widest), `diagnose` or `test` — and a structural budget. It returns the
smallest context the repository can justify, one relationship step from each seed: each item
with its reason, the seed it came from, its priority class, its authority and how to retrieve it
again; the seed's exact source, other definitions by shape, and sections by their exact text. What
a budget cuts is listed and makes the answer `partial`; a budget the seeds do not fit is
`insufficient_budget`. A `state` the repository no longer has is `stale_state`; a seed is an id and
never a path — `/etc/passwd` or `../x.nz` is `invalid_request`, and nothing is opened for it.

## What it reads, and when

Every call opens the root **from disk as it is then**, derives its answer and drops the analysis.
An edit written between two calls is seen by the second; a broken file gives no packet, snapshot or
delta, never a previous one. The documentation and repository tools read the repository at each
call in the same way.
No snapshot, history or index is kept between calls. It does not see an editor's unsaved buffers — for those, use `nazm lsp`.

## What it does not do

- Write, rename or delete anything, or run a command. `nazm.semantic_patch` describes an edit
  and applies nothing; there is no tool that applies one, and no plan is kept to apply later.
- Read a file its root's compilation did not load, other than the fixed documentation corpus:
  `file` is matched against the compilation's own files and is otherwise refused unopened, a
  baseline is an object, never a path, and a documentation section is asked for by id.
- Search documentation or the repository: ids are exact, with no fuzzy, full-text or semantic
  matching, and no directory listing or glob.
- Keep a previous snapshot, watch the disk, or infer a rename.
- Serve resources, prompts, completions or sampling; only stdio — no HTTP, no authentication.
- Report effects, capabilities, tests, semantic history or behavioural equivalence: the compiler
  has no such facts, and a packet, snapshot or delta marks each `unsupported` rather than empty or
  unchanged.
