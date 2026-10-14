import sys
from collections import Counter

counts = Counter(open(sys.argv[1]).read().split())
print(len(counts))
for word, n in sorted(counts.items(), key=lambda e: (-e[1], e[0]))[:10]:
    print(word, n)
