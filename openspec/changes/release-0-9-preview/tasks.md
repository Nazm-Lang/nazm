## 1. Decision

- [x] 1.1 The copyright holder decided (a)–(f), 2026-10-12

## 2. The release commit

- [x] 2.1 Version 0.9.0 in the workspace; "1.0 draft" in the documents that claimed a released 1.0
- [x] 2.2 `THIRD-PARTY-NOTICES.md` generated from `Cargo.lock`, and the release script ships it
- [x] 2.3 `docs/user/releases/0.9.0.md`; `docs/user/licensing.md` item 7 and the rewrite programme updated
- [x] 2.4 Host suite, `cargo xtask check`, contained tests, selfhost, bootstrap and lifecycle on it
- [x] 2.5 The archive built for `aarch64-apple-darwin`, ad-hoc signed, with checksums and the SBOM
- [x] 2.6 An install of the archive on this machine, from a clean directory, runs the examples

## 3. History

- [x] 3.1 A mirror backup of the repository
- [x] 3.2 `git filter-repo` removes `docs/Nazm_N40-N48_FULL_Exhaustive_Prompts/` and
      `docs/nazm_N49_N75_FULL_prompts/`; the commit hashes cited in documents are remapped

## 4. Publication (each step on the copyright holder's go-ahead)

- [x] 4.1 The GitHub repository, recreated or force-updated so no old object is reachable, made public
- [x] 4.2 The tag `v0.9.0` and the release with the archive and its checksums
- [x] 4.3 `nazm.dev` built from the release commit and deployed
