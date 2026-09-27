# `nazm-mcp`

A read-only [Model Context Protocol](https://modelcontextprotocol.io) server over stdio, built on
the official Rust SDK. It offers three tools: the compiler's semantic context packet for a
definition (N27), and a semantic snapshot of the root's compilation and the delta against one kept
earlier (N28). It is local compiler tooling — it answers questions about one program, and changes
nothing.

## Running it

```sh
cargo build --release -p nazm-mcp
nazm-mcp --root path/to/root.nz            # optionally --source-root DIR
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

## What it reads, and when

Every call opens the root **from disk as it is then**, derives its answer and drops the analysis.
An edit written between two calls is seen by the second; a broken file gives no packet, snapshot or
delta, never a previous one. No snapshot, history or index is kept between calls. It does not see an editor's unsaved buffers — for those, use `nazm lsp`.

## What it does not do

- Write, rename or delete anything, run a command, or offer any edit.
- Read a file its root's compilation did not load: `file` is matched against the compilation's
  own files and is otherwise refused unopened, and a baseline is an object, never a path.
- Keep a previous snapshot, watch the disk, or infer a rename.
- Serve resources, prompts, completions or sampling; only stdio — no HTTP, no authentication.
- Report effects, capabilities, tests, semantic history or behavioural equivalence: the compiler
  has no such facts, and a packet, snapshot or delta marks each `unsupported` rather than empty or
  unchanged.
