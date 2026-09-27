# `nazm-mcp`

A read-only [Model Context Protocol](https://modelcontextprotocol.io) server over stdio, built on
the official Rust SDK. It offers one tool: the compiler's semantic context packet for a definition.
It is local compiler tooling — it answers questions about one program, and changes nothing.

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

## The tool

`nazm.semantic_context`, with two arguments:

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

## What it reads, and when

Every call opens the root **from disk as it is then**, derives the packet and drops the analysis.
An edit written between two calls is seen by the second; a broken file gives no packet, never the
previous one. It does not see an editor's unsaved buffers — for those, use `nazm lsp`.

## What it does not do

- Write, rename or delete anything, run a command, or offer any edit.
- Read a file its root's compilation did not load: `file` is matched against the compilation's
  own files and is otherwise refused unopened.
- Serve resources, prompts, completions or sampling; only stdio — no HTTP, no authentication.
- Report effects, capabilities, tests, semantic history or semantic deltas: the compiler has no
  such facts, and a packet marks each `unsupported` rather than empty.
