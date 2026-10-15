// C for fail_callback.nz: calls the Nazm export it is given.
#include <stdint.h>
int64_t apply(int64_t (*f)(int64_t), int64_t x) { return f(x) + 1; }
