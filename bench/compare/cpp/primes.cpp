// The parallel job: primes below 3,000,000 by trial division, blocks of 1,000 dealt round robin to
// W threads (block b to worker b mod W); prints 216816.
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>

static bool is_prime(long n) {
    if (n < 2) return false;
    if (n % 2 == 0) return n == 2;
    for (long d = 3; d * d <= n; d += 2)
        if (n % d == 0) return false;
    return true;
}

int main(int argc, char **argv) {
    long w = argc > 1 ? std::atol(argv[1]) : 1;
    std::vector<long> found(w);
    {
        std::vector<std::jthread> t;
        for (long k = 0; k < w; k++)
            t.emplace_back([&found, k, w] {
                for (long b = k; b < 3000; b += w)
                    for (long n = b * 1000; n < b * 1000 + 1000; n++) found[k] += is_prime(n);
            });
    }
    long total = 0;
    for (long f : found) total += f;
    std::printf("%ld\n", total);
}
