#!/usr/bin/env python3
"""The `words` workload's input: 2,000,000 words from a fixed 5,000-word vocabulary.

    python3 bench/compare/gen_words.py OUT_FILE

Deterministic on every Python version: the generator is a 64-bit xorshift* with a fixed seed,
written here, not the `random` module. The vocabulary is 5,000 distinct lowercase words of three
to eight letters; word i is drawn with a probability falling roughly as log(5000 / i), so the
counts are skewed. Sixteen words to a line, separated by single spaces, each line ending in a
newline. The file is about 13 MB and is never committed.
"""

import sys

MASK = (1 << 64) - 1
VOCABULARY = 5000
WORDS = 2_000_000
PER_LINE = 16


class XorShift:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        x = self.state
        x ^= x >> 12
        x ^= (x << 25) & MASK
        x ^= x >> 27
        self.state = x
        return (x * 0x2545F4914F6CDD1D) & MASK

    def below(self, n):
        return (self.next() >> 11) % n


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    rng = XorShift(0x9E3779B97F4A7C15)
    vocab, seen = [], set()
    while len(vocab) < VOCABULARY:
        n = 3 + rng.below(6)
        w = "".join(chr(97 + rng.below(26)) for _ in range(n))
        if w not in seen:
            seen.add(w)
            vocab.append(w)
    lines, line = [], []
    for _ in range(WORDS):
        line.append(vocab[rng.below(1 + rng.below(VOCABULARY))])
        if len(line) == PER_LINE:
            lines.append(" ".join(line))
            line = []
    if line:
        lines.append(" ".join(line))
    with open(sys.argv[1], "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
