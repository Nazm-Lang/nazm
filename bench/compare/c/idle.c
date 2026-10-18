/* Concurrency: memory per live task — N pthreads (100,000 by default) all blocked on one gate at once;
   main waits until every one has started, then opens the gate and they end. */
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
static int open_gate;
static atomic_long started;
static void *work(void *a) {
    (void)a;
    atomic_fetch_add(&started, 1);
    pthread_mutex_lock(&m);
    while (!open_gate) pthread_cond_wait(&cv, &m);
    pthread_mutex_unlock(&m);
    return NULL;
}
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 100000;
    pthread_t *ts = malloc(sizeof(pthread_t) * n);
    for (long i = 0; i < n; i++) pthread_create(&ts[i], NULL, work, NULL);
    while (atomic_load(&started) < n) usleep(1000);
    pthread_mutex_lock(&m);
    open_gate = 1;
    pthread_cond_broadcast(&cv);
    pthread_mutex_unlock(&m);
    for (long i = 0; i < n; i++) pthread_join(ts[i], NULL);
    printf("%ld\n", (long)atomic_load(&started));
    return 0;
}
