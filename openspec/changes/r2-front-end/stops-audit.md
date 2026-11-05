# R2: the remaining stops of the Nazm-written compiler (audit 2026-11-05)

A stop is a construct the compiler written in Nazm refuses (the checker says what it stopped at and
reports only the missing `main`; the lowering prints `- REASON` for a function). This is the
inventory of every stop site in `compiler/*.nz` and of the parity record, with what the released set
needs. Nothing here was run: it is read from the sources and from the evidence recorded in the task
lists (see *What I could not verify*).

## Evidence that the released set hits none of them

- `r2-front-end` 7.8 and 7.9 (reference 5b132e5e rebuilt, now dcabce7a): `check` 3,247 inputs agree, 0 differ,
  *the checker carries every one* (the inputs include every Nazm source under `toolchain/`, 3.1zg);
  `core`: every one of 105,232 functions carried, 1,895 whole documents byte-identical. 5.1f: "Nothing is stopped".
- `r8-cut-over` 16.3 (aarch64) and 16.4 (x86_64 under Rosetta), 2026-11-04/05: the nine programs of
  `toolchain/selfbuild/programs.txt` are built by Nazm to a stage 1-3 fixpoint, and their Core IR from
  the Nazm front end (`fe` = `compiler/diagnose.nz`, which runs `sema.nz`) is identical to the seed's, 9/9.
  A construct any of the nine hits and the front end stopped on would have failed that check.
- `compiler/parity.json` (test `the_parity_record_is_what_both_compilers_do_with_each_feature`,
  `crates/nazm-cli/tests/selfhost.rs:5207`): 21 probes, 21 equal, 0 refused, 0 differs. It exercises the
  bootstrap chain (`compiler/emit.nz`), not `sema.nz` (the record's own header says so).

## Stop sites in the checker `compiler/sema.nz`

| # | Construct | Where | Needed by the released set? | Verdict |
| --- | --- | --- | --- | --- |
| 1 | an effect parameter of a method | sema.nz:2502 | not hit (corpus carried) | does not block 0.11.0: language feature outside the set; after 0.11.0 before 1.0 |
| 2 | an effect parameter in `declared_type_params` (a type-parameter list carrying the effect flag) | sema.nz:2596 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 (could not tell which declarations reach it) |
| 3 | a function without a body that is not foreign, in a compilation without a syntax error | sema.nz:2504 | not hit | does not block 0.11.0 (a defensive guard: the parser refuses it first) |
| 4 | a compilation without the core prelude / nothing loaded | sema.nz:1809, 15053 | not hit | does not block (guards; the loader always supplies it) |
| 5 | an import the loader did not resolve | sema.nz:1907, 1957 | not hit | does not block (diagnostic already made by the loader) |
| 6 | a module imported under a name by itself | sema.nz:1916 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 |
| 7 | a module re-exporting two types of a kind beside public items of its own | sema.nz:2305 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 |
| 8 | a type written in a shape the checker does not read (a length, an empty or lower-case name) | sema.nz:2989 | not hit | does not block (fallback) |
| 9 | an array whose byte count overflows | sema.nz:3719 | not hit | does not block |
| 10 | a statement / an expression kind with no case (fallbacks) | sema.nz:5949, 5891, 6254, 6370 | not hit: every syntax kind of the corpus is checked | does not block |
| 11 | an expression kind inside a contract | sema.nz:6742, 6785 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 |
| 12 | a call of a built-in whose signature names its own parameter or a type the checker cannot shape (`bok == 0`), also spawned | sema.nz:7744, 7889; table at 1686-1726 | not hit | does not block; **which built-ins have `bok == 0` is not recorded and I did not compute it** |
| 13 | a renamed built-in or a capability spawned | sema.nz:7847 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 |
| 14 | a type parameter named in an expression | sema.nz:8742 | not hit | does not block (the reference refuses it too, in words the checker lacks) |
| 15 | an enum written under two names | sema.nz:8781 | not hit | does not block |
| 16 | in a constant: a float converted to an integer, a float compared, a shift by a float | sema.nz:11232, 11271, 11278 | not hit | does not block 0.11.0; after 0.11.0 before 1.0 |
| 17 | a `return` / a `?` in a body whose result type is inferred | sema.nz:6151, 11530 | not hit | does not block 0.11.0 (R1.5 D2); after 0.11.0 before 1.0 |
| 18 | a function of more than 60 parameters and captures (provenance mask) | sema.nz:14568 | not hit | does not block |
| 19 | `@cfg` on `os` or `arch` when the caller passes "" | comments at facts.nz:84, diagnose.nz:19 | the commands pass the host's values | does not block |

Stops already closed (task text said they stop; they no longer do): `@derive` (D27, `derive.nz`, 7.8), packages (3.1ze), re-exports
(3.1zh), provenance (3.1r), effect parameters of functions (3.1v), `for` over `Bytes` and map literals (3.1zf), statics and constants of
every type (3.1t, 3.1za), the prelude's implicit edges (3.1n). The comment at `compiler/configure.nz:752` ("a type that derives a format
stops the checker") is stale since D27: already closed, a wording fix in `compiler/` (not done here: out of scope).

## Stop sites in the Core IR lowering `compiler/core.nz`

`co_stopped`, `co_stopped_stmt`, `co_general_stop` (about 40 sites, e.g. core.nz:684-692, 826, 962, 1481-1617, 2006-2486): each is a guard
for data the checker did not record (a binding declared nowhere, a variant the checker did not resolve, a statement or an expression
kind with no lowering). Verdict: already closed as a construct list. 5.1f ends "Nothing is stopped": all 105,232 functions of the
corpus lowered, and the nine released programs' Core IR is identical to the seed's. The guards stay as defence.

## Manifest stops `compiler/package.nz` (`unsupported_m`, 21 sites, lines 324-548)

A control character, an array of tables, table headers or keys or values it does not read, `[registry]`, an inline table at the top, a
heap limit of more than sixteen digits, a dependency's features, a registry dependency, a version it does not read, a package's `default`
features. `r8-cut-over` design S11.2 records what stays refused: `[registry]`, registry dependencies (the solver, N90), dependency
`features` and `default` (they enable features), an array of tables. Verdict: does not block 0.11.0 (the nine programs and the std packages
are read, 16.3/16.4); registry and features are ecosystem features after 1.0 or earlier by decision (not recorded in the decisions log).

## The bootstrap chain's record (`NOT_CARRIED`, `selfhost.rs:2869`)

About 90 built-ins (Chan, Select, MMIO, Box/Cell/Chain, checked/wrapping/saturating arithmetic, floats, atomics, `os_*`) are not carried
by the bootstrap checker `compiler/analyse.nz`. This is the legacy chain (`task 2.8b`), not `sema.nz` (7.6 says so). Verdict: does not
block 0.11.0; closes when the chain retires (4.3b).

## What I could not verify

- I ran nothing. The agreement numbers are copied from the task lists, not re-measured; the corpus 3,247 are the numbers at 7.8/7.9
  (2026-10-16), and the corpus has grown since.
- Whether `library/std` is among the corpus inputs (`extract-inputs.py` not read); the self-build proves the nine programs, which import it.
- Which built-ins have `bok == 0` (row 12), and which declarations reach row 2.
- The rows say "not hit" for the released set because the self-build and the corpus carry everything; a stop that the nine programs do hit
  would have failed 16.3/16.4, which passed. Rows 1-19 are not each tested one by one.
- `compiler/parity.json` has no record for `sema.nz` stops at all; there is no per-stop test.
