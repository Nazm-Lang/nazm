/* The C side of ffi.nz: what Nazm calls, and calls back into its exports with every number class
   (design D4's C convention: narrow arguments and results, `bool`, stack arguments). */
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

double nz_scale(double x, int32_t k);
uint32_t nz_mask(uint8_t x, uint16_t y);
float nz_half(float x);
int16_t nz_small(int8_t x, int16_t y);
bool nz_odd(int64_t x);
int64_t nz_wide(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f, int64_t g,
                int64_t h, int64_t i, int64_t j);

struct Pair { int64_t a; bool flag; int64_t b; };
struct Box { int64_t v; };

int64_t c_add(int64_t a, int64_t b) { return a + b; }
bool c_both(bool a, int64_t b) { return a && b > 0; }
int64_t c_many(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f, int64_t g,
               int64_t h, int64_t i, bool j, int64_t k) {
  return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + (j ? 10 : 0) + 11 * k;
}
int64_t c_len(const char *s) { return (int64_t)strlen(s); }
const char *c_word(int64_t n) { return n == 2 ? "two" : "other"; }
const char *c_maybe(int64_t n) { return n ? "something" : NULL; }
int64_t c_pair(const struct Pair *p) { return p->a * 10 + p->b + (p->flag ? 100 : 0); }
struct Box *box_new(int64_t v) { struct Box *b = malloc(sizeof *b); b->v = v; return b; }
int64_t box_take(struct Box *b) { int64_t v = b->v + 1; free(b); return v; }
int64_t c_apply(int64_t (*f)(int64_t), int64_t x) { return f(f(x)); }

/* The exports, called from C, their results folded into one number. */
int64_t c_numbers(void) {
  double s = nz_scale(1.5, -4);
  uint32_t m = nz_mask(0xAB, 0x1234);
  float h = nz_half(5.0f);
  int16_t sm = nz_small(-3, 1000);
  bool o1 = nz_odd(7), o2 = nz_odd(8);
  int64_t w = nz_wide(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
  return (int64_t)(s * 10) + (int64_t)m + (int64_t)(h * 10) + sm + (o1 ? 1000000 : 0) +
         (o2 ? 2000000 : 0) + w * 100000000;
}
