// Concurrency: N round trips (100,000 by default) between two threads over two channels of capacity 1.
#include "chan.h"
#include <cstdio>
#include <cstdlib>
#include <thread>
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 100000;
    Chan<long> ping(1), pong(1);
    std::thread echo([&] {
        long v;
        while (ping.recv(v)) pong.send(v + 1);
    });
    long sum = 0, r;
    for (long i = 0; i < n; i++) {
        ping.send(i);
        pong.recv(r);
        sum += r;
    }
    ping.close();
    echo.join();
    printf("%ld\n", sum);
}
