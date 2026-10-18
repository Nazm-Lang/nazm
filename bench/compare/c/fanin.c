/* Concurrency: P producer threads (8 by default) each send 1,000,000 / P integers into one channel of
   capacity 1,024; main sums them. */
#include "chan.h"
#include <stdio.h>
static chan *c;
static long each;
static void *produce(void *a) {
    long from = (long)a;
    for (long i = from; i < from + each; i++) chan_send(c, i);
    return NULL;
}
static pthread_t *ts;
static long producers;
static void *closer(void *a) {
    (void)a;
    for (long i = 0; i < producers; i++) pthread_join(ts[i], NULL);
    chan_close(c);
    return NULL;
}
int main(int argc, char **argv) {
    producers = argc > 1 ? atol(argv[1]) : 8;
    each = 1000000 / producers;
    c = chan_new(1024);
    ts = malloc(sizeof(pthread_t) * producers);
    for (long p = 0; p < producers; p++) pthread_create(&ts[p], NULL, produce, (void *)(p * each));
    pthread_t cl;
    pthread_create(&cl, NULL, closer, NULL);
    long sum = 0, v;
    while (chan_recv(c, &v)) sum += v;
    pthread_join(cl, NULL);
    printf("%ld\n", sum);
    return 0;
}
