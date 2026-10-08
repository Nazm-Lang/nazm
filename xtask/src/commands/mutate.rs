//! `cargo xtask mutate`: argument handling, the run, and the campaign's lock and recovery.

use std::process::ExitCode;

use crate::{campaign, mutate, procs};

/// Inject every catalogued defect and require the suite to catch it.
///
/// Not part of `check`: it rebuilds and re-tests the workspace once per mutation, so it is
/// a scheduled job rather than a pre-commit gate.
/// `--name value` pairs and bare `--name` switches, in any order.
///
/// Deliberately minimal. A switch is a `--name` whose next token is another `--name` or
/// the end, and it maps to an empty string — the caller asks with `contains_key`. The
/// first version required a value unconditionally, so a trailing `--confirm-no-writers`
/// silently vanished and recovery refused a tree it had been told about. A flag a user
/// typed and the program did not see is worse than no flag.
fn flags(args: &mut impl Iterator<Item = String>) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        let Some(name) = a.strip_prefix("--") else {
            continue;
        };
        let value = match args.peek() {
            Some(next) if !next.starts_with("--") => args.next().unwrap_or_default(),
            _ => String::new(),
        };
        out.insert(name.to_owned(), value);
    }
    out
}

fn need(f: &std::collections::HashMap<String, String>, k: &str) -> Result<String, ExitCode> {
    match f.get(k) {
        Some(v) => Ok(v.clone()),
        None => {
            eprintln!("missing --{k}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// Run the catalogue against an arbitrary root. The lifecycle tests point this at a
/// disposable project so an interrupted run damages nothing that matters.
pub(crate) fn mutate_run(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let (Ok(root), Ok(catalogue)) = (need(&f, "root"), need(&f, "catalogue")) else {
        return ExitCode::FAILURE;
    };
    // The lifecycle tests want the whole of whatever catalogue they point at, so no
    // filter here. They do choose the strategy, and whether to resume.
    let mut o = mutate::Options::new(
        std::path::Path::new(&root),
        std::path::Path::new(&catalogue),
    );
    if let Some(s) = f.get("strategy") {
        match campaign::Strategy::parse(s) {
            Some(s) => o.strategy = s,
            None => {
                eprintln!("unknown strategy {s}");
                return ExitCode::FAILURE;
            }
        }
    }
    o.resume = f.contains_key("resume");
    if let Some(d) = f.get("campaign-dir") {
        o.campaign_dir = Some(std::path::PathBuf::from(d));
    }
    if let Some(n) = f.get("killer-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.killer_secs = n;
    }
    if let Some(n) = f.get("profile-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.profile_target_secs = n;
    }
    if let Some(n) = f.get("broad-deadline-secs").and_then(|n| n.parse().ok()) {
        o.deadlines.broad_secs = n;
    }
    if let Some(n) = f.get("budget-secs").and_then(|n| n.parse().ok()) {
        o.budget = Some(std::time::Duration::from_secs(n));
    }
    if let Some(n) = f.get("cargo-jobs").and_then(|n| n.parse().ok()) {
        o.parallelism.cargo_jobs = n;
    }
    if let Some(n) = f.get("test-threads").and_then(|n| n.parse().ok()) {
        o.parallelism.test_threads = n;
    }
    o.verify_killers = f.contains_key("verify-killers");
    o.disposable_target = f.contains_key("disposable-target");
    finish_campaign(mutate::run_all(&o))
}

/// Exit status for a campaign: 0 clean, 1 problems or refused, 3 incomplete. Incomplete
/// is its own code so a supervisor can resume it without mistaking it for either.
fn finish_campaign(end: mutate::End) -> ExitCode {
    match end {
        mutate::End::Clean => {
            println!("ok    mutation coverage");
            ExitCode::SUCCESS
        }
        mutate::End::Problems(p) | mutate::End::Refused(p) => {
            println!("FAIL  mutation coverage");
            for x in p {
                println!("problem: {x}");
            }
            ExitCode::FAILURE
        }
        mutate::End::Incomplete(p) => {
            println!("INCOMPLETE mutation coverage");
            for x in p {
                println!("problem: {x}");
            }
            ExitCode::from(3)
        }
    }
}

/// `cargo xtask mutate …`. Every option is parsed strictly: a typo that silently ran the
/// whole catalogue, or none of it, would waste hours either way.
pub(crate) fn mutate_cli(rest: &[String]) -> ExitCode {
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        eprintln!("could not locate workspace root");
        return ExitCode::FAILURE;
    };
    let catalogue = root.join("xtask/mutations/mutations.toml");
    let mut o = mutate::Options::new(root, &catalogue);
    let mut plan_only = false;
    let mut verify: Vec<String> = Vec::new();
    let mut candidates = Vec::new();
    let mut i = 0;
    let value = |i: usize, what: &str| -> Result<String, ExitCode> {
        rest.get(i + 1).cloned().ok_or_else(|| {
            eprintln!("mutate: {what} needs a value");
            ExitCode::FAILURE
        })
    };
    while let Some(arg) = rest.get(i) {
        let step = match arg.as_str() {
            "--only" => match value(i, "--only") {
                Ok(v) => {
                    o.only.push(v);
                    2
                }
                Err(e) => return e,
            },
            "--strategy" => match value(i, "--strategy").map(|v| campaign::Strategy::parse(&v)) {
                Ok(Some(s)) => {
                    o.strategy = s;
                    2
                }
                Ok(None) => {
                    eprintln!("mutate: --strategy is `targeted` or `full`");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--plan" => {
                plan_only = true;
                1
            }
            "--resume" => {
                o.resume = true;
                1
            }
            "--reset-derived-caches" => {
                o.reset_derived = true;
                1
            }
            "--export-journal" => {
                o.export_journal = true;
                1
            }
            // N32-H: verify each decided mutant's killers in the same warm session.
            "--verify-killers" => {
                o.verify_killers = true;
                1
            }
            // N32-H2: the target is the harness's cache and may be cleaned.
            "--disposable-target" => {
                o.disposable_target = true;
                1
            }
            // N14.1: each step's internal parallelism. Never more mutants at once.
            "--cargo-jobs" | "--test-threads" => match value(i, arg).map(|v| v.parse::<u32>()) {
                Ok(Ok(n)) if (1..=16).contains(&n) => {
                    if arg == "--cargo-jobs" {
                        o.parallelism.cargo_jobs = n;
                    } else {
                        o.parallelism.test_threads = n;
                    }
                    2
                }
                Ok(_) => {
                    eprintln!("mutate: {arg} needs a whole number from 1 to 16");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--budget-secs" => match value(i, "--budget-secs").map(|v| v.parse::<u64>()) {
                Ok(Ok(n)) => {
                    o.budget = Some(std::time::Duration::from_secs(n));
                    2
                }
                Ok(Err(_)) => {
                    eprintln!("mutate: --budget-secs needs a whole number");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            "--campaign-dir" => match value(i, "--campaign-dir") {
                Ok(v) => {
                    o.campaign_dir = Some(v.into());
                    2
                }
                Err(e) => return e,
            },
            "--resume-from" => match value(i, "--resume-from") {
                Ok(v) => {
                    o.resume_from = Some(v.into());
                    2
                }
                Err(e) => return e,
            },
            "--source-commit" => match value(i, "--source-commit") {
                Ok(v) => {
                    o.source_commit = Some(v);
                    2
                }
                Err(e) => return e,
            },
            "--verify-killer" => match value(i, "--verify-killer") {
                Ok(v) => {
                    verify.push(v);
                    2
                }
                Err(e) => return e,
            },
            "--killer" => match value(i, "--killer").map(|v| mutate::parse_killer(&v)) {
                Ok(Ok(k)) => {
                    candidates.push(k);
                    2
                }
                Ok(Err(e)) => {
                    eprintln!("mutate: {e}");
                    return ExitCode::FAILURE;
                }
                Err(e) => return e,
            },
            other => {
                eprintln!("mutate: unknown option {other}");
                return ExitCode::FAILURE;
            }
        };
        i += step;
    }
    if !candidates.is_empty() && verify.len() != 1 {
        eprintln!("mutate: --killer names candidates for exactly one --verify-killer");
        return ExitCode::FAILURE;
    }
    if plan_only {
        return match mutate::plan_only(root, &catalogue, &o.only) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(problems) => {
                for p in problems {
                    println!("problem: {p}");
                }
                ExitCode::FAILURE
            }
        };
    }
    if !verify.is_empty() {
        // One after another, each with its own pristine, mutated and restored builds,
        // so a session pays its cold build once for all of them.
        let mut failed = false;
        for name in &verify {
            println!("verifying killers for {name}");
            match mutate::verify_killer(&o, name, &candidates) {
                Ok(report) => print!("{report}"),
                Err(problems) => {
                    failed = true;
                    for p in problems {
                        println!("problem: {p}");
                    }
                }
            }
        }
        return if failed {
            println!("FAIL  killer verification");
            ExitCode::FAILURE
        } else {
            println!("ok    killer verification");
            ExitCode::SUCCESS
        };
    }
    finish_campaign(mutate::run_all(&o))
}

/// Run one script through the harness's subprocess runner and report how it ended.
///
/// Hidden, for the lifecycle tests: the watchdog can only be shown to work by killing
/// the process that armed it, which a test can do to this process and not to itself.
pub(crate) fn mutate_exec(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let Ok(script) = need(&f, "script") else {
        return ExitCode::FAILURE;
    };
    let secs = f
        .get("deadline-secs")
        .and_then(|n| n.parse().ok())
        .unwrap_or(60);
    let r = procs::run(&procs::Invocation {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), script],
        cwd: std::env::current_dir().unwrap_or_default(),
        env: Vec::new(),
        deadline: std::time::Duration::from_secs(secs),
    });
    println!(
        "ended {:?} after {:.1}s, swept {}",
        r.ended,
        r.elapsed.as_secs_f64(),
        r.swept
    );
    ExitCode::SUCCESS
}

/// Take the lock, announce it, and hold it until told to stop.
///
/// The announcement is a file rather than a line on stdout because the test has to know
/// the lock is *held* before it starts competing, and a pipe read is one more thing that
/// can be buffered. Written after acquisition, so its existence is the fact it claims.
pub(crate) fn mutate_hold(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    let (Ok(root), Ok(ready), Ok(release)) =
        (need(&f, "root"), need(&f, "ready"), need(&f, "release"))
    else {
        return ExitCode::FAILURE;
    };

    let Ok(lock) = mutate::acquire_lock(std::path::Path::new(&root)) else {
        eprintln!("hold: could not acquire");
        return ExitCode::FAILURE;
    };
    if std::fs::write(&ready, "held").is_err() {
        return ExitCode::FAILURE;
    }

    // Bounded, so a test that fails to signal leaves no process behind for the next one
    // to trip over.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !std::path::Path::new(&release).exists() {
        if std::time::Instant::now() > deadline {
            eprintln!("hold: released on the deadline, not on request");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(lock);
    ExitCode::SUCCESS
}

/// Startup recovery on its own, through the same function a real run calls.
pub(crate) fn mutate_recover(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let f = flags(args);
    // An operator runs this from the checkout and should not have to say where it is.
    // The lifecycle tests pass `--root` to point it at a disposable project instead.
    let root = match f.get("root") {
        Some(r) => r.clone(),
        None => match std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            Some(p) => p.display().to_string(),
            None => {
                eprintln!("could not locate the workspace root");
                return ExitCode::FAILURE;
            }
        },
    };
    match mutate::recover(
        std::path::Path::new(&root),
        f.contains_key("confirm-no-writers"),
    ) {
        Ok(outcome) => {
            println!("{outcome}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
