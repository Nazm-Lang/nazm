# {{name}}

A library package: no `main`. `nazm check .` checks every module; `nazm test tests` replays the
tests; `nazm doc . -o api` documents the public API. Another package depends on it with
`{{name}} = { path = "../{{name}}", version = "0.1.0" }` and writes `use "{{name}}:lib.nz";`.
`nazm publish --registry DIR . --dry-run` shows what publishing would record.
