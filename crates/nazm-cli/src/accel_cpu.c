// Written by nazm accel (Gate 3, docs/user/spec.md, *Providers*): the CPU provider. The kernel's one
// source text — the OpenCL C a device builds — compiled here as C, after a prelude that gives
// OpenCL's few words their C meaning, and its element function `nz_item` run over every element
// by a pool of threads, one per core, each taking a contiguous share.
// argv: kernel.cl output.bin input.bin [input2.bin]; built with NZ_T, the element's C type, and
// NZ_ZIP for a kernel of two.
// One line of JSON on stdout says what happened, in the OpenCL host's words where they apply.
#include <math.h>
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#ifdef _WIN32
#include <windows.h>
// OpenCL's `long` is 64 bits, and so is every kernel's `long` and `ulong`; Windows's C `long` is
// 32 (LLP64). Past the system headers, which want their own `long`, the word is `long long`.
#define long long long
#endif

// OpenCL's words, in C. `long` is 64 bits on every host the CPU provider runs on (LP64), and on
// Windows through the definition above.
#define __kernel static
#define __global
#define __private
#define get_global_id(d) ((size_t)0)
typedef unsigned long ulong;
static inline int atomic_min(volatile int *p, int v) {
    return __atomic_fetch_min(p, v, __ATOMIC_SEQ_CST);
}
static inline long mul_hi(long a, long b) {
    return (long)(((__int128)a * (__int128)b) >> 64);
}
static inline float as_float(unsigned int b) {
    float f;
    memcpy(&f, &b, 4);
    return f;
}
static inline double as_double(unsigned long b) {
    double d;
    memcpy(&d, &b, 8);
    return d;
}

#include "kernel.cl"

static double now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1e3 + (double)t.tv_nsec / 1e6;
}

static char *slurp(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *b = malloc((size_t)n + 1);
    if (!b || fread(b, 1, (size_t)n, f) != (size_t)n) { fclose(f); return NULL; }
    b[n] = 0;
    fclose(f);
    *len = (size_t)n;
    return b;
}

static const NZ_T *in1, *in2;
static NZ_T *out;
static int fail = 0x7fffffff;

struct share { size_t lo, hi; };

// One thread's share, in index order; the element function inlined into the loop, which LLVM's
// vectoriser may turn into SIMD lanes.
static void *work(void *arg) {
    struct share *s = arg;
#ifdef NZ_ZIP
    for (size_t i = s->lo; i < s->hi; i++) out[i] = nz_item(in1[i], in2[i], &fail, i);
#else
    for (size_t i = s->lo; i < s->hi; i++) out[i] = nz_item(in1[i], &fail, i);
#endif
    return NULL;
}

int main(int argc, char **argv) {
    if (argc != 4 && argc != 5) return 4;
    size_t in_len, in2_len = 0;
    in1 = (const NZ_T *)slurp(argv[3], &in_len);
    in2 = argc == 5 ? (const NZ_T *)slurp(argv[4], &in2_len) : NULL;
    if (!in1 || (argc == 5 && (!in2 || in2_len != in_len))) { printf("{\"error\":\"unreadable input\"}\n"); return 3; }
    size_t n = in_len / sizeof(NZ_T);
    out = malloc(n * sizeof(NZ_T) + 8);
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    long cores = (long)si.dwNumberOfProcessors;
#else
    long cores = sysconf(_SC_NPROCESSORS_ONLN);
#endif
    size_t threads = cores > 0 ? (size_t)cores : 1;
    if (threads > n) threads = n > 0 ? n : 1;
    pthread_t pool[256];
    struct share shares[256];
    if (threads > 256) threads = 256;
    double t0 = now_ms();
    for (size_t k = 0; k < threads; k++) {
        shares[k].lo = n * k / threads;
        shares[k].hi = n * (k + 1) / threads;
        if (pthread_create(&pool[k], NULL, work, &shares[k]) != 0) { printf("{\"error\":\"thread\"}\n"); return 3; }
    }
    // The one synchronisation: every share has finished before anything is read.
    for (size_t k = 0; k < threads; k++) pthread_join(pool[k], NULL);
    double t1 = now_ms();
    FILE *o = fopen(argv[2], "wb");
    if (!o || fwrite(out, sizeof(NZ_T), n, o) != n) { printf("{\"error\":\"unwritable output\"}\n"); return 3; }
    fclose(o);
    printf("{\"device\":\"the host's CPU, %zu threads\",\"driver\":\"%s\",\"version\":\"C, %s\","
           "\"elements\":%zu,\"threads\":%zu,\"run_ms\":%.3f,\"fail\":%d}\n",
           threads, __VERSION__, NZ_VECTORISED, n, threads, t1 - t0, fail);
    return 0;
}
