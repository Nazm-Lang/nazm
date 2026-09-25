//! A mutation campaign: one baseline, then each mutant injected, judged, and restored.
//!
//! # The rule this module exists to keep
//!
//! > **Escalate only while a mutant is still alive.**
//!
//! A mutant that a verified killer catches has been caught by a member of the workspace
//! suite. Running the other twelve hundred tests afterwards does not strengthen that
//! verdict, it only costs time. So the targeted strategy runs, in order and stopping at
//! the first failure:
//!
//! 1. **Direct**: each of the entry's killers, one exact test each.
//! 2. **Focused**: each whole target of the entry's profile.
//! 3. **Full**: the workspace suite, [`plan::broad_suite`].
//!
//! Only the last can report `SURVIVED`. [`decide`] makes that structural: the tier list
//! always ends in `Full`, and `Survived` is returned only after a `Full` step passed. A
//! timeout, a crashed runner, a missing result or an infrastructure fault stops the
//! mutant with **no verdict**. It is not escalated past, because a later tier passing
//! would not mean the earlier one said nothing.
//!
//! The full strategy is the legacy evidence shape (build, then the workspace suite for
//! every mutant) and remains available as the oracle the targeted one is checked
//! against. Both call the same build definition, the same broad-suite definition, and
//! the same subprocess runner.
//!
//! # What a verdict is, and when it is final
//!
//! ```text
//! inject → evidence → provisional verdict → restore → verify bytes → record → next
//! ```
//!
//! [`Record`] can only be built from a [`Restored`], and a `Restored` exists only once
//! the file has been read back and matched against the original's digest. A journal line
//! is therefore a verdict *and* a proof that the tree was put back.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::plan::{self, MutationPlan, TestSelector};
use crate::procs::{self, Ended, Invocation};

// ---------------------------------------------------------------------------
// Strategy and tiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Strategy {
    /// Every mutant: build, then the workspace suite. The legacy runner, kept as the
    /// oracle.
    Full,
    /// Killers, then profile, then the workspace suite, stopping at the first failure.
    Targeted,
}

impl Strategy {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Self::Full),
            "targeted" => Some(Self::Targeted),
            _ => None,
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Targeted => "targeted",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Tier {
    Direct,
    Focused,
    Full,
}

impl Tier {
    fn number(self) -> u8 {
        match self {
            Self::Direct => 1,
            Self::Focused => 2,
            Self::Full => 3,
        }
    }
}

/// One step of evidence: a tier, and what runs at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) tier: Tier,
    /// `None` at `Full`: the workspace suite has no selector.
    pub(crate) selector: Option<TestSelector>,
}

impl Step {
    fn label(&self) -> String {
        self.selector
            .as_ref()
            .map_or_else(|| "workspace suite".to_owned(), TestSelector::label)
    }
}

/// The steps a strategy takes for a plan, in order. Always ends in the workspace suite.
pub(crate) fn steps(p: &MutationPlan, strategy: Strategy) -> Vec<Step> {
    let mut v = Vec::new();
    if strategy == Strategy::Targeted {
        for k in &p.killers {
            v.push(Step {
                tier: Tier::Direct,
                selector: Some(k.clone()),
            });
        }
        if let Some((_, _, targets)) = &p.profile {
            for t in targets {
                v.push(Step {
                    tier: Tier::Focused,
                    selector: Some(t.clone()),
                });
            }
        }
    }
    v.push(Step {
        tier: Tier::Full,
        selector: None,
    });
    v
}

// ---------------------------------------------------------------------------
// Observations (what a process did) and verdicts (what that means)
// ---------------------------------------------------------------------------

/// What one test invocation produced. A fact about a process, not about a mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observation {
    /// It ran to completion and reported `result_lines` binaries.
    Completed {
        passed: usize,
        failing: usize,
        result_lines: usize,
        /// A crash signature, when fewer binaries reported than expected and the output
        /// says why.
        crash: Option<String>,
    },
    /// The deadline passed.
    TimedOut(String),
    /// It did not run to completion for a reason about the runner, not the code.
    Failed(String),
}

/// What the build step produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Build {
    Built,
    /// The compiler refused the tree. The legacy meaning of UNUSABLE, unchanged.
    Refused,
    TimedOut(String),
    Failed(String),
}

/// A mutant's verdict. The repository's vocabulary, extended by one word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Injected, built, and a tier's tests failed. Which tier, and which selector.
    Caught {
        failing: usize,
        tier: Tier,
        by: String,
    },
    /// Injected, built, and every step passed, the workspace suite last.
    Survived,
    /// The pattern was absent, or matched more than once, or changed nothing.
    NotInjected(String),
    /// The mutated tree does not build, tests included.
    DidNotBuild,
    /// A test binary died while the mutant was installed.
    MutantCrashed { signature: String, missing: usize },
    /// A step did not run to completion. **No verdict.**
    RunnerFailed(String),
    /// A step passed its deadline. **No verdict**, and never read as caught or survived.
    TimedOut(String),
}

impl Verdict {
    pub(crate) fn word(&self) -> &'static str {
        match self {
            Self::Caught { .. } => "caught",
            Self::Survived => "survived",
            Self::NotInjected(_) => "not-injected",
            Self::DidNotBuild => "unusable",
            Self::MutantCrashed { .. } => "mutant-crash",
            Self::RunnerFailed(_) => "runner-error",
            Self::TimedOut(_) => "timeout",
        }
    }

    /// A verdict about the mutant, as opposed to about the run. Only these may be
    /// reused by a resumed campaign; a no-verdict is always attempted again.
    pub(crate) fn is_semantic(word: &str) -> bool {
        matches!(
            word,
            "caught" | "survived" | "not-injected" | "unusable" | "mutant-crash"
        )
    }
}

/// One step as it ran.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StepRun {
    pub(crate) tier: Tier,
    pub(crate) selector: String,
    /// `pass`, `fail`, `crash`, `incomplete`, `timeout`.
    pub(crate) outcome: String,
    pub(crate) secs: f64,
    /// The part of `secs` cargo reported spending on compilation.
    pub(crate) compile_secs: f64,
}

/// Runs builds and tests. The real one spawns processes; tests use a script.
pub(crate) trait Executor {
    fn build(&mut self) -> (Build, f64);
    /// Runs one step. `expected_results` is how many `test result:` lines a complete
    /// run reports.
    fn test(&mut self, step: &Step, expected_results: usize) -> (Observation, f64, f64);
}

/// What deciding produced, before restoration.
#[derive(Debug)]
pub(crate) struct Provisional {
    pub(crate) verdict: Verdict,
    pub(crate) build_secs: f64,
    pub(crate) steps: Vec<StepRun>,
}

/// The tier state machine. Runs the build, then each step until one decides.
pub(crate) fn decide(
    p: &MutationPlan,
    strategy: Strategy,
    ex: &mut dyn Executor,
    baseline_results: usize,
) -> Provisional {
    let (build, build_secs) = ex.build();
    let mut runs = Vec::new();
    let stop = |verdict, runs| Provisional {
        verdict,
        build_secs,
        steps: runs,
    };
    match build {
        Build::Built => {}
        Build::Refused => return stop(Verdict::DidNotBuild, runs),
        Build::TimedOut(why) => return stop(Verdict::TimedOut(format!("build: {why}")), runs),
        Build::Failed(why) => return stop(Verdict::RunnerFailed(format!("build: {why}")), runs),
    }

    let mut last_passed: Option<Tier> = None;
    for step in steps(p, strategy) {
        let expected = if step.selector.is_some() {
            1
        } else {
            baseline_results
        };
        let (obs, secs, compile_secs) = ex.test(&step, expected);
        let exact = step.selector.as_ref().is_some_and(|s| s.exact.is_some());
        let (outcome, verdict) = judge(&step, &obs, expected, exact);
        runs.push(StepRun {
            tier: step.tier,
            selector: step.label(),
            outcome: outcome.to_owned(),
            secs,
            compile_secs,
        });
        match verdict {
            None => last_passed = Some(step.tier),
            Some(v) => return stop(v, runs),
        }
    }
    // Every step passed. The list always ends in the workspace suite, so this is only
    // reached after it passed; the check says so where it matters instead of relying
    // on the list being built correctly forever.
    if last_passed == Some(Tier::Full) {
        stop(Verdict::Survived, runs)
    } else {
        stop(
            Verdict::RunnerFailed(
                "internal: no workspace suite ran, so survival is unestablished".to_owned(),
            ),
            runs,
        )
    }
}

/// One observation, read under the rules of the step it came from. `None` means pass.
fn judge(
    step: &Step,
    obs: &Observation,
    expected: usize,
    exact: bool,
) -> (&'static str, Option<Verdict>) {
    match obs {
        Observation::TimedOut(why) => (
            "timeout",
            Some(Verdict::TimedOut(format!(
                "tier {} ({}): {why}",
                step.tier.number(),
                step.label()
            ))),
        ),
        Observation::Failed(why) => (
            "incomplete",
            Some(Verdict::RunnerFailed(format!(
                "tier {} ({}): {why}",
                step.tier.number(),
                step.label()
            ))),
        ),
        Observation::Completed {
            crash: Some(signature),
            result_lines,
            ..
        } if *result_lines < expected => (
            "crash",
            Some(Verdict::MutantCrashed {
                signature: signature.clone(),
                missing: expected - result_lines,
            }),
        ),
        Observation::Completed { result_lines, .. } if *result_lines < expected => (
            "incomplete",
            Some(Verdict::RunnerFailed(format!(
                "tier {} ({}): only {result_lines} of {expected} test binaries reported",
                step.tier.number(),
                step.label()
            ))),
        ),
        // A killer is one test. If it selected none, or several, the metadata is not
        // what the plan says it is, and neither a pass nor a failure means anything.
        Observation::Completed {
            passed, failing, ..
        } if exact && passed + failing != 1 => (
            "incomplete",
            Some(Verdict::RunnerFailed(format!(
                "tier {} ({}): an exact killer selected {} tests, not one",
                step.tier.number(),
                step.label(),
                passed + failing
            ))),
        ),
        Observation::Completed { failing: 0, .. } => ("pass", None),
        Observation::Completed { failing, .. } => (
            "fail",
            Some(Verdict::Caught {
                failing: *failing,
                tier: step.tier,
                by: step.label(),
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// Reading libtest output
// ---------------------------------------------------------------------------

/// Read a `cargo test` run the way the legacy runner did, with the completeness count
/// it compares against. `broad` keeps the legacy order of checks exactly.
pub(crate) fn observe(f: &procs::Finished, expected: usize, broad: bool) -> Observation {
    match &f.ended {
        Ended::TimedOut => {
            return Observation::TimedOut(format!(
                "deadline passed after {:.0}s",
                f.elapsed.as_secs_f64()
            ))
        }
        Ended::SpawnFailed(why) => return Observation::Failed(format!("could not start: {why}")),
        Ended::PipeHeld => {
            return Observation::Failed(
                "the process exited but something outside its group still held its output"
                    .to_owned(),
            )
        }
        Ended::Overflowed => {
            return Observation::Failed(format!("output exceeded {} bytes", procs::OUTPUT_CAP))
        }
        Ended::Exited(_) => {}
    }
    let Ended::Exited(status) = &f.ended else {
        return Observation::Failed("unreachable".to_owned());
    };
    let result_lines = f
        .stdout
        .lines()
        .filter(|l| l.starts_with("test result:"))
        .count();
    let crash = crash_signature(&f.stdout, &f.stderr).map(str::to_owned);

    // A selector's single binary that dies reports nothing at all. That is a crash
    // when the output says so, exactly as it would be inside the workspace suite.
    if !broad && result_lines < expected && crash.is_some() {
        return Observation::Completed {
            passed: 0,
            failing: 0,
            result_lines,
            crash,
        };
    }
    if result_lines == 0 {
        return Observation::Failed(format!(
            "the run produced no results at all ({status}). stderr: {}",
            f.stderr.lines().last().unwrap_or("").trim()
        ));
    }
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(sig) = status.signal() {
            return Observation::Failed(format!("the test runner was killed by signal {sig}"));
        }
    }
    let count = |key: &str| -> usize {
        f.stdout
            .lines()
            .filter_map(|l| l.strip_prefix("test result: "))
            .filter_map(|rest| rest.split_once(". ").map(|(_, counts)| counts))
            .flat_map(|counts| counts.split(';'))
            .filter_map(|part| part.trim().strip_suffix(key))
            .filter_map(|n| n.trim().parse::<usize>().ok())
            .sum()
    };
    Observation::Completed {
        passed: count(" passed"),
        failing: count(" failed"),
        result_lines,
        crash: if result_lines < expected { crash } else { None },
    }
}

/// What a test binary died of, if one did. The legacy list, unchanged.
pub(crate) fn crash_signature(stdout: &str, stderr: &str) -> Option<&'static str> {
    [
        "fatal runtime error: stack overflow",
        "has overflowed its stack",
        "memory allocation of",
        "SIGSEGV",
        "SIGABRT",
        "signal: 6",
        "signal: 11",
    ]
    .into_iter()
    .find(|sig| stderr.contains(sig) || stdout.contains(sig))
}

/// The seconds cargo said it spent compiling, from `Finished … in 1.23s`.
fn compile_secs(stderr: &str) -> f64 {
    stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("Finished"))
        .filter_map(|l| l.rsplit_once(" in ").map(|(_, t)| t.trim()))
        .filter_map(|t| {
            if let Some(s) = t.strip_suffix('s') {
                if let Some((m, rest)) = s.split_once("m ") {
                    return Some(m.parse::<f64>().ok()? * 60.0 + rest.parse::<f64>().ok()?);
                }
                return s.parse::<f64>().ok();
            }
            None
        })
        .sum()
}

/// Failing tests, each with the binary it ran in: advisory, for authoring killers, and
/// never read by any verdict.
///
/// Cargo announces each binary on stderr (`Running tests/memory.rs (…)`) and libtest
/// opens each block on stdout (`running 3 tests`), one of each per binary and in the
/// same order, so the n-th announcement names the n-th block. If the counts disagree the
/// pairing is not trusted and names are reported bare.
fn failing_tests(stdout: &str, stderr: &str) -> Vec<String> {
    let binaries: Vec<String> = stderr
        .lines()
        .map(str::trim_start)
        .filter_map(|l| {
            l.strip_prefix("Running ")
                .map(|r| r.split(" (").next().unwrap_or(r).to_owned())
                .or_else(|| {
                    l.strip_prefix("Doc-tests ")
                        .map(|d| format!("doctests {d}"))
                })
        })
        .collect();
    let blocks = stdout.lines().filter(|l| l.starts_with("running ")).count();
    let paired = blocks == binaries.len();
    let mut out = Vec::new();
    let mut block: Option<usize> = None;
    for l in stdout.lines() {
        if l.starts_with("running ") {
            block = Some(block.map_or(0, |b| b + 1));
        } else if let Some(name) = l
            .strip_prefix("test ")
            .and_then(|l| l.strip_suffix(" ... FAILED"))
        {
            let at = block
                .filter(|_| paired)
                .and_then(|b| binaries.get(b))
                .map_or_else(String::new, |b| format!("{b}: "));
            out.push(format!("{at}{name}"));
            if out.len() == 20 {
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The real executor
// ---------------------------------------------------------------------------

/// How long each kind of step may run before it is a timeout, which is a no-verdict.
///
/// Budgets for pathological runs, not tuning knobs. A passing killer finishes long
/// before its deadline; lowering these to make a benchmark look better converts slow
/// catches into no-verdicts and measures nothing.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Deadlines {
    pub(crate) build_secs: u64,
    pub(crate) list_secs: u64,
    pub(crate) killer_secs: u64,
    pub(crate) profile_target_secs: u64,
    pub(crate) broad_secs: u64,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            build_secs: 1800,
            list_secs: 600,
            killer_secs: 1200,
            profile_target_secs: 1500,
            broad_secs: 2400,
        }
    }
}

pub(crate) struct Real<'a> {
    pub(crate) root: &'a Path,
    pub(crate) deadlines: Deadlines,
    /// How much each build and test step may use inside itself. N14.1.
    pub(crate) parallelism: plan::Parallelism,
    pub(crate) reset_derived: bool,
    /// Where this mutant's full logs go, when there is somewhere to put them.
    pub(crate) logs: Option<PathBuf>,
    /// Failing test names seen at any step. Advisory.
    pub(crate) failing: Vec<String>,
}

pub(crate) fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

impl Real<'_> {
    fn invoke(&self, what: &str, args: Vec<String>, deadline: u64) -> procs::Finished {
        let inv = Invocation {
            program: cargo(),
            args,
            cwd: self.root.to_path_buf(),
            env: Vec::new(),
            deadline: Duration::from_secs(deadline),
        };
        let f = procs::run(&inv);
        if let Some(dir) = &self.logs {
            let _ = fs::create_dir_all(dir);
            let _ = fs::write(
                dir.join(format!("{what}.log")),
                format!(
                    "{}\n{:?} after {:.1}s; {} leftover process(es) swept\n--- stdout ---\n{}\n--- stderr ---\n{}",
                    inv.display(),
                    f.ended,
                    f.elapsed.as_secs_f64(),
                    f.swept,
                    f.stdout,
                    f.stderr
                ),
            );
        }
        f
    }
}

impl Executor for Real<'_> {
    fn build(&mut self) -> (Build, f64) {
        let f = self.invoke(
            "build",
            plan::workspace_build(self.parallelism),
            self.deadlines.build_secs,
        );
        let secs = f.elapsed.as_secs_f64();
        let b = match &f.ended {
            Ended::Exited(s) if s.success() => Build::Built,
            Ended::Exited(s) => {
                use std::os::unix::process::ExitStatusExt as _;
                match s.signal() {
                    Some(sig) => Build::Failed(format!("cargo build was killed by signal {sig}")),
                    None => Build::Refused,
                }
            }
            Ended::TimedOut => Build::TimedOut(format!("deadline passed after {secs:.0}s")),
            other => Build::Failed(format!("{other:?}")),
        };
        (b, secs)
    }

    fn test(&mut self, step: &Step, expected: usize) -> (Observation, f64, f64) {
        if self.reset_derived {
            reset_derived(self.root);
        }
        let (args, deadline, what) = match (&step.selector, step.tier) {
            (None, _) => (
                plan::broad_suite(self.parallelism),
                self.deadlines.broad_secs,
                "tier3".to_owned(),
            ),
            (Some(s), Tier::Direct) => (
                s.cargo_args(self.parallelism),
                self.deadlines.killer_secs,
                format!("tier1-{}", sanitize(&s.label())),
            ),
            (Some(s), _) => (
                s.cargo_args(self.parallelism),
                self.deadlines.profile_target_secs,
                format!("tier2-{}", sanitize(&s.label())),
            ),
        };
        let f = self.invoke(&what, args, deadline);
        self.failing.extend(failing_tests(&f.stdout, &f.stderr));
        let obs = observe(&f, expected, step.selector.is_none());
        (obs, f.elapsed.as_secs_f64(), compile_secs(&f.stderr))
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

/// Remove every derived `.nazm/` cache in the disposable tree, so no invocation reads
/// what an earlier one wrote.
///
/// # Why this is needed, and why only here
///
/// A `.nazm/check` entry is keyed by the checker's *declared* identity, the semantic
/// epoch and its code sets, not by the compiler binary. That is right for users: the
/// same rules give the same answer. Under mutation it is wrong: a mutated checker has
/// the same declared identity as the pristine one, so a test that compiles an in-repo
/// program (`compiler/check.nz`, `examples/*.nz`) would read the verdict the *baseline*
/// wrote and never exercise the mutant. That is a false survivor waiting to happen, and
/// the legacy runner had the channel open too.
///
/// Everything under `.nazm/` is derived and deleting it changes no program's meaning,
/// which the CLI suite proves. It is removed only when the campaign is told the root is
/// disposable (`--reset-derived-caches`, which the contained workload passes), never
/// from a checkout someone works in.
pub(crate) fn reset_derived(root: &Path) -> usize {
    fn walk(dir: &Path, top: &Path, removed: &mut usize) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(kind) = e.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let name = e.file_name();
            if name == ".git" || (dir == top && name == "target") {
                continue;
            }
            if name == ".nazm" {
                if fs::remove_dir_all(&path).is_ok() {
                    *removed += 1;
                }
                continue;
            }
            walk(&path, top, removed);
        }
    }
    let mut removed = 0;
    walk(root, root, &mut removed);
    removed
}

// ---------------------------------------------------------------------------
// Restoration, and the record that can only follow it
// ---------------------------------------------------------------------------

/// Proof that a file holds its original bytes again. Only [`restore`] and
/// [`untouched`] make one, and only after reading the file back.
#[derive(Debug)]
pub(crate) struct Restored {
    digest: String,
}

/// Put the original back, read it back, compare, and only then clear the record.
///
/// `write` is a seam for the test that fails it at the boundary that matters.
pub(crate) fn restore(
    path: &Path,
    original: &str,
    backup: &Path,
    write: impl FnOnce(&Path, &str) -> std::io::Result<()>,
) -> Result<Restored, String> {
    write(path, original).map_err(|e| format!("could not write the original back ({e})"))?;
    let back =
        fs::read_to_string(path).map_err(|e| format!("could not read the restored file ({e})"))?;
    if back != original {
        return Err("the restored file does not hold the original bytes".to_owned());
    }
    let _ = fs::remove_file(backup);
    Ok(Restored {
        digest: blake(original.as_bytes()),
    })
}

/// Proof for a mutant that was never written: the file still holds `original`.
pub(crate) fn untouched(path: &Path, original: &str) -> Result<Restored, String> {
    let now =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if now == original {
        Ok(Restored {
            digest: blake(original.as_bytes()),
        })
    } else {
        Err(format!(
            "{} changed while nothing was injected",
            path.display()
        ))
    }
}

/// Proof for a mutant whose file could not even be read: nothing was written, so there
/// is nothing to restore. The digests are of the empty input, and say so.
pub(crate) fn untouched_absent() -> Restored {
    Restored { digest: blake(b"") }
}

pub(crate) fn blake(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Timing {
    pub(crate) apply: f64,
    pub(crate) build: f64,
    pub(crate) tests: f64,
    pub(crate) compile_in_tests: f64,
    pub(crate) restore: f64,
    pub(crate) total: f64,
}

/// One finalized mutant: a journal line.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Record {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) strategy: Strategy,
    /// One of [`Verdict::word`].
    pub(crate) verdict: String,
    pub(crate) detail: String,
    /// The tier that decided a `caught`.
    pub(crate) tier: Option<Tier>,
    pub(crate) by: Option<String>,
    pub(crate) failing: usize,
    pub(crate) steps: Vec<StepRun>,
    pub(crate) original_digest: String,
    pub(crate) mutated_digest: Option<String>,
    pub(crate) restored_digest: String,
    /// Always true in a record that exists. Kept in the line so a reader does not have
    /// to trust the type system of a program it cannot see.
    pub(crate) restored: bool,
    pub(crate) secs: Timing,
    /// Failing tests seen on the way, advisory, for authoring killers.
    pub(crate) candidates: Vec<String>,
}

impl Record {
    /// The only constructor. It takes the restoration proof by value.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finalize(
        p: &MutationPlan,
        strategy: Strategy,
        verdict: &Verdict,
        provisional_steps: Vec<StepRun>,
        original_digest: String,
        mutated_digest: Option<String>,
        proof: Restored,
        secs: Timing,
        candidates: Vec<String>,
    ) -> Self {
        let (tier, by, failing) = match verdict {
            Verdict::Caught { failing, tier, by } => (Some(*tier), Some(by.clone()), *failing),
            _ => (None, None, 0),
        };
        Self {
            index: p.index,
            name: p.name.clone(),
            file: p.file.clone(),
            strategy,
            verdict: verdict.word().to_owned(),
            detail: detail(verdict),
            tier,
            by,
            failing,
            steps: provisional_steps,
            original_digest,
            mutated_digest,
            restored_digest: proof.digest,
            restored: true,
            secs,
            candidates,
        }
    }
}

fn detail(v: &Verdict) -> String {
    match v {
        Verdict::Caught { failing, tier, by } => {
            format!("tier {}: {by}; {failing} test(s) failed", tier.number())
        }
        Verdict::Survived => "every step passed, the workspace suite last".to_owned(),
        Verdict::NotInjected(w) | Verdict::RunnerFailed(w) | Verdict::TimedOut(w) => w.clone(),
        Verdict::DidNotBuild => "the mutated tree does not build (tests included)".to_owned(),
        Verdict::MutantCrashed { signature, missing } => {
            format!("{signature}; {missing} binary(ies) did not report")
        }
    }
}

// ---------------------------------------------------------------------------
// Identity and journal
// ---------------------------------------------------------------------------

/// Everything a verdict depends on. Two campaigns with equal identities judge the same
/// mutants of the same tree by the same rules; a journal is reusable only between them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Identity {
    pub(crate) schema: String,
    /// Every file of the disposable tree, derived caches and `target/` aside. Covers
    /// the compiler, the tests, the catalogue and this harness.
    pub(crate) tree: String,
    pub(crate) catalogue: String,
    pub(crate) harness: String,
    pub(crate) toolchain: String,
    pub(crate) strategy: Strategy,
    pub(crate) selected: Vec<String>,
    pub(crate) deadlines: Deadlines,
    pub(crate) reset_derived: bool,
    /// Each step's internal parallelism, N14.1. A deadline-bearing verdict is evidence
    /// about the execution that produced it.
    pub(crate) parallelism: plan::Parallelism,
    /// The CPU quota the campaign ran under, as the cgroup states it (`cpu.max`), or
    /// `unbounded` where there is none to read. N14.1: the same step under another quota
    /// is another execution.
    pub(crate) cpu_quota: String,
}

pub(crate) const JOURNAL_SCHEMA: &str = "nazm.mutation-campaign/1";

impl Identity {
    pub(crate) fn key(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        blake(json.as_bytes()).chars().take(16).collect()
    }
}

/// A digest of every file under `dir`, sorted, skipping `.git`, `.nazm` and a
/// top-level `target`.
pub(crate) fn tree_digest(dir: &Path) -> Result<String, String> {
    fn walk(dir: &Path, top: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name();
            let kind = e
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if name == ".git" || name == ".nazm" || (dir == top && name == "target") {
                continue;
            }
            if kind.is_dir() {
                walk(&path, top, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    files.sort();
    let mut h = blake3::Hasher::new();
    for f in &files {
        let rel = f.strip_prefix(dir).unwrap_or(f);
        let bytes = match fs::symlink_metadata(f) {
            Ok(m) if m.file_type().is_symlink() => fs::read_link(f)
                .map(|t| t.to_string_lossy().into_owned().into_bytes())
                .unwrap_or_default(),
            _ => fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?,
        };
        h.update(rel.to_string_lossy().as_bytes());
        h.update(&[0]);
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(h.finalize().to_hex().to_string())
}

/// The first line of a journal file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Header {
    pub(crate) kind: String,
    pub(crate) key: String,
    pub(crate) identity: Identity,
    /// Informational: the checkout's commit, when the supervisor knows it. Not part of
    /// the key, because the tree digest already says what was tested.
    pub(crate) source_commit: Option<String>,
}

/// A line of the journal as written: the header, or a record. Both boxed: the two are
/// read and written a line at a time, never stored in bulk.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Line {
    Campaign {
        key: String,
        identity: Box<Identity>,
        source_commit: Option<String>,
    },
    Mutant(Box<Record>),
}

/// The journal line prefix a supervisor collects from stdout.
pub(crate) const EXPORT_PREFIX: &str = "nazm-journal ";

pub(crate) struct Journal {
    file: Option<fs::File>,
    key: String,
    /// Also print each line for a supervisor to collect. Off for a person at a
    /// terminal, who wants verdicts, not JSON.
    export: bool,
}

impl Journal {
    /// Open (append) the journal for `header` under `dir/<key>/journal.jsonl`, and
    /// announce the header on stdout for a supervisor to collect.
    pub(crate) fn open(dir: Option<&Path>, header: &Header, export: bool) -> Result<Self, String> {
        let line = serde_json::to_string(&Line::Campaign {
            key: header.key.clone(),
            identity: Box::new(header.identity.clone()),
            source_commit: header.source_commit.clone(),
        })
        .map_err(|e| e.to_string())?;
        let mut file = None;
        if let Some(dir) = dir {
            let d = dir.join(&header.key);
            fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            let path = d.join("journal.jsonl");
            let fresh = !path.exists();
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if fresh {
                writeln!(f, "{line}").map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
            }
            file = Some(f);
        }
        if export {
            println!("{EXPORT_PREFIX}{} {line}", header.key);
            let _ = std::io::stdout().flush();
        }
        Ok(Self {
            file,
            key: header.key.clone(),
            export,
        })
    }

    /// Record one finalized mutant, durably, then announce it.
    pub(crate) fn append(&mut self, r: &Record) -> Result<(), String> {
        let line =
            serde_json::to_string(&Line::Mutant(Box::new(r.clone()))).map_err(|e| e.to_string())?;
        if let Some(f) = &mut self.file {
            writeln!(f, "{line}").map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        if self.export {
            println!("{EXPORT_PREFIX}{} {line}", self.key);
            let _ = std::io::stdout().flush();
        }
        Ok(())
    }
}

/// Read a journal and return its finalized records, **only** if its identity is this
/// one. Any mismatch, and any record claiming a verdict without restoration, refuses.
pub(crate) fn read_journal(
    path: &Path,
    identity: &Identity,
) -> Result<BTreeMap<String, Record>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err(format!("{} is empty", path.display()));
    };
    match serde_json::from_str::<Line>(first) {
        Ok(Line::Campaign {
            identity: theirs, ..
        }) => {
            if *theirs != *identity {
                return Err(format!(
                    "{} was recorded against a different campaign identity ({}). Its \
                     verdicts describe another tree, catalogue, harness, toolchain, \
                     strategy, selection or deadline set, and are not reused.",
                    path.display(),
                    differences(&theirs, identity).join(", ")
                ));
            }
        }
        _ => {
            return Err(format!(
                "{} does not begin with a campaign header",
                path.display()
            ))
        }
    }
    let mut out = BTreeMap::new();
    for (n, l) in lines.enumerate() {
        match serde_json::from_str::<Line>(l) {
            Ok(Line::Mutant(r)) => {
                if !r.restored || r.restored_digest != r.original_digest {
                    return Err(format!(
                        "{} line {}: `{}` claims a verdict without a verified restoration. \
                         The journal is not trusted.",
                        path.display(),
                        n + 2,
                        r.name
                    ));
                }
                out.insert(r.name.clone(), *r);
            }
            // Headers repeat when a supervisor appends several sessions. Same identity
            // or refuse, as above.
            Ok(Line::Campaign {
                identity: theirs, ..
            }) if *theirs == *identity => {}
            Ok(Line::Campaign { .. }) => {
                return Err(format!("{} mixes campaign identities", path.display()))
            }
            Err(e) => return Err(format!("{} line {}: {e}", path.display(), n + 2)),
        }
    }
    Ok(out)
}

fn differences(a: &Identity, b: &Identity) -> Vec<&'static str> {
    let mut v = Vec::new();
    if a.schema != b.schema {
        v.push("schema");
    }
    if a.tree != b.tree {
        v.push("tree");
    }
    if a.catalogue != b.catalogue {
        v.push("catalogue");
    }
    if a.harness != b.harness {
        v.push("harness");
    }
    if a.toolchain != b.toolchain {
        v.push("toolchain");
    }
    if a.strategy != b.strategy {
        v.push("strategy");
    }
    if a.selected != b.selected {
        v.push("selection");
    }
    if a.deadlines != b.deadlines {
        v.push("deadlines");
    }
    if a.reset_derived != b.reset_derived {
        v.push("cache isolation");
    }
    v
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct Summary {
    pub(crate) selected: usize,
    pub(crate) by_word: BTreeMap<String, usize>,
    pub(crate) caught_at: BTreeMap<u8, usize>,
    pub(crate) resumed: usize,
    pub(crate) not_reached: Vec<String>,
    pub(crate) broad_runs: usize,
    pub(crate) decided_before_broad: usize,
    pub(crate) setup_secs: f64,
    pub(crate) apply: f64,
    pub(crate) build: f64,
    pub(crate) tests: f64,
    pub(crate) compile_in_tests: f64,
    pub(crate) restore: f64,
    pub(crate) wall: f64,
}

impl Summary {
    pub(crate) fn add(&mut self, r: &Record, resumed: bool) {
        *self.by_word.entry(r.verdict.clone()).or_default() += 1;
        if let Some(t) = r.tier {
            *self.caught_at.entry(t.number()).or_default() += 1;
        }
        if resumed {
            self.resumed += 1;
            return;
        }
        let broad = r.steps.iter().filter(|s| s.tier == Tier::Full).count();
        self.broad_runs += broad;
        if broad == 0
            && Verdict::is_semantic(&r.verdict)
            && r.verdict != "not-injected"
            && r.verdict != "unusable"
        {
            self.decided_before_broad += 1;
        }
        self.apply += r.secs.apply;
        self.build += r.secs.build;
        self.tests += r.secs.tests;
        self.compile_in_tests += r.secs.compile_in_tests;
        self.restore += r.secs.restore;
    }

    pub(crate) fn render(&self, key: &str, strategy: Strategy) -> String {
        use std::fmt::Write as _;
        let n = |w: &str| self.by_word.get(w).copied().unwrap_or(0);
        let t = |k: u8| self.caught_at.get(&k).copied().unwrap_or(0);
        let finalized: usize = self.by_word.values().sum();
        let mut s = String::new();
        let _ = writeln!(
            s,
            "\ncampaign {key} — strategy {}, {} selected",
            strategy.word(),
            self.selected
        );
        let _ = writeln!(s, "  caught at tier 1 (direct)    {}", t(1));
        let _ = writeln!(s, "  caught at tier 2 (focused)   {}", t(2));
        let _ = writeln!(s, "  caught at tier 3 (full)      {}", t(3));
        let _ = writeln!(s, "  survived                     {}", n("survived"));
        let _ = writeln!(s, "  unusable                     {}", n("unusable"));
        let _ = writeln!(s, "  not injected                 {}", n("not-injected"));
        let _ = writeln!(s, "  mutant crash                 {}", n("mutant-crash"));
        let _ = writeln!(s, "  no verdict: timeout          {}", n("timeout"));
        let _ = writeln!(s, "  no verdict: runner error     {}", n("runner-error"));
        let _ = writeln!(s, "  retired-equivalent           0 (the catalogue has no such state; retiring means removing the entry)");
        let _ = writeln!(
            s,
            "  of which reused from journal {} (identity {key})",
            self.resumed
        );
        let _ = writeln!(
            s,
            "  not reached this session     {}",
            self.not_reached.len()
        );
        let _ = writeln!(
            s,
            "  accounted                    {} = {finalized} finalized + {} not reached",
            self.selected,
            self.not_reached.len()
        );
        let _ = writeln!(
            s,
            "  workspace suites run         {} (decided before the workspace suite: {})",
            self.broad_runs, self.decided_before_broad
        );
        let _ = writeln!(
            s,
            "  seconds: wall {:.0}; setup+baseline {:.0}; apply {:.1}; build {:.0}; tests {:.0} (of which cargo compiling {:.0}); restore {:.1}",
            self.wall, self.setup_secs, self.apply, self.build, self.tests, self.compile_in_tests, self.restore
        );
        s
    }
}

/// Is the accounting whole? `selected = finalized + not reached`, with nothing missing.
pub(crate) fn accounted(
    selected: &[String],
    finalized: &BTreeSet<String>,
    not_reached: &[String],
) -> bool {
    let mut all: BTreeSet<&String> = finalized.iter().collect();
    for n in not_reached {
        if !all.insert(n) {
            return false;
        }
    }
    all.len() == selected.len() && selected.iter().all(|s| all.contains(s))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::plan::{ProfileSource, Target, TargetKind};
    use std::collections::VecDeque;

    /// Scripted executor: answers each call from a queue, and records what was asked.
    struct Script {
        build: Build,
        answers: VecDeque<Observation>,
        asked: Vec<Tier>,
    }

    impl Executor for Script {
        fn build(&mut self) -> (Build, f64) {
            (self.build.clone(), 1.0)
        }
        fn test(&mut self, step: &Step, _expected: usize) -> (Observation, f64, f64) {
            self.asked.push(step.tier);
            (
                self.answers.pop_front().expect("an unscripted step ran"),
                1.0,
                0.0,
            )
        }
    }

    fn script(answers: Vec<Observation>) -> Script {
        Script {
            build: Build::Built,
            answers: answers.into(),
            asked: Vec::new(),
        }
    }

    fn sel(exact: Option<&str>) -> TestSelector {
        TestSelector {
            target: Target {
                package: "p".to_owned(),
                kind: TargetKind::Test("t".to_owned()),
            },
            exact: exact.map(str::to_owned),
        }
    }

    fn a_plan(killers: usize, profile_targets: usize) -> MutationPlan {
        MutationPlan {
            index: 0,
            name: "m".to_owned(),
            file: "f".to_owned(),
            killers: (0..killers).map(|_| sel(Some("k"))).collect(),
            profile: (profile_targets > 0).then(|| {
                (
                    "prof".to_owned(),
                    ProfileSource::Explicit,
                    (0..profile_targets).map(|_| sel(None)).collect(),
                )
            }),
        }
    }

    fn pass() -> Observation {
        Observation::Completed {
            passed: 1,
            failing: 0,
            result_lines: 1,
            crash: None,
        }
    }
    fn fail() -> Observation {
        Observation::Completed {
            passed: 0,
            failing: 1,
            result_lines: 1,
            crash: None,
        }
    }
    fn broad_pass() -> Observation {
        Observation::Completed {
            passed: 100,
            failing: 0,
            result_lines: 10,
            crash: None,
        }
    }
    fn broad_fail() -> Observation {
        Observation::Completed {
            passed: 99,
            failing: 1,
            result_lines: 10,
            crash: None,
        }
    }

    #[test]
    fn a_direct_catch_stops_before_the_profile_and_the_suite() {
        let mut ex = script(vec![fail()]);
        let p = decide(&a_plan(1, 2), Strategy::Targeted, &mut ex, 10);
        assert!(
            matches!(
                p.verdict,
                Verdict::Caught {
                    tier: Tier::Direct,
                    ..
                }
            ),
            "{:?}",
            p.verdict
        );
        assert_eq!(ex.asked, vec![Tier::Direct]);
    }

    #[test]
    fn a_passing_killer_escalates_to_the_profile() {
        let mut ex = script(vec![pass(), fail()]);
        let p = decide(&a_plan(1, 2), Strategy::Targeted, &mut ex, 10);
        assert!(matches!(
            p.verdict,
            Verdict::Caught {
                tier: Tier::Focused,
                ..
            }
        ));
        assert_eq!(ex.asked, vec![Tier::Direct, Tier::Focused]);
    }

    #[test]
    fn a_focused_catch_does_not_run_the_suite() {
        let mut ex = script(vec![pass(), pass(), fail()]);
        let p = decide(&a_plan(1, 3), Strategy::Targeted, &mut ex, 10);
        assert!(matches!(
            p.verdict,
            Verdict::Caught {
                tier: Tier::Focused,
                ..
            }
        ));
        assert!(!ex.asked.contains(&Tier::Full));
    }

    #[test]
    fn a_passing_profile_escalates_to_the_suite_and_only_the_suite_can_say_survived() {
        let mut ex = script(vec![pass(), pass(), broad_pass()]);
        let p = decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10);
        assert_eq!(p.verdict, Verdict::Survived);
        assert_eq!(ex.asked, vec![Tier::Direct, Tier::Focused, Tier::Full]);

        let mut ex = script(vec![pass(), pass(), broad_fail()]);
        let p = decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10);
        assert!(matches!(
            p.verdict,
            Verdict::Caught {
                tier: Tier::Full,
                ..
            }
        ));
    }

    #[test]
    fn with_no_killer_and_no_profile_the_suite_runs_first() {
        let mut ex = script(vec![broad_fail()]);
        let p = decide(&a_plan(0, 0), Strategy::Targeted, &mut ex, 10);
        assert!(matches!(
            p.verdict,
            Verdict::Caught {
                tier: Tier::Full,
                ..
            }
        ));
        assert_eq!(ex.asked, vec![Tier::Full]);
    }

    #[test]
    fn the_full_strategy_ignores_killers_and_profiles() {
        let mut ex = script(vec![broad_pass()]);
        let p = decide(&a_plan(2, 2), Strategy::Full, &mut ex, 10);
        assert_eq!(p.verdict, Verdict::Survived);
        assert_eq!(ex.asked, vec![Tier::Full]);
    }

    #[test]
    fn a_timeout_at_any_tier_is_no_verdict_and_nothing_runs_after_it() {
        for (answers, n) in [
            (vec![Observation::TimedOut("d".into())], 1),
            (vec![pass(), Observation::TimedOut("d".into())], 2),
            (vec![pass(), pass(), Observation::TimedOut("d".into())], 3),
        ] {
            let mut ex = script(answers);
            let p = decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10);
            assert!(matches!(p.verdict, Verdict::TimedOut(_)), "{:?}", p.verdict);
            assert_eq!(ex.asked.len(), n, "a step ran after the timeout");
        }
    }

    #[test]
    fn a_build_that_times_out_is_no_verdict_and_a_refused_build_is_unusable() {
        let mut ex = script(vec![]);
        ex.build = Build::TimedOut("d".into());
        assert!(matches!(
            decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10).verdict,
            Verdict::TimedOut(_)
        ));
        let mut ex = script(vec![]);
        ex.build = Build::Refused;
        assert_eq!(
            decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10).verdict,
            Verdict::DidNotBuild
        );
        assert!(ex.asked.is_empty());
    }

    #[test]
    fn an_incomplete_step_is_no_verdict_not_a_pass_to_escalate_past() {
        let mut ex = script(vec![Observation::Completed {
            passed: 0,
            failing: 0,
            result_lines: 0,
            crash: None,
        }]);
        let p = decide(&a_plan(1, 1), Strategy::Targeted, &mut ex, 10);
        assert!(
            matches!(p.verdict, Verdict::RunnerFailed(_)),
            "{:?}",
            p.verdict
        );
        assert_eq!(ex.asked.len(), 1);
    }

    #[test]
    fn an_exact_killer_that_selected_no_test_is_no_verdict() {
        let mut ex = script(vec![Observation::Completed {
            passed: 0,
            failing: 0,
            result_lines: 1,
            crash: None,
        }]);
        let p = decide(&a_plan(1, 0), Strategy::Targeted, &mut ex, 10);
        assert!(
            matches!(p.verdict, Verdict::RunnerFailed(ref w) if w.contains("selected 0")),
            "{:?}",
            p.verdict
        );
    }

    #[test]
    fn a_crash_in_the_killer_binary_is_a_mutant_crash_as_it_would_be_in_the_suite() {
        let mut ex = script(vec![Observation::Completed {
            passed: 0,
            failing: 0,
            result_lines: 0,
            crash: Some("SIGABRT".into()),
        }]);
        let p = decide(&a_plan(1, 0), Strategy::Targeted, &mut ex, 10);
        assert!(
            matches!(p.verdict, Verdict::MutantCrashed { missing: 1, .. }),
            "{:?}",
            p.verdict
        );
    }

    #[test]
    fn a_suite_reporting_fewer_binaries_than_the_baseline_is_no_verdict() {
        let mut ex = script(vec![Observation::Completed {
            passed: 5,
            failing: 0,
            result_lines: 9,
            crash: None,
        }]);
        let p = decide(&a_plan(0, 0), Strategy::Full, &mut ex, 10);
        assert!(
            matches!(p.verdict, Verdict::RunnerFailed(ref w) if w.contains("9 of 10")),
            "{:?}",
            p.verdict
        );
    }

    #[test]
    fn the_libtest_summary_is_read_as_the_legacy_runner_read_it() {
        let f = procs::Finished {
            ended: Ended::Exited(std::os::unix::process::ExitStatusExt::from_raw(256)),
            stdout: "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s\n".to_owned(),
            stderr: "    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 2.50s\n".to_owned(),
            elapsed: Duration::from_secs(70),
            swept: 0,
        };
        assert_eq!(
            observe(&f, 1, false),
            Observation::Completed {
                passed: 1,
                failing: 1,
                result_lines: 1,
                crash: None
            }
        );
        assert!((compile_secs(&f.stderr) - 62.5).abs() < 1e-9);
        assert_eq!(failing_tests(&f.stdout, ""), vec!["b".to_owned()]);
        let stderr = "     Running unittests src/lib.rs (target/debug/deps/a-1)\n     Running tests/memory.rs (target/debug/deps/memory-2)\n";
        let stdout = "running 1 test\ntest x ... ok\n\nrunning 2 tests\ntest y ... FAILED\n";
        assert_eq!(
            failing_tests(stdout, stderr),
            vec!["tests/memory.rs: y".to_owned()]
        );
    }

    #[test]
    fn a_timed_out_process_is_observed_as_a_timeout_never_as_a_failure() {
        let f = procs::Finished {
            ended: Ended::TimedOut,
            stdout: "test result: FAILED. 0 passed; 1 failed;".to_owned(),
            stderr: String::new(),
            elapsed: Duration::from_secs(5),
            swept: 0,
        };
        assert!(matches!(observe(&f, 1, false), Observation::TimedOut(_)));
    }

    fn identity() -> Identity {
        Identity {
            schema: JOURNAL_SCHEMA.to_owned(),
            tree: "t".into(),
            catalogue: "c".into(),
            harness: "h".into(),
            toolchain: "rustc".into(),
            strategy: Strategy::Targeted,
            selected: vec!["m".into()],
            deadlines: Deadlines::default(),
            reset_derived: true,
            parallelism: plan::Parallelism::SERIAL,
            cpu_quota: "unbounded".into(),
        }
    }

    #[test]
    fn a_different_execution_profile_is_a_different_campaign() {
        let base = identity();
        assert_eq!(base.key(), identity().key());
        let mut jobs = identity();
        jobs.parallelism.cargo_jobs = 4;
        let mut threads = identity();
        threads.parallelism.test_threads = 4;
        let mut quota = identity();
        quota.cpu_quota = "400000 100000".into();
        let keys = [base.key(), jobs.key(), threads.key(), quota.key()];
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b, "{keys:?}");
            }
        }
    }

    fn record(p: &MutationPlan) -> Record {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f");
        fs::write(&f, "x").unwrap();
        let proof = untouched(&f, "x").unwrap();
        Record::finalize(
            p,
            Strategy::Targeted,
            &Verdict::Caught {
                failing: 1,
                tier: Tier::Direct,
                by: "k".into(),
            },
            Vec::new(),
            blake(b"x"),
            Some(blake(b"y")),
            proof,
            Timing {
                apply: 0.0,
                build: 0.0,
                tests: 0.0,
                compile_in_tests: 0.0,
                restore: 0.0,
                total: 0.0,
            },
            Vec::new(),
        )
    }

    #[test]
    fn a_journal_is_reused_only_by_the_identity_that_wrote_it() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity();
        let header = Header {
            kind: "campaign".into(),
            key: id.key(),
            identity: id.clone(),
            source_commit: None,
        };
        let mut j = Journal::open(Some(dir.path()), &header, false).unwrap();
        let r = record(&a_plan(1, 0));
        j.append(&r).unwrap();
        let path = dir.path().join(id.key()).join("journal.jsonl");

        let got = read_journal(&path, &id).unwrap();
        assert_eq!(got.get("m"), Some(&r));

        for change in [
            |i: &mut Identity| i.tree = "other".into(),
            |i: &mut Identity| i.catalogue = "other".into(),
            |i: &mut Identity| i.harness = "other".into(),
            |i: &mut Identity| i.strategy = Strategy::Full,
            |i: &mut Identity| i.selected.push("n".into()),
            |i: &mut Identity| i.toolchain = "other".into(),
            |i: &mut Identity| i.deadlines.broad_secs = 1,
        ] {
            let mut other = id.clone();
            change(&mut other);
            assert_ne!(other.key(), id.key(), "the key must move with the identity");
            let e = read_journal(&path, &other).expect_err("a different identity must be refused");
            assert!(e.contains("different campaign identity"), "{e}");
        }
    }

    #[test]
    fn a_journal_line_claiming_a_verdict_without_restoration_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity();
        let header = Header {
            kind: "campaign".into(),
            key: id.key(),
            identity: id.clone(),
            source_commit: None,
        };
        let mut j = Journal::open(Some(dir.path()), &header, false).unwrap();
        let mut r = record(&a_plan(1, 0));
        r.restored = false;
        j.append(&r).unwrap();
        let path = dir.path().join(id.key()).join("journal.jsonl");
        let e = read_journal(&path, &id).expect_err("must refuse");
        assert!(e.contains("without a verified restoration"), "{e}");

        // And a record whose restored digest is not the original's is the same lie.
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(Some(dir.path()), &header, false).unwrap();
        let mut r = record(&a_plan(1, 0));
        r.restored_digest = blake(b"y");
        j.append(&r).unwrap();
        let path = dir.path().join(id.key()).join("journal.jsonl");
        assert!(read_journal(&path, &id).is_err());
    }

    #[test]
    fn restoration_is_verified_by_reading_back_and_a_failed_write_is_not_a_proof() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("src.rs");
        let backup = dir.path().join("backup");
        fs::write(&f, "mutated").unwrap();
        fs::write(&backup, "record").unwrap();
        let e = restore(&f, "original", &backup, |_, _| {
            Err(std::io::Error::other("injected"))
        })
        .expect_err("no proof");
        assert!(e.contains("could not write"));
        assert!(backup.exists(), "the record must survive a failed restore");

        // A write that "succeeds" but leaves other bytes is caught by the read-back.
        let e = restore(&f, "original", &backup, |p, _| {
            fs::write(p, "something else")
        })
        .expect_err("no proof");
        assert!(e.contains("does not hold the original"));
        assert!(backup.exists());

        let ok = restore(&f, "original", &backup, |p, c| fs::write(p, c)).expect("proof");
        assert_eq!(ok.digest, blake(b"original"));
        assert!(!backup.exists());
        assert_eq!(fs::read_to_string(&f).unwrap(), "original");
    }

    #[test]
    fn derived_caches_are_removed_inside_the_root_and_nowhere_else() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        for d in [
            "examples/.nazm/check",
            "compiler/.nazm",
            "target/.nazm",
            "deep/er/.nazm/native",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join("examples/.nazm/check/entry"), "stale").unwrap();
        let outside = dir.path().join("outside/.nazm");
        fs::create_dir_all(&outside).unwrap();

        assert_eq!(reset_derived(&root), 3);
        assert!(!root.join("examples/.nazm").exists());
        assert!(!root.join("compiler/.nazm").exists());
        assert!(!root.join("deep/er/.nazm").exists());
        assert!(
            root.join("target/.nazm").exists(),
            "the build directory is not the harness's to clean"
        );
        assert!(outside.exists(), "nothing outside the root is touched");
    }

    #[test]
    fn the_tree_digest_moves_with_content_and_ignores_derived_data() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        fs::write(r.join("a"), "1").unwrap();
        let before = tree_digest(r).unwrap();
        fs::create_dir_all(r.join("target")).unwrap();
        fs::write(r.join("target/x"), "build output").unwrap();
        fs::create_dir_all(r.join("sub/.nazm")).unwrap();
        fs::write(r.join("sub/.nazm/y"), "cache").unwrap();
        assert_eq!(tree_digest(r).unwrap(), before);
        fs::write(r.join("a"), "2").unwrap();
        assert_ne!(tree_digest(r).unwrap(), before);
    }

    #[test]
    fn the_accounting_invariant_catches_a_missing_or_doubled_name() {
        let sel = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let fin: BTreeSet<String> = ["a".to_owned(), "b".to_owned()].into();
        assert!(accounted(&sel, &fin, &["c".to_owned()]));
        assert!(!accounted(&sel, &fin, &[]));
        assert!(!accounted(&sel, &fin, &["b".to_owned(), "c".to_owned()]));
    }
}
