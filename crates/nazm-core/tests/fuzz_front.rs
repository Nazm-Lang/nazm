//! N73: front-end robustness fuzzing — no input makes the parser or the checker panic.
//!
//! `docs/internal/architecture.md` §7.75. Every source of the repository's corpora is mutated by a seeded
//! generator — ranges deleted, duplicated and swapped, characters replaced, tokens of the language
//! inserted — and parsed and checked in process. A diagnostic is the right answer to any input; a
//! panic is a defect, shrunk to the smallest input that still panics and printed. The seed is fixed,
//! so a run is the same inputs on every host; `NAZM_FUZZ_SEED` and `NAZM_FUZZ_CASES` choose another.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: small, seeded, and the same on every host.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
        }
    }
}

const TOKENS: &[&str] = &[
    "fn ",
    "{",
    "}",
    "(",
    ")",
    "let ",
    "mut ",
    "while ",
    "if ",
    "else ",
    "return ",
    "struct ",
    "enum ",
    "match ",
    "=>",
    "!",
    "! {}",
    "[",
    "]",
    ",",
    ";",
    "::",
    "->",
    "9223372036854775807",
    "\"",
    "'",
    "\\",
    ".",
    "|",
    "&&",
    "||",
    "pub ",
    "extern \"C\" ",
    "spawn ",
    "scope ",
    "=",
    "==",
    "Int",
    "Bool",
    "Str",
    "Vec[",
    "Result[",
    "x",
    "0",
    "-",
    "*",
    "/",
    "%",
    "\n",
    "//",
    "?",
    "chan_new_of[Int](",
    "|x| x",
];

fn corpus() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in [
        "examples",
        "compiler/conformance",
        "bench/programs",
        "crates/nazm-cli/tests/transfers",
        "library/std",
        "library/core",
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.extension().is_some_and(|x| x == "nz") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    // The large ones cost time without adding shapes.
                    if text.len() < 20_000 {
                        out.push(text);
                    }
                }
            }
        }
    }
    out
}

fn mutate(rng: &mut Rng, src: &str) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    for _ in 0..=rng.below(4) {
        let n = chars.len().max(1);
        let a = rng.below(n);
        let b = (a + 1 + rng.below(40)).min(chars.len());
        match rng.below(5) {
            0 if a < chars.len() => {
                chars.drain(a..b);
            }
            1 if a < chars.len() => {
                let piece: Vec<char> = chars[a..b].to_vec();
                let at = rng.below(chars.len() + 1);
                chars.splice(at..at, piece);
            }
            2 => {
                let t = TOKENS[rng.below(TOKENS.len())];
                let at = rng.below(chars.len() + 1);
                chars.splice(at..at, t.chars());
            }
            3 if a < chars.len() => {
                let c = char::from_u32(u32::try_from(rng.below(0x80)).unwrap()).unwrap_or(' ');
                chars[a] = c;
            }
            _ if b + 1 < chars.len() => {
                let c = rng.below(chars.len() - b);
                let (x, y) = (
                    chars[a..b].to_vec(),
                    chars[b + c..(b + c + b - a).min(chars.len())].to_vec(),
                );
                if !y.is_empty() {
                    let mut next: Vec<char> = chars[..a].to_vec();
                    next.extend_from_slice(&y);
                    next.extend_from_slice(&chars[b..b + c]);
                    next.extend_from_slice(&x);
                    next.extend_from_slice(&chars[(b + c + y.len()).min(chars.len())..]);
                    chars = next;
                }
            }
            _ => {}
        }
    }
    chars.into_iter().collect()
}

fn panics(src: &str) -> bool {
    let s = src.to_owned();
    std::panic::catch_unwind(move || {
        let _ = nazm_core::check_source(&s);
    })
    .is_err()
}

/// The smallest input, by repeatedly removing pieces, that still panics.
fn shrink(mut src: String) -> String {
    let mut step = src.chars().count() >> 1;
    while step > 0 {
        let mut progress = false;
        let mut at = 0;
        loop {
            let chars: Vec<char> = src.chars().collect();
            if at >= chars.len() {
                break;
            }
            let end = (at + step).min(chars.len());
            let candidate: String = chars[..at].iter().chain(chars[end..].iter()).collect();
            if panics(&candidate) {
                src = candidate;
                progress = true;
            } else {
                at += step;
            }
        }
        if !progress {
            step >>= 1;
        }
    }
    src
}

#[test]
fn no_mutated_source_makes_the_front_end_panic() {
    let seed: u64 = std::env::var("NAZM_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x6e61_7a6d_0073);
    let cases: usize = std::env::var("NAZM_FUZZ_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    let corpus = corpus();
    assert!(corpus.len() > 20, "the corpus is {} files", corpus.len());
    // Panics are the property; their messages would only be noise while the run continues.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng(seed | 1);
    let mut found = None;
    for i in 0..cases {
        let base = &corpus[i % corpus.len()];
        let input = mutate(&mut rng, base);
        if panics(&input) {
            found = Some(shrink(input));
            break;
        }
    }
    std::panic::set_hook(hook);
    if let Some(input) = found {
        panic!("the front end panicked on (shrunk, seed {seed:#x}):\n---\n{input}\n---");
    }
}
