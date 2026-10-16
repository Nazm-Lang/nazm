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
| `nazm.resolved/1` | `nazm resolve FILE`, and stored in each `nazm.check/4` entry (N77) | stdout | one object per module, per line |
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
`nazm.core-ir/3`, is a debugging form with its own version and is not one of these schemas.

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
build refusal by name, not a code: it is a fact of the target, as a host without a linker is
(§7.59) — exit 1 since Q1-C1 (the program asks for what the target lacks), 4 before. No schema moved; `nazm inspect` lists one more target, a new value of an
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

**Applied to R1b, L15 (warnings).** A severity appeared, compatibly: `"warning"` beside
`"error"`, in `nazm.diagnostic/1`'s `severity` — a consumer that knows only `"error"` reads a
diagnostic it does not recognise, as the rule above requires it to tolerate. A warning is a
program the checker accepts, with something its author should know: `nazm check` and `nazm build`
report it and succeed, and refuse under `--deny-warnings`; the language service publishes it at
the warning severity. One code appeared, `N0632`, the first warning. No schema's major moved.

**Applied to R1.5, D7 (`List[T]` and `Chain[T]`).** One code appeared, compatibly: `N0637`, a
call of one of R1a's `list_` built-ins, renamed `chain_` with their type; its help names the new
built-in. Messages show the renamed sequence as `Chain[T]`; a `List[T]`, being `Vec[T]`, is shown
as a `Vec[T]` always was. No schema moved.

**Applied to R1.5, D4 (positional fields, variants without their enum).** One code appeared,
compatibly: `N0636`, a variant name two enums in scope share. `N0334` gained two cases — fields
given both by position and by name, and a positional list that does not give every field. No
schema moved.

**Applied to R1.5, D3 (short closures).** One code appeared, compatibly: `N0635`, a short
closure's parameter with nothing to take its type from. No schema moved.

**Applied to R1.5, D2 (expression bodies).** One code appeared, compatibly: `N0634`, a function
written `= e` that must still write its result type. No schema moved.

**Applied to R1b, L12 (`defer`).** One code appeared, compatibly: `N0631`, a `defer` body that
would leave it. No schema moved.

**Applied to R1b, L11 (default trait methods; impls of generic types).** `N0602` narrowed
compatibly: an impl may leave out a method its trait gives a default body. `N0604` gained a case —
an impl with type parameters whose header is not a generic record or enum applied to exactly them —
with its meaning unchanged: an impl for a type that cannot have one. No code appeared; no schema
moved.

**Applied to R1b, L10 (closures in generic functions, a shared `let mut`).** Two codes narrowed,
both compatibly — each now refuses less: `N0387` no longer covers a closure or a named function's
value written inside a generic function, and `N0386` no longer covers a captured `let mut` binding,
which is shared through a cell, except one whose type can hold a function value. No code appeared
and none moved; no schema moved.

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

**Applied to N80, provenance v3.** No code appeared. `N0372` keeps its meaning, and is now raised
also at a call through a value whose candidate writes to a path from its parameter, and at a closure
that captures what reaches a path, with a message saying which; its secondary labels follow cells,
captures, callers and exports. `checked-paths` refuses with the existing `N0510`. `nazm.flow/1 →
/2` adds `traces`; the check entry moved to `nazm.check/4`; `nazm.profile-report/1` gains a rule.

**Applied to N81, the backend contract v2, stage one.** No code appeared: a layout table that does
not hold is the existing `N0900`, a compiler defect, raised before any code is generated.
`nazm.lir/1` is new (`nazm lir`), with `schema/nazm.lir-1.json` checked against real output.

**Applied to N82, the runtime as an artifact.** No code appeared: an artifact that does not fit is
refused with one sentence naming the mismatch, not a code — exit 1 since Q1-C1 (the artifact named
does not fit the request), 3 when it cannot be read; it was 4, a missing toolchain's status. `nazm.runtime/1` is new, with `schema/nazm.runtime-1.json` checked against real
manifests of every profile; `nazm.provenance/1` gains an optional `runtime.archive`.

**Applied to N83, the task pool.** No code appeared: a task past its stack is the existing
`N0408`, a pool that was asked for and could not start makes `spawn` fail as a thread that could not
be created does (`N0404`), and five C symbols the pool calls join the reserved list (`N0383`).
`NAZM_SCHED_REPORT` gains a second line, `nazm-pool: workers=… peak=… stack=…`, printed only where
the pool ran; the first line is unchanged.

**Applied to N85, FFI v3.** Four codes appeared, all compatible. `N0409` is a run-time failure: a
foreign function returned null where its declaration promised a value — a handle or a `Str` not
written as an `Option` — raised after C returns and before Nazm sees the pointer, status 2 as every
failure. `N0611` (an opaque handle built or taken apart) and `N0612` (a callback that is not an
export's name) are the checker's, continuing N78's block. `N0613` names `c_errno()` in a build for a
freestanding or WebAssembly target, inside the existing refusal of what only a hosted runtime has.
Existing codes answer for the new types without changing meaning: `N0381` for what may cross where
(now: handles, C structs, export function types; and `Str` and handle results), `N0304` for `==` on a
handle, `N0321` and `N0390` for a handle reaching a task, `N0380`/`N0384` for an `extern "C" struct`'s
ABI and type parameters, `N0385` for `c_errno()` under `nazm run`. The persisted interface moved to
`nazm.interface/9` (a record's `c`); `nazm.runtime/1` gains the service name `foreign`, additive.

**Applied to N86, embedded v2.** No code appeared. The four device-register widths are refused
where there is no device exactly as the 32-bit pair is (`N0392`), whose help now names both boards;
a RISC-V build on a host whose `clang` has no RISC-V code generator is an environment refusal, exit
4 and one sentence, as a missing toolchain is.

**Applied to N87, contracts.** Four codes appeared, all compatible. `N0410` and `N0411` are run-time
failures — a precondition or a postcondition that evaluated `false`, at the clause, the same sentence
from the interpreter and both backends. `N0614` (a clause that is not pure) and `N0615` (a call of
literals that breaks its callee's precondition) are the checker's, continuing N78's block. A clause
that is not a `Bool` is the existing `N0300`. `nazm.obligations/1` is new, with
`schema/nazm.obligations-1.json` checked against real output; the persisted interface moved to
`nazm.interface/10` (an export's `requires`).

**Applied to N90, packages v3.** No code appeared. `N0513`'s meaning is unchanged — no choice of
versions satisfies every requirement — and it is now said after a backtracking search, naming the
package no version fitted where the search got deepest and every requirement on it with who placed
it; a search that reaches its 10,000-candidate bound is `N0513` saying it stopped. `nazm update`
naming no registry package is the existing `N0501`.

**Applied to N91, debugger and profiler v3.** No code appeared. One schema moved, compatibly in
every field it had: `nazm.profile/1 → /2` adds `samples` — `null` unless `--sample`, else the
sampler, the interval, the total, the samples by Nazm function and line, by runtime, foreign and
system symbol (each system frame with `blocked`), the blocked count and the unattributed count. The
removed refusal of `--debug` with Cranelift was a toolchain message, not a code.

**Applied to N92, performance evidence v2.** No code appeared. `nazm.bench/1 → /2`, additive:
`cpu_model`, `memory_bytes`, `kernel`, and `cpus` read on Linux too; per measurement `p90_ms`,
`max_ms`, `mad_ms` and `runs_ms`; `noise_floor_ms`, `binary_bytes`, `references` (how each is built
and whether its arithmetic is checked) and `not_measured` (each absent reference, with why). A `/1`
baseline is still read by `--check`. `bench/claims.toml` is new and is a register, not a schema.

**Applied to N103, re-exports.** Three codes: `N0211`, a `pub use` names something the module it
re-exports from does not offer (or renames a trait, which is offered under its own name only);
`N0212`, a module offers one name twice; `N0213`, a cycle of re-exports, at each `pub`. Each at the
name or keyword it is about. Two schemas moved: `nazm.interface/10` → `nazm.interface/11` (a re-export's export
carries `reexport` and the defining module's identity; `reexported_types`), and
`nazm.api-doc/1` → `nazm.api-doc/2` (a re-export is an item of kind `reexport` with `of`, the identity it
offers).

**Applied to N106, the pool by default.** No new code. `N0404`, *this task could not be started*,
is now provoked on purpose as well: a `NAZM_SCHEDULER` that names neither `pool` nor `threads` starts
no task, and the first `spawn` reports it at its span — the unprovokable list lost it. One schema
moved: `nazm.cost/1` → `nazm.cost/2`, additive — `scheduler`, `null` for a program that starts no
task, otherwise `default` (`pool` or `threads`), `why`, `workers`, `task_stack_bytes` and the
`environment` that changes it.

**Applied to N105, one LIR both backends consume.** No code appeared: a module LIR's validator refuses
is still `N0900`, a compiler defect, and its message names the function and the broken rule. One
schema moved: `nazm.lir/1` → `nazm.lir/2`, additive — each unit carries `lir`, its instruction-level
LIR as text (what `nazm lir --ops` prints) and that text's BLAKE3 `digest`, validated before it is
shown. `nazm lir --run` (LIR's interpreter) writes the program's own output and status, and exits 1,
naming what it does not run, for a program outside its subset (4 until Q1-C1); a spent budget is 2.

**Applied to Gate 1 and Gate 1-C1, Nazm 1.0.** One code appeared, compatible: `N0616`, a closure
capturing a value that can hold a function value behind a counted handle — the one way a closure's
environment could come to own itself (`spec.md`, *Cycles*). It names the captured binding and its
type, and the handle through which it can hold function values where that differs; it carries no
fix, because the remedy — passing the value as an argument instead — changes a signature. It
continues N78's block, the N03xx block being full. One refusal changed code: `1.5` is `N0101`
(floating point, refused by name) where it was `N0100`; `N0100`'s meaning is unchanged, and no
code's meaning moved. `nazm.capabilities/1`'s `types` gained the five capability types it lacked,
new array values.

## The code registry

Every code `nazm` can report, generated from `crates/nazm-diag/src/lib.rs` (`codes`) and held to it
by `crates/nazm-cli/tests/codes.rs`, which fails if a declared code is missing here, a row names a
code that is not declared, or two rows share a code (v1, Q1-D-11). A code's meaning is the first
sentence of its declaration's comment, or its constant's name where it has none; the sections above
say how each family is used. A **reserved** row names a code that is not declared and will not be
given a different meaning. Codes are never reused (*What compatibility means*).

<!-- registry:begin -->
| Code | Constant | What it reports |
| --- | --- | --- |
| `N0001` | `UNKNOWN_CHARACTER` | Unknown character |
| `N0002` | `UNTERMINATED_STRING` | Unterminated string |
| `N0003` | `INTEGER_TOO_LARGE` | Integer too large |
| `N0004` | `INVALID_ESCAPE` | A backslash in a string literal that is not one of the escapes `docs/spec.md` lists (N49) |
| `N0007` | `UNTERMINATED_COMMENT` | A block comment, `/* … */`, that the file ends inside; block comments nest (R1, 5b.17, `docs/spec.md`, *Comments*) |
| `N0006` | `INTERPOLATION` | An interpolation's hole that is not one: empty, unclosed on its line, a lone `}`, a format interpolation does not have, a precision on something not a float, or not one expression (R1a, `docs/spec.md`, *String interpolation*) |
| `N0005` | `STRING_LAYOUT` | A multi-line string with text after its opening `"""`, or a line not indented as its closing `"""` is (R1a, `docs/spec.md`, *Multi-line and raw strings*) |
| `N0100` | `UNEXPECTED_TOKEN` | Unexpected token |
| `N0101` | `UNSUPPORTED_CONSTRUCT` | Unsupported construct — **the umbrella code** for a construct this implementation refuses; its message names the construct, and a more specific code replaces it where one exists (Q1-D-13) |
| `N0102` | `NESTING_TOO_DEEP` | An expression or block nested past the parser's limit |
| `N0200` | `UNDEFINED_VARIABLE` | Undefined variable |
| `N0201` | `UNDEFINED_FUNCTION` | Undefined function |
| `N0202` | `ALREADY_DEFINED_IN_SCOPE` | Already defined in scope |
| `N0203` | `DUPLICATE_FUNCTION` | Duplicate function |
| `N0204` | `MISSING_MAIN` | Missing main |
| `N0205` | `MODULE_NOT_FOUND` | `use "path";` named a file that could not be read |
| `N0206` | `PRIVATE_DEFINITION` | A name that exists in an imported module, and is private there |
| `N0207` | `AMBIGUOUS_IMPORT` | Two directly imported modules export the same name |
| `N0208` | `IMPORT_COLLIDES_WITH_DEFINITION` | An import brings in a name this module also defines |
| `N0209` | `ALIAS_COLLIDES` | `use "PATH" as NAME;` where `NAME` is already a function, a type or another import's name in the importing module (N49) |
| `N0210` | `UNKNOWN_QUALIFIED_NAME` | `NAME::item` where `NAME` is no alias, or the aliased module exports no `item` (N49) |
| `N0211` | `REEXPORT_UNKNOWN` | A `pub use` names something the module it re-exports from does not export (N103) |
| `N0212` | `REEXPORT_COLLIDES` | A module offers one name twice, its own or a re-export's (N103) |
| `N0213` | `REEXPORT_CYCLE` | Modules re-export from each other in a cycle (N103) |
| `N0300` | `TYPE_MISMATCH` | Type mismatch |
| `N0301` | `CONDITION_NOT_BOOL` | Condition not bool |
| `N0302` | `BRANCH_TYPES_DIFFER` | Branch types differ |
| `N0303` | `WRONG_ARGUMENT_COUNT` | Wrong argument count |
| `N0304` | `INVALID_OPERANDS` | Invalid operands |
| `N0305` | `UNKNOWN_TYPE` | Unknown type |
| `N0310` | `ASSIGN_TO_IMMUTABLE` | Assign to immutable |
| `N0311` | `MUTABLE_PARAMETER` | Mutable parameter |
| `N0312` | `NOT_IN_A_LOOP` | `break` or `continue` outside a loop |
| `N0313` | `UNREACHABLE_STATEMENT` | A statement that provably cannot run |
| `N0314` | `CONTROL_FLOW_AS_VALUE` | `break` or `continue` where a value is expected |
| `N0320` | `SPAWN_OUTSIDE_SCOPE` | `spawn` outside a `scope` |
| `N0321` | `ILLEGAL_TASK_CAPTURE` | A value that may not cross into a task |
| `N0330` | `DUPLICATE_TYPE` | Two types declared with one name in one module |
| `N0331` | `BUILTIN_TYPE_REDEFINED` | A type declared with a built-in type's name |
| `N0332` | `DUPLICATE_FIELD` | Two fields of one record, or of one variant's payload, with one name |
| `N0333` | `UNKNOWN_FIELD` | A field named in a construction, a projection or a pattern that the record or variant does not have |
| `N0334` | `MISSING_FIELD` | A construction that does not give every field a value, or a pattern that does not name every payload field |
| `N0335` | `INVALID_PROJECTION` | `.field` on something that is not a record — including an enum, whose payload is reached by `match` and not by projection |
| `N0336` | `RECURSIVE_RECORD` | A user-defined type that contains itself by value, directly or through others |
| `N0337` | `PRIVATE_TYPE_EXPOSED` | A public function, record or enum whose signature, fields or payloads name a private type, which an importer could not resolve |
| `N0338` | `TYPE_NESTING_TOO_DEEP` | User-defined types nested inside one another past the stated bound |
| `N0339` | `EMPTY_ENUM` | An enum with no variants |
| `N0340` | `DUPLICATE_VARIANT` | Two variants of one enum with one name |
| `N0341` | `UNKNOWN_VARIANT` | A variant named in a construction or a pattern that the enum does not have |
| `N0342` | `MATCH_ON_NON_ENUM` | `match` on a value that is not an enum |
| `N0343` | `NON_EXHAUSTIVE_MATCH` | A `match` that does not name every variant of its enum |
| `N0344` | `DUPLICATE_MATCH_ARM` | A `match` that names one variant twice |
| `N0345` | `WRONG_ENUM_IN_PATTERN` | A pattern whose variant belongs to a different enum than the scrutinee |
| `N0350` | `DUPLICATE_TYPE_PARAMETER` | Two type parameters of one declaration with one name |
| `N0351` | `TYPE_ARGUMENTS_ON_NON_GENERIC` | Type arguments written on something that takes none |
| `N0352` | `MISSING_TYPE_ARGUMENTS` | A generic definition, or `Vec`, written where a complete type is required but without its type arguments |
| `N0353` | `WRONG_TYPE_ARGUMENT_COUNT` | A generic definition applied to the wrong number of type arguments |
| `N0354` | `CANNOT_INFER_TYPE_ARGUMENT` | A generic call whose arguments do not determine every type parameter |
| `N0355` | `CONFLICTING_TYPE_ARGUMENTS` | Two arguments of one call that give one type parameter different types |
| `N0356` | `GENERIC_MAIN` | A generic `main` |
| `N0357` | `UNBOUNDED_SPECIALISATION` | A generic function whose native instances would never end — polymorphic recursion, `f[T]` calling `f[Vec[T]]` |
| `N0358` | `GENERIC_INSTANCE_LIMIT` | More native generic instances than one program may have, or one nested too deeply |
| `N0359` | `OWNERSHIP_CYCLE` | A type that can own itself through a reference-counted container — `Node { children |
| `N0360` | `PROPAGATE_NON_RESULT` | `?` applied to something that is not the core prelude's `Result` — an `Option`, a project enum shaped like one, anything else |
| `N0361` | `PROPAGATE_OUTSIDE_RESULT` | `?` in a function whose return type is not the core prelude's `Result` |
| `N0362` | `PROPAGATE_ERROR_MISMATCH` | `?` whose operand's error type is not exactly the enclosing function's |
| `N0363` | `PRELUDE_TYPE_REDECLARED` | A project type named like one of the core prelude's, which every module sees |
| `N0364` | `UNSATISFIED_REQUIREMENT` | A type argument, written or inferred, that does not have a capability its parameter requires — a concrete type without equality, or a caller's own parameter whose … |
| `N0365` | `UNKNOWN_REQUIREMENT` | A requirement naming something that is not a capability |
| `N0366` | `UNDECLARED_EFFECT` | A function whose declared effect set lacks an effect its body requires |
| `N0367` | `UNKNOWN_EFFECT` | A name in an effect set that is not an effect |
| `N0368` | `DUPLICATE_EFFECT` | An effect written twice in one set |
| `N0369` | `MISSING_AUTHORITY` | An operation whose authority the function does not hold (N37) |
| `N0370` | `CAPABILITY_CONSTRUCTED` | An attempt to build a capability |
| `N0371` | `MAIN_PARAMETER` | A `main` parameter that is not a capability |
| `N0372` | `RESTRICTED_FLOW` | A restricted flow (N38) |
| `N0380` | `FOREIGN_ABI` | A foreign declaration names an ABI other than `"C"` (N42) |
| `N0381` | `FOREIGN_TYPE` | A foreign declaration takes or returns a type that cannot cross the C boundary (N42) |
| `N0382` | `FOREIGN_SYMBOL` | A foreign declaration's symbol is not a C identifier a linker can be handed (N42) |
| `N0383` | `FOREIGN_CONFLICT` | Two foreign declarations of one symbol disagree about its signature (N42) |
| `N0384` | `FOREIGN_SHAPE` | A foreign declaration has type parameters or a written effect set (N42) |
| `N0385` | `FOREIGN_NOT_INTERPRETED` | `nazm run` was asked to run a program that calls a foreign function (N42) |
| `N0386` | `CAPTURE_REFUSED` | A closure captures a `let mut` binding whose type can hold a function value (N50; narrowed by N51 and R1b) |
| `N0387` | `NOT_A_FUNCTION_VALUE` | A function that cannot be a value — generic, foreign or built-in — is used as one (N50; narrowed by R1b) |
| `N0388` | `EFFECT_PARAMETER` | An effect parameter where none may be |
| `N0389` | `CONFLICTING_EFFECT_ARGUMENT` | A call binds its callee's effect parameter to two different sets (N51) |
| `N0390` | `CHANNEL_ELEMENT` | A `Chan[T]` whose `T` may not cross into a task (N53) |
| `N0391` | `EXPORT_SHAPE` | An export (N55) that is not public, declares an effect, is generic, or takes or returns a type other than `Int` and `Bool` |
| `N0392` | `MACHINE_ONLY` | A device-register built-in (`mmio_read32`, `mmio_write32`) reached by something that has no device registers |
| `N0393` | `COMPTIME_REFUSED` | A function `nazm comptime` will not evaluate |
| `N0394` | `KERNEL_INELIGIBLE` | A function `nazm accel` will not run as a kernel |
| `N0395` | `NOT_A_CONTRACT` | A module `nazm contract` was given that does not have a contract's shape |
| `N0396` | `CONSERVATION_BROKEN` | A transaction that would change the contract's conserved quantity |
| `N0397` | `CALLER_NOT_INT` | A sender whose address is not an `Int`, as the language's `caller` must be |
| `N0398` | `BAD_CALLDATA` | Calldata that is not a known entrypoint's selector and `int64` arguments |
| `N0399` | `EVM_UNSUPPORTED` | A contract construct the EVM backend does not emit, or a storage upgrade it refuses |
| `N0400` | `INTEGER_OVERFLOW` | Integer overflow |
| `N0401` | `DIVISION_BY_ZERO` | Division by zero |
| `N0402` | `ITERATION_LIMIT` | The interpreter's iteration guard, not a language rule |
| `N0403` | `CALL_DEPTH_LIMIT` | Runaway recursion, caught before the stack is exhausted |
| `N0404` | `INTERPRETER_UNAVAILABLE` | The host refused what running needs |
| `N0405` | `INDEX_OUT_OF_BOUNDS` | An index outside a value's bounds |
| `N0406` | `ALLOCATION_FAILED` | The allocator refused |
| `N0407` | `IO_FAILED` | A file could not be read or written |
| `N0408` | `STACK_EXHAUSTED` | A compiled program's call would exhaust its thread's stack (N49) |
| `N0409` | `NULL_FROM_C` | A foreign function returned null where its declaration promised a value (N85) |
| `N0410` | `PRECONDITION_FAILED` | A precondition that did not hold as the function was entered (N87) |
| `N0411` | `POSTCONDITION_FAILED` | A postcondition that did not hold as the function returned (N87) |
| `N0412` | `SHIFT_OUT_OF_RANGE` | A shift or rotation by a negative amount, or a shift by the operand's width or more (Gate 2) |
| `N0413` | `NOT_UTF8` | Bytes that are not UTF-8 made into a `Text`, a `Text` cut or read away from a scalar boundary, or an `Int` that is not a Unicode scalar value made into one (Gate 2) |
| `N0414` | `TASK_DEADLINE` | A board's task released again before it had run |
| `N0415` | `WATCHDOG_EXPIRED` | A board's watchdog expired |
| `N0416` | `EXIT_STATUS` | `exit_with` given a status outside `0..=255`, the statuses every host reports unchanged (v1, Q1-B-05, `docs/spec.md`, *Ending*) |
| `N0500` | `MANIFEST_INVALID` | A manifest that cannot be read as `nazm.package/1` |
| `N0501` | `PACKAGE_NOT_FOUND` | A dependency's directory or manifest is not there |
| `N0502` | `VERSION_MISMATCH` | A dependency's manifest states another version than the one declared for it |
| `N0503` | `PACKAGE_CONFLICT` | Two packages of one name in one build |
| `N0504` | `PACKAGE_CYCLE` | A package depends on itself, through its dependencies |
| `N0505` | `LOCK_MISSING` | `--locked`, and there is no lockfile |
| `N0506` | `LOCK_MISMATCH` | `--locked`, and the packages resolved are not the ones the lockfile records |
| `N0507` | `DIGEST_MISMATCH` | A package's bytes no longer match the digest the lockfile records for it |
| `N0508` | `UNKNOWN_DEPENDENCY` | An import names a package the importing package does not declare |
| `N0509` | `NO_ENTRY` | A package built as a program has no `main` |
| `N0510` | `PROFILE_VIOLATION` | A program that breaks a rule of a profile in force |
| `N0511` | `UNKNOWN_PROFILE` | A profile named on the command line or in a manifest that does not exist |
| `N0512` | `REGISTRY_INVALID` | A registry index that is not `nazm.registry-index/1`, names another package, records a version twice or a digest of the wrong shape; or a publish of a version already … |
| `N0513` | `VERSION_UNSATISFIABLE` | No version in the registry satisfies every requirement on a package |
| `N0514` | `PACKAGE_UNSAFE` | A registry package holding a symbolic link or anything but files and directories |
| `N0600` | `IMPL_CONFLICT` | Two impls of one trait for one type in one compilation |
| `N0601` | `IMPL_ORPHAN` | An impl in a module that declares neither the trait nor the type |
| `N0602` | `IMPL_MISSING_METHOD` | An impl that does not define every method of its trait that has no default body (narrowed by R1b) |
| `N0603` | `IMPL_METHOD_MISMATCH` | An impl method its trait does not declare, or one whose signature is not the trait's with `Self` replaced by the impl's type, effects included |
| `N0604` | `IMPL_TYPE_REFUSED` | An impl for a type that cannot have one, or one with type parameters not written for a generic record or enum applied to exactly them (R1b) |
| `N0605` | `NO_METHOD` | A method call no trait in scope provides for the receiver's type |
| `N0606` | `AMBIGUOUS_METHOD` | A method call more than one trait in scope provides |
| `N0607` | `TRAIT_NOT_A_TYPE` | A trait written where a type is expected |
| `N0608` | `METHOD_NOT_A_VALUE` | A method named without being called |
| `N0609` | `TRAIT_METHOD_SELF` | A trait method whose first parameter is not `self |
| `N0610` | `NOT_A_TRAIT` | A bound, an impl or a qualified call naming a type that is not a trait |
| `N0611` | `OPAQUE_HANDLE` | An opaque handle (`extern "C" struct S;`) built or taken apart |
| `N0612` | `CALLBACK_NOT_EXPORT` | A callback argument to C that is not an export's name |
| `N0613` | `ERRNO_UNAVAILABLE` | `c_errno()` in a build for a target with no `errno` |
| `N0614` | `CONTRACT_SHAPE` | A contract clause that is not made only of literals, names, operators, field reads and calls of pure functions |
| `N0615` | `PRECONDITION_REFUTED` | A call whose arguments are all literals and that breaks its callee's precondition |
| `N0616` | `CAPTURE_CYCLE` | A closure capturing a value that can hold a function value behind a counted handle — a `Vec[fn() -> Int]`, a `Vec` of records with a function field, a record holding one |
| `N0617` | `SHIFT_AMOUNT_REFUSED` | A shift whose amount is a constant outside `0..width` (Gate 2) |
| `N0618` | `ARRAY_TOO_LARGE` | An array over 64 KiB, or of no elements (Gate 2) |
| `N0619` | `ATTRIBUTE` | An attribute the language does not have, on a declaration it does not apply to, or with arguments it does not take (Gate 2, `docs/spec.md`, *Attributes*) |
| `N0620` | — | **reserved**, never assigned: kept for a warning at each use of a deprecated item, should a warning severity ever exist (Q1-D-14) |
| `N0621` | `ARRAY_ELEMENT` | An array whose element owns something — a `Str`, a handle, a function value — rather than plain data (Gate 2) |
| `N0622` | `NOT_A_CONSTANT` | A constant's value, or an array's length, that is not computed from literals, other constants, operators and numeric conversions (Gate 2) |
| `N0623` | `CONSTANT_FAILS` | A constant whose computation overflows, divides by zero or shifts out of range (Gate 2) |
| `N0624` | `ARRAY_INDEX` | An index that is not into an array, or a constant index outside it (Gate 2) |
| `N0625` | `STATIC_ASSIGNED` | An assignment to a static, which is read-only — changed, if an `Atomic`, only by the atomic operations (Gate 3, `docs/spec.md`, *Statics*) |
| `N0626` | `BOARD_MANIFEST` | A board's manifest the build cannot honour |
| `N0627` | `INTERRUPT_HANDLER` | An interrupt handler of a shape the board cannot call, two handlers for one interrupt, or one for an interrupt the board does not have (Gate 3, `docs/spec.md`, *Boards*) |
| `N0628` | `NOT_ITERABLE` | A `for` loop over something that is not an `Ints`, `Strs`, `Vec[T]` or `Bytes` (R1a, `docs/spec.md`, *`for` loops*) |
| `N0629` | `OR_PATTERN_BINDINGS` | An or-pattern's alternatives that bind different names, or one name with different types (R1a, `docs/spec.md`, *Patterns*) |
| `N0630` | `LET_ELSE_COMPLETES` | A `let … else` whose `else` can finish (R1a, `docs/spec.md`, *Patterns*) |
| `N0631` | `DEFER_LEAVES` | A `defer` body that would leave it: `return`, `?`, or `break`/`continue` out of it (R1b, `docs/spec.md`, *`defer`*) |
| `N0632` | `WILDCARD_COVERS_VARIANTS` | A **warning**: a `_` or a name in a `match` of an enum covers variants no arm names (R1b, `docs/spec.md`, *Patterns*) |
| `N0635` | `CLOSURE_PARAMETER_UNTYPED` | A short closure's parameter without a type where no function type is expected to take it from (R1.5, `docs/spec.md`, *Shorter forms*) |
| `N0636` | `VARIANT_AMBIGUOUS` | A variant written without its enum where two enums in scope have a variant of that name (R1.5, `docs/spec.md`, *Shorter forms*) |
| `N0637` | `BUILTIN_RENAMED` | A call of one of R1a's `list_` built-ins, renamed `chain_` with their type, `Chain[T]` (R1.5, `docs/spec.md`, *Immutable indirection*) |
| `N0634` | `RESULT_TYPE_NOT_INFERRED` | A function written `= e` with no result type that must write one: a `pub` function, a method, or one whose body reaches itself (R1.5, `docs/spec.md`, *Shorter forms*) |
| `N0850` | `BOARD_MEMORY` | An image, its data or its stack that does not fit a microcontroller board's flash or RAM, naming the region and the overflow (M1) |
| `N0851` | `BOARD_INIT` | A board manifest's `init` that names no function `fn NAME() -> Int ! {}` or `fn NAME(m: MmioCap) -> Int ! {}` of the program, or names more than one (M1) |
| `N0854` | `BOARD_FLOAT` | A floating-point value a microcontroller target cannot compute with: `Float64` on `thumbv8m.main-none-eabihf`, whose FPU is single-precision, and either float on `riscv32imc-unknown-none-elf`, which has none (M1) |
| `N0900` | `COMPILER_DEFECT` | The compiler contradicted itself (N39) |
<!-- registry:end -->

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
| 0 | Success — nothing to report |
| 1 | The source, program, package or request is invalid |
| 2 | The program ran and failed at run time |
| 3 | A file or resource the command needs could not be found or read |
| 4 | The compiler, a tool it runs, or the machine failed |
| 5 | The command line itself is wrong |

4 is separate from 1 on purpose: one means the request is wrong, the other means the
machine could not carry out one that is fine. 5 is separate from 2: a mistyped option is not a
program that failed. The full definitions, and what moved when the contract was frozen (Q1-C1),
are `docs/stability.md`, *Diagnostics and exit statuses*.

**Applied to Gate 3, requirements traceability.** No diagnostic code. `nazm.trace/1` is new, with
`schema/nazm.trace-1.json` checked against real output (`schemas.rs`): each requirement of a
`requirements.toml` (`schema = "nazm.requirements/1"`) to its definitions by durable identity and
digest, their obligations, and its tests' outcomes. A broken link — a definition renamed away, a test
missing or failing, a requirement naming nothing to verify it — is written to `broken` and to
standard error, and the command exits 1.

**Applied to the v1 fixes.** One code appeared, compatible: `N0416`, a run-time failure — `exit_with`
given a status outside `0..=255` (Q1-B-05). Until then such a status reached the host and was
truncated differently by each implementation; no program that ran with a status in range changes.
`N0381` gained a use, also compatible: on a 32-bit target, a foreign declaration whose crossing is a
C pointer (Q1-A1-05).

**Applied to R3, Core IR for a lowering in Nazm (2026-10-15).** No code appeared. One debugging
schema moved: `nazm.core-ir/2` → `nazm.core-ir/3`, a new major because a field's values changed —
a function or array type is spelled (`fn(Int) -> Str`, `[Int; 4]`) where `/2` wrote `?`. It also
gains each function's `pub`, `closure`, `captures` and `attributes`, and the root's `modules`,
`types` (records and enums in the checker's id order), `statics`, `impls` and `derived_hash`
(`architecture.md` §7.41). Every `/2` field stays where it was; the digests are unchanged.
