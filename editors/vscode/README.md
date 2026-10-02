# Nazm for VS Code

A client for the compiler's language server, `nazm lsp`, and nothing else: every answer an editor
shows — diagnostics, definition, references, hover, completion, signature help, rename, document
and workspace symbols, call hierarchy, quick fixes, semantic tokens — is computed by the compiler,
across a package's dependencies as `nazm check` sees them. Set `nazm.path` if `nazm` is not on
`PATH`.

**Debugging** goes through an existing LLDB debug adapter (for example CodeLLDB): build with
`nazm build --debug`, which carries DWARF line tables and Nazm function names (N48, N58), and launch
the executable. `launch.example.json` and `tasks.example.json` hold a launch configuration and the build task for
`.vscode/launch.json` and `.vscode/tasks.json`. There is no Nazm debug adapter; a breakpoint is a
Nazm function or a `.nz` line.

Not published to a marketplace. To use it: `npm install` here, then load this folder as an
extension (`code --extensionDevelopmentPath=editors/vscode`).
