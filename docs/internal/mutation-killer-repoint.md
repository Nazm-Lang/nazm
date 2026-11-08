# Repointing the host-only mutation killers

**Decision (holder, 2026-11-08):** each of the 41 mutants below gets a killer that runs in the Linux
container — an existing test that runs there, or a new one written for it. No mutant is dropped or
excepted, no test is weakened, and the campaign's preflight (`verify_listed`) is not relaxed. The
release waits for the full targeted campaign (`decisions-log.md`).

A killer counts once `cargo xtask contained mutate --only NAME --verify-killers` passes it: the
pristine tree passes it, the mutant fails it, the restored tree passes it.

| Group | Mutants | Why the old killer cannot run in the container | Status |
| --- | --- | --- | --- |
| host-run (xtask) | r6-host-run-withdrawal-marker, -second-stage, -not-executable, -host-environment, -deadline | `host_run.rs` tests are `ignore` off macOS (the runner is macOS only) | **done**: `Host` seam, `host_run::tests::*`; 13 `r6-host-run*` mutants verified in the container |
| Windows objects | r9-8-3-a-small-allocation-is-misdescribed, r9-8-2 (xmm pair, argument swap, rsi/rdi, shadow space, large frames), r9-8-1 (pdata section, pc-relative field) | needs mingw or clang on the Mac | open |
| generator through the driver | r9-nazm-quietly-becomes-llvm, r9-one-thread-is-said-backwards, r9-a-refusal-is-reported-as-a-defect, r9-a-debug-build-hands-over-no-file-names, r9-7-8-the-archive-carries-no-generator, r9-7-9-the-record-does-not-name-the-generator, pt-nzcg-allocates-from-c-beside-the-small-heap | `nazm_backend` tests are macOS only before 1.0 | open |
| generator x86-64 | r9-7-6 (fused branch, remainder reads the select, remainder divides by minus one), ci-every-lsda-shares-one-atom | Rosetta 2 and Mach-O only | open |
| preemption | pp-an-asked-task-does-not-yield, pp-the-monitor-sees-no-one-waiting, pp-the-monitor-interrupts-no-one, pp-the-handler-finds-no-generated-code, pp-generated-code-has-no-section, pp-cranelift-code-stays-in-the-text-section, pp-nzcg-code-stays-in-the-text-section | `scheduler_pool` test is `cfg(target_os = "macos")` | open |
| release build | r1-5-d15-a-release-build-has-no-canary, -is-not-stripped, -is-signed-without-the-hardened-runtime | `otool`, `nm`, `codesign`; `cfg(target_os = "macos")` | **done**: `finish_plan` seam and unit tests; the canary mutant reuses the portable `release::tests` case; 3 mutants verified |
| attestation | n94-the-signature-is-not-checked, n94-an-artefact-is-not-checked, n94-a-stray-artefact-is-attested | the test is `ignore` | **done**: `openssh-client` added to the image; the test no longer ignored; 4 `n94-*` mutants verified |
| debugger | n91-sampling-without-lines, n91-macho-addresses-are-not-settled | the test is `ignore` (lldb, Mach-O) | **done**: `build_command` seam in `run_profile.rs` and a unit test; a Linux test reads the Cranelift Mach-O object for `aarch64-apple-darwin` and checks its DWARF words are settled; both seed exemptions logged |
| others | n95-a-cpu-is-misrecorded, g3-a-toolchain-failure-is-blamed-on-the-ir | `ignore` / `cfg(target_os = "macos")` | **done**: a stub-`clang` link-failure test; `n95` keeps its portable killer; 2 mutants verified |
