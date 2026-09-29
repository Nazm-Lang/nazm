//! Usage and money, kept apart.
//!
//! **Usage law.** [`Usage`] is what the provider reported, normalised into a few named
//! categories — total input, the part of it read from a cache, the part written to one,
//! output, and the reasoning part of output where a provider reports it — with the provider's
//! own usage object kept raw beside it. Nothing is folded silently: `input_tokens` always
//! includes the cached parts, and `uncached_input_tokens` is what is left.
//!
//! **Price law.** A [`Price`] is a dated entry of `tools/agent-bench/pricing.toml`, with its
//! source; it never enters a record's identity, so a result stays readable after prices change
//! and can be priced again under another list. Money is integer pico-USD: a price in
//! micro-USD per million tokens *is* pico-USD per token, so every cost is an exact product and
//! a sum is exact in any order.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The provider's usage, normalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Every input token, cached or not.
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    /// Read from a cache.
    pub cached_input_tokens: u64,
    /// Written to a cache.
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
    /// The part of `output_tokens` spent reasoning, where the provider reports it.
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    /// Input plus output. Cached tokens are input and are in it; nothing else is added.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// One dated price entry. Every rate is micro-USD per million tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    pub id: String,
    pub provider: String,
    pub model: String,
    /// The day the rates were read from `source`.
    pub effective: String,
    pub source: String,
    pub input: u64,
    pub cached_input: u64,
    pub cache_write_input: u64,
    pub output: u64,
}

/// A request's cost in pico-USD, by component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cost {
    pub input: u64,
    pub cached_input: u64,
    pub cache_write_input: u64,
    pub output: u64,
    pub total: u64,
}

/// `usage` at `price`, or `None` on overflow — which would be a unit error, not a bill.
#[must_use]
pub fn cost(usage: &Usage, price: &Price) -> Option<Cost> {
    let input = usage.uncached_input_tokens.checked_mul(price.input)?;
    let cached_input = usage.cached_input_tokens.checked_mul(price.cached_input)?;
    let cache_write_input = usage
        .cache_write_input_tokens
        .checked_mul(price.cache_write_input)?;
    let output = usage.output_tokens.checked_mul(price.output)?;
    let total = input
        .checked_add(cached_input)?
        .checked_add(cache_write_input)?
        .checked_add(output)?;
    Some(Cost {
        input,
        cached_input,
        cache_write_input,
        output,
        total,
    })
}

/// The most a request can cost: every input token at the dearest input rate — a cache write
/// costs more than uncached input — and the whole output budget used.
#[must_use]
pub fn worst_case(input_tokens: u64, max_output_tokens: u64, price: &Price) -> Option<u64> {
    input_tokens
        .checked_mul(price.input.max(price.cache_write_input))?
        .checked_add(max_output_tokens.checked_mul(price.output)?)
}

/// `"0.40"` USD → 400 000 micro-USD, exactly; at most six decimal places.
///
/// # Errors
///
/// When `s` is not a non-negative decimal with at most six places.
pub fn micros(s: &str) -> Result<u64, String> {
    let (whole, frac) = s.split_once('.').map_or((s, None), |(w, f)| (w, Some(f)));
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || frac.is_some_and(|f| !digits(f) || f.len() > 6) {
        return Err(format!(
            "{s:?} is not a price in dollars with at most six decimals"
        ));
    }
    let whole: u64 = whole.parse().map_err(|e| format!("{s:?}: {e}"))?;
    let padded = format!("{:0<6}", frac.unwrap_or(""));
    let frac: u64 = padded.parse().map_err(|e| format!("{s:?}: {e}"))?;
    whole
        .checked_mul(1_000_000)
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(|| format!("{s:?} overflows"))
}

/// pico-USD as dollars, rounded half up to the micro-dollar: `$0.001234`.
#[must_use]
// Integer division is the intent: splitting an exact count into its printed parts.
#[allow(clippy::integer_division)]
pub fn dollars(pico: u64) -> String {
    let micro = pico / 1_000_000 + u64::from(pico % 1_000_000 >= 500_000);
    format!("${}.{:06}", micro / 1_000_000, micro % 1_000_000)
}

/// Dollars as pico-USD: the configured cap.
///
/// # Errors
///
/// As [`micros`].
pub fn cap(dollars: &str) -> Result<u64, String> {
    micros(dollars)?
        .checked_mul(1_000_000)
        .ok_or_else(|| format!("{dollars:?} overflows"))
}

#[derive(Deserialize)]
struct File {
    price: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    provider: String,
    model: String,
    effective: String,
    source: String,
    input_usd_per_mtok: String,
    cached_input_usd_per_mtok: String,
    cache_write_input_usd_per_mtok: String,
    output_usd_per_mtok: String,
}

/// Every entry of a price list.
///
/// # Errors
///
/// When the file cannot be read or an entry is malformed.
pub fn load(path: &Path) -> Result<Vec<Price>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: File = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    file.price
        .into_iter()
        .map(|e| {
            Ok(Price {
                id: format!("{}/{}@{}", e.provider, e.model, e.effective),
                input: micros(&e.input_usd_per_mtok)?,
                cached_input: micros(&e.cached_input_usd_per_mtok)?,
                cache_write_input: micros(&e.cache_write_input_usd_per_mtok)?,
                output: micros(&e.output_usd_per_mtok)?,
                provider: e.provider,
                model: e.model,
                effective: e.effective,
                source: e.source,
            })
        })
        .collect()
}

/// The newest entry for `provider`/`model`.
#[must_use]
pub fn find<'a>(prices: &'a [Price], provider: &str, model: &str) -> Option<&'a Price> {
    prices
        .iter()
        .filter(|p| p.provider == provider && p.model == model)
        .max_by(|a, b| a.effective.cmp(&b.effective))
}
