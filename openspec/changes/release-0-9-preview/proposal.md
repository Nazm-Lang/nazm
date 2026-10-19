## Why

The copyright holder decided on 2026-10-12 to publish a **preview** before the rewrite in Nazm
completes: toolchain **0.9.0**, built by the Rust implementation, for **macOS on Apple Silicon**
only, with the language described as a **1.0 draft**. Later versions carry the toolchain rewritten
in Nazm (`rewrite-in-nazm`). This amends that change's "nothing is published before the rewrite":
the rewrite still comes before a 1.0, and before the third-party notices go away, but a preview no
longer waits for it.

Decisions (the copyright holder, 2026-10-12):

| | Decision |
| --- | --- |
| (a) Version | toolchain `0.9.0` (first proposed as 0.3.0, a number an earlier internal candidate used, `docs/internal/releases/996984f.md`); the language a **1.0 draft** — no compatibility promise until 1.0 |
| (b) Target | macOS, Apple Silicon (`aarch64-apple-darwin`) only |
| (c) History | the two prompt-pack directories committed in `191b092` are removed from the history before the repository is made public |
| (d) Publication | a public GitHub repository, a binary release, and `nazm.dev`; external contributions are not accepted yet (the CLA's legal review is open) |
| (e) Mutation testing | the full release mutation catalogue is waived for the preview and named in the release notes; it stays outstanding for 1.0 |
| (f) Signing | no Apple Developer ID: the binary is ad-hoc signed; the notes say how to clear the quarantine a browser download gets |

## What Changes

- The toolchain's version becomes `0.9.0` everywhere it is stated; the documents that call the
  language or the toolchain a released 1.0 say *1.0 draft*; the stability classes keep their names
  and are read as the promises 1.0 will make.
- The release archive carries `THIRD-PARTY-NOTICES.md`, generated from `Cargo.lock` for what
  `bin/nazm` links (`docs/user/licensing.md`, item 7), beside the project's own licence files.
- `docs/user/releases/0.9.0.md`: what the preview is and is not, the target, the licences, the waived
  mutation run, the unsigned binary, and that contributions wait for the CLA's review.
- The R1 language items committed by then ship as EXPERIMENTAL, as their sections say.
- The history is rewritten to drop the prompt packs, and the commit hashes the documents cite are
  remapped in one commit after it.

## Impact

`Cargo.toml` versions, `README.md`, `docs/user/stability.md`, `docs/user/spec.md`, `docs/user/roadmap.md`,
`docs/user/licensing.md`, the release scripts, and every document citing a commit hash. No language
semantics change: the semantic epoch, the interface schema and the runtime ABI are unchanged.
Publishing — the push, the repository's visibility, the release upload and the site — waits for the
copyright holder's go-ahead.
