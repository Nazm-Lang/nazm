/* Concurrency: N round trips (100,000 by default) between two threads over two channels of capacity 1. */
#include "chan.h"
#include <stdio.h>
static chan *ping, *pong;
static void *echo(void *a) {
    (void)a;
    long v;
    while (chan_recv(ping, &v)) chan_send(pong, v + 1);
    return NULL;
}
int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 100000;
    ping = chan_new(1);
    pong = chan_new(1);
    pthread_t t;
    pthread_create(&t, NULL, echo, NULL);
    long sum = 0, r;
    for (long i = 0; i < n; i++) {
        chan_send(ping, i);
        chan_recv(pong, &r);
        sum += r;
    }
    chan_close(ping);
    pthread_join(t, NULL);
    printf("%ld\n", sum);
    return 0;
}
