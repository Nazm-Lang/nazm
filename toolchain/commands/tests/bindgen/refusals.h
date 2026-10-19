// refusals
void nothing(void);
int plain(int a);
double dbl(double d);
float flt(float f);
long double ld(void);
int64_t ptr(int64_t *p);
char *mutstr(char *s);
int64_t variadic(const char *f, ...);
int64_t cb(int64_t (*fn)(int64_t));
unsigned long ul(unsigned long u);
int64_t trailing(int64_t a) __attribute__((x));
int64_t noparen;
int x;
