//! `cargo xtask workspace-tests`: the host-safe workspace test run.

use std::process::ExitCode;

use crate::checks::workspace_root;
use crate::{campaign, procs, suite};

/// `xtask workspace-tests --jobs J --workers W --test-threads T --shape NAME`: every test
/// binary `cargo test --workspace` would run, built by Cargo and named by its JSON messages,
/// each run once over W workers, longest first by the timing history for NAME; then the
/// doc-tests. The report is in canonical order and a failure cancels nothing. N32-H3.
///
/// `--host-safe` (R1) builds every binary and runs every one except those that build and run
/// generated compiler stages ([`suite::needs_containment`]), naming each it leaves out. It is
/// what may run where no contained runner exists — GitHub's CI — and it is a subset: the
/// stages it leaves out run in `cargo xtask contained tests` and the release gate, never
/// uncontained, and the guard that refuses them is not weakened.
pub(crate) fn workspace_tests(rest: &[String]) -> ExitCode {
    let value = |flag: &str, default: &str| -> String {
        rest.iter()
            .position(|a| a == flag)
            .and_then(|i| rest.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let num = |flag: &str, default: u32| {
        value(flag, &default.to_string())
            .parse::<u32>()
            .unwrap_or(default)
    };
    let (jobs, workers, threads) = (
        num("--jobs", 1),
        num("--workers", 1),
        num("--test-threads", 1),
    );
    let shape = value("--shape", "default");
    let host_safe = rest.iter().any(|a| a == "--host-safe");
    let Some(root) = workspace_root() else {
        eprintln!("workspace-tests: could not locate the workspace root");
        return ExitCode::FAILURE;
    };
    let started = std::time::Instant::now();
    let build = procs::run(&procs::Invocation {
        program: campaign::cargo(),
        args: [
            "test",
            "--locked",
            "--workspace",
            "--no-run",
            "--message-format=json",
            "--jobs",
            &jobs.to_string(),
        ]
        .map(str::to_owned)
        .to_vec(),
        cwd: root.clone(),
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(2400),
    });
    if !matches!(&build.ended, procs::Ended::Exited(s) if s.success()) {
        eprint!("{}", build.stderr);
        eprintln!(
            "workspace-tests: the suite did not build ({:?})",
            build.ended
        );
        return ExitCode::FAILURE;
    }
    let built = started.elapsed().as_secs_f64();
    let (bins, contained_only): (Vec<suite::Binary>, Vec<suite::Binary>) =
        suite::binaries(&build.stdout)
            .into_iter()
            .partition(|b| !(host_safe && suite::needs_containment(b)));
    for b in &contained_only {
        eprintln!(
            "workspace-tests: NOT RUN {} — it builds generated compiler stages, which run only \
             under `cargo xtask contained`",
            b.id()
        );
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), std::path::PathBuf::from);
    let history_file = suite::history_path(&target);
    let history = suite::read_history(&history_file, &shape);
    let order = suite::schedule(&bins, &history);
    let t0 = std::time::Instant::now();
    let results = suite::execute(&bins, &order, workers as usize, |b| {
        suite::run_binary(b, threads, std::time::Duration::from_secs(1800))
    });
    let ran = t0.elapsed().as_secs_f64();

    let mut failed = Vec::new();
    let mut now = std::collections::BTreeMap::new();
    for (b, r) in bins.iter().zip(&results) {
        eprintln!("     Running {} ({})", b.id(), b.executable.display());
        match r {
            Some(r) => {
                print!("{}", r.stdout);
                eprint!("{}", r.stderr);
                now.insert(b.id(), r.secs);
                if !r.passed() {
                    failed.push(format!("{} ({:?})", b.id(), r.ended));
                }
            }
            None => failed.push(format!("{} (did not run)", b.id())),
        }
    }
    suite::write_history(&history_file, &shape, &now);
    let doc = procs::run(&procs::Invocation {
        program: campaign::cargo(),
        args: vec![
            "test".to_owned(),
            "--locked".to_owned(),
            "--workspace".to_owned(),
            "--doc".to_owned(),
            "--jobs".to_owned(),
            jobs.to_string(),
            "--".to_owned(),
            "--test-threads".to_owned(),
            threads.to_string(),
        ],
        cwd: root,
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(1200),
    });
    print!("{}", doc.stdout);
    eprint!("{}", doc.stderr);
    if !matches!(&doc.ended, procs::Ended::Exited(s) if s.success()) {
        failed.push(format!("doc-tests ({:?})", doc.ended));
    }
    let mut slowest: Vec<(&String, &f64)> = now.iter().collect();
    slowest.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
    eprintln!(
        "workspace-tests: {} binaries over {workers} worker(s) × {threads} thread(s): build {built:.1}s, run {ran:.1}s, doc-tests {:.1}s; history {}",
        bins.len(),
        doc.elapsed.as_secs_f64(),
        if history.is_empty() { "none (canonical order)" } else { "used" }
    );
    if host_safe {
        eprintln!(
            "workspace-tests: host-safe subset — {} binar{} not run here; this is not the whole suite",
            contained_only.len(),
            if contained_only.len() == 1 { "y" } else { "ies" }
        );
    }
    for (id, secs) in slowest.iter().take(10) {
        eprintln!("workspace-tests: slowest {secs:7.2}s {id}");
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        for f in &failed {
            eprintln!("workspace-tests: FAILED {f}");
        }
        ExitCode::FAILURE
    }
}
