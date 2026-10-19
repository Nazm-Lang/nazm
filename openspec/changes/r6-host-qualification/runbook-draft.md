# Draft — `docs/internal/runbook.md`, *Mandatory containment* (R6, task 7.2)

**Accepted 2026-10-20 and applied** to `docs/internal/runbook.md` (with the runner's name and refusals added
there, which now is the text that rules). Kept as the record of what was proposed.

```diff
 ### What must never run uncontained

 Only through the verified contained runner, `cargo xtask contained <workload>`:

-- any generated compiler stage — `nzlex`, `nzparse`, `nzcheck`, `nzemit`, `nazmc`;
+- any generated compiler stage built **without a heap limit** — `nzlex`, `nzparse`, `nzcheck`,
+  `nzemit`, `nazmc` — and any program written in Nazm built without one;
 - the selfhost suite and `compiler/bootstrap.sh`;
+- the self-build, `toolchain/selfbuild/selfbuild.sh` (`cargo xtask contained selfbuild`);
 - fuzzing, the mutation catalogue, and heavy benchmarks.

 **An environment variable is a declaration, not a ceiling. Never set `NAZM_CONTAINED=1` on
 the host, and never use `NAZM_ALLOW_UNCONTAINED=1`.** The runner sets `NAZM_CONTAINED=1`
 only *inside* an environment where a real ceiling already applies.
+
+### What may run on the host: one bounded-heap stage — R6, YYYY-MM-DD
+
+*Decided by the copyright holder on the evidence of `openspec/changes/r6-host-qualification/`.*
+
+One program written in Nazm — the front end (`compiler/diagnose.nz`), the pipeline
+(`toolchain/pipeline/main.nz`), the code generator (`toolchain/codegen/main.nz`), a command of
+`toolchain/commands/`, or a harness tool — may run on the host when **all** of these hold:
+
+- it was built with a heap limit of **at most 2 GiB** (`nazm build --heap-limit`, or the
+  self-build's runtime with that limit), which its runner checks before starting it: the counted
+  heap is linked (`nz.heap_malloc` is defined) and its limit is not larger;
+- it is the **only** such process, and no contained workload is running;
+- it runs under a wall-clock deadline, its output bounded and drained as the contained runner's is;
+- its peak resident memory is recorded (`/usr/bin/time -l` reports memory; no timing is taken
+  from it).
+
+The heap limit is a ceiling the program enforces on itself: every byte its code and runtime ask
+for is counted first, and an allocation past the limit does not happen (`N0406`, status 2;
+`docs/user/spec.md`, *A bounded heap*). It does not count a task's stack, the image, C's own
+allocations or child processes — hence one at a time, and the deadline.
+
+**Withdrawn at once**, back to contained only until investigated, if a host run's peak resident
+memory exceeds its heap limit by more than 256 MiB, if an `N0406` does not stop the program, or
+when the counted heap's runtime changes (`crates/nazm-runtime/src/heap.rs`, the pipeline's
+`count_heap`) until `cargo xtask contained selfbuild` passes again with its heap probe.
```
