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
| `nazm.resolved/1` | `nazm resolve FILE`, and stored in each `nazm.check/3` entry (N77) | stdout | one object per module, per line |
| `nazm.references/1` | `nazm references DEF FILE` (N77) | stdout | one object |
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

**Applied to N60, restriction profiles v2.** No code appeared: a rule's refusal is `N0510`, and an
`unknown` verdict is `N0510` too, with a label saying the rule's facts could not be established.
`nazm.profile-report/1` gains two rule identities (`no-recursion`, `no-select`) and one verdict
value (`unknown`), new values of existing fields.

**Applied to N58, debugger and profiler v2.** No code appeared. One schema did: `nazm.profile/1`
(`nazm profile FILE`): `schema`, `backend`, `exit`, `wall_ms`, `stdout_bytes`, `memory` (per class:
`allocated`, `reclaimed`, `live`) and `scheduler` (`spawned`, `joined`, `selects`, `timeouts`).

**Applied to N56, packages v2.** Three codes appeared, compatible: `N0512` (a registry index that
is not `nazm.registry-index/1`, names another package, records a version twice or a malformed
digest; or a publish of a version already there), `N0513` (no published version satisfies every
requirement on a package, each named with who made it) and `N0514` (a registry package holding a
link or anything but files and directories). A copy changed after publishing is the existing
`N0507`; a registry package conflicting with a path package of its name is `N0503`; a cycle through
the registry is `N0504` — each with its meaning unchanged. Schemas: `nazm.registry-index/1` is new;
`nazm.package/1` gains an optional `[registry]` table and path-less dependencies, which an older
reader refuses as unknown rather than misreads; `nazm.lock/1` gains a source form,
`registry+NAME@VERSION`.

**Applied to N74, ecosystem tooling.** No code appeared: `nazm check DIR` on a package with no `main`
now checks its modules, so `N0509` is raised by `run` and `build` alone; `nazm bindgen` reports the
checker's own code for a declaration it refuses. `nazm.api-doc/1` (`nazm doc`), `nazm.bindgen/1`
(`nazm bindgen -o`) and `nazm.publish-plan/1` (`nazm publish --dry-run`) are new, each with a schema
in `schema/` checked against real output; `nazm.capabilities/1` gains `authority`, `targets` and
`commands` — new fields, compatible.

**Applied to N73, trust evidence.** No code appeared. `nazm.provenance/1` (`nazm build --provenance`)
is new, with a schema checked against real output; `cargo xtask evidence` writes `nazm.evidence/1`
and `nazm.sbom/1`, tooling of the repository rather than of the compiler, tested by
`xtask/tests/evidence.rs`.

**Applied to N72, account-oriented contracts.** No code appeared: `signed-writes` refuses with the
existing `N0510`. `nazm.sbpf-accounts/1` is new; `nazm.profile-report/1` gains a profile and a rule.

**Applied to N71, contracts on WebAssembly.** No code appeared: a failure is the existing code,
through the host's `report`. `nazm.wasm-contract/1` and `nazm.wasm-contract-run/1` are new.

**Applied to N70, the EVM backend.** Three codes appeared, compatible: `N0397` (a sender address
that is not an `Int`), `N0398` (calldata that is not an entrypoint's selector and `int64`
arguments), `N0399` (a construct the backend does not emit, or a storage upgrade it refuses). On the
EVM a failure reverts with its existing code's five bytes. `nazm.evm/1`, `nazm.evm-abi/1`,
`nazm.evm-storage/1`, `nazm.evm-upgrade/1` and `nazm.evm-run/1` are new.

**Applied to N69, contracts.** Two codes appeared, compatible: `N0395` (a module `nazm contract`
was given that is not a contract, naming what it lacks) and `N0396` (a transaction changed the
conserved quantity and was rejected). The `web3` profile refuses with the existing `N0510`.
`nazm.contract/1` and `nazm.contract-run/1` are new; `nazm.profile-report/1` gains a profile name.

**Applied to N68, layout specialisation.** No code appeared: a field-by-field `Vec`'s failures are
the existing `N0405` and `N0406`, unchanged. `nazm.cost/1` gains `layouts` — each `Vec` type and
whether `--layout soa` lays it out field by field, or why not — a new field, compatible.

**Applied to N67, accelerator kernels.** One code appeared, compatible: `N0394` (a function `nazm
accel` will not run as a kernel, naming why). A kernel's failures are the existing `N0400` and
`N0401`, with "at element *i*" added to the message. `nazm.accel/1` and `nazm.kernel/1` are new.

**Applied to N66, vectorisation.** No code appeared: `ints_sum_from`'s overflow is the existing
`N0400`, at the addition a replaced loop failed at. `nazm.cost/1` gains `loops` — each loop, whether
it was vectorised and by which idiom, or why not — a new field, compatible.

**Applied to N65, the interactive tier.** One code appeared, compatible: `N0393` (a function
`nazm comptime` will not evaluate: it has parameters, or it is not pure). A runaway comptime
evaluation is the interpreter's existing `N0402`; a REPL line's errors are the checker's own codes.
`nazm.reload-check/1` is new.

**Applied to N64, WebAssembly.** No code appeared: device registers in a module are the existing
`N0392`, a stack exhausted is `N0408`, an overflow `N0400`, each reported through the host's
`report`. What a module cannot have is the freestanding refusal (§7.64), which now names each part's
symbols. `nazm inspect` lists one more target; `nazm.bounds/1` is written for a module too, rooted at
`nazm_main`. No schema moved.

**Applied to N63, real-time bounds.** No code appeared: `no-blocking` and `bounded-loops` refuse with
the existing `N0510`, an unbounded loop as `unknown` with its reason, the meaning unchanged.
`nazm.profile-report/1` gains a profile name (`realtime`), two rule identities and an optional
`bounds` object — new values and a new field, compatible. `nazm.bounds/1` is new, written by a board
build beside `link.ld`; `nazm-stack:` is a UART line under `--stack-watermark`, not a schema.

**Applied to N62, a freestanding target.** One code appeared, compatible: `N0392` (`mmio_read32` or
`mmio_write32` reached by `nazm run` or a hosted build, which have no device registers). A register
access without an `MmioCap` held is the existing `N0369`; a board program's overflow and stack
exhaustion are the existing `N0400` and `N0408`, written to the UART. What a board cannot have is a
build refusal by name with exit status 4, not a code: it is a fact of the target, as a host without
a linker is (§7.59). No schema moved; `nazm inspect` lists one more target, a new value of an
existing field.

**Applied to N55, FFI v2.** One code appeared, compatible: `N0391` (an `extern "C"` export that is
not `pub`, does not declare `! {}`, is generic, or takes or returns a type other than `Int` and
`Bool`). A `Str` holding a NUL byte passed to C is the existing `N0405` at run time; an export's
symbol defined twice or also imported is the existing `N0383`; its ABI and symbol rules are
`N0380` and `N0382`, unchanged. No schema moved; `nazm build --lib` writes a C header, which is C,
not a Nazm schema.

**Applied to N54, select, deadlines and the scheduler's counters.** No code appeared: a clock read
without a `TimeCap` held is the existing `N0369`, with its meaning unchanged, and `critical`'s new
rule, `no-ambient-time`, is the existing `N0510`. `nazm.profile-report/1` gains that rule's identity,
a new value of an existing field. No schema moved; the counters are a diagnostic line,
`nazm-sched:`, on standard error under `NAZM_SCHED_REPORT`, as the memory report is.

**Applied to N53, the runtime component and generic channels.** One code appeared, compatible:
`N0390` (a `Chan[T]` written or made whose element may not cross into a task — a sequence, a
closure, or a record or enum holding one — with the path to the offending field). Everything
else a typed channel can get wrong is an existing code with its meaning unchanged: an element of
the wrong type is `N0303`, `Chan` with two arguments `N0300`. No schema moved: a channel type in
a persisted interface is a new shape, `{"chan":T}`, which a `nazm.interface/7` reader refuses
rather than misreads. `nazm.cost/1` gains no field; a typed channel's operations are sites of
the existing kinds.

**Applied to N52, resource facts and information flow.** No code appeared: a vouch without a
`VouchCap` is the existing `N0369`, and a profile's new rules are the existing `N0510`. Two
schemas appeared, `nazm.cost/1` (`nazm explain-cost --json`: per function, each site's `kind`,
`op`, `at`, `count` — `at-most-once` or `unknown`, never a number — and `on_failure`; and per
kind, totals by count) and `nazm.flow/1` (`nazm explain-flow --json`: each sink's `class`,
`function`, `at` and `origins`). `nazm.profile-report/1` gains three rule identities, a new value
of an existing field. The check artefact's sink facts gain an optional `class`, unread by an older
checker whose entries the semantic epoch already keeps apart.

**Applied to N51, effect parameters.** Two codes appeared, both compatible: `N0388` (an effect
parameter where none may be: a second one, on a foreign function, on `main`, on a function with
no declared effect set, or named inside a type's arguments) and `N0389` (a call binding one effect
parameter to two different sets). `N0386` narrowed to a `let mut` binding: a captured capability
is now accepted. `N0369` gained a case — making a value of an undeclared function without the
authority its type's effects need — with its meaning unchanged. No schema moved: an effect list
may hold `"*"`, the effect parameter, which a `nazm.interface/7` reader refuses as an unknown
effect rather than misreads.

**Applied to N50, function values and closures.** Two codes appeared, both compatible: `N0386`
(a closure captures a `let mut` binding or a capability) and `N0387` (a generic, built-in or
foreign function used as a value, or a closure or named function value written inside a generic
function). Everything else a function value can get wrong is an existing code with its meaning
unchanged: a call through one with the wrong arguments is `N0303` or `N0300`, `==` on one is
`N0304`, passing one to a task is `N0321`, effects its type or body lacks are `N0366`, and missing
authority is `N0369`. No schema moved: a function type in a persisted interface is a fifth shape
of type that a `nazm.interface/7` reader refuses rather than misreads. The memory report line gained
a fourth class, `closures`, after the other three, so a reader that finds classes by name is
unaffected.

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

**Applied to N45, the standard library.** No schema moved and no code appeared. `N0205` (a module
that cannot be found) gained a case: an import beginning with `@std/` that names no standard module,
whose help lists the ones there are. A standard module's definitions are named `@std/NAME::…` in
every packet that names a definition, exactly as a project module's are named by its path.

**Applied to N46, packages.** Ten codes appeared, all compatible: `N0500` (an invalid manifest),
`N0501` (a dependency that is not there), `N0502` (a version mismatch), `N0503` (two packages of
one name, or a declared name the package does not have), `N0504` (a cycle), `N0505` (`--locked`
with no lockfile), `N0506` (other packages than the lockfile records), `N0507` (a package whose
bytes no longer match its digest), `N0508` (an import naming a package the importer does not
declare) and `N0509` (a package with no `main`). They are about manifests, directories and the
lockfile rather than positions in a module, so they carry a synthetic span. Two new formats have
their own versions and are not these schemas: the manifest, `nazm.package/1`, and the lockfile,
`nazm.lock/1`. `nazm.check-report/1` and `nazm.build-report/1` gained `"package"` as a value of
`root`, compatible.

**Applied to N47, restriction profiles.** Two codes appeared, compatible: `N0510` (a rule of a
profile in force refused the program — one per rule, at its first witness, with the rest as
secondary labels) and `N0511` (an unknown profile). One new format has its own version:
`nazm.profile-report/1`, one line of JSON — `schema`, `profiles`, `outcome` (`accepted` or
`refused`), `rules` (each `id`, `verdict` `pass`/`fail`, `witnesses` of `file`, `start`, `end`,
`note`), and a `note` that it is not a certification. Rule ids are stable: `declared-effects`,
`no-io`, `no-spawn`, `no-foreign`, `locked-build`.

A consumer that fails on an unrecognised field is not compatible with this contract, and
the `additionalProperties: false` in the schema documents is a statement about *this*
version's output, not a promise that the next one adds nothing.

**Applied to N76, syntax.** No schema moved and no code appeared. `N0004` also covers a refused
`\u{…}` escape, at the escape's own bytes. Recovery inside lists reports existing codes at new
positions: a later independent mistake in the same item is now its own diagnostic.

**Applied to N77, persisted resolution.** Two schemas appeared: `nazm.resolved/1`, a module's
imports, definitions and resolved occurrences by durable identity, and `nazm.references/1`, one
entity's occurrences read from those units. The cache's own entry moved to `nazm.check/3`, and a
`/2` entry is a miss. No code appeared, and the interface stays `nazm.interface/7`.

**Applied to N78, traits and methods.** Eleven codes appeared, all compatible: `N0600` (two impls
of one trait for one type), `N0601` (an impl in a module declaring neither), `N0602` (an impl
missing a method), `N0603` (an impl method its trait does not declare, or with another signature
or effects), `N0604` (an impl for a type that cannot have one), `N0605` (no trait in scope gives the
receiver the method), `N0606` (more than one does), `N0607` (a trait where a type is expected),
`N0608` (a method taken as a value), `N0609` (a trait method whose first parameter is not `self:
Self`) and `N0610` (a bound or an impl naming a type that is not a trait). They are the checker's,
in a new block because N03xx is full. The persisted interface moved to `nazm.interface/8`, and Core
IR's printed form to `nazm.core-ir/2`. No other schema moved; `nazm.resolved/1` gained the target
kinds `trait` and `trait method`.

**Applied to N79, effects and capabilities v3.** No code appeared. An `OutCap` that is asked to
authorise a file, the arguments or exit is the existing `N0369`, naming the `IoCap` it lacks; an
`OutCap` given for an `IoCap`, and a function value with more effects than expected, are the
existing `N0300`; a program defining `OutCap` is the existing `N0331`; the `authority` profile's
`explicit-authority` refuses with the existing `N0510`, naming the function, the kinds it inherits
and where it first exercises them. `nazm inspect` gains `inherits` per definition, a new field and
compatible; `nazm.profile-report/1` gains a profile and a rule. The interface stays
`nazm.interface/8`.

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
