# Versions and stability

What a Nazm release number means, which other identities exist beside it, and what v0.x
promises a person writing code against it. Settled in R1, 2026-10-07, for the first public
release.

## Six identities, each versioned on its own

A release carries several version identities. They answer different questions and move
independently: a toolchain release does **not** imply a new semantic epoch, interface schema
or runtime ABI, and none of those implies a new toolchain release.

| Identity | Current | What it identifies | Changes when | Where it is reported |
| --- | --- | --- | --- | --- |
| **Language version** | **v0.3** | the language `docs/spec.md` specifies: what a program means | the settled semantics gain or change a construct | `docs/spec.md`'s status line; `nazm --help` |
| **Toolchain version** | **0.3.0** | this release of the `nazm` binaries and crates | every release, by the rules below | `nazm --version`; `toolchain.compiler` in `nazm inspect`; the release manifest |
| **Semantic epoch** | **29** | the checker's rules: what it accepts and refuses | any change to what the checker accepts or refuses, in that same commit | `toolchain.semantic_epoch` in `nazm inspect`; build provenance |
| **Interface schema** | **`nazm.interface/11`** | the persisted shape of what a module publishes | a module's published facts change shape | `toolchain.interface` in `nazm inspect` |
| **Runtime ABI** | **14** | the contract between compiled code and the runtime it links | a runtime symbol, layout or calling rule changes | `toolchain.runtime_abi` in `nazm inspect`; `nazm.runtime/1` manifests |
| **Package and lock schemas** | `nazm.package/1`, `nazm.lock/1`, `nazm.registry-index/1` | the manifest, the lockfile and a local registry's index | their shape changes incompatibly | the `schema` field of each file |

Two more are versioned separately and on purpose: the **standard library** is **1.0**
(`library/std/API-1.0`, `docs/spec.md` *The standard library*), and every machine-readable
output names its own schema, `name/major` (`docs/diagnostics.md`, *What compatibility means*).
The VS Code client in `editors/vscode` has its own extension version.

The language version and the toolchain's major and minor numbers are kept equal while the
language is pre-1.0. Toolchain 0.3.x implements language v0.3; a patch release, 0.3.1, fixes
defects and changes no settled semantics. The public tag for a release is the toolchain
version with a `v`: `v0.3.0`.

*History.* Until R1 the crates said `0.0.1` while `docs/spec.md` said v0.3, and project
records called the work "the v1 foundation". The **v1 foundation** is an architecture phase
of the roadmap, not a semantic version; it does not mean 1.0.

## What v0.x promises

Nazm is pre-1.0 and experimental. **v0.x may evolve, but a breaking change is deliberate,
documented and versioned, and never silently reinterprets existing code.** In detail:

- **Syntax and semantics.** A minor release (0.3 → 0.4) may change the language. Every such
  change is written into `docs/spec.md` first, and a program whose meaning would change is
  refused with a diagnostic rather than run with the new meaning. A patch release does not
  change the language. There are no editions yet; if one is ever needed, it is how a program
  would opt into a changed meaning.
- **Standard library.** 1.0 and stable within 1.x: an item may be added, never removed or
  changed in signature, effects or documented behaviour. A removal or change is 2.0.
- **Diagnostics and machine output.** Diagnostic codes are stable and never reused; message
  and help text are prose and may change in any release — match on `code`. A machine schema
  changes major version for any change a consumer written against the previous one would
  read wrongly; new fields, codes and enum values are compatible and may appear in any
  release (`docs/diagnostics.md`).
- **Packages.** `nazm.toml` and `nazm.lock` keep their schema identifiers; a toolchain that
  cannot read a manifest or lock refuses it by schema rather than guessing. A lockfile
  written by 0.3.0 is read by every 0.3.x.
- **Caches.** A cached check result is keyed by, among other inputs, the toolchain version,
  the semantic epoch and the interface schema, so one written by another toolchain is
  recomputed rather than reused. A cached object is keyed by its exact backend input, the
  backend's own identity and the code-generation options, which is what determines its bytes
  (`docs/architecture.md` §7.6 states the one case it cannot tell apart).
- **Runtime ABI.** Not a public interface in v0.x. Objects and prebuilt runtimes
  (`nazm runtime build`) are linked only with the toolchain and ABI revision that produced
  them, and a mismatch is refused. Nothing promises an ABI across releases.
- **C interop.** What crosses an `extern "C"` boundary is specified in `docs/spec.md`
  (*Foreign functions*, v1–v3) and follows the language version.

## How a change is made

1. **Constitution before code.** A semantic change is written into `docs/spec.md` (and,
   where architectural, `docs/architecture.md`) before it is implemented.
2. **The right identity moves in the same commit.** A changed checking rule bumps the
   semantic epoch (`cargo xtask check`'s cache-soundness gate fails when the checker can raise
   a code the identity does not list, and says to bump it); a
   changed runtime contract bumps the ABI revision; a changed persisted shape bumps its
   schema's major version.
3. **Deprecation.** Something to be removed is first marked deprecated in the spec and, where
   the compiler can tell, reported with a diagnostic carrying a fix, for at least one minor
   release before removal. A removal is listed in the release record of the release that
   makes it.
4. **Evidence follows.** `docs/capability-matrix.md` records what is verified and how;
   `docs/limitations.md` records what is not done.

## What 1.0 would mean

Not promised, and not scheduled. A 1.0 language would mean a compatibility promise across
minor releases of the kind the standard library already makes. Until then, pin the toolchain
version a project builds with.
