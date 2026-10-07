//! The release examples and the getting-started page, held to what they say (R1).
//!
//! `examples/tour/` is what a new reader runs first and `docs/getting-started.md` is what they
//! read first, so a tour program that no longer checks, a command whose output changed, or two
//! backends that disagree is a broken first impression — and nothing else in the suite would
//! notice. Every tour program is checked, formatted, interpreted, and built and run under both
//! backends. Every `console` block of the page is run, command by command, in a scratch
//! directory holding the files the page itself defines.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every tour program and exactly what it prints: its own output, then `main`'s value.
const TOUR: &[(&str, &str)] = &[
    ("hello.nz", "Hello, Nazm!\n0\n"),
    ("result-option.nz", "49\n"),
    ("records-enums.nz", "75\n"),
    ("generics.nz", "131\n"),
    ("closures.nz", "47\n"),
    ("traits.nz", "62\n"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn nazm(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(NAZM)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run nazm")
}

#[test]
fn every_tour_program_is_listed_here() {
    let dir = repo_root().join("examples/tour");
    let present: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("examples/tour")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".nz"))
        .collect();
    let listed: BTreeSet<String> = TOUR.iter().map(|(f, _)| (*f).to_owned()).collect();
    assert_eq!(
        present, listed,
        "examples/tour and TOUR disagree: a program here is run by nothing, or a listed one is gone"
    );
}

#[test]
fn every_tour_program_checks_formats_runs_and_builds_alike_under_both_backends() {
    let dir = repo_root().join("examples/tour");
    let scratch = tempfile::tempdir().expect("scratch");
    for (file, expected) in TOUR {
        let src = dir.join(file);
        let src = src.to_str().unwrap();

        let out = nazm(&["check", src], &dir);
        assert_eq!(out.status.code(), Some(0), "check {file}: {out:?}");

        let out = nazm(&["fmt", "--check", src], &dir);
        assert_eq!(out.status.code(), Some(0), "fmt --check {file}: {out:?}");

        let out = nazm(&["run", src], &dir);
        assert_eq!(out.status.code(), Some(0), "run {file}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            *expected,
            "run {file}"
        );

        for backend in ["llvm", "cranelift"] {
            let exe = scratch.path().join(format!("{file}.{backend}"));
            let exe_s = exe.to_str().unwrap();
            let out = nazm(&["build", src, "-o", exe_s, "--backend", backend], &dir);
            assert_eq!(
                out.status.code(),
                Some(0),
                "build {file} ({backend}): {out:?}"
            );
            let ran = Command::new(&exe).output().expect("run the executable");
            assert_eq!(ran.status.code(), Some(0), "{file} ({backend}): {ran:?}");
            assert_eq!(
                String::from_utf8_lossy(&ran.stdout),
                *expected,
                "{file} built by {backend}"
            );
        }
    }
}

/// One command of a `console` block: what is typed, whether it is meant to fail, and what the
/// terminal shows — standard output, then standard error.
struct Step {
    command: String,
    fails: bool,
    shows: String,
}

/// The page, read as instructions: `<!-- file: PATH -->` before a block writes that block to
/// `PATH`; `<!-- run -->` before a `console` block runs it.
fn read_page(text: &str) -> (Vec<(String, String)>, Vec<Vec<Step>>, usize) {
    let mut files = Vec::new();
    let mut runs = Vec::new();
    let mut consoles = 0;
    let mut pending_file: Option<String> = None;
    let mut pending_run = false;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if let Some(path) = t
            .strip_prefix("<!-- file:")
            .and_then(|r| r.strip_suffix("-->"))
        {
            pending_file = Some(path.trim().to_owned());
            continue;
        }
        if t == "<!-- run -->" {
            pending_run = true;
            continue;
        }
        let Some(lang) = t.strip_prefix("```") else {
            continue;
        };
        let mut body = Vec::new();
        for l in lines.by_ref() {
            if l.trim() == "```" {
                break;
            }
            body.push(l);
        }
        if lang == "console" {
            consoles += 1;
        }
        if let Some(path) = pending_file.take() {
            files.push((path, body.iter().map(|l| format!("{l}\n")).collect()));
        }
        if std::mem::take(&mut pending_run) {
            assert_eq!(lang, "console", "`<!-- run -->` before a ```{lang} block");
            let mut steps: Vec<Step> = Vec::new();
            for l in body {
                if let Some(cmd) = l.strip_prefix("$ ") {
                    let (command, fails) = match cmd.strip_suffix("# fails") {
                        Some(c) => (c.trim_end().to_owned(), true),
                        None => (cmd.to_owned(), false),
                    };
                    steps.push(Step {
                        command,
                        fails,
                        shows: String::new(),
                    });
                } else {
                    let step = steps.last_mut().expect("output before any command");
                    step.shows.push_str(l);
                    step.shows.push('\n');
                }
            }
            runs.push(steps);
        }
    }
    (files, runs, consoles)
}

/// Every file the page defines, written into a scratch directory, and every command of its
/// `<!-- run -->` blocks run there in order: the exit status and what it printed must be what
/// the page shows.
fn run_page(page: &str, min_runs: usize, min_commands: usize) {
    let text = std::fs::read_to_string(repo_root().join(page)).expect(page);
    let (files, runs, consoles) = read_page(&text);
    assert_eq!(
        runs.len(),
        consoles,
        "{page}: every console block must be marked `<!-- run -->`, so none goes unchecked"
    );
    assert!(runs.len() >= min_runs, "{page} lost its runnable steps");

    let scratch = tempfile::tempdir().expect("scratch");
    for (path, text) in &files {
        let p = scratch.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).expect("mkdir");
        std::fs::write(&p, text).expect("write");
    }
    let mut ran = 0;
    for step in runs.iter().flatten() {
        // `nazm` is this build's binary, wherever it appears in the command line.
        let command = step.command.replace("nazm ", &format!("'{NAZM}' "));
        let out = Command::new("sh")
            .args(["-c", &command])
            .current_dir(scratch.path())
            .output()
            .expect("sh");
        let shows = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            out.status.success(),
            !step.fails,
            "{page}: `{}` {} — it printed:\n{shows}",
            step.command,
            if step.fails {
                "was meant to fail and succeeded"
            } else {
                "failed"
            }
        );
        assert_eq!(
            shows, step.shows,
            "{page}: `{}` printed something else",
            step.command
        );
        ran += 1;
    }
    assert!(ran >= min_commands, "{page}: only {ran} commands ran");
}

#[test]
fn the_getting_started_page_runs_as_written() {
    run_page("docs/getting-started.md", 6, 15);
}

#[test]
fn the_readme_example_runs_as_written() {
    run_page("README.md", 1, 2);
}
