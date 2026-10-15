# Design — v1-every-domain

## Context

1.0 waits for five domain tracks (`proposal.md`). Each track's design is written here before any of
its code, one top-level section per track. This file holds **H — hardware and chip design** (task
0.4) and **Q — quantum computing** (task 0.5); C, M and D are written by their own tracks and sit
beside these sections.

Both designs start from what the language already is at `3e4db897`, and are held to its
constitution rather than bending it:

- **A profile only refuses** (`docs/spec.md`, *Restriction profiles*): it never changes what an
  accepted program does. Both domains are restrictions of ordinary Nazm plus a backend, never a
  second language with a second meaning.
- **One meaning, many oracles** (`docs/architecture.md`, *Two backends is two sets of semantics
  bugs*): every new backend is held, by differential test, to the interpreter's meaning of the same
  program, and to an independent outside implementation where one exists — as the EVM backend is
  held to py-evm (§7.72) and accelerator kernels to the interpreter and the CPU provider (§7.69).
- **Constitution before code** (`docs/stability.md`, *How a change is made*): every rule below
  enters `docs/spec.md` first, with its diagnostic code, an epoch bump and a compatibility-corpus
  case, in the same commit as the checker change.
- **The rewrite direction**: Rust is frozen as the seed. The two new backends are written in Nazm,
  under `toolchain/`, as separate programs that read a printed IR from the compiler — the pattern
  `nzcg` set (§7.116). Language rules (types, checker) land in both the reference checker and the
  compiler written in Nazm until the rewrite cuts over, and the cost below counts both.

Diagnostic numbers below are **provisional**: H takes the unused block `N07xx` and Q the unused
block `N08xx`, so the parallel tracks (which continue `N06xx`) cannot collide; the registry in
`docs/diagnostics.md` assigns the final numbers when each rule is implemented. Refusals of a
profile rule keep the existing `N0510`, naming the rule.

---

## H — hardware and chip design

### H.1 The idea in one paragraph

A synchronous circuit **is** a function from the current state and the current inputs to the next
state and the current outputs (a Mealy machine). Nazm already has everything that sentence needs:
records are values, plain data is copied, a function without handles or authority is pure, and
`if` and `match` are expressions with every branch present. So a hardware design in Nazm is an
ordinary function marked `@hardware`, whose state record's fields are the registers, whose `let`
bindings are the wires, and whose body is the combinational logic between one clock edge and the
next. Its **software meaning** — what `nazm run` and `nazm test` compute when they call it in a loop
— is the cycle-accurate behaviour of the circuit, by construction; the backend's only job is to
emit Verilog that behaves the same, and the gate's job is to show that it does.

Two properties follow for free and are worth naming, because they are the two most common classes
of HDL bug: **no combinational loop** can be written (a step body is a DAG of `let`s; feedback
exists only through the state), and **no latch** can be inferred (every `if` and `match` that
produces a value has every branch, and the step returns a whole next state).

### H.2 Types: bit-precise integers

The numeric family already names its widths — `UInt8`, `UInt16`, `UInt32`, `UInt64`, `UInt128`,
`Int8` … `Int128`, with `Int` the 64-bit signed one and no `Int64` alias (*Numbers*). The design
**extends the same family to every width from 1 to 1024**: `UInt1`, `UInt5`, `UInt12`, `UInt24`,
`Int7`, `UInt512`. One type, one name, as today. Every rule of *Numbers* holds at the new widths
unchanged — literals at the expected type, `N0003` outside the range, traps on overflow (`N0400`),
two's complement, bit operators, `wrap_convert`/`saturate_convert`/`try_convert`, the method forms
(`a.wrapping_add(b)`, by *Methods and indexes by name*).

These are **general-language** types, not hardware-only ones: a protocol parser or a codec wants
`UInt12` as much as a circuit does, and the design's software meaning needs them in the interpreter
and in compiled test benches. Every implementation supports them: the interpreter at any width,
LLVM natively (`iN`), Nazm's own code generator by multi-word lowering above 64 bits; Cranelift,
being retired, refuses widths it lacks by name.

New built-ins, each generic over every integer type, each total (none can trap):

| Built-in | Meaning |
| --- | --- |
| `bit(x, i) -> Bool` | bit `i` of `x`; `i` a constant (`N0703` otherwise) |
| `bits[T](x, lo) -> T` | the `width(T)` bits of `x` from bit `lo`, `T` unsigned; `lo + width(T) <= width(x)`, `lo` constant (`N0703`) |
| `concat[T](hi, lo) -> T` | `hi`'s bits above `lo`'s; `width(T) == width(hi) + width(lo)` (`N0702`) |
| `zero_extend[T](x)`, `sign_extend[T](x)` | widen to `T` (never narrower: `N0702`) |
| `widening_add(a, b)`, `widening_sub(a, b)` | the exact result at one bit wider, as `widening_mul` gives twice the width (*Numbers*, L13b) |
| `with_bit(x, i, b)` | `x` with bit `i` replaced |

Two compatible widenings of existing rules, both refused before and both meaning in software what
they mean in hardware: a shift amount and an array index may be **any integer type**, not only
`Int`. That is what lets a design write `regs[rs1]` with `rs1: UInt5` and know, from the type alone,
that the index is in range of `[UInt32; 32]`.

**Width parameters (phase H5, EXPERIMENTAL at 1.0).** Reusable hardware — a FIFO of any width and
depth — needs a value-level generic parameter: `fn fifo[const W: Int, const D: Int](…)`, with
`UInt[W]` and `[T; D]` written through it. This departs from N11's *no type-level computation*, so
it is kept as small as works: a `const` parameter is an `Int`, an argument is a constant expression,
and a width expression is `+`, `-`, `*` of parameters and literals and `clog2(…)`; the checker
normalises linear expressions and treats `clog2` and products of parameters as opaque terms, so a
generic definition is still **checked once, parametrically** — two widths it cannot prove equal are
`N0702`, and the fix is an explicit `zero_extend`/`bits`. `UInt[W]` with a literal is refused
(`N0701`: the type is named `UInt12`), so a type keeps one name. Without H5, designs are
parameterised by module constants — enough for the gate.

### H.3 Surface syntax

A sequential module: state, inputs and outputs are records of plain data, and the reset value is a
module constant (aggregate constants exist since R1a).

```nazm
struct Blink { count: UInt24, led: Bool }
struct BlinkIn { enable: Bool }
struct BlinkOut { led: Bool }

const BLINK_RESET: Blink = Blink(count: 0, led: false);

@hardware(clock = "clk", reset = "rst", reset_value = "BLINK_RESET")
fn blink(s: Blink, i: BlinkIn) -> (Blink, BlinkOut) ! {} {
    let tick = s.count == 12_000_000
    let count: UInt24 = if tick || !i.enable { 0 } else { s.count.wrapping_add(1) }
    let led = if tick { !s.led } else { s.led }
    (Blink(count: count, led: led), BlinkOut(led: s.led))
}
```

A combinational module has no state, no clock and no reset — one parameter, one result:

```nazm
enum AluOp { Add, Sub, And, Or, Xor, Sltu }

struct AluIn { op: AluOp, a: UInt32, b: UInt32 }

@hardware
fn alu(i: AluIn) -> UInt32 ! {} {
    match i.op {
        AluOp.Add => i.a.wrapping_add(i.b),
        AluOp.Sub => i.a.wrapping_sub(i.b),
        AluOp.And => i.a & i.b,
        AluOp.Or => i.a | i.b,
        AluOp.Xor => i.a ^ i.b,
        AluOp.Sltu => if i.a < i.b { 1 } else { 0 },
    }
}
```

Hierarchy is a call: a child module's state is a field of its parent's state, and the parent
threads it (abridged: `CoreIn`, `fetch_unit` and the decoders are ordinary declarations).

```nazm
struct Core { pc: UInt32, regs: [UInt32; 32], fetch: FetchState }

@hardware(clock = "clk", reset = "rst", reset_value = "CORE_RESET")
fn core(s: Core, i: CoreIn) -> (Core, CoreOut) ! {} {
    let (fetch, f) = fetch_unit(s.fetch, FetchIn(pc: s.pc, mem: i.imem))   // an instance
    let r = alu(AluIn(op: decode_op(f.word), a: s.regs[rs1(f.word)], b: s.regs[rs2(f.word)]))
    let mut regs = s.regs
    if writes(f.word) && rd(f.word) != 0 { regs[rd(f.word)] = r }    // rd: UInt5, in range by type
    (Core(pc: s.pc.wrapping_add(4), regs: regs, fetch: fetch), CoreOut(addr: s.pc))
}
```

Memories are state fields marked `@memory` (an attribute before a field, as `@codec` already is):

```nazm
struct Ram {
    @memory
    cells: [UInt32; 4096],
    rdata: UInt32,
}
```

**What each construct is in hardware.**

| Nazm | Hardware |
| --- | --- |
| a leaf field of the state record | a register (flip-flops), reset to the reset value's field |
| a `let` in the step | a wire; its name kept where the emitter can (not promised) |
| a pure function called from the step | combinational logic, inlined |
| a call of another `@hardware` function | an instance, kept as a Verilog module |
| a field of the input / output record | an input / output port, named by its field path (`bus_addr`) |
| `if`, `match` | a multiplexer; an enum is encoded in the fewest bits, by the backend |
| a bounded `for`/`while` | unrolled logic (its bound is the `bounded-loops` rule's) |
| an `@memory` array | a RAM: not reset, initialised at configuration from the reset value, read by index, written by `regs[a] = v` in the next state |
| `@hardware(clock = …, reset = …)` | one clock domain, a synchronous active-high reset by default; `reset_kind = "async"` and `reset_active = "low"` select the others |

**Ports and order.** Field order is not part of what a record means (*Records*), so ports are
connected by name only; the emitter writes them in declaration order for readability and
`ports.json` (`nazm.hw-ports/1`) records each port's name, direction and width. An enum at a
top-level port is refused (`N0710`) in 1.0: its encoding is the backend's, and a port's bits must
not depend on it.

### H.4 Checker rules

**Which code is hardware.** The design of a `@hardware` function is everything it reaches. The
`hardware` rule set is checked on that reachable set — so a file can hold a design and its test
bench, which prints and allocates — and `--profile hardware` applies the same rules to the whole
program, for a package that is only a design. Each rule is a profile rule with the profile
machinery's verdicts (`pass`, `fail`, `unknown`, and `unknown` refused) and appears in
`--profile-report`; a refusal on the reachable set is `N0706`, naming the rule and the call path
from the `@hardware` root, and under `--profile hardware` it is `N0510` as every profile's is.

| Rule | Refuses | Exists today? |
| --- | --- | --- |
| `no-io`, `no-spawn`, `no-foreign` | any effect or authority | yes |
| `no-heap` | every allocation site: `Str` built at run time, `Vec`, `Box`, `Chain`, `Cell`, closures, channels, atomics | yes (`kernel`) |
| `no-recursion`, `bounded-stack` | a call cycle, a call through a function value | yes |
| `bounded-loops` | a loop without a trip bound the compiler establishes | yes (`realtime`) |
| `plain-data` | a value that is not plain data: `Str`, `Text`, `Bytes`, handles, capabilities | new |
| `no-float` | `Float32`, `Float64` (fixed point is a library over integers, `@std/hw/fixed`) | new |
| `trap-free` | an operation that can trap: `+ - *` and unary `-` on non-constant operands, `/` and `%` except by a constant power of two, a narrowing `T(x)`, a shift whose amount's type can reach the width, an index whose type can leave the array | new |

`trap-free` is what keeps the profile a pure refusal. In software a design's `a + b` would trap on
overflow and the circuit cannot; rather than give `+` a second meaning, the rule refuses it and the
design says what it means — `wrapping_add`, `saturating_add`, `widening_add`. Every accepted design
therefore has one meaning in both worlds, and there is no "hardware mode".

**Shape rules** (new codes; provisional numbers):

| Code | Constant | Refuses |
| --- | --- | --- |
| `N0700` | `WIDTH_OUT_OF_RANGE` | a width outside `1..=1024` |
| `N0701` | `WIDTH_NAMED` | `UInt[12]` with a literal: the type is named `UInt12` |
| `N0702` | `WIDTH_MISMATCH` | `concat`, extension or a width parameter whose widths are not provably equal |
| `N0703` | `BIT_POSITION` | `bit`, `bits`, `with_bit` with a position that is not constant or not inside the value |
| `N0704` | `HARDWARE_SHAPE` | `@hardware` on a function that is not `(S, I) -> (S, O)` or `(I) -> O` of plain data, or with `clock` but no state |
| `N0705` | `RESET_VALUE` | a `reset_value` that names no constant of the state's type |
| `N0706` | `HARDWARE_RULE` | a `hardware` rule failed on a design's reachable set (above) |
| `N0707` | `INSTANCE_THREADING` | a child module whose state is not threaded: on every path, the parent's next-state field must be either the child's state unchanged (the child's registers hold) or the next state of exactly one call of it, read from that same field |
| `N0708` | `CLOCK_DOMAIN` | a call of a child module of another clock: crossing domains is the system manifest's (H.5), through a synchroniser |
| `N0709` | `MEMORY_ACCESS` | an `@memory` field that is not an array, written more than once in a step, or whose writes the checker cannot count |
| `N0710` | `PORT` | two ports whose flattened names collide, or an enum at a top-level port |
| `N0711` | `CONST_PARAMETER` | a `const` parameter that is not an `Int`, or an argument that is not a constant expression (H5) |

`N0707` is the one rule with a linearity flavour — each child's state flows into exactly one
place on every path — and it is checked by the same path-sensitive "used exactly once" analysis
that Q's qubits need (Q.4); one analysis, built once, serves both tracks.

Design manifests and board pin maps reuse `N0626` (a manifest key unknown or out of range), as
board manifests do. No design can fail at run time, so H adds no `N04xx` failure.

### H.5 Clock domains and the system manifest

In the language, **one module has one clock**, and a child of another clock cannot be called
(`N0708`). A design with several clocks is composed **outside the language**, as a board is today
(*A freestanding target*: "a board is a description outside the language"): a system manifest,
`hw.toml`, names the top modules, the clock of each, the FPGA pins, and every crossing between
domains — each through a primitive of `@std/hw/cdc` (a two-flop synchroniser for one bit, a
handshake for a value, an asynchronous FIFO with Gray-coded pointers). The manifest is checked
against the modules' port types; a crossing that does not go through a primitive is refused. The
primitives are hand-written, reviewed Verilog with a Nazm model each, so the simulator has their
software meaning. This keeps the language single-clock and puts every crossing, the most common
source of silicon bugs, in one reviewed place (SpinalHDL's lesson, below).

Black boxes — a vendor PLL, an I/O buffer, a hard DSP block — are declared the same way: a
manifest entry names the Verilog module and a Nazm function that is its model, and the three-way
check (H.7) runs with the model.

### H.6 IR and backend

```text
Mono MIR (checked, monomorphised) ─ printed ─▶ nzhw (toolchain/hw/, Nazm)
    ─ flatten records to bit vectors, encode enums, inline combinational calls, unroll loops
    ─▶ RTL IR (nazm.rtl/1): modules, ports, nodes (bit-vector SSA), registers, memories, instances
    ─┬─▶ Verilog-2005 emitter      (1.0)
     ├─▶ SystemVerilog emitter     (experimental)
     └─▶ RTL interpreter           (the backend's own oracle)
```

- **Input: Mono MIR, printed.** Field access is a symbolic projection through Mono MIR (Settled,
  item 2), so records and enums are still records and enums there, every generic is instantiated,
  and moves and drops are explicit. LIR would be too low: it has byte offsets and a heap. The
  compiler gains a printer for the subset (`nazm mir --print`, internal schema), as `nzcg` reads
  printed LIR.
- **RTL IR.** A module is ports, combinational nodes over fixed-width bit vectors (and, or, xor,
  not, add/sub/mul at a width, compare, shift, mux, slice, concat, extend, constant), registers
  (clock, reset kind, reset value, next-value node), memories (depth, width, initial contents,
  read and write ports), and instances. It is typed by width only; every node has one width; a
  verifier checks it before emission. It is deliberately close to Yosys's RTLIL and FIRRTL's low
  form, so its meaning is uncontroversial.
- **Verilog-2005 (IEEE 1364-2005)** is the 1.0 output: frozen, and accepted by every open and
  commercial tool. The emitter writes a small, fixed idiom: `assign` for nodes, one
  `always @(posedge clk)` per module for registers, the canonical inferred-RAM idiom for memories
  (`$readmemh` for initial contents), no `initial` blocks otherwise, no `x` or `z` anywhere, one
  module per instantiated `@hardware` function per parameter set, names prefixed so a design's
  names cannot collide with Verilog keywords. Output is linted clean by `verilator --lint-only -Wall`.
- **SystemVerilog (IEEE 1800)** is a second emitter (`logic`, `always_ff`, `always_comb`, packed
  structs for records), experimental at 1.0: its readability is valuable, its tool support uneven.
- **Memory inference is the backend's.** An `@memory` field with registered reads becomes the
  idiom Yosys maps to block RAM; otherwise distributed RAM or registers. `nazm.hw-report/1` says
  which, and why not.
- **No X.** Every register has a reset value and every memory initial contents, so no wire is ever
  unknown after configuration; this is what lets a two-state simulator (Verilator) and a four-state
  one (Icarus) be held to the same answer.
- **ASIC.** An ASIC flow (OpenLane/OpenROAD on an open PDK) consumes the same Verilog but needs
  memories without initial contents, scan and timing constraints; it is not in 1.0 unless the
  copyright holder decides otherwise (H.11).

**Commands** (experimental, as `nazm contract` and `nazm accel` are): `nazm hw build FILE --top F
--objects DIR` (Verilog, `ports.json`, `hw-report.json`); `nazm hw sim FILE --top F --stimulus
S.jsonl --cycles N [--check]`; `nazm hw fpga --system hw.toml --objects DIR` (Yosys, nextpnr and the
family's packer, run where they are installed or contained, refused by name where they are not).

### H.7 Simulation and evidence

**Testing a design is ordinary `nazm test`.** A design is a function, so a test bench is a `@test`
function that starts from the reset value and calls the step in a loop, checking outputs — with
every facility of the language (records, `Vec`, `print`, fuzzing with `@fuzz`). `@std/hw/sim` gives
`sim_run(top, reset, inputs) -> Vec[O]` and `sim_trace`, which records a stimulus file
(`nazm.hw-stimulus/1`: one input record per cycle, JSON lines).

**Three-way check.** `nazm hw sim --check` (and `nazm hw test`, which replays every trace recorded
by the tests) runs the same stimulus through:

1. the **interpreter**, calling the step function — the design's meaning;
2. the **RTL interpreter**, on the RTL IR — the backend's lowering;
3. **Verilator** and **Icarus Verilog**, on the emitted Verilog — independent implementations of
   the output language, Icarus four-state so that an X on any output after reset is a failure;

and compares every output port on every cycle. A difference is `failed`, never a warning, and is a
compiler defect (`N0900`) until shown to be the reference simulator's. Verilator and Icarus run in
the contained runner (a pinned image, as `docker/evm.Dockerfile` pins py-evm).

**The gate (task 1.4)** — `passed` only on cited evidence:

1. *Corpus.* At least twenty designs — counter, LFSR, ALU, barrel shifter, CRC-32, debouncer, PWM,
   UART transmitter and receiver, SPI master, synchronous FIFO, iterative multiplier and divider,
   VGA timing, a cache tag array, and an **RV32I core** — each through the three-way check on fixed
   and random stimulus of at least 10 000 cycles; the RV32I core passes the `rv32ui` subset of
   `riscv-tests` under all four simulators.
2. *Differential fuzzing.* A generator of random designs over the accepted subset; every generated
   design three-way checked (contained, one workload at a time).
3. *Refusals.* A compatibility-corpus case for every rule and code of H.4; the `trap-free` rule
   shown to refuse every trapping operator.
4. *Mutation.* Mutants of the checker rules caught by the corpus, mutants of the lowering and the
   emitter caught by the three-way check (the mutation-campaign law).
5. *Synthesis.* Yosys synthesises every corpus design for iCE40 and ECP5 with `check -assert` (no
   latch, no combinational loop, no multiple drivers); nextpnr places and routes them and reports
   timing at a stated clock.
6. *FPGA.* One board runs the RV32I core and a UART design: the bitstream's digest, the build log
   and the UART transcript recorded. It is run by the copyright holder, who has the board; the
   evidence says "ran on board X", and nothing is claimed for a board that did not run it.

### H.8 Phases and estimates

Lines are Nazm unless marked; *weeks* are focused single-track weeks, and the parallel tracks
(rewrite, Windows, stage P, C/M/D) share the gates.

| Phase | Content | Size | Weeks |
| --- | --- | --- | --- |
| H1 | bit-precise widths 1–1024 in spec, both checkers, interpreter, LLVM, `nzcg` (multi-word), built-ins of H.2, shifts and indexes by any integer type | 6–9k lines + tests | 4–6 |
| H2 | `@hardware`, `@memory`, the `hardware` rules and H.4's codes, in both checkers; Mono MIR printer | 3–4k | 2–3 |
| H3 | `nzhw`: lowering to RTL IR, verifier, RTL interpreter, Verilog-2005 emitter, `nazm hw build/sim` | 8–12k | 6–8 |
| H4 | three-way harness: contained Verilator and Icarus image, stimulus and trace formats, VCD, design fuzzer | 3–4k | 2–3 |
| H5 | `const` parameters and width arithmetic; `@std/hw` (FIFO, stream handshakes, fixed point, `sim`); system manifest, CDC primitives, black boxes | 6–8k | 5–7 |
| H6 | FPGA flow (Yosys, nextpnr, packers; pin maps from the system manifest); RV32I core; board run | 4–6k | 3–5 + board time |
| H7 | SystemVerilog emitter | 1–2k | 1–2 |

**Total: about 31–45k lines and 23–34 weeks** of one track's work — the largest of the five tracks
after the rewrite itself. H1 and H2 are language changes and carry the double-checker cost; H3–H7
are new Nazm code only.

### H.9 What to take from others, and what to avoid

| Language | Take | Avoid |
| --- | --- | --- |
| **Clash** (Haskell) | the closest model: a design is a function, `mealy` state machines, the host language's evaluation *is* the simulation | type-level naturals with plugin solvers for widths; laziness; a GHC dependency |
| **Chisel** (Scala) | generators: ordinary code computing structure at elaboration (Nazm: constants, `const` parameters, `comptime`); FIRRTL's idea of a narrow RTL IR | an embedded DSL with two meanings for one expression (a Scala value vs a hardware node); last-connect `:=`; `+` silently truncating; an implicit clock and reset nobody wrote |
| **SpinalHDL** (Scala) | explicit clock domains, unsynchronised crossings refused; `Stream`/`Flow` handshake libraries | the same embedding cost as Chisel |
| **Amaranth** (Python) | `comb` vs `sync` as distinct, checked; reset-less signals for memories; first-class Yosys/nextpnr platform files | a Python program whose run-time meaning and hardware meaning differ |
| **Bluespec** | guarded atomic actions show composition through handshakes can be checked | an implicit rule scheduler: the cycle a thing happens in should be what the source says |

And from HLS (C to gates): nothing in 1.0. Nazm does not schedule a software loop into a pipeline;
a step is a cycle, and the cycle is what the source says.

### H.10 Twenty-year stability

| Promised at 1.0 (STABLE) | Left experimental |
| --- | --- |
| the bit-precise integer types and their arithmetic (general language) | `const` parameters and width arithmetic (H5) |
| a design's meaning is its step function's: nothing new to keep stable | the `hardware` rule set — as every profile's rule set is experimental and may grow |
| the attributes `@hardware`, `@memory` and their keys (the attribute vocabulary: may add, never change) | the Verilog text: structure and names, as LLVM IR's text is not promised |
| emitted Verilog-2005 is accepted by Yosys, Icarus and Verilator at the versions the release records, and simulates as the design means | SystemVerilog; `nazm hw` commands and their schemas (each `/1`, under the schema law) |
| | system manifests, CDC primitives, FPGA families, boards |

Verilog-2005 is a frozen IEEE standard that every tool of the last twenty years reads, which is why
it, not SystemVerilog or a tool's IR (FIRRTL, CIRCT), is the promised output.

### H.11 Open questions for the copyright holder (direction-level)

1. **"Chip design" at 1.0: FPGA only, or an ASIC flow too?** Recommendation: FPGA in the 1.0 gate;
   an ASIC flow (OpenLane on the SkyWater 130 nm PDK, possibly a Tiny Tapeout submission) as an
   experimental track after it.
2. **The board, and buying it.** Recommendation: an iCEBreaker (Lattice iCE40UP5K, fully open
   toolchain); an ECP5 board (ULX3S or OrangeCrab) as the second if the RV32I core does not fit.
   The board run is yours to do.
3. **Value-level generic parameters** (`[const W: Int]`, with a small width arithmetic) enter the
   general language, departing from N11's "no type-level computation". Recommendation: yes,
   experimental at 1.0; without them, designs are parameterised by constants only.

---

## Q — quantum computing

### Q.1 The idea in one paragraph

A quantum program is classical code around **circuits**. A circuit is a Nazm function marked
`@circuit` that allocates qubits, applies gates, measures, and returns classical values; ordinary
code never calls one, but builds a `Circuit[R]` value naming it with its classical arguments and
hands that to a simulator (pure, seeded) or, holding a `QuantumCap`, to a provider. Qubits are
**linear**: no operation copies one, two names never reach the same qubit at once, and a qubit
cannot simply be dropped — so the no-cloning theorem and the "you forgot to measure or reset it"
bug are both type errors. The backend emits **OpenQASM 3**; a state-vector simulator written in
Nazm is the reference meaning, held to Qiskit's on the same text.

### Q.2 Does Nazm's ownership already give move-only values?

**No, and the constitution says so.** *The memory constitution* has four kinds of value, and in
every one assignment shares or copies: a handle's `let b = a` is another reference to the same
storage. Capabilities are explicitly "copied freely … there is no linear or affine use", and
*Open — Capabilities* lists linearity as needing "move semantics the language does not have". What
Nazm does have is half of it: **parameters are borrowed** (rule 2: a callee may use a handle and may
not keep it), **returning transfers** (rule 3), and MIR already writes out every move and drop
(§7.42). The constitution also foresaw this addition: "a fifth would need a value that is none of:
a number, an immutable byte string, a mutable container, or a synchronisation primitive". A qubit
is none of those.

So Q **adds a fifth kind, linear values**, as an amendment of *The memory constitution*, in 1.0
restricted to compiler-owned types — `Qubit`, and the records, tuples and arrays that hold one.
User-declared linear types (an affine capability, a must-close handle) would be a later
extension built on the same analysis.

### Q.3 Surface syntax

```nazm
use "@std/quantum"

struct Pair { a: Bool, b: Bool }

@circuit
fn bell() -> Pair ! {} {
    let a = qubit_new()
    let b = qubit_new()
    h(a)
    cx(a, b)
    Pair(a: measure_release(a), b: measure_release(b))
}

fn main(out: OutCap) -> Int ! {io} {
    let shots = quantum_sample(circuit(bell), 1000, 7)    // Vec[Pair], seeded, deterministic
    let mut same = 0
    for p in shots { if p.a == p.b { same += 1 } }
    print(f"{same} of 1000 agree\n")                       // 1000: a Bell pair always agrees
    0
}
```

Gates **borrow** their qubits and change them in place — the existing parameter rule, so no
threading of new names through every line (`h(a)`, not `let a2 = h(a)`). A function that takes a
`Qubit` parameter borrows it; one that returns a `Qubit` transfers it. Measurement comes in two
forms: `measure(q) -> Bool` borrows (the qubit stays, collapsed, as in OpenQASM and Q#), and
`measure_release(q) -> Bool` consumes it. `release(q)` resets and consumes.

User-defined unitaries are `@unitary` functions; `adjoint` and `controlled` are compiler forms over
a named one, and uncomputation is `within … apply`:

```nazm
@unitary
fn qft(r: [Qubit; 4]) ! {} {
    for i in 0..4 {
        h(r[i])
        for j in i + 1..4 { cp(PI / Float64(1 << (j - i)), r[j], r[i]) }
    }
    swap(r[0], r[3])
    swap(r[1], r[2])
}

@circuit
fn phase_estimate(theta: Float64) -> UInt4 ! {} {
    let r: [Qubit; 4] = qubits_new()
    let target = qubit_new()
    x(target)
    for k in 0..4 {
        h(r[k])
        for n in 0..(1 << k) { controlled(phase_gate, r[k], theta, target) }
    }
    adjoint(qft, r)
    let bits = measure_release_all(r)          // [Bool; 4]; @std/quantum, as qubits_new and PI are
    release(target)
    bits_to_uint(bits)
}

@unitary
fn phase_gate(theta: Float64, q: Qubit) ! {} { p(theta, q) }

@circuit
fn mark_if_both(r: [Qubit; 3]) ! {} {           // borrows r from the circuit that calls it
    let anc = qubit_new()
    within {
        ccx(r[0], r[1], anc)                    // computed
    } apply {
        cz(anc, r[2])                           // used
    }                                           // uncomputed: adjoint of the `within` block
    release(anc)                                // back in |0>, so its reset can be elided
}
```

`within` and `apply` are words only at a statement's start before `{`, inside a circuit — the
precedent `defer` set — so no program that names a binding `within` changes. Classical control is
ordinary Nazm: `if measure(q) { x(r) }` is feed-forward; a `while` on a measurement is
repeat-until-success.

**The standard gate set** is OpenQASM 3's `stdgates.inc`, by name and by matrix, **including
global phase** (which becomes a relative phase under `controlled`, so it is not a detail): `x y z h
s sdg t tdg sx`, `rx ry rz p u` with `Float64` angles first as in OpenQASM, `cx cy cz ch cp crx cry
crz cu swap ccx cswap`, and `gphase`. They are `@std/quantum`'s built-ins.

**Classical types in a circuit** are plain data: `Bool`, the integer types (H's bit-precise ones
map to OpenQASM's `uint[n]` and `bit[n]`), `Float64` for angles, and records, tuples and arrays of
them. A root circuit's parameters and result are classical plain data; its parameters become OpenQASM
`input`s, so a provider can sweep them without recompiling.

**The classical boundary.** `circuit(f, args…) -> Circuit[R]` names a `@circuit` function and its
arguments, checked as a call; it runs nothing. Then:

| Built-in | Authority | Meaning |
| --- | --- | --- |
| `quantum_sample(c, shots, seed: UInt64) -> Vec[R]` | none, pure | the reference simulator's samples; deterministic in the seed |
| `quantum_probabilities(c) -> Vec[(R, Float64)]` | none, pure | the exact distribution of results, by branching simulation |
| `quantum_qasm(c) -> Str` | none, pure | the OpenQASM 3 text |
| `quantum_run(qc: QuantumCap, device: Str, c, shots) -> Result[Vec[R], QuantumError]` | `QuantumCap`, effect `io` | the circuit run by a provider |

### Q.4 Checker rules

**The linear kind.** A type is *linear* when it is `Qubit` or a record, tuple or array holding a
linear type. The rules, all on MIR's places, path-sensitively:

1. **No copy.** `let b = a` and passing to a binding **move**: the source is dead afterwards, and a
   use of it is `N0800`. A parameter is borrowed and cannot be moved or consumed by its callee
   (`N0800`, naming the parameter).
2. **No alias.** One call or gate given the same qubit twice — `cx(q, q)`, `cx(r[0], r[0])`, a
   record and one of its fields — is `N0801`. Register indices must be constant once loops over
   them are unrolled (`N0807`), so distinctness is decided at compile time, never at run time.
3. **No leak.** On every path, an owned linear binding is consumed — `measure_release`, `release`,
   returned, or moved into a binding that is — before it dies (`N0802`, naming the path: early
   `return`, `break`, `?` and a failure edge included). This is *linear*, not *affine*, on
   purpose: an implicit release would silently collapse every qubit entangled with it.
4. **Agreement at joins.** Both branches of an `if` or every arm of a `match` leave the same
   linear bindings owned (`N0803`); a loop body leaves them as it found them.
5. **Containment.** A linear value cannot enter a generic type (`Vec`, `Option`, `Box`, `Cell`, a
   user's generic), a closure's capture, a `static`, a `const`, a channel or a task, and has no
   `==` and no text (`N0804`). The kind stays an island.
6. **The circuit boundary.** A `Qubit`, a gate, `measure` or `within` outside a `@circuit` or
   `@unitary` function, or a direct call of a `@circuit` function from classical code, is
   `N0805`. A circuit may borrow qubits from the circuit that calls it, but a **root** — the `f`
   of `circuit(f, …)` — takes and returns classical plain data only; another root, or an `f` that is
   not a named circuit, is `N0808`.
7. **Unitarity.** A `@unitary` body, a `within` block, and the function `adjoint` or `controlled`
   names must be reversible: gates and other unitaries on borrowed qubits, classical arithmetic on
   parameters, no measurement, allocation, release, or branch on a quantum result (`N0806`).
8. **Feature level.** A circuit using a feature the selected OpenQASM level does not carry (Q.5)
   is refused at build (`N0809`).

Inside a circuit the rule set `quantum` also holds — `no-io`, `no-spawn`, `no-foreign`, `no-heap`,
`no-recursion`, `bounded-stack`, `plain-data` for classical values — checked on the reachable set
and reported as H's are (`N0706`'s twin, `N0810` `QUANTUM_RULE`). A simulator asked for more qubits
than its limit fails at run time with a resource failure (`N0820` `QUBIT_LIMIT`, provisional; the
registry may place it among `N04xx`).

Rules 1–4 are the "used exactly once on every path" analysis H's `N0707` needs; MIR's explicit
moves and drops (§7.42) are where it runs, and the MIR validator re-checks the invariant, so a
lowering that broke linearity is `N0900`, not a wrong circuit.

**`QuantumCap`.** An eleventh capability kind (*Capabilities* says "exactly ten"; this amends it):
authorises `quantum_run`, effect `io`, implies nothing and nothing implies it, may be taken by
`main`. Simulation needs no authority: it is computation. It must land before 1.0, because a new
built-in type name refuses a program's own type of that name (`N0331`).

### Q.5 IR and backend

```text
Mono MIR of each @circuit/@unitary ─ printed ─▶ nzq (toolchain/quantum/, Nazm)
    ─ unroll register loops, resolve indices, allocate physical qubit indices (reuse after release)
    ─▶ circuit IR (nazm.qir/1): gates on qubit indices, measurements to bits,
       classical ops, structured if/while, gate definitions with ctrl@/inv@
    ─┬─▶ OpenQASM 3 emitter
     └─▶ state-vector simulator (@std/quantum/sim, Nazm)
```

- **OpenQASM 3** is the only output. `@unitary` functions become `gate` definitions; `adjoint` and
  `controlled` become OpenQASM's `inv @` and `ctrl @` modifiers; a circuit with classical control
  uses `if`, `while`, `for` and `bit` registers; parameters are `input float[64]`. Two levels:
  `full` (modifiers, gate definitions, mid-circuit measurement and feed-forward) and `flat`
  (stdgates only, every definition and modifier expanded, measurement only at the end), because
  providers support different subsets. A construct a level cannot express is `N0809`, never a
  silent rewrite.
- **The simulator** is the reference meaning: a state vector of `2^n` complex `Float64` amplitudes
  (24 qubits by default, 256 MiB; a limit of the implementation, raisable), gates applied as their
  exact matrices, measurement by the Born rule with collapse and renormalisation, a seeded
  generator for sampling. Its rotations use **Nazm's own correctly-rounded sine and cosine**, not
  the host C library's (whose last place differs between hosts, *Numbers*), so the same seed gives
  the same samples on every host and under every backend.
- **Providers.** `quantum_run` hands the OpenQASM text, the inputs and the shot count to an
  **adapter executable** (`nazm-quantum-PROVIDER`, found as `nzcg` is) over a versioned job schema
  (`nazm.quantum-job/1`), and reads back the results. Credentials are the adapter's, from its own
  configuration, never in the program. Provider APIs change every few years; the language does not,
  so the adapter is the part that is replaced, and it lives outside the stable core.

### Q.6 Evidence

**The gate (task 1.5):**

1. *Known results.* At least thirty circuits with analytic answers: the four Bell states, GHZ on 3
   to 10 qubits, teleportation (mid-circuit measurement and feed-forward), superdense coding,
   Deutsch–Jozsa, Bernstein–Vazirani, Grover on 2, 3 and 4 qubits, QFT followed by its adjoint (the
   identity), phase estimation of `T` (exactly `001`), a reversible adder on basis states, the
   Toffoli truth table, `within/apply` leaving every ancilla in `|0>`, `adjoint(f)` after `f` the
   identity on random states, `controlled(f)` against its matrix. `quantum_probabilities` equals
   the analytic distribution within `1e-12`; sampled results pass a stated chi-square test at a
   fixed seed.
2. *Differential.* Every emitted program is accepted by the OpenQASM project's reference parser
   (`openqasm3`) and imported by Qiskit's `qasm3` loader; Qiskit Aer's state vector of the import
   equals Nazm's simulator's up to global phase (fidelity ≥ `1 − 1e-10`). Contained, in a pinned
   Python image.
3. *Determinism.* The same seed gives byte-identical samples on every supported host, interpreted
   and compiled.
4. *Refusals.* A compatibility-corpus case for every rule and code of Q.4, including each no-clone,
   alias, leak and join case on every exit edge.
5. *Mutation.* Mutants of the linear analysis caught by the corpus; mutants of the lowering, the
   emitter and the simulator caught by the known results and the differential check.
6. *Provider.* One real provider accepts the OpenQASM of the Bell and GHZ circuits and returns
   results, recorded with the job identifiers. Run by the copyright holder with his account. The
   claim is "accepted and executed"; correctness is claimed on the simulator only, since hardware
   noise is the provider's.

### Q.7 Phases and estimates

| Phase | Content | Size | Weeks |
| --- | --- | --- | --- |
| Q1 | the linear kind: spec amendment, rules 1–5 in both checkers on MIR places, the MIR validator invariant (shared with H's `N0707`) | 4–6k lines + tests | 3–5 |
| Q2 | `Qubit`, `@circuit`, `@unitary`, gates, measurement, `within/apply`, `adjoint`, `controlled`, `circuit(…)`, `Circuit[R]`, rules 6–8, the `quantum` rule set | 3–5k | 3–4 |
| Q3 | `nzq`: unrolling, qubit allocation, circuit IR, OpenQASM 3 emitter at two levels | 4–5k | 3–4 |
| Q4 | the simulator in `@std/quantum`: state vector, exact distributions, seeded sampling, correctly-rounded sine and cosine | 3–4k | 2–3 |
| Q5 | the harness: contained `openqasm3` and Qiskit image, the known-results corpus, statistics | 2–3k | 2 |
| Q6 | `QuantumCap`, `quantum_run`, `nazm.quantum-job/1`, one provider adapter; the provider run | 2–3k | 2–3 + account |

**Total: about 18–26k lines and 15–21 weeks** of one track's work. Q1 is the semantic heart and the
largest risk; the rest is well-trodden.

### Q.8 What to take from others, and what to avoid

| Language | Take | Avoid |
| --- | --- | --- |
| **Q#** | `within … apply`; compiler-derived `Adjoint` and `Controlled`; operations (circuits) apart from functions; qubits allocated and released in scope | aliasing caught only at run time; no-cloning not a type rule; implicit release |
| **Qiskit** | interoperability: OpenQASM 3 import, Aer as an independent oracle | circuits as mutable objects assembled at run time with no static checking |
| **Silq** | linear qubits checked by the type system; knowing statically what is reversible (`qfree`) | automatic uncomputation: powerful, but its cost is invisible and its typing research-grade — Nazm uncomputes where the source says `within` |
| **Quipper** | circuits as values; reversible functions lifted to circuits; boxed subcircuits | a Haskell embedding in which a qubit can be copied at the host level |
| **OpenQASM 3** | the target: its gate library, `ctrl@`/`inv@`, `input`, classical control | as a source language: untyped, no linearity beyond duplicate gate operands |

### Q.9 Twenty-year stability

| Promised at 1.0 (STABLE) | Left experimental |
| --- | --- |
| the linear kind and its rules — a memory-constitution amendment is stable or absent | `quantum_run`'s provider contract, `nazm.quantum-job/1`, adapters |
| `Qubit`, the gates by name and matrix (OpenQASM 3's, physics' own), measurement by the Born rule | the OpenQASM text's structure, and the `full`/`flat` levels |
| `@circuit`, `@unitary`, `within/apply`, `adjoint`, `controlled`, `circuit`, `Circuit[R]` | exact samples for a seed across toolchain releases (the distribution is stable, the generator's stream is not) |
| `quantum_sample`, `quantum_probabilities`, `quantum_qasm`; `QuantumCap` as a kind | the simulator's qubit limit (the implementation's, raisable); noise models (not in 1.0) |

Physics will not change in twenty years and OpenQASM 3 is an open standard with a governing body;
provider APIs will change. So the stable surface is what touches physics and the standard, and the
provider adapter is a replaceable executable behind a versioned schema.

### Q.10 Open questions for the copyright holder (direction-level)

1. **A fifth kind of value.** Amend *The memory constitution* with linear values, restricted in 1.0
   to `Qubit` and aggregates of it. Recommendation: yes; user-declared linear types later.
2. **An eleventh capability**, `QuantumCap`. Recommendation: yes, before 1.0.
3. **The provider, and the account.** IBM Quantum's free plan accepts OpenQASM 3 through Qiskit
   Runtime and costs nothing within its monthly minutes; Amazon Braket and Azure Quantum are paid.
   Recommendation: IBM Quantum for the gate. The provider run uses your credentials and is yours to
   do.
4. **What 1.0 claims about hardware.** Recommendation: "accepted and executed by a provider";
   correctness claimed on the simulator only, no noise-aware claim.

---

## Shared decisions between H and Q

- Both backends are Nazm programs under `toolchain/` (`nzhw`, `nzq`) reading printed Mono MIR —
  no new Rust, no MLIR or CIRCT dependency (`docs/architecture.md`, *Not MLIR, in v1*).
- Both are restrictions checked on a reachable set, with the profile machinery's verdicts and
  report, so a design or a circuit sits beside its ordinary-Nazm test in one file.
- One path-sensitive "exactly once" analysis on MIR serves H's instance threading and Q's linearity.
- Bit-precise integers (H1) are shared: they are Q's classical `uint[n]` and `bit[n]`.
- Order: H1 and Q1 are language changes that touch both checkers and should land before the rewrite
  cuts over, or after it in the Nazm compiler alone — whichever comes first; the backends (H3–H7,
  Q3–Q6) are independent of the cut-over.
