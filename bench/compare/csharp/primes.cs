// The parallel job: primes below 3,000,000 by trial division, blocks of 1,000 dealt round robin to
// W threads (block b to worker b mod W); prints 216816.
using System.Threading;

int w = args.Length > 0 ? int.Parse(args[0]) : 1;
var found = new long[w];
var t = new Thread[w];
for (int k = 0; k < w; k++) {
    int kk = k;
    t[k] = new Thread(() => {
        for (long b = kk; b < 3000; b += w)
            for (long n = b * 1000; n < b * 1000 + 1000; n++)
                if (IsPrime(n)) found[kk]++;
    });
    t[k].Start();
}
long total = 0;
for (int k = 0; k < w; k++) { t[k].Join(); total += found[k]; }
System.Console.WriteLine(total);

static bool IsPrime(long n) {
    if (n < 2) return false;
    if (n % 2 == 0) return n == 2;
    for (long d = 3; d * d <= n; d += 2)
        if (n % d == 0) return false;
    return true;
}
