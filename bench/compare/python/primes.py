# The parallel job: primes below 3,000,000 by trial division, blocks of 1,000 dealt round robin to W
# threads (block b to worker b mod W); prints 216816. CPython's threads share one interpreter lock, so
# this shows what threads give a CPU-bound job there.
import sys
import threading

w = int(sys.argv[1]) if len(sys.argv) > 1 else 1
found = [0] * w


def is_prime(n):
    if n < 2:
        return False
    if n % 2 == 0:
        return n == 2
    d = 3
    while d * d <= n:
        if n % d == 0:
            return False
        d += 2
    return True


def count(k):
    c = 0
    for b in range(k, 3000, w):
        for n in range(b * 1000, b * 1000 + 1000):
            if is_prime(n):
                c += 1
    found[k] = c


ts = [threading.Thread(target=count, args=(k,)) for k in range(w)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print(sum(found))
