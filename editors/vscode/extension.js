// The whole client: start `nazm lsp` on standard input and output and let the protocol do the
// rest. Diagnostics, definition, references, hover, completion, signature help, rename, symbols,
// call hierarchy, quick fixes and semantic tokens are all the compiler's (`docs/internal/architecture.md`
// §7.76); nothing here knows the language.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const command = vscode.workspace.getConfiguration("nazm").get("path", "nazm");
  const server = { command, args: ["lsp"] };
  client = new LanguageClient(
    "nazm",
    "Nazm",
    { run: server, debug: server },
    { documentSelector: [{ scheme: "file", language: "nazm" }] }
  );
  context.subscriptions.push({ dispose: () => client && client.stop() });
  return client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
