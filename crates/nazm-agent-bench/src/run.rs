//! The runner: a plan of independent requests, a spend cap no attempt may pass, bounded
//! retries of transport failures only, and a journal that resumes without repeating a request.
//!
//! **Identity law.** A request is identified by its task, arm, trial, provider, requested model,
//! every request parameter, the prompt version and template digest, the suite version and the
//! context's digest. A benchmark is identified by the suite's digest, the prompt, the mode,
//! the trial count, the provider, the model and the parameters. A change to any of them is a
//! different identity, so a resumed run never counts an earlier request as this one's. Prices
//! are in neither: re-pricing is not re-running.
//!
//! **Budget law.** Before every attempt, retries included, the runner adds that attempt's
//! worst case — every input token uncached at the bound the provider gives over the local
//! `o200k_base` count (or the provider's own count, if larger), and the whole output budget —
//! to everything the evidence directory's ledger has charged, and refuses to send if the sum
//! exceeds the cap. An attempt that failed without a reply is charged nothing when the server
//! said no, and its worst case when the transport failed, since that may have been billed.
//!
//! **Retry law.** A transient failure is retried at most `max_retries` times; a reply is never
//! retried, whatever it says. A request with no reply after that is a runner error: not an
//! answer, not scored, never counted incorrect.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::oracle::{self, Score};
use crate::price::{self, Cost, Price, Usage};
use crate::prompt::{self, Request};
use crate::provider::{CallError, Provider};
use crate::suite::{Arm, Category, Suite, Task, PILOT};

/// A pilot checks the integration and is never evidence; an authoritative run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Pilot,
    Authoritative,
}

impl Mode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pilot => "pilot",
            Self::Authoritative => "authoritative",
        }
    }
}

/// What a provider is, without a credential: enough to plan, identify and bound a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub provider: String,
    pub model: String,
    pub parameters: Value,
    pub max_output_tokens: u64,
    pub input_bound: (u64, u64),
    pub framing: u64,
    pub calls_per_request: u64,
}

impl Spec {
    #[must_use]
    pub fn of(p: &dyn Provider) -> Self {
        Self {
            provider: p.name().to_owned(),
            model: p.model().to_owned(),
            parameters: p.parameters(),
            max_output_tokens: p.max_output_tokens(),
            input_bound: p.input_bound(),
            framing: p.framing(),
            calls_per_request: p.calls_per_request(),
        }
    }
}

/// A run's settings.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub trials: u32,
    /// pico-USD, over everything the evidence directory's ledger has charged.
    pub cap: u64,
    pub max_retries: u32,
    pub evidence: PathBuf,
    /// The evidence directory may not grow past this many bytes.
    pub evidence_limit: u64,
    pub price: Price,
    /// Whether a resumed run tries a runner error's request again.
    pub retry_runner_errors: bool,
}

/// One planned request.
#[derive(Debug, Clone)]
pub struct Planned<'a> {
    pub task: &'a Task,
    pub arm: Arm,
    pub trial: u32,
    /// Which of the trial's two requests for this task it is: the arms alternate by trial.
    pub order: u8,
    pub identity: String,
    pub request: Request,
}

/// The tasks a mode runs.
#[must_use]
pub fn tasks(suite: &Suite, mode: Mode) -> Vec<&Task> {
    match mode {
        Mode::Pilot => PILOT.iter().filter_map(|id| suite.task(id)).collect(),
        Mode::Authoritative => suite.tasks.iter().collect(),
    }
}

/// A request's identity.
#[must_use]
pub fn identity(task: &Task, arm: Arm, trial: u32, spec: &Spec) -> String {
    let v = json!({
        "task": task.id,
        "arm": arm,
        "trial": trial,
        "provider": spec.provider,
        "model": spec.model,
        "parameters": spec.parameters,
        "prompt_version": crate::PROMPT_VERSION,
        "prompt": prompt::digest(),
        "suite": crate::SUITE_VERSION,
        "context": task.context(arm).digest,
    });
    crate::digest(v.to_string().as_bytes())
        .get(..32)
        .unwrap_or_default()
        .to_owned()
}

/// A benchmark's identity.
#[must_use]
pub fn benchmark(suite: &Suite, mode: Mode, trials: u32, spec: &Spec) -> String {
    let v = json!({
        "suite": suite.digest,
        "suite_version": crate::SUITE_VERSION,
        "prompt_version": crate::PROMPT_VERSION,
        "prompt": prompt::digest(),
        "mode": mode,
        "trials": trials,
        "provider": spec.provider,
        "model": spec.model,
        "parameters": spec.parameters,
    });
    crate::digest(v.to_string().as_bytes())
        .get(..16)
        .unwrap_or_default()
        .to_owned()
}

/// Every request, in order: trial by trial, task by task, the baseline first in odd trials
/// and the N33 context first in even ones.
#[must_use]
pub fn plan<'a>(suite: &'a Suite, mode: Mode, trials: u32, spec: &Spec) -> Vec<Planned<'a>> {
    let mut out = Vec::new();
    for trial in 1..=trials {
        let arms = if trial % 2 == 1 {
            [Arm::Baseline, Arm::N33]
        } else {
            [Arm::N33, Arm::Baseline]
        };
        for task in tasks(suite, mode) {
            for (order, arm) in (0u8..).zip(arms) {
                out.push(Planned {
                    task,
                    arm,
                    trial,
                    order,
                    identity: identity(task, arm, trial, spec),
                    request: prompt::request(task, arm),
                });
            }
        }
    }
    out
}

/// Local counts under N34's `o200k_base`: the context payload, and the fixed rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Local {
    pub context_tokens: u64,
    pub fixed_tokens: u64,
}

/// The provider's own counts, where it has a counting endpoint: the whole request, and the
/// same request with an empty context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counted {
    pub full: u64,
    pub fixed: u64,
}

/// What became of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Answered,
    RunnerError,
    /// Answered by a model other than the one requested: kept, never compared.
    ModelDrift,
}

/// One journal line: `nazm.agent-benchmark/1`'s record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub schema: String,
    pub benchmark: String,
    /// The suite's digest: a benchmark is of one checkout's contexts.
    pub suite: String,
    pub mode: Mode,
    pub identity: String,
    pub task: String,
    pub category: Category,
    pub arm: Arm,
    pub trial: u32,
    pub order: u8,
    pub provider: String,
    pub model_requested: String,
    pub model_returned: Option<String>,
    pub prompt_version: String,
    pub prompt_digest: String,
    pub context_digest: String,
    pub context_bytes: u64,
    pub parameters: Value,
    pub status: Status,
    pub attempts: u32,
    pub retries: u32,
    pub retry_reasons: Vec<String>,
    pub usage: Option<Usage>,
    pub raw_usage: Option<Value>,
    pub fingerprint: Option<String>,
    pub stop_reason: Option<String>,
    pub local: Local,
    pub counted: Option<Counted>,
    pub price: String,
    pub cost: Option<Cost>,
    /// pico-USD this request charged to the ledger, retries included.
    pub charged: u64,
    pub latency_ms: Option<u64>,
    pub retry_latency_ms: u64,
    pub answer_digest: Option<String>,
    pub answer: Option<Value>,
    pub score: Option<Score>,
    /// The oracle's failure, or `runner_error`, or none when solved.
    pub failure: Option<String>,
}

/// Why a run stopped early. What it had done is in the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The next attempt could pass the cap.
    Budget { spent: u64, needed: u64, cap: u64 },
    /// A failure no retry fixes.
    Fatal(String),
    /// The provider answered with another model.
    ModelDrift { requested: String, returned: String },
    /// The evidence directory, the journal or the suite is not what the run needs.
    Refused(String),
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub benchmark: String,
    pub dir: PathBuf,
    pub sent: usize,
    pub resumed: usize,
    pub spent: u64,
    pub stop: Option<Stop>,
}

// Ceiling division of a bound: the intent is integer arithmetic on counts.
#[allow(clippy::integer_division)]
fn bound(tokens: u64, (num, den): (u64, u64)) -> u64 {
    (tokens.saturating_mul(num) + den.saturating_sub(1)) / den.max(1)
}

fn count(t: &nazm_tokens::Tokenizer, text: &str) -> u64 {
    t.count(text).map_or(0, |c| c.tokens as u64)
}

/// The local counts of a request.
#[must_use]
pub fn local(t: &nazm_tokens::Tokenizer, task: &Task, arm: Arm) -> Local {
    let fixed = prompt::fixed(task);
    Local {
        context_tokens: count(t, &task.context(arm).text),
        fixed_tokens: count(t, &fixed.system) + count(t, &fixed.user),
    }
}

/// The input bound a request is admitted under, before any provider count.
#[must_use]
pub fn estimate(l: Local, spec: &Spec) -> u64 {
    bound(
        l.context_tokens + l.fixed_tokens + spec.framing,
        spec.input_bound,
    )
}

/// The most one call of a request can cost, and the most the request can: the call's worst
/// case times the calls a request can become.
#[must_use]
pub fn worst(input: u64, spec: &Spec, price: &Price) -> Option<(u64, u64)> {
    let call = price::worst_case(input, spec.max_output_tokens, price)?;
    Some((call, call.checked_mul(spec.calls_per_request.max(1))?))
}

fn ledger(evidence: &Path) -> Result<u64, String> {
    let path = evidence.join("spend.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut total: u64 = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value =
            serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display()))?;
        let c = v
            .get("charged")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{}: a line has no charge", path.display()))?;
        total = total
            .checked_add(c)
            .ok_or_else(|| format!("{}: the total overflows", path.display()))?;
    }
    Ok(total)
}

/// Everything the evidence directory's ledger has charged, in pico-USD.
///
/// # Errors
///
/// When the ledger cannot be read.
pub fn spent(evidence: &Path) -> Result<u64, String> {
    ledger(evidence)
}

fn append(path: &Path, line: &Value) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{line}")
        .and_then(|()| f.sync_data())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => size(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    })
}

/// Every record of a journal.
///
/// # Errors
///
/// When a line is not a record.
pub fn journal(path: &Path) -> Result<Vec<Record>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

/// A dry run: what a run would send and the most it could cost, with no provider at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRun {
    pub benchmark: String,
    pub mode: Mode,
    pub provider: String,
    pub model: String,
    pub parameters: Value,
    pub suite: String,
    pub prompt_version: String,
    pub trials: u32,
    pub tasks: Vec<DryTask>,
    pub requests: usize,
    pub max_attempts: usize,
    pub estimated_input_tokens: u64,
    pub max_output_tokens: u64,
    pub price: String,
    /// pico-USD: every request at its worst case, once.
    pub worst_case: u64,
    /// pico-USD: and every retry too.
    pub worst_case_with_retries: u64,
    pub already_spent: u64,
    pub cap: u64,
    /// Whether every request at its worst case fits in what the cap leaves.
    pub within_cap: bool,
    /// pico-USD: the dearest single request's worst case.
    pub largest_request: u64,
    /// Whether that one fits: a run whose pass may not fit starts, and stops at the cap.
    pub startable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryTask {
    pub id: String,
    pub category: Category,
    pub scenario: String,
    pub required_facts: usize,
    pub contexts: Vec<DryContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryContext {
    pub arm: Arm,
    pub bytes: u64,
    pub digest: String,
    pub o200k_tokens: u64,
    pub fixed_o200k_tokens: u64,
}

/// What `run` would do, computed without a provider, a credential or a network.
///
/// # Errors
///
/// When the ledger cannot be read or a bound overflows.
pub fn dry_run(
    suite: &Suite,
    spec: &Spec,
    cfg: &Config,
    t: &nazm_tokens::Tokenizer,
) -> Result<DryRun, String> {
    let planned = plan(suite, cfg.mode, cfg.trials, spec);
    let mut input = 0u64;
    let mut worst = 0u64;
    let mut largest = 0u64;
    for p in &planned {
        let e = estimate(local(t, p.task, p.arm), spec);
        input += e;
        let w = self::worst(e, spec, &cfg.price)
            .ok_or("a worst case overflows")?
            .1;
        largest = largest.max(w);
        worst = worst.checked_add(w).ok_or("the worst case overflows")?;
    }
    let tasks = tasks(suite, cfg.mode)
        .iter()
        .map(|task| DryTask {
            id: task.id.to_owned(),
            category: task.category,
            scenario: task.scenario.to_owned(),
            required_facts: task.required(),
            contexts: Arm::ALL
                .iter()
                .map(|&arm| {
                    let l = local(t, task, arm);
                    DryContext {
                        arm,
                        bytes: task.context(arm).text.len() as u64,
                        digest: task.context(arm).digest.clone(),
                        o200k_tokens: l.context_tokens,
                        fixed_o200k_tokens: l.fixed_tokens,
                    }
                })
                .collect(),
        })
        .collect();
    let already = ledger(&cfg.evidence)?;
    let attempts = 1 + u64::from(cfg.max_retries);
    let with_retries = worst
        .checked_mul(attempts)
        .ok_or("the worst case overflows")?;
    Ok(DryRun {
        benchmark: benchmark(suite, cfg.mode, cfg.trials, spec),
        mode: cfg.mode,
        provider: spec.provider.clone(),
        model: spec.model.clone(),
        parameters: spec.parameters.clone(),
        suite: suite.digest.clone(),
        prompt_version: crate::PROMPT_VERSION.to_owned(),
        trials: cfg.trials,
        tasks,
        requests: planned.len(),
        max_attempts: planned.len() * (1 + cfg.max_retries as usize),
        estimated_input_tokens: input,
        max_output_tokens: planned.len() as u64 * spec.max_output_tokens,
        price: cfg.price.id.clone(),
        worst_case: worst,
        worst_case_with_retries: with_retries,
        already_spent: already,
        cap: cfg.cap,
        within_cap: already.saturating_add(worst) <= cfg.cap,
        largest_request: largest,
        startable: already.saturating_add(largest) <= cfg.cap,
    })
}

/// Runs, or resumes, the benchmark `cfg` and `provider` identify.
///
/// # Errors
///
/// When the evidence directory cannot be written. Every other reason to stop is an
/// [`Outcome`]'s `stop`, after the journal has what was done.
#[allow(clippy::too_many_lines)]
pub fn run(
    suite: &Suite,
    provider: &dyn Provider,
    cfg: &Config,
    t: &nazm_tokens::Tokenizer,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome, String> {
    let spec = Spec::of(provider);
    let bench = benchmark(suite, cfg.mode, cfg.trials, &spec);
    let dir = cfg
        .evidence
        .join(format!("{}-{}-{bench}", spec.provider, cfg.mode.name()));
    std::fs::create_dir_all(dir.join("raw")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let meta = json!({
        "schema": crate::SCHEMA,
        "benchmark": bench,
        "mode": cfg.mode,
        "trials": cfg.trials,
        "suite": suite.digest,
        "suite_version": crate::SUITE_VERSION,
        "prompt_version": crate::PROMPT_VERSION,
        "prompt_digest": prompt::digest(),
        "provider": spec.provider,
        "model": spec.model,
        "parameters": spec.parameters,
        "max_retries": cfg.max_retries,
    });
    let meta_path = dir.join("benchmark.json");
    match std::fs::read_to_string(&meta_path) {
        Ok(old) => {
            let old: Value = serde_json::from_str(&old).unwrap_or(Value::Null);
            if old != meta {
                return Ok(stopped(
                    bench,
                    dir,
                    0,
                    0,
                    0,
                    Stop::Refused(
                        "the evidence directory holds another benchmark under this identity"
                            .to_owned(),
                    ),
                ));
            }
        }
        Err(_) => std::fs::write(&meta_path, format!("{meta:#}\n"))
            .map_err(|e| format!("{}: {e}", meta_path.display()))?,
    }
    let journal_path = dir.join("journal.jsonl");
    let done: BTreeMap<String, Record> = journal(&journal_path)?
        .into_iter()
        .map(|r| (r.identity.clone(), r))
        .collect();
    let mut spent = ledger(&cfg.evidence)?;
    let spend_path = cfg.evidence.join("spend.jsonl");
    let mut counts: BTreeMap<(String, Arm), Option<Counted>> = BTreeMap::new();
    let (mut sent, mut resumed) = (0, 0);
    for p in plan(suite, cfg.mode, cfg.trials, &spec) {
        if let Some(r) = done.get(&p.identity) {
            if !(r.status == Status::RunnerError && cfg.retry_runner_errors) {
                resumed += 1;
                continue;
            }
        }
        if size(&cfg.evidence) > cfg.evidence_limit {
            return Ok(stopped(
                bench,
                dir,
                sent,
                resumed,
                spent,
                Stop::Refused(format!(
                    "the evidence directory is past its {} byte limit",
                    cfg.evidence_limit
                )),
            ));
        }
        let l = local(t, p.task, p.arm);
        let key = (p.task.id.to_owned(), p.arm);
        let counted = match counts.get(&key) {
            Some(c) => *c,
            None => {
                let full = provider.count(&p.request);
                let fixed = provider.count(&prompt::fixed(p.task));
                let c = match (full, fixed) {
                    (Ok(Some(full)), Ok(Some(fixed))) => Some(Counted { full, fixed }),
                    _ => None,
                };
                counts.insert(key, c);
                c
            }
        };
        let input = estimate(l, &spec).max(counted.map_or(0, |c| c.full + spec.framing));
        let (call_worst, worst) =
            worst(input, &spec, &cfg.price).ok_or("a worst case overflows")?;
        let mut attempts = 0;
        let mut reasons = Vec::new();
        let mut retry_ms = 0u64;
        let mut charged = 0u64;
        let mut reply = None;
        let mut fatal = None;
        while attempts <= cfg.max_retries {
            if spent.saturating_add(worst) > cfg.cap {
                return Ok(stopped(
                    bench,
                    dir,
                    sent,
                    resumed,
                    spent,
                    Stop::Budget {
                        spent,
                        needed: worst,
                        cap: cfg.cap,
                    },
                ));
            }
            attempts += 1;
            let start = Instant::now();
            let r = provider.complete_within(&p.request, call_worst);
            let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let charge = match &r {
                Ok(reply) => price::cost(&reply.usage, &cfg.price).map_or(worst, |c| c.total),
                Err(CallError::Retryable { billed: true, .. }) => worst,
                Err(_) => 0,
            };
            append(
                &spend_path,
                &json!({"benchmark": bench, "identity": p.identity, "attempt": attempts, "charged": charge}),
            )?;
            spent = spent.saturating_add(charge);
            charged = charged.saturating_add(charge);
            match r {
                Ok(x) => {
                    reply = Some((x, ms));
                    break;
                }
                Err(CallError::Retryable { reason, .. }) => {
                    log(&format!(
                        "{} {} trial {}: {reason}",
                        p.task.id,
                        p.arm.name(),
                        p.trial
                    ));
                    reasons.push(reason);
                    retry_ms = retry_ms.saturating_add(ms);
                }
                Err(CallError::Fatal { reason }) => {
                    reasons.push(reason.clone());
                    fatal = Some(reason);
                    break;
                }
            }
        }
        sent += 1;
        let context = p.task.context(p.arm);
        let mut record = Record {
            schema: crate::SCHEMA.to_owned(),
            benchmark: bench.clone(),
            suite: suite.digest.clone(),
            mode: cfg.mode,
            identity: p.identity.clone(),
            task: p.task.id.to_owned(),
            category: p.task.category,
            arm: p.arm,
            trial: p.trial,
            order: p.order,
            provider: spec.provider.clone(),
            model_requested: spec.model.clone(),
            model_returned: None,
            prompt_version: crate::PROMPT_VERSION.to_owned(),
            prompt_digest: prompt::digest(),
            context_digest: context.digest.clone(),
            context_bytes: context.text.len() as u64,
            parameters: spec.parameters.clone(),
            status: Status::RunnerError,
            attempts,
            retries: attempts.saturating_sub(1),
            retry_reasons: reasons,
            usage: None,
            raw_usage: None,
            fingerprint: None,
            stop_reason: None,
            local: l,
            counted,
            price: cfg.price.id.clone(),
            cost: None,
            charged,
            latency_ms: None,
            retry_latency_ms: 0,
            answer_digest: None,
            answer: None,
            score: None,
            failure: Some("runner_error".to_owned()),
        };
        let mut drift = None;
        if let Some((reply, ms)) = reply {
            // The retry that succeeded is the reply's latency; the ones before it are not.
            record.retry_latency_ms = retry_ms;
            record.latency_ms = Some(ms);
            record.model_returned = Some(reply.model.clone());
            record.usage = Some(reply.usage);
            record.raw_usage = Some(reply.raw_usage.clone());
            record.fingerprint.clone_from(&reply.fingerprint);
            record.stop_reason.clone_from(&reply.stop_reason);
            record.cost = price::cost(&reply.usage, &cfg.price);
            record.answer_digest = Some(crate::digest(reply.text.as_bytes()));
            record.answer = oracle::parse(&reply.text).ok().map(Value::Object);
            let raw = dir.join("raw").join(format!("{}.json", p.identity));
            std::fs::write(&raw, &reply.raw).map_err(|e| format!("{}: {e}", raw.display()))?;
            if reply.model == spec.model {
                let score = oracle::score(p.task, p.arm, &reply.text);
                record.failure = score.failure.map(|f| {
                    serde_json::to_value(f)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default()
                });
                record.score = Some(score);
                record.status = Status::Answered;
            } else {
                record.status = Status::ModelDrift;
                record.failure = Some("model_drift".to_owned());
                drift = Some(Stop::ModelDrift {
                    requested: spec.model.clone(),
                    returned: reply.model,
                });
            }
        } else {
            record.retry_latency_ms = retry_ms;
        }
        let line = serde_json::to_value(&record).map_err(|e| format!("a record: {e}"))?;
        append(&journal_path, &line)?;
        log(&format!(
            "{} {} trial {}: {}",
            p.task.id,
            p.arm.name(),
            p.trial,
            record.failure.as_deref().unwrap_or("solved")
        ));
        if let Some(stop) = drift {
            return Ok(stopped(bench, dir, sent, resumed, spent, stop));
        }
        if let Some(reason) = fatal {
            return Ok(stopped(
                bench,
                dir,
                sent,
                resumed,
                spent,
                Stop::Fatal(reason),
            ));
        }
    }
    Ok(Outcome {
        benchmark: bench,
        dir,
        sent,
        resumed,
        spent,
        stop: None,
    })
}

fn stopped(
    benchmark: String,
    dir: PathBuf,
    sent: usize,
    resumed: usize,
    spent: u64,
    stop: Stop,
) -> Outcome {
    Outcome {
        benchmark,
        dir,
        sent,
        resumed,
        spent,
        stop: Some(stop),
    }
}
