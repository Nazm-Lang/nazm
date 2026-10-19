## 1. The proposal and the harness

- [x] 1.1 The order of the commands (`proposal.md`) and how each is compared (`design.md`, D6, D7)
- [x] 1.2 `toolchain/commands/tests/run.sh`: the reference built from this checkout, every input
      formatted by both sides with and without `--check`, one summary line

## 2. `fmt`

- [x] 2.1 `toolchain/commands/fmt.nz`: `nazm_syntax::format` ported over `compiler/lex.nz`'s tokens,
      comments recovered from the gaps (D2)
- [x] 2.2 The command as `crates/nazm-cli/src/fmt.rs`: `--check`, the listing and summaries, exit
      statuses, reading as `read_to_string` (D4)
- [x] 2.3 Lexing diagnostics rendered as `Diagnostic::render` (D3)
- [x] 2.4 Every input agrees: 2,984 of 2,984 (2026-10-15), the batch run and the missing file too
- [x] 2.5 clap's usage errors and `--help` for `fmt`: the shared parser (`toolchain/commands/cli.nz`,
      design D8); a directory named as a file is `cannot read` (D4); every command line of
      `tests/args/fmt.txt` agrees (2026-10-15)
- [ ] 2.6 Future: drop a statement's `;` once the repository's sources may use the new syntax
      (design, *Future work*) — in both formatters together

## 3. `doc`
- [x] 3.1 `nazm.api-doc/2` and `-o DIR` (`api.json`, `api.md`), from R2's published facts
      (`compiler/facts.nz`) and package resolution (design D13): `toolchain/commands/doc.nz`.
      2026-10-15, 3,115 inputs (every input of `check`, the repository's packages, a project of
      every template): 3,108 agree, 0 differ, 7 refused (6 where the checker stops, 1 package
      `compiler/package.nz` does not resolve); every command line of `tests/args/doc.txt` agrees

## 4. `init`
- [x] 4.1 Every template written as the reference writes it; the directory rules (design D9): 24 of
      24 projects and every command line of `tests/args/init.txt` agree (2026-10-15)

## 5. `check`
- [x] 5.1 Text and `--json` diagnostics, as R2's checker carries them; refusing past what it carries
      (design D11): `toolchain/commands/check.nz`. 2026-10-15, 2,993 inputs: 2,837 agree, 146
      refused (the checker's stops, packages, package imports), 10 differ — every one R2's checker
      trapping (`N0405` in `compiler/syntax.nz`'s `sx_kid`) on the interpreter's modules that reach
      `toolchain/interp/vm.nz`, a defect of `compiler/` (hand-back); every command line of
      `tests/args/check.txt` agrees. Since 2026-10-15 run through `fx_check` (design D13): 3,097
      inputs, 3,048 agree, 0 differ, 49 refused (32 packages, 10 package imports, 6 the checker's
      stops, 1 not UTF-8)
- [x] 5.2 Packages and package imports (design D15, 2026-10-16): a package's directory (its `main`,
      or a library's every module), a file in a package with its dependencies, `N0508`, manifests
      that are not ones, `--locked` without a lockfile, `--features` in a package, `--profile`
      naming none or `general`, `--cache-report --no-cache`. 3,163 inputs (the repository's
      packages added): 3,112 agree, 0 differ, 51 refused (44 a pattern `let`, epoch 88, not in
      R2's checker — hand-back; 6 the checker's stops; 1 not UTF-8); `tests/args/check.txt` 72 of
      77 agree, 5 refused. Still refused, and why (D15): `--locked` with a lockfile (package
      digests), other profiles and `--profile-report` (MIR facts), `--cache-report` with the cache,
      `--source-root` (`fx_check` takes no root), `--summary-json` (N30's diagnostic ids and
      `state.source`), `nazm.root` markers (the loader stops there)

- [x] 5.3 The pattern-`let` refusal lifted (R2's checker holds epoch 88 since `ff1dce6a`) from `check`, `doc`
      and `fix`, and `check --source-root` carried (`fx_check_in`, design D15). 2026-10-19, 3,349
      inputs of `check`: 3,348 agree, 0 differ, 1 refused (not UTF-8); `doc` 3,356: 3,354 agree, 0
      differ, 2 refused; `fix` 3,339: 3,338 agree, 0 differ, 1 refused (not UTF-8; 17 applied);
      `tests/args/` 357 command lines: 350 agree, 0 differ, 7 refused (`--cache-report` with the
      cache, a profile other than `general`, `--profile-report`, `--summary-json`, `--source-root`
      outside the root module's directory or for a package's module, a directory as `fix`'s root)

- [x] 5.4 `check --summary-json` and `check --locked` with a lockfile (design D23, 2026-10-19): the summary's
      references linked to the language service's identities (`fx_diag_ids`, `fx_state_source`), `Graph::verify` of the
      lockfile (`pkg.nz`) with the reference's words. `tests/args/check.txt` 123 of 128 command lines agree (twelve
      lockfiles each wrong in its own way, every package shape under `--summary-json`), 0 differ, the 5 refused named in
      5.3; 3,393 inputs of `check` each with `--summary-json`: 3,392 agree, 0 differ, 1 refused (not UTF-8)

## 6. `test`
- [x] 6.1 The corpus runner, interpreted and compiled (design D17, 2026-10-19):
      `toolchain/commands/test.nz`, the two legs run by the `nazm` `NAZM_TEST_DRIVER` names (the port
      has no `run` or `build`; without it the command refuses). 123 recorded cases of the repository
      (text and `--json`): 123 agree, 0 differ, 0 refused; 11 whole runs (the conformance corpus, a
      fixture, compiled legs, `--summary-json`, `--filter`, `--fuzz`): 11 agree; `tests/args/test.txt`
      38 command lines agree

## 7. `fix`
- [x] 7.1 `nazm fix`: `check`'s fixes reported and, with `--apply`, applied (design D14):
      `toolchain/commands/fix.nz`. 2026-10-15, 3,098 inputs: 3,091 agree, 0 differ, 7 refused (6
      the checker's stops, 1 not UTF-8), 17 of them applied on a copy; `tests/args/fix.txt` 22 of 23
      agree, 1 refused (a directory as the root, which `compiler/load.nz` traps on)
- [x] 7.2 The plan form, `nazm patch fix` (`--root`, `--file`, `--byte`, `--fix`) and `patch rename`, with `patch`
      (design D18, D24, 2026-10-19): `patch.nz` carries the whole command line (`group.nz`) and `patch apply` (plan read,
      digests, expected bytes, all-or-nothing writes); `patchplan.nz` plans a rename (`rename.nz`, the engine the language
      server's rename shares) and a fix over R2's reference index, call graph and provenance, bound to `nazm.snapshot/3`
      (`snap.nz`). Measured on the checkout 2026-10-19 (R5d): `run.sh agent` over 3,399 inputs, `patch rename` at six names
      each, 19,999 runs: 19,237 agree, 0 differ, 762 refused; `patch fix` at three diagnostics with a fix, 52 runs: 52
      agree; `tests/args/patch.txt` 63 command lines agree. Refused, counted and named (status 6): a plan in a file other
      than the root, a rename of an exported name in a declared workspace, a local declared in a closure or in no
      function, a compilation after which the facts' index differs from the reference's (D24)

## 8. `lsp`
- [x] 8.1 The Language Server over stdin/stdout, compared by scripted sessions (design D16,
      2026-10-16): `toolchain/commands/lsp.nz` — framing, the handshake, `shutdown`/`exit`,
      `MethodNotFound` (formatting included: the reference offers none), `didOpen`/`didChange`/
      `didClose` with published diagnostics, hover (locals of built-in types, calls to plain
      functions) and definition (locals, such calls). `tests/lsp/`: 13 sessions, 11 agree, 0
      differ, 2 refused (two documents open; a document unlike its file). Every input opened, and
      with hover at seven positions: 3,151 inputs, 2,147 agree, 0 differ, 1,003 refused (hover of
      what the facts do not index, a compilation that does not load, pattern `let`s …), 1 not
      UTF-8; `tests/args/lsp.txt` 8 of 8 agree
- [ ] 8.2 What 8.1 refused, part done (2026-10-19, design D16): the editor's unsaved buffers
      (`fx_check_over`), hover of any type (`fx_show`), definition of anything `fx_declaration_at`
      indexes (types named in signatures too), `--source-root`. `tests/lsp/` 15 sessions: 14 agree,
      0 differ, 1 refused (two open documents); every input opened, hover at seven positions: 3,340
      inputs, 2,602 agree, 0 differ, 737 refused (375 hover of a name the facts publish no binding
      for, 303 a compilation that does not load, 24 a position in a document that does not lex, 21
      a source root the reference refuses, 10 hover of a method, closure, generic or foreign
      function, 2 a warning beside a syntax error, 2 a call through a value or with effects the facts
      do not know), 1 not UTF-8. Still refused: two open documents, completion, references,
      signature help, rename, symbols, semantic tokens, code actions, call hierarchy

- [ ] 8.3 The reference index's navigation (design D23, 2026-10-19): `definition` of anything `fx_ref_*` indexes,
      `references`, `prepareRename` and `rename` (with the candidate program re-checked and its occurrences
      partitioned as before), `documentSymbol` and `workspace/symbol`, nested and flat. `tests/lsp/` 18 sessions: 17
      agree, 0 differ, 1 refused (two open documents); 3,378 inputs in six sessions each: 2,730 agree, 0 differ, 647
      refused (named in D23), 1 not UTF-8. Still refused: completion, signature help, semantic tokens, code actions, call
      hierarchy, a rename of an exported name in a declared workspace, every index query in a compilation with errors
      the facts' index is not promised for (hand-back to R2: after `N0203`, `N0305`, `N0334`, `N0386`, `N0610`, `N0101`
      the index differs from the reference's; and the facts record a trait method's uses inside its own default
      methods, which the reference does not)

## 9. The agent commands and the MCP server
- [ ] 9.1 `context`, `snapshot`, `delta`, `diagnostics`, `docs`, `repo`, `patch`, `resolve`,
      `references`, `inspect`, `interface`, the MCP server. Part done (2026-10-19): `interface` (design
      D19; 3,342 inputs, `--hash` too: 3,342 agree, 0 differ, 0 refused; 28 command lines agree) and
      `docs` (D20; every one of the 1,498 sections by id and every form of the index: 1,652 runs
      agree; 13 corpora each with one thing wrong: 39 runs agree; 36 command lines agree) and
      `patch apply` (7.2). Not ported: `context`, `snapshot`, `delta`, `diagnostics`, `resolve`,
      `references`, `inspect`, `repo` and the MCP server, which need the reference index, the call
      graph, provenance, Core IR/MIR and the language service's diagnostic ids from `compiler/` (D20)

## 10. Packages
- [x] 10.1 `lock`, `update`, `publish`, `yank`, `attest` (design D21, 2026-10-19): `pkg.nz`, `toml.nz`,
      `pkgcmd.nz`, `attest.nz`, `proc.nz`. Command lines over a fixture of packages, a registry of
      published versions (made by the reference's `publish`), wrong indexes and manifests, a key pair:
      `lock` 76, `update` 17, `publish` 35, `yank` 24 and `attest` 54 command lines, every one agreeing
      (files written compared too); the whole `tests/args/` run: 206 of 206 agree. Refused, with status 6: a form of
      TOML the reader does not read, a record that is not JSON (`attest sign`)

## 11. The rest
- [x] 11.1 The command-line grammar: one `nazm` dispatching to every command, `--help`, `-V`, usage
      errors (design D1, D12): `toolchain/commands/nazm.nz` routes to `fmt`, `init` and `check` and
      refuses the rest; every line of `tests/args/nazm.txt` agrees (2026-10-15). Left in `cli.nz`:
      a command's own subcommands (`runtime`, `attest`, `patch`), aliases, possible values, groups,
      `requires` — each with the first command that declares it
- [ ] 11.2 `bindgen`, `contract`, `accel`, `bench`, `profile`, `explain-cost`, `explain-flow`,
      `obligations`, `trace`, `reload-check`, `repl`, `comptime`, `capabilities`. Part done
      (design D22, 2026-10-19): `bindgen` (every header of `tests/bindgen` and `bench/compare`, with
      and without `-o`: 28 runs agree; 30 command lines agree), `reload-check` (every pair of 24
      programs: 576 agree; the programs with a record or an enum refused, the types of their fields
      not being published; 36 command lines agree) and `doctor` (5 command lines, and with `PATH`
      and `TMPDIR` made wrong: agree). Not ported: `contract`, `accel`, `obligations`,
      `explain-cost`, `explain-flow`, `profile`, `trace` (MIR, LIR, Core IR digests and the
      runtimes), `bench` (timings), `repl`, `comptime` (the interpreter in-process),
      `capabilities` (the backend's own table)
