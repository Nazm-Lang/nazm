//! `nazm doctor` — whether this machine has what a native build needs (W2).
//!
//! `nazm run --interpret`, `check`, `fmt` and the rest need nothing beside `nazm`. `nazm build`,
//! and `run` and `test` without `--interpret`, hand the LLVM IR they generate to `clang`, which
//! assembles it and links through the platform's linker; a library build archives with `ar`, and
//! a debug build on macOS gathers its debug information with `dsymutil`. Each is asked here the
//! way a build asks it — the driver with the build's own environment ([`crate::backend::PASSED`])
//! — and a program is actually linked, so "found" means "a build would work", not "a file of that
//! name exists". What is missing is named, with where to get it on this platform.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::backend::{driver_command, DRIVER};
use crate::exit;

/// One tool's answer.
struct Row {
    tool: &'static str,
    /// What it is for, said when it is missing.
    needed_for: &'static str,
    /// `Ok(what it said)` when it works, `Err(why not)` when it does not.
    answer: Result<String, String>,
    /// Whether a missing one stops `nazm build` itself, or only one kind of build.
    required: bool,
}

pub(crate) fn run() -> ExitCode {
    let os = std::env::consts::OS;
    println!(
        "nazm {} on {os} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH
    );
    let driver = clang(os);
    let linkable = driver.answer.is_ok();
    let mut rows = vec![driver];
    if linkable {
        rows.push(link());
    }
    // No `ar`: nazm writes its static archives itself (`crate::archive`).
    if os == "macos" {
        rows.push(present(
            "dsymutil",
            &["--version"],
            "a debug build's debug information (`--debug`)",
        ));
    }
    let width = rows.iter().map(|r| r.tool.len()).max().unwrap_or(0);
    for r in &rows {
        match &r.answer {
            Ok(said) => println!("{:width$}  ok       {said}", r.tool),
            Err(why) => println!(
                "{:width$}  MISSING  {why} — needed for {}",
                r.tool, r.needed_for
            ),
        }
    }
    let missing_required = rows.iter().any(|r| r.required && r.answer.is_err());
    let missing = rows.iter().any(|r| r.answer.is_err());
    if missing {
        println!("{}", advice(os));
    }
    if missing_required {
        println!(
            "`nazm build` cannot make an executable here yet; `nazm run --interpret`, `check`, \
             `fmt` and `test --interpret` need none of this."
        );
        ExitCode::from(exit::TOOLCHAIN_FAILED)
    } else {
        println!("`nazm build` has what it needs here.");
        ExitCode::from(exit::OK)
    }
}

/// The driver: found on `PATH`, its version, and the target it builds for by default — which on
/// Windows must be MinGW-w64's (`*-windows-gnu`): Microsoft's (`*-windows-msvc`) needs its SDK and
/// C runtime, which Nazm does not use.
fn clang(os: &str) -> Row {
    let mut row = Row {
        tool: DRIVER,
        needed_for: "`nazm build`, and `nazm run` and `nazm test` without `--interpret`",
        answer: Err(String::new()),
        required: true,
    };
    let version = driver_command().arg("--version").output();
    row.answer = match version {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("`{DRIVER}` was not found on PATH"))
        }
        Err(e) => Err(format!("`{DRIVER}` could not be started: {e}")),
        Ok(o) if !o.status.success() => Err(format!("`{DRIVER} --version` failed")),
        Ok(o) => {
            let said = String::from_utf8_lossy(&o.stdout);
            let banner = said.lines().next().unwrap_or("").trim().to_owned();
            let target = driver_command()
                .arg("-dumpmachine")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default();
            if os == "windows" && target.contains("msvc") {
                Err(format!(
                    "{banner} builds for {target}, Microsoft's toolchain; Nazm on Windows uses \
                     MinGW-w64's (a clang whose target ends in `-windows-gnu`)"
                ))
            } else {
                Ok(format!("{banner} (target {target})"))
            }
        }
    };
    row
}

/// A program linked from LLVM IR, as `nazm build` links one: the linker and the C runtime's
/// start-up files and libraries are all there.
fn link() -> Row {
    let answer = (|| {
        let dir = crate::scratch::temporary("nazm-doctor-")
            .map_err(|e| format!("no temporary directory: {e}"))?;
        let result = link_in(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        result
    })();
    Row {
        tool: "linker",
        needed_for: "every executable `nazm build` writes",
        answer,
        required: true,
    }
}

fn link_in(dir: &Path) -> Result<String, String> {
    let ll = dir.join("probe.ll");
    let exe = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    std::fs::write(&ll, "define i32 @main() {\n  ret i32 0\n}\n")
        .map_err(|e| format!("cannot write {}: {e}", ll.display()))?;
    let out = driver_command()
        .current_dir(dir)
        .args(["-x", "ir", "-Wno-override-module", "-v"])
        .arg(&ll)
        .arg("-o")
        .arg(&exe)
        .output()
        .map_err(|e| format!("`{DRIVER}` could not be started: {e}"))?;
    let said = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() || !exe.exists() {
        let first = said
            .lines()
            .rev()
            .find(|l| l.contains("error") || l.contains("not found") || l.contains("cannot"))
            .unwrap_or("the link failed");
        return Err(format!(
            "`{DRIVER}` could not link a program: {}",
            first.trim()
        ));
    }
    // The linker `-v` reported running: its first word, as the driver resolved it.
    let linker = said
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('"'))
        .and_then(|l| l.trim_start().split('"').nth(1))
        .map(|p| {
            Path::new(p)
                .file_name()
                .map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "the driver's linker".to_owned());
    Ok(format!("links a program through `{linker}`"))
}

/// A tool found by starting it: its first line of output when it says one.
fn present(tool: &'static str, args: &[&str], needed_for: &'static str) -> Row {
    let answer = match Command::new(tool).args(args).output() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("`{tool}` was not found on PATH"))
        }
        Err(e) => Err(format!("`{tool}` could not be started: {e}")),
        Ok(o) => {
            let text = if o.stdout.is_empty() {
                &o.stderr
            } else {
                &o.stdout
            };
            // The line naming a version, else the first: LLVM's tools open with a banner.
            let text = String::from_utf8_lossy(text);
            let line = text
                .lines()
                .find(|l| l.contains("version"))
                .or_else(|| text.lines().next())
                .unwrap_or("")
                .trim()
                .to_owned();
            Ok(if line.is_empty() || !o.status.success() {
                "found".to_owned()
            } else {
                line
            })
        }
    };
    Row {
        tool,
        needed_for,
        answer,
        required: false,
    }
}

/// Where this platform's tools come from.
pub(crate) fn advice(os: &str) -> &'static str {
    match os {
        "macos" => {
            "On macOS: `xcode-select --install` (Apple's command line tools give clang, \
                    the linker, ar and dsymutil), or Homebrew's `llvm` on PATH."
        }
        "windows" => {
            "On Windows: unpack llvm-mingw's `ucrt-x86_64` release \
                      (https://github.com/mstorsjo/llvm-mingw/releases, no installer and no \
                      administrator) and put its `bin` directory on PATH; it gives clang, \
                      lld and ar. Microsoft's SDK is not used."
        }
        _ => {
            "Install your distribution's clang and lld (or binutils) packages and put them on \
              PATH."
        }
    }
}
