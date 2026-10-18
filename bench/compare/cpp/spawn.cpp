// Concurrency: spawn N OS threads (1,000,000 by default), each adding 1 to a shared atomic; spawned and
// joined 1,000 at a time (a process cannot hold a million live threads).
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 1000000;
    std::atomic<long> done{0};
    while (n > 0) {
        long b = n < 1000 ? n : 1000;
        std::vector<std::thread> hs;
        for (long i = 0; i < b; i++) hs.emplace_back([&] { done.fetch_add(1); });
        for (auto &h : hs) h.join();
        n -= b;
    }
    printf("%ld\n", done.load());
}
