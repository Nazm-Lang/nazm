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
- [ ] 5.2 `--summary-json`, `--cache-report`, `--source-root`, `--profile`, `--profile-report`, packages
      and `nazm.root` markers (refused today, design D11)

## 6. `test`
- [ ] 6.1 The corpus runner, interpreted and compiled (after R3, R4 and R9 are integrated)

## 7. `fix`
- [ ] 7.1 `check`'s fixes applied, `--apply` and the plan

## 8. `lsp`
- [ ] 8.1 The Language Server over stdin/stdout, compared by scripted sessions

## 9. The agent commands and the MCP server
- [ ] 9.1 `context`, `snapshot`, `delta`, `diagnostics`, `docs`, `repo`, `patch`, `resolve`,
      `references`, `inspect`, `interface`, the MCP server

## 10. Packages
- [ ] 10.1 `lock`, `update`, `publish`, `yank`, `attest`

## 11. The rest
- [x] 11.1 The command-line grammar: one `nazm` dispatching to every command, `--help`, `-V`, usage
      errors (design D1, D12): `toolchain/commands/nazm.nz` routes to `fmt`, `init` and `check` and
      refuses the rest; every line of `tests/args/nazm.txt` agrees (2026-10-15). Left in `cli.nz`:
      a command's own subcommands (`runtime`, `attest`, `patch`), aliases, possible values, groups,
      `requires` — each with the first command that declares it
- [ ] 11.2 `bindgen`, `contract`, `accel`, `bench`, `profile`, `explain-cost`, `explain-flow`,
      `obligations`, `trace`, `reload-check`, `repl`, `comptime`, `capabilities`
