//! `nazm-bench` — the agent-cost benchmark harness.
//!
//! Phase 2 status: the data model is real and tested; the runners are not written. Every
//! subcommand that would produce a number says so rather than producing a placeholder,
//! because a harness that emits plausible-looking fake numbers is worse than one that
//! emits none.

use clap::{Parser, Subcommand};
use nazm_bench::{Arm, PostSuccessCategory};

#[derive(Parser, Debug)]
#[command(
    name = "nazm-bench",
    about = "Measures what it costs a coding agent to build and maintain backend services.",
    long_about = "Implements the protocol in docs/evaluation.md. Three arms: TypeScript \
                  baseline, TypeScript with improved agent tooling, and Nazm. The primary \
                  comparison is Nazm against the TOOLED TypeScript arm, never the baseline \
                  — that control is what separates a language benefit from a tooling one.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Describe the evaluation protocol as currently configured.
    Protocol,
    /// Run the harness self-tests: the known-answer and failure-case checks from
    /// docs/evaluation.md §8 that must pass before any result is believable.
    Selftest,
    /// Execute a benchmark run. Not implemented.
    Run {
        #[arg(long, value_enum)]
        arm: Option<ArmArg>,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum ArmArg {
    A1,
    A2,
    A3,
}

impl From<ArmArg> for Arm {
    fn from(a: ArmArg) -> Self {
        match a {
            ArmArg::A1 => Self::TypescriptBaseline,
            ArmArg::A2 => Self::TypescriptTooled,
            ArmArg::A3 => Self::Nazm,
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nazm_bench=info".into()),
        )
        .init();

    match Cli::parse().command {
        Command::Protocol => protocol(),
        Command::Selftest => selftest(),
        Command::Run { arm } => run(arm.map(Arm::from)),
    }
}

fn protocol() -> anyhow::Result<()> {
    println!("Arms:");
    for arm in Arm::all() {
        let against = arm
            .gate_comparator()
            .map_or_else(|| "—".to_owned(), |c| format!("gate compares against {c}"));
        println!("  {arm}  {against}");
    }
    println!("\nPost-success categories (analysis labels — none excluded from cost):");
    for c in PostSuccessCategory::all() {
        let note = if c.is_prima_facie_waste() {
            "prima facie waste"
        } else {
            "legitimate work"
        };
        println!("  {c:?}  — {note}");
    }
    println!("\nProtocol: docs/evaluation.md");
    println!("Decision rule: NOT YET PRE-REGISTERED — draft, not binding.");
    Ok(())
}

fn selftest() -> anyhow::Result<()> {
    let results = nazm_bench::selftest::run();
    let failed = results.iter().filter(|c| !c.passed).count();

    for case in &results {
        let mark = if case.passed { "ok  " } else { "FAIL" };
        println!("{mark}  {}", case.name);
        if !case.passed {
            println!("        {}", case.detail);
        }
    }

    println!(
        "\n{}/{} cases passed",
        results.len() - failed,
        results.len()
    );

    if failed > 0 {
        anyhow::bail!("{failed} self-test case(s) failed — no run from this harness is believable");
    }

    println!(
        "\nThe accounting and comparison path reports known fixtures correctly.\n\
         Still unvalidated, because the code does not exist yet: agent drivers, workspace \
         isolation between attempts, and independent acceptance tests executing outside \
         the agent's writable environment."
    );
    Ok(())
}

fn run(arm: Option<Arm>) -> anyhow::Result<()> {
    anyhow::bail!(
        "not implemented{}.\n\n\
         Needs, in order: a task corpus (docs/evaluation.md §3), agent drivers, isolated \
         per-attempt workspaces, independent acceptance tests running outside the agent's \
         writable environment, and the self-test passing.\n\n\
         Phase 2 builds arm A1 only.",
        arm.map_or_else(String::new, |a| format!(" (requested {a})"))
    )
}
