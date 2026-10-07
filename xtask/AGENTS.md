# xtask/ — gates, the contained runner, mutation and benchmarks

Specialises the root `AGENTS.md` for this subtree; it relaxes nothing there.

- **This crate owns containment.** `src/contained.rs` and `src/procs.rs` enforce the ceilings,
  ownership labels and cleanup `docs/runbook.md` requires. A change here that weakens a ceiling,
  an ownership check, or the explicit failure when containment is unavailable is out of scope for
  any task that did not name it, and needs the lifecycle suite
  (`cargo test -p xtask --test lifecycle -- --ignored --test-threads=1`).
- **The mutation catalogue (`mutations/mutations.toml`) is law, not a list.** Each entry's `find`
  matches its file exactly once (`mutations apply` gate) and names the tests that kill it; an entry
  is repointed or retired with a note, never deleted to make a number look better
  (`docs/runbook.md`, *Mutation campaigns*; `CONTRIBUTING.md`).
- **A new `cargo xtask check` gate names the invariant it protects** in its doc comment, and is a
  pure read of the tree: it builds nothing and needs no network.
- One heavy workload at a time; never start a campaign, bootstrap or contained suite beside another.
