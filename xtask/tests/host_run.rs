//! R6, task 7.2: `cargo xtask host-run` — the host runner for one bounded-heap stage
//! (`docs/internal/runbook.md`, *What may run on the host*). Each refusal is exercised by name, and a passing
//! run, a deadline and the withdrawal condition are exercised end to end.
//!
//! Host-safe: the programs the runner is given are either small shell scripts that carry the two
//! things the runner reads (the symbol `nz.heap_malloc` and the `N0406` message with a limit), or a
//! small program built by the *reference* compiler with a small `--heap-limit`. No Nazm-built
//! compiler stage is run. Every run is a child process whose environment this test builds, with its
//! lock and marker in a temporary directory.
//!
//! macOS only: the runner refuses anywhere else (`not_macos_is_refused_by_name` runs there).

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `xtask host-run ARGS` with the state in `state` and neither containment variable set.
fn host_run(state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("host-run")
        .args(args)
        .current_dir(root())
        .env("NAZM_HOST_RUN_DIR", state)
        .env_remove("NAZM_CONTAINED")
        .env_remove("NAZM_ALLOW_UNCONTAINED")
        .output()
        .unwrap()
}

/// A script that carries what the runner reads, then runs `body`.
fn script(dir: &Path, name: &str, symbol: bool, limit: Option<&str>, body: &str) -> PathBuf {
    let mut s = String::from("#!/bin/sh\n");
    if symbol {
        s.push_str("# _nz.heap_malloc\n");
    }
    if let Some(l) = limit {
        s.push_str(&format!(
            "# nazm: error[N0406]: out of memory: the heap limit of {l} bytes is reached\n"
        ));
    }
    s.push_str(body);
    let p = dir.join(name);
    fs::write(&p, s).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn refused_as(o: &Output, name: &str) {
    assert!(!o.status.success(), "{}", text(o));
    assert!(
        text(o).contains(&format!("refused [{name}]")),
        "wanted refusal {name}: {}",
        text(o)
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_stage_within_its_limit_runs_and_its_peak_is_recorded() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(
        d.path(),
        "ok",
        true,
        Some("4194304"),
        "echo the stage ran\n",
    );
    let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("the stage ran"), "{t}");
    assert!(
        t.contains("heap limit 4194304 bytes; peak resident "),
        "{t}"
    );
    assert!(!t.contains("peak resident unknown"), "{t}");
    assert!(!d.path().join("withdrawn").exists());
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn the_programs_exit_status_is_the_runners_and_its_report_is_not_on_stdout() {
    let d = tempfile::tempdir().unwrap();
    for code in [0, 1, 2, 7, 255] {
        let exe = script(
            d.path(),
            "status",
            true,
            Some("4194304"),
            &format!("echo out-line\necho err-line >&2\nexit {code}\n"),
        );
        let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(code), "{}", text(&o));
        // Standard output is the program's, byte for byte; the report is apart from it.
        assert_eq!(String::from_utf8_lossy(&o.stdout), "out-line\n");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.starts_with("err-line\n"), "{err}");
        assert!(
            err.contains("host-run: heap limit 4194304 bytes; peak resident "),
            "{err}"
        );
        assert!(err.contains(&format!("exit Some({code})")), "{err}");
        assert!(!d.path().join("withdrawn").exists());
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_report_file_leaves_standard_error_the_programs_alone() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(
        d.path(),
        "status",
        true,
        Some("4194304"),
        "echo out-line\necho err-line >&2\nexit 3\n",
    );
    let report = d.path().join("peaks.log");
    for _ in 0..2 {
        let o = host_run(
            d.path(),
            &[
                "--report",
                report.to_str().unwrap(),
                "--",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(o.status.code(), Some(3), "{}", text(&o));
        assert_eq!(String::from_utf8_lossy(&o.stdout), "out-line\n");
        assert_eq!(String::from_utf8_lossy(&o.stderr), "err-line\n");
    }
    // Appended, one line per run.
    let log = fs::read_to_string(&report).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(
        log.lines()
            .all(|l| l.contains("heap limit 4194304 bytes; peak resident ")
                && l.contains("exit Some(3)")),
        "{log}"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn output_is_passed_as_bytes_and_a_truncation_is_a_failure_of_its_own() {
    let d = tempfile::tempdir().unwrap();
    // Bytes that are not UTF-8 arrive as they were written.
    let raw = script(
        d.path(),
        "raw",
        true,
        Some("4194304"),
        "printf '\\377\\376ok\\n'\n",
    );
    let o = host_run(d.path(), &["--", raw.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(o.stdout, b"\xff\xfeok\n");
    // Output past the cap is dropped, and the run says so with a status that is not the program's.
    let big = script(
        d.path(),
        "big",
        true,
        Some("4194304"),
        "yes 0123456789abcdef | head -c 2097152\n",
    );
    let o = host_run(
        d.path(),
        &["--max-output-mib", "1", "--", big.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(126), "{}", text(&o));
    assert_eq!(o.stdout.len(), 1024 * 1024);
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("OUTPUT TRUNCATED at 1048576 bytes"),
        "{}",
        text(&o)
    );
    let o = host_run(
        d.path(),
        &["--max-output-mib", "3", "--", big.to_str().unwrap()],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(o.stdout.len(), 2 * 1024 * 1024);
    refused_as(
        &host_run(
            d.path(),
            &["--max-output-mib", "257", "--", big.to_str().unwrap()],
        ),
        "usage",
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_refusal_and_a_deadline_have_statuses_of_their_own() {
    let d = tempfile::tempdir().unwrap();
    let plain = script(d.path(), "plain", false, Some("4194304"), "exit 0\n");
    assert_eq!(
        host_run(d.path(), &["--", plain.to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
    let hangs = script(d.path(), "hangs", true, Some("4194304"), "sleep 30\n");
    let o = host_run(
        d.path(),
        &["--deadline-secs", "1", "--", hangs.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(124), "{}", text(&o));
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_program_without_the_counted_heap_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "plain", false, Some("4194304"), "echo ran\n");
    let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
    refused_as(&o, "no-counted-heap");
    assert!(!text(&o).contains("\nran"), "{}", text(&o));
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_program_with_no_limit_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "nolimit", true, None, "echo ran\n");
    refused_as(
        &host_run(d.path(), &["--", exe.to_str().unwrap()]),
        "no-heap-limit",
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_limit_over_two_gib_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "big", true, Some("2147483649"), "echo ran\n");
    refused_as(
        &host_run(d.path(), &["--", exe.to_str().unwrap()]),
        "heap-limit-too-large",
    );
    let at = script(d.path(), "edge", true, Some("2147483648"), "echo ran\n");
    assert!(
        host_run(d.path(), &["--", at.to_str().unwrap()])
            .status
            .success(),
        "exactly 2 GiB is allowed"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_missing_or_non_executable_file_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    refused_as(
        &host_run(d.path(), &["--", d.path().join("absent").to_str().unwrap()]),
        "no-executable",
    );
    let p = d.path().join("data");
    fs::write(&p, "# nz.heap_malloc the heap limit of 1 bytes is reached").unwrap();
    refused_as(
        &host_run(d.path(), &["--", p.to_str().unwrap()]),
        "no-executable",
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_second_stage_is_refused_while_one_runs() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "ok", true, Some("4194304"), "echo ran\n");
    // The first stage's lock, held as a running stage holds it.
    let held = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(d.path().join("lock"))
        .unwrap();
    held.try_lock().unwrap();
    refused_as(
        &host_run(d.path(), &["--", exe.to_str().unwrap()]),
        "another-stage-running",
    );
    drop(held);
    assert!(
        host_run(d.path(), &["--", exe.to_str().unwrap()])
            .status
            .success(),
        "the lock is free once the first stage ends"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_host_that_declares_containment_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "ok", true, Some("4194304"), "echo ran\n");
    for var in ["NAZM_CONTAINED", "NAZM_ALLOW_UNCONTAINED"] {
        let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["host-run", "--", exe.to_str().unwrap()])
            .current_dir(root())
            .env("NAZM_HOST_RUN_DIR", d.path())
            .env(var, "1")
            .output()
            .unwrap();
        refused_as(&o, "host-environment");
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_peak_past_the_limit_is_the_withdrawal_condition_and_later_runs_are_refused() {
    let d = tempfile::tempdir().unwrap();
    // A one-byte limit and no margin: any process's resident memory is past it.
    let exe = script(d.path(), "over", true, Some("1"), "echo ran\n");
    let o = host_run(
        d.path(),
        &["--margin-bytes", "0", "--", exe.to_str().unwrap()],
    );
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("WITHDRAWAL CONDITION"), "{}", text(&o));
    assert!(d.path().join("withdrawn").exists());

    let fine = script(d.path(), "fine", true, Some("4194304"), "echo ran\n");
    refused_as(
        &host_run(d.path(), &["--", fine.to_str().unwrap()]),
        "withdrawn",
    );
    fs::remove_file(d.path().join("withdrawn")).unwrap();
    assert!(host_run(d.path(), &["--", fine.to_str().unwrap()])
        .status
        .success());
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn an_n0406_that_does_not_stop_the_program_is_the_withdrawal_condition() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(
        d.path(),
        "ignores",
        true,
        Some("4194304"),
        "echo 'nazm: error[N0406]: out of memory' >&2\nexit 0\n",
    );
    let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("WITHDRAWAL CONDITION"), "{}", text(&o));
    assert!(d.path().join("withdrawn").exists());
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_stage_that_stops_with_n0406_and_status_2_is_not_a_withdrawal() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(
        d.path(),
        "stops",
        true,
        Some("4194304"),
        "echo 'nazm: error[N0406]: out of memory' >&2\nexit 2\n",
    );
    let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
    assert!(!o.status.success(), "the stage failed: {}", text(&o));
    assert!(!text(&o).contains("WITHDRAWAL"), "{}", text(&o));
    assert!(!d.path().join("withdrawn").exists());
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_stage_past_its_deadline_is_killed_with_its_children() {
    let d = tempfile::tempdir().unwrap();
    let pidfile = d.path().join("child.pid");
    let exe = script(
        d.path(),
        "hangs",
        true,
        Some("4194304"),
        &format!("sleep 30 &\necho $! > {}\nwait\n", pidfile.display()),
    );
    let o = host_run(
        d.path(),
        &["--deadline-secs", "2", "--", exe.to_str().unwrap()],
    );
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("DEADLINE reached"), "{}", text(&o));
    // The sleeper this test's stage started is gone: signal 0 finds no such process.
    let pid = fs::read_to_string(&pidfile).unwrap().trim().to_owned();
    let alive = Command::new("kill")
        .args(["-0", &pid])
        .output()
        .unwrap()
        .status
        .success();
    assert!(!alive, "the stage's child {pid} outlived its deadline");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the host runner is macOS only")]
fn a_program_built_with_a_small_heap_limit_runs() {
    let d = tempfile::tempdir().unwrap();
    // The reference compiler, built by Cargo from this checkout, builds the small program.
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf();
    let built = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "nazm-cli", "--bin", "nazm"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", text(&built));
    let nazm = target.join("debug").join("nazm");
    fs::write(
        d.path().join("prog.nz"),
        // It allocates, so the counted heap stays in the program (a program that never
        // allocates has none linked, and the runner refuses it as having no counted heap).
        "fn main(out: OutCap) -> Int ! {io} {\n    let xs = strs_new();\n    let mut i = 0;\n    \
         while i < 100 {\n        strs_push(xs, str_concat(\"item \", int_to_str(i)));\n        \
         i = i + 1;\n    }\n    print(f\"hello from a bounded heap {strs_len(xs)}\\n\");\n    0\n}\n",
    )
    .unwrap();
    let exe = d.path().join("prog");
    for limit in ["4M", "8M"] {
        let b = Command::new(&nazm)
            .args(["build", "--no-cache", "prog.nz", "-o"])
            .arg(&exe)
            .args(["--heap-limit", limit])
            .current_dir(d.path())
            .output()
            .unwrap();
        assert!(b.status.success(), "{}", text(&b));
        let o = host_run(d.path(), &["--", exe.to_str().unwrap()]);
        assert!(o.status.success(), "{limit}: {}", text(&o));
        let want = if limit == "4M" { 4_194_304 } else { 8_388_608 };
        assert!(
            text(&o).contains(&format!("heap limit {want} bytes; peak resident ")),
            "{}",
            text(&o)
        );
        assert!(
            text(&o).contains("hello from a bounded heap"),
            "{}",
            text(&o)
        );
    }
    // Built without a limit, the same program is refused.
    let b = Command::new(&nazm)
        .args(["build", "--no-cache", "prog.nz", "-o"])
        .arg(&exe)
        .current_dir(d.path())
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", text(&b));
    refused_as(
        &host_run(d.path(), &["--", exe.to_str().unwrap()]),
        "no-counted-heap",
    );
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "runs where the runner refuses: not macOS"
)]
fn not_macos_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    let exe = script(d.path(), "ok", true, Some("4194304"), "echo ran\n");
    refused_as(
        &host_run(d.path(), &["--", exe.to_str().unwrap()]),
        "not-macos",
    );
}
