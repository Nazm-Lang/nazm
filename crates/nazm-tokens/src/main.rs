//! `nazm-tokens` — exact token counts of a text under pinned, offline tokenizers of three
//! families, as `nazm.token-cost/1`. N34.
//!
//! ```text
//! nazm-tokens list                          every tokenizer's identity, one object
//! nazm-tokens measure [--tokenizer ID]...   the text on standard input, exactly as given
//! nazm-tokens … [--assets DIR]              another checkout's tools/tokenizers
//! ```
//!
//! Standard output is exactly one JSON object and standard error is empty; the exit status is
//! 0 with an answer, 1 with a refusal (`unsupported`, `invalid_input`, `unavailable`) and 2 for
//! a usage error. The text is read from standard input and never from a path argument: to
//! measure a task context, pipe `nazm repo --task … --json` into it. Nothing is downloaded.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use nazm_tokens::{CostResult, Registry, SCHEMA, TOKENIZERS};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return usage("a command: list or measure");
    };
    let mut wanted = Vec::new();
    let mut assets: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--tokenizer", Some(id)) if command == "measure" => wanted.push(id),
            ("--assets", Some(dir)) => assets = Some(PathBuf::from(dir)),
            _ => return usage(&format!("unexpected argument `{a}`")),
        }
    }
    let assets = assets.unwrap_or_else(nazm_tokens::default_assets);
    match command.as_str() {
        "list" => {
            let value = serde_json::json!({"schema": SCHEMA, "status": "available", "tokenizers": TOKENIZERS});
            println!("{value}");
            ExitCode::SUCCESS
        }
        "measure" => {
            let mut bytes = Vec::new();
            if std::io::stdin().read_to_end(&mut bytes).is_err() {
                return usage("standard input could not be read");
            }
            let result = match Registry::load(&assets) {
                Ok(r) => r.measure(&bytes, &wanted),
                Err(problems) => nazm_tokens::unavailable(problems),
            };
            println!("{}", result.to_json());
            if matches!(result, CostResult::Available(_)) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        other => usage(&format!("unknown command `{other}`")),
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("nazm-tokens: {why}\nusage: nazm-tokens list | measure [--tokenizer ID]... [--assets DIR] < text");
    ExitCode::from(2)
}
