## 1. Constitution

- [x] 1.1 `docs/systems-domains.md`, `docs/v1-domain-matrix.md`, this change (3A)

## 2. Hosts

- [x] 2.1 Linux x86_64 execution; prebuilt archives; clean installs (3B): the aarch64 archive and clean install (3B-1); the Cranelift x86-64 narrow-conversion fix (3B-2); x86_64 execution, archive and clean install carried to final v1 (user, 2026-10-10)
- [x] 2.2 Windows x86_64: constitution, Win32 runtime port, evidence under Wine (3C); real Windows host execution carried to final v1

## 3. Systems, embedded, realtime

- [ ] 3.1 Atomics, statics, sections, board manifests, interrupts, arena, failure hook; kernel workload (3D)
- [ ] 3.2 Cortex-M or its blocker; `@std/hal`; timers; state-machine workload (3E)
- [ ] 3.3 Realtime rules; fixed-priority board scheduler (3F)

## 4. Domains

- [ ] 4.1 Robotics, industrial, high assurance, aerospace, automotive, medical-style, cyber (3G)
- [ ] 4.2 AI/HPC CPU provider, float kernels, SIMD (3H)
- [ ] 4.3 Storage engine; distributed foundation (3I)
- [ ] 4.4 WASI (3J)
- [ ] 4.5 Android, iOS, desktop host (3K)
- [ ] 4.6 Web3 qualification; requirements traceability; the domain matrix completed (3L)

## 5. Evidence

- [ ] 5.1 Contained gate: workspace, lifecycle, selfhost/bootstrap where affected, targeted mutations, platform tests
