// Drives the runtime's TLS part and a target's engine unit in memory, a client and a server
// passing ciphertext between them as `@std/tls` would through a socket (v1 track C1b). Built by
// `tests/tls.rs` for Windows with MinGW-w64 against the SChannel unit, to show that every entry the
// unit calls is in `libsecur32`, `libcrypt32` and `libncrypt`; run on a Windows host by hand. The
// certificates are the test's: ca.pem, srv.pem/srv.key, old.pem, self.pem/self.key.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#define NZ(name) __asm__("nz." name)
#else
#define NZ(name) __asm__("_nz." name)
#endif

long cc(const char *, long, const char *, long) NZ("os_tls_client_config");
long sc(const char *, long, const char *, long, const char *, long) NZ("os_tls_server_config");
long conn(long, const char *, long) NZ("os_tls_connect");
long acc(long) NZ("os_tls_accept");
long hs(long) NZ("os_tls_handshake");
long push(long, const char *, long) NZ("os_tls_push");
long pull(long, char *, long) NZ("os_tls_pull");
long rd(long, char *, long) NZ("os_tls_read");
long wr(long, const char *, long) NZ("os_tls_write");
long cl(long, char *, long) NZ("os_tls_close");
long info(long, long, char *, long) NZ("os_tls_info");

static char *slurp(const char *p, long *n) {
  FILE *f = fopen(p, "rb");
  char *o = calloc(1, 16384);
  *n = f ? (long)fread(o, 1, 16383, f) : 0;
  if (f) fclose(f);
  return o;
}

static void pump(long a, long b) {
  char buf[20000];
  long n = pull(a, buf, sizeof buf);
  if (n > 0) push(b, buf, n);
}

static void trial(const char *name, const char *ca_path, const char *cert_path, const char *key_path, const char *host) {
  long nca = 0, nc, nk;
  char *ca = ca_path ? slurp(ca_path, &nca) : calloc(1, 1);
  char *cert = slurp(cert_path, &nc), *key = slurp(key_path, &nk);
  long scfg = sc(cert, nc, key, nk, "\x08http/1.1", 9);
  long ccfg = cc(ca, nca, "\x02h2\x08http/1.1", 12);
  long s = acc(scfg), c = conn(ccfg, host, (long)strlen(host));
  long rc = 0, rs = 0;
  for (int i = 0; i < 20; i++) {
    rc = hs(c);
    pump(c, s);
    rs = hs(s);
    pump(s, c);
    if (rc == 1 && rs == 1) break;
    if (rc < 0 && rc != -65600) break;
  }
  char a[64] = {0}, v[64] = {0};
  info(c, 0, a, 63);
  info(c, 1, v, 63);
  printf("%s: config %s, client %ld", name, scfg > 0 && ccfg > 0 ? "ok" : "refused", rc);
  if (rc == 1) {
    printf(", %s [%s]", v, a);
    long w = wr(c, "hello", 5);
    pump(c, s);
    char got[16] = {0};
    long r = rd(s, got, 15);
    char out[256];
    long z = cl(c, out, sizeof out);
    push(s, out, z);
    long r2 = rd(s, got + 8, 7);
    printf(", wrote %ld, read %ld [%.*s], closed %ld", w, r, (int)(r > 0 ? r : 0), got, r2);
  } else {
    cl(c, NULL, 0);
  }
  printf("\n");
  cl(s, NULL, 0);
  cl(scfg, NULL, 0);
  cl(ccfg, NULL, 0);
}

int main(void) {
  trial("trusted", "ca.pem", "srv.pem", "srv.key", "localhost");
  trial("another name", "ca.pem", "srv.pem", "srv.key", "example.org");
  trial("expired", "ca.pem", "old.pem", "srv.key", "localhost");
  trial("self-signed", "ca.pem", "self.pem", "self.key", "localhost");
  trial("platform store", NULL, "srv.pem", "srv.key", "localhost");
  return 0;
}
