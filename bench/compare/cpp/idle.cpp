// Concurrency: memory per live task — N threads (100,000 by default) all blocked on one gate at once;
// main waits until every one has started, then opens the gate and they end.
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <thread>
#include <vector>
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 100000;
    std::mutex m;
    std::condition_variable cv;
    bool open = false;
    std::atomic<long> started{0};
    std::vector<std::thread> hs;
    for (long i = 0; i < n; i++)
        hs.emplace_back([&] {
            started++;
            std::unique_lock<std::mutex> l(m);
            cv.wait(l, [&] { return open; });
        });
    while (started.load() < n) std::this_thread::sleep_for(std::chrono::milliseconds(1));
    {
        std::lock_guard<std::mutex> l(m);
        open = true;
    }
    cv.notify_all();
    for (auto &h : hs) h.join();
    printf("%ld\n", started.load());
}
