//! `nazm-agent-bench` — the N35 agent task benchmark.
//!
//! ```text
//! nazm-agent-bench suite                                   the tasks, their contexts' digests
//! nazm-agent-bench dry-run --provider P --model M --cap-usd X [--pilot] [--trials N]
//! nazm-agent-bench run     --provider P --model M --cap-usd X [--pilot] [--trials N]
//!                          [--suite-digest D] [--retry-runner-errors]
//! nazm-agent-bench report  [--markdown]
//! … [--evidence DIR] [--pricing FILE] [--max-output N]
//! ```
//!
//! `P` is `openai`, `anthropic` or `claude-code`. The first two bring their own key:
//! `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, or `~/.config/nazm-agent-bench/<P>.key` (mode
//! 0600). `claude-code` runs the Claude Code client in print mode on the credentials it is
//! already signed in with. Only `run` reads a credential, and only `run` reaches a network. The cap is required and covers everything the evidence directory's
//! ledger has charged, pilots included. Exit status: 0 done, 1 stopped or refused, 2 usage.

use std::path::PathBuf;
use std::process::ExitCode;

use nazm_agent_bench::provider::{Anthropic, ClaudeCode, Curl, OpenAi, Process, Provider, Secret};
use nazm_agent_bench::run::{self, Config, Mode, Spec, Stop};
use nazm_agent_bench::{price, report, suite};

struct Args {
    command: String,
    provider: Option<String>,
    model: Option<String>,
    cap: Option<String>,
    pilot: bool,
    trials: u32,
    evidence: PathBuf,
    pricing: PathBuf,
    max_output: u64,
    suite_digest: Option<String>,
    retry_runner_errors: bool,
    markdown: bool,
}

fn parse() -> Result<Args, String> {
    let root = nazm_docs::default_root();
    let mut it = std::env::args().skip(1);
    let command = it
        .next()
        .ok_or("a command: suite, dry-run, run or report")?;
    let mut a = Args {
        command,
        provider: None,
        model: None,
        cap: None,
        pilot: false,
        trials: 2,
        evidence: root.join("target/n35/agent-bench"),
        pricing: root.join("tools/agent-bench/pricing.toml"),
        max_output: 1024,
        suite_digest: None,
        retry_runner_errors: false,
        markdown: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--provider" => a.provider = Some(value()?),
            "--model" => a.model = Some(value()?),
            "--cap-usd" => a.cap = Some(value()?),
            "--trials" => a.trials = value()?.parse().map_err(|_| "--trials is a count")?,
            "--evidence" => a.evidence = PathBuf::from(value()?),
            "--pricing" => a.pricing = PathBuf::from(value()?),
            "--max-output" => {
                a.max_output = value()?.parse().map_err(|_| "--max-output is a count")?
            }
            "--suite-digest" => a.suite_digest = Some(value()?),
            "--pilot" => a.pilot = true,
            "--retry-runner-errors" => a.retry_runner_errors = true,
            "--markdown" => a.markdown = true,
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    if a.pilot {
        a.trials = 1;
    }
    if a.trials == 0 {
        return Err("--trials must be at least 1".to_owned());
    }
    Ok(a)
}

fn provider(
    name: &str,
    model: &str,
    max_output: u64,
    key: Secret,
) -> Result<Box<dyn Provider>, String> {
    let transport = Curl {
        timeout_seconds: 300,
    };
    match name {
        "openai" => Ok(Box::new(OpenAi {
            transport,
            key,
            model: model.to_owned(),
            max_output_tokens: max_output,
            seed: 35,
        })),
        "anthropic" => Ok(Box::new(Anthropic {
            transport,
            key,
            model: model.to_owned(),
            max_output_tokens: max_output,
        })),
        "claude-code" => Ok(Box::new(ClaudeCode {
            exec: Process {
                program: "claude".to_owned(),
                timeout_seconds: 600,
            },
            model: model.to_owned(),
            version: claude_version(),
            max_output_tokens: max_output,
        })),
        other => Err(format!(
            "unknown provider `{other}`: openai, anthropic or claude-code"
        )),
    }
}

/// `claude --version`, or `unavailable` when there is no client to ask.
fn claude_version() -> String {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unavailable".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

fn key_var(name: &str) -> &'static str {
    if name == "anthropic" {
        "ANTHROPIC_API_KEY"
    } else {
        "OPENAI_API_KEY"
    }
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nazm-agent-bench: {e}\nusage: nazm-agent-bench suite | dry-run | run | report  (see the crate's documentation)");
            return ExitCode::from(2);
        }
    };
    match go(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("nazm-agent-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

fn go(a: &Args) -> Result<bool, String> {
    let prices = price::load(&a.pricing)?;
    // Everything a run can be refused for without the suite is checked before building it.
    let mut key = None;
    if a.command == "dry-run" || a.command == "run" {
        let name = a.provider.as_deref().ok_or("--provider is required")?;
        a.model.as_deref().ok_or("--model is required")?;
        price::cap(a.cap.as_deref().ok_or("--cap-usd is required")?)?;
        if a.command == "run" {
            if !a.pilot && a.suite_digest.is_none() {
                return Err(
                    "an authoritative run needs --suite-digest, the frozen suite's".to_owned(),
                );
            }
            // The Claude Code client signs in with the session's own credentials.
            key = Some(if name == "claude-code" {
                Secret::new("")
            } else {
                Secret::load(key_var(name), name)?
            });
        }
    }
    let suite = suite::build()?;
    match a.command.as_str() {
        "suite" => {
            let tasks: Vec<_> = suite
                .tasks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "category": t.category,
                        "scenario": t.scenario,
                        "required_facts": t.required(),
                        "wording": t.wording,
                        "contract": t.contract(),
                        "fields": t.fields,
                        "contexts": t.contexts.iter().map(|c| serde_json::json!({
                            "arm": c.arm, "bytes": c.text.len(), "digest": c.digest,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let v = serde_json::json!({
                "schema": nazm_agent_bench::SCHEMA,
                "kind": "suite",
                "suite": suite.digest,
                "suite_version": nazm_agent_bench::SUITE_VERSION,
                "prompt_version": nazm_agent_bench::PROMPT_VERSION,
                "prompt_digest": nazm_agent_bench::prompt::digest(),
                "system": nazm_agent_bench::prompt::SYSTEM,
                "tasks": tasks,
            });
            println!("{v:#}");
            Ok(true)
        }
        "report" => {
            let mut records = Vec::new();
            let mut dirs: Vec<PathBuf> = std::fs::read_dir(&a.evidence)
                .map_err(|e| format!("{}: {e}", a.evidence.display()))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.join("journal.jsonl").is_file())
                .collect();
            dirs.sort();
            for d in dirs {
                records.extend(run::journal(&d.join("journal.jsonl"))?);
            }
            let r = report::build(&suite, &records, &prices);
            if a.markdown {
                print!("{}", report::markdown(&r));
            } else {
                let v = serde_json::to_value(&r).map_err(|e| e.to_string())?;
                println!("{v:#}");
            }
            Ok(true)
        }
        "dry-run" | "run" => {
            let name = a.provider.as_deref().ok_or("--provider is required")?;
            let model = a.model.as_deref().ok_or("--model is required")?;
            let cap = price::cap(a.cap.as_deref().ok_or("--cap-usd is required")?)?;
            let price = price::find(&prices, name, model)
                .ok_or_else(|| format!("{} has no price for {name}/{model}", a.pricing.display()))?
                .clone();
            let registry = nazm_tokens::Registry::load(&nazm_tokens::default_assets())
                .map_err(|p| format!("the N34 tokenizers are unavailable: {p:?}"))?;
            let o200k = registry
                .get("openai-bpe/o200k_base")
                .ok_or("o200k_base is not in the registry")?;
            let cfg = Config {
                mode: if a.pilot {
                    Mode::Pilot
                } else {
                    Mode::Authoritative
                },
                trials: a.trials,
                cap,
                max_retries: 2,
                evidence: a.evidence.clone(),
                evidence_limit: 256 * 1024 * 1024,
                price,
                retry_runner_errors: a.retry_runner_errors,
            };
            // The dry run is computed from a provider with no credential, which it never calls.
            let spec = Spec::of(provider(name, model, a.max_output, Secret::new(""))?.as_ref());
            let dry = run::dry_run(&suite, &spec, &cfg, o200k)?;
            let v = serde_json::to_value(&dry).map_err(|e| e.to_string())?;
            eprintln!(
                "{} {} {}: {} requests, at most {} attempts; ~{} input tokens, at most {} output tokens; worst case {} ({} with every retry); already spent {}; cap {}",
                dry.mode.name(),
                dry.provider,
                dry.model,
                dry.requests,
                dry.max_attempts,
                dry.estimated_input_tokens,
                dry.max_output_tokens,
                price::dollars(dry.worst_case),
                price::dollars(dry.worst_case_with_retries),
                price::dollars(dry.already_spent),
                price::dollars(dry.cap),
            );
            if a.command == "dry-run" {
                println!("{v:#}");
                return Ok(dry.startable);
            }
            if let Some(d) = &a.suite_digest {
                if *d != suite.digest {
                    return Err(format!(
                        "the suite is {}, not the frozen {d}: refusing to run",
                        suite.digest
                    ));
                }
            }
            if !dry.startable {
                return Err(
                    "the dearest request's worst case exceeds what the cap leaves: refusing to start"
                        .to_owned(),
                );
            }
            if !dry.within_cap {
                eprintln!(
                    "note: every request at its worst case would pass the cap; each attempt is \
                     checked before it is sent, and the run stops there"
                );
            }
            let key = key.ok_or("no credential was loaded")?;
            let p = provider(name, model, a.max_output, key)?;
            let out = run::run(&suite, p.as_ref(), &cfg, o200k, &mut |line| {
                eprintln!("  {line}")
            })?;
            eprintln!(
                "{}: {} sent, {} resumed, {} spent in the evidence directory; {}",
                out.benchmark,
                out.sent,
                out.resumed,
                price::dollars(out.spent),
                out.dir.display()
            );
            match out.stop {
                None => Ok(true),
                Some(Stop::Budget { spent, needed, cap }) => {
                    eprintln!(
                        "stopped: the next attempt could cost {}, and {} of the {} cap is spent",
                        price::dollars(needed),
                        price::dollars(spent),
                        price::dollars(cap)
                    );
                    Ok(false)
                }
                Some(Stop::Fatal(r)) => {
                    eprintln!("stopped: {r}");
                    Ok(false)
                }
                Some(Stop::ModelDrift {
                    requested,
                    returned,
                }) => {
                    eprintln!("stopped: {requested} was requested and {returned} answered");
                    Ok(false)
                }
                Some(Stop::Refused(r)) => {
                    eprintln!("refused: {r}");
                    Ok(false)
                }
            }
        }
        other => Err(format!("unknown command `{other}`")),
    }
}
