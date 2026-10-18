/* Concurrency: spawn N pthreads (1,000,000 by default), each adding 1 to a shared atomic; spawned and
   joined 1,000 at a time (a process cannot hold a million live threads). */
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
static atomic_long done;
static void *work(void *a) {
    (void)a;
    atomic_fetch_add(&done, 1);
    return NULL;
}
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 1000000;
    pthread_t ts[1000];
    while (n > 0) {
        long b = n < 1000 ? n : 1000;
        for (long i = 0; i < b; i++) pthread_create(&ts[i], NULL, work, NULL);
        for (long i = 0; i < b; i++) pthread_join(ts[i], NULL);
        n -= b;
    }
    printf("%ld\n", (long)atomic_load(&done));
    return 0;
}
