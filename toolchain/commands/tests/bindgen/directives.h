#define VERSION 3
#define GUARD
#define MAX(a, b) ((a) > (b) ? (a) : (b))
#if defined(X)
#else
#endif
#undef GUARD
#error nope
#line 5
int64_t kept(int64_t a);
