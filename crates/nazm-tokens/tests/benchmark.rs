//! The N34 benchmark: the seven N33 tasks, their unchanged naive baselines and their N33
//! contexts, counted under every tokenizer; fixtures of source, JSON, Markdown and three
//! scripts; where a tiny context's overhead goes; how far one byte budget's selection varies in
//! tokens. Run with `--nocapture` for the report `docs/internal/performance-log.md` records.
//!
//! Reductions are integer basis points of the naive count — `10000 − 10000·N33/naive`, rounded
//! half away from zero — so a report is the same on every machine. Nothing here chooses what a
//! context contains: the planner selected it, from bytes and structure, before any tokenizer
//! saw it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::integer_division
)]

#[path = "../../nazm-repo/tests/scenarios/mod.rs"]
mod scenarios;

use std::fmt::Write as _;
use std::time::Instant;

use nazm_tokens::Registry;
use serde_json::{json, Value};

fn registry() -> Registry {
    Registry::load(&nazm_tokens::default_assets()).unwrap()
}

/// Tokens of `texts`, each counted on its own and summed: files are read one at a time.
fn tokens(r: &Registry, texts: &[String]) -> Vec<usize> {
    r.all()
        .iter()
        .map(|t| texts.iter().map(|x| t.count(x).unwrap().tokens).sum())
        .collect()
}

/// `10000 − 10000·after/before`, rounded half away from zero.
fn reduction_bp(before: usize, after: usize) -> i64 {
    let (b, a) = (before as i128, after as i128);
    let num = 10_000 * (b - a);
    let q = (2 * num + b.signum() * b) / (2 * b);
    let q = if num < 0 {
        -((2 * -num + b) / (2 * b))
    } else {
        q
    };
    q as i64
}

fn pct(bp: i64) -> String {
    let sign = if bp < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}%", bp.abs() / 100, bp.abs() % 100)
}

fn median(mut v: Vec<i64>) -> i64 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

/// The report, as one canonical object, and its table.
fn report() -> (Value, String) {
    let r = registry();
    let names: Vec<&str> = r.all().iter().map(|t| t.identity.id).collect();
    let mut table = String::new();
    let _ = writeln!(
        table,
        "| Task | Naive bytes | N33 bytes | Byte reduction | {} | Spread |",
        names.join(" | ")
    );
    let _ = writeln!(
        table,
        "| --- | ---: | ---: | ---: |{} ---: |",
        " ---: |".repeat(names.len())
    );
    let mut rows = Vec::new();
    let mut by_tokenizer: Vec<Vec<(i64, &'static str)>> = vec![Vec::new(); names.len()];
    let mut spreads = Vec::new();
    for s in scenarios::all() {
        let naive = tokens(&r, &s.naive);
        let context = tokens(&r, std::slice::from_ref(&s.json));
        let bytes_bp = reduction_bp(s.naive_bytes(), s.json.len());
        let bps: Vec<i64> = naive
            .iter()
            .zip(&context)
            .map(|(n, c)| reduction_bp(*n, *c))
            .collect();
        let spread = bps.iter().max().unwrap() - bps.iter().min().unwrap();
        let stress = s.name.starts_with('C');
        if !stress {
            spreads.push(spread);
            for (i, bp) in bps.iter().enumerate() {
                by_tokenizer[i].push((*bp, s.name));
            }
        }
        let cells: Vec<String> = naive
            .iter()
            .zip(&context)
            .zip(&bps)
            .map(|((n, c), bp)| format!("{n} → {c} ({})", pct(*bp)))
            .collect();
        let _ = writeln!(
            table,
            "| {}{} | {} | {} | {} | {} | {} pts |",
            s.name,
            if stress {
                " — fixed-overhead stress case"
            } else {
                ""
            },
            s.naive_bytes(),
            s.json.len(),
            pct(bytes_bp),
            cells.join(" | "),
            pct(spread).trim_end_matches('%')
        );
        rows.push(json!({
            "scenario": s.name,
            "stress_case": stress,
            "checks": s.checks,
            "naive_bytes": s.naive_bytes(),
            "context_bytes": s.json.len(),
            "byte_reduction_bp": bytes_bp,
            "tokenizers": names.iter().enumerate().map(|(i, id)| json!({
                "tokenizer": id, "naive_tokens": naive[i], "context_tokens": context[i], "reduction_bp": bps[i]
            })).collect::<Vec<_>>(),
            "spread_bp": spread,
        }));
    }
    let aggregate: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let v = &by_tokenizer[i];
            let worst = v.iter().min().unwrap();
            let best = v.iter().max().unwrap();
            json!({
                "tokenizer": id,
                "median_reduction_bp": median(v.iter().map(|x| x.0).collect()),
                "worst": {"scenario": worst.1, "reduction_bp": worst.0},
                "best": {"scenario": best.1, "reduction_bp": best.0},
            })
        })
        .collect();
    let report = json!({
        "report": "nazm N34 multi-tokenizer benchmark",
        "tokenizers": nazm_tokens::TOKENIZERS,
        "scenarios": rows,
        "aggregate_excluding_stress_case": {
            "by_tokenizer": aggregate,
            "median_spread_bp": median(spreads.clone()),
            "max_spread_bp": spreads.iter().max(),
        },
    });
    (report, table)
}

#[test]
fn every_n33_task_is_cheaper_in_tokens_under_every_tokenizer() {
    let (report, table) = report();
    println!("\n{table}");
    println!(
        "{}",
        serde_json::to_string_pretty(&report["aggregate_excluding_stress_case"]).unwrap()
    );
    // The same inputs give the same report, byte for byte.
    assert_eq!(report.to_string(), self::report().0.to_string());
    let scenarios = report["scenarios"].as_array().unwrap();
    assert_eq!(scenarios.len(), 7);
    for s in scenarios {
        let stress = s["stress_case"].as_bool().unwrap();
        for t in s["tokenizers"].as_array().unwrap() {
            let bp = t["reduction_bp"].as_i64().unwrap();
            if stress {
                // Recorded as measured: a one-function program is smaller than its context.
                assert!(bp < 0, "{s}");
            } else {
                // Materially useful under every tokenizer, not only one.
                assert!(
                    bp >= 5_000,
                    "{} under {}: {}",
                    s["scenario"],
                    t["tokenizer"],
                    pct(bp)
                );
            }
        }
    }
    let agg = &report["aggregate_excluding_stress_case"];
    for t in agg["by_tokenizer"].as_array().unwrap() {
        let (w, m, b) = (
            t["worst"]["reduction_bp"].as_i64().unwrap(),
            t["median_reduction_bp"].as_i64().unwrap(),
            t["best"]["reduction_bp"].as_i64().unwrap(),
        );
        assert!(w <= m && m <= b, "{t}");
        let per: Vec<i64> = scenarios
            .iter()
            .filter(|s| !s["stress_case"].as_bool().unwrap())
            .map(|s| {
                s["tokenizers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|x| x["tokenizer"] == t["tokenizer"])
                    .unwrap()["reduction_bp"]
                    .as_i64()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            w,
            *per.iter().min().unwrap(),
            "the worst scenario is the worst one"
        );
    }
    assert!(agg["max_spread_bp"].as_i64().unwrap() < 2_000, "{agg}");
}

/// Where a tiny context's tokens go: the same object with parts removed, under each tokenizer.
#[test]
fn where_the_fixed_overhead_of_a_tiny_context_goes() {
    let r = registry();
    let c = scenarios::all()
        .into_iter()
        .find(|s| s.name.starts_with('C'))
        .unwrap();
    let full: Value = serde_json::from_str(&c.json).unwrap();
    let variant = |f: &dyn Fn(&mut Value)| {
        let mut v = full.clone();
        f(&mut v);
        serde_json::to_string(&v).unwrap()
    };
    let content: Vec<String> = full["items"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|i| {
            let c = &i["content"];
            [c.get("source"), c.get("message"), c.get("help")]
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    let parts: Vec<(&str, String)> = vec![
        ("the whole context", c.json.clone()),
        (
            "without retrieval references",
            variant(&|v| {
                for i in v["items"].as_array_mut().unwrap() {
                    i.as_object_mut().unwrap().remove("retrieval");
                }
            }),
        ),
        (
            "without the envelope (schema, state, budget, used, empty lists)",
            variant(&|v| {
                let o = v.as_object_mut().unwrap();
                for k in [
                    "schema", "state", "budget", "used", "omitted", "bounded", "excluded", "seeds",
                ] {
                    o.remove(k);
                }
            }),
        ),
        (
            "without reason, class, authority, via",
            variant(&|v| {
                for i in v["items"].as_array_mut().unwrap() {
                    let o = i.as_object_mut().unwrap();
                    for k in ["reason", "class", "authority", "via", "compilation"] {
                        o.remove(k);
                    }
                }
            }),
        ),
        ("the program source (the naive baseline)", c.naive.concat()),
        (
            "the content strings alone (source, message, help)",
            content.concat(),
        ),
    ];
    println!("\nScenario C, tokens by representation:");
    for (name, text) in &parts {
        let t = tokens(&r, std::slice::from_ref(text));
        println!("  {name}: {} bytes, tokens {t:?}", text.len());
    }
    // Every tokenizer agrees on the ordering: the structure costs more than the program.
    let whole = tokens(&r, std::slice::from_ref(&c.json));
    let naive = tokens(&r, &c.naive);
    assert!(whole.iter().zip(&naive).all(|(w, n)| w > n));
}

/// Source, JSON, Markdown and three scripts, per tokenizer: which kind of text each is kind to.
#[test]
fn source_json_markdown_and_scripts_vary_by_tokenizer() {
    let r = registry();
    let root = nazm_docs::default_root();
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    let section = |id: &str| match nazm_docs::section(&root, id, None) {
        nazm_docs::SectionResult::Available(s) => s.body.clone(),
        _ => panic!("{id}"),
    };
    let fixtures: Vec<(&str, String)> = vec![
        ("Nazm source: compiler/lex.nz", read("compiler/lex.nz")),
        ("Nazm source: examples/pipeline.nz", read("examples/pipeline.nz")),
        ("JSON: the repository map", nazm_repo::map(&root).to_json()),
        ("Markdown: spec:generics", section("spec:generics")),
        ("Markdown: architecture:7.35", section("architecture:7.35")),
        ("Bangla prose", "নাজম একটি প্রোগ্রামিং ভাষা। এটি এআই-সহায়ক উন্নয়নের জন্য তৈরি, যেখানে প্রতিটি ধারণার একটি স্থির পরিচয় আছে।".to_owned()),
        ("Arabic prose", "نظم لغة برمجة مصممة للتطوير بمساعدة الذكاء الاصطناعي، ولكل مفهوم فيها هوية ثابتة.".to_owned()),
        ("Mixed English, Bangla and code", "The function `check_program` (চেক প্রোগ্রাম) returns `Result[Checked, Vec[Diag]]` — কোনো ত্রুটি থাকলে Err।".to_owned()),
    ];
    println!(
        "\n| Fixture | Bytes | {} |",
        r.all()
            .iter()
            .map(|t| t.identity.id)
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!("| --- | ---: |{}", " ---: |".repeat(r.all().len()));
    for (name, text) in &fixtures {
        let cells: Vec<String> = r
            .all()
            .iter()
            .map(|t| {
                let c = t.count(text).unwrap();
                let unk = if c.unknown_tokens > 0 {
                    format!(", {} unknown", c.unknown_tokens)
                } else {
                    String::new()
                };
                format!(
                    "{} ({} B/t{unk})",
                    c.tokens,
                    nazm_tokens::ratio(text.len(), c.tokens).unwrap()
                )
            })
            .collect();
        println!("| {name} | {} | {} |", text.len(), cells.join(" | "));
        assert!(r.all().iter().all(|t| t.count(text).unwrap().tokens > 0));
    }
}

/// One byte budget, one selection, and how far its size in tokens varies by tokenizer. The
/// selection is the planner's and is the same whichever tokenizer counts it.
#[test]
fn one_byte_budget_is_one_selection_and_a_range_of_token_counts() {
    let r = registry();
    let root = nazm_docs::default_root();
    println!("\nOne selection under a byte budget, in tokens:");
    for max_entities in [3, 8, 20] {
        let request = nazm_repo::TaskRequest {
            intent: Some(nazm_repo::Intent::Understand),
            seeds: vec!["def:compiler:analyse.nz::fn check_program".to_owned()],
            state: None,
            budget: nazm_repo::Budget {
                max_entities: Some(max_entities),
                ..nazm_repo::Budget::default()
            },
        };
        let json = nazm_repo::task(&root, &request).to_json();
        let again = nazm_repo::task(&root, &request).to_json();
        assert_eq!(json, again);
        let v: Value = serde_json::from_str(&json).unwrap();
        let t = tokens(&r, std::slice::from_ref(&json));
        let (lo, hi) = (*t.iter().min().unwrap(), *t.iter().max().unwrap());
        println!(
            "  max_entities {max_entities}: {} items, {} source bytes, {} JSON bytes; tokens {t:?}, max/min {}",
            v["items"].as_array().unwrap().len(),
            v["used"]["source_bytes"],
            json.len(),
            nazm_tokens::ratio(hi, lo).unwrap()
        );
    }
}

/// What measuring costs: loading, one small and one large text, and a thousand repetitions
/// with resident memory before and after. Printed, not asserted, and run by hand in a release
/// build: `cargo test --release -p nazm-tokens --test benchmark -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, printed and not asserted"]
fn what_measuring_costs() {
    let rss = || {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0)
    };
    let before = rss();
    let start = Instant::now();
    let r = registry();
    let load = start.elapsed().as_secs_f64() * 1000.0;
    let loaded = rss();
    let all = scenarios::all();
    let small = all
        .iter()
        .find(|s| s.name.starts_with('C'))
        .unwrap()
        .json
        .clone();
    let large = all
        .iter()
        .find(|s| s.name.starts_with('A'))
        .unwrap()
        .naive
        .concat();
    println!("\nregistry load {load:.0} ms; RSS {before} kB before, {loaded} kB after loading four tokenizers");
    for (name, text) in [("small (C context)", &small), ("large (A naive)", &large)] {
        for t in r.all() {
            let start = Instant::now();
            let n = t.count(text).unwrap().tokens;
            println!(
                "  {name}, {} bytes, {}: {n} tokens in {:.2} ms",
                text.len(),
                t.identity.id,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    let every: Vec<String> = all.iter().map(|s| s.json.clone()).collect();
    let first = tokens(&r, &every);
    for _ in 0..20 {
        let _ = tokens(&r, &every);
    }
    let settled = rss();
    let start = Instant::now();
    for i in 0..1000 {
        let s = &every[i % every.len()];
        let c = r.measure(s.as_bytes(), &[]);
        assert!(matches!(c, nazm_tokens::CostResult::Available(_)));
    }
    let per = start.elapsed().as_secs_f64();
    assert_eq!(tokens(&r, &every), first);
    println!("  1,000 measurements of an N33 context under all four tokenizers: {per:.3} ms each on average; RSS {settled} kB settled, {} kB after", rss());
}
