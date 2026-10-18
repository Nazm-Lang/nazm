// Concurrency: P producer threads (8 by default) each send 1,000,000 / P integers into one channel of
// capacity 1,024; main sums them.
#include "chan.h"
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>
int main(int argc, char **argv) {
    long p = argc > 1 ? atol(argv[1]) : 8, each = 1000000 / p;
    Chan<long> c(1024);
    std::vector<std::thread> ts;
    for (long k = 0; k < p; k++)
        ts.emplace_back([&, k] {
            for (long i = k * each; i < k * each + each; i++) c.send(i);
        });
    std::thread closer([&] {
        for (auto &t : ts) t.join();
        c.close();
    });
    long sum = 0, v;
    while (c.recv(v)) sum += v;
    closer.join();
    printf("%ld\n", sum);
}
