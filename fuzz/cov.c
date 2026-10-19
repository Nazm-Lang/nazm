/* Edge coverage for the Nazm fuzzer (N93, docs/internal/architecture.md §7.94).
 *
 * The Rust code under test is compiled with LLVM's SanitizerCoverage in trace-pc-guard mode:
 * every edge calls `__sanitizer_cov_trace_pc_guard` with a pointer to its own guard. These are
 * the callbacks, in C and compiled without instrumentation, so they never call themselves.
 * Each guard is numbered once, from 1; a run marks the guards it reached, and the fuzzer asks
 * how many of them no earlier run had reached. Nothing here is Nazm's semantics.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define NZ_GUARDS (1u << 22)

static uint32_t guards;
static uint8_t hit[NZ_GUARDS];
static uint8_t seen[NZ_GUARDS];

void __sanitizer_cov_trace_pc_guard_init(uint32_t *start, uint32_t *stop) {
    if (start == stop || *start) return;
    for (uint32_t *g = start; g < stop; g++) {
        if (guards + 1 < NZ_GUARDS) *g = ++guards;
    }
}

void __sanitizer_cov_trace_pc_guard(uint32_t *guard) {
    uint32_t i = *guard;
    if (i) hit[i] = 1;
}

/* Forget the last run's edges. */
void nz_cov_reset(void) { memset(hit, 0, (size_t)guards + 1); }

/* How many of the last run's edges no earlier run reached, without remembering them. */
size_t nz_cov_fresh(void) {
    size_t fresh = 0;
    for (uint32_t i = 1; i <= guards; i++) fresh += hit[i] && !seen[i];
    return fresh;
}

/* Remember the last run's edges as reached. */
void nz_cov_keep(void) {
    for (uint32_t i = 1; i <= guards; i++) seen[i] |= hit[i];
}

/* Whether the last run reached edge `i` — for minimising a corpus. */
int nz_cov_hit(uint32_t i) { return i <= guards && hit[i]; }

size_t nz_cov_guards(void) { return guards; }

size_t nz_cov_seen(void) {
    size_t n = 0;
    for (uint32_t i = 1; i <= guards; i++) n += seen[i];
    return n;
}
