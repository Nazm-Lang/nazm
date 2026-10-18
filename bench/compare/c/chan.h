/* A bounded channel of longs (capacity cap) over a mutex and two condition variables. */
#include <pthread.h>
#include <stdlib.h>
typedef struct {
    pthread_mutex_t m;
    pthread_cond_t not_full, not_empty;
    long *buf;
    long cap, head, len;
    int closed;
} chan;
static chan *chan_new(long cap) {
    chan *c = calloc(1, sizeof(chan));
    pthread_mutex_init(&c->m, NULL);
    pthread_cond_init(&c->not_full, NULL);
    pthread_cond_init(&c->not_empty, NULL);
    c->buf = malloc(sizeof(long) * cap);
    c->cap = cap;
    return c;
}
static void chan_send(chan *c, long v) {
    pthread_mutex_lock(&c->m);
    while (c->len == c->cap) pthread_cond_wait(&c->not_full, &c->m);
    c->buf[(c->head + c->len++) % c->cap] = v;
    pthread_cond_signal(&c->not_empty);
    pthread_mutex_unlock(&c->m);
}
static int chan_recv(chan *c, long *out) {
    pthread_mutex_lock(&c->m);
    while (c->len == 0 && !c->closed) pthread_cond_wait(&c->not_empty, &c->m);
    if (c->len == 0) {
        pthread_mutex_unlock(&c->m);
        return 0;
    }
    *out = c->buf[c->head];
    c->head = (c->head + 1) % c->cap;
    c->len--;
    pthread_cond_signal(&c->not_full);
    pthread_mutex_unlock(&c->m);
    return 1;
}
static void chan_close(chan *c) {
    pthread_mutex_lock(&c->m);
    c->closed = 1;
    pthread_cond_broadcast(&c->not_empty);
    pthread_mutex_unlock(&c->m);
}
