# Security policy

## Supported releases

| Release | Supported |
| --- | --- |
| the latest 0.9.x preview (macOS on Apple Silicon) | yes — a fix lands in the next preview release; a preview makes no compatibility promise |
| from 1.0: the latest 1.x release | yes — a fix lands in the next patch release of the latest 1.x minor |
| from 1.0: an older 1.x minor | no — upgrade to the latest 1.x, which the 1.x compatibility promise keeps compatible for programs on stable surfaces (`docs/stability.md`) |
| the internal 0.3.x candidates, and anything older | no |

No maintenance branch is kept for an older minor release, and none is promised. A security fix
may refuse a program that was accepted only because of the defect it fixes; it is then refused
with a diagnostic naming the rule, never run with a different result, and the release record
says so (`docs/stability.md`, *What a 1.x release may change*).

## Reporting a vulnerability

Report privately. Do not open a public issue, pull request or discussion for a suspected
vulnerability, and do not publish exploit details before a fix is available and the reporter and
maintainer have agreed a disclosure date.

- **Preferred:** GitHub's private vulnerability reporting for this repository — the *Security* tab,
  *Report a vulnerability*.
- **Otherwise:** email the maintainer at the address in the workspace manifest's `authors`
  (`Cargo.toml`), with "Nazm security" in the subject.

*Status, 0.9.0 (2026-10-12):* the repository is public at `Nazm-Lang/nazm` and private
vulnerability reporting is enabled on it, so the preferred channel is open. That the fallback
address receives mail has not been confirmed (`docs/vulnerability-process.md`, *Intake*).

Include the toolchain version (`nazm --version`), the host and target, a minimal program or
input that shows the problem, and what you expected. You will get an acknowledgement, and the
outcome will be stated plainly, whichever way it goes.

## What counts

Security-sensitive, among others:

- a program the checker accepts that violates a property the specification states — reaching the
  outside world without the capability `docs/spec.md` requires, data reaching the restricted sink
  the provenance rules forbid, a restriction profile admitting what it says it refuses;
- memory unsafety in a compiled Nazm program that makes no foreign call: an out-of-bounds access,
  a use after reclamation, a data race between tasks;
- a compiler, runtime or tool crash or hang on crafted input — source, a package manifest, a
  lockfile, a registry index, an interface or cache file — that a fuzzer would class as a defect;
- `nazm attest verify`, `nazm.lock` digests or the release manifest accepting something they
  should refuse;
- a heavy workload escaping the contained runner's bounds (`docs/runbook.md`).

Not vulnerabilities in themselves, because they are stated limits: anything a C function does once
called, a deadlock, an unchecked runaway loop in a native executable, implicit information flow,
and anything `docs/limitations.md` lists.

## What the security features are not

Capabilities, provenance and restriction profiles are **compile-time checks of stated rules**.
They are not a sandbox, not run-time isolation, and not a certification under any standard. The
`critical` and `cyber` profiles refuse the constructs their rules name and claim nothing else.
`docs/limitations.md` and `docs/capability-matrix.md` (areas 6, 7, 19 and 20) state their scope.
