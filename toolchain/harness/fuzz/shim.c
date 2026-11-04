/* The coverage shim of `nazm-fuzz` (R8 slice 13, S13.5, question 6: one C file in the harness).
 *
 * In-process libFuzzer needs `LLVMFuzzerTestOneInput`, a C function of a pointer and a length;
 * Nazm exports only effect-free functions of numbers, so this file is the harness's, compiled by
 * the platform's `clang` beside the target's units, never shipped (as `fuzz/cov.c` is for the
 * Rust fuzzer).
 *
 * The target is a Nazm program whose `main` was renamed `nz_program_main`
 * (`llvm-objcopy --redefine-sym`) and whose units carry LLVM's SanitizerCoverage counters
 * (`-fsanitize-coverage=inline-8bit-counters,pc-table`). Each input is written to a `memfd`, then
 * the program is run in a forked child: standard input and output on /dev/null, standard error
 * on a second `memfd`, the program's one argument the input's file. A fork per input keeps every
 * run a fresh program: no state, heap or exit path of the toolchain has to be re-entrant. The
 * child copies its counters into a region it shares with the parent when it exits, and the
 * parent adds them to its own, which is what libFuzzer reads.
 *
 * What the exit tells: status 0, 1 or 2 is an ordinary input (taken, refused, the interpreted
 * program's own failure); `N0406` on standard error is a runaway allocation (the target is built
 * with a counted heap), kept under OUT/oom/ and not a crash; a deadline passed is a hang, kept
 * under OUT/hangs/; anything else, a signal or any other status, is a defect of the toolchain:
 * the standard error is printed and `abort()` makes libFuzzer record the input.
 *
 * Environment: NZ_FUZZ_OUT, the directory hangs/ and oom/ are made in (none: not kept);
 * NZ_FUZZ_DEADLINE_MS, the deadline of one input (default 10000); NZ_FUZZ_TRACE, set: one line
 * on standard error per input, its status and how many counters its run reached.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern int nz_program_main(int argc, char **argv);
extern uint8_t __start___sancov_cntrs[] __attribute__((weak));
extern uint8_t __stop___sancov_cntrs[] __attribute__((weak));

static uint8_t *shared;
static size_t shared_len;
static const char *out_dir;
static long deadline_ms = 10000;
static int trace;
/* Set in the forked child only. */
static int in_child;

static size_t counters(void) { return (size_t)(__stop___sancov_cntrs - __start___sancov_cntrs); }

int LLVMFuzzerInitialize(int *argc, char ***argv) {
    (void)argc;
    (void)argv;
    out_dir = getenv("NZ_FUZZ_OUT");
    const char *d = getenv("NZ_FUZZ_DEADLINE_MS");
    if (d && atol(d) > 0) deadline_ms = atol(d);
    trace = getenv("NZ_FUZZ_TRACE") != NULL;
    shared_len = counters();
    if (shared_len) {
        shared = mmap(NULL, shared_len, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
        if (shared == MAP_FAILED) {
            perror("nazm-fuzz shim: mmap");
            _exit(77);
        }
    }
    return 0;
}

/* The child's last act: its counters, for the parent. */
static void publish(void) {
    if (shared) memcpy(shared, __start___sancov_cntrs, shared_len);
}

/* libFuzzer treats a call to `exit` as the target ending the fuzzing (it registers an `atexit`
 * handler that ends the process with status 77), and a Nazm program ends with `exit`. In the child
 * the call is this one: publish the counters and end with the program's own status. In the parent
 * it is libc's. */
void exit(int code) {
    if (in_child) {
        publish();
        _exit(code);
    }
    void (*real)(int) = (void (*)(int))dlsym(RTLD_NEXT, "exit");
    real(code);
    for (;;) _exit(code);
}

static uint64_t fnv(const uint8_t *p, size_t n) {
    uint64_t h = 1469598103934665603ULL;
    for (size_t i = 0; i < n; i++) h = (h ^ p[i]) * 1099511628211ULL;
    return h;
}

/* Keep `data` as OUT/KIND/<hash>, once. */
static void keep(const char *kind, const uint8_t *data, size_t size) {
    if (!out_dir) return;
    char path[4096];
    snprintf(path, sizeof path, "%s/%s", out_dir, kind);
    mkdir(path, 0777);
    snprintf(path, sizeof path, "%s/%s/%016llx", out_dir, kind, (unsigned long long)fnv(data, size));
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0666);
    if (fd < 0) return;
    size_t at = 0;
    while (at < size) {
        ssize_t w = write(fd, data + at, size - at);
        if (w <= 0) break;
        at += (size_t)w;
    }
    close(fd);
}

static long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    int in = memfd_create("nz-input", 0);
    int err = memfd_create("nz-stderr", 0);
    if (in < 0 || err < 0) {
        perror("nazm-fuzz shim: memfd_create");
        _exit(77);
    }
    size_t at = 0;
    while (at < size) {
        ssize_t w = write(in, data + at, size - at);
        if (w <= 0) break;
        at += (size_t)w;
    }
    if (shared) memset(shared, 0, shared_len);
    fflush(NULL);
    pid_t pid = fork();
    if (pid < 0) {
        perror("nazm-fuzz shim: fork");
        _exit(77);
    }
    if (pid == 0) {
        setpgid(0, 0);
        int null = open("/dev/null", O_RDWR);
        dup2(null, 0);
        dup2(null, 1);
        dup2(err, 2);
        char path[64];
        snprintf(path, sizeof path, "/proc/self/fd/%d", in);
        char *argv[] = {"target", path, NULL};
        in_child = 1;
        exit(nz_program_main(2, argv));
    }
    long start = now_ms();
    int status = 0;
    int hung = 0;
    for (;;) {
        pid_t r = waitpid(pid, &status, WNOHANG);
        if (r == pid) break;
        if (r < 0 && errno != EINTR) break;
        if (now_ms() - start > deadline_ms) {
            kill(-pid, SIGKILL);
            kill(pid, SIGKILL);
            waitpid(pid, &status, 0);
            hung = 1;
            break;
        }
        struct timespec nap = {0, 200000};
        nanosleep(&nap, NULL);
    }
    if (trace) {
        size_t reached = 0;
        for (size_t i = 0; shared && i < shared_len; i++) reached += shared[i] != 0;
        fprintf(stderr, "nazm-fuzz trace: %zu bytes, %s %d, %zu of %zu counters\n", size, hung ? "hung" : "status", WIFEXITED(status) ? WEXITSTATUS(status) : -1, reached, shared_len);
    }
    uint8_t *mine = __start___sancov_cntrs;
    if (shared && !hung) {
        for (size_t i = 0; i < shared_len; i++) {
            if (shared[i] && !mine[i]) mine[i] = shared[i];
        }
    }
    char errtext[2049];
    ssize_t got = pread(err, errtext, sizeof errtext - 1, 0);
    errtext[got > 0 ? got : 0] = 0;
    close(in);
    close(err);
    if (hung) {
        keep("hangs", data, size);
        return 0;
    }
    if (WIFEXITED(status)) {
        int code = WEXITSTATUS(status);
        if (code == 0 || code == 1 || code == 2) return 0;
        if (strstr(errtext, "N0406")) {
            keep("oom", data, size);
            return 0;
        }
        fprintf(stderr, "nazm-fuzz: the target ended with status %d\n%s\n", code, errtext);
    } else if (WIFSIGNALED(status)) {
        if (strstr(errtext, "N0406")) {
            keep("oom", data, size);
            return 0;
        }
        fprintf(stderr, "nazm-fuzz: the target was ended by signal %d\n%s\n", WTERMSIG(status), errtext);
    }
    abort();
}
