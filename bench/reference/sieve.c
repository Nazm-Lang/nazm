/* The same sieve as `bench/programs/sieve.nz`, statement for statement.
 *
 * "The same" is the whole point and the easiest thing to get wrong. This does not use a
 * bitset, does not stop the inner loop early, and does not skip even numbers — because
 * the Nazm program does none of those. A C program written the way a C programmer would
 * write it would be measuring a different algorithm, and the comparison would say
 * nothing about the compilers.
 *
 * The one difference that cannot be removed: Nazm's `Ints` is a growable heap sequence
 * whose reads are bounds-checked, and this is a plain `long*`. That is a language
 * difference rather than a compiler one, and it is what the numbers are mostly about.
 */
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    long limit = 400000;
    long *sieve = malloc((size_t)(limit + 1) * sizeof(long));
    if (!sieve) return 1;

    long i = 0;
    while (i <= limit) { sieve[i] = 1; i = i + 1; }

    long p = 2;
    while (p * p <= limit) {
        if (sieve[p] == 1) {
            long m = p * p;
            while (m <= limit) { sieve[m] = 0; m = m + p; }
        }
        p = p + 1;
    }

    long count = 0;
    long n = 2;
    while (n <= limit) { count = count + sieve[n]; n = n + 1; }

    printf("%ld\n", count);
    return 0;
}
