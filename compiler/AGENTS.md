# compiler/ — the compiler written in Nazm

Specialises the root `AGENTS.md` for this subtree; it relaxes nothing there.

- **Nothing here runs on the host as a stage.** `nzlex`, `nzparse`, `nzcheck`, `nzemit`, `nazmc`,
  the selfhost suite and `bootstrap.sh` run only through `cargo xtask contained selfhost` or
  `cargo xtask contained bootstrap` (`docs/internal/runbook.md`, *Mandatory containment*). The stage builders
  refuse to start uncontained; never work around that.
- **`emit.nz` carries runtime text derived from `crates/nazm-runtime/src/core.rs`.** The
  `runtime parity` gate in `cargo xtask check` re-derives it; change the Rust constant and then this
  file, never this file alone (`crates/nazm-runtime/AGENTS.md`).
- **`analyse.nz`'s built-in table** is held to `Intrinsic::ALL` by
  `the_nazm_checker_knows_every_built_in_the_front_end_has` (`crates/nazm-cli/tests/selfhost.rs`),
  with an explicit `NOT_CARRIED` list. A built-in the self-hosted checker does not support goes on
  that list, named, rather than being silently absent.
- `conformance/` is a corpus with expected outputs; a case is changed only with the rule it tests.
- Layout: `nazm fmt --check compiler/*.nz`.
