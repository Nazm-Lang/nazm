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
- [ ] 3.1 `nazm.api-doc/2` and `-o DIR` (`api.json`, `api.md`), from the checker's public definitions

## 4. `init`
- [x] 4.1 Every template written as the reference writes it; the directory rules (design D9): 24 of
      24 projects and every command line of `tests/args/init.txt` agree (2026-10-15)

## 5. `check`
- [ ] 5.1 Text and `--json` diagnostics, as R2's checker carries them; refusing past what it carries

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
- [ ] 11.1 The command-line grammar: one `nazm` dispatching to every command, `--help`, `-V`, usage
      errors (each command's own arguments, usage errors and help are `cli.nz`'s since 2.5; what is
      left is the top level and what `cli.nz` does not carry yet: subcommands, aliases, possible
      values, groups, conflicts)
- [ ] 11.2 `bindgen`, `contract`, `accel`, `bench`, `profile`, `explain-cost`, `explain-flow`,
      `obligations`, `trace`, `reload-check`, `repl`, `comptime`, `capabilities`
