# Tokenizer assets — provenance

The two files here are the only tokenizer data this repository carries. `nazm-tokens`
(`crates/nazm-tokens/`) reads them from this directory and nowhere else, checks each against the
BLAKE3 digest pinned in `crates/nazm-tokens/src/lib.rs` before use, and refuses a missing or
altered one by name. Nothing downloads them: they were fetched once, on 2026-09-29, for N34, and
the gate runs with the network off. No model weights are here — only the tokenizer definitions.

| File | Source, pinned revision | Model | Licence | Bytes | SHA-256 | BLAKE3 |
| --- | --- | --- | --- | ---: | --- | --- |
| `mistral-7b-v0.1.tokenizer.json` | `https://huggingface.co/mistralai/Mistral-7B-v0.1/resolve/27d67f1b5f57dc0953326b2601d68371d40ea8da/tokenizer.json` | SentencePiece BPE, 32,000 pieces, byte fallback, 58,980 merges | Apache-2.0 (the repository's `license` at that revision) | 1,795,188 | `11c08db21487c885d8c792180f0be237f6a261b89a46f128a6a80a3aa4bd1720` | `0d8c8f93f99c57d58d54ea6c81562579dd5a04de4bd1f8ff900250ec69b34a0e` |
| `t5-small.tokenizer.json` | `https://huggingface.co/google-t5/t5-small/resolve/df1b051c49625cf57a3d0d8d3863ed4d13564fe4/tokenizer.json` | SentencePiece Unigram, 32,100 scored pieces, precompiled NFKC-style normaliser | Apache-2.0 (the repository's `license` at that revision) | 1,389,353 | `d2acde0d8d71dd30a711834b07781b9c89feaac33fd332f60507699282740066` | `9ce04442387ff177fd52c14e1bd1901ba563d71e956ca2cffd39eeb3ef4bb820` |

The OpenAI vocabularies are not files here: `tiktoken-rs` 0.12.1 (MIT) bundles OpenAI's published
`cl100k_base.tiktoken` (SHA-256 `223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7`)
and `o200k_base.tiktoken` (SHA-256 `446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d`),
and `nazm-tokens` pins a BLAKE3 over every rank's bytes as the library decodes them
(`bddedcb6…` and `6a16ce72…`).

Both licences are Apache-2.0, the licence of this repository (`LICENSE`), which permits
redistribution with the licence and attribution kept; this file is that attribution.

## Libraries

`tiktoken-rs =0.12.1` (MIT) and `tokenizers =0.23.1` (Apache-2.0, `default-features = false`,
`features = ["fancy-regex"]`: no HTTP, no hub, no progress bar, no C regex engine), pinned exactly
in `crates/nazm-tokens/Cargo.toml`; every transitive crate is in `Cargo.lock`, and `cargo deny
check licenses` accepts them.

## The oracle

`oracle.py` wrote `crates/nazm-tokens/tests/fixtures/golden.json` — thirteen fixtures, each with
every tokenizer's exact ids — from independent implementations: Python `tiktoken` 0.12.0 and
Python `tokenizers` 0.22.2 (whose regex engine is onig, not the fancy-regex under test). It needed
the network to install those, once; the tests read its output offline.
