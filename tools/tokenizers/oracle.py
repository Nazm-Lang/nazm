# Golden token ids for nazm-tokens, from independent reference implementations.
#
# Bootstrap only: it needs Python `tiktoken` 0.12.0 and `tokenizers` 0.22.2 (the Rust crate
# under test is `tokenizers` 0.23.1 with the fancy-regex backend; the Python wheel uses
# onig). It writes crates/nazm-tokens/tests/fixtures/golden.json, which the tests read
# offline. Nothing in the gate runs it.
import json, sys, tiktoken, tokenizers

FIXTURES = [
    ("empty", ""),
    ("one_byte", "x"),
    ("ascii", "The quick brown fox jumps over the lazy dog."),
    ("nazm", 'pub fn add(a: Int, b: Int) -> Int { a + b }\nlet s = "héllo";\n'),
    ("json", '{"schema":"nazm.task-context/1","status":"complete","items":[{"id":"def:compiler:lex.nz::fn is_digit","reason":"seed"}]}'),
    ("markdown", "## 7.35 Repository map\n\n**Not another source of truth.** See [`spec`](spec.md#returns).\n"),
    ("bangla", "আমি বাংলায় গান গাই। নাজম একটি প্রোগ্রামিং ভাষা।"),
    ("arabic", "مرحبا بالعالم، هذه لغة برمجة."),
    ("mixed", "\U0001F469‍\U0001F4BB ship it \U0001F680 — café vs café (NFC/NFD), 漢字 and मिश्रित"),
    ("combining", "é̂ ä Z̵̡"),
    ("punctuation", "x=>{y?.z??[0]}!=(a<<=2)&&b||c;;"),
    ("long_identifier", "a_very_long_identifier_" * 20),
    ("special_strings", "<|endoftext|> <s> </s> <unk>"),
]

out = {"oracle": {"tiktoken": tiktoken.__version__, "tokenizers": tokenizers.__version__}, "fixtures": []}
cl, o2 = tiktoken.get_encoding("cl100k_base"), tiktoken.get_encoding("o200k_base")
hf = {n: tokenizers.Tokenizer.from_file(f"tools/tokenizers/{n}.tokenizer.json") for n in ["mistral-7b-v0.1", "t5-small"]}
for name, text in FIXTURES:
    out["fixtures"].append({
        "name": name,
        "text": text,
        "ids": {
            "openai-bpe/cl100k_base": cl.encode_ordinary(text),
            "openai-bpe/o200k_base": o2.encode_ordinary(text),
            "sentencepiece-bpe/mistral-7b-v0.1": hf["mistral-7b-v0.1"].encode(text, add_special_tokens=False).ids,
            "sentencepiece-unigram/t5-small": hf["t5-small"].encode(text, add_special_tokens=False).ids,
        },
    })
json.dump(out, open("crates/nazm-tokens/tests/fixtures/golden.json", "w"), ensure_ascii=False, indent=1)
print("wrote", len(FIXTURES), "fixtures")
