#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

static long workers;

static int is_prime(long n) {
    if (n < 2) return 0;
    if (n % 2 == 0) return n == 2;
    for (long d = 3; d * d <= n; d += 2)
        if (n % d == 0) return 0;
    return 1;
}

// Blocks of 1,000 numbers below 3,000,000: block b is worker b mod W's.
static void *count(void *arg) {
    long found = 0;
    for (long b = (long)arg; b < 3000; b += workers)
        for (long n = b * 1000; n < b * 1000 + 1000; n++)
            found += is_prime(n);
    return (void *)found;
}

int main(int argc, char **argv) {
    workers = argc > 1 ? atol(argv[1]) : 1;
    pthread_t *threads = malloc(sizeof(pthread_t) * workers);
    for (long w = 0; w < workers; w++)
        pthread_create(&threads[w], NULL, count, (void *)w);
    long total = 0;
    for (long w = 0; w < workers; w++) {
        void *found;
        pthread_join(threads[w], &found);
        total += (long)found;
    }
    printf("%ld\n", total);
    free(threads);
    return 0;
}
