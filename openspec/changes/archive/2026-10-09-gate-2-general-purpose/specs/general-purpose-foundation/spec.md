## ADDED Requirements

### Requirement: Hash maps, sets and deques in the standard library
The standard library SHALL offer `@std/hash`, `@std/hashmap`, `@std/hashset` and `@std/deque` with
the complexities `docs/general-purpose.md` §7 states, and SHALL give every program the same hash and
the same iteration order in every implementation and every run unless a seed is given.

#### Scenario: a map agrees in every implementation
- **WHEN** a program inserts, looks up, removes and iterates a `HashMap`
- **THEN** its output is the same interpreted, under LLVM and under Cranelift

#### Scenario: a number is a key
- **WHEN** a program implements a trait for a numeric type
- **THEN** the impl is accepted, as one for `Int` was (`docs/spec.md`, *Traits and methods*)

### Requirement: Operating-system work is a value, under a capability
Files, the network, child processes, clocks and entropy SHALL be reached only through the
capability `docs/general-purpose.md` names for each, and their failures SHALL be values
(`Result[_, IoError]`), never traps.

#### Scenario: a missing file
- **WHEN** a program opens a path that does not exist
- **THEN** it receives `Err` with kind `NotFound`, in every implementation

### Requirement: A network wait does not hold a worker
On the M:N pool, a task waiting on a socket SHALL release its worker until the socket is ready or
its deadline passes (`docs/general-purpose.md` §10).

#### Scenario: one worker, many readers
- **WHEN** one worker runs many tasks parked in network reads and one busy task
- **THEN** the busy task completes while the readers wait
