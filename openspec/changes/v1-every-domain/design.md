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

## The copyright holder's answers on H and Q (2026-10-15)

1. **Chip design at 1.0 is FPGA only**; an ASIC flow (OpenLane on SkyWater 130 nm, possibly a Tiny
   Tapeout submission) is an experimental track after 1.0.
2. **All three language changes are approved:** value parameters `[const W: Int]` (experimental at
   1.0), a fifth, **linear** kind of value (limited to `Qubit` at 1.0), and an eleventh capability,
   **`QuantumCap`** (its name reserved before 1.0).
3. **Boards:** an iCEBreaker (iCE40UP5K) for H, and microcontroller boards for M (one Cortex-M, one
   RISC-V) — to be bought when the board runs come; designs and simulation do not wait for them.
4. **Quantum provider:** IBM Quantum's free plan, on the copyright holder's account; 1.0 claims
   correctness on the simulator and that real hardware accepts and executes the programs.

---

## Diagnostic ranges for C, M and D

Provisional, as H's and Q's are, and placed after Q's last code (`N0820`) so that the five tracks
cannot collide: **C takes `N0830`–`N0849`, M `N0850`–`N0869`, D `N0870`–`N0889`**; `N0890`–`N0899`
stay free. The registry in `docs/diagnostics.md` assigns the final numbers when each rule is
implemented. A manifest key unknown or out of range keeps `N0626`, a handler of the wrong shape
`N0627`, a construct refused for its target `N0392`, and a profile refusal `N0510`, as today.

---

## C — cloud computing

### C.1 The idea in one paragraph

A cloud service in Nazm is an ordinary hosted program: `main` holds a `NetCap` and a `SpawnCap`,
listens, and serves each connection in a task on the M:N pool that is already the default
(§7.107). Nothing about the language changes. What is missing is a set of **protocol libraries**
(HTTP/1.1, HTTP/2, TLS), **operational libraries** (metrics, configuration from the environment,
termination signals), and **packaging** (a static Linux executable in an image with nothing else in
it, and a WebAssembly component a WASI host runs). The constraints come from what Nazm already
promises: a socket wait never holds a worker (*Networking*, *Waiting*), so TLS must be driven over
buffers, never handed the socket; there is no hand-off of a blocking C call (§7.84), so no library
may block in C on the pool; and a profile only refuses, so a TLS service must pass `cyber` without
a `ForeignCap` — TLS is a **runtime part**, as sockets are, not user FFI.

### C.2 What exists, and what is missing

| Need | Today | C adds |
| --- | --- | --- |
| Sockets, deadlines, reactor | `@std/net` TCP/UDP over kqueue/epoll/`WSAPoll`, `os_set_timeout`, parking waits | `SO_REUSEPORT` listeners (Linux), a connection cap |
| Tasks on cores | the pool: pinned tasks, round-robin placement, no stealing (§7.84) | least-loaded placement at `spawn` (still pinned), measured |
| JSON, TOML | `@std/json`, `@std/toml`, `@derive(json, toml)` with schemas (R1c) | nothing new; configuration decoding reuses the derived schema |
| Structured logging | `@std/log`: one JSON object a line, spans | a request-id field, OpenTelemetry-compatible field names |
| Metrics | none | `@std/metrics` |
| Configuration | `os_env_*` | `@std/config`: the environment into a `@derive(json)` record |
| Signals | none ("no program watches for signals in 1.0") | `@std/signal`: termination requests as a channel |
| TLS | none (`cyber`'s domain row: "TLS, cryptography" a gap) | `@std/tls` over a runtime TLS part |
| HTTP | none (MISSING in the capability matrix) | `@std/http`: HTTP/1.1 and HTTP/2, server and client, a router |
| Static executable | glibc targets only | `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-musl` |
| WASI service | `wasm32-wasip1`: no sockets, no tasks | `wasm32-wasip2`: a `wasi:http` handler as a component |

### C.3 TLS: through a vetted library, never a hand-written cipher

**The constraint that decides it.** The pool parks a task on a socket and wakes it from the
reactor; a TLS engine that owns the socket, or blocks in its own read, would hold a worker and every
task pinned to it. So the engine must be **sans-I/O**: the runtime reads ciphertext through the
existing parking `os_read`, hands it to the engine as a buffer, and writes what the engine produces
through `os_write`. Engines that work over buffers: OpenSSL (memory BIOs), SChannel (SSPI is
buffer in, buffer out), mbedTLS (I/O callbacks). Engines that do not: Apple's Network.framework
owns the connection and calls back on a dispatch queue — a second socket model on one platform;
Apple's SecureTransport works over callbacks but is deprecated and stops at TLS 1.2.

| Option | Licence | Fit | Verdict |
| --- | --- | --- | --- |
| **OpenSSL 3.x** (LTS 3.5, supported to 2030) | Apache-2.0 | sans-I/O by memory BIO; TLS 1.3, ALPN, every platform; the Linux distributions patch it | **chosen for Linux and macOS** |
| **SChannel** (Windows, through SSPI) | part of Windows; reached through MinGW-w64's `libsecur32` and `libcrypt32` import libraries, no Microsoft SDK | sans-I/O; the system's trust store and patching | **chosen for Windows** |
| Security.framework `SecTrust` (macOS) | part of macOS | a verifier, not an engine | **used for certificate trust on macOS**, so trust decisions are Apple's |
| Network.framework (macOS) | part of macOS | owns the socket: a second I/O path beside the reactor | refused as the engine |
| SecureTransport (macOS) | part of macOS | deprecated; TLS 1.2 at most | refused |
| mbedTLS 3 | Apache-2.0 OR GPL-2.0-or-later | small, sans-I/O, TLS 1.3; fits microcontrollers too | the named alternative; M's candidate after 1.0 |
| aws-lc / s2n-tls | Apache-2.0 (aws-lc also ISC) | OpenSSL-compatible, FIPS; s2n needs a libcrypto anyway | not chosen: a second OpenSSL-shaped dependency |
| LibreSSL (`libtls`) | ISC + OpenSSL/SSLeay | a clean API; OpenBSD's cadence; rarely packaged on Linux | not chosen |
| BoringSSL | ISC + OpenSSL | no stable API, by its owner's statement | refused: a twenty-year promise cannot stand on it |
| rustls-ffi | Apache-2.0 / MIT / ISC | memory-safe; a 0.x C API; built with Rust | not chosen for 1.0; watched |
| wolfSSL | GPL-2.0-or-later, or commercial | linked into every service, it would make the service GPL | refused |
| BearSSL | MIT | TLS 1.2 only; last release 2018 | refused |

**The binding.** A runtime part `tls`, declared in the runtime's `INVENTORY` and `IMPORTS` as every
part is, with three bindings behind one set of entries — `nz_tls_client_new`, `nz_tls_server_new`,
`nz_tls_feed` (ciphertext in), `nz_tls_drain` (ciphertext out), `nz_tls_read`, `nz_tls_write`,
`nz_tls_close`, `nz_tls_alpn`, `nz_tls_peer`. OpenSSL is linked dynamically (`libssl.so.3`) for an
ordinary Linux build, so the distribution's security updates reach the service, and statically
into a musl executable (C.8). On macOS it is linked statically from a pinned build carried in the
toolchain's archive, its notice in `NOTICE`. `--sbom` and `--provenance` name the engine and its
version, since it is now part of the artefact. Because these are runtime entries, a program that
uses TLS needs no `ForeignCap`, passes `no-foreign`, and a TLS service runs under `cyber`.

**Defaults that are not options.** TLS 1.2 at least, 1.3 preferred; the engine's default cipher
suites; certificate and host-name verification always on for a client, against the platform's
trust store (Security.framework on macOS, SChannel's on Windows, the system bundle on Linux) or a CA
the program names (`tls_client_trusting(pem)`); ALPN offers `h2` and `http/1.1`. There is **no**
function that turns verification off. Revocation (OCSP stapling) and client certificates are an
extension, not 1.0.

### C.4 Surface

```nazm
use std.http
use std.tls
use std.signal
use std.metrics
use std.config

@derive(json)
struct Settings { addr: Str, cert_file: Str, key_file: Str, drain_ms: Int }

fn hello(req: Request) -> Response ! {} {
    let name = req.param("name").unwrap_or("world")
    http_text(200, f"hello, {name}\n")
}

fn counted(c: Counter, next: Handler) -> Handler ! {} {
    |req| { c.add(1); next(req) }                      // middleware is a function: Handler -> Handler
}

fn main(io: IoCap, net: NetCap, tasks: SpawnCap) -> Int ! {io, spawn} {
    let cfg = match config_from_env[Settings](io, "HELLO_") {   // HELLO_ADDR, HELLO_CERT_FILE, …
        Ok(c) => c
        Err(e) => { eprint(f"configuration: {e}\n"); return 2 }
    }
    let requests = metrics_counter("http_requests_total")
    let routes = router_new()
        .get("/hello/{name}", hello)
        .get("/metrics", metrics_handler())
    let tls = match tls_server_from_pem(io, cfg.cert_file, cfg.key_file) {
        Ok(t) => t
        Err(e) => { eprint(f"tls: {e}\n"); return 2 }
    }
    let stop = signal_on_terminate(io)                  // SIGTERM, SIGINT; Ctrl-C on Windows
    let opts = ServeOptions(tls: Some(tls), stop: stop, drain_ms: cfg.drain_ms)
    match http_serve(net, tasks, cfg.addr, counted(requests, routes.handler()), opts) {
        Ok(s) => { print(f"served {s.requests}, drained {s.drained}\n"); 0 }
        Err(e) => { eprint(f"{e}\n"); 1 }
    }
}
```

A client negotiates HTTP/2 by ALPN and always verifies the server:

```nazm
fn status(net: NetCap) -> Result[Str, HttpError] ! {io} {
    let c = http_client_new(net, ClientOptions(timeout_ms: 5000))
    let r = c.get("https://example.org/status")?
    if r.status != 200 { return Err(HttpError.Status(r.status)) }
    r.text()
}
```

(`ServeOptions` and `ClientOptions` are records whose other fields take their defaults through a
constructor function; the exact spelling of defaults follows whatever R1.5 settles for records.)

### C.5 What is in `@std`, and what is a package

The rule: **`@std` holds what an IETF standard or the operating system fixes for twenty years;
anything a taste could change is a package.**

| Module | Content | Why `@std` |
| --- | --- | --- |
| `@std/http` | `Method`, `Status`, `Headers`, `Request`, `Response`, `Body` (streamed, size-limited); HTTP/1.1 (RFC 9112) and HTTP/2 (RFC 9113, HPACK RFC 7541) server and client; `http_serve` and `ServeOptions` (limits, timeouts, connection cap, drain); a minimal router (`{param}` and prefix segments, dispatch by method) and `Handler = fn(Request) -> Response ! {io}` | the wire protocols are standards; one `Handler` type lets every package compose |
| `@std/tls` | `TlsServer`, `TlsClient`, PEM loading, ALPN, a summary of the peer's certificate | the boundary to the runtime part |
| `@std/signal` | `signal_on_terminate(io) -> Chan[Signal]`; `Signal.Terminate`, `Interrupt`, `Hangup` (POSIX only) | process lifecycle is the operating system's |
| `@std/metrics` | `Counter`, `Gauge`, `Histogram` (fixed buckets), a registry, the Prometheus text exposition (OpenMetrics' text format) | the format every collector reads |
| `@std/config` | `config_from_env[T](io, prefix)`: each field of a `@derive(json)` record from `PREFIX_FIELD`, typed by the derived schema; a missing or ill-typed variable an error naming it | the environment is the contract containers use |
| `@std/log` (exists) | a `request_id` field, OpenTelemetry-compatible field names | extended, not replaced |

**Packages (first-party, in the registry, not under the 1.0 promise):** a framework with typed
extractors, cookies and sessions, CORS and compression middleware, OpenTelemetry export (OTLP),
database drivers, WebSockets, gRPC (once HTTP/2 is stable). **Not in 1.0 at all:** HTTP/3 and
QUIC, a proxy client.

### C.6 The service model on the multicore pool

- **One task per connection**, spawned in the accept loop's scope. HTTP/1.1 keep-alive serves a
  connection's requests in order; an HTTP/2 connection is a task that reads frames and **one task
  per stream**, at most `SETTINGS_MAX_CONCURRENT_STREAMS` (default 100), with flow control as the
  RFC states. Every wait is a socket or channel wait, so it parks.
- **Placement.** Today `spawn` assigns a worker round robin and pins the task for life; pinning is
  load-bearing (thread-locals, §7.84) and stays. C changes only **where** a task is pinned: the
  worker with the fewest runnable tasks, ties round robin — a runtime policy that changes no
  meaning, measured against round robin in the gate. Work stealing is not proposed: it would unpin.
- **Accepting on many cores.** On Linux, `ServeOptions(listeners: N)` opens `N` listeners on one
  address with `SO_REUSEPORT`, one accept task each, so the kernel spreads connections; elsewhere,
  one accept task. A connection cap (`max_connections`, default 10 000) stops accepting when
  reached, so overload queues in the kernel's backlog instead of exhausting memory.
- **Timeouts** are `os_set_timeout` on each socket — header read, idle keep-alive, body — so a
  slow client cannot hold a task forever.
- **Graceful shutdown** is structured concurrency, not a new mechanism: on a message from `stop`,
  the accept tasks close their listeners (a waiting accept wakes `Closed`), each connection task
  finishes its current request, sends `Connection: close` or HTTP/2 `GOAWAY`, and ends; the scope
  joins them. After `drain_ms` the server closes the remaining sockets, whose waits wake `Closed`,
  and the scope still joins. `http_serve` answers how many requests it served and drained.
- **Signals.** The runtime installs its handlers once — a self-pipe read by the reactor on POSIX,
  `SetConsoleCtrlHandler` on Windows — and sends each request to the channels that asked. A program
  that asked for none keeps today's behaviour, with one fix containers need: as process 1, a program
  with no handler would ignore `SIGTERM`, so the runtime ends such a program on `SIGTERM` with
  status 143, as a shell would.

### C.7 Checker rules and diagnostics

C is libraries; its language surface is small. Provisional codes:

| Code | Constant | Refuses |
| --- | --- | --- |
| `N0830` | `TLS_TARGET` | a program reaching `@std/tls` built for a target with no TLS binding (the boards, `wasm32-unknown-unknown`, `wasm32-wasip1`, `wasm32-wasip2` — where the host terminates TLS, C.9) |
| `N0831` | `SIGNAL_TARGET` | `Signal.Hangup` on Windows, which has none |
| `N0832` | `HTTP_HANDLER` | `--http-handler F` naming no function of type `Handler`, or a `wasm32-wasip2` handler program that reaches tasks, sockets or a listener |
| `N0833` | `CONFIG_TYPE` | `config_from_env[T]` with a `T` that does not derive `json`, or a field type an environment variable cannot carry (a nested record, a sequence) |

No new capability: listening and connecting are `NetCap`'s, certificates are files (`IoCap`), and
signals are the process's (`IoCap`). No new profile: the gate's service runs under `cyber`. The
semantic epoch moves for the new built-ins (`os_signal_*`, `os_tls_*`) and the runtime ABI for the
`tls` and `signal` parts; the standard library API gains five modules.

### C.8 Container images

**A static executable.** `nazm build --target aarch64-unknown-linux-musl --static` (and
`x86_64`): Linux's platform layer unchanged — musl gives the same system calls through the same C
names, and `__errno_location` as glibc does — linked against musl (MIT) and a static OpenSSL built
against it, in a pinned builder image. glibc is not used for static builds: its resolver loads NSS
modules at run time. `os_resolve` uses musl's resolver, which reads `/etc/resolv.conf` and
`/etc/hosts` only, and says so.

**The minimal image.** `nazm image` writes the recipe and its inputs; it does not run Docker:

```dockerfile
FROM scratch
COPY --chmod=0555 hello /hello
COPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY passwd /etc/passwd
USER 65532:65532
EXPOSE 8443
ENTRYPOINT ["/hello"]
```

No shell, no package manager; a read-only root file system works; `passwd` is one line, a
`nonroot` user. The CA bundle is Mozilla's (MPL-2.0 data, its notice beside it). The image is
built with a fixed `SOURCE_DATE_EPOCH`, so two builds of one locked package give one digest — the
reproducibility `cyber` already proves for the executable, extended to the image.

### C.9 WASI services

`wasm32-wasip1` has no sockets and no threads, and stays as it is. Services use **WASI 0.2's
`wasi:http/proxy` world**: the host owns the socket, TLS and HTTP parsing, and calls the
component's `incoming-handler` once per request, in an instance of its own — concurrency is the
host's, which suits a module without tasks. A new target, `wasm32-wasip2`:

- `nazm build --target wasm32-wasip2 --http-handler hello` builds a component whose exported
  handler calls `hello`, a `Handler` — **the same function** the native service routes to. No new
  entry construct: the handler is named by the build, as H names its `--top`, and `N0832` refuses a
  handler program that reaches tasks or a listener.
- The canonical-ABI glue for `wasi:http` is generated once by `wit-bindgen` (Apache-2.0 WITH
  LLVM-exception) and kept as a runtime part; `wasm-tools component new` (same licence) wraps the
  core module. Both run in the `nazm-wasm` contained image beside `wasm-ld`.
- The reference host is **wasmtime** (`wasmtime serve`, Apache-2.0 WITH LLVM-exception), pinned in
  that image. TLS is the host's or a proxy's: the WASI leg of the gate runs behind a
  TLS-terminating proxy (Caddy, Apache-2.0) and says so; nothing claims the component did TLS.
- WASI 0.3 (native async) is expected to change the world's shape, so the target is
  **experimental** at 1.0.

### C.10 Evidence and the gate (task 1.1)

Every leg runs in the contained runner, one workload at a time; the load generator runs in a
container of its own on CPUs of its own (`--cpuset-cpus`), so it does not measure itself. The label
is *run-verified in a Linux container*; "production-ready" is never claimed.

1. *Conformance.* HTTP/2: `h2spec` (MIT) passes every case. HTTP/1.1: the parser fuzzed (`@fuzz`)
   and checked against `llhttp`'s test corpus (MIT) for every request it accepts or refuses. TLS: a
   client refuses an expired, a self-signed, a wrong-host and an untrusted-root certificate; a
   server offers nothing below TLS 1.2 and negotiates `h2` and `http/1.1` by ALPN; `testssl.sh`
   (GPL-2.0, run as a tool, never shipped) records the server's configuration.
2. *Load, closed loop.* `h2load` (nghttp2, MIT) at 1, 2, 4 and 8 workers over HTTP/1.1 and HTTP/2:
   a small `GET`, a 16 KiB JSON echo, and new-connection TLS handshakes. Throughput rises at each
   step and reaches at least 4× at 8 workers what it is at 1 on the small `GET`; a miss is a defect
   to fix, not a waiver.
3. *Load, open loop.* `wrk2` (Apache-2.0) at fixed offered rates, so latency is not hidden by
   coordinated omission: p50, p99 and p99.9 at each rate, filed.
4. *Soak.* Thirty minutes at 10 000 concurrent TLS connections: zero errors, resident memory flat
   within 10 % after the first minute, no growth in tasks or handles (`NAZM_SCHED_REPORT`).
5. *Graceful shutdown under load.* `SIGTERM` in the middle of a run: every request whose response
   had begun completes, no accepted connection is reset, the process exits within `drain_ms`, and
   the server's count of served requests equals the clients' count of answered ones.
6. *In a container.* Steps 1–5 against the `FROM scratch` image, as uid 65532 with a read-only
   root; its size and digest recorded, the digest reproduced by a second build.
7. *On WASI.* The same handler as a `wasm32-wasip2` component under `wasmtime serve` behind the
   proxy: step 1's functional corpus passes; steps 2 and 3 measured and filed, with no threshold
   (the host's concurrency is not Nazm's).
8. *Comparison.* The same workloads against Go's `net/http` and Rust's `hyper` in the same
   container, filed in `performance.md`; nothing is claimed beyond the filed numbers.
9. *Refusals and mutation.* A compatibility-corpus case for each code of C.7; mutants of the HTTP
   parser, HPACK, flow control and the drain caught by the conformance and shutdown tests.

### C.11 Phases and estimates

| Phase | Content | Size | Weeks |
| --- | --- | --- | --- |
| C1 | the `tls` runtime part with three bindings (OpenSSL; SChannel; macOS trust through `SecTrust`), `@std/tls`, verification tests | 4–6k lines (≈2k runtime text and shims) | 4–6 |
| C2 | `@std/http` HTTP/1.1 server and client, router, middleware, limits and timeouts | 5–7k | 4–5 |
| C3 | HTTP/2: frames, HPACK with Huffman, flow control, streams, ALPN; `h2spec` | 5–7k | 5–7 |
| C4 | `@std/signal` and the drain, `@std/config`, `@std/metrics`, `@std/log` fields | 2–3k | 2 |
| C5 | least-loaded placement, `SO_REUSEPORT`, connection cap; the musl targets; `nazm image` | 3–4k | 3–4 |
| C6 | `wasm32-wasip2`: component glue, `--http-handler`, the wasmtime harness | 3–5k | 4–6 |
| C7 | the gate harness: contained load, conformance and soak images, the shutdown test, the comparison | 2–3k | 2–3 |

**Total: about 24–35k lines and 24–33 weeks** of one track's work. C1 and C3 carry the risk: TLS on
three platforms and HTTP/2's flow control are where services fail in production. All of it is new
Nazm (libraries) and runtime text; no checker rule beyond C.7's codes, so the double-checker cost
is small.

### C.12 Twenty-year stability

| Promised at 1.0 (STABLE) | Left experimental |
| --- | --- |
| `@std/http`'s message model (`Method`, `Status`, `Headers`, `Request`, `Response`, `Body`, `Handler`) and its HTTP/1.1 and HTTP/2 behaviour as the RFCs state it | the router beyond `get`/`post`/`route` and `{param}`; middleware helpers |
| `@std/tls`'s API and its non-optional defaults (verification on, TLS 1.2 at least) | which engine a platform uses, and its cipher suites |
| `@std/signal`, `@std/config`, `@std/metrics` and the Prometheus text they write | `ServeOptions`' tuning fields; the placement policy; `SO_REUSEPORT` |
| the `*-unknown-linux-musl` targets | `wasm32-wasip2` and `--http-handler` (WASI 0.3 will move) |
| | `nazm image` and its recipe; every load number |

### C.13 Open questions for the copyright holder (direction-level)

1. **The TLS engine.** OpenSSL 3 on Linux and macOS (macOS trust decided by Security.framework) and
   SChannel on Windows; or one engine everywhere; or Network.framework on macOS at the cost of a
   second I/O path. Recommendation: the first.
2. **How much in `@std`.** Protocols, TLS, signals, metrics and configuration in `@std` under the
   twenty-year promise; frameworks, OpenTelemetry export and database drivers as packages.
   Recommendation: yes.
3. **Programs may watch for termination signals**, amending "no program watches for signals in
   1.0" — terminate, interrupt and hang-up only. Recommendation: yes.
4. **Static Linux executables on musl** (two new targets), not glibc. Recommendation: yes.
5. **WASI services on WASI 0.2's `wasi:http`**, experimental at 1.0, TLS by the host.
   Recommendation: yes, with wasmtime as the reference host.

---

## M — microcontrollers

### M.1 The idea in one paragraph

Everything a microcontroller program needs is already in the language and has run under QEMU:
`@std/hal`'s traits, `MmioCap`, statics and atomics, `@interrupt`, `@task` and the fixed-priority
scheduler, the watchdog, `@on_failure`, `bounds.json`, and board manifests (*Boards*). What has
never happened is a **real chip**: flash rather than an emulator's loaded memory, a clock tree to
bring up, a boot ROM with its own expectations, a vendor's interrupt controller, a probe to write
the image and a debugger to stop it. M adds two real boards through the existing mechanism — a
board is a manifest plus generated startup, a program names devices through `@std/hal` — and two
commands, `nazm flash` and `nazm debug`. The language gains no board knowledge; a board still
changes nothing about what a program means.

### M.2 The boards

**Cortex-M: the Raspberry Pi Pico 2 (RP2350, two Arm Cortex-M33 cores).** Chosen over an RP2040
Pico and an STM32 Nucleo:

| | Pico 2 (RP2350) — **chosen** | Pico (RP2040) | STM32 Nucleo (e.g. G474RE) |
| --- | --- | --- | --- |
| Core | Cortex-M33, ARMv8-M Mainline (a superset of the M3's ARMv7-M the tree already targets), single-precision FPU | Cortex-M0+, ARMv6-M: no hardware divide, no compare-and-swap | Cortex-M4F, ARMv7E-M |
| Emulator first | QEMU `mps2-an505` is a Cortex-M33 board upstream | none upstream for the chip | none upstream for the chip |
| Documentation and vendor code | full datasheet; SDK and SVD BSD-3-Clause | same | reference manual; some ST code under ST's own licence (SLA0044) |
| Flashing with no tool | UF2 drag-and-drop to its boot ROM's mass-storage device | UF2 | needs ST-LINK and a tool |
| Debug | SWD with the Raspberry Pi Debug Probe (CMSIS-DAP, open firmware) | same | on-board ST-LINK (proprietary firmware) |
| Bonus | its two **Hazard3 RISC-V** cores boot the same board | — | — |

The UART is an Arm PL011 — `@std/hal`'s `Pl011` impl already writes one — and the core's SysTick
and NVIC are the bindings Gate 3E wrote for the Cortex-M3. The work is the chip, not the core.

**RISC-V: the ESP32-C3 (RV32IMC, one core), on an ESP32-C3-DevKitM-1 or similar.** Chosen over the
RP2350's own Hazard3 cores, a GD32VF103 and a CH32V003:

| | ESP32-C3 — **chosen** | RP2350 Hazard3 | GD32VF103 | CH32V003 |
| --- | --- | --- | --- | --- |
| Why | a different vendor, boot ROM, memory map and interrupt controller from the Cortex-M board — so the second board tests the abstraction, not a second core on the same chip | same chip as the Pico 2 | old core (Bumblebee), small community | RV32EC; WCH's probe protocol is proprietary |
| Flash and debug | **built-in USB-Serial-JTAG**: one USB cable flashes, debugs and carries the console, no probe | the Debug Probe | an external JTAG probe | WCH-LinkE |
| Documentation | Technical Reference Manual; ESP-IDF Apache-2.0 | as RP2350 | vendor manual | vendor manual |

The Hazard3 cores (`riscv32imac`) are M's first "after the gate" family: the same board, cheaply.

### M.3 Targets

| Target | Chip | New in the backend | Floats | Atomics |
| --- | --- | --- | --- | --- |
| `thumbv8m.main-none-eabihf` | RP2350 (and QEMU `mps2-an505`) | the triple; hard-float calling convention | `Float32` in hardware; `Float64` refused by name (`N0854`, M1) at 1.0 — the FPU is single-precision | an `Int` cell is 64 bits and ARMv8-M has no 64-bit exclusive (`LDREXD`): interrupts masked around each operation, as the Cortex-M3's (corrected by M1) |
| `riscv32imc-unknown-none-elf` | ESP32-C3 (and QEMU `virt` with an RV32IMC CPU) | 32-bit RISC-V; the board runtime's 64-bit division | none: `Float32` and `Float64` refused by name | **no `A` extension**: interrupts masked around each operation, as the Cortex-M3's runtime does |

Both are 32-bit-pointer targets like `thumbv7m-none-eabi`: `Int` stays 64-bit and means what it
means everywhere, at a cost on a 32-bit core that `performance.md` will state; H's bit-precise
types (`UInt32`, `UInt16`) are the fast path for register work.

### M.4 What `@std/hal` maps to on each

| `@std/hal` | RP2350 | ESP32-C3 |
| --- | --- | --- |
| `Uart` | UART0, a PL011 at `0x4007_0000` — the existing `Pl011`, with baud divisors set by the board's `init` | UART0 at `0x6000_0000` (`Esp32c3Uart`), and the **USB-Serial-JTAG** FIFO at `0x6004_3000` (`UsbSerialJtag`) as the default console |
| `Pin` | SIO's set, clear and input registers (`Rp2350Pin`); function select in `IO_BANK0`, pads in `PADS_BANK0` (pad isolation cleared — an RP2350-specific step) | GPIO's `W1TS`/`W1TC` and input registers (`Esp32c3Pin`); function select in `IO_MUX` |
| `Timer` | `Ticks`, advanced by SysTick from the core clock (150 MHz) | `Ticks`, advanced by `SYSTIMER` alarm 0 (16 MHz), through the interrupt matrix |
| interrupt controller | NVIC — the existing binding | the interrupt matrix and the CPU's interrupt controller: a **new** `controller = "esp32c3"`, RISC-V's first; `N0626`'s refusal of `[interrupts]` on RISC-V is lifted for this binding only |
| watchdog | the chip's `WATCHDOG` block: the manifest's `[watchdog]` gains `kind = "hardware"`, fed by `watchdog_feed()` | `TIMG0`'s watchdog; the RTC and super watchdogs the boot ROM leaves running are fed or disabled by startup |
| failure hook, stack watermark, `bounds.json` | unchanged | unchanged |

Addresses are from each vendor's manual and are checked against it, register by register, in the
phase that writes them; the manifest, not the program, holds them.

**What `@std/hal` gains** (all at once before 1.0, because adding a method to a stable trait later
breaks every impl): `Uart.get(m) -> Option[Int]` (a byte if one has arrived); `Pin.configure(m,
mode)` with `PinMode.Output`, `Input`, `InputPullUp`, `InputPullDown`; `EdgeIrq` (a pin's rising or
falling edge as an interrupt the manifest names). `Spi` and `I2c` (blocking, polled) are
experimental at 1.0. Nothing for the RP2350's PIO, DMA or USB device, or the ESP32-C3's radio — the
radio's stack is Espressif's binary, which is not ours to ship.

### M.5 Startup, memory and linker scripts

Real flash changes three things QEMU let the boards skip:

1. **`.data` and `.bss`.** QEMU loads the whole image into memory; a chip runs from flash. The
   manifest's memory gains regions with a load and a run address (`[memory] flash = { origin,
   length }`, `ram = { origin, length }`), the generated `link.ld` places `.data` at a RAM address
   with a flash load address, and the generated `_start` copies `.data` and zeroes `.bss` before
   statics are initialised — §7.87 recorded this as "outside this guarantee" on hardware; M closes
   it.
2. **The chip's own start.** A manifest key `init = "rp2350_init"` names a Nazm function of the
   board's module (`@std/hal/rp2350`, `@std/hal/esp32c3`) that `_start` calls after memory and
   before `main`: clocks (RP2350: crystal, PLL to 150 MHz, peripheral resets released; ESP32-C3:
   the PLL the ROM left, watchdogs), pads, the console. It is ordinary Nazm over `mmio_*`, not
   runtime text, which keeps new code in Nazm (the rewrite direction) and keeps a board's
   initialisation readable by its users.
3. **The boot ROM's format.** RP2350's boot ROM boots only an image carrying an `IMAGE_DEF` block in
   its first 4 KiB; the generated startup emits it in its own section, placed first. ESP32-C3's ROM
   either boots the application directly from flash offset 0 (its direct-boot mode) or loads
   Espressif's second-stage bootloader (Apache-2.0), which loads an application in ESP's image
   format. M3 settles which against the chip revision; the image format, if needed, is written by
   `nazm flash` itself — it is a documented header and segments.

A manifest for the Pico 2 (abridged):

```toml
name = "pico2"
isa = "thumbv8m.main"
chip = "rp2350"
init = "rp2350_init"
stack = 16384
[memory]
flash = { origin = 0x1000_0000, length = 0x40_0000 }
ram = { origin = 0x2000_0000, length = 0x8_2000 }
[uart]
kind = "pl011"
address = 0x4007_0000
[interrupts]
controller = "nvic"
clock_hz = 150_000_000
timer_period_us = 1000
[interrupts.lines]
gpio = 21
[watchdog]
kind = "hardware"
ticks = 100
[image]
format = "rp2350"
```

And a program, generic over the HAL as Gate 3E's traffic light is, with one `main` per board:

```nazm
use std.hal
use std.hal.rp2350          // both board modules: `@cfg` is not written before a `use`
use std.hal.esp32c3

static TICKS: Atomic = atomic_new(0)

@interrupt("timer")
fn tick() -> Int ! {} {
    atomic_add(TICKS, 1)
    0
}

fn loopback[O: Pin, I: Pin, U: Uart](out: O, back: I, u: U, m: MmioCap) -> Int ! {} {
    out.configure(m, PinMode.Output)
    back.configure(m, PinMode.InputPullDown)
    let levels: [Bool; 4] = [false, true, false, true]      // an array: no heap on a board
    for level in levels {
        out.set(m, level)
        wait_ticks(ticks_of(TICKS), 2)
        uart_write(u, m, if back.get(m) == level { "ok\n" } else { "MISMATCH\n" })
    }
    0
}

@cfg(target = "thumbv8m.main-none-eabihf")
fn main(m: MmioCap) -> Int ! {} {
    loopback(rp2350_pin(2), rp2350_pin(3), rp2350_uart0(), m)
}

@cfg(target = "riscv32imc-unknown-none-elf")
fn main(m: MmioCap) -> Int ! {} {
    loopback(esp32c3_pin(4), esp32c3_pin(5), esp32c3_console(), m)
}
```

### M.6 Flashing and debugging

**Commands** (experimental, as `nazm hw` and `nazm accel` are):

- `nazm flash FILE --board pico2.toml [--probe ID]` builds, links, converts (ELF to UF2 or ESP's
  image) and writes the image, then **reads it back and compares its digest** before resetting the
  chip; the transcript of the console follows with `--monitor`. Before writing anything it reads the
  attached chip's identity through the probe and refuses a chip that is not the manifest's
  (`N0853`).
- `nazm debug FILE --board …` starts a GDB server on the probe and an `lldb` (or `gdb`) connected
  to it, with the image's DWARF — a breakpoint on a Nazm function by name, a step, a backtrace.

**The tools, never bundled, found on `PATH` as `clang` is, refused by name when absent (`N0852`):**

| Tool | Licence | Role | Decision |
| --- | --- | --- | --- |
| **UF2 copy** (no tool) | — | the RP2350 boot ROM mounts as a drive in BOOTSEL mode; `nazm flash` writes the UF2 it made (family id for RP2350 Arm, or RISC-V) | **the zero-dependency path for the Pico 2** |
| **probe-rs** | Apache-2.0 OR MIT | CMSIS-DAP (the Debug Probe), the ESP32-C3's USB-Serial-JTAG, ST-LINK and J-Link; flashing algorithms for both chips; a GDB server, RTT | **the one probe backend** for flash and debug on both boards |
| picotool | BSD-3-Clause (uses libusb, LGPL-2.1, dynamically) | RP2350 loading over USB | accepted as an alternative, not required |
| espflash | MIT OR Apache-2.0 | ESP32-C3 flashing over its USB serial | **the fallback** for the ESP32-C3 |
| esptool | GPL-2.0-or-later | Espressif's own flasher | invoked if present, never bundled or required |
| OpenOCD | GPL-2.0-or-later | GDB server | invoked if present, never bundled or required |
| STM32CubeProgrammer | ST's proprietary licence | — | never used |

Docker Desktop on macOS cannot pass USB devices into a container, so flashing runs **on the host**:
it is light, needs no build of the compiler, and the build of the image itself still runs where it
always does. RTT (a ring buffer in RAM a probe reads without stopping the core) is an optional
console channel, implemented in the board runtime from the protocol's description.

### M.7 Checker rules and diagnostics

No language rule changes: boards are data. Provisional codes:

| Code | Constant | Refuses |
| --- | --- | --- |
| `N0850` | `BOARD_MEMORY` | an image, its `.data` or its stack bound (`bounds.json`) that does not fit the manifest's flash or RAM, at link time, naming the region and the overflow |
| `N0851` | `BOARD_INIT` | a manifest `init` that names no function `fn() -> Int ! {}` or `fn(m: MmioCap) -> Int ! {}` reachable in the image |
| `N0852` | `FLASH_TOOL` | `nazm flash` or `nazm debug` with no tool for the board's probe, or no probe or boot-ROM drive attached — refused by name, never a silent fallback |
| `N0853` | `BOARD_CHIP` | the attached chip, as the probe reads it, is not the manifest's `chip`, or is a revision the board does not support — refused before anything is written |

A hardware watchdog's expiry resets the chip; the next start reads the chip's reset reason and
reports `N0415` (*WATCHDOG_EXPIRED*) on the console before `main`, so the existing code keeps its
meaning on real hardware.

### M.8 Evidence and the gate (task 1.2)

**First under emulation, then on the board.** Every program below is first *emulator-verified* — on
QEMU `mps2-an505` for the Cortex-M33, and QEMU `virt` with an RV32IMC CPU for the ISA (the
ESP32-C3's peripherals are not emulated upstream; Espressif's QEMU fork, GPL-2.0, is a tool that
may be run, never shipped) — and then run on the boards. A new label, **run-verified on hardware
(board, chip revision)**, is used only for what ran on that board.

1. *The HAL programs on both boards:* Gate 3E's traffic light, unchanged in source; the loopback
   above (a jumper from one pin to another, so the board checks its own output and no one has to
   look at an LED); a UART echo (the host sends 10 000 random bytes, the board returns them, the host
   compares); a GPIO edge interrupt fired through the loopback.
2. *Time:* the board prints its tick count each second by its own timer; the host timestamps each
   line; drift over sixty seconds within 0.1 % (both boards have crystals).
3. *The scheduler and failure paths:* the realtime workload's `@task`s at two priorities; a missed
   deadline reported as `N0414`; a hung loop caught by the **hardware** watchdog and reported as
   `N0415` after the reset; an overflow reported through `@on_failure`; the stack watermark beside
   the static bound.
4. *Domain workloads that fit:* `industrial`, `automotive` and `medical` (no floats) on both boards;
   `robotics` and `aerospace` need `Float64` and are refused by name on both, said so.
5. *Agreement:* each console transcript, timing lines aside, byte-identical to the same source's
   transcript under QEMU and under the interpreter.
6. *Flash and debug:* `nazm flash` writes and verifies by read-back on both boards; `nazm debug`
   stops at a breakpoint on a Nazm function, prints a backtrace with Nazm names, and steps.
7. *The record:* for each run, the image's digest, the manifest, the probe-rs version, the chip's
   identity and revision as read, and the transcript, under `evidence/boards/`, written by
   `scripts/board-run.sh`.
8. *Refusals and mutation:* corpus cases for `N0850`–`N0853` (the last two with a fake probe); the
   startup's `.data`/`.bss` handling held by a test that changes a static and resets.

**What the copyright holder does by hand.** Buys the boards (a Pico 2, a Raspberry Pi Debug Probe,
an ESP32-C3-DevKitM-1, jumper wires, two USB cables — about USD 30); installs probe-rs on the Mac
(one command; Apache-2.0 OR MIT, nothing to accept); connects the Debug Probe's cable to the Pico
2's SWD header and puts two jumpers on each board; plugs both in; and starts
`scripts/board-run.sh pico2` and `scripts/board-run.sh esp32c3` (or lets the session run them on
the host). Every check after that is automated — nobody judges an LED by eye. The first Pico 2 boot
may need the BOOTSEL button held once.

### M.9 Phases and estimates

| Phase | Content | Size | Weeks |
| --- | --- | --- | --- |
| M1 | the two targets; manifests with flash/RAM regions, `.data` copy and `.bss` zeroing, `init`, `[image]`; emulator legs on `mps2-an505` and RV32 `virt` | 4–6k lines (runtime text, backend, manifests) | 4–5 |
| M2 | RP2350: `@std/hal/rp2350` (clocks, resets, pads, PL011 set-up, SIO pins, hardware watchdog), `IMAGE_DEF`, UF2 | 2–3k Nazm | 2–3 |
| M3 | ESP32-C3: boot (direct or second stage), watchdogs, USB-Serial-JTAG, UART, GPIO, `SYSTIMER`, the interrupt-matrix controller binding | 3–4k | 3–5 |
| M4 | `nazm flash` and `nazm debug`: UF2 and ESP image writers, probe-rs and espflash adapters, read-back, chip identity, RTT | 2–3k | 2–3 |
| M5 | `@std/hal` growth before 1.0: `Uart.get`, `Pin.configure`, `EdgeIrq`; `Spi` and `I2c` experimental | 2–3k | 2–3 |
| M6 | `scripts/board-run.sh`, transcripts, the board runs | 1–2k | 1–2 + board time |

**Total to the gate: about 14–21k lines and 14–21 weeks.** M3 is the risk: the ESP32-C3's boot path
and interrupt controller are the least like anything in the tree. **After the gate** (not 1.0's):
the RP2350's Hazard3 cores (`riscv32imac`), an STM32 (Cortex-M4F, `thumbv7em-none-eabihf`), the
RP2040 (`thumbv6m`: no divide, no compare-and-swap, so atomics by masking), the ESP32-C6
(`riscv32imac`) — about 1–2k lines and 1–2 weeks each, a manifest and a board module apiece.

### M.10 Twenty-year stability

| Promised at 1.0 (STABLE) | Left experimental |
| --- | --- |
| `@std/hal`'s traits `Uart`, `Pin`, `Timer`, `EdgeIrq` with M.4's methods, and `PinMode` | `Spi`, `I2c`; each board module's own functions (`rp2350_*`, `esp32c3_*`) |
| the two targets, and that a board never changes a program's meaning | manifest keys added by M (`[memory]`, `init`, `[image]`, `chip`), under the manifest schema's version |
| `N0414`, `N0415` keep their meaning on hardware | `nazm flash`, `nazm debug`, their adapters and the tools they call |
| | which boards are on the support matrix; the board list grows and may shrink with the hardware market |

Chips are discontinued within twenty years; the stable promise is the HAL's traits and the
targets' ISAs, not a particular board.

### M.11 Open questions for the copyright holder (direction-level)

1. **The two boards:** the Raspberry Pi Pico 2 (RP2350) with a Raspberry Pi Debug Probe, and an
   ESP32-C3-DevKitM-1. Recommendation: these; bought when M6 comes (the answers of 2026-10-15
   already say "bought when the board runs come").
2. **probe-rs on the Mac host**, as the one flashing and debugging tool, outside any container
   (Docker cannot reach USB on macOS). Recommendation: yes; UF2 copy as the no-tool path.
3. **`Float64` on microcontrollers** at 1.0: refused by name (the RP2350 FPU is single-precision,
   the ESP32-C3 has none), or software routines (compiler-rt's, Apache-2.0 WITH LLVM-exception).
   Recommendation: refused at 1.0, software floats after.
4. **A hardware watchdog behind `[watchdog]`** on boards that have one, rather than the
   scheduler's. Recommendation: yes, selected by `kind = "hardware"`.

---

## D — drivers

### D.1 The idea in one paragraph

A driver is a freestanding program whose platform is a **kernel** instead of a board. Nazm already
has the low level a driver needs — `mmio_*` under `MmioCap`, `@interrupt`, statics and sequentially
consistent atomics, `@on_failure`, a `kernel` profile (`no-spawn`, `no-foreign`, `no-blocking`,
`no-heap`), stack bounds — and has run it on emulated boards. D puts that inside two operating
systems: a **Linux kernel module** and a **Windows kernel-mode driver**, each serving one virtual
character device, `nazmring`, written once in Nazm. As with boards, a kernel's interface is
**data plus generated text**: a driver manifest (`driver.toml`) names the device and which Nazm
functions serve which operations, and `nazm build` generates the small C shim that the kernel's own
build compiles against its own headers. The language gains one storage form (`AtomicArray`), one
kernel-owned type (`KLock`), one handle (`UserBuf`) and a `driver` profile whose new rules catch the
two classic kernel bugs at compile time: a lock not released on some path, and a call that may sleep
while a spinlock is held.

### D.2 The ABI to each kernel

**Linux has no stable in-kernel ABI.** A module is tied to one kernel build: its configuration
decides structure layouts, `vermagic` and, under `CONFIG_MODVERSIONS`, a CRC per imported symbol;
x86-64's `objtool` checks every object for its unwinder and mitigations. Two ways in were weighed:

| | **Kbuild + generated C shim** — chosen | Direct objects |
| --- | --- | --- |
| How | `nazm build` emits the Nazm code as one object and generates `shim.c`, `Kbuild` and a `Makefile`; `make -C KDIR M=…` compiles the shim against that kernel's headers and links both into `nazmring.ko` | Nazm writes the `.ko` itself: `.modinfo`, `__versions` CRCs, `struct module`'s layout, `.gnu.linkonce.this_module` |
| Layouts (`struct file_operations`, `struct miscdevice`) | the kernel's own headers, by the compiler that built the kernel | hand-copied per configuration |
| `modpost`, `vermagic`, CRCs, `objtool` | the kernel's own tools | reimplemented, forever behind |
| Verdict | **chosen**: the kernel's build owns everything that changes between kernels; Nazm owns the code | refused: it would chase every kernel release |

The Nazm object calls only shim functions, never a kernel export directly, so its undefined symbols
are the shim's and stable across kernels. Code generation must match the kernel's: `nazm build`
asks Kbuild for its flags (`make -s -C KDIR M=… nazm-flags`, a target the generated `Makefile`
defines) and maps each by a table to LLVM function attributes and module flags — the kernel code
model and no red zone on x86-64, no floating-point or vector registers, return thunks and
retpolines, indirect-branch tracking (`endbr64`) and `-fpatchable-function-entry`, kCFI type hashes
on a `CONFIG_CFI_CLANG` kernel; on AArch64, pointer authentication and BTI, and the shadow call
stack (`x18` reserved). **A flag outside the table refuses the build** (`N0873`), naming it — never
a module that loads and then corrupts. Both a clang-built kernel (`LLVM=1`) and a GCC-built
distribution kernel are supported; `objtool` must pass on the Nazm object without a warning.

**Windows has a stable kernel ABI** — `ntoskrnl.exe`'s exports, the Windows x64 calling convention,
`IRP` and `DEVICE_OBJECT` layouts fixed per architecture — but the usual way to build against it is
the Windows Driver Kit, whose licence the copyright holder has not accepted and which no agent may
accept for him (*Windows x86_64*: "no licence is accepted on the user's behalf"). D designs around
it:

| | **WDM, built with MinGW-w64** — chosen for 1.0 | KMDF |
| --- | --- | --- |
| Headers and import libraries | MinGW-w64's own `ddk/` headers (`wdm.h`, `ntddk.h`) and `libntoskrnl.a`, `libhal.a` — the permissive licences the existing Windows target already uses | the WDK's headers and `WdfDriverEntry.lib` stub |
| Licence | nothing of Microsoft's accepted | **the WDK's licence: the copyright holder's decision** (D.12) |
| Model | a legacy (non-PnP) driver: `DriverEntry`, `IoCreateDevice`, a symbolic link `\\.\NazmRing`, dispatch routines for create, close, read, write and device control, **buffered I/O** (the I/O manager copies user buffers, so no user pointer is probed) | the framework's objects and queues; the "KMDF-style sample" the brief names |
| Verdict | **chosen** | written as an alternative the shim generator can grow into, behind the holder's decision |

The `.sys` is a PE image of the native subsystem (`-nostdlib`, `--subsystem,native`, entry
`DriverEntry`, `-lntoskrnl -lhal`), its checksum set, its shim compiled by MinGW-w64's `gcc` in the
existing `docker/wine.Dockerfile` image.

### D.3 The device: `nazmring`

One virtual device on both systems, so one Nazm module serves both and one test suite checks both:
a 4 KiB byte ring. `write` appends what fits and answers how many bytes it took; `read` takes up to
what was asked and answers how many (`0` when empty — the device does not block, D.6); an ioctl
`STATS` answers bytes written, bytes read and bytes held; `CLEAR` empties it. Linux serves it as a
misc device, `/dev/nazmring`; Windows as `\\.\NazmRing`. The ring's logic is ordinary Nazm,
`nazmring/ring.nz`, also compiled into a hosted model that `nazm test` drives, so the device's
transcript can be compared with the model's.

### D.4 Surface

```nazm
use std.kernel                         // KLock, UserBuf, Chunk, user_copy_in, user_copy_out

const SIZE: Int = 4096

static RING: AtomicArray = atomic_array_new(4096)   // one address; each cell changed atomically
static HEAD: Atomic = atomic_new(0)
static COUNT: Atomic = atomic_new(0)
static LOCK: KLock = klock_new()                     // the kernel's spinlock, interrupts saved

fn ring_write(u: UserBuf) -> Int ! {io} {
    let c = match user_copy_in(u) {          // up to 256 bytes, copied before the lock: it may sleep
        Ok(c) => c
        Err(e) => return e.errno()           // -EFAULT: a bad pointer is an error, never an oops
    }
    let flags = klock_acquire(LOCK)
    let head = atomic_load(HEAD)
    let mut count = atomic_load(COUNT)
    let mut stored = 0
    for i in 0..256 {                        // c.bytes is [UInt8; 256]: in range by the constant bound
        if i == c.len || count == SIZE { break }
        atomic_array_store(RING, wrap_convert[UInt12](head.wrapping_add(count)), Int(c.bytes[i]))
        count = count.wrapping_add(1)
        stored = stored.wrapping_add(1)
    }
    atomic_store(COUNT, count)
    klock_release(LOCK, flags)               // on every path, or N0870
    stored
}
```

`ring_read` is its mirror: it takes bytes into a `Chunk` under the lock, releases it, and only then
calls `user_copy_out` — written the other way round, the copy is a call that may sleep under a
spinlock, refused by `N0871`. The index `wrap_convert[UInt12](…)` is in range of a 4096-cell array
by its type (H's bit-precise integers and indexes by any integer type, H.2), which is what lets the
`driver` profile refuse every operation that could trap and still read naturally.

The manifest:

```toml
name = "nazmring"
os = "linux"                  # or "windows"
kind = "misc"                 # Linux: a misc character device; Windows: kind = "legacy"
device = "nazmring"
module_licence = "Dual MIT/GPL"
stack = 4096                  # bytes: the deepest operation's bound must fit (N0872)
[operations]
open = "ring_open"
release = "ring_release"
read = "ring_read"
write = "ring_write"
ioctl = "ring_ioctl"
exit = "ring_exit"
```

`main` is the module's initialisation, as a board's `main` is its image's (*A program's entry is
always `main`*): its `0` loads the module, anything else fails the load with that status as a
negative errno (`NTSTATUS` on Windows). `exit` names the function the unload calls. Each operation
has a fixed shape — `fn(u: UserBuf) -> Int ! {io}` for read and write, `fn(cmd: Int, u: UserBuf)
-> Int ! {io}` for ioctl, `fn() -> Int ! {}` for open, release and exit — and another shape is
`N0874`. Operations run concurrently on many processors; they share state only through statics,
atomics and `KLock`, so Nazm's absence of data races holds inside the kernel.

### D.5 New language and library pieces

| Piece | Meaning | Why it is not a second mechanism |
| --- | --- | --- |
| `AtomicArray` | a `static` of `N` `Int` cells at one address, `N` constant (`atomic_array_new(N)`), each cell read and changed only by `atomic_array_load`, `_store`, `_add`, `_swap`, `_compare_swap` with an index (outside it, `N0405`); every operation sequentially consistent, as `Atomic`'s are | statics are read-only except through atomics (*Statics*); this is the same rule for a table, and it works on boards and hosts too |
| `KLock` | a kernel-owned spinlock as a `static` (`klock_new()`): `klock_acquire(l) -> Int` saves and masks interrupts (`spin_lock_irqsave`; `KeAcquireSpinLock`, raising to `DISPATCH_LEVEL`) and answers the saved state; `klock_release(l, saved)` restores it | the kernel's own primitive behind the shim: Nazm adds no lock implementation, and the realtime profiles' "no lock exists" stays true there |
| `UserBuf` | the caller's buffer for the operation in progress: valid only during it; read and written only by `user_copy_in(u) -> Result[Chunk, KernelError]` and `user_copy_out(u, c: Chunk) -> Result[Int, KernelError]` (`copy_from_user`/`copy_to_user`; on Windows the I/O manager's system buffer); a bad address is `Err(Fault)` | the handle is the authority, as a socket's handle is for `os_read`: no new capability |
| `Chunk` | `@std/kernel`'s record of a `[UInt8; 256]` and a length | plain data, no heap |
| `@std/kernel` | the above, `KernelError` with `errno()`, `kernel_log(level, s)` (`printk`/`DbgPrintEx`) | a library over built-ins, as `@std/hal` is |

**The `driver` profile** — in force for every build of a kernel-driver target, as the freestanding
refusals are for a board — is `kernel`'s rules and:

| Rule | Refuses | Source |
| --- | --- | --- |
| `bounded-stack`, `no-recursion` | a call cycle or a call through a function value | exists |
| `no-float` | `Float32`, `Float64`: neither kernel lets a driver touch floating-point state unguarded | H.4 |
| `trap-free` | every operation that can fail at run time, as H.4 defines it, with one admitted form both tracks gain: an index by a `for` counter over a constant range inside the array | H.4, extended |
| `kernel-stack` | an operation whose stack bound (`bounds.json`) is unknown or larger than the manifest's `stack` (Linux gives a thread 16 KiB, Windows 24 KiB on x64, and the driver is a guest on them) | new, `N0872` |
| `lock-balance` | a `klock_acquire` not followed by exactly one `klock_release` of the same lock on every path out of the function — early `return`, `break`, `?` included — or a release with no acquire | new, `N0870`; the "exactly once on every path" analysis H's `N0707` and Q's linearity share (H.4, Q.4) |
| `no-sleep-in-atomic` | a call that may sleep — `user_copy_in`, `user_copy_out`, anything that reaches them — between an acquire and its release, and in an `@interrupt` handler; on Windows the same rule is IRQL: nothing pageable or waiting at `DISPATCH_LEVEL` | new, `N0871`, over a table of which built-ins may sleep |
| `user-buffer` | a `UserBuf` stored in a static, returned, put in a record, or captured: it outlives its operation | new, `N0875` |

**Why `trap-free`.** A failure in a driver cannot unwind into the kernel: on Linux the defence is
`BUG()` (an oops, the calling process killed, the kernel tainted), on Windows `KeBugCheckEx` (the
machine stops). Rust for Linux accepts the first for its panics; Nazm refuses, at compile time,
every operation that could reach it, so the failure path in a driver — `kernel_log` at critical
level, `@on_failure`, then `BUG()` or a bug check — is reachable only through a compiler defect,
and is kept as defence, not as a mechanism. It is said in the matrix exactly so.

### D.6 What a 1.0 driver cannot do

No heap (`no-heap`: no `kmalloc`, so state is static), no blocking (`no-blocking`: a read of an
empty ring answers `0`; wait queues and `poll` are an extension), no tasks, no floats, no C called
by the program (the shim is generated, not written). Device drivers for real hardware — a PCI
device's registers through `mmio_*` lowered to `readl`/`writel` or `READ_REGISTER_ULONG`, its
interrupt through `@interrupt` and `request_irq`, a DMA buffer — are D5, experimental, against
QEMU's `edu` device in the Linux VM. Production signing for Windows is not in 1.0 (D.12).

### D.7 Where it is tested

**Linux, in the contained runner.** An image `nazm-kernel` with QEMU's system emulators and two
pinned guest kernels: an upstream LTS (6.12) built with `LLVM=1` and `KASAN`, `PROVE_LOCKING`
(lockdep), `DEBUG_ATOMIC_SLEEP` and `KCSAN` — so a sleeping call under a lock, a lock imbalance, a
race or an out-of-bounds access the checker missed is reported by the kernel itself — and Debian's
GCC-built distribution kernel with its headers package. Both run as AArch64 guests and the LTS also
as an x86-64 guest, under QEMU's emulation (Docker Desktop on Apple silicon gives no KVM): slow,
but a module test needs seconds of guest time. Each test boots a guest with the module and a hosted
Nazm test program in its initramfs, and reads the guest's console. Evidence label: *run-verified in
a Linux VM (QEMU, kernel X.Y, architecture)*.

**Windows, on the Dell.** The `.sys` is built and test-signed in the container: a self-signed code
signing certificate made by OpenSSL, the driver signed by `osslsigncode` (GPL-3.0, a tool run in a
container, never shipped). On the Windows 10 laptop that `scripts/windows-host.sh` already reaches
over `ssh`, with tools that ship with Windows only — `certutil` to trust the test certificate,
`bcdedit /set testsigning on`, `sc create … type= kernel` and `sc start`, `verifier` (Driver
Verifier) on the driver — the same hosted test program, built for `x86_64-pc-windows-gnu`, opens
`\\.\NazmRing`. Evidence label: *run-verified on a Windows host (test-signing mode)*.

### D.8 Evidence and the gate (task 1.3)

1. *Build.* The module builds against both Linux kernels and both architectures, `objtool` clean;
   the `.sys` builds and is signed; refused builds (`N0873` on an unknown flag) shown.
2. *Load and unload.* `insmod`/`rmmod` and `sc start`/`sc stop`, one hundred times each, with the
   device open in between where the system allows (Linux holds the module by its `owner`).
3. *Function.* The hosted test program: order kept, wrap-around, full and empty, `STATS` and
   `CLEAR`, a length of zero, a length beyond the ring; each transcript byte-identical to the hosted
   model's (`nazm test` on `ring.nz`).
4. *Hostile callers.* A null, unmapped and kernel-space pointer to `read`, `write` and the ioctl,
   and an unknown ioctl: an error status every time, never an oops or a bug check.
5. *Concurrency.* Sixteen tasks (the pool) reading and writing for sixty seconds on four virtual
   processors; conservation — bytes written equal bytes read plus bytes held — checked at the end.
6. *Clean.* After all of it, the guest's log has no `WARN`, `BUG`, KASAN, KCSAN or lockdep report,
   and no taint but the out-of-tree one; Windows ran under Driver Verifier with no bug check.
7. *Refusals.* A compatibility-corpus case for each rule and code of D.5 and D.9 — an imbalanced
   lock on each exit edge, a copy under a lock, a stored `UserBuf`, a deep operation, a float.
8. *Mutation.* Mutants of `lock-balance`, `no-sleep-in-atomic` and `user-buffer` caught by the
   corpus; mutants of the shim generator caught by the device tests.

### D.9 Diagnostics

| Code | Constant | Refuses |
| --- | --- | --- |
| `N0870` | `LOCK_BALANCE` | a `KLock` acquired and not released exactly once on some path, or released unacquired |
| `N0871` | `SLEEP_IN_ATOMIC` | a call that may sleep while a `KLock` is held or in an interrupt handler (IRQL on Windows), naming the call path |
| `N0872` | `KERNEL_STACK` | an operation whose stack bound is unknown or over the manifest's `stack` |
| `N0873` | `KERNEL_FLAGS` | a kernel build flag Nazm's code generation cannot match, the kernel's build tree missing, or an architecture the kernel is not |
| `N0874` | `DRIVER_OPERATION` | a manifest operation naming no function, or one of the wrong shape |
| `N0875` | `USER_BUFFER` | a `UserBuf` that could outlive its operation |

Refusals of the `driver` profile's other rules are `N0510`, naming the rule; `AtomicArray`,
`KLock` and `UserBuf` outside a target that has them follow `N0392` (`KLock` and `UserBuf` exist on
kernel-driver targets only; `AtomicArray` everywhere). The semantic epoch moves for the built-ins
and types, the runtime ABI for the driver runtime part, and `nazm.bounds/1` gains each operation's
bound. `AtomicArray`, `KLock` and `UserBuf` become built-in type names, so they must land before 1.0
(a new built-in name refuses a program's own type of that name, `N0331`).

### D.10 Phases and estimates

| Phase | Content | Size | Weeks |
| --- | --- | --- | --- |
| D1 | constitution: `AtomicArray`, `KLock`, `UserBuf`, `@std/kernel`, the `driver` profile and its rules in both checkers (on the shared exactly-once analysis), the `trap-free` loop-index form | 4–6k lines + tests | 4–5 |
| D2 | Linux: the kernel targets (`x86_64-linux-kernel`, `aarch64-linux-kernel`), Kbuild's flags mapped to LLVM attributes, the driver runtime part (failure path, user copies), the shim and Kbuild generator | 4–6k | 4–6 |
| D3 | `nazmring` and its hosted model and test program; the `nazm-kernel` image with two kernels and two architectures; the gate's Linux legs | 3–4k | 3–4 |
| D4 | Windows: `x86_64-windows-kernel` (native-subsystem PE, MinGW-w64's DDK), the WDM shim generator, IRQL in `no-sleep-in-atomic`, test signing, the Dell runbook and its run | 4–6k | 4–6 + host time |
| D5 | experimental: a PCI driver for QEMU's `edu` device — `mmio_*` through `readl`/`writel`, `@interrupt` through `request_irq`, a coherent DMA buffer | 2–3k | 2–3 |

**Total: about 17–25k lines and 17–24 weeks** of one track's work; D1 depends on Q1's exactly-once
analysis and H1's bit-precise integers, so it lands after them or builds the analysis first and
hands it to them. D2's flag table is the long-term cost: every kernel generation may add a flag.

### D.11 Twenty-year stability

| Promised at 1.0 (STABLE) | Left experimental |
| --- | --- |
| `AtomicArray` and its operations (every target) | the kernel-driver targets themselves: Linux promises no in-kernel ABI, so Nazm cannot promise one across Linux releases |
| `KLock`, `UserBuf`, `Chunk` and their meaning; `@std/kernel`'s functions | the driver manifest's schema and the generated shims |
| `N0870`–`N0875` keep their meaning | the `driver` profile's rule set (as every profile's may grow) |
| | Windows drivers: test-signed only; KMDF; the `edu` PCI driver |

### D.12 Open questions for the copyright holder (direction-level)

1. **The licence of what a module links.** A module needs a GPL-compatible `MODULE_LICENSE` to use
   the kernel's GPL-only exports and to avoid tainting it, and Apache-2.0 is not compatible with the
   GPL version 2 the kernel is under. The Nazm code in a module is the user's (the LLVM exception
   covers the compiler's output), but the driver runtime part and the shim templates are the
   toolchain's. Recommendation: license those two — a few hundred lines — under MIT as well as the
   toolchain's licence, and let the sample declare `Dual MIT/GPL`. Yours to decide, with the CLA's
   legal review.
2. **Windows: WDM without Microsoft's kit, or KMDF with it.** KMDF needs the WDK's licence accepted,
   which only you can do. Recommendation: WDM with MinGW-w64 for 1.0; KMDF only if you choose to
   accept the WDK.
3. **Testing on the Dell itself.** Test-signing needs Secure Boot turned off in the laptop's
   firmware (by you), shows a watermark, and a driver defect stops the machine with a blue screen;
   if BitLocker is on, have the recovery key before changing boot settings. The alternative is a
   Windows VM on it, which needs a Windows image and its licence. Recommendation: the laptop itself,
   with Driver Verifier on, a restore point first, and test-signing turned off afterwards.
4. **Production signing** (Microsoft's attestation signing: an EV certificate and a Partner Center
   agreement, both paid). Recommendation: not in 1.0; 1.0 claims "test-signed" only.
5. **Drivers refuse every trap** (`trap-free`), rather than oops as Rust for Linux does.
   Recommendation: yes — a driver that cannot fail at run time is the claim worth making.

---

## Shared decisions across C, M and D

- **No new capability.** C uses `NetCap`, `IoCap` and `SpawnCap`; M uses `MmioCap`; D's `UserBuf`
  carries its own authority as a socket handle does. The count stays at Q's eleven.
- **Platforms stay data.** Boards (M) and kernels (D) are manifests plus generated text; the
  checker, Core IR and MIR do not learn which board or kernel they target (`systems-domains.md`,
  *Platform behaviour lives in the runtime's platform layer*).
- **Outside tools are found, never bundled**, and refused by name when absent: OpenSSL is linked,
  probe-rs and espflash are run on the host, `wit-bindgen`, `wasmtime`, QEMU, MinGW-w64 and
  `osslsigncode` in contained images. Nothing GPL is linked into a Nazm program; GPL tools are run.
  Nothing of Microsoft's or a chip vendor's is accepted on the copyright holder's behalf.
- **H and Q supply language pieces** C, M and D reuse: bit-precise integers (H1) for registers and
  ring indexes, `trap-free` (H.4) for drivers, the exactly-once path analysis (Q1) for
  `lock-balance`. D1 is therefore ordered after H1 and Q1, or builds the analysis first for all.
- **Order.** C1–C3 and M1 have no language dependency and can start at once; C's and M's runtime
  parts are runtime text and Nazm, independent of the rewrite's cut-over; D1 is a language change
  and carries the double-checker cost until the cut-over.
- **Totals, for planning:** C 24–35k lines and 24–33 weeks; M 14–21k and 14–21 weeks to its gate;
  D 17–25k and 17–24 weeks — about 55–81k lines and 55–78 track-weeks for the three, beside H's
  23–34 and Q's 15–21.

## Decisions on C, M and D (2026-10-15)

**The copyright holder's:**
1. **Driver licensing:** the driver runtime and the C shim templates are licensed under MIT as well
   (the compiler stays Apache-2.0 WITH LLVM-exception), and driver samples declare "Dual MIT/GPL".
   This joins the open CLA and licence legal review before anything is published.
2. **Testing the Windows driver on the Dell:** decided when track D reaches it; until then the
   Windows driver is built and checked statically, not loaded.

**Delegated, taken on the designs' recommendations:** OpenSSL 3 on Linux and macOS with Apple's trust
evaluation, SChannel on Windows; protocols and operations in `@std`, frameworks as packages; programs
may watch for termination signals (amending the 1.0 non-claim, for graceful shutdown); musl targets
for static executables; WASI services on WASI 0.2, experimental at 1.0; the Raspberry Pi Pico 2 with
its Debug Probe and an ESP32-C3-DevKitM-1; probe-rs on the Mac host; Float64 refused on the MCUs at
1.0; a hardware watchdog behind `[watchdog]` where a board has one; no production driver signing in
1.0; drivers refuse every possible trap.

## Decisions during C2 (2026-10-16, delegated)

- **Handlers in tasks (`N0321`).** A function value may not cross into a task, so `http_serve` runs
  handlers one at a time on the serving task, and the parallel shape is a named function spawned per
  connection. Lifting `N0321` for function values that capture only crossable values is a checker
  change: it is made after the cut-over (R8), written once in Nazm, as the rule for every language
  change in these tracks; `@std/http` then gains a parallel `http_serve` without breaking the two shapes.
- **Allocation counts across tiers in networked programs** differ (the interpreter counts fewer
  string allocations than compiled code on the same exchange) while every tier reclaims everything;
  the socket tests check output and full reclamation. Recorded for the interpreter's next round.

## Decisions during C1 (2026-10-16, delegated)

- **OpenSSL on macOS is found, not bundled.** C.3 planned a pinned static build carried in the
  toolchain's archive; C1 links Homebrew's `openssl@3` dynamically instead (`NAZM_OPENSSL_DIR` for
  another installation), as Linux links the distribution's `libssl.so.3`. Nothing of OpenSSL's is
  shipped, so `THIRD-PARTY-NOTICES.md` gains nothing, and Homebrew's security updates reach every
  program built against it. The cost: a TLS program built on a Mac needs `openssl@3` on the Mac it
  runs on. C5's static musl executable keeps the plan of a pinned static OpenSSL, with its notice.
- **Only live code links the library.** `@std/http` reaches the engine, and every module is lowered
  whole, so every HTTP program reaches TLS's runtime part. An executable is dead-stripped, so the
  link runs first without the TLS library and again with it only when a call the stripping kept
  needs it: a plain HTTP program never needs OpenSSL installed.
- **No capability.** The built-ins reach nothing but the engine — bytes in, bytes out — so each is
  `io` and needs nothing held; the socket the bytes travel through is the authority, as C.7 said.
- **Configurations and sessions are ids**, a slot and a generation in a table of 65 535, never an
  address: a closed or made-up id answers `Closed`. A configuration is parsed once and shared by its
  sessions, each keeping its own reference to what it took, so closing one early harms none.
- **macOS trust without network fetches.** `SecTrustSetNetworkFetchAllowed(false)`: a chain the
  server did not complete fails there as it does under OpenSSL's verification on Linux.
- **ALPN by the server's preference**; no name in common goes on without one.
- **The interpreter refuses TLS as a value** (`-ENOSYS`, then `Closed`): the workspace forbids
  `unsafe`, so the interpreter cannot call C. TLS programs run on every compiled tier. An engine for
  the interpreter would be a helper process built from the same runtime part — recorded, not built.
- **`@std/http`'s surface:** `http_listen_tls` and `http_connect_tls` beside the plain ones, an
  `https` URL verified against the platform's store by default, and `HttpError.Tls(error)`.
- **Windows (SChannel) is C1's next step**; until it lands a Windows build reaching TLS is refused
  by name, `N0830`, the code C.7 reserved.

