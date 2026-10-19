# The core-scope audit — N107, 2026-10-05

`architecture.md` §7.108 is the method. Every language-core row that was PARTIAL, every candidate
gap it named, and the decision. *Required* means a program the repository or a stated goal
(`NAZM_LANGUAGE_GOALS.md`) needs and cannot write, or an unsound answer; anything else is another
language's mechanism, and the row stops waiting for it. Every absent construct stays refused **by
name** — narrowing a promise removes no refusal.

Each candidate was first checked for being a defect. One was looked at closely: area 3's *a
persisted interface's traits are not rehydrated*. A module that imports a trait and its impl from
a module the check cache reused (`nazm check`, edit the importer, `nazm check` again, `nazm run`)
checks and runs: the cache path reads the defining module's source facts, and only a check against
an interface **alone** (no source) cannot use traits — a tooling boundary of area 26, not a wrong
answer, and stated there.

| area | root problem | implemented mechanism | candidate gap | required? | reason | final scope / status |
| --- | --- | --- | --- | --- | --- | --- |
| 2 | One meaning per name, across modules and packages | durable identities, persisted interfaces and resolved units (N77), a declared root, `@std`, packages, re-exports with one identity (N103), both compilers | `nazm build` and `nazm run` re-check every module | no | a cost, not a meaning: every answer is the same; warm builds already reuse objects | **VERIFIED** — resolution and modules; check reuse for builds moves to area 26's limitation |
| 3 | Typed programs that say what they mean, polymorphically | records, closed enums, exhaustive match, first-order generics, `Vec[T]`, typed `Result` and `?`, structural equality and equality bounds, function values and closures (N50), traits, impls, methods and trait bounds with static dispatch (N78) | trait objects / dynamic dispatch | no | function values, closures and closed enums already dispatch at run time; the goals ask that dynamic dispatch be *exposed and restrictable* (G-performance), not added; a vtable is a second dispatch model in a small kernel | **VERIFIED as scoped**: the type system as specified; `N0607` keeps refusing trait objects |
| 3 | | | generic traits, associated types, default methods, supertraits, impls for generic types | no | no program in the repository or the stdlib needs one (the N78 dogfood and `@std/show`); each is a convenience whose absence is refused by name | not here, refused by name (`N0602`–`N0608`) |
| 3 | | | ordering, hashing, custom equality on user types; richer patterns | no | derived equality covers comparison; a program writes an ordering as a function; no use case is blocked | not here |
| 5 | Knowing what a function can do to the world | two compiler-owned effects (`io`, `spawn`), inferred to the least fixed point, declared sets as contracts, purity assertions, effect-bearing interfaces (N36), subsumption for function values (N79) | effect handlers, user-defined effects, effect polymorphism | no | handlers are algebraic effects (Koka, OCaml 5) — a control mechanism, not a description; *who may act* is capabilities' (area 6), so a user-defined effect would be a second authority system | **VERIFIED as scoped**: inferred, checked compiler-owned effects |
| 5 | | | divergence as an effect | open, not a defect | `pure` has never promised termination, and no answer is wrong today; whether divergence is an effect blocks one optimisation only (dropping an unused pure call), which nothing performs | stated: *pure is not total*; the question stays in `research-register.md` (open questions) |
| 6 | No authority without being handed it | capabilities as parameters, `main` the only root, no inheritance (N104), `OutCap` attenuation (N79), the `authority` profile, both compilers | runtime enforcement, revocation, finer kinds (a path, a host), linear (unshareable) capabilities | no | the promise is static and is checked; finer kinds are library authority built from today's kinds, not a language change; revocation and linearity have no use case here (§7.53) | **VERIFIED as scoped**: static, coarse, shareable capabilities, said so in the row's name |
| 7 | A secret reaching an output | explicit data flow by cell, target and field, through calls, values, bounds and channels (N80), `N0372` and the profile rules as one engine, traces | implicit flow (non-interference) | no — and not claimed | implicit flow is RESEARCH (`research-register.md` R6): its known costs are an unusable false-positive rate or annotations; the promise never said non-interference | **VERIFIED as scoped**: explicit data flow; implicit flow RESEARCH |
| 7 | | | labels a program writes; precision by handle, index or across a call | no | the classes are the compiler's (`Secret`, sinks); per-handle precision would only remove false refusals, never admit a wrong flow | not here |

**Not audited here** — ecosystem and domain rows, classified by N108: 21 (AI/HPC), 25 (LSP, MCP),
26 (packages and build), 27 (debugger, profiler), 28 (performance), 30's fuzzing half, 32
(reproducibility), 33 (platforms), 34 (contracts).

**After N107:** twenty-nine VERIFIED (2, 3, 5, 6 and 7 moved, the last four as scoped), eight
PARTIAL — all of them ecosystem or domain rows — none RESEARCH or MISSING.
