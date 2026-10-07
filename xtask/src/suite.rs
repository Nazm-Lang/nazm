//! The workspace suite, scheduled: every test binary `cargo test --workspace` would run,
//! each run once, longest first, over a bounded number of workers. N32-H3.
//!
//! # What does not change
//!
//! The same binaries — Cargo builds them (`cargo test --workspace --no-run`) and names them
//! in its JSON messages, so Cargo stays the only authority on what is fresh; nothing here
//! guesses a path or runs a binary Cargo did not just report. Each runs with its package's
//! directory as its working directory and `CARGO_MANIFEST_DIR` and the `CARGO_PKG_*`
//! variables set, as `cargo test` runs it. Every one runs whatever another does: a failure
//! cancels nothing. The doc-tests follow, through `cargo test --doc`. The report is in the
//! binaries' canonical order — package, kind, target — never the order they finished in.
//!
//! # What does
//!
//! Only when each starts. `cargo test` runs binaries one after another, so the suite's wall
//! clock is the sum of theirs; here `workers` run at once, each with `test_threads`, and the
//! longest start first, which bounds the tail. How long each took last time is kept in a
//! timing history that decides the order and nothing else: without it the order is the
//! canonical one, and deleting it changes no test that runs and no outcome.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::procs::{self, Ended, Invocation};

/// One test binary, as Cargo reported it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Binary {
    pub(crate) package: String,
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) manifest_dir: PathBuf,
    pub(crate) executable: PathBuf,
    /// The target's root source file, as Cargo reported it.
    pub(crate) src_path: PathBuf,
}

impl Binary {
    /// Its identity for scheduling and reporting: no path.
    pub(crate) fn id(&self) -> String {
        format!("{} {}:{}", self.package, self.kind, self.target)
    }
}

/// Every test executable in `cargo test --no-run --message-format=json` output, in
/// canonical order, each once.
pub(crate) fn binaries(stdout: &str) -> Vec<Binary> {
    let mut out: Vec<Binary> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| {
            v.pointer("/reason").and_then(serde_json::Value::as_str) == Some("compiler-artifact")
                && v.pointer("/profile/test")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        })
        .filter_map(|v| {
            let text = |p: &str| {
                v.pointer(p)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            };
            let executable = PathBuf::from(text("/executable")?);
            let manifest = PathBuf::from(text("/manifest_path")?);
            let (package, version) = crate::campaign::package_of(&text("/package_id")?);
            Some(Binary {
                package,
                kind: text("/target/kind/0")?,
                target: text("/target/name")?,
                version,
                manifest_dir: manifest.parent()?.to_path_buf(),
                executable,
                src_path: PathBuf::from(text("/target/src_path").unwrap_or_default()),
            })
        })
        .collect();
    out.sort();
    out.dedup_by(|a, b| a.id() == b.id());
    out
}

/// Whether this binary builds and runs generated compiler stages, which only the contained
/// runner may do (`docs/runbook.md`, *Mandatory containment*).
///
/// Read from the guard itself rather than from a list: a target whose own source calls
/// `require_containment(` is one, so a new stage-building test is recognised the day it is
/// written and a test that stops building stages stops being excluded. A call reached only
/// through another file of the target is not seen here — that target then runs, reaches the
/// guard and fails loudly, which is the guard doing its job and never a silent pass.
pub(crate) fn needs_containment(b: &Binary) -> bool {
    std::fs::read_to_string(&b.src_path).is_ok_and(|text| {
        text.lines().any(|l| {
            let l = l.trim_start();
            !l.starts_with("//") && l.contains("require_containment(")
        })
    })
}

/// The order binaries start in: longest known duration first; a binary with no history is
/// treated as the longest, so the unknown are not left for the tail; ties and the unknown in
/// canonical order. Deterministic for a given history.
pub(crate) fn schedule(bins: &[Binary], history: &BTreeMap<String, f64>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..bins.len()).collect();
    order.sort_by(|&a, &b| {
        let key = |i: usize| bins.get(i).and_then(|x| history.get(&x.id()).copied());
        match (key(a), key(b)) {
            (None, None) => a.cmp(&b),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => y.total_cmp(&x).then(a.cmp(&b)),
        }
    });
    order
}

/// One binary's run.
#[derive(Debug, Clone)]
pub(crate) struct Ran {
    pub(crate) ended: Ended,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) secs: f64,
}

impl Ran {
    pub(crate) fn passed(&self) -> bool {
        matches!(&self.ended, Ended::Exited(s) if s.success())
    }
}

/// Run every binary once over `workers` workers, in `order`. Returns each binary's run, by
/// its index in `bins` — so the caller reports in canonical order whatever finished first.
/// `run` is how one binary is run; the real one is [`run_binary`].
pub(crate) fn execute<F>(
    bins: &[Binary],
    order: &[usize],
    workers: usize,
    run: F,
) -> Vec<Option<Ran>>
where
    F: Fn(&Binary) -> Ran + Send + Sync,
{
    let queue = Arc::new(Mutex::new(
        order.iter().copied().collect::<VecDeque<usize>>(),
    ));
    let results: Arc<Mutex<Vec<Option<Ran>>>> = Arc::new(Mutex::new(vec![None; bins.len()]));
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let run = &run;
            scope.spawn(move || loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some(i) = next else { break };
                let Some(b) = bins.get(i) else { continue };
                let r = run(b);
                if let Ok(mut all) = results.lock() {
                    if let Some(slot) = all.get_mut(i) {
                        *slot = Some(r);
                    }
                }
            });
        }
    });
    Arc::try_unwrap(results)
        .ok()
        .and_then(|m| m.into_inner().ok())
        .unwrap_or_default()
}

/// Run one binary as `cargo test` runs it: its package's directory, its package's
/// variables, the thread count.
pub(crate) fn run_binary(b: &Binary, test_threads: u32, deadline: Duration) -> Ran {
    let started = Instant::now();
    let f = procs::run(&Invocation {
        program: b.executable.display().to_string(),
        args: vec!["--test-threads".to_owned(), test_threads.to_string()],
        cwd: b.manifest_dir.clone(),
        env: vec![
            (
                "CARGO_MANIFEST_DIR".to_owned(),
                b.manifest_dir.display().to_string(),
            ),
            ("CARGO_PKG_NAME".to_owned(), b.package.clone()),
            ("CARGO_PKG_VERSION".to_owned(), b.version.clone()),
            ("CARGO_CRATE_NAME".to_owned(), b.target.replace('-', "_")),
        ],
        deadline,
    });
    Ran {
        ended: f.ended,
        stdout: f.stdout,
        stderr: f.stderr,
        secs: started.elapsed().as_secs_f64(),
    }
}

/// Where the timing history lives: the Cargo target the suite builds in, which in a
/// contained run is the harness-owned checkout cache — so `contained cache clear` removes it
/// with everything else, and it is never in the source tree.
pub(crate) fn history_path(target: &Path) -> PathBuf {
    target.join(".nazm-suite-timings.json")
}

/// The history, keyed by binary identity and the schedule it was measured under. Absent or
/// unreadable is empty.
pub(crate) fn read_history(path: &Path, shape: &str) -> BTreeMap<String, f64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, BTreeMap<String, f64>>>(&t).ok())
        .and_then(|mut all| all.remove(shape))
        .unwrap_or_default()
}

/// Record this run's durations under `shape`, and only it: the file holds the latest
/// measurement of one schedule over the binaries that exist now, so it is bounded by the
/// suite and never grows with history.
pub(crate) fn write_history(path: &Path, shape: &str, now: &BTreeMap<String, f64>) {
    let all: BTreeMap<&str, &BTreeMap<String, f64>> = [(shape, now)].into();
    if let Ok(text) = serde_json::to_string(&all) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt as _;

    fn bin(package: &str, target: &str) -> Binary {
        Binary {
            package: package.to_owned(),
            kind: "test".to_owned(),
            target: target.to_owned(),
            version: "0.0.1".to_owned(),
            manifest_dir: PathBuf::from("/p"),
            executable: PathBuf::from(format!("/t/{target}")),
            src_path: PathBuf::from(format!("/p/tests/{target}.rs")),
        }
    }

    #[test]
    fn a_target_that_calls_the_containment_guard_is_the_one_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, text: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, text).unwrap();
            Binary {
                src_path: p,
                ..bin("p", name)
            }
        };
        let stages = file(
            "selfhost.rs",
            "mod common;\nfn build() {\n    common::require_containment(\"nzlex\");\n}\n",
        );
        let mentions = file(
            "other.rs",
            "// require_containment(\"x\") is called by selfhost.rs only\nmod common;\n",
        );
        let plain = file("plain.rs", "#[test]\nfn t() {}\n");
        let missing = Binary {
            src_path: dir.path().join("absent.rs"),
            ..bin("p", "absent")
        };
        assert!(needs_containment(&stages));
        assert!(!needs_containment(&mentions));
        assert!(!needs_containment(&plain));
        assert!(!needs_containment(&missing));
    }

    #[test]
    fn cargo_names_the_binaries_once_each_in_canonical_order() {
        let msg = |pkg: &str, kind: &str, name: &str, test: bool, exe: Option<&str>| {
            serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": format!("path+file:///src/crates/{pkg}#0.0.1"),
                "manifest_path": format!("/src/crates/{pkg}/Cargo.toml"),
                "target": {"kind": [kind], "name": name},
                "profile": {"test": test},
                "executable": exe,
            })
            .to_string()
        };
        let out = [
            msg("zeta", "test", "b", true, Some("/t/b")),
            msg("alpha", "lib", "alpha", true, Some("/t/alpha")),
            msg("alpha", "lib", "alpha", false, None),
            "{\"reason\":\"build-finished\",\"success\":true}".to_owned(),
            msg("alpha", "test", "a", true, Some("/t/a")),
            msg("alpha", "test", "a", true, Some("/t/a")),
        ]
        .join("\n");
        let ids: Vec<String> = binaries(&out).iter().map(Binary::id).collect();
        assert_eq!(ids, ["alpha lib:alpha", "alpha test:a", "zeta test:b"]);
        assert_eq!(
            binaries(&out)[0].manifest_dir,
            PathBuf::from("/src/crates/alpha")
        );
    }

    #[test]
    fn the_longest_start_first_and_the_unknown_before_the_known() {
        let bins = [bin("p", "a"), bin("p", "b"), bin("p", "c"), bin("p", "d")];
        let history: BTreeMap<String, f64> = [
            ("p test:a".to_owned(), 1.0),
            ("p test:b".to_owned(), 9.0),
            ("p test:d".to_owned(), 5.0),
        ]
        .into();
        assert_eq!(schedule(&bins, &history), [2, 1, 3, 0]);
        // With no history, the canonical order.
        assert_eq!(schedule(&bins, &BTreeMap::new()), [0, 1, 2, 3]);
        // The same history, the same order.
        assert_eq!(schedule(&bins, &history), schedule(&bins, &history));
    }

    fn fake(ok: bool, secs: f64) -> Ran {
        Ran {
            ended: Ended::Exited(std::process::ExitStatus::from_raw(if ok { 0 } else { 256 })),
            stdout: String::new(),
            stderr: String::new(),
            secs,
        }
    }

    /// Every binary runs exactly once, whatever fails, whatever the worker count, and the
    /// results come back by index — canonical order — not completion order.
    #[test]
    fn every_binary_runs_once_and_a_failure_cancels_nothing() {
        let bins: Vec<Binary> = (0..9).map(|i| bin("p", &format!("t{i}"))).collect();
        for workers in [1, 2, 3, 8] {
            let seen = Mutex::new(Vec::new());
            let order = schedule(&bins, &BTreeMap::new());
            let results = execute(&bins, &order, workers, |b| {
                seen.lock().unwrap().push(b.target.clone());
                // Every third fails; later ones must still run.
                fake(!b.target.ends_with('0') && !b.target.ends_with('3'), 0.0)
            });
            let mut ran = seen.into_inner().unwrap();
            ran.sort();
            let mut all: Vec<String> = bins.iter().map(|b| b.target.clone()).collect();
            all.sort();
            assert_eq!(ran, all, "{workers} workers");
            assert!(results.iter().all(Option::is_some));
            let failed: Vec<usize> = results
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.as_ref().unwrap().passed())
                .map(|(i, _)| i)
                .collect();
            assert_eq!(failed, [0, 3], "{workers} workers");
        }
    }

    /// Results are the binary's own, by its index: however the history orders the start,
    /// the report names each binary's outcome, not the outcome of whichever finished first.
    #[test]
    fn a_result_is_its_binarys_not_the_next_to_finish() {
        let bins: Vec<Binary> = (0..3).map(|i| bin("p", &format!("t{i}"))).collect();
        let history: BTreeMap<String, f64> = bins
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id(), f64::from(u32::try_from(i).unwrap())))
            .collect();
        let order = schedule(&bins, &history);
        assert_eq!(order, [2, 1, 0]);
        let results = execute(&bins, &order, 1, |b| fake(b.target == "t0", 0.0));
        let passed: Vec<bool> = results
            .iter()
            .map(|r| r.as_ref().unwrap().passed())
            .collect();
        assert_eq!(passed, [true, false, false]);
    }

    /// Under real concurrency — varied durations, completion out of start order, one
    /// failure — every run selects the same binaries, runs each once, never more than the
    /// worker count at a time, and reports the same results in canonical order.
    #[test]
    fn the_schedule_is_stable_under_real_concurrency() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let bins: Vec<Binary> = (0..12).map(|i| bin("p", &format!("t{i:02}"))).collect();
        let history: BTreeMap<String, f64> = bins
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id(), f64::from(u32::try_from((i * 7) % 12).unwrap())))
            .collect();
        let mut first: Option<Vec<bool>> = None;
        for round in 0..10 {
            for workers in [1, 3] {
                let running = AtomicUsize::new(0);
                let most = AtomicUsize::new(0);
                let runs = AtomicUsize::new(0);
                let order = schedule(&bins, &history);
                let results = execute(&bins, &order, workers, |b| {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    runs.fetch_add(1, Ordering::SeqCst);
                    let n: u64 = b.target.trim_start_matches('t').parse().unwrap();
                    std::thread::sleep(Duration::from_millis((n * 3 + round) % 11));
                    running.fetch_sub(1, Ordering::SeqCst);
                    fake(b.target != "t05", 0.0)
                });
                assert_eq!(runs.load(Ordering::SeqCst), bins.len(), "round {round}");
                assert!(most.load(Ordering::SeqCst) <= workers, "round {round}");
                let passed: Vec<bool> = results
                    .iter()
                    .map(|r| r.as_ref().unwrap().passed())
                    .collect();
                assert_eq!(passed.iter().filter(|p| !**p).count(), 1);
                assert!(!passed[5]);
                match &first {
                    None => first = Some(passed),
                    Some(f) => assert_eq!(&passed, f, "round {round}, {workers} workers"),
                }
            }
        }
    }

    /// The history decides the order and nothing else: with it, without it, or damaged,
    /// the same binaries run and the same outcomes come back.
    #[test]
    fn the_timing_history_changes_only_the_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = history_path(dir.path());
        let bins: Vec<Binary> = (0..5).map(|i| bin("p", &format!("t{i}"))).collect();
        let outcome = |history: &BTreeMap<String, f64>| {
            let order = schedule(&bins, history);
            let r = execute(&bins, &order, 2, |b| fake(b.target != "t2", 0.0));
            r.iter()
                .map(|x| x.as_ref().unwrap().passed())
                .collect::<Vec<_>>()
        };
        let cold = outcome(&read_history(&path, "w2t2"));
        let now: BTreeMap<String, f64> = bins
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id(), f64::from(u32::try_from(i).unwrap())))
            .collect();
        write_history(&path, "w2t2", &now);
        let warm_history = read_history(&path, "w2t2");
        assert_eq!(warm_history, now);
        assert_ne!(
            schedule(&bins, &warm_history),
            schedule(&bins, &BTreeMap::new())
        );
        assert_eq!(outcome(&warm_history), cold);
        std::fs::write(&path, "not json").unwrap();
        assert!(read_history(&path, "w2t2").is_empty());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(outcome(&read_history(&path, "w2t2")), cold);
    }
}
