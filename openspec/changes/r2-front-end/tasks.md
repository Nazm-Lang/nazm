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
- [x] 5.1c The document's other parts: modules, the type table in the reference's order, statics,
      the impl table, `derived_hash`; then the whole document compared (5.1e): the entry, the
      modules, the type table (instances completed as made, the reference's interning order), the
      statics (`record_module_names`: module by module, each in declaration order, a value or an
      `Atomic`'s cell and its start, its `@section`; a function's `@section` likewise) and the impl
      table and `derived_hash`, each a line of `run.sh core` (D25). `run.sh core`: 3,048 inputs
      agree, 0 differ; of 56,727 functions the lowering carries 43,510, all agree; of the
      document's other parts 10,374 carried — every one — all agree; 944 whole documents, all the
      reference's bytes
- [ ] 5.1d The first stops (D25): variants and records built (by name, by position, `Dot` alone),
      generic functions and generic calls with their settled type arguments (written or not),
      impls' methods, and `match` of plain arms as a value and as a statement. `run.sh core`:
      3,047 inputs agree, 0 differ; of 56,681 functions the lowering carries 43,473, all agree;
      of the document's other parts 10,349 carried, all agree. check 3,045 agree, 2 differ
      (carried 3,041, all agree), render likewise, facts 3,041 agree, 6 differ (carried 3,041,
      all agree)
- [ ] 5.1e The whole document, byte for byte, wherever every function and part is carried:
      `cr_core` prints it as `print.rs` does (`cdg_pretty`), and `run.sh core` compares it with
      `nazm core-ir`'s output. `run.sh core`: 3,047 inputs agree, 0 differ; of 56,701 functions
      the lowering carries 43,490, all agree; of the other parts 10,349 carried, all agree; 944
      whole documents carried, all 944 the reference's bytes
- [ ] 5.1f The remaining stops, in the order of their frequency (D25), one group a commit:
  - [x] `for` (`for_loop`, `split_loop`): over a range, a `Vec`, a `Chain`, a `Bytes`, and
        `@std/text`'s `text_split` without the list, its hidden locals (`temp i`, `seq`, `end`)
        made as the reference makes them. `run.sh core`: 3,048 inputs agree, 0 differ; of 56,772
        functions the lowering carries 44,910, all agree; the other parts 10,374, all agree; 949
        whole documents, all the reference's bytes
  - [x] `?` (`propagate`): a `match` of origin `propagate` on a `Result` or an `Option`, its
        success's payload held in `temp ok`, its failure's in `temp err` and returned rebuilt in
        the function's own type. `run.sh core`: 3,048 agree, 0 differ; of 56,791 functions the
        lowering carries 45,712, all agree; the other parts 10,374, all agree; 964 whole
        documents, all the reference's bytes
  - [x] Operators and numbers: arithmetic, `-` and comparisons of every type that has them (the
        law by type: `int`, `num`, `bool`, `bytes`, `structural`, `parametric`), the bit
        operators, integer literals of every integer type (their 128-bit two's complement in the
        digests) and a negated one as one constant, `s + t` as `str_concat` and an append as
        `str_append` (`appends_to`). The checker now checks `s + t`'s operands a second time, as
        the reference checks them again as `str_concat`'s arguments (what they declare is
        declared twice, as there), and records a negated literal's type. `run.sh core`: 3,048
        agree, 0 differ; of 56,805 functions the lowering carries 48,939, all agree; the other
        parts 10,374, all agree; 1,022 whole documents, all the reference's bytes. check 3,046
        agree, 2 differ (carried 3,042, all agree), render likewise, facts 3,042 agree, 6 differ
        (carried 3,042, all agree)
  - [x] Interpolation (`interpolation`, `hole_text`): each piece joined left to right by
        `str_concat`, an integer's unpadded digits by `str_concat_num`; a hole's text by its type
        (`Str` itself, `Bool` by an `if`, a number by `num_to_str`), padded by `str_pad_start` or
        `str_pad_end`. Not yet: a hole read through `show()`, a float's with a precision. `run.sh
        core`: 3,048 agree, 0 differ; of 56,811 functions the lowering carries 50,980, all agree;
        the other parts 10,374, all agree; 1,126 whole documents, all the reference's bytes
  - [x] General `match` patterns (`matching_general`, `pattern_test`, `extract`, `guarded`):
        literals, or-patterns, tuples, nested variants and guards, as a value and as a
        statement — the scrutinee held in `temp scrutinee`, the arm chosen in `temp arm`, each
        tested payload in `temp payload`, a branch exhaustiveness rules out ending the process
        (`exit_with(70)`). digest.nz now reads a tuple's type (its record is named `(,)`).
        `run.sh core`: 3,048 agree, 0 differ; of 56,861 functions the lowering carries 51,235,
        all agree; the other parts 10,374, all agree; 1,133 whole documents, all the
        reference's bytes
  - [x] Constants and conversions: a constant's use is its value written as a literal of its type
        (`const_value`; a record's, enum's, tuple's or array's, and a float's, still stop), a
        static `Atomic`'s its cell (`Const::Cell`, with its `static` key); `T(x)`,
        `wrap_convert` and `saturate_convert` as `convert`, `try_convert` as `{ let c = x;
        if fits(c) { Some(T(c)) } else { None } }` (a conversion to `Text` still stops). The
        checker records each constant's use with its value, and each conversion. `run.sh core`:
        3,048 agree, 0 differ; of 56,919 functions the lowering carries 53,517, all agree; the
        other parts 10,374, all agree; 1,164 whole documents, all the reference's bytes. check,
        render and facts as before
  - [x] Attributes and `scope`: every attribute is carried (`@test`, `@bench`, `@fuzz`,
        `@cfg` and the rest change nothing Core IR prints); `@interrupt`, `@task` (`task_of`) and
        `@on_failure` are printed in the function's attributes; a `scope` block as `scope`.
        `run.sh core`: 3,048 agree, 0 differ; of 56,929 functions the lowering carries 53,923,
        all agree; the other parts 10,374, all agree; 1,184 whole documents, all the
        reference's bytes
  - [x] Method calls (`method_call`): by name, the call the checker read (the receiver first;
        `x.m` without parentheses too); an impl's method, or a trait's default body at the
        receiver's type, as a call of it; a receiver naming no value is no argument. A call
        through a trait's bound (`CallTrait`) still stops. `run.sh core`: 3,048 agree, 0 differ;
        of 56,937 functions the lowering carries 53,959, all agree; the other parts 10,374, all
        agree; 1,195 whole documents, all the reference's bytes
  - [x] String constants of any text (`text`, then `quote`): UTF-8 printed as it is, its
        control characters `\u00XX`; text that is not UTF-8 or holds `\x`, `\xNN` per byte.
        `run.sh core`: 3,048 agree, 0 differ; of 56,942 functions the lowering carries 54,341,
        all agree; the other parts 10,374, all agree; 1,223 whole documents, all the
        reference's bytes
  - [x] Indexing, tuples and wide literals: `s[i]` on a sequence as the call the checker read it
        as (`element_call`), on an array as `index`; `(a, b, …)` as its tuple record; an integer
        literal past `Int`'s digits (`WideInt`) as any other. `run.sh core`: 3,048 agree, 0
        differ; of 56,956 functions the lowering carries 54,798, all agree; the other parts
        10,374, all agree; 1,228 whole documents, all the reference's bytes
  - [x] Calls through a trait's bound and tasks: a method call the checker reached through a
        bound, or a derived method, as `call_trait` (the trait's method, the receiver's type as
        the body sees it, the result at that type); `spawn` as the task it starts. `run.sh
        core`: 3,048 agree, 0 differ; of 56,978 functions the lowering carries 55,841, all
        agree; the other parts 10,374, all agree; 1,447 whole documents, all the reference's
        bytes. Remaining stops, most frequent first: closures and the bindings they share,
        effect parameters, float literals, `[v; n]` and array literals, an assignment to a
        field or an element, aggregate constants, calls through a value, foreign functions,
        contracts, `defer`, the built-ins `box`/`cell`/`checked`/`chain`; `@derive` last
  - [x] Closures and the bindings they share (`closure`, `shared`, `through_cell`): a closure
        as a function of its own — `env` first, its parameters, then its copy of each binding it
        captures where the checker made the copy (`synthetic`, among its own bindings), owned
        by its body first; `type_params` its maker's; the closure expression with the maker's
        locals it captures. A `let mut` a closure shares is a `Cell` made holding its value
        (`made_holding`), read by `vec_get` and assigned by `vec_set` at 0. sema.nz records
        where each capture's copy is made (`capat`), each closure's type, a closure
        parameter's declaration and a closure's settled effects. `run.sh core` (reference
        rebuilt at 5dd044c4): 3,094 inputs agree, 0 differ; of 76,929 functions the lowering
        carries 75,937, all agree; the other parts 10,662, all agree; 1,488 whole documents,
        all the reference's bytes. check 3,092 agree, 2 differ (carried 3,088, all agree);
        facts 3,088 agree, 6 differ (carried 3,088, all agree). Remaining stops, most frequent
        first: effect parameters
        (176, and 129 calls of an effect-polymorphic function), float literals (130), an
        assignment to a field or an element (112), calls through a value (86), `[v; n]` (66),
        a function used as a value (65), aggregate constants (51), foreign functions (49, 15
        calls), array literals (24), contracts (23), `defer` (12), `~` (10), the built-ins
        `checked_*`/`box_new`/`cell_*`/`chain_from_vec` (35)
  - [x] The reference's checker followed (main's aa9f22e9 and c02d12e5, epoch 88): a desugared
        call no longer checks its operands twice — `s + t` on `Str`s checks them once, and
        `x.m()` by name and `xs[i]` hand the call the receiver or sequence checked already
        (`check_again`, `take_checked` by span); a pattern `let`'s arm and the rest of its
        block are joined to the scope the `let` is written in (`open_joined_scope`), so a name
        bound there already is N0202, "already defined in this scope", once per name.
        `inputs/check/pattern-let-rebinds.nz`. Reference rebuilt at f90693fb. `run.sh`: core
        3,095 agree, 0 differ; of 76,198 functions the lowering carries 75,206, all agree (a
        closure in `s + t` is now declared once, as there); the other parts 10,644, all agree;
        1,485 whole documents, all the reference's bytes. check 3,093 agree, 2 differ (carried
        3,089, all agree), render likewise, facts 3,089 agree, 6 differ (carried 3,089, all
        agree)
  - [x] Effect parameters (N51): a function with an effect parameter, and a call of one, as
        any other — its effect sets name the parameter `*`, a function type's `$effects`, and
        a call binds it without a type argument. `run.sh core`: 3,095 agree, 0 differ; of
        76,198 functions the lowering carries 75,363, all agree; the other parts 10,644, all
        agree; 1,485 whole documents, all the reference's bytes (most of these functions call
        through a value, the next group)
  - [x] Float literals (`Const::Num`): of the type the checker recorded (`Float32` where one
        is expected, `Float64` else), the literal read as the nearest value of that type, ties
        to even, and printed as its shortest digits that read back (`numeric::to_str`: the
        first precision whose correctly rounded `{:.p$e}` parses back, laid out as ECMAScript
        does); its digest the bits, read back from those digits. Both exactly, without a float
        type (`digest.nz`: decimal digits in `Ints`), checked against Python's own on 1,800
        random literals of both widths. `run.sh core`: 3,095 agree, 0 differ; of 76,240
        functions the lowering carries 75,530, all agree; the other parts 10,644, all agree;
        1,514 whole documents, all the reference's bytes. check 3,093 agree, 2 differ (carried
        3,089, all agree)
  - [x] `[v; n]` and list and array literals: `[a, b, …]` as an array (`ExprKind::Array`) where
        the checker made one, else as the list it read it as (`sequence_literal`: `{ let made
        = vec_new(); vec_push(made, a); …; made }`); `[v; n]` as `array-repeat` of the type's
        length. sema.nz records the type each literal was made as. And a `for` over
        `s.split(sep)` written as a method (by name) is the split loop, as one over
        `text_split(s, sep)` is — newly reached, and found, here. `run.sh core`: 3,095 agree,
        0 differ; of 76,272 functions the lowering carries 75,581, all agree; the other parts
        10,644, all agree; 1,524 whole documents, all the reference's bytes
  - [x] Assignment to a field or an element, and `~`: the value first, then each step — a
        field by the checker's record of it (its record the type reached so far), an element of
        an array with its length; `xs[i] = v` on a sequence or a map the call the checker read
        it as; on a binding a closure shares, its value copied out of the cell, changed and set
        back. A compound update's read of a map's absent entry passes the value type's zero
        (`map_fallback`; sema.nz records the literal made for it). `~x` as `complement`.
        `run.sh core`: 3,095 agree, 0 differ; of 76,284 functions the lowering carries 75,769,
        all agree; the other parts 10,644, all agree; 1,562 whole documents, all the
        reference's bytes
  - [x] Aggregate constants (`const_value`): a constant of a record, enum, tuple or array type
        as the value it is built of, every part at the name — fields in declared order, a
        `[v; n]` its element `n` times. sema.nz's evaluator now keeps that value (`parts`, one
        token a part, as the constant's text, exported with it), and a float literal's text,
        negated or not, so a `Float32` or `Float64` constant is its value printed as
        `numeric::to_str` prints it. `run.sh`: core 3,095 agree, 0 differ; of 76,328 functions
        the lowering carries 75,882, all agree; the other parts 10,644, all agree; 1,578 whole
        documents, all the reference's bytes. check 3,093 agree, 2 differ (carried 3,089, all
        agree); facts 3,089 agree, 6 differ (carried 3,089, all agree)
  - [x] Calls through a value and functions as values (N50): a call through a binding of
        function type as `call_indirect` of the binding, of the type the checker reached; a
        named function used as a value as `fn_value`, of the type it was given (sema.nz now
        keeps it). A call of an effect-polymorphic function has the result the checker formed,
        its effect parameter bound. `core-ir.py` now quotes the reference's side as `print.rs`
        does (`\b` and `\f` as `\u0008`, `\u000c`, not Python's short forms). Rebased on main
        e7effe0e, reference rebuilt there. `run.sh`: core 3,153 agree, 0 differ; of 93,517
        functions the lowering carries 93,369, all agree; the other parts 10,992, all agree;
        1,749 whole documents, all the reference's bytes. check 3,151 agree, 2 differ (carried
        3,147, all agree); facts 3,147 agree, 6 differ (carried 3,147, all agree)
  - [x] The built-ins made of operations a program cannot name: `box_new` and `cell_new` as
        `{ let made = box_fresh() | vec_new(); box_fill | vec_push(made, v); made }`
        (`made_holding`), `cell_get` and `cell_set` as `vec_get` and `vec_set` at 0,
        `checked_add`/`_sub` by wrapping and saturating and `checked_mul` by a wrapping product
        divided back (`checked`), `chain_from_vec` as its copying loop. `run.sh core`: 3,153
        agree, 0 differ; of 93,537 functions the lowering carries 93,424, all agree; the other
        parts 10,992, all agree; 1,768 whole documents, all the reference's bytes
  - [x] `Text` and the last holes: a string literal where a `Text` is expected as
        `text_assume` of its bytes; `Text(s)` as `text_from_str`, `try_convert[Text](s)` as
        `utf8_valid` and `text_assume` (each at its argument, as `internal` places it); a hole
        read through `show()` as the method call the checker recorded; a float's hole with a
        precision as `float_to_str_fixed`. `run.sh core`: 3,153 agree, 0 differ; of 93,541
        functions the lowering carries 93,436, all agree; the other parts 10,992, all agree;
        1,776 whole documents, all the reference's bytes
  - [x] Foreign functions and calls (N42, N85): a foreign function with no body, only its C
        symbol (`foreign`), its parameters and result as C has them (`raw_foreign`: a handle or
        a callback an `Int`, a result crossing as a pointer an `Int`), its digest's body the
        empty block that transfers; a foreign call as C sees it (`foreign_call`): a handle
        argument its pointer, a callback `callback_address` of the export's symbol, and the
        result checked against null (`foreign_nonnull`), copied (`cstr_copy`), or tested for
        null into an `Option`. `run.sh core`: 3,153 agree, 0 differ; of 93,575 functions the
        lowering carries 93,536, all agree; the other parts 10,992, all agree; 1,799 whole
        documents, all the reference's bytes
  - [x] Contracts and `defer` (N87, L12), one commit since both leave through `leave`: every
        `requires` checked first (`contract_check(cond, 0)`), every `ensures` where the
        function returns — its tail held in `result` (`ensured`), each `return` and `?` —
        with `result` the held value; a `defer` registered where written (an empty block) and
        run, last first, where its block is left: after a value block's tail (held in
        `result`), as a statement block falls through, before a `break` or `continue` leaving
        the loops it was registered in, and at every return. sema.nz makes `result` one
        binding of the function's own at the first `ensures`, at its keyword, as the reference
        allocates it. `run.sh`: core 3,153 agree, 0 differ; of 93,603 functions the lowering
        carries 93,599, all agree; the other parts 10,992, all agree; 1,828 whole documents,
        all the reference's bytes. check 3,151 agree, 2 differ (carried 3,147, all agree),
        render likewise, facts 3,147 agree, 6 differ (carried 3,147, all agree)
  - [x] The last stops — methods of a trait's default body and of a generic impl: a default
        body (no impl's method) is called as a method at the receiver's type, the first
        argument's where it is called by its trait's name (`Trait.m(x)`); a generic impl's
        method takes the receiver type's arguments as its type arguments, where the checker
        did not record them on the call. `run.sh core`: 3,153 agree, 0 differ; of 93,603
        functions the lowering carries every one, all agree; the other parts 10,992, all
        agree; 1,832 whole documents, all the reference's bytes. Nothing is stopped: `@derive`
        needed nothing of its own (derived methods are reached through `call_trait`)
- [ ] 5.2 Per-module output; the performance budgets measured (`rewrite-in-nazm` 3.3)
  - [x] The budgets measured and set (D26): `check` 7–9 times the reference's time in about its
        memory, Core IR 13–15 times and 4–5 times; gates at 10× / 1.5× and 16× / 5×. Core IR
        now reads each function's bindings and captures from an index the checker makes once
        (`ck_fn_binding`, `ck_fn_capture`) rather than walking every binding per function.
        `run.sh core` after it: 3,241 agree, 0 differ; of 96,561 functions every one carried,
        all agree; the other parts 11,310, all agree; 1,885 whole documents, all the
        reference's bytes
  - [ ] Per-module output: what it is to be is not written down anywhere (`rewrite-in-nazm`
        3.3 names it only) — asked of the coordinator

## 6. R1.5's forms (after the coordinator declares them final)

- [x] 6.0 Parsed as main's reference parses them (D1–D5): `:` result types, `= e` bodies and
      `inferred_result`, `|a, b|` closures, map literals, fields and payloads by position, variants
      without their enum, `use std.json`
- [x] 6.1 Checked: R1.5's forms in the checker (D1–D10, final on main), against the reference
      on every input: `inputs/check/r15-forms.nz` (each form once where accepted) and
      `r15-refusals.nz` (N0634 three ways, N0635, N0636, N0637), and the programs of main's
      tests re-extracted (`extract-inputs.py` writes `toolchain/frontend-tests/inputs/rs` again:
      2,392 programs). The tree dump now escapes the space separators but the ASCII space
      (`\u{a0}`, `\u{3000}`, …) as Rust's `Debug` does — a regex test's literal showed it.
      `run.sh` on main dfda47ac (reference rebuilt at 91f3da0a, no Rust change since): tokens,
      tree, syntax 3,240 agree, 0 differ (1 not UTF-8, skipped); check 3,240 agree, 1 differ
      (the checker carries 3,235, all agree), render likewise; facts 3,235 agree, 6 differ
      (carried 3,235, all agree); core 3,241 agree, 0 differ, of 96,495 functions every one
      carried and agreeing, the other parts 11,310, all agree, 1,885 whole documents, all the
      reference's bytes. What the checker does not carry is `@derive`'s encodings (`Json`,
      `Toml`: "the checker stops at a derived encoding", 6 inputs) — every difference above

## 7. What R5's commands ask of the front end (handed over by the coordinator)

- [x] 7.1 Epoch 88: a name re-bound by a pattern `let` in the same scope is N0202 (5.1f's
      follow-up of main's checker, `open_joined_scope`)
- [x] 7.2 A source root given (`check --source-root`): `fx_check_in` and `fx_check_module_in`
      take it, `pk_given` decides it as `root::decide` does with one — a directory, an ancestor
      of the root module, the modules identified relative to it. Ancestry is found lexically
      (no `realpath` here): a root reached through a link, or one the module is not under as
      written, is not decided (the front end gives up rather than refuse what the reference's
      `canonicalize` might accept); a path that is not a directory is refused with the
      reference's words. Compared by hand with `nazm check --source-root` on a root above the
      module, its parent's parent, one that does not exist and one the module is not under
- [x] 7.3 Loading from editor buffers, for `lsp`'s unsaved documents: `fx_check_over` (and
      `ld_load_over`) take paths and texts that stand for those files, read, found by imports
      and parsed instead of the disk's (a standard module is always the library's); `ld_load`
      is the same with none
- [x] 7.4 Any type shown as messages show it (`ck_show`, `fx_show`), and the type of the
      binding a name at a place is (`fx_binding_type_at`), for `lsp` hover
- [x] 7.5 Where a name that is no local is declared (`fx_declaration_at`, for `lsp`
      definition): a function called or used as a value, a record built, an enum's variant
      named — its declaration's module and name span; the declarations' name spans are public
      (`ck_fn_name_start`, `ck_record_name_start`, `ck_enum_name_start`, `ck_trait_name_start`
      and their ends). A type written in a signature is not recorded by place, so not yet:
      done since — see 7.7.
      7.3–7.5 tried by hand on two unsaved modules (an import, a call, a construction, a
      binding's type). `run.sh` after them: core 3,153 agree, 0 differ; of 93,807 functions the
      lowering carries every one, all agree; the other parts 10,992, all agree; 1,832 whole
      documents, all the reference's bytes; check 3,151 agree, 2 differ (carried 3,147, all
      agree), render likewise, facts 3,147 agree, 6 differ (carried 3,147, all agree)
- [x] 7.6 C1's TLS (main's 98e271e0, epoch 89): the eleven `os_tls_*` built-ins in
      `compiler/builtins.nz` (regenerated by `builtins.sh` from `Intrinsic`), and the engine's
      ten `nz_tls_*` C symbols reserved (N0383). `inputs/check/tls-builtins.nz` (a reserved
      symbol declared) and `tls-builtins-run.nz` (`@std/tls` and `os_tls_info` lowered).
      Reference rebuilt at 98e271e0. `run.sh`: core 3,156 agree, 0 differ; of 95,595
      functions the lowering carries every one, all agree; the other parts 11,004, all agree;
      1,834 whole documents, all the reference's bytes; check 3,154 agree, 2 differ (carried
      3,150, all agree), render likewise, facts 3,150 agree, 6 differ (carried 3,150, all
      agree). `selfhost.rs`'s `NOT_CARRIED` still lists them: that test reads
      `compiler/analyse.nz` (the bootstrap checker), which has no `Bytes`, not `sema.nz`
- [x] 7.7 Type names written anywhere — a signature, an annotation, a construction's type — for
      `lsp` definition: sema.nz records the record or enum each name resolves to by its span
      (`ck_type_name_at`) and `fx_declaration_at` answers it with its declaration (signatures
      unchanged). Tried by hand on an unsaved module naming an imported record and enum in a
      signature, an annotation and a construction. `run.sh` after it: check 3,240 agree, 1
      differ (carried 3,235, all agree); facts 3,235 agree, 6 differ (carried 3,235, all agree);
      core 3,241 agree, 0 differ, every one of 96,572 functions carried and agreeing, the other
      parts 11,310, all agree, 1,885 whole documents, all the reference's bytes
- [x] 7.8 `@derive(json)` and `@derive(toml)` (D27): the expansion ported (`compiler/derive.nz`,
      applied by the loader to every project file), its refusals in `configure.nz`, and the
      checker's stop at a derived encoding removed. `inputs/check/derive-refusals.nz` holds the
      refusals. `run.sh` on 5b132e5e (reference rebuilt there): tokens, tree, syntax 3,246
      agree, 0 differ (1 not UTF-8, skipped); check 3,247 agree, 0 differ (the checker carries
      every one), render likewise, facts likewise; core 3,247 agree, 0 differ, every one of
      105,232 functions carried and agreeing, the other parts 11,370, all agree, 1,895 whole
      documents, all the reference's bytes. Nothing differs and nothing is stopped
