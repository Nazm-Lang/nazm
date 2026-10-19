//! Gate 2: the reference applications (`examples/apps/`, `docs/general-purpose.md` §24).
//!
//! Five programs that do ordinary work — a command-line counter, a concurrent server, a JSON
//! pipeline, a numeric simulation and a persistent store — each run under the interpreter and
//! built by LLVM at `-O0` and `-O2` and by Cranelift, with real arguments, files and standard
//! input, and held to the output recorded beside it (`NAME.expected`) and its exit status.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{run_bounded, Ran};
use differential::{nazm, NAZM};

fn app_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/apps")
        .join(name)
}

/// The app copied into a fresh directory, so a run that writes leaves the repository alone.
fn staged(name: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(app_dir(name)).unwrap().flatten() {
        if e.path().is_file() {
            std::fs::copy(e.path(), d.path().join(e.file_name())).unwrap();
        }
    }
    d
}

/// `sh -c` running `program…` with standard input from `stdin`, when there is one.
fn shell(dir: &Path, program: &[String], args: &[String], stdin: Option<&str>) -> Ran {
    // Windows has no `sh` of its own: the program is started directly, its input the file.
    if cfg!(windows) {
        let mut c = Command::new(&program[0]);
        c.current_dir(dir).args(&program[1..]).args(args);
        let input = stdin.map_or_else(std::process::Stdio::null, |f| {
            std::fs::File::open(dir.join(f)).unwrap().into()
        });
        let what = program.join(" ");
        return common::run_within_stdin(&mut c, common::DEADLINE, input)
            .unwrap_or_else(|| panic!("{what} did not terminate within {:?}", common::DEADLINE));
    }
    let mut line = String::from("exec");
    for w in program.iter().chain(args) {
        line.push_str(" '");
        line.push_str(&w.replace('\'', r"'\''"));
        line.push('\'');
    }
    if let Some(f) = stdin {
        line.push_str(&format!(" < '{f}'"));
    }
    let mut c = Command::new("sh");
    c.current_dir(dir).arg("-c").arg(line);
    run_bounded(&mut c, &program.join(" "))
}

/// `file` under the interpreter and each backend, with `args(label)`: one run per implementation.
fn every_way(
    dir: &Path,
    file: &str,
    args: &dyn Fn(&str) -> Vec<String>,
    stdin: Option<&str>,
) -> Vec<(String, Ran)> {
    let mut out = vec![(
        "interpreter".to_owned(),
        shell(
            dir,
            &[
                NAZM.to_owned(),
                "run".to_owned(),
                "--interpret".to_owned(),
                file.to_owned(),
                "--".to_owned(),
            ],
            &args("interpreter"),
            stdin,
        ),
    )];
    for (label, backend, opt) in [
        ("llvm-O0", "llvm", "0"),
        ("llvm-O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(crate::common::exe_name(format!("exe-{label}")));
        let b = nazm(
            dir,
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                file,
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            shell(dir, &[exe.display().to_string()], &args(label), stdin),
        ));
    }
    out
}

fn expected(name: &str, which: &str) -> String {
    std::fs::read_to_string(app_dir(name).join(which))
        .unwrap_or_else(|e| panic!("{name}/{which}: {e}"))
}

fn hold(runs: &[(String, Ran)], stdout: &str, code: i32) {
    for (label, r) in runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, stdout, "{label}\n{}", r.stderr);
    }
}

fn words(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn nwc_counts_files_flags_and_standard_input_as_wc_does() {
    let d = staged("nwc");
    let files = every_way(d.path(), "nwc.nz", &|_| words(&["a.txt", "b.txt"]), None);
    hold(&files, &expected("nwc", "files.expected"), 0);
    let flags = every_way(d.path(), "nwc.nz", &|_| words(&["-l", "-c", "a.txt"]), None);
    hold(&flags, &expected("nwc", "flags.expected"), 0);
    let stdin = every_way(d.path(), "nwc.nz", &|_| Vec::new(), Some("a.txt"));
    hold(&stdin, &expected("nwc", "stdin.expected"), 0);
    let missing = every_way(
        d.path(),
        "nwc.nz",
        &|_| words(&["a.txt", "no-such-file"]),
        None,
    );
    for (label, r) in &missing {
        assert_eq!(r.code, Some(1), "{label}: {}", r.stderr);
        assert!(
            r.stderr.contains("nwc: no-such-file: ") && r.stderr.contains("(NotFound)"),
            "{label}: {}",
            r.stderr
        );
        assert!(
            r.stdout.starts_with("       4      11      63 a.txt\n"),
            "{label}: {}",
            r.stdout
        );
    }
}

#[test]
fn jsonpipe_filters_and_totals_records_and_names_every_rejected_line() {
    let d = staged("jsonpipe");
    let all = every_way(
        d.path(),
        "jsonpipe.nz",
        &|_| words(&["access-1.jsonl", "access-2.jsonl"]),
        None,
    );
    hold(&all, &expected("jsonpipe", "all.expected"), 0);
    for (label, r) in &all {
        assert_eq!(
            r.stderr
                .lines()
                .filter(|l| l.starts_with("access-"))
                .count(),
            3,
            "{label}: {}",
            r.stderr
        );
    }
    let errors = every_way(
        d.path(),
        "jsonpipe.nz",
        &|_| words(&["--min-status", "400", "access-1.jsonl", "access-2.jsonl"]),
        None,
    );
    hold(&errors, &expected("jsonpipe", "errors.expected"), 0);
}

#[test]
fn nbody_conserves_energy_to_the_same_nine_digits_everywhere() {
    let d = staged("nbody");
    let runs = every_way(d.path(), "nbody.nz", &|_| words(&["1000"]), None);
    hold(&runs, &expected("nbody", "nbody.expected"), 0);
}

#[test]
fn kvstore_survives_a_reopen_and_a_second_process_and_compacts_atomically() {
    let d = staged("kvstore");
    // A store of each implementation's own, so no run reads another's.
    let first = every_way(
        d.path(),
        "kvstore.nz",
        &|label| vec![format!("store-{label}"), "script.txt".to_owned()],
        None,
    );
    hold(&first, &expected("kvstore", "script.expected"), 0);
    let second = every_way(
        d.path(),
        "kvstore.nz",
        &|label| vec![format!("store-{label}"), "again.txt".to_owned()],
        None,
    );
    hold(&second, &expected("kvstore", "again.expected"), 0);
    for (label, _) in &second {
        let log =
            std::fs::read_to_string(d.path().join(format!("store-{label}/store.log"))).unwrap();
        assert_eq!(
            log, "S\tcolour\tgreen\nS\tgreeting\thello, world\nS\tcount\t1\n",
            "{label}"
        );
        let leftovers: Vec<_> = std::fs::read_dir(d.path().join(format!("store-{label}")))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "{label}: compaction left {leftovers:?}");
    }
}

#[test]
fn kvd_serves_concurrent_clients_from_one_owner_task() {
    let d = staged("kvd");
    let runs = every_way(d.path(), "kvd.nz", &|_| Vec::new(), None);
    hold(&runs, &expected("kvd", "kvd.expected"), 0);
}
