//! `nazm init DIR --template T` (N74, `docs/architecture.md` §7.76): a project a fresh user can
//! run at once.
//!
//! Six templates, each written only from what the toolchain supports and each held by its own
//! commands in `tests/init.rs`. The files are compiled into the binary from `templates/`, so an
//! installed `nazm` needs nothing beside it; `{{name}}` in a file becomes the package's name, the
//! directory's. A directory that exists and is not empty is refused, and nothing is overwritten.

use std::path::Path;
use std::process::ExitCode;

use crate::exit;

/// Each template's name and what it is.
pub(crate) const TEMPLATES: &[(&str, &str)] = &[
    (
        "cli",
        "a command-line program with `IoCap`, a module and a test",
    ),
    (
        "library",
        "a package with no `main`: public functions and a test",
    ),
    (
        "server",
        "a worker pool over channels serving the requests in a file",
    ),
    (
        "embedded",
        "a freestanding `aarch64-unknown-none` program reaching a register with `MmioCap`",
    ),
    (
        "wasm",
        "a `wasm32-unknown-unknown` module whose authority is its imports",
    ),
    (
        "contract",
        "a contract under the `web3` profile, with transactions to run",
    ),
];

/// `(template, path, text)` for every file of every template.
const FILES: &[(&str, &str, &str)] = &[
    (
        "cli",
        "README.md",
        include_str!("../templates/cli/README.md"),
    ),
    (
        "cli",
        "nazm.toml",
        include_str!("../templates/cli/nazm.toml"),
    ),
    (
        "cli",
        "src/greet.nz",
        include_str!("../templates/cli/src/greet.nz"),
    ),
    (
        "cli",
        "src/main.nz",
        include_str!("../templates/cli/src/main.nz"),
    ),
    (
        "cli",
        "tests/greet.expected",
        include_str!("../templates/cli/tests/greet.expected"),
    ),
    (
        "cli",
        "tests/greet.nz",
        include_str!("../templates/cli/tests/greet.nz"),
    ),
    (
        "library",
        "README.md",
        include_str!("../templates/library/README.md"),
    ),
    (
        "library",
        "nazm.toml",
        include_str!("../templates/library/nazm.toml"),
    ),
    (
        "library",
        "src/lib.nz",
        include_str!("../templates/library/src/lib.nz"),
    ),
    (
        "library",
        "tests/lib.expected",
        include_str!("../templates/library/tests/lib.expected"),
    ),
    (
        "library",
        "tests/lib.nz",
        include_str!("../templates/library/tests/lib.nz"),
    ),
    (
        "server",
        "README.md",
        include_str!("../templates/server/README.md"),
    ),
    (
        "server",
        "nazm.toml",
        include_str!("../templates/server/nazm.toml"),
    ),
    (
        "server",
        "requests.txt",
        include_str!("../templates/server/requests.txt"),
    ),
    (
        "server",
        "src/handle.nz",
        include_str!("../templates/server/src/handle.nz"),
    ),
    (
        "server",
        "src/main.nz",
        include_str!("../templates/server/src/main.nz"),
    ),
    (
        "server",
        "tests/handle.expected",
        include_str!("../templates/server/tests/handle.expected"),
    ),
    (
        "server",
        "tests/handle.nz",
        include_str!("../templates/server/tests/handle.nz"),
    ),
    (
        "embedded",
        "README.md",
        include_str!("../templates/embedded/README.md"),
    ),
    (
        "embedded",
        "nazm.toml",
        include_str!("../templates/embedded/nazm.toml"),
    ),
    (
        "embedded",
        "src/board.nz",
        include_str!("../templates/embedded/src/board.nz"),
    ),
    (
        "embedded",
        "src/main.nz",
        include_str!("../templates/embedded/src/main.nz"),
    ),
    (
        "embedded",
        "tests/board.expected",
        include_str!("../templates/embedded/tests/board.expected"),
    ),
    (
        "embedded",
        "tests/board.nz",
        include_str!("../templates/embedded/tests/board.nz"),
    ),
    (
        "wasm",
        "README.md",
        include_str!("../templates/wasm/README.md"),
    ),
    (
        "wasm",
        "nazm.toml",
        include_str!("../templates/wasm/nazm.toml"),
    ),
    (
        "wasm",
        "src/fib.nz",
        include_str!("../templates/wasm/src/fib.nz"),
    ),
    (
        "wasm",
        "src/main.nz",
        include_str!("../templates/wasm/src/main.nz"),
    ),
    (
        "wasm",
        "tests/fib.expected",
        include_str!("../templates/wasm/tests/fib.expected"),
    ),
    (
        "wasm",
        "tests/fib.nz",
        include_str!("../templates/wasm/tests/fib.nz"),
    ),
    (
        "contract",
        "README.md",
        include_str!("../templates/contract/README.md"),
    ),
    (
        "contract",
        "nazm.toml",
        include_str!("../templates/contract/nazm.toml"),
    ),
    (
        "contract",
        "src/main.nz",
        include_str!("../templates/contract/src/main.nz"),
    ),
    (
        "contract",
        "tests/transfer.expected",
        include_str!("../templates/contract/tests/transfer.expected"),
    ),
    (
        "contract",
        "tests/transfer.nz",
        include_str!("../templates/contract/tests/transfer.nz"),
    ),
    (
        "contract",
        "txs.txt",
        include_str!("../templates/contract/txs.txt"),
    ),
];

pub(crate) fn run(dir: &Path, template: &str) -> ExitCode {
    if !TEMPLATES.iter().any(|(t, _)| *t == template) {
        let names: Vec<&str> = TEMPLATES.iter().map(|(t, _)| *t).collect();
        eprintln!(
            "nazm: no template `{template}`; there are {}",
            names.join(", ")
        );
        return ExitCode::from(exit::NO_SUCH_FILE);
    }
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        eprintln!("nazm: `{}` names no directory to create", dir.display());
        return ExitCode::from(exit::NO_SUCH_FILE);
    };
    if !nazm_package::valid_name(name) {
        eprintln!(
            "nazm: `{name}` is not a package name: lowercase ASCII letters, digits and `-`, \
             starting with a letter — the directory's name is the package's"
        );
        return ExitCode::from(exit::NO_SUCH_FILE);
    }
    if std::fs::read_dir(dir).is_ok_and(|mut e| e.next().is_some()) {
        eprintln!(
            "nazm: `{}` exists and is not empty; nothing was written",
            dir.display()
        );
        return ExitCode::from(exit::NO_SUCH_FILE);
    }
    let mut written = 0;
    for (_, path, text) in FILES.iter().filter(|(t, _, _)| *t == template) {
        let to = dir.join(path);
        let made = to
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&to, text.replace("{{name}}", name)));
        if let Err(e) = made {
            eprintln!("nazm: cannot write {}: {e}", to.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
        written += 1;
    }
    eprintln!(
        "nazm: created `{name}` from the `{template}` template ({written} files); its README.md says \
         what to run"
    );
    ExitCode::from(exit::OK)
}
