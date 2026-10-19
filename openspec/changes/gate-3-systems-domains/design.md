## Context

`docs/internal/systems-domains.md` is the design: what Gate 3 starts from at `3a2e3ba`, the rules every part
follows (one semantic language, platform behaviour only in the runtime's platform layer, evidence
labelled by where it ran), and a decision per part with the reason it is not a second mechanism.

## Goals / Non-Goals

**Goals:** executable evidence for every accepted v1 area; precise blockers where it cannot run here.

**Non-Goals:** certification or qualification claims; ecosystems (ROS, AUTOSAR, fieldbuses, GUI
frameworks, chain host interfaces); Microsoft's CRT/SDK in Gate 3.

## Decisions

As `docs/internal/systems-domains.md` records, including the user's of 2026-10-10: Windows through Wine in
Docker, never called Windows-host evidence; the Android NDK in `target/`; iOS after Xcode.

## Risks / Trade-offs

- [The Win32 port is the largest single change] → its own step (3C), independent of the rest.
- [Cortex-M is the first 32-bit native-code target beside wasm32] → a recorded blocker if unsound.
- [Emulated hosts are slower and not native] → labelled; timing claims only from native runs.
