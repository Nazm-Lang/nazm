//! The N35 benchmark's own laws, offline: the suite and its arms, one prompt for both, the
//! oracle's exactness, the cost arithmetic, the zero guards, identities and resume, the dry
//! run, the spend cap, the retry cap, failure classification, the adapters' normalisation and
//! redaction, and a deterministic report. No test reaches a network: the runner is driven by
//! the fake provider, and the adapters by recorded responses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::integer_division
)]

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use nazm_agent_bench::oracle::{self, ClaimKind, Failure};
use nazm_agent_bench::price::{self, Price, Usage};
use nazm_agent_bench::prompt::{self, Request};
use nazm_agent_bench::provider::{
    Anthropic, CallError, Fake, Http, OpenAi, Provider, Reply, Secret, Transport,
};
use nazm_agent_bench::report::{self, Conclusion};
use nazm_agent_bench::run::{self, Config, Mode, Spec, Status, Stop};
use nazm_agent_bench::suite::{Arm, Category, Suite, Task, Truth};
use serde_json::{json, Value};

fn suite() -> &'static Suite {
    static S: OnceLock<Suite> = OnceLock::new();
    S.get_or_init(|| nazm_agent_bench::suite::build().unwrap())
}

fn registry() -> &'static nazm_tokens::Registry {
    static R: OnceLock<nazm_tokens::Registry> = OnceLock::new();
    R.get_or_init(|| nazm_tokens::Registry::load(&nazm_tokens::default_assets()).unwrap())
}

fn o200k() -> &'static nazm_tokens::Tokenizer {
    registry().get("openai-bpe/o200k_base").unwrap()
}

fn task(id: &str) -> &'static Task {
    suite().task(id).unwrap()
}

fn fake_price() -> Price {
    let prices =
        price::load(&nazm_docs::default_root().join("tools/agent-bench/pricing.toml")).unwrap();
    price::find(&prices, "fake", "fake-1").unwrap().clone()
}

/// The answer every truth asks for.
fn perfect(task: &Task) -> Value {
    let mut answer = serde_json::Map::new();
    for f in &task.fields {
        let v = match &f.truth {
            Truth::Definitions { items } | Truth::Tests { items } | Truth::Targets { items } => {
                json!(items)
            }
            Truth::Citations { required, .. } => json!(required),
            Truth::Definition { value }
            | Truth::Text { value, .. }
            | Truth::Signature { value } => {
                json!(value)
            }
            Truth::Flag { value } => json!(value),
        };
        answer.insert(f.name.to_owned(), v);
    }
    json!({"answer": answer, "insufficient_context": false})
}

fn with(task: &Task, field: &str, value: Value) -> String {
    let mut v = perfect(task);
    v["answer"][field] = value;
    v.to_string()
}

/// A reply the fake provider gives: `text`, usage from the request's length.
fn reply(request: &Request, text: &str, model: &str) -> Reply {
    let input = ((request.system.len() + request.user.len()) / 4) as u64;
    let output = (text.len() / 4) as u64;
    Reply {
        text: text.to_owned(),
        model: model.to_owned(),
        stop_reason: Some("end_turn".to_owned()),
        usage: Usage {
            input_tokens: input,
            uncached_input_tokens: input,
            cached_input_tokens: 0,
            cache_write_input_tokens: 0,
            output_tokens: output,
            reasoning_tokens: None,
        },
        raw_usage: json!({"input_tokens": input, "output_tokens": output}),
        fingerprint: None,
        raw: json!({"text": text}).to_string(),
    }
}

/// Which task a request is for, by its wording.
fn task_of(request: &Request) -> &'static Task {
    suite()
        .tasks
        .iter()
        .find(|t| request.user.contains(&t.wording))
        .unwrap()
}

/// A fake that answers every request perfectly.
fn perfect_fake() -> Fake {
    Fake::new(Box::new(|r, _| {
        Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-1"))
    }))
}

fn config(dir: &std::path::Path, mode: Mode, trials: u32, cap: u64) -> Config {
    Config {
        mode,
        trials,
        cap,
        max_retries: 2,
        evidence: dir.to_path_buf(),
        evidence_limit: 256 * 1024 * 1024,
        price: fake_price(),
        retry_runner_errors: false,
    }
}

fn records(dir: &std::path::Path) -> Vec<run::Record> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path().join("journal.jsonl");
        if p.is_file() {
            out.extend(run::journal(&p).unwrap());
        }
    }
    out
}

const DOLLAR: u64 = 1_000_000_000_000;

#[test]
fn the_suite_has_every_category_keeps_scenario_c_and_gives_each_arm_its_own_context() {
    let s = suite();
    assert_eq!(s.tasks.len(), 8);
    let categories: BTreeSet<Category> = s.tasks.iter().map(|t| t.category).collect();
    assert_eq!(categories.len(), 5, "{categories:?}");
    let c = task("T4");
    assert_eq!(c.scenario, "C diagnose N0300");
    assert_eq!(c.category, Category::Diagnose);
    assert!(c.context(Arm::Baseline).text.len() < c.context(Arm::N33).text.len());
    for t in &s.tasks {
        let (b, n) = (t.context(Arm::Baseline), t.context(Arm::N33));
        assert_ne!(b.digest, n.digest, "{}", t.id);
        assert_eq!(b.digest, nazm_agent_bench::digest(b.text.as_bytes()));
        assert_eq!(n.digest, nazm_agent_bench::digest(n.text.as_bytes()));
        assert!(b.text.starts_with("==> "), "{}", t.id);
        assert!(serde_json::from_str::<Value>(&n.text).unwrap()["schema"] == "nazm.task-context/1");
        assert!(t.required() > 0);
        for w in &t.witnesses {
            assert!(b.mentions(w) && n.mentions(w), "{}: {w}", t.id);
        }
    }
    // The N33 arm is the planner's own answer, byte for byte.
    let fresh = nazm_repo::task(
        &nazm_docs::default_root(),
        &nazm_repo::TaskRequest {
            intent: Some(nazm_repo::Intent::Understand),
            seeds: vec!["def:compiler:analyse.nz::fn check_program".to_owned()],
            state: None,
            budget: nazm_repo::Budget::default(),
        },
    )
    .to_json();
    assert_eq!(task("T1").context(Arm::N33).text, fresh);
    // Deterministic: built again, the same digest.
    assert_eq!(nazm_agent_bench::suite::build().unwrap().digest, s.digest);
}

#[test]
fn both_arms_get_one_prompt_and_only_the_context_differs() {
    for t in &suite().tasks {
        let (b, n) = (
            prompt::request(t, Arm::Baseline),
            prompt::request(t, Arm::N33),
        );
        assert_eq!(b.system, n.system, "{}", t.id);
        assert_eq!(b.system, prompt::SYSTEM);
        assert_eq!(
            b.user.replacen(&t.context(Arm::Baseline).text, "", 1),
            n.user.replacen(&t.context(Arm::N33).text, "", 1),
            "{}",
            t.id
        );
        assert_eq!(
            b.user.replacen(&t.context(Arm::Baseline).text, "", 1),
            prompt::fixed(t).user
        );
        assert!(b.user.contains(&t.wording) && b.user.contains(&t.contract()));
    }
    // And what a provider receives is that: the fake records each request.
    let seen = RefCell::new(Vec::new());
    let fake = Fake::new(Box::new(|r, _| {
        Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-1"))
    }));
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), Mode::Pilot, 1, DOLLAR);
    struct Spy<'a>(&'a Fake, &'a RefCell<Vec<Request>>);
    impl Provider for Spy<'_> {
        fn name(&self) -> &str {
            self.0.name()
        }
        fn model(&self) -> &str {
            self.0.model()
        }
        fn parameters(&self) -> Value {
            self.0.parameters()
        }
        fn max_output_tokens(&self) -> u64 {
            self.0.max_output_tokens()
        }
        fn complete(&self, r: &Request) -> Result<Reply, CallError> {
            self.1.borrow_mut().push(r.clone());
            self.0.complete(r)
        }
        fn count(&self, r: &Request) -> Result<Option<u64>, CallError> {
            self.0.count(r)
        }
        fn input_bound(&self) -> (u64, u64) {
            self.0.input_bound()
        }
    }
    let out = run::run(suite(), &Spy(&fake, &seen), &cfg, o200k(), &mut |_| {}).unwrap();
    assert_eq!(out.stop, None);
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 4);
    for pair in seen.chunks(2) {
        assert_eq!(pair[0].system, pair[1].system);
        let t = task_of(&pair[0]);
        assert_eq!(
            prompt::fixed(t).user,
            pair[1].user.replacen(&t.context(Arm::N33).text, "", 1)
        );
    }
}

#[test]
fn a_perfect_answer_solves_every_task_under_either_arm_fenced_or_not() {
    for t in &suite().tasks {
        for arm in Arm::ALL {
            let s = oracle::score(t, arm, &perfect(t).to_string());
            assert!(s.solved, "{} {arm:?}: {s:?}", t.id);
            assert_eq!(
                (s.correct, s.required, s.missing),
                (t.required(), t.required(), 0)
            );
            assert_eq!(s.failure, None);
            let fenced = format!("```json\n{:#}\n```\n", perfect(t));
            assert!(oracle::score(t, arm, &fenced).solved, "{}", t.id);
        }
    }
}

#[test]
fn the_oracle_counts_missing_wrong_and_hallucinated_claims_separately() {
    let t = task("T2");
    let answer = json!({
        "answer": {
            "dependencies": ["module.nz::core_key"],
            "callers": [
                "compiler/check.nz::main",
                "def:compiler:emit.nz::fn main",
                "module.nz::attach_prelude_everywhere",
                "module.nz::core_module"
            ]
        },
        "insufficient_context": false
    });
    let s = oracle::score(t, Arm::Baseline, &answer.to_string());
    assert_eq!(s.required, 4);
    assert_eq!(s.correct, 3);
    assert_eq!(s.missing, 1);
    assert_eq!(s.hallucinated, 1, "{:?}", s.claims);
    assert_eq!(s.wrong, 1, "{:?}", s.claims);
    assert!(!s.solved);
    assert_eq!(s.failure, Some(Failure::HallucinatedFact));
    let kinds: Vec<(String, ClaimKind)> =
        s.claims.iter().map(|c| (c.value.clone(), c.kind)).collect();
    assert!(kinds.contains(&(
        "module.nz::attach_prelude_everywhere".to_owned(),
        ClaimKind::Hallucinated
    )));
    assert!(kinds.contains(&("module.nz::core_module".to_owned(), ClaimKind::Wrong)));

    // Only missing: the least serious failure.
    let s = oracle::score(t, Arm::N33, &with(t, "callers", json!(["check.nz::main"])));
    assert_eq!(
        (s.correct, s.missing, s.wrong, s.hallucinated),
        (3, 1, 0, 0)
    );
    assert_eq!(s.failure, Some(Failure::MissingRequiredFact));

    // A wrong scalar is a missing fact and a false claim; an invented one is hallucinated.
    let t = task("T4");
    let s = oracle::score(t, Arm::N33, &with(t, "declared_return_type", json!("Str")));
    assert_eq!((s.correct, s.wrong, s.hallucinated), (2, 1, 0));
    let s = oracle::score(
        t,
        Arm::N33,
        &with(t, "declared_return_type", json!("Float64")),
    );
    assert_eq!((s.correct, s.wrong, s.hallucinated), (2, 0, 1));
    // A wrong flag is a wrong fact.
    let t = task("T5");
    let s = oracle::score(
        t,
        Arm::Baseline,
        &with(t, "bare_return_supported", json!(true)),
    );
    assert_eq!((s.correct, s.wrong), (5, 1));
    assert_eq!(s.failure, Some(Failure::WrongRepositoryFact));
    // Test names by their last segment.
    let t = task("T8");
    let items: Vec<String> = match &t.fields[0].truth {
        Truth::Tests { items } => items.iter().map(|n| format!("selfhost::{n}()")).collect(),
        _ => unreachable!(),
    };
    assert!(oracle::score(t, Arm::Baseline, &with(t, "tests", json!(items))).solved);
    // A test that is not one: named in the context, a wrong fact; named nowhere, hallucinated.
    let mut more = items.clone();
    more.push("core_key".to_owned());
    more.push("the_prelude_is_loaded_twice_and_nobody_notices".to_owned());
    let s = oracle::score(t, Arm::Baseline, &with(t, "tests", json!(more)));
    assert_eq!(
        (s.correct, s.wrong, s.hallucinated),
        (2, 1, 1),
        "{:?}",
        s.claims
    );
    assert!(!s.solved);
}

#[test]
fn a_bare_name_counts_only_when_it_names_one_truth() {
    let t = task("T2");
    let s = oracle::score(t, Arm::N33, &with(t, "callers", json!(["main"])));
    assert_eq!(s.correct, 2, "both dependencies, no caller");
    assert_eq!(s.wrong, 1);
    let t = task("T4");
    assert!(oracle::score(t, Arm::Baseline, &with(t, "function", json!("describe"))).solved);
    assert!(
        oracle::score(
            t,
            Arm::Baseline,
            &with(t, "offending_expression", json!("positive"))
        )
        .solved
    );
}

#[test]
fn invalid_output_and_refusal_are_failures_of_their_own() {
    let t = task("T6");
    let s = oracle::score(t, Arm::N33, "The keyword is struct.");
    assert!(s.invalid.is_some());
    assert_eq!(s.failure, Some(Failure::InvalidStructuredOutput));
    assert_eq!(s.correct, 0);
    let s = oracle::score(t, Arm::N33, r#"{"keyword": "struct"}"#);
    assert_eq!(s.failure, Some(Failure::InvalidStructuredOutput));
    // Prose around a fenced answer breaks the contract; the lenient reading is kept apart.
    let prose = format!(
        "The production is `record`.\n\n```json\n{}\n```",
        perfect(t)
    );
    assert_eq!(
        oracle::score(t, Arm::N33, &prose).failure,
        Some(Failure::InvalidStructuredOutput)
    );
    let block = oracle::fenced(&prose).unwrap();
    assert!(oracle::score(t, Arm::N33, &block).solved);
    assert_eq!(oracle::fenced(&perfect(t).to_string()), None);
    assert_eq!(oracle::fenced("no fence at all"), None);
    let s = oracle::score(
        t,
        Arm::N33,
        r#"{"answer": {}, "insufficient_context": true}"#,
    );
    assert!(s.refusal);
    assert_eq!(s.failure, Some(Failure::Refusal));
    // A refusal that answers anyway is still not solved.
    let mut v = perfect(t);
    v["insufficient_context"] = json!(true);
    let s = oracle::score(t, Arm::N33, &v.to_string());
    assert!(!s.solved);
    assert_eq!(s.failure, Some(Failure::Refusal));
}

#[test]
fn a_real_extra_citation_is_neither_credit_nor_false_and_an_invented_one_is_hallucinated() {
    let t = task("T5");
    let s = oracle::score(
        t,
        Arm::Baseline,
        &with(
            t,
            "citations",
            json!(["spec:returns", "docs/user/spec.md#return", "spec:blocks"]),
        ),
    );
    assert!(s.solved, "{s:?}");
    assert_eq!(s.extra_citations, 1);
    let s = oracle::score(
        t,
        Arm::Baseline,
        &with(t, "citations", json!(["spec:return", "spec:early-exit"])),
    );
    assert_eq!((s.correct, s.hallucinated), (5, 1));
}

#[test]
fn cost_arithmetic_is_exact_and_prices_each_category_at_its_own_rate() {
    let p = fake_price();
    assert_eq!(
        (p.input, p.cached_input, p.cache_write_input, p.output),
        (1_000_000, 100_000, 1_250_000, 5_000_000)
    );
    let u = Usage {
        input_tokens: 1700,
        uncached_input_tokens: 1000,
        cached_input_tokens: 500,
        cache_write_input_tokens: 200,
        output_tokens: 300,
        reasoning_tokens: Some(40),
    };
    let c = price::cost(&u, &p).unwrap();
    assert_eq!(c.input, 1_000_000_000);
    assert_eq!(c.cached_input, 50_000_000);
    assert_eq!(c.cache_write_input, 250_000_000);
    assert_eq!(c.output, 1_500_000_000);
    assert_eq!(c.total, 2_800_000_000);
    assert_eq!(price::dollars(c.total), "$0.002800");
    assert_eq!(price::dollars(499_999), "$0.000000");
    assert_eq!(price::dollars(500_000), "$0.000001");
    assert_eq!(u.total_tokens(), 2000);
    assert_eq!(
        price::worst_case(1000, 1024, &p),
        // Input at the dearest input rate: here a cache write's.
        Some(1_250_000_000 + 1024 * 5_000_000)
    );
    assert_eq!(price::micros("0.40"), Ok(400_000));
    assert_eq!(price::micros("1"), Ok(1_000_000));
    assert_eq!(price::micros("0.025"), Ok(25_000));
    for bad in ["1.2345678", "-1", "abc", "", ".5", "1."] {
        assert!(price::micros(bad).is_err(), "{bad}");
    }
    assert_eq!(price::cap("2"), Ok(2 * DOLLAR));
    // Every real entry of the list parses, and the two providers' entries are there.
    let prices =
        price::load(&nazm_docs::default_root().join("tools/agent-bench/pricing.toml")).unwrap();
    let openai = price::find(&prices, "openai", "gpt-4.1-mini-2025-04-14").unwrap();
    assert_eq!(
        (openai.input, openai.cached_input, openai.output),
        (400_000, 100_000, 1_600_000)
    );
    let anthropic = price::find(&prices, "anthropic", "claude-haiku-4-5-20251001").unwrap();
    assert_eq!(
        (
            anthropic.input,
            anthropic.cached_input,
            anthropic.cache_write_input,
            anthropic.output
        ),
        (1_000_000, 100_000, 1_250_000, 5_000_000)
    );
}

#[test]
fn a_zero_denominator_is_an_absent_ratio_never_an_infinity() {
    assert_eq!(report::ratio(5, 0), None);
    assert_eq!(report::ratio(0, 5), Some("0.00".to_owned()));
    assert_eq!(report::ratio(2, 3), Some("0.67".to_owned()));
    assert_eq!(report::reduction_bp(0, 5), None);
    assert_eq!(report::reduction_bp(100, 3), Some(9700));
    assert_eq!(report::reduction_bp(40, 563), Some(-130_750));
    // A benchmark where nothing is solved and nothing is spent.
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Box::new(|r, _| {
        let mut x = reply(r, "{\"answer\": {\"x\": 1}}", "fake-1");
        x.usage = Usage::default();
        Ok(x)
    }));
    let out = run::run(
        suite(),
        &fake,
        &config(dir.path(), Mode::Authoritative, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out.stop, None);
    let r = report::build(suite(), &records(dir.path()), &[fake_price()]);
    let all = &r.providers[0].all;
    assert_eq!(all.n33.solved, 0);
    assert_eq!(all.n33.tokens_per_solved_task, None);
    assert_eq!(all.n33.cost_per_solved_task, None);
    assert_eq!(all.n33.correct_facts_per_1k_input_tokens, None);
    assert_eq!(all.n33.correct_facts_per_dollar, None);
    assert_eq!(all.input_reduction_bp, None);
    let text = serde_json::to_string(&r).unwrap();
    assert!(!text.contains("NaN") && !text.contains("inf"));
}

#[test]
fn an_identity_is_everything_that_decides_a_result_and_nothing_else() {
    let t = task("T1");
    let spec = Spec::of(&perfect_fake());
    let expected = |task: &Task, arm: Arm, trial: u32, spec: &Spec| {
        let v = json!({
            "task": task.id,
            "arm": arm,
            "trial": trial,
            "provider": spec.provider,
            "model": spec.model,
            "parameters": spec.parameters,
            "prompt_version": nazm_agent_bench::PROMPT_VERSION,
            "prompt": prompt::digest(),
            "suite": nazm_agent_bench::SUITE_VERSION,
            "context": task.context(arm).digest,
        });
        nazm_agent_bench::digest(v.to_string().as_bytes())[..32].to_owned()
    };
    assert_eq!(
        run::identity(t, Arm::N33, 1, &spec),
        expected(t, Arm::N33, 1, &spec)
    );
    let mut ids = BTreeSet::new();
    for arm in Arm::ALL {
        for trial in 1..=2 {
            ids.insert(run::identity(t, arm, trial, &spec));
        }
    }
    ids.insert(run::identity(task("T2"), Arm::N33, 1, &spec));
    let mut other = spec.clone();
    other.model = "fake-2".to_owned();
    ids.insert(run::identity(t, Arm::N33, 1, &other));
    let mut other = spec.clone();
    other.parameters["temperature"] = json!(1);
    ids.insert(run::identity(t, Arm::N33, 1, &other));
    assert_eq!(ids.len(), 7, "every changed input is another identity");
    // A price is not part of it.
    assert!(!expected(t, Arm::N33, 1, &spec).is_empty());
    let b = run::benchmark(suite(), Mode::Authoritative, 2, &spec);
    assert_ne!(b, run::benchmark(suite(), Mode::Pilot, 2, &spec));
    assert_ne!(b, run::benchmark(suite(), Mode::Authoritative, 3, &spec));
    let mut other = spec.clone();
    other.model = "fake-2".to_owned();
    assert_ne!(b, run::benchmark(suite(), Mode::Authoritative, 2, &other));
    let v = json!({
        "suite": suite().digest,
        "suite_version": nazm_agent_bench::SUITE_VERSION,
        "prompt_version": nazm_agent_bench::PROMPT_VERSION,
        "prompt": prompt::digest(),
        "mode": Mode::Authoritative,
        "trials": 2,
        "provider": spec.provider,
        "model": spec.model,
        "parameters": spec.parameters,
    });
    assert_eq!(b, nazm_agent_bench::digest(v.to_string().as_bytes())[..16]);
}

#[test]
fn the_plan_is_every_task_arm_and_trial_with_the_arms_alternating() {
    let spec = Spec::of(&perfect_fake());
    let plan = run::plan(suite(), Mode::Authoritative, 2, &spec);
    assert_eq!(plan.len(), 8 * 2 * 2);
    assert!(
        plan.iter().any(|p| p.task.id == "T4"),
        "Scenario C stays in the benchmark"
    );
    for p in &plan {
        let first = if p.trial % 2 == 1 {
            Arm::Baseline
        } else {
            Arm::N33
        };
        assert_eq!(
            p.order == 0,
            p.arm == first,
            "{} trial {}",
            p.task.id,
            p.trial
        );
        assert_eq!(p.request, prompt::request(p.task, p.arm));
    }
    let ids: BTreeSet<&String> = plan.iter().map(|p| &p.identity).collect();
    assert_eq!(ids.len(), plan.len());
    let pilot = run::plan(suite(), Mode::Pilot, 1, &spec);
    let tasks: BTreeSet<&str> = pilot.iter().map(|p| p.task.id).collect();
    assert_eq!(tasks, BTreeSet::from(["T4", "T8"]));
}

#[test]
fn a_resumed_run_sends_only_what_was_not_done_and_a_new_model_is_a_new_benchmark() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), Mode::Pilot, 1, DOLLAR);
    // Interrupted: the second request fails for good.
    let failing = Fake::new(Box::new(|r, i| {
        if i == 1 {
            Err(CallError::Fatal {
                reason: "HTTP 401: no".to_owned(),
            })
        } else {
            Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-1"))
        }
    }));
    let out = run::run(suite(), &failing, &cfg, o200k(), &mut |_| {}).unwrap();
    assert!(matches!(out.stop, Some(Stop::Fatal(_))));
    assert_eq!((out.sent, failing.calls.get()), (2, 2));
    // Resumed: the done one is not sent again, and the runner error is not retried.
    let fake = perfect_fake();
    let out = run::run(suite(), &fake, &cfg, o200k(), &mut |_| {}).unwrap();
    assert_eq!(out.stop, None);
    assert_eq!((out.sent, out.resumed, fake.calls.get()), (2, 2, 2));
    // Asked to, it retries the runner error — once more, not until an answer appears.
    let mut again = cfg.clone();
    again.retry_runner_errors = true;
    let fake = perfect_fake();
    let out = run::run(suite(), &fake, &again, o200k(), &mut |_| {}).unwrap();
    assert_eq!((out.sent, fake.calls.get()), (1, 1));
    // Nothing left.
    let fake = perfect_fake();
    let out = run::run(suite(), &fake, &cfg, o200k(), &mut |_| {}).unwrap();
    assert_eq!((out.sent, out.resumed, fake.calls.get()), (0, 4, 0));
    // Another model is another benchmark, from the start.
    let mut fake = perfect_fake();
    fake.model = "fake-2".to_owned();
    fake.script = Box::new(|r, _| Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-2")));
    let out = run::run(suite(), &fake, &cfg, o200k(), &mut |_| {}).unwrap();
    assert_eq!((out.sent, out.resumed), (4, 0));
}

#[test]
fn the_cap_is_never_passed_before_or_during_a_run() {
    let dir = tempfile::tempdir().unwrap();
    // A cap below the first request's worst case: nothing is sent.
    let fake = perfect_fake();
    let out = run::run(
        suite(),
        &fake,
        &config(dir.path(), Mode::Pilot, 1, 1),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert!(matches!(out.stop, Some(Stop::Budget { .. })));
    assert_eq!((out.sent, fake.calls.get()), (0, 0));
    // A cap that admits some: the run stops before the attempt that could pass it.
    let spec = Spec::of(&fake);
    let p = fake_price();
    let first = run::plan(suite(), Mode::Pilot, 1, &spec);
    let worst: Vec<u64> = first
        .iter()
        .map(|x| {
            price::worst_case(
                run::estimate(run::local(o200k(), x.task, x.arm), &spec),
                1024,
                &p,
            )
            .unwrap()
        })
        .collect();
    let cap = worst[0] + worst[1] / 2;
    let fake = perfect_fake();
    let out = run::run(
        suite(),
        &fake,
        &config(dir.path(), Mode::Pilot, 1, cap),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert!(
        matches!(out.stop, Some(Stop::Budget { .. })),
        "{:?}",
        out.stop
    );
    assert!(out.spent <= cap);
    assert!(fake.calls.get() >= 1);
    assert_eq!(run::spent(dir.path()).unwrap(), out.spent);
    // The dry run says the same before anything is sent.
    let dry = run::dry_run(
        suite(),
        &spec,
        &config(dir.path(), Mode::Pilot, 1, 1),
        o200k(),
    )
    .unwrap();
    assert!(!dry.within_cap);
}

#[test]
fn a_transport_failure_is_retried_twice_and_is_never_an_incorrect_answer() {
    let dir = tempfile::tempdir().unwrap();
    let down = Fake::new(Box::new(|_, _| {
        Err(CallError::Retryable {
            reason: "HTTP 503: overloaded".to_owned(),
            billed: false,
        })
    }));
    let out = run::run(
        suite(),
        &down,
        &config(dir.path(), Mode::Authoritative, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out.stop, None);
    assert_eq!(
        down.calls.get(),
        16 * 3,
        "one try and two retries per request"
    );
    let recs = records(dir.path());
    assert_eq!(recs.len(), 16);
    for r in &recs {
        assert_eq!(r.status, Status::RunnerError);
        assert_eq!((r.attempts, r.retries), (3, 2));
        assert_eq!(r.score, None);
        assert_eq!(r.failure.as_deref(), Some("runner_error"));
        assert_eq!(r.charged, 0, "a refusal by the server is not billed");
    }
    let rep = report::build(suite(), &recs, &[fake_price()]);
    for t in &rep.providers[0].tasks {
        assert_eq!(t.conclusion, Conclusion::Inconclusive);
        assert_eq!(
            (t.n33.runner_errors, t.n33.answered, t.n33.required_facts),
            (1, 0, 0)
        );
        assert!(!t.correctness_preserved);
    }
    // One failure and then a reply: answered, the retry's time kept apart.
    let dir = tempfile::tempdir().unwrap();
    let flaky = Fake::new(Box::new(|r, i| {
        if i % 2 == 0 {
            Err(CallError::Retryable {
                reason: "curl exited with 28".to_owned(),
                billed: true,
            })
        } else {
            Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-1"))
        }
    }));
    let out = run::run(
        suite(),
        &flaky,
        &config(dir.path(), Mode::Pilot, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out.stop, None);
    for r in records(dir.path()) {
        assert_eq!((r.status, r.attempts, r.retries), (Status::Answered, 2, 1));
        assert!(r.score.unwrap().solved);
        assert!(
            r.charged > r.cost.unwrap().total,
            "a transport failure is charged its worst case"
        );
    }
}

#[test]
fn a_reply_from_another_model_is_recorded_and_stops_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Box::new(|r, _| Ok(reply(r, "{}", "fake-1-preview"))));
    let out = run::run(
        suite(),
        &fake,
        &config(dir.path(), Mode::Pilot, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert!(matches!(out.stop, Some(Stop::ModelDrift { .. })));
    let recs = records(dir.path());
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].status, Status::ModelDrift);
    assert_eq!(recs[0].score, None);
}

/// What a recorded transport was sent: each URL, its headers and its body.
type Sent = Vec<(String, Vec<(String, String)>, String)>;

/// A transport that answers with one recorded response and keeps what it was sent.
struct Recorded {
    status: u16,
    body: String,
    sent: RefCell<Sent>,
    fail: Option<String>,
}

impl Transport for Recorded {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<Http, String> {
        self.sent.borrow_mut().push((
            url.to_owned(),
            headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            body.to_owned(),
        ));
        match &self.fail {
            Some(e) => Err(e.clone()),
            None => Ok(Http {
                status: self.status,
                body: self.body.clone(),
            }),
        }
    }
}

fn recorded(status: u16, body: Value) -> Recorded {
    Recorded {
        status,
        body: body.to_string(),
        sent: RefCell::new(Vec::new()),
        fail: None,
    }
}

const KEY: &str = "sk-test-0123456789abcdef";

#[test]
fn openai_usage_is_normalised_and_the_key_never_leaks() {
    let body = json!({
        "model": "gpt-4.1-mini-2025-04-14",
        "system_fingerprint": "fp_1",
        "choices": [{"message": {"content": "{\"answer\": {}}"}, "finish_reason": "stop"}],
        "usage": {
            "prompt_tokens": 1200,
            "completion_tokens": 80,
            "prompt_tokens_details": {"cached_tokens": 1024},
            "completion_tokens_details": {"reasoning_tokens": 0}
        }
    });
    let p = OpenAi {
        transport: recorded(200, body),
        key: Secret::new(KEY),
        model: "gpt-4.1-mini-2025-04-14".to_owned(),
        max_output_tokens: 1024,
        seed: 35,
    };
    let r = prompt::request(task("T4"), Arm::N33);
    let reply = p.complete(&r).unwrap();
    assert_eq!(reply.model, "gpt-4.1-mini-2025-04-14");
    assert_eq!(
        reply.usage,
        Usage {
            input_tokens: 1200,
            uncached_input_tokens: 176,
            cached_input_tokens: 1024,
            cache_write_input_tokens: 0,
            output_tokens: 80,
            reasoning_tokens: Some(0),
        }
    );
    assert_eq!(reply.stop_reason.as_deref(), Some("stop"));
    let sent = p.transport.sent.borrow();
    let (url, headers, body) = &sent[0];
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    assert!(!body.contains(KEY));
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["max_completion_tokens"], 1024);
    assert!(body.get("tools").is_none());
    assert_eq!(body["messages"][0]["content"], r.system);
    assert_eq!(body["messages"][1]["content"], r.user);
    assert!(headers
        .iter()
        .any(|(k, v)| k == "authorization" && v.ends_with(KEY)));
    assert_eq!(format!("{:?}", p.key), "Secret([redacted])");
    // An error body that echoes the key is kept without it.
    let p = OpenAi {
        transport: recorded(401, json!({"error": format!("bad key {KEY}")})),
        ..p
    };
    match p.complete(&r) {
        Err(CallError::Fatal { reason }) => {
            assert!(
                !reason.contains(KEY) && reason.contains("[redacted]"),
                "{reason}"
            );
        }
        other => panic!("{other:?}"),
    }
    let p = OpenAi {
        transport: recorded(429, json!({})),
        ..p
    };
    assert!(matches!(
        p.complete(&r),
        Err(CallError::Retryable { billed: false, .. })
    ));
    let mut t = recorded(0, json!({}));
    t.fail = Some(format!("curl exited with 28: timeout sending {KEY}"));
    let p = OpenAi { transport: t, ..p };
    match p.complete(&r) {
        Err(CallError::Retryable {
            reason,
            billed: true,
        }) => assert!(!reason.contains(KEY)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn anthropic_usage_is_normalised_and_counted_without_generating() {
    let body = json!({
        "model": "claude-haiku-4-5-20251001",
        "content": [{"type": "text", "text": "{\"answer\""}, {"type": "text", "text": ": {}}"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 100, "cache_read_input_tokens": 50, "cache_creation_input_tokens": 25, "output_tokens": 9}
    });
    let p = Anthropic {
        transport: recorded(200, body),
        key: Secret::new(KEY),
        model: "claude-haiku-4-5-20251001".to_owned(),
        max_output_tokens: 1024,
    };
    let r = prompt::request(task("T8"), Arm::Baseline);
    let reply = p.complete(&r).unwrap();
    assert_eq!(reply.text, "{\"answer\": {}}");
    assert_eq!(reply.usage.input_tokens, 175);
    assert_eq!(reply.usage.uncached_input_tokens, 100);
    assert_eq!(reply.usage.cached_input_tokens, 50);
    assert_eq!(reply.usage.cache_write_input_tokens, 25);
    assert_eq!(reply.usage.output_tokens, 9);
    {
        let sent = p.transport.sent.borrow();
        let (url, headers, body) = &sent[0];
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
        assert!(!body.contains(KEY));
        assert!(headers.iter().any(|(k, v)| k == "x-api-key" && v == KEY));
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["system"], r.system);
        assert_eq!(body["temperature"], 0);
        assert!(body.get("tools").is_none());
    }
    let p = Anthropic {
        transport: recorded(200, json!({"input_tokens": 4321})),
        ..p
    };
    assert_eq!(p.count(&r), Ok(Some(4321)));
    assert!(p.transport.sent.borrow()[0]
        .0
        .ends_with("/v1/messages/count_tokens"));
    let p = Anthropic {
        transport: recorded(529, json!({"type": "overloaded_error"})),
        ..p
    };
    assert!(matches!(p.complete(&r), Err(CallError::Retryable { .. })));
    let p = Anthropic {
        transport: recorded(400, json!({"type": "invalid_request_error"})),
        ..p
    };
    assert!(matches!(p.complete(&r), Err(CallError::Fatal { .. })));
}

#[test]
fn the_report_is_the_same_for_the_same_records_in_any_order_and_puts_correctness_first() {
    let dir = tempfile::tempdir().unwrap();
    // Both arms answer perfectly, except that the baseline misses one of T7's tests.
    let fake = Fake::new(Box::new(|r, _| {
        let t = task_of(r);
        let baseline = r.user.contains(&t.context(Arm::Baseline).text);
        let mut v = perfect(t);
        if baseline && t.id == "T7" {
            let items = v["answer"]["tests"].as_array().unwrap()[1..].to_vec();
            v["answer"]["tests"] = json!(items);
        }
        Ok(reply(r, &v.to_string(), "fake-1"))
    }));
    // Not a test of the cap: every one of the 32 calls must be made, and a context carries the
    // repository's documents, which grow (Gate 3's spec took two trials past $1 at the fake
    // price). The cap is far above what they cost.
    let out = run::run(
        suite(),
        &fake,
        &config(dir.path(), Mode::Authoritative, 2, 100 * DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out.stop, None);
    let mut recs = records(dir.path());
    assert_eq!(recs.len(), 32);
    let a = serde_json::to_string(&report::build(suite(), &recs, &[fake_price()])).unwrap();
    recs.reverse();
    let b = serde_json::to_string(&report::build(suite(), &recs, &[fake_price()])).unwrap();
    assert_eq!(a, b);
    recs.sort_by(|x, y| x.identity.cmp(&y.identity));
    assert_eq!(
        a,
        serde_json::to_string(&report::build(suite(), &recs, &[fake_price()])).unwrap()
    );
    let rep = report::build(suite(), &recs, &[fake_price()]);
    let p = &rep.providers[0];
    assert_eq!(p.tasks.len(), 8);
    let row = |id: &str| p.tasks.iter().find(|t| t.task == id).unwrap();
    // More correct wins; equal correctness goes to the cheaper arm; the tiny case to the baseline.
    assert_eq!(row("T7").conclusion, Conclusion::N33Better);
    assert_eq!(row("T1").conclusion, Conclusion::N33Better);
    assert_eq!(row("T4").conclusion, Conclusion::BaselineBetter);
    assert!(row("T4").input_reduction_bp.unwrap() < 0);
    assert!(row("T4").correctness_preserved);
    assert!(!row("T7").baseline.failures.is_empty() && row("T7").n33.failures.is_empty());
    assert_eq!(p.all.tasks, 8);
    assert_eq!(p.categories.len(), 5);
    assert_eq!(p.crossover.n33_dearer.len(), 1);
    assert_eq!(p.crossover.n33_dearer[0].0, "T4");
    assert!(p.crossover.between.0 < p.crossover.between.1);
    assert!(report::markdown(&rep).contains("| T4 C diagnose N0300 |"));
    // Pilots never reach a report.
    let pilot_dir = tempfile::tempdir().unwrap();
    run::run(
        suite(),
        &perfect_fake(),
        &config(pilot_dir.path(), Mode::Pilot, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    assert!(report::build(suite(), &records(pilot_dir.path()), &[])
        .providers
        .is_empty());
}

/// The keywords `schema/nazm.agent-benchmark-1.json` uses: required, additionalProperties,
/// enum, const, type, items, $ref, oneOf, pattern (as a length check of hex digests).
fn conforms(v: &Value, node: &Value, root: &Value) -> Result<(), String> {
    if let Some(r) = node.get("$ref").and_then(Value::as_str) {
        let name = r.trim_start_matches("#/$defs/");
        return conforms(v, &root["$defs"][name], root);
    }
    if let Some(options) = node.get("oneOf").and_then(Value::as_array) {
        let n = options
            .iter()
            .filter(|o| conforms(v, o, root).is_ok())
            .count();
        if n != 1 {
            return Err(format!("{n} alternatives accept {v}"));
        }
    }
    if let Some(c) = node.get("const") {
        if v != c {
            return Err(format!("{v} is not {c}"));
        }
    }
    if let Some(e) = node.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            return Err(format!("{v} is not one of {e:?}"));
        }
    }
    if let Some(t) = node.get("type") {
        let types: Vec<&str> = t.as_array().map_or_else(
            || vec![t.as_str().unwrap()],
            |a| a.iter().filter_map(Value::as_str).collect(),
        );
        let actual = match v {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(n) if n.is_u64() || n.is_i64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        if !types.contains(&actual) {
            return Err(format!("{v} is {actual}, not {types:?}"));
        }
        if actual == "null" {
            return Ok(());
        }
    }
    if let (Some(p), Some(s)) = (node.get("pattern").and_then(Value::as_str), v.as_str()) {
        let hex = s
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        let len = p.trim_start_matches("^[0-9a-f]{").trim_end_matches("}$");
        if !hex || len.parse::<usize>().ok() != Some(s.len()) {
            return Err(format!("{s} is not {p}"));
        }
    }
    if let Some(obj) = v.as_object() {
        for k in node
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !obj.contains_key(k.as_str().unwrap()) {
                return Err(format!("{k} missing"));
            }
        }
        if let Some(props) = node.get("properties").and_then(Value::as_object) {
            if node.get("additionalProperties") == Some(&Value::Bool(false)) {
                if let Some(k) = obj.keys().find(|k| !props.contains_key(*k)) {
                    return Err(format!("{k} is not in the schema"));
                }
            }
            for (k, sub) in props {
                if let Some(x) = obj.get(k) {
                    conforms(x, sub, root).map_err(|e| format!("{k}: {e}"))?;
                }
            }
        }
    }
    if let (Some(items), Some(a)) = (node.get("items"), v.as_array()) {
        for x in a {
            conforms(x, items, root)?;
        }
    }
    Ok(())
}

#[test]
fn every_record_the_report_and_the_suite_are_the_published_schema() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            nazm_docs::default_root().join("schema/nazm.agent-benchmark-1.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mixed = Fake::new(Box::new(|r, i| match i % 5 {
        0 => Err(CallError::Retryable {
            reason: "HTTP 503".to_owned(),
            billed: false,
        }),
        1 => Ok(reply(r, "not json", "fake-1")),
        _ => Ok(reply(r, &perfect(task_of(r)).to_string(), "fake-1")),
    }));
    run::run(
        suite(),
        &mixed,
        &config(dir.path(), Mode::Authoritative, 1, DOLLAR),
        o200k(),
        &mut |_| {},
    )
    .unwrap();
    let recs = records(dir.path());
    assert!(
        recs.iter().any(|r| r.status == Status::RunnerError) || recs.iter().any(|r| r.retries > 0)
    );
    for r in &recs {
        let v = serde_json::to_value(r).unwrap();
        conforms(&v, &schema, &schema).unwrap_or_else(|e| panic!("{e}: {v}"));
    }
    let rep = serde_json::to_value(report::build(suite(), &recs, &[fake_price()])).unwrap();
    conforms(&rep, &schema, &schema).unwrap_or_else(|e| panic!("{e}"));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nazm-agent-bench"))
        .arg("suite")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    conforms(&v, &schema, &schema).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(v["suite"], suite().digest);
}

#[test]
fn the_dry_run_needs_no_credential_and_no_network_and_a_run_refuses_without_them() {
    let home = tempfile::tempdir().unwrap();
    let evidence = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_nazm-agent-bench");
    let cli = |args: &[&str]| {
        std::process::Command::new(bin)
            .args(args)
            .arg("--evidence")
            .arg(evidence.path())
            .env_clear()
            .env("HOME", home.path())
            // No curl on the path: a dry run that reached for one would fail.
            .env("PATH", "")
            .output()
            .unwrap()
    };
    // Two dry runs, one per adapter: each builds the suite, which is what makes them slow.
    let out = cli(&[
        "dry-run",
        "--provider",
        "openai",
        "--model",
        "gpt-4.1-mini-2025-04-14",
        "--cap-usd",
        "2",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["requests"], 32);
    assert_eq!(v["max_attempts"], 96);
    assert_eq!(v["trials"], 2);
    assert_eq!(v["tasks"].as_array().unwrap().len(), 8);
    assert_eq!(v["already_spent"], 0);
    assert_eq!(v["within_cap"], true);
    assert_eq!(v["parameters"]["temperature"], 0);
    let out = cli(&[
        "dry-run",
        "--provider",
        "anthropic",
        "--model",
        "claude-haiku-4-5-20251001",
        "--cap-usd",
        "2",
        "--pilot",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["requests"], 4);
    assert_eq!(v["mode"], "pilot");
    assert_eq!(v["parameters"]["max_tokens"], 1024);
    let out = cli(&[
        "dry-run",
        "--provider",
        "openai",
        "--model",
        "gpt-4.1-mini-2025-04-14",
    ]);
    assert!(!out.status.success(), "a cap is required");
    let out = cli(&[
        "run",
        "--provider",
        "openai",
        "--model",
        "gpt-4.1-mini-2025-04-14",
        "--cap-usd",
        "2",
    ]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--suite-digest"));
    let out = cli(&[
        "run",
        "--provider",
        "openai",
        "--model",
        "gpt-4.1-mini-2025-04-14",
        "--cap-usd",
        "2",
        "--suite-digest",
        &suite().digest,
    ]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && err.contains("no credential for openai"),
        "{err}"
    );
    assert!(
        std::fs::read_dir(evidence.path()).unwrap().next().is_none(),
        "nothing was sent or written"
    );
}

/// How a recorded client was run: its arguments, environment and standard input.
type Runs = Vec<(Vec<String>, Vec<(String, String)>, String)>;

/// A Claude Code client that answers with one recorded result and keeps how it was run.
struct Client {
    status: Option<i32>,
    stdout: String,
    runs: RefCell<Runs>,
}

impl nazm_agent_bench::provider::Exec for Client {
    fn exec(
        &self,
        args: &[String],
        env: &[(&str, String)],
        stdin: &str,
    ) -> Result<(Option<i32>, String, String), String> {
        self.runs.borrow_mut().push((
            args.to_vec(),
            env.iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            stdin.to_owned(),
        ));
        Ok((self.status, self.stdout.clone(), String::new()))
    }
}

fn client(status: i32, stdout: Value) -> Client {
    Client {
        status: Some(status),
        stdout: stdout.to_string(),
        runs: RefCell::new(Vec::new()),
    }
}

#[test]
fn the_claude_code_client_runs_one_bare_turn_and_its_usage_is_normalised() {
    use nazm_agent_bench::provider::ClaudeCode;
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "num_turns": 1,
        "result": "{\"answer\": {}}", "stop_reason": "end_turn", "api_error_status": null,
        "total_cost_usd": 0.001,
        "usage": {"input_tokens": 400, "output_tokens": 9, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        "modelUsage": {"claude-haiku-4-5-20251001": {"inputTokens": 400, "outputTokens": 9, "cacheReadInputTokens": 3000, "cacheCreationInputTokens": 200, "thinkingTokens": 0}}
    });
    let p = ClaudeCode {
        exec: client(0, result),
        model: "claude-haiku-4-5-20251001".to_owned(),
        version: "2.1.283 (Claude Code)".to_owned(),
        max_output_tokens: 1024,
    };
    let r = prompt::request(task("T4"), Arm::N33);
    let reply = p.complete_within(&r, 12_345_678).unwrap();
    assert_eq!(reply.model, "claude-haiku-4-5-20251001");
    assert_eq!(reply.text, "{\"answer\": {}}");
    assert_eq!(
        reply.usage,
        Usage {
            input_tokens: 3600,
            uncached_input_tokens: 400,
            cached_input_tokens: 3000,
            cache_write_input_tokens: 200,
            output_tokens: 9,
            reasoning_tokens: Some(0),
        }
    );
    let runs = p.exec.runs.borrow();
    let (args, env, stdin) = &runs[0];
    assert_eq!(stdin, &r.user, "the prompt goes on standard input");
    let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(after("--system-prompt"), r.system);
    assert_eq!(after("--tools"), "");
    assert_eq!(after("--setting-sources"), "");
    assert_eq!(after("--model"), "claude-haiku-4-5-20251001");
    assert_eq!(after("--max-budget-usd"), "0.000013");
    assert!(
        args.iter().any(|a| a == "--strict-mcp-config")
            && args.iter().any(|a| a == "--no-session-persistence")
    );
    assert!(env.contains(&("MAX_THINKING_TOKENS".to_owned(), "0".to_owned())));
    assert!(env.contains(&(
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
        "1024".to_owned()
    )));
    assert_eq!(p.parameters()["client_version"], "2.1.283 (Claude Code)");
    assert_eq!(p.calls_per_request(), 2);
    // An answer cut off at the output limit is a reply, and the oracle's to judge.
    let cut = json!({"result": "API Error: Claude's response exceeded the 1024 output token maximum.", "is_error": true, "api_error_status": null,
        "modelUsage": {"claude-haiku-4-5-20251001": {"inputTokens": 900, "outputTokens": 1024}}});
    let p = ClaudeCode {
        exec: client(1, cut),
        ..p
    };
    let reply = p.complete(&r).unwrap();
    assert_eq!(
        oracle::score(task("T4"), Arm::N33, &reply.text).failure,
        Some(Failure::InvalidStructuredOutput)
    );
    // An overloaded server is retryable; a refused request is not.
    let p = ClaudeCode {
        exec: client(1, json!({"api_error_status": 529, "result": "Overloaded"})),
        ..p
    };
    assert!(matches!(
        p.complete(&r),
        Err(CallError::Retryable { billed: false, .. })
    ));
    let p = ClaudeCode {
        exec: client(1, json!({"api_error_status": 400, "result": "bad"})),
        ..p
    };
    assert!(matches!(p.complete(&r), Err(CallError::Fatal { .. })));
    // The client's spend limit withheld an answer: not the model's reply, and not transient.
    let cut_off = json!({"subtype": "error_max_budget_usd", "is_error": true,
        "errors": ["Reached maximum budget ($0.015004)"],
        "modelUsage": {"claude-haiku-4-5-20251001": {"inputTokens": 3, "outputTokens": 683}}});
    let p = ClaudeCode {
        exec: client(1, cut_off),
        ..p
    };
    assert!(matches!(p.complete(&r), Err(CallError::Fatal { .. })));
    let p = ClaudeCode {
        exec: Client {
            status: Some(1),
            stdout: "not json".to_owned(),
            runs: RefCell::new(Vec::new()),
        },
        ..p
    };
    assert!(matches!(
        p.complete(&r),
        Err(CallError::Retryable { billed: true, .. })
    ));
}
