//! The tokenizers as measured things: pinned identities, exact ids against independent
//! reference implementations, determinism, explicit refusals, and no normalisation or framing
//! added by this crate. N34.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use nazm_tokens::{CostResult, Registry, TOKENIZERS};
use serde_json::Value;

fn registry() -> Registry {
    Registry::load(&nazm_tokens::default_assets()).unwrap()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("fixtures/golden.json")).unwrap()
}

#[test]
fn every_tokenizer_gives_the_reference_implementations_exact_ids() {
    let r = registry();
    let g = golden();
    let mut checked = 0;
    for f in g["fixtures"].as_array().unwrap() {
        let text = f["text"].as_str().unwrap();
        for t in r.all() {
            let expected: Vec<u32> = f["ids"][t.identity.id]
                .as_array()
                .unwrap_or_else(|| panic!("no golden for {}", t.identity.id))
                .iter()
                .map(|v| u32::try_from(v.as_u64().unwrap()).unwrap())
                .collect();
            assert_eq!(
                t.ids(text).unwrap(),
                expected,
                "{} on {}",
                t.identity.id,
                f["name"]
            );
            assert_eq!(t.count(text).unwrap().tokens, expected.len());
            checked += 1;
        }
        // And the measurement reports each tokenizer's own count, in its own row.
        let CostResult::Available(m) = r.measure(text.as_bytes(), &[]) else {
            panic!()
        };
        for row in &m.tokenizers {
            let n = f["ids"][row.tokenizer.id].as_array().unwrap().len();
            assert_eq!(row.tokens, n, "{} on {}", row.tokenizer.id, f["name"]);
        }
    }
    assert_eq!(checked, 13 * 4);
}

#[test]
fn three_families_and_the_two_sentencepiece_models_are_different_algorithms() {
    let families: BTreeSet<&str> = TOKENIZERS.iter().map(|t| t.family).collect();
    assert_eq!(
        families,
        BTreeSet::from(["openai_bpe", "sentencepiece_bpe", "sentencepiece_unigram"])
    );
    // cl100k and o200k are one family, and count as one.
    assert_eq!(
        TOKENIZERS
            .iter()
            .filter(|t| t.family == "openai_bpe")
            .count(),
        2
    );
    // The files say what model each is: merges over a vocabulary, or scored pieces.
    let model = |n: &str| -> Value {
        let text = std::fs::read_to_string(
            nazm_tokens::default_assets().join(format!("{n}.tokenizer.json")),
        )
        .unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["model"].clone()
    };
    let mistral = model("mistral-7b-v0.1");
    let t5 = model("t5-small");
    assert_eq!(mistral["type"], "BPE");
    assert!(mistral["merges"].as_array().unwrap().len() > 50_000);
    assert!(
        t5.get("type").is_none() && t5["vocab"][0].is_array(),
        "Unigram: [piece, score] pairs"
    );
    assert!(t5["vocab"][5][1].as_f64().is_some());
    // And they segment the same text differently.
    let r = registry();
    let text = "The quick brown fox jumps over the lazy dog.";
    let ids: Vec<Vec<u32>> = r.all().iter().map(|t| t.ids(text).unwrap()).collect();
    assert_ne!(ids[2], ids[3]);
    // Every identity is complete and distinct.
    let mut seen = BTreeSet::new();
    for t in TOKENIZERS {
        assert!(seen.insert(t.id) && seen.insert(t.asset_digest));
        for field in [
            t.family,
            t.name,
            t.algorithm,
            t.library,
            t.source,
            t.license,
            t.normalization,
            t.special_tokens,
        ] {
            assert!(!field.is_empty(), "{}", t.id);
        }
        assert_eq!(t.asset_digest.len(), 64);
        assert!(t.library.contains("0.12.1") || t.library.contains("0.23.1"));
    }
}

#[test]
fn the_same_text_counts_the_same_every_time_and_in_every_registry() {
    let a = registry();
    let b = registry();
    for f in golden()["fixtures"].as_array().unwrap() {
        let bytes = f["text"].as_str().unwrap().as_bytes();
        let first = a.measure(bytes, &[]).to_json();
        for _ in 0..5 {
            assert_eq!(a.measure(bytes, &[]).to_json(), first);
        }
        assert_eq!(b.measure(bytes, &[]).to_json(), first);
    }
}

#[test]
fn content_is_counted_and_framing_is_reported_beside_it() {
    let r = registry();
    let t = |id: &str| r.get(id).unwrap();
    // A special-token string is ordinary text: never the special token.
    let cl = t("openai-bpe/cl100k_base").ids("<|endoftext|>").unwrap();
    assert!(cl.len() > 1 && !cl.contains(&100_257), "{cl:?}");
    // The empty text has no content tokens; the SentencePiece templates would add one each.
    for id in [
        "sentencepiece-bpe/mistral-7b-v0.1",
        "sentencepiece-unigram/t5-small",
    ] {
        let c = t(id).count("").unwrap();
        assert_eq!((c.tokens, c.framing_tokens), (0, 1), "{id}");
    }
    let c = t("openai-bpe/cl100k_base").count("").unwrap();
    assert_eq!((c.tokens, c.framing_tokens), (0, 0));
    // A Unigram model without a character counts an unknown piece, and says so.
    let bangla = "\u{0986}\u{09ae}\u{09bf} \u{09ac}\u{09be}\u{0982}\u{09b2}\u{09be}";
    assert!(
        t("sentencepiece-unigram/t5-small")
            .count(bangla)
            .unwrap()
            .unknown_tokens
            > 0
    );
    assert_eq!(
        t("sentencepiece-bpe/mistral-7b-v0.1")
            .count(bangla)
            .unwrap()
            .unknown_tokens,
        0
    );
}

#[test]
fn the_exact_bytes_are_measured_and_nothing_is_normalised_first() {
    let r = registry();
    let nfc = "caf\u{e9}";
    let nfd = "cafe\u{301}";
    let (a, b) = (
        r.measure(nfc.as_bytes(), &[]),
        r.measure(nfd.as_bytes(), &[]),
    );
    let (CostResult::Available(a), CostResult::Available(b)) = (a, b) else {
        panic!()
    };
    assert_ne!(a.input.digest, b.input.digest);
    assert_eq!((a.input.bytes, b.input.bytes), (5, 6));
    assert_eq!((a.input.scalars, b.input.scalars), (4, 5));
    // The byte-level BPE sees different bytes and counts them differently.
    assert_ne!(a.tokenizers[0].tokens, b.tokenizers[0].tokens);
}

#[test]
fn an_unknown_tokenizer_is_unsupported_and_never_another_one() {
    let r = registry();
    for wanted in [
        vec!["gpt".to_owned()],
        vec!["openai-bpe/cl100k_base".to_owned(), "llama".to_owned()],
    ] {
        let CostResult::Refused(x) = r.measure(b"hello", &wanted) else {
            panic!("{wanted:?} answered")
        };
        assert_eq!(x.status, "unsupported");
        assert!(!x.tokenizers.is_empty());
    }
    let CostResult::Available(one) =
        r.measure(b"hello", &["sentencepiece-unigram/t5-small".to_owned()])
    else {
        panic!()
    };
    assert_eq!(one.tokenizers.len(), 1);
    assert_eq!(
        one.tokenizers[0].tokenizer.id,
        "sentencepiece-unigram/t5-small"
    );
    let CostResult::Refused(x) = r.measure(&[0xff, 0xfe], &[]) else {
        panic!()
    };
    assert_eq!(x.status, "invalid_input");
}

#[test]
fn a_missing_or_altered_asset_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let problems = Registry::load(dir.path()).unwrap_err();
    let kinds: Vec<(&str, &str)> = problems
        .iter()
        .map(|p| (p.kind, p.tokenizer.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("missing_asset", "sentencepiece-bpe/mistral-7b-v0.1"),
            ("missing_asset", "sentencepiece-unigram/t5-small"),
        ]
    );
    // One byte changed, or a file that is not a tokenizer at all: the digest refuses both
    // before the library is asked to read them.
    let real = nazm_tokens::default_assets();
    let mut bytes = std::fs::read(real.join("mistral-7b-v0.1.tokenizer.json")).unwrap();
    let last = bytes.len() - 2;
    bytes[last] ^= 1;
    std::fs::write(dir.path().join("mistral-7b-v0.1.tokenizer.json"), &bytes).unwrap();
    std::fs::write(dir.path().join("t5-small.tokenizer.json"), b"{not json").unwrap();
    let problems = Registry::load(dir.path()).unwrap_err();
    assert!(
        problems.iter().all(|p| p.kind == "asset_digest_mismatch"),
        "{problems:?}"
    );
    assert_eq!(problems.len(), 2);
}

#[test]
fn large_and_unusual_texts_count_without_panicking() {
    let r = registry();
    let spec =
        std::fs::read_to_string(nazm_docs::default_root().join("docs/user/spec.md")).unwrap();
    let CostResult::Available(c) = r.measure(spec.as_bytes(), &[]) else {
        panic!()
    };
    assert!(c.tokenizers.iter().all(|t| t.tokens > 10_000));
    for text in [
        "\u{0}",
        "\u{feff}",
        &"a".repeat(100_000),
        &"\u{1F600}".repeat(5_000),
        "\r\n\t  \u{a0}",
    ] {
        assert!(matches!(
            r.measure(text.as_bytes(), &[]),
            CostResult::Available(_)
        ));
    }
}

fn tool(args: &[&str], input: &[u8]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nazm-tokens"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), input).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn the_tool_prints_one_object_and_reads_only_standard_input() {
    let (code, out, err) = tool(&["measure"], "fn main() -> Int { 0 }".as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(out.lines().count(), 1);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "nazm.token-cost/1");
    assert_eq!(v["input"]["bytes"], 22);
    assert_eq!(v["tokenizers"].as_array().unwrap().len(), 4);
    let (code, out, _) = tool(&["measure", "--tokenizer", "claude"], b"x");
    assert_eq!(code, Some(1));
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap()["status"],
        "unsupported"
    );
    let (code, out, _) = tool(&["list"], b"");
    assert_eq!(code, Some(0));
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap()["tokenizers"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    // A path is not an argument: the text comes from standard input and nothing else.
    for args in [
        vec!["measure", "/etc/passwd"],
        vec!["measure", "--file", "/etc/passwd"],
        vec![],
        vec!["fetch"],
    ] {
        let (code, out, _) = tool(&args, b"");
        assert_eq!(code, Some(2), "{args:?}");
        assert_eq!(out, "");
    }
    let empty = tempfile::tempdir().unwrap();
    let (code, out, _) = tool(
        &["measure", "--assets", empty.path().to_str().unwrap()],
        b"x",
    );
    assert_eq!(code, Some(1));
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap()["status"],
        "unavailable"
    );
}

/// The keywords `schema/nazm.token-cost-1.json` uses, checked on real output: required,
/// additionalProperties, enum, const, type, items, $ref, oneOf.
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
    if let Some(props) = node.get("properties").and_then(Value::as_object) {
        let obj = v.as_object().ok_or("not an object")?;
        for k in node
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !obj.contains_key(k.as_str().unwrap()) {
                return Err(format!("{k} missing from {v}"));
            }
        }
        if node.get("additionalProperties") == Some(&Value::Bool(false)) {
            if let Some(k) = obj.keys().find(|k| !props.contains_key(*k)) {
                return Err(format!("{k} is not in the schema"));
            }
        }
        for (k, sub) in props {
            if let Some(x) = obj.get(k) {
                conforms(x, sub, root)?;
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
fn every_answer_is_the_published_schema() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(nazm_docs::default_root().join("schema/nazm.token-cost-1.json"))
            .unwrap(),
    )
    .unwrap();
    let r = registry();
    let empty = tempfile::tempdir().unwrap();
    let answers = [
        r.measure("fn main() -> Int { 0 }".as_bytes(), &[]),
        r.measure(b"", &[]),
        r.measure(b"x", &["openai-bpe/o200k_base".to_owned()]),
        r.measure(b"x", &["gpt".to_owned()]),
        r.measure(&[0xff], &[]),
        nazm_tokens::unavailable(Registry::load(empty.path()).unwrap_err()),
    ];
    let mut statuses = BTreeSet::new();
    for a in &answers {
        let v: Value = serde_json::from_str(&a.to_json()).unwrap();
        statuses.insert(v["status"].as_str().unwrap().to_owned());
        conforms(&v, &schema, &schema).unwrap_or_else(|e| panic!("{e}: {v}"));
    }
    assert_eq!(statuses.len(), 4);
}
