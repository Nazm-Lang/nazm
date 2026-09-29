//! Tokenizer-independent measurement of text, and exact token counts under pinned tokenizers
//! of three families. N34.
//!
//! # The contract
//!
//! **A measurement, never a decision.** This crate counts what it is given: bytes, Unicode
//! scalars, lines, and — for each tokenizer asked — the tokens the tokenizer's own
//! implementation produces for exactly those bytes. Nothing the compiler, the language
//! service, the documentation index or the repository planner decides depends on it; the
//! layering gate forbids every one of them to depend on this crate, so a tokenizer cannot
//! reach parsing, checking, identity, selection or code.
//!
//! **Real tokenizers, pinned, offline.** Four tokenizers of three families:
//!
//! | Id | Family | Algorithm | Implementation | Asset |
//! | --- | --- | --- | --- | --- |
//! | `openai-bpe/cl100k_base` | `openai_bpe` | byte-level BPE over a regex pre-split, no normalisation | `tiktoken-rs` 0.12.1 | its bundled `cl100k_base.tiktoken` |
//! | `openai-bpe/o200k_base` | `openai_bpe` | the same algorithm, another vocabulary — a variant, not a family | `tiktoken-rs` 0.12.1 | its bundled `o200k_base.tiktoken` |
//! | `sentencepiece-bpe/mistral-7b-v0.1` | `sentencepiece_bpe` | SentencePiece BPE: merges over `▁`-marked pieces, byte fallback, no normalisation | `tokenizers` 0.23.1 | `tools/tokenizers/mistral-7b-v0.1.tokenizer.json` |
//! | `sentencepiece-unigram/t5-small` | `sentencepiece_unigram` | SentencePiece Unigram: the most probable segmentation under per-piece scores, after the model's own NFKC-style normalisation and whitespace split | `tokenizers` 0.23.1 | `tools/tokenizers/t5-small.tokenizer.json` |
//!
//! Every asset is checked against a pinned BLAKE3 digest before it is used — the OpenAI
//! vocabularies over the bytes of every rank in order, as the library decodes them — and a
//! missing, altered or malformed one is refused by name. Nothing is ever downloaded:
//! there is no network code here and no library feature that has any.
//!
//! **Content, not framing.** A count is of the text as ordinary content: OpenAI's special-token
//! strings count as ordinary text, and the SentencePiece models' templates (`<s>` before, `</s>`
//! after) are not added. What a template would add is reported beside the count as
//! `framing_tokens`, and a tokenizer's `<unk>` pieces as `unknown_tokens` — a Unigram model
//! without a character counts it as one unknown piece, which is a smaller number and a lossy
//! one. No text is normalised before it is counted; a tokenizer that normalises does so itself,
//! and its identity says so.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The schema every measurement declares.
pub const SCHEMA: &str = "nazm.token-cost/1";

/// Where this repository keeps its tokenizer assets, from its root.
pub const ASSETS: &str = "tools/tokenizers";

/// A tokenizer's identity: everything a count depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenizerId {
    /// Stable, `family-slug/name`.
    pub id: &'static str,
    pub family: &'static str,
    pub name: &'static str,
    pub algorithm: &'static str,
    /// The implementation and its exact version.
    pub library: &'static str,
    /// Where the asset came from, at which revision.
    pub source: &'static str,
    pub license: &'static str,
    /// BLAKE3 of the asset, checked at every load.
    pub asset_digest: &'static str,
    pub normalization: &'static str,
    pub special_tokens: &'static str,
}

/// The four tokenizers, in canonical order.
pub const TOKENIZERS: &[TokenizerId] = &[
    TokenizerId {
        id: "openai-bpe/cl100k_base",
        family: "openai_bpe",
        name: "cl100k_base",
        algorithm: "byte-level BPE over a regex pre-split",
        library: "tiktoken-rs 0.12.1",
        source: "tiktoken-rs 0.12.1 assets/cl100k_base.tiktoken, sha256 223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7, OpenAI's published file",
        license: "MIT",
        asset_digest: CL100K_DIGEST,
        normalization: "none",
        special_tokens: "none added; special-token strings are ordinary text",
    },
    TokenizerId {
        id: "openai-bpe/o200k_base",
        family: "openai_bpe",
        name: "o200k_base",
        algorithm: "byte-level BPE over a regex pre-split",
        library: "tiktoken-rs 0.12.1",
        source: "tiktoken-rs 0.12.1 assets/o200k_base.tiktoken, sha256 446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d, OpenAI's published file",
        license: "MIT",
        asset_digest: O200K_DIGEST,
        normalization: "none",
        special_tokens: "none added; special-token strings are ordinary text",
    },
    TokenizerId {
        id: "sentencepiece-bpe/mistral-7b-v0.1",
        family: "sentencepiece_bpe",
        name: "mistral-7b-v0.1",
        algorithm: "SentencePiece BPE: merges over metaspace pieces, byte fallback",
        library: "tokenizers 0.23.1, fancy-regex",
        source: "huggingface.co/mistralai/Mistral-7B-v0.1 tokenizer.json at 27d67f1b5f57dc0953326b2601d68371d40ea8da",
        license: "Apache-2.0",
        asset_digest: MISTRAL_DIGEST,
        normalization: "none",
        special_tokens: "not added; the template's <s> is framing_tokens",
    },
    TokenizerId {
        id: "sentencepiece-unigram/t5-small",
        family: "sentencepiece_unigram",
        name: "t5-small",
        algorithm: "SentencePiece Unigram: most probable segmentation under piece scores",
        library: "tokenizers 0.23.1, fancy-regex",
        source: "huggingface.co/google-t5/t5-small tokenizer.json at df1b051c49625cf57a3d0d8d3863ed4d13564fe4",
        license: "Apache-2.0",
        asset_digest: T5_DIGEST,
        normalization: "the model's own: a precompiled NFKC-style charsmap, then whitespace split",
        special_tokens: "not added; the template's </s> is framing_tokens",
    },
];

const CL100K_DIGEST: &str = "bddedcb6884c64705155c1f7caefa77e69fd68c6eb38e83437d7efd52ec81729";
const O200K_DIGEST: &str = "6a16ce727f30407bfd85cbbc543d95a5e71985097b8b148877c70ac9989bca34";
const MISTRAL_DIGEST: &str = "0d8c8f93f99c57d58d54ea6c81562579dd5a04de4bd1f8ff900250ec69b34a0e";
const T5_DIGEST: &str = "9ce04442387ff177fd52c14e1bd1901ba563d71e956ca2cffd39eeb3ef4bb820";

/// Why a tokenizer cannot be used: by kind and tokenizer, with the offending value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Problem {
    pub kind: &'static str,
    pub tokenizer: String,
    pub value: Option<String>,
}

impl Problem {
    fn new(kind: &'static str, tokenizer: &str, value: impl Into<String>) -> Self {
        Self {
            kind,
            tokenizer: tokenizer.to_owned(),
            value: Some(value.into()),
        }
    }
}

enum Engine {
    Tiktoken(Box<tiktoken_rs::CoreBPE>),
    Pieces {
        tokenizer: Box<tokenizers::Tokenizer>,
        unknown: Option<u32>,
        framing: usize,
    },
}

/// One loaded tokenizer.
pub struct Tokenizer {
    pub identity: &'static TokenizerId,
    engine: Engine,
}

impl std::fmt::Debug for Tokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokenizer")
            .field("identity", &self.identity.id)
            .finish_non_exhaustive()
    }
}

/// A count of one text under one tokenizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Count {
    /// Content tokens: the text as ordinary content, nothing added.
    pub tokens: usize,
    /// What the tokenizer's template would add around it; not in `tokens`.
    pub framing_tokens: usize,
    /// Pieces of `tokens` that are the tokenizer's unknown piece.
    pub unknown_tokens: usize,
}

impl Tokenizer {
    /// The count of `text`, exactly as given.
    ///
    /// # Errors
    ///
    /// Only if the implementation refuses the text, which is reported, never approximated.
    pub fn count(&self, text: &str) -> Result<Count, Problem> {
        match &self.engine {
            Engine::Tiktoken(bpe) => Ok(Count {
                tokens: bpe.encode_ordinary(text).len(),
                framing_tokens: 0,
                unknown_tokens: 0,
            }),
            Engine::Pieces {
                tokenizer,
                unknown,
                framing,
            } => {
                let e = tokenizer
                    .encode(text, false)
                    .map_err(|e| Problem::new("encode_failed", self.identity.id, e.to_string()))?;
                Ok(Count {
                    tokens: e.get_ids().len(),
                    framing_tokens: *framing,
                    unknown_tokens: unknown
                        .map_or(0, |u| e.get_ids().iter().filter(|i| **i == u).count()),
                })
            }
        }
    }

    /// The token ids of `text`, for the golden tests that pin an implementation.
    ///
    /// # Errors
    ///
    /// As [`Tokenizer::count`].
    pub fn ids(&self, text: &str) -> Result<Vec<u32>, Problem> {
        match &self.engine {
            Engine::Tiktoken(bpe) => Ok(bpe.encode_ordinary(text)),
            Engine::Pieces { tokenizer, .. } => tokenizer
                .encode(text, false)
                .map(|e| e.get_ids().to_vec())
                .map_err(|e| Problem::new("encode_failed", self.identity.id, e.to_string())),
        }
    }
}

/// Every tokenizer, loaded once and immutable: the process-local cache, keyed by identity.
#[derive(Debug)]
pub struct Registry {
    tokenizers: Vec<Tokenizer>,
}

/// The directory this repository's assets are in, from the checkout the binary was built
/// from. Used to find files, never written into an answer.
#[must_use]
pub fn default_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(ASSETS)
}

impl Registry {
    /// Load every tokenizer, checking each asset against its pinned digest.
    ///
    /// # Errors
    ///
    /// Every tokenizer that cannot be used, and why: a missing, altered or malformed asset.
    pub fn load(assets: &Path) -> Result<Self, Vec<Problem>> {
        let mut problems = Vec::new();
        let mut tokenizers = Vec::new();
        for identity in TOKENIZERS {
            match load_one(identity, assets) {
                Ok(engine) => tokenizers.push(Tokenizer { identity, engine }),
                Err(p) => problems.push(p),
            }
        }
        if problems.is_empty() {
            Ok(Self { tokenizers })
        } else {
            Err(problems)
        }
    }

    /// Every loaded tokenizer, in canonical order.
    #[must_use]
    pub fn all(&self) -> &[Tokenizer] {
        &self.tokenizers
    }

    /// One tokenizer by its exact id — never another in its place.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Tokenizer> {
        self.tokenizers.iter().find(|t| t.identity.id == id)
    }

    /// The `nazm.token-cost/1` measurement of `bytes` under `wanted` — every tokenizer when
    /// it is empty.
    #[must_use]
    pub fn measure(&self, bytes: &[u8], wanted: &[String]) -> CostResult {
        let unknown: Vec<String> = wanted
            .iter()
            .filter(|w| self.get(w).is_none())
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return CostResult::Refused(Refused {
                schema: SCHEMA,
                status: "unsupported",
                tokenizers: unknown,
                problems: Vec::new(),
            });
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return CostResult::Refused(Refused {
                schema: SCHEMA,
                status: "invalid_input",
                tokenizers: Vec::new(),
                problems: vec![Problem {
                    kind: "not_utf8",
                    tokenizer: String::new(),
                    value: None,
                }],
            });
        };
        let mut rows = Vec::new();
        let mut problems = Vec::new();
        for t in &self.tokenizers {
            if !wanted.is_empty() && !wanted.iter().any(|w| w == t.identity.id) {
                continue;
            }
            match t.count(text) {
                Ok(c) => rows.push(Row {
                    tokenizer: t.identity.clone(),
                    tokens: c.tokens,
                    framing_tokens: c.framing_tokens,
                    unknown_tokens: c.unknown_tokens,
                    bytes_per_token: ratio(bytes.len(), c.tokens),
                }),
                Err(p) => problems.push(p),
            }
        }
        if !problems.is_empty() {
            return CostResult::Refused(Refused {
                schema: SCHEMA,
                status: "unavailable",
                tokenizers: Vec::new(),
                problems,
            });
        }
        CostResult::Available(Box::new(Cost {
            schema: SCHEMA,
            status: "available",
            input: Input::of(bytes, text),
            tokenizers: rows,
        }))
    }
}

fn load_one(identity: &'static TokenizerId, assets: &Path) -> Result<Engine, Problem> {
    let bpe = match identity.name {
        "cl100k_base" => Some(tiktoken_rs::cl100k_base()),
        "o200k_base" => Some(tiktoken_rs::o200k_base()),
        _ => None,
    };
    if let Some(bpe) = bpe {
        let bpe = bpe.map_err(|e| Problem::new("malformed_asset", identity.id, e.to_string()))?;
        let digest = vocabulary_digest(&bpe);
        if digest != identity.asset_digest {
            return Err(Problem::new("asset_digest_mismatch", identity.id, digest));
        }
        return Ok(Engine::Tiktoken(Box::new(bpe)));
    }
    let file = assets.join(format!("{}.tokenizer.json", identity.name));
    let bytes = std::fs::read(&file).map_err(|_| {
        Problem::new(
            "missing_asset",
            identity.id,
            format!("{}.tokenizer.json", identity.name),
        )
    })?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    if digest != identity.asset_digest {
        return Err(Problem::new("asset_digest_mismatch", identity.id, digest));
    }
    let tokenizer = tokenizers::Tokenizer::from_bytes(&bytes)
        .map_err(|e| Problem::new("malformed_asset", identity.id, e.to_string()))?;
    let framing = match (tokenizer.encode("", true), tokenizer.encode("", false)) {
        (Ok(with), Ok(without)) => with.get_ids().len().saturating_sub(without.get_ids().len()),
        _ => {
            return Err(Problem::new(
                "malformed_asset",
                identity.id,
                "cannot encode the empty text",
            ))
        }
    };
    let unknown = tokenizer.token_to_id("<unk>");
    Ok(Engine::Pieces {
        tokenizer: Box::new(tokenizer),
        unknown,
        framing,
    })
}

/// BLAKE3 over every rank's bytes, length-prefixed, in rank order until the first rank the
/// vocabulary does not have: the vocabulary exactly as the library decodes it.
fn vocabulary_digest(bpe: &tiktoken_rs::CoreBPE) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"nazm.tiktoken-vocabulary/1");
    let mut rank: u32 = 0;
    while let Ok(bytes) = bpe.decode_bytes(&[rank]) {
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
        rank += 1;
    }
    h.update(&u64::from(rank).to_le_bytes());
    h.finalize().to_hex().to_string()
}

/// `bytes / tokens` to two decimal places, rounded half up, by integer arithmetic: the same
/// string on every machine. `None` when there are no tokens.
#[must_use]
// Integer division is the point: it rounds exactly, where a float would not be the same
// string on every machine.
#[allow(clippy::integer_division)]
pub fn ratio(bytes: usize, tokens: usize) -> Option<String> {
    if tokens == 0 {
        return None;
    }
    let hundredths = (bytes as u128 * 200 + tokens as u128) / (tokens as u128 * 2);
    Some(format!("{}.{:02}", hundredths / 100, hundredths % 100))
}

/// What was measured: its size in bytes, scalars and lines, and its digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Input {
    pub bytes: usize,
    pub scalars: usize,
    pub lines: usize,
    pub digest: String,
}

impl Input {
    fn of(bytes: &[u8], text: &str) -> Self {
        Self {
            bytes: bytes.len(),
            scalars: text.chars().count(),
            lines: text.lines().count(),
            digest: blake3::hash(bytes).to_hex().to_string(),
        }
    }
}

/// One tokenizer's count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub tokenizer: TokenizerId,
    pub tokens: usize,
    pub framing_tokens: usize,
    pub unknown_tokens: usize,
    /// Two decimal places, `None` for no tokens.
    pub bytes_per_token: Option<String>,
}

/// `nazm.token-cost/1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cost {
    pub schema: &'static str,
    pub status: &'static str,
    pub input: Input,
    /// In canonical order.
    pub tokenizers: Vec<Row>,
}

/// No measurement, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub schema: &'static str,
    /// `unsupported`, `invalid_input` or `unavailable`.
    pub status: &'static str,
    /// For `unsupported`: the ids no tokenizer has.
    pub tokenizers: Vec<String>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CostResult {
    Available(Box<Cost>),
    Refused(Refused),
}

impl CostResult {
    /// Compact JSON, one line, fields in declaration order.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// A refusal for a registry that could not be loaded.
#[must_use]
pub fn unavailable(problems: Vec<Problem>) -> CostResult {
    CostResult::Refused(Refused {
        schema: SCHEMA,
        status: "unavailable",
        tokenizers: Vec::new(),
        problems,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_ratio_is_integer_arithmetic_rounded_half_up() {
        assert_eq!(super::ratio(10, 4).as_deref(), Some("2.50"));
        assert_eq!(super::ratio(1, 3).as_deref(), Some("0.33"));
        assert_eq!(super::ratio(2, 3).as_deref(), Some("0.67"));
        assert_eq!(super::ratio(5, 0), None);
    }

    /// Nothing here fetches, reads the environment, or keeps state beyond the registry.
    #[test]
    fn nothing_here_can_reach_the_network() {
        let source = include_str!("lib.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "http",
            "download",
            "hf_hub",
            "from_pretrained",
            "env::var",
            "Command",
            "HashMap",
        ] {
            assert!(!code.contains(forbidden), "`{forbidden}` in nazm-tokens");
        }
    }
}
