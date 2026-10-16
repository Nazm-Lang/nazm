## Context

The Rust interpreter (`crates/nazm-core/src/eval/`, ~5,600 lines) is a tree walker over
`cir::Program`. It recurses on the host stack once per expression and a dozen frames per Nazm
call, bounds Nazm calls at 512 (`--max-depth`) and loop turns at 10,000,000
(`--max-iterations`), runs each `spawn` on an OS thread, counts four storage classes for
`NAZM_MEMORY_REPORT` and four scheduler events for `NAZM_SCHED_REPORT`, and reports every
run-time failure as a `Diagnostic` rendered as compile errors are (status 2). `nazm run` prints
nothing for `main`'s `Int` result and exits with it, unless `NAZM_RESULT_REPORT` asks for it printed
(`crates/nazm-cli/src/interpret.rs`).

R4 replaces it with a program written in Nazm. Two constraints shape the design: the 0.10.0
language (no recursive type through a mutable handle, so an interpreted `Vec` holding values cannot
be a Nazm `Vec` inside the value type; no destructors, so reclamation is counted explicitly), and
slice 5 (tasks that block on channels must be suspendable without host threads sharing a heap,
which 0.10.0 does not allow — a `Vec` does not cross `spawn`, `N0321`).

## Goals / Non-Goals

**Goals:** byte-identical stdout and stderr and the same exit status as the Rust interpreter on
every input of the suite; the same memory and scheduler reports; the same budgets and their
diagnostics; written in the 0.10.0 language; fast enough that the interpreter-driven test suites
do not slow down noticeably.

**Non-goals:** parsing or checking source (R2's — the input is checked Core IR); a new semantics
(where the Rust and `docs/spec.md` disagree the spec is the authority and the case is reported, not
silently followed); C calls (`nazm run` refuses them before running, `N0385`, a check the front end
makes); the `comptime`, `contract`, `accel` and `repl` drivers (R5 commands that call the
interpreter — the code is shaped so they can).

## Decisions

### D1. Input and invocation

```
interp [--json] [--max-iterations N] [--max-depth N] CORE_IR.json ROOT.nz [-- ARGS...]
```

`CORE_IR.json` is `nazm core-ir --json ROOT.nz` (spans included). `ROOT.nz` is the path exactly as
the user gave it to `nazm run`: the diagnostic renderer prints it, and the source files are read
from it — the root module's key names the root file, and every other user module's key is a path
relative to the root's directory. Until R5 the harness produces the Core IR; at R5 `nazm run`
does both steps in one process. `ARGS` are what `arg(i)` sees, the program's name excluded.
Environment: `NAZM_RESULT_REPORT`, `NAZM_MEMORY_REPORT`, `NAZM_SCHED_REPORT`, read as the Rust
driver reads them.

### D2. Load: JSON to a flat code, once

`@std/json` parses the file into its arena; one pass lowers it into the interpreter's own tables,
after which the JSON is dropped:

- **functions**, by def key, sorted for binary search: parameter and local counts, the closure
  flag and capture slots, the entry address in the code;
- **constants**, interned per program: `Int`, `Bool`, `Str` (the printer's `\xNN` form decoded:
  a constant whose text contains `\x` is wholly in that form), numbers of other kinds;
- **types**, interned as strings: a value records the type key it needs (a record's or enum's
  definition, a number's kind) and nothing else;
- **code**: parallel `Ints` — opcode and up to three operands, plus the span (module, start, end)
  per instruction. Structured control flow is lowered to jumps; each region exit, on every path
  that leaves it (fall-through, `break`, `continue`, `return`), gets an explicit `release` of the
  slots the region owns, in the order Rust's `Block::owns` loop releases them.

Lowering is a recursive walk over the JSON tree (bounded by the parser's nesting limit, so host
recursion here is safe); execution is not recursive.

### D3. Execution: one loop, explicit frames

A task is a value stack (operands), a slot stack (each frame's locals, `Unset` when unbound) and a
frame stack (function, return address, slot base, operand base). A call pushes a frame — checking
the depth budget first, with the Rust sentence `` `f` recursed deeper than N `` at the call's span —
and a `ret` pops it. A loop's back edge charges the iteration budget once per turn, at the point the
Rust `while_loop` does (after the condition holds, before the body). Because nothing recurses on the
host per Nazm call, `--max-depth` can be raised without the stack the Rust interpreter must reserve;
its "deeper than the interpreter's stack holds" failure never arises here — reported as a
difference only if a test depends on it.

### D4. Values and the heap

```
enum V { Unset, Unit, Int(v), Bool(v), Str(s, obj), Num(kind, bits…), Arr(items: List[V]),
         Rec(def, fields: List[V]), Enum(def, variant, fields: List[V]), Ref(obj), Cap(kind) }
```

- `Int`, `Bool`, numbers and capabilities are plain.
- A **record, enum value or array** is an immutable aggregate, as the language makes it (no object
  identity); a field assignment builds the new aggregate along its path.
- A **`Str`** is a Nazm `Str` (a view: slicing shares, as the Rust `Str` does) plus `obj`, the heap
  object of its backing, or 0 for an immortal one (a literal, an argument).
- Every **handle** is `Ref(obj)`: an index into the heap, which holds for each object its kind, its
  reference count and its contents (`Vec[V]` storage for `Vec`/`Ints`/`Strs`/`Box`/`List`/`Cell`,
  bytes for `Bytes`, buffer and waiters for a channel, function and captures for a function value,
  the cell of an `Atomic`, the room of a grown string backing). `Box` and `List` are sequences, as
  in the Rust interpreter, so they count as sequences.

**Counting.** The Rust interpreter reclaims by `Drop`; this one by explicit `retain`/`release`
at the same points: a value read from a slot or a field is retained; a value written over, a
slot a region releases, an operand a built-in consumed, a scrutinee after its arm, are released.
An object whose count reaches zero is reclaimed, counted in its class, and releases what it holds
through a worklist (never recursion), as Rust's `release` does. A slot of a free object is reused.
The memory report is the four classes' allocated and reclaimed counts, printed after the run —
including after a failure, after which every frame has been unwound and released.

**Aggregates count once.** A record or enum value whose fields hold a reference carries one heap
object of its own (`K_AGG`, never in the memory report) that counts its copies, packed into its
`def` with its type's name: copying it takes one reference, not one per field, and its fields are
released only when its last copy goes. A value of plain data is copied at no cost. A field
assignment on the only copy moves the fields into the new value. Built-in arguments that are a
constant, a local or a local's projection are lent rather than copied (`OP_BORROW`, `OP_LFIELD`):
nothing between their push and the built-in can fail or leave, so the slot keeps them alive. None
of this is visible in the report; it is what keeps the interpreter's speed near the Rust one's.

**Append in place.** `s = str_append(s, t)` takes the slot's value out first, as Rust's
`exec::append` does; the backing grows in place exactly when its count is one and the result fits
its `room`; otherwise a copy into `max(2n, 16)` room, counted as a new string.

### D5. Control and failure

`break`, `continue` and `return` are jumps preceded by the releases of every region they leave
(D2). A run-time failure records code, message, help and span, then unwinds the task: every frame's
slots and operands are released. The renderer reproduces `Diagnostic::render`:

```
error[CODE]: MESSAGE
  --> FILE:LINE:COLUMN
PAD |
LINE | SOURCE-LINE
PAD | <spaces>^^^ MESSAGE
PAD = help: HELP
```

with the column counted in characters, the caret width in bytes clamped to the line's remaining
characters, and `--json` writing `nazm.diagnostic/1` with spans as concatenation offsets.
Exit status 2. `exit_with` flushes and exits with its status; no report follows it, as in Rust.

### D6. Built-ins

Each `builtin` name is resolved to a number at load; an unknown name fails the load naming it (so
a built-in added to the Rust toolchain is a visible gap, not a silent difference). `Ints`, `Strs`,
`Box`, `List` operations are the `Vec` operations, as Rust maps them. Every sentence (message and
help) is copied from the Rust source and cited by file in the code.

### D7. Numbers

`Int` uses Nazm's `checked_add`/`checked_sub`/`checked_mul`; a `None` is the Rust failure. The
other widths are `Num(kind, value)` held in an `Int128` (an unsigned 128-bit value by its bit
pattern) with each kind's range checks, and floats in a `Float64` (a `Float32` rounded after each
operation through the language's own `Float32` conversion). Textual forms follow
`nazm_sema::numeric::to_str`; any rounding difference is a finding.

### D8. Traits, ordering and hashing without the printed tables

`call_trait` names `trait.method` and a type that mentions the frame's type parameters. The
printed Core IR does not carry the impl table, so dispatch uses the receiver's runtime type (as
Rust does) and finds the impl function by its key, `MODULE::fn Trait/Type/method`; a lookup with
no or several candidates fails as a compiler defect naming the call. Derived order needs the
canonical orders: they are rebuilt from the names the printed `record`, `variant`, `match` and
`field` nodes carry (fields and variants sorted by name). This is exact for any value the program
can make, because only constructed variants can be compared, but it is a reconstruction; see
*Requests*.

With `nazm.core-ir/3` (main, 2026-10-15) the printed tables answer first and the reconstruction is
only the fallback for a `/2` input: `impls` maps each `trait method | self type` to its function,
a trait's default method included (which no `Trait/Type/method` key names, so a default reached
through a generic call failed before); `types` gives each record's, enum's and variant's canonical
order; `statics` gives each static `Atomic` its initial value (it started at 0 before). A static is
found by its durable key (main `ef1bf70a`): the constant's `static` field is the `def` of its
`statics` entry, so two modules each declaring `static COUNT` are two cells (case
`tests/cases/statics/`); a `/2` input, which has neither, keys the cell by its `value`.

### D9. Concurrency (slice 5)

Tasks are the interpreter's own: each a task of D3 (its stacks), scheduled cooperatively on one
host thread. `spawn` creates a runnable task (counted `spawned`); a `scope` exit waits for its
tasks (counted `joined`); a channel operation that cannot proceed suspends the task on that channel;
`chan_select_*` and `time_sleep_ms` suspend with a deadline on the monotonic clock. When every task
is suspended and one has a deadline, the host sleeps until the earliest; when none has one, the
program is deadlocked — the Rust interpreter would hang, so this is reported as a new failure only
behind a decision (open question 1). One heap, so a value sent through a channel shares its backing
as in Rust, and the counts agree. A task whose `chan_select_until` times out gives way at its next
loop turn, so one polling with a zero timeout lets the task it waits for run, as that task's own
thread would on the Rust interpreter (`selects` and `timeouts` then agree).

Rejected: running interpreted tasks on Nazm tasks. 0.10.0 lets nothing but channels, atomics and
immutable values cross `spawn`, so each task would need its own heap and every channel message an
encoding — a received `Str` would be a new allocation the Rust interpreter never counts.

### D10. Files, processes, time

The `os_*`, `read_file`/`write_file`, process and time built-ins map onto the same 0.10.0 built-ins,
which run with the interpreter's own capabilities (`IoCap`, `ProcessCap`, `TimeCap`, `NetCap`,
`RandomCap`): the program's authority was proved by the checker, the interpreter only exercises it.
Handles are interpreter heap objects wrapping the host's `OsHandle`, so the generation check of
the Rust `os` module is kept.

**Sockets on one thread.** The Rust interpreter gives each task a thread, so a socket operation
that waits holds only its own task; here every task shares one thread (D9). So `os_tcp_accept`,
`os_read` and `os_udp_recv_from` on a socket are tried with a deadline of one millisecond; when it
runs out, the task waits. A try that finds nothing holds the whole interpreter for as long as
0.10.0's runtime takes to time out a 1 ms deadline — about 100 ms on macOS (`nz.timed_nap`, a
finding for the coordinator) — so a waiting task tries again only after a whole round of the
scheduler in which no other task progressed (the task that would answer it may be one of them),
and no sooner than 25 ms after its last try, doubling up to 400 ms — until the deadline the program gave the socket with `os_set_timeout`, if any, has passed — then
the last try's own answer (`ETIMEDOUT`) is the program's. Each socket value carries its place in
the heap's table of the deadlines the program set. A child's pipe (`os_child_pipe`) is read the
same way: `process_run` reads the child's output while another task collects its errors, and a
child held writing a full pipe would otherwise hold the whole run. Writes and `connect` are not
polled: they do not wait on another task of the same program in any input.

**The clocks.** `time_now_ms` and `time_now_ns` count from the program's first reading of each, as
the Rust interpreter's do (`intrinsic.rs`, `monotonic_ms`), and `os_wait_until`'s deadline is on
that clock. A socket's deadline is the runtime's (`os_set_timeout`); the waits around it here are
on `time_now_ms`, monotonic (main `0a8f07cf` moved `nz.net_now` there too). A program that
returns `time_now_ms()` from `main` ends with status 0 on both.

### D11. What `nazm run` refuses before the program starts

`nazm_core::pipeline` refuses, after checking and before running, a call into C (`N0385`: a call
of a function the Core IR marks `foreign`, or of `c_errno`), a device register's built-in (`N0392`:
`mmio_*`, `watchdog_feed`) and a placement only a board's image has (`N0392`: a function's
`@section`, `@interrupt`, `@on_failure` or `@task`, a static's `@section`), in that order, each the
first in the program's functions. The loader records the first refusal of each rank; `main`
reports the lowest one as the Rust toolchain does — the reports with nothing counted, the
diagnostic with its own label, `nazm: 1 error` — with the check's status, 1. The span of a call is
its callee's name, from where the call starts; a function's is its name, found in the source from
the function's start; a static's is the one the table prints.

## Requests

To the coordinator — none blocks slices 2 and 3:

1. **The printed Core IR should carry `impls`, `templates`, `orders`, `derived_hash` and `statics`**
   (additive fields of `nazm.core-ir/2`, or `/3`). The in-memory `cir::Program` has all five; the
   printer (`crates/nazm-cir/src/print.rs`) writes only `schema`, `entry` and `functions`. Without
   them D8 is a reconstruction and static `Atomic` cells have no initial value. *Answered by
   `nazm.core-ir/3` (main, 2026-10-15); reading it is task 6.4.*
2. **`nazm run --interpret`** is not on `main`; the suite compares with plain `nazm run`, which is the
   interpreter in 0.10.0. *Answered (R1.5 D11): the suite uses `--interpret` where the reference
   has it.*

## Differences that remain

`run.sh --rust-tests` leaves 13 or 14 of the 2,686 comparisons, none the interpreter's to remove:

- **Different on every run of the Rust interpreter too:** process and group ids
  (`platform_files-18`, `platform_files-m1`, `process_control-2`, `-3`, `-11`), a span's
  nanoseconds (`stdlib-12`), the order 32 threads finish in (`scheduler_default-5`), how often a
  task polling with a zero timeout spins before the thread it waits for runs
  (`scheduler_pool-10`: 78 to 2,158 selects on four Rust runs).
- **Timing:** `process-1`'s memory report counts one string per read of a child's pipe, and how
  much a read finds depends on when it runs.
- **The comparison itself:** `process-2` lists its environment, and the Rust run's has the three
  report variables, which this interpreter takes as flags.

`collections_r1e-2` (300,000 ordered-map inserts) agrees under `--rust-tests`' 300 s bound.

## The Rust tests' programs

`run.sh --rust-tests` takes the programs `crates/nazm-cli/tests/*.rs` embeds, as
`tests/rust-tests.pl` writes them (2026-10-16): each string literal with a `fn main` as a program of
one file, as before (now with Rust's `\xNN` escapes undone too: 32 programs of `build.rs` indent
with `\x20` and did not parse); and, new, the files a test names — `("lib.nz", "...")`,
`write(dir, "lib.nz", "...")`, `.join("lib.nz"), r#"..."#`, a name beside a `const` — as programs
of several modules, written as the test's directory holds them: a name written again first closes
the program so far, and the root is the file with a `fn main` written last.

Of the 869 programs that did not check with the old extraction (the brief's 858, plus main's new
tests), none was a program `nazm run --interpret` runs: each one the checker refuses (status 1) or
a module it cannot find (status 3). With this extraction 849 do not check; sorted:

| | programs | what they are | carried? |
| --- | --- | --- | --- |
| Refused on purpose | 462 | the test asserts the code the checker gives (it appears in the test file) | no — the checker's, not the interpreter's |
| A module not found | 164 | a module of a program of several modules, alone; most are now also part of a program of several modules that checks | yes, as those programs |
| A name not found | 109 | a snippet the test completes with a prelude of its own (`format!`, `concat!`, a helper's text) | no — stale snippets |
| Other refusals | 114 | text deliberately broken (`N0100`, 26 more), a package's module outside its package (`N0508`, 24), the rest refusals whose code the test does not spell | no |
| No `main` | 0 | | |

What the interpreter should carry, it now does: 409 programs of several modules, 274 of which
check, and the 32 `\x20` programs. In all: 2,056 programs, 1,207 that check, 2,686
comparisons (before this round: 1,808 programs, 939 that checked; 2,067 comparisons agreed).

## Risks / Trade-offs

- **Explicit counting can drift from `Drop`.** Every difference shows in the memory report on some
  case of the suite, which is why the report is compared, not just the output.
- **Speed.** A loop interpreter compiled natively should match or beat the Rust tree walker; the
  suite records both times so a regression is visible.
- **Messages copied by hand.** A sentence changed in Rust and not here is a difference the suite
  catches only where a case fails that way; the `trap/` corpus covers each code once.

## Open Questions

1. A deadlock (every task suspended, no deadline): the Rust interpreter hangs. Report it as a
   failure with a new code, or hang to agree? Recommendation: report — a new code is the
   coordinator's call, so until then the Nazm interpreter hangs too.
2. `--max-depth` above what the Rust interpreter's stack can hold: the Rust one fails with
   "recursed deeper than the interpreter's stack holds"; this one does not. Recommendation: keep
   the difference; it is the Rust interpreter's limit, not the language's.
