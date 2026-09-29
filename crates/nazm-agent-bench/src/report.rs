//! The report: per provider, per task and per category, correctness first and cost second,
//! from journals alone and deterministically — the same records in any order are the same
//! bytes, and nothing in it is a time.
//!
//! **Arithmetic.** Counts are summed; a median of an even number of values is the lower of
//! the two middle ones' mean, rounded down; a reduction is integer basis points of the
//! baseline, `10000 − 10000·n33/baseline` rounded half away from zero, as N34's. A ratio with
//! a zero denominator is absent (`null`), never infinite and never zero.
//!
//! **Conclusion law.** For each task, an arm's correctness is the pair (tasks solved, correct
//! facts minus false claims) over its trials, compared in that order. The arm with the higher
//! pair is better whatever it cost. Equal pairs go to the arm whose median total tokens are at
//! least 10% lower; within 10% they are equivalent. A task with a trial missing, a runner error
//! or a model drift in either arm is inconclusive.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::price::{dollars, Price};
use crate::run::{Record, Status};
use crate::suite::{Arm, Category, Suite};

/// Values' spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Stat {
    pub n: usize,
    pub sum: u64,
    pub median: Option<u64>,
    pub min: Option<u64>,
    pub max: Option<u64>,
}

// The median of an even count is the mean of the middle pair, rounded down: integer by intent.
#[allow(clippy::integer_division)]
fn stat(mut v: Vec<u64>) -> Stat {
    v.sort_unstable();
    let n = v.len();
    let median = match n {
        0 => None,
        _ if n % 2 == 1 => v.get(n / 2).copied(),
        _ => match (v.get(n / 2 - 1), v.get(n / 2)) {
            (Some(a), Some(b)) => Some(a / 2 + b / 2 + (a % 2 + b % 2) / 2),
            _ => None,
        },
    };
    Stat {
        n,
        sum: v.iter().sum(),
        median,
        min: v.first().copied(),
        max: v.last().copied(),
    }
}

/// `10000 − 10000·after/before` in basis points, rounded half away from zero; absent when
/// `before` is zero.
#[must_use]
// Integer division is the rounding.
#[allow(clippy::integer_division)]
pub fn reduction_bp(before: u64, after: u64) -> Option<i64> {
    if before == 0 {
        return None;
    }
    let (b, a) = (i128::from(before), i128::from(after));
    let num = 10_000 * (b - a);
    let q = if num >= 0 {
        (2 * num + b) / (2 * b)
    } else {
        -((2 * -num + b) / (2 * b))
    };
    i64::try_from(q).ok()
}

/// `num / den` to two decimals, rounded half up; absent when `den` is zero.
#[must_use]
#[allow(clippy::integer_division)]
pub fn ratio(num: u64, den: u64) -> Option<String> {
    if den == 0 {
        return None;
    }
    let hundredths = (u128::from(num) * 200 + u128::from(den)) / (2 * u128::from(den));
    Some(format!("{}.{:02}", hundredths / 100, hundredths % 100))
}

/// Basis points as a percentage: `93.95%`.
#[must_use]
// Integer division splits an exact count into its printed parts.
#[allow(clippy::integer_division)]
pub fn pct(bp: Option<i64>) -> String {
    bp.map_or_else(
        || "—".to_owned(),
        |bp| {
            let sign = if bp < 0 { "-" } else { "" };
            format!("{sign}{}.{:02}%", bp.abs() / 100, bp.abs() % 100)
        },
    )
}

/// One arm of one task, over its trials.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ArmRow {
    pub trials: usize,
    pub answered: usize,
    pub runner_errors: usize,
    pub model_drift: usize,
    pub solved: usize,
    /// Solved when an answer that was prose around a JSON block is read by that block.
    pub solved_lenient: usize,
    pub required_facts: usize,
    pub correct_facts: usize,
    pub wrong_claims: usize,
    pub hallucinations: usize,
    pub extra_citations: usize,
    pub invalid_outputs: usize,
    pub refusals: usize,
    pub failures: BTreeMap<String, usize>,
    pub input_tokens: Stat,
    pub output_tokens: Stat,
    pub total_tokens: Stat,
    pub cached_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub reasoning_tokens: u64,
    /// The context payload under N34's `o200k_base`.
    pub local_context_tokens: u64,
    /// Everything else the request carries, under `o200k_base`.
    pub local_fixed_tokens: u64,
    /// The context payload by the provider's own count, where it has one.
    pub provider_context_tokens: Option<u64>,
    pub provider_fixed_tokens: Option<u64>,
    /// pico-USD.
    pub cost: Stat,
    pub latency_ms: Stat,
    pub retries: u64,
    pub retry_latency_ms: u64,
}

impl ArmRow {
    fn key(&self) -> (usize, i64) {
        let net = i64::try_from(self.correct_facts).unwrap_or(i64::MAX)
            - i64::try_from(self.wrong_claims + self.hallucinations).unwrap_or(i64::MAX);
        (self.solved, net)
    }

    fn complete(&self, trials: usize) -> bool {
        self.trials == trials && self.answered == trials
    }
}

/// A task's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Conclusion {
    N33Better,
    Equivalent,
    BaselineBetter,
    Inconclusive,
}

/// One task under one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskRow {
    pub task: String,
    pub category: Category,
    pub scenario: String,
    pub baseline: ArmRow,
    pub n33: ArmRow,
    pub correctness_preserved: bool,
    pub conclusion: Conclusion,
    pub input_reduction_bp: Option<i64>,
    pub total_reduction_bp: Option<i64>,
    pub cost_reduction_bp: Option<i64>,
    pub local_context_reduction_bp: Option<i64>,
}

/// Sums over a set of tasks, per arm.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Totals {
    pub requests: usize,
    pub answered: usize,
    pub solved: usize,
    pub required_facts: usize,
    pub correct_facts: usize,
    pub wrong_claims: usize,
    pub hallucinations: usize,
    pub invalid_outputs: usize,
    pub refusals: usize,
    pub runner_errors: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub local_context_tokens: u64,
    pub cost: u64,
    pub tokens_per_solved_task: Option<String>,
    /// pico-USD.
    pub cost_per_solved_task: Option<u64>,
    pub correct_facts_per_1k_input_tokens: Option<String>,
    pub correct_facts_per_dollar: Option<String>,
}

fn totals<'a>(rows: impl Iterator<Item = &'a ArmRow>) -> Totals {
    let mut t = Totals::default();
    for r in rows {
        t.requests += r.trials;
        t.answered += r.answered;
        t.solved += r.solved;
        t.required_facts += r.required_facts;
        t.correct_facts += r.correct_facts;
        t.wrong_claims += r.wrong_claims;
        t.hallucinations += r.hallucinations;
        t.invalid_outputs += r.invalid_outputs;
        t.refusals += r.refusals;
        t.runner_errors += r.runner_errors;
        t.input_tokens += r.input_tokens.sum;
        t.output_tokens += r.output_tokens.sum;
        t.total_tokens += r.total_tokens.sum;
        t.cached_input_tokens += r.cached_input_tokens;
        t.local_context_tokens += r.local_context_tokens;
        t.cost += r.cost.sum;
    }
    let solved = t.solved as u64;
    t.tokens_per_solved_task = ratio(t.total_tokens, solved);
    t.cost_per_solved_task = (solved > 0).then(|| {
        // Rounded down to the pico-dollar: integer by intent.
        #[allow(clippy::integer_division)]
        let c = t.cost / solved;
        c
    });
    t.correct_facts_per_1k_input_tokens = ratio(t.correct_facts as u64 * 1000, t.input_tokens);
    // correct facts per dollar: facts · 10¹² / pico-USD.
    t.correct_facts_per_dollar = ratio(
        (t.correct_facts as u64).saturating_mul(1_000_000_000_000),
        t.cost,
    );
    t
}

/// A category, or all tasks, under one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Group {
    pub name: String,
    pub tasks: usize,
    pub correctness_preserved: usize,
    pub conclusions: BTreeMap<String, usize>,
    pub baseline: Totals,
    pub n33: Totals,
    pub input_reduction_bp: Option<i64>,
    pub total_reduction_bp: Option<i64>,
    pub cost_reduction_bp: Option<i64>,
    pub local_context_reduction_bp: Option<i64>,
    pub median_task_input_reduction_bp: Option<i64>,
}

// The median of basis points, by the same rule as `stat`.
#[allow(clippy::integer_division)]
fn median_bp(mut v: Vec<i64>) -> Option<i64> {
    v.sort_unstable();
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => v.get(n / 2).copied(),
        _ => match (v.get(n / 2 - 1), v.get(n / 2)) {
            (Some(a), Some(b)) => Some((a + b).div_euclid(2)),
            _ => None,
        },
    }
}

fn group(name: &str, rows: &[&TaskRow]) -> Group {
    let baseline = totals(rows.iter().map(|r| &r.baseline));
    let n33 = totals(rows.iter().map(|r| &r.n33));
    let mut conclusions = BTreeMap::new();
    for r in rows {
        let k = serde_json::to_value(r.conclusion)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        *conclusions.entry(k).or_insert(0) += 1;
    }
    Group {
        name: name.to_owned(),
        tasks: rows.len(),
        correctness_preserved: rows.iter().filter(|r| r.correctness_preserved).count(),
        conclusions,
        input_reduction_bp: reduction_bp(baseline.input_tokens, n33.input_tokens),
        total_reduction_bp: reduction_bp(baseline.total_tokens, n33.total_tokens),
        cost_reduction_bp: reduction_bp(baseline.cost, n33.cost),
        local_context_reduction_bp: reduction_bp(
            baseline.local_context_tokens,
            n33.local_context_tokens,
        ),
        median_task_input_reduction_bp: median_bp(
            rows.iter().filter_map(|r| r.input_reduction_bp).collect(),
        ),
        baseline,
        n33,
    }
}

/// Where the structured context stops costing more than the naive one, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Crossover {
    /// (task, baseline median input, N33 median input) where N33 cost more input.
    pub n33_dearer: Vec<(String, u64, u64)>,
    pub n33_cheaper: Vec<(String, u64, u64)>,
    /// The largest baseline input at which N33 was dearer, and the smallest at which it was
    /// cheaper: the crossover lies between, for this model and suite.
    pub between: (Option<u64>, Option<u64>),
}

/// One provider's results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderReport {
    pub provider: String,
    pub model_requested: String,
    pub models_returned: Vec<String>,
    pub benchmarks: Vec<String>,
    pub parameters: serde_json::Value,
    pub price: Option<Price>,
    pub trials: usize,
    pub tasks: Vec<TaskRow>,
    pub categories: Vec<Group>,
    pub all: Group,
    pub crossover: Crossover,
    pub latency_ms: BTreeMap<String, Stat>,
    pub retries: u64,
    pub retry_latency_ms: u64,
}

/// The report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub kind: &'static str,
    /// The suites the records were made under — one, for a comparable report.
    pub suites: Vec<String>,
    pub suite_version: &'static str,
    pub prompt_version: &'static str,
    pub prompt_digest: String,
    pub providers: Vec<ProviderReport>,
}

fn arm_row(records: &[&Record], required: usize) -> ArmRow {
    let mut r = ArmRow {
        trials: records.len(),
        ..ArmRow::default()
    };
    let (mut input, mut output, mut total, mut cost, mut latency) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for x in records {
        r.local_context_tokens = x.local.context_tokens;
        r.local_fixed_tokens = x.local.fixed_tokens;
        if let Some(c) = x.counted {
            r.provider_context_tokens = Some(c.full.saturating_sub(c.fixed));
            r.provider_fixed_tokens = Some(c.fixed);
        }
        r.retries += u64::from(x.retries);
        r.retry_latency_ms += x.retry_latency_ms;
        match x.status {
            Status::RunnerError => {
                r.runner_errors += 1;
                *r.failures.entry("runner_error".to_owned()).or_insert(0) += 1;
                continue;
            }
            Status::ModelDrift => {
                r.model_drift += 1;
                *r.failures.entry("model_drift".to_owned()).or_insert(0) += 1;
                continue;
            }
            Status::Answered => {}
        }
        r.answered += 1;
        r.required_facts += required;
        let lenient = x.lenient.as_ref().or(x.score.as_ref());
        r.solved_lenient += usize::from(lenient.is_some_and(|s| s.solved));
        if let Some(s) = &x.score {
            r.solved += usize::from(s.solved);
            r.correct_facts += s.correct;
            r.wrong_claims += s.wrong;
            r.hallucinations += s.hallucinated;
            r.extra_citations += s.extra_citations;
            r.invalid_outputs += usize::from(s.invalid.is_some());
            r.refusals += usize::from(s.refusal);
        }
        if let Some(f) = &x.failure {
            *r.failures.entry(f.clone()).or_insert(0) += 1;
        }
        if let Some(u) = x.usage {
            input.push(u.input_tokens);
            output.push(u.output_tokens);
            total.push(u.total_tokens());
            r.cached_input_tokens += u.cached_input_tokens;
            r.cache_write_input_tokens += u.cache_write_input_tokens;
            r.reasoning_tokens += u.reasoning_tokens.unwrap_or(0);
        }
        if let Some(c) = x.cost {
            cost.push(c.total);
        }
        if let Some(ms) = x.latency_ms {
            latency.push(ms);
        }
    }
    r.input_tokens = stat(input);
    r.output_tokens = stat(output);
    r.total_tokens = stat(total);
    r.cost = stat(cost);
    r.latency_ms = stat(latency);
    r
}

fn conclude(base: &ArmRow, n33: &ArmRow, trials: usize) -> Conclusion {
    if !base.complete(trials) || !n33.complete(trials) {
        return Conclusion::Inconclusive;
    }
    match n33.key().cmp(&base.key()) {
        std::cmp::Ordering::Greater => Conclusion::N33Better,
        std::cmp::Ordering::Less => Conclusion::BaselineBetter,
        std::cmp::Ordering::Equal => {
            let (b, n) = (
                u128::from(base.total_tokens.median.unwrap_or(0)),
                u128::from(n33.total_tokens.median.unwrap_or(0)),
            );
            if n * 10 <= b * 9 {
                Conclusion::N33Better
            } else if n * 10 >= b * 11 {
                Conclusion::BaselineBetter
            } else {
                Conclusion::Equivalent
            }
        }
    }
}

/// The report over `records`: authoritative ones only, the last record of each identity.
/// `suite` gives the tasks' order and facts; the contexts are the records' own.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build(suite: &Suite, records: &[Record], prices: &[Price]) -> Report {
    let mut last: BTreeMap<(String, String), &Record> = BTreeMap::new();
    for r in records {
        if r.mode == crate::run::Mode::Authoritative {
            last.insert((r.provider.clone(), r.identity.clone()), r);
        }
    }
    let mut by_provider: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in last.values() {
        by_provider.entry(r.provider.clone()).or_default().push(r);
    }
    let mut providers = Vec::new();
    for (provider, recs) in by_provider {
        let trials = recs.iter().map(|r| r.trial as usize).max().unwrap_or(0);
        let mut rows = Vec::new();
        for task in &suite.tasks {
            let of = |arm: Arm| -> Vec<&Record> {
                let mut v: Vec<&Record> = recs
                    .iter()
                    .copied()
                    .filter(|r| r.task == task.id && r.arm == arm)
                    .collect();
                v.sort_by_key(|r| r.trial);
                v
            };
            let (b, n) = (of(Arm::Baseline), of(Arm::N33));
            if b.is_empty() && n.is_empty() {
                continue;
            }
            let baseline = arm_row(&b, task.required());
            let n33 = arm_row(&n, task.required());
            let conclusion = conclude(&baseline, &n33, trials);
            rows.push(TaskRow {
                task: task.id.to_owned(),
                category: task.category,
                scenario: task.scenario.to_owned(),
                correctness_preserved: conclusion != Conclusion::Inconclusive
                    && n33.key() >= baseline.key(),
                conclusion,
                input_reduction_bp: reduction_bp(
                    baseline.input_tokens.median.unwrap_or(0),
                    n33.input_tokens.median.unwrap_or(0),
                ),
                total_reduction_bp: reduction_bp(
                    baseline.total_tokens.median.unwrap_or(0),
                    n33.total_tokens.median.unwrap_or(0),
                ),
                cost_reduction_bp: reduction_bp(baseline.cost.sum, n33.cost.sum),
                local_context_reduction_bp: reduction_bp(
                    baseline.local_context_tokens,
                    n33.local_context_tokens,
                ),
                baseline,
                n33,
            });
        }
        let mut categories = Vec::new();
        for c in [
            Category::Understand,
            Category::Edit,
            Category::Diagnose,
            Category::Docs,
            Category::Tests,
        ] {
            let rs: Vec<&TaskRow> = rows.iter().filter(|r| r.category == c).collect();
            if !rs.is_empty() {
                categories.push(group(c.name(), &rs));
            }
        }
        let all = group("all", &rows.iter().collect::<Vec<_>>());
        let mut dearer = Vec::new();
        let mut cheaper = Vec::new();
        for r in &rows {
            let (b, n) = (
                r.baseline.input_tokens.median.unwrap_or(0),
                r.n33.input_tokens.median.unwrap_or(0),
            );
            if b == 0 || n == 0 {
                continue;
            }
            if n > b {
                dearer.push((r.task.clone(), b, n));
            } else {
                cheaper.push((r.task.clone(), b, n));
            }
        }
        let between = (
            dearer.iter().map(|d| d.1).max(),
            cheaper.iter().map(|c| c.1).min(),
        );
        let mut latency = BTreeMap::new();
        for arm in Arm::ALL {
            latency.insert(
                arm.name().to_owned(),
                stat(
                    recs.iter()
                        .filter(|r| r.arm == arm)
                        .filter_map(|r| r.latency_ms)
                        .collect(),
                ),
            );
        }
        let first = recs.first();
        let model = first.map(|r| r.model_requested.clone()).unwrap_or_default();
        let mut returned: Vec<String> = recs
            .iter()
            .filter_map(|r| r.model_returned.clone())
            .collect();
        returned.sort();
        returned.dedup();
        let mut benchmarks: Vec<String> = recs.iter().map(|r| r.benchmark.clone()).collect();
        benchmarks.sort();
        benchmarks.dedup();
        providers.push(ProviderReport {
            price: crate::price::find(prices, &provider, &model).cloned(),
            provider,
            model_requested: model,
            models_returned: returned,
            benchmarks,
            parameters: first.map(|r| r.parameters.clone()).unwrap_or_default(),
            trials,
            retries: recs.iter().map(|r| u64::from(r.retries)).sum(),
            retry_latency_ms: recs.iter().map(|r| r.retry_latency_ms).sum(),
            tasks: rows,
            categories,
            all,
            crossover: Crossover {
                n33_dearer: dearer,
                n33_cheaper: cheaper,
                between,
            },
            latency_ms: latency,
        });
    }
    let mut suites: Vec<String> = last.values().map(|r| r.suite.clone()).collect();
    suites.sort();
    suites.dedup();
    Report {
        schema: crate::SCHEMA,
        kind: "report",
        suites,
        suite_version: crate::SUITE_VERSION,
        prompt_version: crate::PROMPT_VERSION,
        prompt_digest: crate::prompt::digest(),
        providers,
    }
}

fn opt(v: Option<u64>) -> String {
    v.map_or_else(|| "—".to_owned(), |x| x.to_string())
}

fn conclusion_name(c: Conclusion) -> &'static str {
    match c {
        Conclusion::N33Better => "N33 better",
        Conclusion::Equivalent => "equivalent",
        Conclusion::BaselineBetter => "baseline better",
        Conclusion::Inconclusive => "inconclusive",
    }
}

/// The report as Markdown tables, for `docs/performance.md`.
#[must_use]
pub fn markdown(r: &Report) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for p in &r.providers {
        let _ = writeln!(
            s,
            "### {} `{}` — {} trials per task and arm\n",
            p.provider, p.model_requested, p.trials
        );
        let _ = writeln!(
            s,
            "| Task | Category | Solved (base / N33) | Facts (base / N33) | False claims (base / N33) | Hallucinated (base / N33) | Input tokens, median (base → N33) | Output tokens, median | Total tokens, median | Cost (base → N33) | Verdict |"
        );
        let _ = writeln!(
            s,
            "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"
        );
        for t in &p.tasks {
            let (b, n) = (&t.baseline, &t.n33);
            let _ = writeln!(
                s,
                "| {} {} | {} | {}/{} / {}/{} | {}/{} / {}/{} | {}/{} / {}/{} | {} / {} | {} / {} | {} → {} ({}) | {} / {} | {} → {} ({}) | {} → {} | {} |",
                t.task,
                t.scenario,
                t.category.name(),
                b.solved,
                b.trials,
                n.solved,
                n.trials,
                b.solved_lenient,
                b.trials,
                n.solved_lenient,
                n.trials,
                b.correct_facts,
                b.required_facts,
                n.correct_facts,
                n.required_facts,
                b.wrong_claims + b.hallucinations,
                n.wrong_claims + n.hallucinations,
                b.hallucinations,
                n.hallucinations,
                opt(b.input_tokens.median),
                opt(n.input_tokens.median),
                pct(t.input_reduction_bp),
                opt(b.output_tokens.median),
                opt(n.output_tokens.median),
                opt(b.total_tokens.median),
                opt(n.total_tokens.median),
                pct(t.total_reduction_bp),
                dollars(b.cost.sum),
                dollars(n.cost.sum),
                conclusion_name(t.conclusion),
            );
        }
        let _ = writeln!(s);
        let _ = writeln!(
            s,
            "| Group | Tasks | Correctness preserved | Solved (base / N33) | Facts (base / N33) | Input reduction | Total-token reduction | Cost reduction | Context-only reduction (o200k) | Tokens per solved task (base / N33) | Cost per solved task (base / N33) |"
        );
        let _ = writeln!(
            s,
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
        );
        for g in p.categories.iter().chain(std::iter::once(&p.all)) {
            let _ = writeln!(
                s,
                "| {} | {} | {}/{} | {}/{} / {}/{} | {}/{} / {}/{} | {} | {} | {} | {} | {} / {} | {} / {} |",
                g.name,
                g.tasks,
                g.correctness_preserved,
                g.tasks,
                g.baseline.solved,
                g.baseline.requests,
                g.n33.solved,
                g.n33.requests,
                g.baseline.correct_facts,
                g.baseline.required_facts,
                g.n33.correct_facts,
                g.n33.required_facts,
                pct(g.input_reduction_bp),
                pct(g.total_reduction_bp),
                pct(g.cost_reduction_bp),
                pct(g.local_context_reduction_bp),
                g.baseline.tokens_per_solved_task.as_deref().unwrap_or("—"),
                g.n33.tokens_per_solved_task.as_deref().unwrap_or("—"),
                g.baseline.cost_per_solved_task.map_or_else(|| "—".to_owned(), dollars),
                g.n33.cost_per_solved_task.map_or_else(|| "—".to_owned(), dollars),
            );
        }
        let _ = writeln!(s);
    }
    s
}
