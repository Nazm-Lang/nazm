## 1. The proposal and the harness

- [x] 1.1 The missing constructs in dependency order (`proposal.md`) and the decisions (`design.md`)
- [x] 1.2 The inputs: `toolchain/frontend-tests/extract-inputs.py` → `toolchain/frontend-tests/inputs/rs/`
- [x] 1.3 The reference dumper `toolchain/frontend-tests/refdump/` (tokens, tree)
- [x] 1.4 `toolchain/frontend-tests/run.sh`: builds the drivers with the reference under the heap limit, compares
      every input, prints `N agree, M differ` per comparison
- [x] 1.5 `toolchain/frontend-tests/host-check.sh`: the reference checks the compiler and the drivers (D6; no
      stage runs on the host)

## 2. Lexer and parser complete

- [x] 2.1 Every token (group A): escapes, `::`, `..`, bit and compound operators, `@`, radix
      integers, `IntTooLarge`, floats, raw, multi-line and interpolated strings, block comments;
      every lexing error collected — compared on every input (`tokens`)
- [x] 2.2 The tree printer: the arena printed as the Rust parser's `{:#?}` (D3) — `compiler/syntax.nz`,
      `sx_print`; `compiler/tree.nz` is the driver
- [x] 2.3 Declarations (group B) parsed
- [x] 2.4 Types (group C) parsed
- [x] 2.5 Statements (group D) parsed
- [x] 2.6 Expressions and patterns (group E) parsed
- [x] 2.7 Syntax diagnostics and recovery as the reference's — every input's tree agrees (`tree`):
      code, span and message, with the reference's recovery, cascade muting and nesting limit — and
      as whole `nazm.diagnostic/1` objects, labels, help and fixes included (`syntax`;
      `compiler/diag.nz`, `compiler/diagnose.nz`)
- [ ] 2.8 The checker and emitter read `syntax.nz`'s tree; `parse.nz` retired

## 3. Names and types

- [ ] 3.1 Modules: `as`, `::`, re-exports, packages and manifests — the loader is done (`load.nz`: the
      root, its imports depth-first, `@std/`, the core prelude, global offsets, every loader
      diagnostic but a package's); packages since 3.1ze (path dependencies), re-exports since 3.1zh;
      derives are not
- [x] 3.1a The `check` comparison (`run.sh check`): loader, every file's syntax diagnostics at their
      global offsets, and the missing `main` — every difference left is a checker diagnostic
- [ ] 3.1b The checker's first stages (`compiler/sema.nz`, D13): one module and the prelude; type
      names, fields, function declarations, bodies of the core subset, the `main` rules; the built-in
      table generated (D14). `run.sh check`: 1,908 agree before, 2,112 after; the checker carries
      922 inputs and all 922 agree (the loader stops at a package, which it does not resolve)
- [ ] 3.1c Effects and authority (`effects::analyse`): declared sets (N0366, N0367, N0368), each body's
      authority (N0369, R1.5 D8's output authority held), the generated table's sinks; a call that
      writes to a path stops the checker until provenance (N38) is carried, and so does a module of a
      package. `run.sh check`: 2,159 agree, 577 differ; the checker carries 1,247, all agree
- [ ] 3.1d Generics (`check_generic_call`, D15): type parameters of functions, records and enums
      (N0331, N0350, N0101, N0365), `Equality` requirements (N0364, N0304's help), calls by
      written or inferred type arguments (N0353, N0354, N0355, N0351), the generic built-ins by
      class, generic enums' arguments inferred — the prelude's `Some`/`None`/`Ok`/`Err` — and
      constructions with written arguments (N0352), `main` generic (N0356), the nesting bound
      (N0338), `List[T]`; `Vec`s and applications of any type. `run.sh check`: 2,238 agree, 498
      differ; the checker carries 1,436, all agree
- [ ] 3.1e `match` and patterns (`check_match`, `usefulness`): every pattern form but a tuple's
      (N0342, N0345, N0341, N0333, N0332, N0334, N0629, N0300, N0202), guards, `if let`,
      `while let`, `let … else` (N0630) and `let` of a pattern, arm types (N0302), reachability
      (N0344) and exhaustiveness with the reference's witness (N0343), and N0632's warning,
      published only where there is no error. `run.sh check`: 2,307 agree, 429 differ; the
      checker carries 1,598, all agree
- [ ] 3.1f `?` (`check_propagate`): the prelude's `Result` and `Option` by definition (N0360,
      N0361, N0362, N0300). `run.sh check`: 2,342 agree, 394 differ; the checker carries 1,648,
      all agree
- [ ] 3.1g Imports and module graphs (D16): every stage over every module in load order, each
      module against its direct imports' interfaces — `pub` visibility, plain imports and imports
      under a name (`alias::f`, `alias::T`, qualified constructions, variants and patterns), their
      collisions (N0207, N0208, N0209), private and unimported names (N0206, N0201 and N0305 with
      where the name is), unknown qualified names (N0210), another module's undeclared effects
      assumed, a standard module's authority; re-exports stop. Hand-written cases in
      `toolchain/frontend-tests/inputs/modules/`. `run.sh check`: 2,369 agree, 392 differ; the
      checker carries 1,704, all agree
- [ ] 3.1h Interpolation (`check_hole`): `f"…"` and what `print(x)` is written as — a precision only
      on a float (N0006), a `Str`, a number or a `Bool` written as it is, anything else refused
      (N0300: no compilation the checker carries has a trait, so none has a `Show`). `run.sh check`:
      2,375 agree, 388 differ; the checker carries 1,793, all agree
- [ ] 3.1i A compilation with a syntax error (`analyse`): the checker checks what the parser
      recovered, every module, a function whose body did not parse declared and its body skipped;
      its errors follow the parser's, without the missing `main` and without warnings.
      `run.sh check`: 2,392 agree, 375 differ; the checker carries 1,890, all agree
- [ ] 3.1j Constants of `Int`, `Bool` and `Str` (`constants::evaluate`, L8): stage 1b in dependency
      order, each module's refusals (N0622, N0623, N0003, N0300, N0203) and its `pub` ones exported to
      its importers, plainly and as `alias::NAME`; a constant named in a body is its type. A
      constant of another type, a static, a float, a shift, a bit operator or a conversion in a
      constant stops the checker. `run.sh check`: 2,399 agree, 371 differ; the checker carries
      1,899, all agree
- [ ] 3.1k Rebased on R1.5 D9 (D16): where the prelude gives a program module a standard module
      the checker stops — two carried inputs differed once the reference had D9 — until the
      implicit edges are carried. `run.sh check` against `main` at `4b060478`: 2,399 agree, 371
      differ; the checker carries 1,897, all agree
- [ ] 3.1l Function types and closures (N50): `fn(T…) -> R ! {e}` written, shown, assignable by
      effect inclusion and unified structurally; a named function as a value (N0387, and N0366
      where an undeclared one does more than its value's type says); calls through a value
      (N0303, N0300); closures, short ones included (N0635), their result (N0300), their
      captures — a `let mut` shared, refused where it can hold a function value (N0386), and a
      capture of what can hold one refused (N0616) — and each closure a function of its own in
      effects and authority ("this closure in `f`"). An effect parameter still stops the checker.
      `run.sh check`: 2,443 agree, 329 differ; the checker carries 1,983, all agree
- [ ] 3.1m Traits and impls (N78, `check/traits.rs`): traits in the type namespace (N0330), their
      methods with `Self` (N0203, N0609), a trait where a type is written (N0607); each module's
      trait scope, plain and `alias::Trait` (N0207); impl headers (N0610, N0305, N0604) and their
      methods declared as `Trait/Type/m` with `Self` and the trait's effects; `register_impl` —
      the orphan rule (N0601), each method against the trait's (N0603), what is missing (N0602),
      coherence (N0600); bounds `[T: Trait]` and their calls (N0364); method calls on a concrete
      type, through a bound and as `Trait.m(x)` (N0605, N0606, N0303, N0300), a method taken as a
      value (N0608); `Show` in interpolation; default methods (L11a), `Trait/default/m` generic over
      `Self`, called where an impl leaves the method out; impls of a generic type (L11b) — the
      header `D[T…]` (N0604), each method's `Self` its own application, what each parameter
      requires, met per instance. A derived `Hash` and a method called by name (5b.15) stop the
      checker. Hand-written cases in `inputs/check/traits-*.nz` and `inputs/modules/traits-*.nz`. `run.sh check`: 2,469 agree,
      309 differ; the checker carries 2,036, all agree — with default methods 2,473 agree, 305
      differ; the checker carries 2,044, all agree — with generic impls 2,478 agree, 301 differ;
      the checker carries 2,050, all agree
- [ ] 3.1n R1.5 D9's implicit edges (D16): the loader loads the prelude's standard modules a
      program module names, after its written imports; the checker adds them as
      `import_implicit` does, their names yielding to the module's own and to written imports'.
      `@std/hash`'s `Hash` is derived (L6, `Traits::derives`): found as a method, satisfying a
      bound, called as a bound's method is, and N0364 saying which component refuses it. The 25
      inputs that stopped at D9 now stop at `@std/hash`'s `UInt64` constants (a constant of
      this type); `inputs/modules/prelude-text.nz` is carried. `run.sh check`: 2,478 agree,
      301 differ; the checker carries 2,051, all agree (2,479 agree with the new case)
- [ ] 3.1o `for` (`check_for`): an `Ints`, a `Strs` or a `Vec[T]` iterated (N0628 otherwise), or
      `Int` bounds `a..b` (N0300); the binding the element, in the body's scope, a loop for
      `break` and `continue`, its last line discarded. A `for` over `Bytes` stops the checker.
      `inputs/check/for-loops.nz`. `run.sh check`: 2,489 agree, 292 differ; the checker carries
      2,073, all agree
- [ ] 3.1p Numbers (Gate 2, D17): every integer width and both floats — a literal of the type
      expected and its range (N0003), `-128` as one literal, wide integers, `Float32` where one is
      expected; the bit operators, `~` and shifts, a constant amount out of range (N0617);
      conversions `T(x)`, `try_convert`/`wrap_convert`/`saturate_convert[T](x)` and `Text(s)`
      (N0300, N0303, N0351, N0354); a `Str` literal as a `Text`; wide integer patterns; constants
      of every number type computed exactly (N0003, N0622, N0623) — `@std/hash`'s `UInt64`
      constants included. A float compared or converted to an integer in a constant stops the
      checker. `inputs/check/numeric-body.nz`, `inputs/check/numeric-consts.nz`. `run.sh check`
      (reference rebuilt at `deca183c`): 2,503 agree, 280 differ; the checker carries 2,132, all
      agree
- [ ] 3.1q Tasks (N53): `Chan[T]` written (N0353, N0390), shown, unified and inferred through its
      element, the generic channel built-ins (`chan_new_of`'s element refused as N0390,
      `chan_select_of` over a `Vec[Chan[T]]`); `scope` (N0300 for a value) and `spawn` — inside a
      scope only (N0320, a closure's body its own), the callee by the ordinary rule (N0201,
      N0303, N0300), a generic one refused (N0101), each argument by the task-safety walk
      (`task_safety`, N0321, with the path to what may not cross); `spawn`'s effect and its
      `SpawnCap` authority (N0366, N0369). `inputs/check/tasks.nz`. `run.sh check`: 2,543 agree,
      241 differ; the checker carries 2,289, all agree
- [ ] 3.1r Provenance (N38, N80, D18): `write_file`, `os_open_write`, `os_spawn*` and the other path
      sinks checked as built-ins; every body reduced to flows from what the checker resolved, the
      program solved and `N0372` reported with its witness chain — records and variants field by
      field, `match` and `?`, summaries and parameters handed on, closures and their captures,
      function values, calls through bounds, containers' cells and shared `let mut` bindings.
      `inputs/check/provenance-flows.nz`. `run.sh check`: 2,549 agree, 235 differ, carried 2,314
      where `N0372` cannot fire; with the flows 2,649 agree, 143 differ; the checker carries
      2,480, all agree
- [ ] 3.1s Attributes (D19, `attributes::configure`): the vocabulary, the board's shapes, one hook
      and one handler for each interrupt, `@codec` without a derive, and `@cfg` over the host —
      what it drops gone before the checker; a `@derive` stops the checker.
      `inputs/check/attributes-board.nz`, `inputs/modules/attributes-across.nz`. `run.sh check`:
      2,612 agree, 176 differ; the checker carries 2,386, all agree
- [ ] 3.1t Statics (Gate 3, `constants::evaluate` and `check_assign`): a static of a number or a
      `Bool` computed as a constant is, an `Atomic` a cell started by `atomic_new(c)` (N0300,
      N0622), named in a body as an `Atomic` and never read by a constant; a static of another
      type (N0622), exported (N0101) or assigned (N0625), and never among a module's exports.
      `inputs/check/statics.nz`, `inputs/modules/statics-across.nz`. `run.sh check`: 2,619
      agree, 172 differ; the checker carries 2,412, all agree
- [ ] 3.1u Tuples (L5, `tuple_of`): the generic record of each arity, `(,)` with fields `0`, `1`, …,
      made the first time it is asked for — written `(A, B)`, built `(a, b)` against the tuple
      expected, read and assigned by `.0`, shown `(A, B)`, inferred through generic calls, compared
      element by element; tuple patterns and their exhaustiveness (the one constructor, the witness
      `(…)`). A field run is written once all its types are resolved, since resolving one may make a
      tuple's record. `inputs/check/tuples.nz`. `run.sh check`: 2,654 agree, 139 differ; the checker
      carries 2,497, all agree
- [ ] 3.1v Effect parameters (N51): `[effects E]` declared once (N0388 for a second, for one
      without a declared set and on `main`), named in function types written in its function
      and refused inside a type's arguments (N0388); a function type's effects naming its owner's
      parameter (`row`), shown by its name or `*`; a polymorphic callee called as a generic one
      is, each argument's function type binding the parameter to what it has beyond the named
      effects (N0389 for a second set, N0300 where the named ones are missing), the result's
      function types bound, and the call performing the callee's set bound; never assignable by
      inclusion, never a value (N0387); N0366 naming the parameter. `@std/seq`'s programs are
      carried. `run.sh check`: 2,665 agree, 128 differ; the checker carries 2,533, all agree
- [ ] 3.1w Arrays and indexing (Gate 2, `check/operators.rs`, `check/layout.rs`): `[T; N]` written,
      its length a literal or an `Int` constant (N0622, N0618), held to the element and size rules
      after the field types and after the constants (N0621, N0618 by `data_bytes`); an array a
      leaf to every walk over type arguments, as the reference's `type_args` makes it, and stripped
      for what a type contains (equality, crossing into a task, holding a function); `[a, b]` as
      the sequence wanted, a list (R1.5, D6) or an array of the element expected; `[v; n]`;
      `a[i]` and `a[i] = v` with constant indices checked (N0624, N0300); `xs[i]` and
      `xs[i] = v` on a sequence or a map by name (`element_by_name`), and `x.m(…)` as
      `<type>_m(x, …)` (`named_method`, R1 5b.15). `inputs/check/arrays.nz`. `run.sh check`:
      2,677 agree, 116 differ; the checker carries 2,571, all agree (2,678 and 2,572 with the new case)
- [ ] 3.1x `Box`, `Chain` and `Cell` (R1a, R1b) and layouts (`UserTypes::ownership_cycles`):
      the three written (N0352), shown, unified and inferred as a `Vec` is, each with its own kind;
      their generic built-ins; equality through a `Box` or `Chain`, neither ordered nor crossing
      into a task; a `Cell` a mutable holder of a function value, the others not; `for` over a
      `Chain`; the renamed `list_*` built-ins (N0637). The ownership graph ported in place of the
      stop at any cycle: each parameter's uses to a fixed point, edges inline, counted or
      immutable, the impossible layouts (N0336) and then the counted cycles (N0359) with the field
      that closes each, and the nesting bound of declarations (N0338).
      `inputs/check/box-cell.nz`. `run.sh check`: 2,727 agree, 67 differ; the checker carries
      2,633, all agree (2,728 and 2,634 with the new case)
- [ ] 3.1y Contracts (N87, `check_contracts`, `prove_preconditions`): each clause a `Bool` (N0300),
      `ensures` over `result` in a scope of its own, each made only of literals, names, operators,
      field reads, indices and calls of pure functions (N0614, with what it may not contain); a
      call of this module's function whose arguments are all literals evaluated against its
      `requires` clauses as `clauses::literal` does — checked `Int` arithmetic, bit operators and
      shifts — and a false one refuted (N0615). `inputs/check/contracts.nz`. `run.sh check`:
      2,740 agree, 56 differ; the checker carries 2,667, all agree
- [ ] 3.1z The C boundary (N42, N55, N85; `check/foreign.rs`, the C-struct rules of
      `check/layout.rs`): `extern "C" struct`s — opaque handles (one unnamed pointer field) and
      C layouts (N0380, N0384, N0381), never built or taken apart (N0611), with no equality, order
      or crossing into a task; foreign functions — ABI, symbol and the runtime's reserved ones
      (N0380, N0382, N0383), one signature per symbol (N0383), not generic, no effects written,
      no effect parameter (N0384, N0388), what crosses each way (N0381), never a value (N0387), a
      callback an export's name (N0612), the effect `foreign` and a `ForeignCap` at every call
      (N0369); exports (N0391); provenance's summary of a foreign function (`Unknown`, from every
      argument) and its step in the chain. `inputs/check/foreign.nz`,
      `inputs/check/foreign-flows.nz`. `run.sh check`: 2,759 agree, 37 differ; the checker
      carries 2,717, all agree (2,761 and 2,719 with the new cases)
- [ ] 3.1za Constant tables (L8, `constants::evaluate_aggregates`): constants of record, enum,
      tuple and array types — their types resolved in a second phase (one error where one does
      not resolve), held to being values (N0622), each built of constant parts by its type
      (`agg`: constructions and variants by the type's own name, fields each once or all by
      position, tuples, arrays and `[v; n]`, other constants; N0300, N0622, N0003, N0623), a
      cycle refused (N0622), a scalar constant naming one refused (N0622); exported and imported
      by name and as `alias::NAME`; a public one's type naming a private one (N0337); a use in a
      body its type. `inputs/check/const-tables.nz`, `inputs/modules/const-tables.nz`.
      `run.sh check`: 2,771 agree, 27 differ; the checker carries 2,732, all agree (2,774 and
      2,735 with the new cases)
- [ ] 3.1zb `defer` (R1b, L12; `check/stmt.rs`): a body checked as a block whose last line is
      discarded, where the loop depth it begins at is its floor — `return` and `?` anywhere in
      it, and `break` and `continue` at its floor, refused (N0631), a closure in it a function
      of its own; its flows already walked at each exit (provenance). `inputs/check/defer.nz`.
      `run.sh check`: 2,779 agree, 22 differ; the checker carries 2,747, all agree (2,780 and
      2,748 with the new case)
- [ ] 3.1zc Result types from the body (R1.5, D2; `check/infer.rs`): a `pub` function's or a
      method's refused (N0634); the inferred bodies checked first, each after the inferred
      functions it calls by name (`called_names`' walk, ported), a cycle's members refused
      (N0634) and placed where met; each body's value its function's result type, one with none
      refused (N0634); a `return` or `?` in such a body stops the checker.
      `inputs/check/inferred-results.nz`. `run.sh check`: 2,785 agree, 17 differ; the checker
      carries 2,754, all agree (2,786 and 2,755 with the new case)
- [ ] 3.1zd The prelude's names and authority (`type_environment`, `forged`): a module's own
      `Result` or `Option` refused where it is declared (N0363) and still the type its name
      finds, while `?` keeps the prelude's; a capability built or called as a function refused
      (N0370), its arguments still checked. `inputs/check/prelude-names.nz`. `run.sh check`:
      2,793 agree, 10 differ; the checker carries 2,762, all agree (2,794 and 2,763 with the
      new case)
- [ ] 3.1ze Packages (N46; `root::decide`, `package_context`, `nazm-package`'s `Manifest::parse` and
      `resolve`, `load_packaged`; `compiler/package.nz`, D22): the source root found upward from
      the root module — the first manifest whose source root holds it — a manifest read as the
      reference reads it (an invalid one publishes nothing), the root package and its path
      dependencies resolved in name order (cycles, diamonds, names and versions), `NAME:path` to
      a declared dependency's module (N0508 otherwise, with what is declared), a relative import
      held inside its package's source root (N0205), and a dependency's modules — and any module
      outside the source root — not the program's (no `main`'s output authority, no prelude
      modules given). Registry dependencies, features, profiles, `heap-limit` and markers stop.
      `inputs/packages/`. `run.sh check`: 2,802 agree, 3 differ; the checker carries 2,794, all
      agree (2,812 and 2,804 with the new cases)
- [ ] 3.1zf Map literals and `for` over `Bytes`: the calls the parser writes for a map literal
      (`hashmap_single`, `hashmap_set`) found by `called_names` (now `syntax.nz`'s
      `sx_called_names`) and given to `prelude_named`, so `@std/hashmap` is the module's as in the
      reference, and checked as written; a `for` over a `Bytes` binds each byte as a `UInt8`, its
      flows read through the `Bytes` cell. `inputs/check/map-literals.nz`,
      `inputs/check/bytes-flows.nz`. `run.sh check`: 2,813 agree, 2 differ; the checker carries
      2,807, all agree (2,815 and 2,809 with the new cases)
- [ ] 3.1zg A trap fixed (found by R5 on `toolchain/interp`): provenance's walk read a tuple
      pattern's `alts`, which only an or-pattern has (`w_bind`), and trapped on the missing field
      wherever a compilation both reads a restricted origin and writes to a path. A root module
      that is not UTF-8 is refused by the loader as the reference refuses it (N0205, "stream did
      not contain valid UTF-8"). Every Nazm source under `toolchain/` is now an input of every
      comparison, so the other tracks' programs keep the checker honest; the lexer and parser
      comparisons skip a file that is not UTF-8 (the reference lexes text) and say so.
      `inputs/check/trap-tuple-let.nz`. `run.sh check`: 2,958 agree, 2 differ; the checker
      carries 2,952, all agree (tokens, tree, syntax: 2,959 agree, 0 differ, 1 skipped)
- [ ] 3.1zh Re-exports (N103, D23): `pub use` of every name or of the names chosen, `as` for
      functions and types, chains of re-exports applied in order, a cycle refused (N0213), a name
      not offered (N0211, a trait renamed included) or offered twice (N0212); importers see them
      plainly, as `alias::name` and `alias::T`, in collisions (N0207, N0208), as a renamed enum's
      variants, and an impl of a re-exported module usable. `inputs/modules/reexport-*.nz`.
      `run.sh check`: 2,977 agree, 2 differ; the checker carries 2,973, all agree (with the 19 new
      inputs). Only `@derive` stops the checker now (6 inputs), left for last.
- [ ] 3.1zi The facts `nazm doc` reads (D24; R5 D10's seven items): `compiler/facts.nz` — the whole
      front end as one call, the checker's tables, durable identities, required effects, spans and
      attributes, `pub use` resolutions, whether the compilation is carried, its diagnostics and
      their text over every file (`dg_render`). A manifest's unknown field worded as serde words it
      (`inputs/packages/unknowntop`, `unknowndep`). `run.sh`: check 2,981 agree, 2 differ (the
      checker carries 2,977, all agree); render 2,981 agree, 2 differ (carried 2,977, all agree);
      facts 2,977 agree, 6 differ (carried 2,977, all agree — 5,619 functions, 516 records, 121
      enums and 20 re-exports documented by the reference among them); tokens, tree, syntax 2,982
      agree, 0 differ
- [ ] 3.1zj R1.5 4.5 and 4.6 followed: `@std/option` and `@std/result` among the prelude's standard
      modules, in `stdlib::PRELUDE`'s order (hash, hashmap, option, ordmap, result, sort, text), and
      an interpolation's holes scanned for the names and methods they hold (`named_in`); a line
      break separates two `match` arms as it ends a statement. `inputs/check/prelude-option-*.nz`,
      `inputs/check/prelude-hole-only.nz`, `inputs/parse/match-arms-*.nz`. `run.sh`: tokens, tree
      3,045 agree, 0 differ (1 not UTF-8, skipped), syntax likewise; check 3,044 agree, 2 differ
      (the checker carries 3,040, all agree); render likewise; facts 3,040 agree, 6 differ (carried
      3,040, all agree)
- [ ] 3.2 Every type of group C resolved and typed
- [ ] 3.3 Generics, traits, impls, default methods, closures and effects

## 4. The checker's rules

- [ ] 4.1 Group by group, every diagnostic identical as `nazm check --json` (D4), with recovery
- [x] 4.2 The text rendering compared — `run.sh render`, every carried input (3.1zi)
- [ ] 4.3 Built-ins of group F; `selfhost.rs`'s `NOT_CARRIED` emptied (asked of the coordinator)

## 5. Core IR

- [ ] 5.1 `nazm.core-ir/3` identical with `nazm core-ir` on every accepted input (D5, D25)
- [ ] 5.1a The lowering's first constructs (D25, `compiler/core.nz`), compared function by function
      without digests (`run.sh core`): functions with a body and no type or effect parameters,
      contracts or attributes but `@deprecated`; `let` of a name, assignment to a binding,
      `while`, `return`, `break`, `continue`, expression statements, `if` and blocks; literals of
      `Int`, `Bool` and `Str`, bindings, `-`, `!`, `&&`, `||`, arithmetic and comparisons of
      `Int`, `Bool` and `Str`, field reads, calls of the compilation's functions and of the
      built-ins that are not generic. Every other construct stops the function, named.
      `run.sh core`: 2,984 inputs agree, 0 differ; of 41,353 functions the lowering carries
      22,595, all agree (the most frequent stops: a variant, a construction, a generic function,
      an impl's method, `match`)
- [ ] 5.1b The digests (`digest.rs`: the canonical encoding, BLAKE3): `compiler/digest.nz`, from
      each function's printed object (D25), BLAKE3 by `@std/blake3`; `run.sh core` compares each
      function with its `digest`. `run.sh core`: 3,047 inputs agree, 0 differ; of 56,435
      functions the lowering carries 29,774, all agree, digests included
- [ ] 5.1c The document's other parts: modules, the type table in the reference's order, statics,
      the impl table, `derived_hash`; then the whole document compared
- [ ] 5.2 Per-module output; the performance budgets measured (`rewrite-in-nazm` 3.3)

## 6. R1.5's forms (after the coordinator declares them final)

- [x] 6.0 Parsed as main's reference parses them (D1–D5): `:` result types, `= e` bodies and
      `inferred_result`, `|a, b|` closures, map literals, fields and payloads by position, variants
      without their enum, `use std.json`
- [ ] 6.1 Checked: R1.5's forms in the checker; D6–D8 once final
