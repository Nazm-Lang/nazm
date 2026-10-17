//! TLS (v1 track C1, `docs/spec.md`, *TLS*): the `tls` runtime part, as target-independent LLVM
//! text over a per-target engine unit ([`emit_tls`]).
//!
//! # Buffers, never the socket
//!
//! The pool parks a task on a socket and wakes it from the reactor (*Networking*, *Waiting*). An
//! engine that read its own socket would block a worker in C, so the engine here is **sans-I/O**:
//! the program reads ciphertext through the parking `os_read`, hands it to the engine
//! (`os_tls_push`), takes what the engine wants sent (`os_tls_pull`) and writes it through
//! `os_write`. Nothing in this part waits; `@std/tls` drives it.
//!
//! # Sessions are ids
//!
//! A configuration or a session is an `Int` the program holds, never an address: an index into a
//! table of 65 535 slots and the slot's generation, so an id that was closed, or one a program made
//! up, answers `Closed` and never reaches an object that has since taken its slot. Each use holds
//! the slot for the length of one engine call — calls are short and never wait — so two tasks
//! sharing a session take turns rather than racing inside the engine.
//!
//! # The engines
//!
//! OpenSSL 3 on macOS and Linux, over memory BIOs; on macOS a client's verdict on the server's
//! chain and name is the Security framework's (`SecTrust`), so trust decisions are Apple's, against
//! the system's store or the anchors the program names. On Linux OpenSSL verifies, against the
//! system bundle or the named anchors, with the host name checked by `SSL_set1_host`. Windows'
//! binding is SChannel through SSPI ([`emit_schannel`], C1b): written, compiled and linked against
//! MinGW-w64's import libraries, but not yet run on a Windows host, so a Windows build reaching
//! TLS is still refused by name (`N0830`), as every target without a binding is. Verification is
//! never optional: there is no entry that turns it off.

use crate::switch::SwitchTarget;

/// The statuses this part adds, each a number no system gives an error (`@std/tls` names them).
pub const WANT: i64 = -65600;
pub const UNTRUSTED: i64 = -65601;
pub const EXPIRED: i64 = -65602;
pub const WRONG_HOST: i64 = -65603;
pub const PROTOCOL: i64 = -65604;
pub const BAD_KEY: i64 = -65605;

/// The runtime text: the table and the `nz.os_tls_*` entries.
pub(crate) const TLS_RUNTIME: &str = r#"
; TLS (C1): 65 535 slots of { object, generation, kind, held }, made on first use. Kind 0 is free,
; -1 being taken, 1 a client configuration, 2 a server's, 3 a session. An id is the generation
; above 16 bits and the slot's index below; index 0 is never a slot, so no id is 0.
@nz.tls_slots = hidden global ptr null
@nz.tls_hint = hidden global i64 1

declare ptr @nz_tls_config(i64, ptr, i64, ptr, i64, ptr, i64, ptr)
declare void @nz_tls_config_free(ptr)
declare ptr @nz_tls_session(ptr, ptr, i64, ptr)
declare i64 @nz_tls_handshake(ptr)
declare i64 @nz_tls_push(ptr, ptr, i64)
declare i64 @nz_tls_pull(ptr, ptr, i64)
declare i64 @nz_tls_read(ptr, ptr, i64)
declare i64 @nz_tls_write(ptr, ptr, i64)
declare i64 @nz_tls_close(ptr, ptr, i64)
declare i64 @nz_tls_info(ptr, i64, ptr, i64)

; The table, made zeroed by the first caller that needs it; null when memory is refused.
define hidden ptr @nz.tls_table() noinline {
entry:
  %t = load atomic ptr, ptr @nz.tls_slots acquire, align 8
  %none = icmp eq ptr %t, null
  br i1 %none, label %make, label %have
have:
  ret ptr %t
make:
  %m = call ptr @malloc(i64 2097152)
  %nm = icmp eq ptr %m, null
  br i1 %nm, label %fail, label %zero
fail:
  ret ptr null
zero:
  call void @llvm.memset.p0.i64(ptr %m, i8 0, i64 2097152, i1 false)
  %pair = cmpxchg ptr @nz.tls_slots, ptr null, ptr %m acq_rel acquire
  %won = extractvalue { ptr, i1 } %pair, 1
  br i1 %won, label %mine, label %theirs
mine:
  ret ptr %m
theirs:
  call void @free(ptr %m)
  %o = extractvalue { ptr, i1 } %pair, 0
  ret ptr %o
}

; A free slot for `obj` of `kind`: its id; -24 (too many open) when every slot is taken, -12 when
; the table cannot be made.
define hidden i64 @nz.tls_put(ptr %obj, i64 %kind) {
entry:
  %table = call ptr @nz.tls_table()
  %nt = icmp eq ptr %table, null
  br i1 %nt, label %nomem, label %start
nomem:
  ret i64 -12
start:
  %h = load atomic i64, ptr @nz.tls_hint monotonic, align 8
  br label %loop
loop:
  %n = phi i64 [ 0, %start ], [ %n1, %next ]
  %i0 = phi i64 [ %h, %start ], [ %i1, %next ]
  %done = icmp eq i64 %n, 65535
  br i1 %done, label %full, label %try
try:
  %low = icmp ult i64 %i0, 1
  %high = icmp ugt i64 %i0, 65535
  %wrap = or i1 %low, %high
  %i = select i1 %wrap, i64 1, i64 %i0
  %s = getelementptr [4 x i64], ptr %table, i64 %i
  %kp = getelementptr i64, ptr %s, i64 2
  %pair = cmpxchg ptr %kp, i64 0, i64 -1 acq_rel monotonic
  %won = extractvalue { i64, i1 } %pair, 1
  br i1 %won, label %take, label %next
next:
  %n1 = add i64 %n, 1
  %i1 = add i64 %i, 1
  br label %loop
take:
  %gp = getelementptr i64, ptr %s, i64 1
  %g = load atomic i64, ptr %gp monotonic, align 8
  %g1 = add i64 %g, 1
  store atomic i64 %g1, ptr %gp monotonic, align 8
  store ptr %obj, ptr %s
  %after = add i64 %i, 1
  store atomic i64 %after, ptr @nz.tls_hint monotonic, align 8
  store atomic i64 %kind, ptr %kp release, align 8
  %hi = shl i64 %g1, 16
  %id = or i64 %hi, %i
  ret i64 %id
full:
  ret i64 -24
}

; `id`'s slot, or null for an id no slot has.
define hidden ptr @nz.tls_slot(i64 %id) {
entry:
  %i = and i64 %id, 65535
  %some = icmp ne i64 %i, 0
  %pos = icmp sgt i64 %id, 65535
  %ok = and i1 %some, %pos
  br i1 %ok, label %table, label %no
table:
  %t = load atomic ptr, ptr @nz.tls_slots acquire, align 8
  %made = icmp ne ptr %t, null
  br i1 %made, label %yes, label %no
yes:
  %s = getelementptr [4 x i64], ptr %t, i64 %i
  ret ptr %s
no:
  ret ptr null
}

; Hold slot `s` for `id`: its object when the id is live and of `kind` (0: any kind), the slot
; then held until `nz.tls_let_go`; null, and nothing held, otherwise.
define hidden ptr @nz.tls_hold(ptr %s, i64 %id, i64 %kind) noinline {
entry:
  %bp = getelementptr i64, ptr %s, i64 3
  br label %spin
spin:
  %pair = cmpxchg ptr %bp, i64 0, i64 1 acquire monotonic
  %won = extractvalue { i64, i1 } %pair, 1
  br i1 %won, label %check, label %spin
check:
  %gp = getelementptr i64, ptr %s, i64 1
  %g = load atomic i64, ptr %gp monotonic, align 8
  %want = lshr i64 %id, 16
  %same = icmp eq i64 %g, %want
  %kp = getelementptr i64, ptr %s, i64 2
  %k = load atomic i64, ptr %kp acquire, align 8
  %live = icmp sgt i64 %k, 0
  %any = icmp eq i64 %kind, 0
  %match = icmp eq i64 %k, %kind
  %kok = or i1 %any, %match
  %a = and i1 %same, %live
  %b = and i1 %a, %kok
  br i1 %b, label %yes, label %no
yes:
  %o = load ptr, ptr %s
  ret ptr %o
no:
  store atomic i64 0, ptr %bp release, align 8
  ret ptr null
}

define hidden void @nz.tls_let_go(ptr %s) {
entry:
  %bp = getelementptr i64, ptr %s, i64 3
  store atomic i64 0, ptr %bp release, align 8
  ret void
}

; A configuration of `kind` from the engine, put in a slot: its id, or the engine's status.
define hidden i64 @nz.tls_configure(i64 %kind, ptr %a, i64 %an, ptr %b, i64 %bn, ptr %p, i64 %pn) {
entry:
  %st = alloca i64, align 8
  store i64 -65604, ptr %st
  %o = call ptr @nz_tls_config(i64 %kind, ptr %a, i64 %an, ptr %b, i64 %bn, ptr %p, i64 %pn, ptr %st)
  %none = icmp eq ptr %o, null
  br i1 %none, label %bad, label %put
bad:
  %s = load i64, ptr %st
  ret i64 %s
put:
  %id = call i64 @nz.tls_put(ptr %o, i64 %kind)
  %full = icmp slt i64 %id, 0
  br i1 %full, label %drop, label %ok
drop:
  call void @nz_tls_config_free(ptr %o)
  ret i64 %id
ok:
  ret i64 %id
}

define hidden i64 @nz.os_tls_client_config(ptr %ca, i64 %can, ptr %p, i64 %pn) {
entry:
  %r = call i64 @nz.tls_configure(i64 1, ptr %ca, i64 %can, ptr null, i64 0, ptr %p, i64 %pn)
  ret i64 %r
}

define hidden i64 @nz.os_tls_server_config(ptr %c, i64 %cn, ptr %k, i64 %kn, ptr %p, i64 %pn) {
entry:
  %r = call i64 @nz.tls_configure(i64 2, ptr %c, i64 %cn, ptr %k, i64 %kn, ptr %p, i64 %pn)
  ret i64 %r
}

; A session from configuration `cfg` of `kind`, verifying `host` for a client: its id or a status.
define hidden i64 @nz.tls_open(i64 %cfg, i64 %kind, ptr %h, i64 %hn) {
entry:
  %st = alloca i64, align 8
  store i64 -65604, ptr %st
  %s = call ptr @nz.tls_slot(i64 %cfg)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %closed, label %hold
hold:
  %c = call ptr @nz.tls_hold(ptr %s, i64 %cfg, i64 %kind)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %closed, label %make
make:
  %e = call ptr @nz_tls_session(ptr %c, ptr %h, i64 %hn, ptr %st)
  call void @nz.tls_let_go(ptr %s)
  %ne = icmp eq ptr %e, null
  br i1 %ne, label %bad, label %put
bad:
  %why = load i64, ptr %st
  ret i64 %why
put:
  %id = call i64 @nz.tls_put(ptr %e, i64 3)
  %full = icmp slt i64 %id, 0
  br i1 %full, label %drop, label %ok
drop:
  %z = call i64 @nz_tls_close(ptr %e, ptr null, i64 0)
  ret i64 %id
ok:
  ret i64 %id
closed:
  ret i64 -65536
}

define hidden i64 @nz.os_tls_connect(i64 %cfg, ptr %h, i64 %hn) {
entry:
  %r = call i64 @nz.tls_open(i64 %cfg, i64 1, ptr %h, i64 %hn)
  ret i64 %r
}

define hidden i64 @nz.os_tls_accept(i64 %cfg) {
entry:
  %r = call i64 @nz.tls_open(i64 %cfg, i64 2, ptr null, i64 0)
  ret i64 %r
}

; One engine call on session `id`, with its slot held: 0 the handshake, 1 push, 2 pull, 3 read,
; 4 write, 5 a fact (`what`). -65536 for an id that is not a live session.
define hidden i64 @nz.tls_call(i64 %id, i64 %op, ptr %p, i64 %n, i64 %what) {
entry:
  %s = call ptr @nz.tls_slot(i64 %id)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %closed, label %hold
hold:
  %e = call ptr @nz.tls_hold(ptr %s, i64 %id, i64 3)
  %ne = icmp eq ptr %e, null
  br i1 %ne, label %closed, label %pick
pick:
  switch i64 %op, label %info [ i64 0, label %hs
                                i64 1, label %push
                                i64 2, label %pull
                                i64 3, label %read
                                i64 4, label %write ]
hs:
  %r0 = call i64 @nz_tls_handshake(ptr %e)
  br label %done
push:
  %r1 = call i64 @nz_tls_push(ptr %e, ptr %p, i64 %n)
  br label %done
pull:
  %r2 = call i64 @nz_tls_pull(ptr %e, ptr %p, i64 %n)
  br label %done
read:
  %r3 = call i64 @nz_tls_read(ptr %e, ptr %p, i64 %n)
  br label %done
write:
  %r4 = call i64 @nz_tls_write(ptr %e, ptr %p, i64 %n)
  br label %done
info:
  %r5 = call i64 @nz_tls_info(ptr %e, i64 %what, ptr %p, i64 %n)
  br label %done
done:
  %r = phi i64 [ %r0, %hs ], [ %r1, %push ], [ %r2, %pull ], [ %r3, %read ], [ %r4, %write ], [ %r5, %info ]
  call void @nz.tls_let_go(ptr %s)
  ret i64 %r
closed:
  ret i64 -65536
}

define hidden i64 @nz.os_tls_handshake(i64 %id) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 0, ptr null, i64 0, i64 0)
  ret i64 %r
}

define hidden i64 @nz.os_tls_push(i64 %id, ptr %p, i64 %n) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 1, ptr %p, i64 %n, i64 0)
  ret i64 %r
}

define hidden i64 @nz.os_tls_pull(i64 %id, ptr %p, i64 %n) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 2, ptr %p, i64 %n, i64 0)
  ret i64 %r
}

define hidden i64 @nz.os_tls_read(i64 %id, ptr %p, i64 %n) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 3, ptr %p, i64 %n, i64 0)
  ret i64 %r
}

define hidden i64 @nz.os_tls_write(i64 %id, ptr %p, i64 %n) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 4, ptr %p, i64 %n, i64 0)
  ret i64 %r
}

define hidden i64 @nz.os_tls_info(i64 %id, i64 %what, ptr %p, i64 %n) {
entry:
  %r = call i64 @nz.tls_call(i64 %id, i64 5, ptr %p, i64 %n, i64 %what)
  ret i64 %r
}

; Close `id`, a session or a configuration: a session's last ciphertext (its close_notify) into
; `p`, and the id dead from now on. A configuration's sessions keep what they took from it.
define hidden i64 @nz.os_tls_close(i64 %id, ptr %p, i64 %n) {
entry:
  %s = call ptr @nz.tls_slot(i64 %id)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %closed, label %hold
hold:
  %o = call ptr @nz.tls_hold(ptr %s, i64 %id, i64 0)
  %no = icmp eq ptr %o, null
  br i1 %no, label %closed, label %which
which:
  %kp = getelementptr i64, ptr %s, i64 2
  %k = load atomic i64, ptr %kp acquire, align 8
  %session = icmp eq i64 %k, 3
  br i1 %session, label %end, label %config
end:
  %r = call i64 @nz_tls_close(ptr %o, ptr %p, i64 %n)
  br label %free
config:
  call void @nz_tls_config_free(ptr %o)
  br label %free
free:
  %out = phi i64 [ %r, %end ], [ 0, %config ]
  store ptr null, ptr %s
  store atomic i64 0, ptr %kp release, align 8
  call void @nz.tls_let_go(ptr %s)
  ret i64 %out
closed:
  ret i64 -65536
}
"#;

/// The engine unit for `target`, or `None` where TLS has no binding yet (`N0830`).
#[must_use]
pub fn emit_tls(target: SwitchTarget) -> Option<String> {
    match target {
        SwitchTarget::Aarch64Darwin | SwitchTarget::X86_64Darwin => {
            Some(format!("{OPENSSL}{SECTRUST}"))
        }
        SwitchTarget::Aarch64Linux | SwitchTarget::X86_64Linux => {
            Some(format!("{OPENSSL}{OPENSSL_TRUST}"))
        }
        // SChannel's unit (`emit_schannel`) is compiled and linked, not yet run on Windows: until
        // it is, Windows stays without a binding.
        SwitchTarget::X86_64Windows | SwitchTarget::Aarch64Android => None,
    }
}

/// The runtime's TLS text alone — the table and the entries — for a check that compiles it beside
/// an engine unit with another target's toolchain.
#[must_use]
pub fn runtime_text() -> &'static str {
    TLS_RUNTIME
}

/// Windows' engine unit (C1b): SChannel through SSPI, over buffers as OpenSSL's is.
///
/// A client's credentials ask for manual validation, and the unit judges the chain the server sent
/// itself, as macOS's does with `SecTrust`: `CertGetCertificateChain` against the user's roots, or
/// an engine whose only roots are the configuration's anchors (`hExclusiveRoot`), with no URL
/// fetched, then `CertVerifyCertificateChainPolicy`'s SSL policy for the host. A server's key, PKCS
/// #8 PEM, is imported into the user's key storage provider under a name of its own — SChannel,
/// running in LSASS, cannot use an ephemeral key — and deleted when the configuration goes. TLS 1.2
/// (`SCHANNEL_CRED` has no TLS 1.3), ALPN through `SECBUFFER_APPLICATION_PROTOCOLS`. Linked with
/// `-lsecur32 -lcrypt32 -lncrypt`, MinGW-w64's import libraries: nothing of Microsoft's SDK.
///
/// Not yet [`emit_tls`]'s for `X86_64Windows`: it has been compiled and linked, never run, and
/// `N0830` stands until a Windows host has run the TLS tests against it.
#[must_use]
pub fn emit_schannel() -> String {
    SCHANNEL.to_owned()
}

/// OpenSSL 3 over memory BIOs: what both of its targets share. A configuration is
/// `{ SSL_CTX, kind, anchors, alpn, alpn length }`; a session
/// `{ SSL, anchors, failure, host, host length, alpn, alpn length, detail[200] }`.
/// The trust decision is the target's: `nz_tls_trust`, `nz_tls_name` and `nz_tls_verdict`.
const OPENSSL: &str = r#"
; The TLS engine (C1): OpenSSL 3, over memory BIOs. Generated by nazm build. Do not edit.
declare ptr @calloc(i64, i64)
declare ptr @malloc(i64)
declare void @free(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare ptr @TLS_client_method()
declare ptr @TLS_server_method()
declare ptr @SSL_CTX_new(ptr)
declare void @SSL_CTX_free(ptr)
declare i64 @SSL_CTX_ctrl(ptr, i32, i64, ptr)
declare void @SSL_CTX_set_verify(ptr, i32, ptr)
declare void @SSL_CTX_set_alpn_select_cb(ptr, ptr, ptr)
declare i32 @SSL_CTX_set_alpn_protos(ptr, ptr, i32)
declare i32 @SSL_CTX_use_certificate(ptr, ptr)
declare i32 @SSL_CTX_use_PrivateKey(ptr, ptr)
declare i32 @SSL_CTX_check_private_key(ptr)
declare ptr @BIO_new_mem_buf(ptr, i32)
declare ptr @BIO_new(ptr)
declare ptr @BIO_s_mem()
declare i32 @BIO_free(ptr)
declare i32 @BIO_read(ptr, ptr, i32)
declare i32 @BIO_write(ptr, ptr, i32)
declare ptr @PEM_read_bio_X509(ptr, ptr, ptr, ptr)
declare ptr @PEM_read_bio_PrivateKey(ptr, ptr, ptr, ptr)
declare void @X509_free(ptr)
declare void @EVP_PKEY_free(ptr)
declare void @ERR_clear_error()
declare i64 @ERR_peek_last_error()
declare void @ERR_error_string_n(i64, ptr, i64)
declare ptr @SSL_new(ptr)
declare void @SSL_free(ptr)
declare i32 @SSL_set_ex_data(ptr, i32, ptr)
declare ptr @SSL_get_ex_data(ptr, i32)
declare void @SSL_set_bio(ptr, ptr, ptr)
declare i64 @SSL_ctrl(ptr, i32, i64, ptr)
declare void @SSL_set_connect_state(ptr)
declare void @SSL_set_accept_state(ptr)
declare i32 @SSL_do_handshake(ptr)
declare i32 @SSL_get_error(ptr, i32)
declare i32 @SSL_read(ptr, ptr, i32)
declare i32 @SSL_write(ptr, ptr, i32)
declare ptr @SSL_get_rbio(ptr)
declare ptr @SSL_get_wbio(ptr)
declare i32 @SSL_shutdown(ptr)
declare i32 @SSL_is_init_finished(ptr)
declare void @SSL_get0_alpn_selected(ptr, ptr, ptr)
declare ptr @SSL_get_version(ptr)
declare ptr @SSL_get1_peer_certificate(ptr)
declare ptr @X509_get_subject_name(ptr)
declare ptr @X509_NAME_oneline(ptr, ptr, i32)
declare i32 @SSL_select_next_proto(ptr, ptr, ptr, i32, ptr, i32)
declare i64 @strlen(ptr)

; A password callback that has none: an encrypted key is refused, never asked for on a terminal.
define hidden i32 @nz_tls_nopass(ptr %b, i32 %n, i32 %rw, ptr %u) {
entry:
  ret i32 0
}

; Whether `p[0..n]` is ALPN's wire form: one or more names, each a length byte of 1 to 255 and
; that many bytes.
define hidden i1 @nz_tls_alpn_ok(ptr %p, i64 %n) {
entry:
  %some = icmp sgt i64 %n, 0
  br i1 %some, label %loop, label %no
loop:
  %at = phi i64 [ 0, %entry ], [ %next, %more ]
  %lp = getelementptr i8, ptr %p, i64 %at
  %lb = load i8, ptr %lp
  %l = zext i8 %lb to i64
  %empty = icmp eq i64 %l, 0
  br i1 %empty, label %no, label %step
step:
  %after = add i64 %at, 1
  %next = add i64 %after, %l
  %over = icmp sgt i64 %next, %n
  br i1 %over, label %no, label %fits
fits:
  %end = icmp eq i64 %next, %n
  br i1 %end, label %yes, label %more
more:
  br label %loop
yes:
  ret i1 true
no:
  ret i1 false
}

; The server's choice among what the client offers: the first of its own, in its order, that the
; client offers too; none when they share none, and the handshake goes on without one.
define hidden i32 @nz_tls_alpn_select(ptr %ssl, ptr %out, ptr %outlen, ptr %in, i32 %inlen, ptr %arg) {
entry:
  %e = call ptr @SSL_get_ex_data(ptr %ssl, i32 0)
  %ne = icmp eq ptr %e, null
  br i1 %ne, label %none, label %have
have:
  %app = getelementptr i8, ptr %e, i64 40
  %a = load ptr, ptr %app
  %anp = getelementptr i8, ptr %e, i64 48
  %an = load i64, ptr %anp
  %zero = icmp eq i64 %an, 0
  br i1 %zero, label %none, label %pick
pick:
  %an32 = trunc i64 %an to i32
  %r = call i32 @SSL_select_next_proto(ptr %out, ptr %outlen, ptr %a, i32 %an32, ptr %in, i32 %inlen)
  %agreed = icmp eq i32 %r, 1
  br i1 %agreed, label %ok, label %none
ok:
  ret i32 0
none:
  ret i32 3
}

; A copy of `p[0..n]` in memory of its own, NUL-terminated; null when memory is refused.
define hidden ptr @nz_tls_copy(ptr %p, i64 %n) {
entry:
  %m = add i64 %n, 1
  %c = call ptr @calloc(i64 %m, i64 1)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %out, label %fill
fill:
  call void @llvm.memcpy.p0.p0.i64(ptr %c, ptr %p, i64 %n, i1 false)
  br label %out
out:
  ret ptr %c
}

; A configuration of `kind` (1 a client's: `a` its anchors, none for the system's; 2 a server's:
; `a` its certificate chain, `b` its key, both PEM), offering the ALPN names `p` (wire form, or
; none). Null with the status in `st` when anything is refused.
define hidden ptr @nz_tls_config(i64 %kind, ptr %a, i64 %an, ptr %b, i64 %bn, ptr %p, i64 %pn, ptr %st) {
entry:
  call void @ERR_clear_error()
  %has = icmp sgt i64 %pn, 0
  br i1 %has, label %check, label %sizes
check:
  %good = call i1 @nz_tls_alpn_ok(ptr %p, i64 %pn)
  br i1 %good, label %sizes, label %invalid
sizes:
  %big = icmp sgt i64 %an, 1073741824
  %big2 = icmp sgt i64 %bn, 1073741824
  %big3 = icmp sgt i64 %pn, 65535
  %b12 = or i1 %big, %big2
  %toobig = or i1 %b12, %big3
  br i1 %toobig, label %invalid, label %make
make:
  %c = call ptr @calloc(i64 1, i64 40)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %nomem, label %method
method:
  %client = icmp eq i64 %kind, 1
  br i1 %client, label %cm, label %sm
cm:
  %mc = call ptr @TLS_client_method()
  br label %ctx
sm:
  %ms = call ptr @TLS_server_method()
  br label %ctx
ctx:
  %m = phi ptr [ %mc, %cm ], [ %ms, %sm ]
  %x = call ptr @SSL_CTX_new(ptr %m)
  %nx = icmp eq ptr %x, null
  br i1 %nx, label %nomem1, label %set
set:
  store ptr %x, ptr %c
  %kp = getelementptr i8, ptr %c, i64 8
  store i64 %kind, ptr %kp
  ; TLS 1.2 at least (SSL_CTRL_SET_MIN_PROTO_VERSION, TLS1_2_VERSION).
  %v = call i64 @SSL_CTX_ctrl(ptr %x, i32 123, i64 771, ptr null)
  br i1 %has, label %alpn, label %side
alpn:
  %ac = call ptr @nz_tls_copy(ptr %p, i64 %pn)
  %nac = icmp eq ptr %ac, null
  br i1 %nac, label %nomem2, label %kept
kept:
  %app = getelementptr i8, ptr %c, i64 24
  store ptr %ac, ptr %app
  %anp = getelementptr i8, ptr %c, i64 32
  store i64 %pn, ptr %anp
  br label %side
side:
  br i1 %client, label %trust, label %identity
trust:
  ; Verification is always on: a client refuses a chain or a name it cannot verify.
  call void @SSL_CTX_set_verify(ptr %x, i32 1, ptr null)
  %t = call i32 @nz_tls_trust(ptr %c, ptr %x, ptr %a, i64 %an)
  %tok = icmp eq i32 %t, 0
  br i1 %tok, label %offer, label %badkey
offer:
  br i1 %has, label %offer1, label %done
offer1:
  %pn32 = trunc i64 %pn to i32
  %o = call i32 @SSL_CTX_set_alpn_protos(ptr %x, ptr %p, i32 %pn32)
  br label %done
identity:
  %an32 = trunc i64 %an to i32
  %cb = call ptr @BIO_new_mem_buf(ptr %a, i32 %an32)
  %leaf = call ptr @PEM_read_bio_X509(ptr %cb, ptr null, ptr @nz_tls_nopass, ptr null)
  %nl = icmp eq ptr %leaf, null
  br i1 %nl, label %badcert, label %use
use:
  %u = call i32 @SSL_CTX_use_certificate(ptr %x, ptr %leaf)
  call void @X509_free(ptr %leaf)
  %uok = icmp eq i32 %u, 1
  br i1 %uok, label %chain, label %badcert
chain:
  %more = call ptr @PEM_read_bio_X509(ptr %cb, ptr null, ptr @nz_tls_nopass, ptr null)
  %nm = icmp eq ptr %more, null
  br i1 %nm, label %key, label %extra
extra:
  ; SSL_CTRL_EXTRA_CHAIN_CERT: the context takes the certificate.
  %ec = call i64 @SSL_CTX_ctrl(ptr %x, i32 14, i64 0, ptr %more)
  br label %chain
key:
  %f1 = call i32 @BIO_free(ptr %cb)
  call void @ERR_clear_error()
  %bn32 = trunc i64 %bn to i32
  %kb = call ptr @BIO_new_mem_buf(ptr %b, i32 %bn32)
  %k = call ptr @PEM_read_bio_PrivateKey(ptr %kb, ptr null, ptr @nz_tls_nopass, ptr null)
  %f2 = call i32 @BIO_free(ptr %kb)
  %nk = icmp eq ptr %k, null
  br i1 %nk, label %badkey, label %usekey
usekey:
  %uk = call i32 @SSL_CTX_use_PrivateKey(ptr %x, ptr %k)
  call void @EVP_PKEY_free(ptr %k)
  %ukok = icmp eq i32 %uk, 1
  br i1 %ukok, label %pair, label %badkey
pair:
  %pk = call i32 @SSL_CTX_check_private_key(ptr %x)
  %pkok = icmp eq i32 %pk, 1
  br i1 %pkok, label %select, label %badkey
select:
  call void @SSL_CTX_set_alpn_select_cb(ptr %x, ptr @nz_tls_alpn_select, ptr null)
  br label %done
done:
  call void @ERR_clear_error()
  ret ptr %c
badcert:
  %f3 = call i32 @BIO_free(ptr %cb)
  br label %badkey
badkey:
  call void @nz_tls_config_free(ptr %c)
  call void @ERR_clear_error()
  store i64 -65605, ptr %st
  ret ptr null
nomem2:
  call void @nz_tls_config_free(ptr %c)
  br label %nomem
nomem1:
  call void @free(ptr %c)
  br label %nomem
nomem:
  call void @ERR_clear_error()
  store i64 -12, ptr %st
  ret ptr null
invalid:
  store i64 -22, ptr %st
  ret ptr null
}

define hidden void @nz_tls_config_free(ptr %c) {
entry:
  %x = load ptr, ptr %c
  %nx = icmp eq ptr %x, null
  br i1 %nx, label %rest, label %ctx
ctx:
  call void @SSL_CTX_free(ptr %x)
  br label %rest
rest:
  %anc = getelementptr i8, ptr %c, i64 16
  call void @nz_tls_unanchor(ptr %anc)
  %app = getelementptr i8, ptr %c, i64 24
  %a = load ptr, ptr %app
  call void @free(ptr %a)
  call void @free(ptr %c)
  ret void
}

; Whether `h[0..n]` is an IP address literal, which a client never sends as its server name.
define hidden i1 @nz_tls_is_ip(ptr %h, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %next ]
  %end = icmp eq i64 %i, %n
  br i1 %end, label %yes, label %one
one:
  %cp = getelementptr i8, ptr %h, i64 %i
  %c = load i8, ptr %cp
  %colon = icmp eq i8 %c, 58
  br i1 %colon, label %yes, label %digit
digit:
  %dot = icmp eq i8 %c, 46
  %ge = icmp uge i8 %c, 48
  %le = icmp ule i8 %c, 57
  %d = and i1 %ge, %le
  %ok = or i1 %d, %dot
  br i1 %ok, label %next, label %no
next:
  %i1 = add i64 %i, 1
  br label %loop
yes:
  ret i1 true
no:
  ret i1 false
}

; A session of configuration `c`: a client's verifying `h[0..n]`. Null with the status in `st`.
define hidden ptr @nz_tls_session(ptr %c, ptr %h, i64 %n, ptr %st) {
entry:
  call void @ERR_clear_error()
  %kp = getelementptr i8, ptr %c, i64 8
  %kind = load i64, ptr %kp
  %client = icmp eq i64 %kind, 1
  br i1 %client, label %name, label %make
name:
  ; A client names the server it verifies: not empty, no NUL, at most 253 bytes.
  %empty = icmp slt i64 %n, 1
  %long = icmp sgt i64 %n, 253
  %bad = or i1 %empty, %long
  br i1 %bad, label %invalid, label %nul
nul:
  br label %scan
scan:
  %i = phi i64 [ 0, %nul ], [ %i1, %step ]
  %done = icmp eq i64 %i, %n
  br i1 %done, label %make, label %look
look:
  %bp = getelementptr i8, ptr %h, i64 %i
  %b = load i8, ptr %bp
  %z = icmp eq i8 %b, 0
  br i1 %z, label %invalid, label %step
step:
  %i1 = add i64 %i, 1
  br label %scan
make:
  %e = call ptr @calloc(i64 1, i64 256)
  %ne = icmp eq ptr %e, null
  br i1 %ne, label %nomem, label %ssl
ssl:
  %x = load ptr, ptr %c
  %s = call ptr @SSL_new(ptr %x)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %nomem1, label %bios
bios:
  store ptr %s, ptr %e
  %xd = call i32 @SSL_set_ex_data(ptr %s, i32 0, ptr %e)
  %mm = call ptr @BIO_s_mem()
  %rb = call ptr @BIO_new(ptr %mm)
  %wb = call ptr @BIO_new(ptr %mm)
  call void @SSL_set_bio(ptr %s, ptr %rb, ptr %wb)
  %nr = icmp eq ptr %rb, null
  %nw = icmp eq ptr %wb, null
  %nb = or i1 %nr, %nw
  br i1 %nb, label %nomem2, label %anchors
anchors:
  ; The session keeps its own reference to the anchors and its own copy of the ALPN names, so a
  ; configuration closed meanwhile leaves it whole.
  %canp = getelementptr i8, ptr %c, i64 16
  %ean = getelementptr i8, ptr %e, i64 8
  call void @nz_tls_anchor(ptr %ean, ptr %canp)
  %capp = getelementptr i8, ptr %c, i64 24
  %ca = load ptr, ptr %capp
  %canlp = getelementptr i8, ptr %c, i64 32
  %can = load i64, ptr %canlp
  %hasa = icmp sgt i64 %can, 0
  br i1 %hasa, label %copya, label %side
copya:
  %ac = call ptr @nz_tls_copy(ptr %ca, i64 %can)
  %nac = icmp eq ptr %ac, null
  br i1 %nac, label %nomem2, label %kepta
kepta:
  %eap = getelementptr i8, ptr %e, i64 40
  store ptr %ac, ptr %eap
  %eanp = getelementptr i8, ptr %e, i64 48
  store i64 %can, ptr %eanp
  br label %side
side:
  br i1 %client, label %connect, label %accept
connect:
  %hc = call ptr @nz_tls_copy(ptr %h, i64 %n)
  %nhc = icmp eq ptr %hc, null
  br i1 %nhc, label %nomem2, label %named
named:
  %ehp = getelementptr i8, ptr %e, i64 24
  store ptr %hc, ptr %ehp
  %ehn = getelementptr i8, ptr %e, i64 32
  store i64 %n, ptr %ehn
  %ip = call i1 @nz_tls_is_ip(ptr %h, i64 %n)
  br i1 %ip, label %check, label %sni
sni:
  ; SSL_CTRL_SET_TLSEXT_HOSTNAME, TLSEXT_NAMETYPE_host_name.
  %sn = call i64 @SSL_ctrl(ptr %s, i32 55, i64 0, ptr %hc)
  br label %check
check:
  call void @nz_tls_name(ptr %s, ptr %hc)
  call void @SSL_set_connect_state(ptr %s)
  br label %ok
accept:
  call void @SSL_set_accept_state(ptr %s)
  br label %ok
ok:
  call void @ERR_clear_error()
  ret ptr %e
nomem2:
  %z2 = call i64 @nz_tls_close(ptr %e, ptr null, i64 0)
  br label %nomem
nomem1:
  call void @free(ptr %e)
  br label %nomem
nomem:
  call void @ERR_clear_error()
  store i64 -12, ptr %st
  ret ptr null
invalid:
  store i64 -22, ptr %st
  ret ptr null
}

; What a failed call of the engine means: the wait for more ciphertext; 0, the peer's close; a
; verdict on the peer's certificate; or a protocol failure, its text kept for `os_tls_info`.
define hidden i64 @nz_tls_failed(ptr %e, i32 %r) {
entry:
  %s = load ptr, ptr %e
  %err = call i32 @SSL_get_error(ptr %s, i32 %r)
  %want = icmp eq i32 %err, 2
  br i1 %want, label %more, label %zero
more:
  ret i64 -65600
zero:
  %closed = icmp eq i32 %err, 6
  br i1 %closed, label %end, label %verdict
end:
  ret i64 0
verdict:
  %v = call i64 @nz_tls_verdict(ptr %e)
  %some = icmp ne i64 %v, 0
  br i1 %some, label %refused, label %protocol
refused:
  ret i64 %v
protocol:
  %dp = getelementptr i8, ptr %e, i64 56
  %d0 = load i8, ptr %dp
  %said = icmp ne i8 %d0, 0
  br i1 %said, label %out, label %text
text:
  %q = call i64 @ERR_peek_last_error()
  %nq = icmp eq i64 %q, 0
  br i1 %nq, label %out, label %write
write:
  call void @ERR_error_string_n(i64 %q, ptr %dp, i64 200)
  br label %out
out:
  ret i64 -65604
}

define hidden i64 @nz_tls_handshake(ptr %e) {
entry:
  call void @ERR_clear_error()
  %s = load ptr, ptr %e
  %r = call i32 @SSL_do_handshake(ptr %s)
  %ok = icmp eq i32 %r, 1
  br i1 %ok, label %done, label %failed
done:
  ret i64 1
failed:
  %f = call i64 @nz_tls_failed(ptr %e, i32 %r)
  ; A handshake the peer ended is not finished: a protocol failure, not the end of the data.
  %z = icmp eq i64 %f, 0
  %g = select i1 %z, i64 -65604, i64 %f
  ret i64 %g
}

; At most 2^30 bytes at a time: the engine counts in C's `int`.
define hidden i32 @nz_tls_clamp(i64 %n) {
entry:
  %big = icmp sgt i64 %n, 1073741824
  %m = select i1 %big, i64 1073741824, i64 %n
  %r = trunc i64 %m to i32
  ret i32 %r
}

define hidden i64 @nz_tls_push(ptr %e, ptr %p, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %zero, label %go
zero:
  ret i64 0
go:
  %s = load ptr, ptr %e
  %b = call ptr @SSL_get_rbio(ptr %s)
  br label %loop
loop:
  %at = phi i64 [ 0, %go ], [ %at1, %wrote ]
  %left = sub i64 %n, %at
  %done = icmp eq i64 %left, 0
  br i1 %done, label %all, label %one
one:
  %c = call i32 @nz_tls_clamp(i64 %left)
  %q = getelementptr i8, ptr %p, i64 %at
  %w = call i32 @BIO_write(ptr %b, ptr %q, i32 %c)
  %ok = icmp sgt i32 %w, 0
  br i1 %ok, label %wrote, label %nomem
wrote:
  %w64 = sext i32 %w to i64
  %at1 = add i64 %at, %w64
  br label %loop
all:
  ret i64 %n
nomem:
  call void @ERR_clear_error()
  ret i64 -12
}

define hidden i64 @nz_tls_pull(ptr %e, ptr %p, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %zero, label %go
go:
  %s = load ptr, ptr %e
  %b = call ptr @SSL_get_wbio(ptr %s)
  %c = call i32 @nz_tls_clamp(i64 %n)
  %r = call i32 @BIO_read(ptr %b, ptr %p, i32 %c)
  %some = icmp sgt i32 %r, 0
  br i1 %some, label %got, label %zero
got:
  %r64 = sext i32 %r to i64
  ret i64 %r64
zero:
  call void @ERR_clear_error()
  ret i64 0
}

define hidden i64 @nz_tls_read(ptr %e, ptr %p, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %invalid, label %go
invalid:
  ret i64 -22
go:
  call void @ERR_clear_error()
  %s = load ptr, ptr %e
  %c = call i32 @nz_tls_clamp(i64 %n)
  %r = call i32 @SSL_read(ptr %s, ptr %p, i32 %c)
  %some = icmp sgt i32 %r, 0
  br i1 %some, label %got, label %failed
got:
  %r64 = sext i32 %r to i64
  ret i64 %r64
failed:
  %f = call i64 @nz_tls_failed(ptr %e, i32 %r)
  ret i64 %f
}

define hidden i64 @nz_tls_write(ptr %e, ptr %p, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %zero, label %go
zero:
  ret i64 0
go:
  call void @ERR_clear_error()
  %s = load ptr, ptr %e
  %c = call i32 @nz_tls_clamp(i64 %n)
  %r = call i32 @SSL_write(ptr %s, ptr %p, i32 %c)
  %some = icmp sgt i32 %r, 0
  br i1 %some, label %got, label %failed
got:
  %r64 = sext i32 %r to i64
  ret i64 %r64
failed:
  %f = call i64 @nz_tls_failed(ptr %e, i32 %r)
  ; A session the peer closed takes nothing more.
  %z = icmp eq i64 %f, 0
  %g = select i1 %z, i64 -65536, i64 %f
  ret i64 %g
}

; The end of session `e`: its close_notify, once the handshake is done, into `p`, then everything
; it holds freed. A null `p` frees without writing.
define hidden i64 @nz_tls_close(ptr %e, ptr %p, i64 %n) {
entry:
  call void @ERR_clear_error()
  %s = load ptr, ptr %e
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %rest, label %bye
bye:
  %fin = call i32 @SSL_is_init_finished(ptr %s)
  %up = icmp ne i32 %fin, 0
  %np = icmp eq ptr %p, null
  %write = xor i1 %np, true
  %both = and i1 %up, %write
  br i1 %both, label %notify, label %drop
notify:
  %sd = call i32 @SSL_shutdown(ptr %s)
  %r = call i64 @nz_tls_pull(ptr %e, ptr %p, i64 %n)
  br label %drop
drop:
  %out = phi i64 [ 0, %bye ], [ %r, %notify ]
  call void @SSL_free(ptr %s)
  br label %rest
rest:
  %res = phi i64 [ 0, %entry ], [ %out, %drop ]
  %anc = getelementptr i8, ptr %e, i64 8
  call void @nz_tls_unanchor(ptr %anc)
  %hp = getelementptr i8, ptr %e, i64 24
  %h = load ptr, ptr %hp
  call void @free(ptr %h)
  %ap = getelementptr i8, ptr %e, i64 40
  %a = load ptr, ptr %ap
  call void @free(ptr %a)
  call void @free(ptr %e)
  call void @ERR_clear_error()
  ret i64 %res
}

; `n` bytes of `src` into `p`, at most `cap`: how many.
define hidden i64 @nz_tls_give(ptr %src, i64 %n, ptr %p, i64 %cap) {
entry:
  %fits = icmp slt i64 %n, %cap
  %m = select i1 %fits, i64 %n, i64 %cap
  %pos = icmp sgt i64 %m, 0
  br i1 %pos, label %copy, label %none
copy:
  call void @llvm.memcpy.p0.p0.i64(ptr %p, ptr %src, i64 %m, i1 false)
  ret i64 %m
none:
  ret i64 0
}

; A fact about session `e`, into `p`: 0 the ALPN name agreed, 1 the protocol's version, 2 the
; peer certificate's subject, 3 why the last call failed. Its length; empty when there is none.
define hidden i64 @nz_tls_info(ptr %e, i64 %what, ptr %p, i64 %cap) {
entry:
  %s = load ptr, ptr %e
  switch i64 %what, label %invalid [ i64 0, label %alpn
                                     i64 1, label %version
                                     i64 2, label %peer
                                     i64 3, label %detail ]
alpn:
  %ap = alloca ptr, align 8
  %al = alloca i32, align 4
  store ptr null, ptr %ap
  store i32 0, ptr %al
  call void @SSL_get0_alpn_selected(ptr %s, ptr %ap, ptr %al)
  %a = load ptr, ptr %ap
  %l = load i32, ptr %al
  %l64 = zext i32 %l to i64
  %na = icmp eq ptr %a, null
  br i1 %na, label %empty, label %givea
givea:
  %ga = call i64 @nz_tls_give(ptr %a, i64 %l64, ptr %p, i64 %cap)
  ret i64 %ga
version:
  %v = call ptr @SSL_get_version(ptr %s)
  %vl = call i64 @strlen(ptr %v)
  %gv = call i64 @nz_tls_give(ptr %v, i64 %vl, ptr %p, i64 %cap)
  ret i64 %gv
peer:
  %x = call ptr @SSL_get1_peer_certificate(ptr %s)
  %nx = icmp eq ptr %x, null
  br i1 %nx, label %empty, label %name
name:
  %buf = alloca [256 x i8], align 1
  %nm = call ptr @X509_get_subject_name(ptr %x)
  %t = call ptr @X509_NAME_oneline(ptr %nm, ptr %buf, i32 256)
  call void @X509_free(ptr %x)
  %nt = icmp eq ptr %t, null
  br i1 %nt, label %empty, label %givep
givep:
  %tl = call i64 @strlen(ptr %buf)
  %gp = call i64 @nz_tls_give(ptr %buf, i64 %tl, ptr %p, i64 %cap)
  ret i64 %gp
detail:
  %dp = getelementptr i8, ptr %e, i64 56
  %dl = call i64 @strlen(ptr %dp)
  %gd = call i64 @nz_tls_give(ptr %dp, i64 %dl, ptr %p, i64 %cap)
  ret i64 %gd
empty:
  ret i64 0
invalid:
  ret i64 -22
}
"#;

/// Trust on macOS: the Security framework decides, by `SecTrustEvaluateWithError` over the chain
/// the server sent, with an SSL policy for the host — against the system's store, or only the
/// anchors the program named. Never a network fetch: a chain the server did not complete fails
/// here as it would under OpenSSL's own verification.
const SECTRUST: &str = r#"
@kCFTypeArrayCallBacks = external global i8
declare ptr @CFArrayCreateMutable(ptr, i64, ptr)
declare void @CFArrayAppendValue(ptr, ptr)
declare i64 @CFArrayGetCount(ptr)
declare ptr @CFDataCreate(ptr, ptr, i64)
declare ptr @CFStringCreateWithBytes(ptr, ptr, i64, i32, i8 zeroext)
declare void @CFRelease(ptr)
declare ptr @CFRetain(ptr)
declare i64 @CFErrorGetCode(ptr)
declare ptr @CFErrorCopyDescription(ptr)
declare zeroext i8 @CFStringGetCString(ptr, ptr, i64, i32)
declare ptr @SecCertificateCreateWithData(ptr, ptr)
declare ptr @SecPolicyCreateSSL(i8 zeroext, ptr)
declare i32 @SecTrustCreateWithCertificates(ptr, ptr, ptr)
declare i32 @SecTrustSetAnchorCertificates(ptr, ptr)
declare i32 @SecTrustSetAnchorCertificatesOnly(ptr, i8 zeroext)
declare i32 @SecTrustSetNetworkFetchAllowed(ptr, i8 zeroext)
declare zeroext i1 @SecTrustEvaluateWithError(ptr, ptr)
declare void @SSL_CTX_set_cert_verify_callback(ptr, ptr, ptr)
declare i32 @SSL_get_ex_data_X509_STORE_CTX_idx()
declare ptr @X509_STORE_CTX_get_ex_data(ptr, i32)
declare ptr @X509_STORE_CTX_get0_untrusted(ptr)
declare i32 @OPENSSL_sk_num(ptr)
declare ptr @OPENSSL_sk_value(ptr, i32)
declare i32 @i2d_X509(ptr, ptr)

; A certificate as the Security framework holds it, from its DER encoding; null if refused.
define hidden ptr @nz_tls_sec(ptr %x) {
entry:
  %n = call i32 @i2d_X509(ptr %x, ptr null)
  %pos = icmp sgt i32 %n, 0
  br i1 %pos, label %der, label %none
der:
  %n64 = sext i32 %n to i64
  %b = call ptr @malloc(i64 %n64)
  %nb = icmp eq ptr %b, null
  br i1 %nb, label %none, label %encode
encode:
  %q = alloca ptr, align 8
  store ptr %b, ptr %q
  %w = call i32 @i2d_X509(ptr %x, ptr %q)
  %d = call ptr @CFDataCreate(ptr null, ptr %b, i64 %n64)
  call void @free(ptr %b)
  %nd = icmp eq ptr %d, null
  br i1 %nd, label %none, label %cert
cert:
  %c = call ptr @SecCertificateCreateWithData(ptr null, ptr %d)
  call void @CFRelease(ptr %d)
  ret ptr %c
none:
  ret ptr null
}

; The verdict OpenSSL asks for: the chain the server sent, judged by SecTrust for the session's
; host. A refusal records its kind and the framework's sentence in the session.
define hidden i32 @nz_tls_verify(ptr %xs, ptr %arg) {
entry:
  %idx = call i32 @SSL_get_ex_data_X509_STORE_CTX_idx()
  %ssl = call ptr @X509_STORE_CTX_get_ex_data(ptr %xs, i32 %idx)
  %e = call ptr @SSL_get_ex_data(ptr %ssl, i32 0)
  %fp = getelementptr i8, ptr %e, i64 16
  store i64 -65601, ptr %fp
  %chain = call ptr @X509_STORE_CTX_get0_untrusted(ptr %xs)
  %nch = icmp eq ptr %chain, null
  br i1 %nch, label %refuse, label %count
count:
  %n = call i32 @OPENSSL_sk_num(ptr %chain)
  %certs = call ptr @CFArrayCreateMutable(ptr null, i64 0, ptr @kCFTypeArrayCallBacks)
  br label %each
each:
  %i = phi i32 [ 0, %count ], [ %i1, %added ]
  %more = icmp slt i32 %i, %n
  br i1 %more, label %one, label %judge
one:
  %x = call ptr @OPENSSL_sk_value(ptr %chain, i32 %i)
  %c = call ptr @nz_tls_sec(ptr %x)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %drop, label %add
add:
  call void @CFArrayAppendValue(ptr %certs, ptr %c)
  call void @CFRelease(ptr %c)
  br label %added
added:
  %i1 = add i32 %i, 1
  br label %each
judge:
  %some = icmp sgt i32 %n, 0
  br i1 %some, label %policy, label %drop
policy:
  %hp = getelementptr i8, ptr %e, i64 24
  %h = load ptr, ptr %hp
  %hnp = getelementptr i8, ptr %e, i64 32
  %hn = load i64, ptr %hnp
  ; kCFStringEncodingUTF8.
  %hs = call ptr @CFStringCreateWithBytes(ptr null, ptr %h, i64 %hn, i32 134217984, i8 zeroext 0)
  %pol = call ptr @SecPolicyCreateSSL(i8 zeroext 1, ptr %hs)
  %tp = alloca ptr, align 8
  store ptr null, ptr %tp
  %made = call i32 @SecTrustCreateWithCertificates(ptr %certs, ptr %pol, ptr %tp)
  %t = load ptr, ptr %tp
  %mok = icmp eq i32 %made, 0
  br i1 %mok, label %anchored, label %release
anchored:
  %ap = getelementptr i8, ptr %e, i64 8
  %a = load ptr, ptr %ap
  %na = icmp eq ptr %a, null
  br i1 %na, label %evaluate, label %only
only:
  %s1 = call i32 @SecTrustSetAnchorCertificates(ptr %t, ptr %a)
  %s2 = call i32 @SecTrustSetAnchorCertificatesOnly(ptr %t, i8 zeroext 1)
  br label %evaluate
evaluate:
  %s3 = call i32 @SecTrustSetNetworkFetchAllowed(ptr %t, i8 zeroext 0)
  %ep = alloca ptr, align 8
  store ptr null, ptr %ep
  %ok = call i1 @SecTrustEvaluateWithError(ptr %t, ptr %ep)
  br i1 %ok, label %trusted, label %why
trusted:
  store i64 0, ptr %fp
  br label %freet
why:
  %err = load ptr, ptr %ep
  %nerr = icmp eq ptr %err, null
  br i1 %nerr, label %freet, label %code
code:
  %k = call i64 @CFErrorGetCode(ptr %err)
  ; errSecCertificateExpired, errSecCertificateNotValidYet: a certificate out of its dates.
  %exp = icmp eq i64 %k, -67818
  %young = icmp eq i64 %k, -67819
  %dated = or i1 %exp, %young
  ; errSecHostNameMismatch: valid, but for another name.
  %host = icmp eq i64 %k, -67602
  %v1 = select i1 %host, i64 -65603, i64 -65601
  %v = select i1 %dated, i64 -65602, i64 %v1
  store i64 %v, ptr %fp
  %desc = call ptr @CFErrorCopyDescription(ptr %err)
  %nd = icmp eq ptr %desc, null
  br i1 %nd, label %freee, label %text
text:
  %dp = getelementptr i8, ptr %e, i64 56
  %gs = call i8 @CFStringGetCString(ptr %desc, ptr %dp, i64 200, i32 134217984)
  call void @CFRelease(ptr %desc)
  br label %freee
freee:
  call void @CFRelease(ptr %err)
  br label %freet
freet:
  call void @CFRelease(ptr %t)
  br label %release
release:
  call void @CFRelease(ptr %pol)
  call void @CFRelease(ptr %hs)
  br label %drop
drop:
  call void @CFRelease(ptr %certs)
  br label %refuse
refuse:
  %f = load i64, ptr %fp
  %good = icmp eq i64 %f, 0
  %r = zext i1 %good to i32
  ret i32 %r
}

; A client's trust: SecTrust as the verifier, and the anchors `a[0..n]` names, if any, kept in
; the configuration. 0, or 1 when `a` holds no certificate.
define hidden i32 @nz_tls_trust(ptr %c, ptr %x, ptr %a, i64 %n) {
entry:
  call void @SSL_CTX_set_cert_verify_callback(ptr %x, ptr @nz_tls_verify, ptr null)
  %none = icmp slt i64 %n, 1
  br i1 %none, label %ok, label %read
read:
  %arr = call ptr @CFArrayCreateMutable(ptr null, i64 0, ptr @kCFTypeArrayCallBacks)
  %anp = getelementptr i8, ptr %c, i64 16
  store ptr %arr, ptr %anp
  %n32 = trunc i64 %n to i32
  %b = call ptr @BIO_new_mem_buf(ptr %a, i32 %n32)
  br label %each
each:
  %x509 = call ptr @PEM_read_bio_X509(ptr %b, ptr null, ptr @nz_tls_nopass, ptr null)
  %end = icmp eq ptr %x509, null
  br i1 %end, label %counted, label %one
one:
  %s = call ptr @nz_tls_sec(ptr %x509)
  call void @X509_free(ptr %x509)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %each, label %keep
keep:
  call void @CFArrayAppendValue(ptr %arr, ptr %s)
  call void @CFRelease(ptr %s)
  br label %each
counted:
  %f = call i32 @BIO_free(ptr %b)
  call void @ERR_clear_error()
  %k = call i64 @CFArrayGetCount(ptr %arr)
  %empty = icmp eq i64 %k, 0
  br i1 %empty, label %bad, label %ok
ok:
  ret i32 0
bad:
  ret i32 1
}

; The session's own reference to its configuration's anchors (`to` from `from`).
define hidden void @nz_tls_anchor(ptr %to, ptr %from) {
entry:
  %a = load ptr, ptr %from
  %na = icmp eq ptr %a, null
  br i1 %na, label %done, label %keep
keep:
  %r = call ptr @CFRetain(ptr %a)
  store ptr %a, ptr %to
  br label %done
done:
  ret void
}

define hidden void @nz_tls_unanchor(ptr %at) {
entry:
  %a = load ptr, ptr %at
  %na = icmp eq ptr %a, null
  br i1 %na, label %done, label %drop
drop:
  call void @CFRelease(ptr %a)
  store ptr null, ptr %at
  br label %done
done:
  ret void
}

; SecTrust names the host itself.
define hidden void @nz_tls_name(ptr %s, ptr %h) {
entry:
  ret void
}

; The verifier's verdict, recorded in the session: 0 when it refused nothing.
define hidden i64 @nz_tls_verdict(ptr %e) {
entry:
  %fp = getelementptr i8, ptr %e, i64 16
  %f = load i64, ptr %fp
  ret i64 %f
}
"#;

/// Trust on Linux: OpenSSL verifies against the system's bundle or only the named anchors, and
/// the host by `SSL_set1_host`.
const OPENSSL_TRUST: &str = r#"
declare i32 @SSL_CTX_set_default_verify_paths(ptr)
declare ptr @SSL_CTX_get_cert_store(ptr)
declare i32 @X509_STORE_add_cert(ptr, ptr)
declare i32 @SSL_set1_host(ptr, ptr)
declare i64 @SSL_get_verify_result(ptr)
declare ptr @X509_verify_cert_error_string(i64)

define hidden i32 @nz_tls_trust(ptr %c, ptr %x, ptr %a, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %system, label %read
system:
  %d = call i32 @SSL_CTX_set_default_verify_paths(ptr %x)
  ret i32 0
read:
  %store = call ptr @SSL_CTX_get_cert_store(ptr %x)
  %n32 = trunc i64 %n to i32
  %b = call ptr @BIO_new_mem_buf(ptr %a, i32 %n32)
  br label %each
each:
  %k = phi i64 [ 0, %read ], [ %k1, %one ]
  %x509 = call ptr @PEM_read_bio_X509(ptr %b, ptr null, ptr @nz_tls_nopass, ptr null)
  %end = icmp eq ptr %x509, null
  br i1 %end, label %counted, label %one
one:
  %r = call i32 @X509_STORE_add_cert(ptr %store, ptr %x509)
  call void @X509_free(ptr %x509)
  %k1 = add i64 %k, 1
  br label %each
counted:
  %f = call i32 @BIO_free(ptr %b)
  call void @ERR_clear_error()
  %empty = icmp eq i64 %k, 0
  %bad = zext i1 %empty to i32
  ret i32 %bad
}

define hidden void @nz_tls_anchor(ptr %to, ptr %from) {
entry:
  ret void
}

define hidden void @nz_tls_unanchor(ptr %at) {
entry:
  ret void
}

define hidden void @nz_tls_name(ptr %s, ptr %h) {
entry:
  %r = call i32 @SSL_set1_host(ptr %s, ptr %h)
  ret void
}

; OpenSSL's verdict: X509_V_ERR_CERT_NOT_YET_VALID (9) and _HAS_EXPIRED (10) are dates,
; _HOSTNAME_MISMATCH (62) and _IP_ADDRESS_MISMATCH (64) the name, any other an untrusted chain.
define hidden i64 @nz_tls_verdict(ptr %e) {
entry:
  %s = load ptr, ptr %e
  %v = call i64 @SSL_get_verify_result(ptr %s)
  %ok = icmp eq i64 %v, 0
  br i1 %ok, label %none, label %why
none:
  ret i64 0
why:
  %t = call ptr @X509_verify_cert_error_string(i64 %v)
  %tl = call i64 @strlen(ptr %t)
  %dp = getelementptr i8, ptr %e, i64 56
  %w = call i64 @nz_tls_give(ptr %t, i64 %tl, ptr %dp, i64 199)
  %young = icmp eq i64 %v, 9
  %old = icmp eq i64 %v, 10
  %dated = or i1 %young, %old
  %h1 = icmp eq i64 %v, 62
  %h2 = icmp eq i64 %v, 64
  %host = or i1 %h1, %h2
  %k1 = select i1 %host, i64 -65603, i64 -65601
  %k = select i1 %dated, i64 -65602, i64 %k1
  ret i64 %k
}
"#;

const SCHANNEL: &str = r#"
; The TLS engine (C1b): SChannel through SSPI, buffer in and buffer out. Generated by nazm build.
; Do not edit. A configuration is { refs, CredHandle[16], kind, anchors store, certificate, key
; name, ALPN buffer, its size, key handle, has credentials } (96 bytes); a session is
; { configuration, CtxtHandle[16], state, in[ptr,len,cap], out[ptr,len,cap],
; plain[ptr,start,len,cap], stream sizes[20], host, host length, failure, detail[96] } (256 bytes).
; State: 0 nothing sent, 1 handshaking, 2 established, 3 failed, 4 the peer closed.
declare ptr @calloc(i64, i64)
declare ptr @malloc(i64)
declare ptr @realloc(ptr, i64)
declare void @free(ptr)
declare i64 @strlen(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare i32 @AcquireCredentialsHandleA(ptr, ptr, i32, ptr, ptr, ptr, ptr, ptr, ptr)
declare i32 @FreeCredentialsHandle(ptr)
declare i32 @InitializeSecurityContextA(ptr, ptr, ptr, i32, i32, i32, ptr, i32, ptr, ptr, ptr, ptr)
declare i32 @AcceptSecurityContext(ptr, ptr, ptr, i32, i32, ptr, ptr, ptr, ptr)
declare i32 @DeleteSecurityContext(ptr)
declare i32 @FreeContextBuffer(ptr)
declare i32 @QueryContextAttributesA(ptr, i32, ptr)
declare i32 @DecryptMessage(ptr, ptr, i32, ptr)
declare i32 @EncryptMessage(ptr, i32, ptr, i32)
declare i32 @ApplyControlToken(ptr, ptr)
declare ptr @CertOpenStore(ptr, i32, i64, i32, ptr)
declare i32 @CertCloseStore(ptr, i32)
declare i32 @CertAddEncodedCertificateToStore(ptr, i32, ptr, i32, i32, ptr)
declare ptr @CertCreateCertificateContext(i32, ptr, i32)
declare i32 @CertFreeCertificateContext(ptr)
declare i32 @CertSetCertificateContextProperty(ptr, i32, i32, ptr)
declare i32 @CertCreateCertificateChainEngine(ptr, ptr)
declare void @CertFreeCertificateChainEngine(ptr)
declare i32 @CertGetCertificateChain(ptr, ptr, ptr, ptr, ptr, i32, ptr, ptr)
declare void @CertFreeCertificateChain(ptr)
declare i32 @CertVerifyCertificateChainPolicy(ptr, ptr, ptr, ptr)
declare i32 @CertGetNameStringA(ptr, i32, i32, ptr, ptr, i32)
declare i32 @CryptStringToBinaryA(ptr, i32, i32, ptr, ptr, ptr, ptr)
declare i32 @NCryptOpenStorageProvider(ptr, ptr, i32)
declare i32 @NCryptImportKey(i64, i64, ptr, ptr, ptr, ptr, i32, i32)
declare i32 @NCryptDeleteKey(i64, i32)
declare i32 @NCryptFreeObject(i64)
declare i32 @MultiByteToWideChar(i32, i32, ptr, i32, ptr, i32)
declare i32 @GetCurrentProcessId()

; UNISP_NAME_A, the OIDs of server authentication and of a common name.
@nz_tls_unisp = private unnamed_addr constant [45 x i8] c"Microsoft Unified Security Protocol Provider\00"
@nz_tls_server_auth = private unnamed_addr constant [18 x i8] c"1.3.6.1.5.5.7.3.1\00"
@nz_tls_cn = private unnamed_addr constant [8 x i8] c"2.5.4.3\00"
; MS_KEY_STORAGE_PROVIDER and NCRYPT_PKCS8_PRIVATE_KEY_BLOB, as UTF-16.
@nz_tls_ksp = private unnamed_addr constant [40 x i16] [i16 77, i16 105, i16 99, i16 114, i16 111, i16 115, i16 111, i16 102, i16 116, i16 32, i16 83, i16 111, i16 102, i16 116, i16 119, i16 97, i16 114, i16 101, i16 32, i16 75, i16 101, i16 121, i16 32, i16 83, i16 116, i16 111, i16 114, i16 97, i16 103, i16 101, i16 32, i16 80, i16 114, i16 111, i16 118, i16 105, i16 100, i16 101, i16 114, i16 0], align 2
@nz_tls_pkcs8 = private unnamed_addr constant [17 x i16] [i16 80, i16 75, i16 67, i16 83, i16 56, i16 95, i16 80, i16 82, i16 73, i16 86, i16 65, i16 84, i16 69, i16 75, i16 69, i16 89, i16 0], align 2
@nz_tls_begin = private unnamed_addr constant [11 x i8] c"-----BEGIN "
@nz_tls_end = private unnamed_addr constant [8 x i8] c"-----END"
@nz_tls_dashes = private unnamed_addr constant [5 x i8] c"-----"
@nz_tls_certificate = private unnamed_addr constant [16 x i8] c"CERTIFICATE-----"
@nz_tls_private_key = private unnamed_addr constant [16 x i8] c"PRIVATE KEY-----"
@nz_tls_v12 = private unnamed_addr constant [8 x i8] c"TLSv1.2\00"
@nz_tls_v13 = private unnamed_addr constant [8 x i8] c"TLSv1.3\00"
@nz_tls_vtls = private unnamed_addr constant [4 x i8] c"TLS\00"
@nz_tls_slash_cn = private unnamed_addr constant [4 x i8] c"/CN="
@nz_tls_why_untrusted = private unnamed_addr constant [40 x i8] c"the certificate chain is not trusted\00\00\00\00"
@nz_tls_why_expired = private unnamed_addr constant [40 x i8] c"a certificate is outside its dates\00\00\00\00\00\00"
@nz_tls_why_host = private unnamed_addr constant [40 x i8] c"the certificate is for another name\00\00\00\00\00"
@nz_tls_why_protocol = private unnamed_addr constant [40 x i8] c"SChannel refused the TLS exchange\00\00\00\00\00\00\00"
@nz_tls_keys = hidden global i64 0

; Whether `p[0..n]` is ALPN's wire form (as the OpenSSL unit's).
define hidden i1 @nz_tls_alpn_ok(ptr %p, i64 %n) {
entry:
  %some = icmp sgt i64 %n, 0
  br i1 %some, label %loop, label %no
loop:
  %at = phi i64 [ 0, %entry ], [ %next, %more ]
  %lp = getelementptr i8, ptr %p, i64 %at
  %lb = load i8, ptr %lp
  %l = zext i8 %lb to i64
  %empty = icmp eq i64 %l, 0
  br i1 %empty, label %no, label %step
step:
  %after = add i64 %at, 1
  %next = add i64 %after, %l
  %over = icmp sgt i64 %next, %n
  br i1 %over, label %no, label %fits
fits:
  %end = icmp eq i64 %next, %n
  br i1 %end, label %yes, label %more
more:
  br label %loop
yes:
  ret i1 true
no:
  ret i1 false
}

; A copy of `p[0..n]`, NUL-terminated; null when memory is refused.
define hidden ptr @nz_tls_copy(ptr %p, i64 %n) {
entry:
  %m = add i64 %n, 1
  %c = call ptr @calloc(i64 %m, i64 1)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %out, label %fill
fill:
  call void @llvm.memcpy.p0.p0.i64(ptr %c, ptr %p, i64 %n, i1 false)
  br label %out
out:
  ret ptr %c
}

; `n` bytes of `src` into `p`, at most `cap`: how many.
define hidden i64 @nz_tls_give(ptr %src, i64 %n, ptr %p, i64 %cap) {
entry:
  %fits = icmp slt i64 %n, %cap
  %m = select i1 %fits, i64 %n, i64 %cap
  %pos = icmp sgt i64 %m, 0
  br i1 %pos, label %copy, label %none
copy:
  call void @llvm.memcpy.p0.p0.i64(ptr %p, ptr %src, i64 %m, i1 false)
  ret i64 %m
none:
  ret i64 0
}

; `d[0..n]` appended to the growable buffer `{ ptr, len, cap }` at `b`: false when memory is refused.
define hidden i1 @nz_tls_append(ptr %b, ptr %d, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %ok, label %go
go:
  %lp = getelementptr i8, ptr %b, i64 8
  %cp = getelementptr i8, ptr %b, i64 16
  %len = load i64, ptr %lp
  %cap = load i64, ptr %cp
  %need = add i64 %len, %n
  %fits = icmp sle i64 %need, %cap
  br i1 %fits, label %copy, label %grow
grow:
  %twice = shl i64 %cap, 1
  %more = icmp sgt i64 %twice, %need
  %c0 = select i1 %more, i64 %twice, i64 %need
  %small = icmp slt i64 %c0, 4096
  %c1 = select i1 %small, i64 4096, i64 %c0
  %old = load ptr, ptr %b
  %new = call ptr @realloc(ptr %old, i64 %c1)
  %nn = icmp eq ptr %new, null
  br i1 %nn, label %fail, label %keep
keep:
  store ptr %new, ptr %b
  store i64 %c1, ptr %cp
  br label %copy
copy:
  %base = load ptr, ptr %b
  %at = getelementptr i8, ptr %base, i64 %len
  call void @llvm.memcpy.p0.p0.i64(ptr %at, ptr %d, i64 %n, i1 false)
  store i64 %need, ptr %lp
  br label %ok
ok:
  ret i1 true
fail:
  ret i1 false
}

; The first `n` bytes of the buffer at `b` taken away; what follows moves to its start.
define hidden void @nz_tls_drop(ptr %b, i64 %n) {
entry:
  %lp = getelementptr i8, ptr %b, i64 8
  %len = load i64, ptr %lp
  %rest = sub i64 %len, %n
  %base = load ptr, ptr %b
  %from = getelementptr i8, ptr %base, i64 %n
  %some = icmp sgt i64 %rest, 0
  br i1 %some, label %move, label %done
move:
  call void @llvm.memmove.p0.p0.i64(ptr %base, ptr %from, i64 %rest, i1 false)
  br label %done
done:
  %left = select i1 %some, i64 %rest, i64 0
  store i64 %left, ptr %lp
  ret void
}

; Where `needle[0..m]` first is in `h[from..n]`, or -1.
define hidden i64 @nz_tls_find(ptr %h, i64 %n, i64 %from, ptr %needle, i64 %m) {
entry:
  br label %loop
loop:
  %i = phi i64 [ %from, %entry ], [ %i1, %next ]
  %end = add i64 %i, %m
  %past = icmp sgt i64 %end, %n
  br i1 %past, label %none, label %look
look:
  %at = getelementptr i8, ptr %h, i64 %i
  %c = call i32 @memcmp(ptr %at, ptr %needle, i64 %m)
  %same = icmp eq i32 %c, 0
  br i1 %same, label %found, label %next
next:
  %i1 = add i64 %i, 1
  br label %loop
found:
  ret i64 %i
none:
  ret i64 -1
}
declare i32 @memcmp(ptr, ptr, i64)

; The next PEM block labelled `label` (16 bytes, `NAME-----`) in `p[*at..n]`, as DER in memory of its
; own (its length into `len`), `*at` moved past it; null when there is none or it does not decode.
define hidden ptr @nz_tls_pem(ptr %p, i64 %n, ptr %atp, ptr %label, ptr %len) {
entry:
  %size = alloca i32, align 4
  %from = load i64, ptr %atp
  br label %seek
seek:
  %f = phi i64 [ %from, %entry ], [ %past, %other ]
  %b = call i64 @nz_tls_find(ptr %p, i64 %n, i64 %f, ptr @nz_tls_begin, i64 11)
  %nob = icmp slt i64 %b, 0
  br i1 %nob, label %none, label %label_at
label_at:
  %lab = add i64 %b, 11
  %labend = add i64 %lab, 16
  %short = icmp sgt i64 %labend, %n
  br i1 %short, label %none, label %compare
compare:
  %lp = getelementptr i8, ptr %p, i64 %lab
  %c = call i32 @memcmp(ptr %lp, ptr %label, i64 16)
  %e = call i64 @nz_tls_find(ptr %p, i64 %n, i64 %labend, ptr @nz_tls_end, i64 8)
  %noe = icmp slt i64 %e, 0
  br i1 %noe, label %none, label %close
close:
  %e8 = add i64 %e, 8
  %d = call i64 @nz_tls_find(ptr %p, i64 %n, i64 %e8, ptr @nz_tls_dashes, i64 5)
  %nod = icmp slt i64 %d, 0
  br i1 %nod, label %none, label %ends
ends:
  %past = add i64 %d, 5
  store i64 %past, ptr %atp
  %mine = icmp eq i32 %c, 0
  br i1 %mine, label %decode, label %other
other:
  br label %seek
decode:
  %bp = getelementptr i8, ptr %p, i64 %b
  %bl = sub i64 %past, %b
  %bl32 = trunc i64 %bl to i32
  store i32 0, ptr %size
  ; CRYPT_STRING_BASE64HEADER: the block with its BEGIN and END lines.
  %r1 = call i32 @CryptStringToBinaryA(ptr %bp, i32 %bl32, i32 0, ptr null, ptr %size, ptr null, ptr null)
  %s = load i32, ptr %size
  %ok1 = icmp ne i32 %r1, 0
  %pos = icmp ugt i32 %s, 0
  %ok = and i1 %ok1, %pos
  br i1 %ok, label %alloc, label %none
alloc:
  %s64 = zext i32 %s to i64
  %der = call ptr @malloc(i64 %s64)
  %nd = icmp eq ptr %der, null
  br i1 %nd, label %none, label %fill
fill:
  %r2 = call i32 @CryptStringToBinaryA(ptr %bp, i32 %bl32, i32 0, ptr %der, ptr %size, ptr null, ptr null)
  %got = load i32, ptr %size
  %got64 = zext i32 %got to i64
  store i64 %got64, ptr %len
  %ok2 = icmp ne i32 %r2, 0
  br i1 %ok2, label %done, label %bad
bad:
  call void @free(ptr %der)
  br label %none
done:
  ret ptr %der
none:
  ret ptr null
}

; Credentials for configuration `c`: SCHANNEL_CRED for TLS 1.2, a server's with its certificate,
; a client's verified by this unit (SCH_CRED_MANUAL_CRED_VALIDATION, SCH_CRED_NO_DEFAULT_CREDS).
; 0, or SSPI's status.
define hidden i32 @nz_tls_credentials(ptr %c) {
entry:
  %sc = alloca [80 x i8], align 8
  %certs = alloca ptr, align 8
  %expiry = alloca i64, align 8
  call void @llvm.memset.p0.i64(ptr %sc, i8 0, i64 80, i1 false)
  ; SCHANNEL_CRED_VERSION.
  store i32 4, ptr %sc
  %kp = getelementptr i8, ptr %c, i64 24
  %kind = load i64, ptr %kp
  %client = icmp eq i64 %kind, 1
  %protop = getelementptr i8, ptr %sc, i64 56
  %flagsp = getelementptr i8, ptr %sc, i64 72
  br i1 %client, label %outbound, label %inbound
outbound:
  store i32 2048, ptr %protop
  store i32 24, ptr %flagsp
  br label %acquire
inbound:
  store i32 1024, ptr %protop
  %certp = getelementptr i8, ptr %c, i64 40
  %cert = load ptr, ptr %certp
  store ptr %cert, ptr %certs
  %ccp = getelementptr i8, ptr %sc, i64 4
  store i32 1, ptr %ccp
  %pacp = getelementptr i8, ptr %sc, i64 8
  store ptr %certs, ptr %pacp
  br label %acquire
acquire:
  ; SECPKG_CRED_OUTBOUND (2) for a client, SECPKG_CRED_INBOUND (1) for a server.
  %use = select i1 %client, i32 2, i32 1
  %cred = getelementptr i8, ptr %c, i64 8
  %r = call i32 @AcquireCredentialsHandleA(ptr null, ptr @nz_tls_unisp, i32 %use, ptr null, ptr %sc, ptr null, ptr null, ptr %cred, ptr %expiry)
  %ok = icmp eq i32 %r, 0
  br i1 %ok, label %have, label %done
have:
  %hp = getelementptr i8, ptr %c, i64 80
  store i64 1, ptr %hp
  br label %done
done:
  ret i32 %r
}
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

; A server's key, PKCS #8 DER, imported under a name of its own into the user's key storage
; provider — SChannel cannot use a key that is not persisted — and named on its certificate. 0,
; or nonzero when anything is refused. The key is deleted when the configuration goes.
define hidden i32 @nz_tls_key(ptr %c, ptr %der, i64 %n) {
entry:
  %prov = alloca i64, align 8
  %key = alloca i64, align 8
  %buf = alloca [16 x i8], align 8
  %desc = alloca [16 x i8], align 8
  %kpi = alloca [48 x i8], align 8
  store i64 0, ptr %prov
  store i64 0, ptr %key
  ; The key's name: `nazm-tls-`, the process's id and a counter, as UTF-16, 64 units.
  %name = call ptr @calloc(i64 64, i64 2)
  %nn = icmp eq ptr %name, null
  br i1 %nn, label %fail, label %named
named:
  %np = getelementptr i8, ptr %c, i64 48
  store ptr %name, ptr %np
  %pid = call i32 @GetCurrentProcessId()
  %pid64 = zext i32 %pid to i64
  %count = atomicrmw add ptr @nz_tls_keys, i64 1 seq_cst
  %hi = shl i64 %pid64, 24
  %mix = or i64 %hi, %count
  ; `nazm-tls-`
  store i16 110, ptr %name
  %n1 = getelementptr i16, ptr %name, i64 1
  store i16 97, ptr %n1
  %n2 = getelementptr i16, ptr %name, i64 2
  store i16 122, ptr %n2
  %n3 = getelementptr i16, ptr %name, i64 3
  store i16 109, ptr %n3
  %n4 = getelementptr i16, ptr %name, i64 4
  store i16 45, ptr %n4
  %n5 = getelementptr i16, ptr %name, i64 5
  store i16 116, ptr %n5
  %n6 = getelementptr i16, ptr %name, i64 6
  store i16 108, ptr %n6
  %n7 = getelementptr i16, ptr %name, i64 7
  store i16 115, ptr %n7
  %n8 = getelementptr i16, ptr %name, i64 8
  store i16 45, ptr %n8
  br label %hex
hex:
  %i = phi i64 [ 0, %named ], [ %i1, %hex ]
  %shift = sub i64 60, %i
  %nib0 = lshr i64 %mix, %shift
  %nib = and i64 %nib0, 15
  %letter = icmp ugt i64 %nib, 9
  %base = select i1 %letter, i64 87, i64 48
  %ch = add i64 %nib, %base
  %ch16 = trunc i64 %ch to i16
  %slot = add i64 %i, 36
  %pos = udiv i64 %slot, 4
  %cp = getelementptr i16, ptr %name, i64 %pos
  store i16 %ch16, ptr %cp
  %i1 = add i64 %i, 4
  %more = icmp ult i64 %i1, 64
  br i1 %more, label %hex, label %open
open:
  %o = call i32 @NCryptOpenStorageProvider(ptr %prov, ptr @nz_tls_ksp, i32 0)
  %ook = icmp eq i32 %o, 0
  br i1 %ook, label %import, label %fail
import:
  ; NCRYPTBUFFER_PKCS_KEY_NAME: 25 UTF-16 units and their NUL.
  store i32 52, ptr %buf
  %bt = getelementptr i8, ptr %buf, i64 4
  store i32 45, ptr %bt
  %bv = getelementptr i8, ptr %buf, i64 8
  store ptr %name, ptr %bv
  store i32 0, ptr %desc
  %dc = getelementptr i8, ptr %desc, i64 4
  store i32 1, ptr %dc
  %db = getelementptr i8, ptr %desc, i64 8
  store ptr %buf, ptr %db
  %pv = load i64, ptr %prov
  %n32 = trunc i64 %n to i32
  ; NCRYPT_OVERWRITE_KEY_FLAG.
  %im = call i32 @NCryptImportKey(i64 %pv, i64 0, ptr @nz_tls_pkcs8, ptr %desc, ptr %key, ptr %der, i32 %n32, i32 128)
  %fo = call i32 @NCryptFreeObject(i64 %pv)
  %iok = icmp eq i32 %im, 0
  br i1 %iok, label %link, label %fail
link:
  %kv = load i64, ptr %key
  %khp = getelementptr i8, ptr %c, i64 72
  store i64 %kv, ptr %khp
  call void @llvm.memset.p0.i64(ptr %kpi, i8 0, i64 48, i1 false)
  store ptr %name, ptr %kpi
  %kpn = getelementptr i8, ptr %kpi, i64 8
  store ptr @nz_tls_ksp, ptr %kpn
  %certp = getelementptr i8, ptr %c, i64 40
  %cert = load ptr, ptr %certp
  ; CERT_KEY_PROV_INFO_PROP_ID.
  %set = call i32 @CertSetCertificateContextProperty(ptr %cert, i32 2, i32 0, ptr %kpi)
  %sok = icmp ne i32 %set, 0
  br i1 %sok, label %done, label %fail
done:
  ret i32 0
fail:
  ret i32 1
}

; A configuration of `kind` (1 a client's: `a` its anchors, PEM, none for the system's store; 2 a
; server's: `a` its certificate, `b` its key, PKCS #8 PEM), offering the ALPN names `p`. Null with
; the status in `st` when anything is refused.
define hidden ptr @nz_tls_config(i64 %kind, ptr %a, i64 %an, ptr %b, i64 %bn, ptr %p, i64 %pn, ptr %st) {
entry:
  %at = alloca i64, align 8
  %len = alloca i64, align 8
  %has = icmp sgt i64 %pn, 0
  br i1 %has, label %check, label %sizes
check:
  %good = call i1 @nz_tls_alpn_ok(ptr %p, i64 %pn)
  br i1 %good, label %sizes, label %invalid
sizes:
  %big = icmp sgt i64 %an, 1073741824
  %big2 = icmp sgt i64 %bn, 1073741824
  %big3 = icmp sgt i64 %pn, 65535
  %b12 = or i1 %big, %big2
  %toobig = or i1 %b12, %big3
  br i1 %toobig, label %invalid, label %make
make:
  %c = call ptr @calloc(i64 1, i64 96)
  %nc = icmp eq ptr %c, null
  br i1 %nc, label %nomem, label %set
set:
  store i64 1, ptr %c
  %kp = getelementptr i8, ptr %c, i64 24
  store i64 %kind, ptr %kp
  br i1 %has, label %alpn, label %side
alpn:
  ; SEC_APPLICATION_PROTOCOLS: its lists' size, ALPN's extension (2), the list's size, the list.
  %size = add i64 %pn, 10
  %ab = call ptr @calloc(i64 %size, i64 1)
  %nab = icmp eq ptr %ab, null
  br i1 %nab, label %nomem1, label %fill
fill:
  %lists = add i64 %pn, 6
  %l32 = trunc i64 %lists to i32
  store i32 %l32, ptr %ab
  %ext = getelementptr i8, ptr %ab, i64 4
  store i32 2, ptr %ext
  %ls = getelementptr i8, ptr %ab, i64 8
  %pn16 = trunc i64 %pn to i16
  store i16 %pn16, ptr %ls, align 1
  %list = getelementptr i8, ptr %ab, i64 10
  call void @llvm.memcpy.p0.p0.i64(ptr %list, ptr %p, i64 %pn, i1 false)
  %abp = getelementptr i8, ptr %c, i64 56
  store ptr %ab, ptr %abp
  %abn = getelementptr i8, ptr %c, i64 64
  store i64 %size, ptr %abn
  br label %side
side:
  store i64 0, ptr %at
  %client = icmp eq i64 %kind, 1
  br i1 %client, label %anchors, label %identity
anchors:
  %none = icmp slt i64 %an, 1
  br i1 %none, label %credentials, label %store
store:
  ; CERT_STORE_PROV_MEMORY.
  %s = call ptr @CertOpenStore(ptr inttoptr (i64 2 to ptr), i32 0, i64 0, i32 0, ptr null)
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %bad, label %kept
kept:
  %sp = getelementptr i8, ptr %c, i64 32
  store ptr %s, ptr %sp
  br label %each
each:
  %k = phi i64 [ 0, %kept ], [ %k1, %added ]
  %der = call ptr @nz_tls_pem(ptr %a, i64 %an, ptr %at, ptr @nz_tls_certificate, ptr %len)
  %end = icmp eq ptr %der, null
  br i1 %end, label %counted, label %add
add:
  %dl = load i64, ptr %len
  %dl32 = trunc i64 %dl to i32
  ; X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, CERT_STORE_ADD_ALWAYS.
  %r = call i32 @CertAddEncodedCertificateToStore(ptr %s, i32 65537, ptr %der, i32 %dl32, i32 4, ptr null)
  call void @free(ptr %der)
  %one = icmp ne i32 %r, 0
  %inc = zext i1 %one to i64
  %k1 = add i64 %k, %inc
  br label %added
added:
  br label %each
counted:
  %zero = icmp eq i64 %k, 0
  br i1 %zero, label %bad, label %credentials
identity:
  %cder = call ptr @nz_tls_pem(ptr %a, i64 %an, ptr %at, ptr @nz_tls_certificate, ptr %len)
  %ncd = icmp eq ptr %cder, null
  br i1 %ncd, label %bad, label %context
context:
  %cl = load i64, ptr %len
  %cl32 = trunc i64 %cl to i32
  %ctx = call ptr @CertCreateCertificateContext(i32 65537, ptr %cder, i32 %cl32)
  call void @free(ptr %cder)
  %nctx = icmp eq ptr %ctx, null
  br i1 %nctx, label %bad, label %cert
cert:
  %certp = getelementptr i8, ptr %c, i64 40
  store ptr %ctx, ptr %certp
  store i64 0, ptr %at
  %kder = call ptr @nz_tls_pem(ptr %b, i64 %bn, ptr %at, ptr @nz_tls_private_key, ptr %len)
  %nkd = icmp eq ptr %kder, null
  br i1 %nkd, label %bad, label %key
key:
  %kl = load i64, ptr %len
  %kr = call i32 @nz_tls_key(ptr %c, ptr %kder, i64 %kl)
  call void @free(ptr %kder)
  %kok = icmp eq i32 %kr, 0
  br i1 %kok, label %credentials, label %bad
credentials:
  %cr = call i32 @nz_tls_credentials(ptr %c)
  %crok = icmp eq i32 %cr, 0
  br i1 %crok, label %done, label %refused
done:
  ret ptr %c
refused:
  ; A client whose credentials SSPI refuses meets a protocol failure; a server's, a key or
  ; certificate it cannot use.
  call void @nz_tls_config_free(ptr %c)
  %why = select i1 %client, i64 -65604, i64 -65605
  store i64 %why, ptr %st
  ret ptr null
bad:
  call void @nz_tls_config_free(ptr %c)
  store i64 -65605, ptr %st
  ret ptr null
nomem1:
  call void @free(ptr %c)
  br label %nomem
nomem:
  store i64 -12, ptr %st
  ret ptr null
invalid:
  store i64 -22, ptr %st
  ret ptr null
}

; One reference fewer to configuration `c`; the last frees its credentials, store, certificate
; and key (the persisted key deleted).
define hidden void @nz_tls_config_free(ptr %c) {
entry:
  %old = atomicrmw sub ptr %c, i64 1 acq_rel
  %last = icmp eq i64 %old, 1
  br i1 %last, label %free, label %done
free:
  %hp = getelementptr i8, ptr %c, i64 80
  %h = load i64, ptr %hp
  %has = icmp ne i64 %h, 0
  br i1 %has, label %cred, label %store
cred:
  %cr = getelementptr i8, ptr %c, i64 8
  %f = call i32 @FreeCredentialsHandle(ptr %cr)
  br label %store
store:
  %sp = getelementptr i8, ptr %c, i64 32
  %s = load ptr, ptr %sp
  %ns = icmp eq ptr %s, null
  br i1 %ns, label %cert, label %close
close:
  %cs = call i32 @CertCloseStore(ptr %s, i32 0)
  br label %cert
cert:
  %kp = getelementptr i8, ptr %c, i64 72
  %k = load i64, ptr %kp
  %hk = icmp ne i64 %k, 0
  br i1 %hk, label %delete, label %context
delete:
  %dk = call i32 @NCryptDeleteKey(i64 %k, i32 0)
  br label %context
context:
  %ctp = getelementptr i8, ptr %c, i64 40
  %ct = load ptr, ptr %ctp
  %nct = icmp eq ptr %ct, null
  br i1 %nct, label %rest, label %fc
fc:
  %fcc = call i32 @CertFreeCertificateContext(ptr %ct)
  br label %rest
rest:
  %np = getelementptr i8, ptr %c, i64 48
  %n = load ptr, ptr %np
  call void @free(ptr %n)
  %ap = getelementptr i8, ptr %c, i64 56
  %a = load ptr, ptr %ap
  call void @free(ptr %a)
  call void @free(ptr %c)
  br label %done
done:
  ret void
}

; A session of configuration `c`: a client's verifying `h[0..n]`. Null with the status in `st`.
define hidden ptr @nz_tls_session(ptr %c, ptr %h, i64 %n, ptr %st) {
entry:
  %kp = getelementptr i8, ptr %c, i64 24
  %kind = load i64, ptr %kp
  %client = icmp eq i64 %kind, 1
  br i1 %client, label %name, label %make
name:
  %empty = icmp slt i64 %n, 1
  %long = icmp sgt i64 %n, 253
  %bad = or i1 %empty, %long
  br i1 %bad, label %invalid, label %scan
scan:
  %i = phi i64 [ 0, %name ], [ %i1, %step ]
  %done = icmp eq i64 %i, %n
  br i1 %done, label %make, label %look
look:
  %bp = getelementptr i8, ptr %h, i64 %i
  %b = load i8, ptr %bp
  %z = icmp eq i8 %b, 0
  br i1 %z, label %invalid, label %step
step:
  %i1 = add i64 %i, 1
  br label %scan
make:
  %e = call ptr @calloc(i64 1, i64 256)
  %ne = icmp eq ptr %e, null
  br i1 %ne, label %nomem, label %hold
hold:
  %r = atomicrmw add ptr %c, i64 1 acq_rel
  store ptr %c, ptr %e
  br i1 %client, label %host, label %ok
host:
  %hc = call ptr @nz_tls_copy(ptr %h, i64 %n)
  %nhc = icmp eq ptr %hc, null
  br i1 %nhc, label %nomem1, label %named
named:
  %hp = getelementptr i8, ptr %e, i64 136
  store ptr %hc, ptr %hp
  %hn = getelementptr i8, ptr %e, i64 144
  store i64 %n, ptr %hn
  br label %ok
ok:
  ret ptr %e
nomem1:
  %z1 = call i64 @nz_tls_close(ptr %e, ptr null, i64 0)
  br label %nomem
nomem:
  store i64 -12, ptr %st
  ret ptr null
invalid:
  store i64 -22, ptr %st
  ret ptr null
}

; The session failed with `why` (a status) and the sentence `text`: it is over.
define hidden i64 @nz_tls_fail(ptr %e, i64 %why, ptr %text) {
entry:
  %sp = getelementptr i8, ptr %e, i64 24
  store i64 3, ptr %sp
  %fp = getelementptr i8, ptr %e, i64 152
  store i64 %why, ptr %fp
  %dp = getelementptr i8, ptr %e, i64 160
  call void @llvm.memcpy.p0.p0.i64(ptr %dp, ptr %text, i64 40, i1 false)
  ret i64 %why
}

; The server's chain judged for the session's host: CertGetCertificateChain against the user's
; roots, or an engine whose only roots are the configuration's anchors, with no URL fetched; then
; CertVerifyCertificateChainPolicy's SSL policy. 0, or the status of the refusal.
define hidden i64 @nz_tls_verify(ptr %e) {
entry:
  %certp = alloca ptr, align 8
  %enginep = alloca ptr, align 8
  %chainp = alloca ptr, align 8
  %ecfg = alloca [88 x i8], align 8
  %para = alloca [32 x i8], align 8
  %oids = alloca ptr, align 8
  %wide = alloca [512 x i8], align 2
  %extra = alloca [24 x i8], align 8
  %pp = alloca [16 x i8], align 8
  %ps = alloca [24 x i8], align 8
  store ptr null, ptr %certp
  store ptr null, ptr %enginep
  store ptr null, ptr %chainp
  %ctx = getelementptr i8, ptr %e, i64 8
  ; SECPKG_ATTR_REMOTE_CERT_CONTEXT.
  %q = call i32 @QueryContextAttributesA(ptr %ctx, i32 83, ptr %certp)
  %cert = load ptr, ptr %certp
  %qok = icmp eq i32 %q, 0
  %nn = icmp ne ptr %cert, null
  %have = and i1 %qok, %nn
  br i1 %have, label %engine, label %untrusted
engine:
  %c = load ptr, ptr %e
  %ap = getelementptr i8, ptr %c, i64 32
  %anchors = load ptr, ptr %ap
  %own = icmp ne ptr %anchors, null
  br i1 %own, label %exclusive, label %chain
exclusive:
  call void @llvm.memset.p0.i64(ptr %ecfg, i8 0, i64 88, i1 false)
  store i32 88, ptr %ecfg
  %xr = getelementptr i8, ptr %ecfg, i64 64
  store ptr %anchors, ptr %xr
  %made = call i32 @CertCreateCertificateChainEngine(ptr %ecfg, ptr %enginep)
  %mok = icmp ne i32 %made, 0
  br i1 %mok, label %chain, label %free_cert
chain:
  call void @llvm.memset.p0.i64(ptr %para, i8 0, i64 32, i1 false)
  store i32 32, ptr %para
  store ptr @nz_tls_server_auth, ptr %oids
  %uc = getelementptr i8, ptr %para, i64 16
  store i32 1, ptr %uc
  %ur = getelementptr i8, ptr %para, i64 24
  store ptr %oids, ptr %ur
  %eng = load ptr, ptr %enginep
  %sp = getelementptr i8, ptr %cert, i64 32
  %store = load ptr, ptr %sp
  ; CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL: nothing fetched over the network.
  %got = call i32 @CertGetCertificateChain(ptr %eng, ptr %cert, ptr null, ptr %store, ptr %para, i32 4, ptr null, ptr %chainp)
  %gok = icmp ne i32 %got, 0
  br i1 %gok, label %policy, label %free_engine
policy:
  %hp = getelementptr i8, ptr %e, i64 136
  %host = load ptr, ptr %hp
  ; CP_UTF8.
  %w = call i32 @MultiByteToWideChar(i32 65001, i32 0, ptr %host, i32 -1, ptr %wide, i32 256)
  call void @llvm.memset.p0.i64(ptr %extra, i8 0, i64 24, i1 false)
  store i32 24, ptr %extra
  %at = getelementptr i8, ptr %extra, i64 4
  ; AUTHTYPE_SERVER.
  store i32 2, ptr %at
  %sn = getelementptr i8, ptr %extra, i64 16
  store ptr %wide, ptr %sn
  call void @llvm.memset.p0.i64(ptr %pp, i8 0, i64 16, i1 false)
  store i32 16, ptr %pp
  %px = getelementptr i8, ptr %pp, i64 8
  store ptr %extra, ptr %px
  call void @llvm.memset.p0.i64(ptr %ps, i8 0, i64 24, i1 false)
  store i32 24, ptr %ps
  %ch = load ptr, ptr %chainp
  ; CERT_CHAIN_POLICY_SSL.
  %v = call i32 @CertVerifyCertificateChainPolicy(ptr inttoptr (i64 4 to ptr), ptr %ch, ptr %pp, ptr %ps)
  %erp = getelementptr i8, ptr %ps, i64 4
  %err = load i32, ptr %erp
  call void @CertFreeCertificateChain(ptr %ch)
  %vok = icmp ne i32 %v, 0
  br i1 %vok, label %judge, label %free_engine
judge:
  %clean = icmp eq i32 %err, 0
  ; CERT_E_EXPIRED, CERT_E_VALIDITYPERIODNESTING; CERT_E_CN_NO_MATCH.
  %exp = icmp eq i32 %err, -2146762495
  %nest = icmp eq i32 %err, -2146762494
  %dated = or i1 %exp, %nest
  %name = icmp eq i32 %err, -2146762481
  %k1 = select i1 %name, i64 -65603, i64 -65601
  %k2 = select i1 %dated, i64 -65602, i64 %k1
  %verdict = select i1 %clean, i64 0, i64 %k2
  br label %free_engine
free_engine:
  %result = phi i64 [ -65601, %chain ], [ -65601, %policy ], [ %verdict, %judge ]
  %eg = load ptr, ptr %enginep
  %ne = icmp eq ptr %eg, null
  br i1 %ne, label %free_cert2, label %drop_engine
drop_engine:
  call void @CertFreeCertificateChainEngine(ptr %eg)
  br label %free_cert2
free_cert2:
  %fc2 = call i32 @CertFreeCertificateContext(ptr %cert)
  ret i64 %result
free_cert:
  %fc = call i32 @CertFreeCertificateContext(ptr %cert)
  br label %untrusted
untrusted:
  ret i64 -65601
}

; SSPI's output tokens in `out` (a SecBuffer array of two, the token and the alert) appended to
; the session's ciphertext to send, and freed.
define hidden void @nz_tls_emit(ptr %e, ptr %out) {
entry:
  br label %each
each:
  %i = phi i64 [ 0, %entry ], [ %i1, %next ]
  %b = getelementptr [16 x i8], ptr %out, i64 %i
  %cb = load i32, ptr %b
  %pvp = getelementptr i8, ptr %b, i64 8
  %pv = load ptr, ptr %pvp
  %some = icmp ne ptr %pv, null
  br i1 %some, label %send, label %next
send:
  %pos = icmp ugt i32 %cb, 0
  br i1 %pos, label %append, label %release
append:
  %n = zext i32 %cb to i64
  %ob = getelementptr i8, ptr %e, i64 56
  %ok = call i1 @nz_tls_append(ptr %ob, ptr %pv, i64 %n)
  br label %release
release:
  %f = call i32 @FreeContextBuffer(ptr %pv)
  store ptr null, ptr %pvp
  br label %next
next:
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, 2
  br i1 %more, label %each, label %done
done:
  ret void
}

; One step of SSPI's handshake: InitializeSecurityContextA for a client, AcceptSecurityContext for a
; server, over what has been pushed. 1 once established, the wait for more ciphertext, or a status.
define hidden i64 @nz_tls_handshake(ptr %e) {
entry:
  %inb = alloca [48 x i8], align 8
  %ind = alloca [16 x i8], align 8
  %outb = alloca [32 x i8], align 8
  %outd = alloca [16 x i8], align 8
  %attr = alloca i32, align 4
  %sp = getelementptr i8, ptr %e, i64 24
  br label %step
step:
  %state = load i64, ptr %sp
  %up = icmp eq i64 %state, 2
  br i1 %up, label %established, label %live
live:
  %over = icmp sgt i64 %state, 2
  br i1 %over, label %finished, label %which
which:
  %c = load ptr, ptr %e
  %kp = getelementptr i8, ptr %c, i64 24
  %kind = load i64, ptr %kp
  %client = icmp eq i64 %kind, 1
  %first = icmp eq i64 %state, 0
  %inp = getelementptr i8, ptr %e, i64 32
  %in = load ptr, ptr %inp
  %inlp = getelementptr i8, ptr %e, i64 40
  %inl = load i64, ptr %inlp
  %nothing = icmp eq i64 %inl, 0
  %opens = and i1 %client, %first
  %starve = xor i1 %opens, true
  %wait = and i1 %starve, %nothing
  br i1 %wait, label %want, label %buffers
buffers:
  ; Input: the ciphertext as a token, an empty buffer for SSPI's extra, and on a first call the
  ; ALPN names. Output: a token and an alert, each SSPI's memory.
  call void @llvm.memset.p0.i64(ptr %inb, i8 0, i64 48, i1 false)
  %in32 = trunc i64 %inl to i32
  store i32 %in32, ptr %inb
  %t0 = getelementptr i8, ptr %inb, i64 4
  store i32 2, ptr %t0
  %p0 = getelementptr i8, ptr %inb, i64 8
  store ptr %in, ptr %p0
  %abp = getelementptr i8, ptr %c, i64 56
  %ab = load ptr, ptr %abp
  %hasab = icmp ne ptr %ab, null
  %offer = and i1 %hasab, %first
  br i1 %offer, label %alpn, label %described
alpn:
  %abnp = getelementptr i8, ptr %c, i64 64
  %abn = load i64, ptr %abnp
  %abn32 = trunc i64 %abn to i32
  %b2 = getelementptr i8, ptr %inb, i64 32
  store i32 %abn32, ptr %b2
  %t2 = getelementptr i8, ptr %inb, i64 36
  ; SECBUFFER_APPLICATION_PROTOCOLS.
  store i32 18, ptr %t2
  %p2 = getelementptr i8, ptr %inb, i64 40
  store ptr %ab, ptr %p2
  br label %described
described:
  %count = select i1 %offer, i32 3, i32 2
  %only = select i1 %offer, i32 1, i32 0
  %n_in = select i1 %opens, i32 %only, i32 %count
  %alpnbuf = getelementptr i8, ptr %inb, i64 32
  %bufs_in = select i1 %opens, ptr %alpnbuf, ptr %inb
  store i32 0, ptr %ind
  %ic = getelementptr i8, ptr %ind, i64 4
  store i32 %n_in, ptr %ic
  %ibp = getelementptr i8, ptr %ind, i64 8
  store ptr %bufs_in, ptr %ibp
  call void @llvm.memset.p0.i64(ptr %outb, i8 0, i64 32, i1 false)
  %to = getelementptr i8, ptr %outb, i64 4
  store i32 2, ptr %to
  %ta = getelementptr i8, ptr %outb, i64 20
  ; SECBUFFER_ALERT.
  store i32 17, ptr %ta
  store i32 0, ptr %outd
  %oc = getelementptr i8, ptr %outd, i64 4
  store i32 2, ptr %oc
  %obp = getelementptr i8, ptr %outd, i64 8
  store ptr %outb, ptr %obp
  %cred = getelementptr i8, ptr %c, i64 8
  %ctx = getelementptr i8, ptr %e, i64 8
  %old = select i1 %first, ptr null, ptr %ctx
  %new = select i1 %first, ptr %ctx, ptr null
  br i1 %client, label %initiate, label %accept
initiate:
  %hp = getelementptr i8, ptr %e, i64 136
  %host = load ptr, ptr %hp
  ; A first call sends the hello, with no input but the ALPN names when there are some.
  %inarg = select i1 %first, ptr null, ptr %ind
  %alpnonly = and i1 %first, %hasab
  %inarg2 = select i1 %alpnonly, ptr %ind, ptr %inarg
  ; ISC_REQ_SEQUENCE_DETECT | REPLAY_DETECT | CONFIDENTIALITY | ALLOCATE_MEMORY | STREAM |
  ; EXTENDED_ERROR | MANUAL_CRED_VALIDATION.
  %ri = call i32 @InitializeSecurityContextA(ptr %cred, ptr %old, ptr %host, i32 573724, i32 0, i32 0, ptr %inarg2, i32 0, ptr %new, ptr %outd, ptr %attr, ptr null)
  br label %called
accept:
  ; ASC_REQ_SEQUENCE_DETECT | REPLAY_DETECT | CONFIDENTIALITY | ALLOCATE_MEMORY | STREAM |
  ; EXTENDED_ERROR.
  %ra = call i32 @AcceptSecurityContext(ptr %cred, ptr %old, ptr %ind, i32 98588, i32 0, ptr %new, ptr %outd, ptr %attr, ptr null)
  br label %called
called:
  %r = phi i32 [ %ri, %initiate ], [ %ra, %accept ]
  call void @nz_tls_emit(ptr %e, ptr %outb)
  ; SEC_E_INCOMPLETE_MESSAGE: more ciphertext first, nothing consumed.
  %incomplete = icmp eq i32 %r, -2146893032
  br i1 %incomplete, label %want, label %result
result:
  %ok = icmp eq i32 %r, 0
  %cont = icmp eq i32 %r, 590610
  %going = or i1 %ok, %cont
  br i1 %going, label %consume, label %refused
consume:
  store i64 1, ptr %sp
  %t1 = getelementptr i8, ptr %inb, i64 20
  %t1v = load i32, ptr %t1
  %extra = icmp eq i32 %t1v, 5
  %x1 = getelementptr i8, ptr %inb, i64 16
  %xn = load i32, ptr %x1
  %xn64 = zext i32 %xn to i64
  %keep = select i1 %extra, i64 %xn64, i64 0
  %fed = select i1 %opens, i64 0, i64 %inl
  %used = sub i64 %fed, %keep
  %ib = getelementptr i8, ptr %e, i64 32
  call void @nz_tls_drop(ptr %ib, i64 %used)
  br i1 %ok, label %up_now, label %again
again:
  %left = load i64, ptr %inlp
  %anything = icmp sgt i64 %left, 0
  br i1 %anything, label %step, label %want
up_now:
  ; SECPKG_ATTR_STREAM_SIZES.
  %sz = getelementptr i8, ptr %e, i64 112
  %qs = call i32 @QueryContextAttributesA(ptr %ctx, i32 4, ptr %sz)
  br i1 %client, label %verify, label %mark
verify:
  %v = call i64 @nz_tls_verify(ptr %e)
  %trusted = icmp eq i64 %v, 0
  br i1 %trusted, label %mark, label %distrust
distrust:
  %fu = icmp eq i64 %v, -65602
  %fh = icmp eq i64 %v, -65603
  %t_e = select i1 %fu, ptr @nz_tls_why_expired, ptr @nz_tls_why_untrusted
  %t_h = select i1 %fh, ptr @nz_tls_why_host, ptr %t_e
  %vf = call i64 @nz_tls_fail(ptr %e, i64 %v, ptr %t_h)
  ret i64 %vf
mark:
  store i64 2, ptr %sp
  br label %established
established:
  ret i64 1
refused:
  %pf = call i64 @nz_tls_fail(ptr %e, i64 -65604, ptr @nz_tls_why_protocol)
  ret i64 %pf
finished:
  %fp = getelementptr i8, ptr %e, i64 152
  %f = load i64, ptr %fp
  %clean = icmp eq i64 %f, 0
  %g = select i1 %clean, i64 -65604, i64 %f
  ret i64 %g
want:
  ret i64 -65600
}

define hidden i64 @nz_tls_push(ptr %e, ptr %p, i64 %n) {
entry:
  %none = icmp slt i64 %n, 1
  br i1 %none, label %zero, label %go
zero:
  ret i64 0
go:
  %ib = getelementptr i8, ptr %e, i64 32
  %ok = call i1 @nz_tls_append(ptr %ib, ptr %p, i64 %n)
  %r = select i1 %ok, i64 %n, i64 -12
  ret i64 %r
}

define hidden i64 @nz_tls_pull(ptr %e, ptr %p, i64 %n) {
entry:
  %ob = getelementptr i8, ptr %e, i64 56
  %olp = getelementptr i8, ptr %e, i64 64
  %ol = load i64, ptr %olp
  %o = load ptr, ptr %ob
  %g = call i64 @nz_tls_give(ptr %o, i64 %ol, ptr %p, i64 %n)
  call void @nz_tls_drop(ptr %ob, i64 %g)
  ret i64 %g
}

; Plaintext: what an earlier record left, else one record decrypted from what has been pushed
; (DecryptMessage over a data buffer and three empty ones). 0 once the peer has closed TLS.
define hidden i64 @nz_tls_read(ptr %e, ptr %p, i64 %n) {
entry:
  %bufs = alloca [64 x i8], align 8
  %desc = alloca [16 x i8], align 8
  %view = alloca [24 x i8], align 8
  %none = icmp slt i64 %n, 1
  br i1 %none, label %invalid, label %start
invalid:
  ret i64 -22
start:
  %sp = getelementptr i8, ptr %e, i64 24
  %state = load i64, ptr %sp
  %closed = icmp eq i64 %state, 4
  br i1 %closed, label %eof, label %ready
ready:
  %up = icmp eq i64 %state, 2
  br i1 %up, label %loop, label %shake
shake:
  %h = call i64 @nz_tls_handshake(ptr %e)
  %hup = icmp eq i64 %h, 1
  br i1 %hup, label %loop, label %early
early:
  ret i64 %h
loop:
  %pb = getelementptr i8, ptr %e, i64 80
  %plp = getelementptr i8, ptr %e, i64 96
  %pl = load i64, ptr %plp
  %have = icmp sgt i64 %pl, 0
  br i1 %have, label %serve, label %decrypt
serve:
  %pspp = getelementptr i8, ptr %e, i64 88
  %ps = load i64, ptr %pspp
  %pbase = load ptr, ptr %pb
  %from = getelementptr i8, ptr %pbase, i64 %ps
  %g = call i64 @nz_tls_give(ptr %from, i64 %pl, ptr %p, i64 %n)
  %ps1 = add i64 %ps, %g
  %pl1 = sub i64 %pl, %g
  %gone = icmp eq i64 %pl1, 0
  %ps2 = select i1 %gone, i64 0, i64 %ps1
  store i64 %ps2, ptr %pspp
  store i64 %pl1, ptr %plp
  ret i64 %g
decrypt:
  %inp = getelementptr i8, ptr %e, i64 32
  %in = load ptr, ptr %inp
  %inlp = getelementptr i8, ptr %e, i64 40
  %inl = load i64, ptr %inlp
  %nothing = icmp eq i64 %inl, 0
  br i1 %nothing, label %want, label %call
call:
  call void @llvm.memset.p0.i64(ptr %bufs, i8 0, i64 64, i1 false)
  %in32 = trunc i64 %inl to i32
  store i32 %in32, ptr %bufs
  %t0 = getelementptr i8, ptr %bufs, i64 4
  ; SECBUFFER_DATA.
  store i32 1, ptr %t0
  %p0 = getelementptr i8, ptr %bufs, i64 8
  store ptr %in, ptr %p0
  store i32 0, ptr %desc
  %dc = getelementptr i8, ptr %desc, i64 4
  store i32 4, ptr %dc
  %dbp = getelementptr i8, ptr %desc, i64 8
  store ptr %bufs, ptr %dbp
  %ctx = getelementptr i8, ptr %e, i64 8
  %r = call i32 @DecryptMessage(ptr %ctx, ptr %desc, i32 0, ptr null)
  %incomplete = icmp eq i32 %r, -2146893032
  br i1 %incomplete, label %want, label %result
result:
  ; SEC_I_CONTEXT_EXPIRED: the peer's close_notify.
  %expired = icmp eq i32 %r, 590615
  br i1 %expired, label %peer_closed, label %decrypted
decrypted:
  %ok = icmp eq i32 %r, 0
  br i1 %ok, label %scan, label %refused
scan:
  br label %each
each:
  %i = phi i64 [ 0, %scan ], [ %i1, %next ]
  %keep = phi i64 [ 0, %scan ], [ %keep1, %next ]
  %b = getelementptr [16 x i8], ptr %bufs, i64 %i
  %cb = load i32, ptr %b
  %cb64 = zext i32 %cb to i64
  %tp = getelementptr i8, ptr %b, i64 4
  %t = load i32, ptr %tp
  %pvp = getelementptr i8, ptr %b, i64 8
  %pv = load ptr, ptr %pvp
  %data = icmp eq i32 %t, 1
  br i1 %data, label %plain, label %extra_q
plain:
  %pbuf = getelementptr i8, ptr %e, i64 80
  %pl0 = getelementptr i8, ptr %e, i64 96
  %pbl = getelementptr i8, ptr %e, i64 104
  ; The plain buffer's { ptr, len, cap } is at 80, 96 and 104 with its start at 88: appended
  ; through a view whose length and capacity are its own.
  %pbase0 = load ptr, ptr %pbuf
  store ptr %pbase0, ptr %view
  %vl = getelementptr i8, ptr %view, i64 8
  %curl = load i64, ptr %pl0
  store i64 %curl, ptr %vl
  %vc = getelementptr i8, ptr %view, i64 16
  %curc = load i64, ptr %pbl
  store i64 %curc, ptr %vc
  %apok = call i1 @nz_tls_append(ptr %view, ptr %pv, i64 %cb64)
  %nb = load ptr, ptr %view
  store ptr %nb, ptr %pbuf
  %nl = load i64, ptr %vl
  store i64 %nl, ptr %pl0
  %nc = load i64, ptr %vc
  store i64 %nc, ptr %pbl
  br label %next
extra_q:
  %isextra = icmp eq i32 %t, 5
  %k = select i1 %isextra, i64 %cb64, i64 %keep
  br label %next
next:
  %keep1 = phi i64 [ %keep, %plain ], [ %k, %extra_q ]
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, 4
  br i1 %more, label %each, label %compact
compact:
  %used = sub i64 %inl, %keep1
  %ib = getelementptr i8, ptr %e, i64 32
  call void @nz_tls_drop(ptr %ib, i64 %used)
  br label %loop
peer_closed:
  store i64 4, ptr %sp
  br label %eof
eof:
  ret i64 0
refused:
  %pf = call i64 @nz_tls_fail(ptr %e, i64 -65604, ptr @nz_tls_why_protocol)
  ret i64 %pf
want:
  ret i64 -65600
}

; One record of plaintext, at most the stream's largest message: EncryptMessage over a header, the
; data and a trailer, laid end to end, appended to the ciphertext to send. How much was taken.
define hidden i64 @nz_tls_write(ptr %e, ptr %p, i64 %n) {
entry:
  %bufs = alloca [64 x i8], align 8
  %desc = alloca [16 x i8], align 8
  %none = icmp slt i64 %n, 1
  br i1 %none, label %zero, label %start
zero:
  ret i64 0
start:
  %sp = getelementptr i8, ptr %e, i64 24
  %state = load i64, ptr %sp
  %closed = icmp eq i64 %state, 4
  br i1 %closed, label %gone, label %ready
gone:
  ret i64 -65536
ready:
  %up = icmp eq i64 %state, 2
  br i1 %up, label %sizes, label %shake
shake:
  %h = call i64 @nz_tls_handshake(ptr %e)
  %hup = icmp eq i64 %h, 1
  br i1 %hup, label %sizes, label %early
early:
  ret i64 %h
sizes:
  %hdp = getelementptr i8, ptr %e, i64 112
  %hd = load i32, ptr %hdp
  %trp = getelementptr i8, ptr %e, i64 116
  %tr = load i32, ptr %trp
  %mxp = getelementptr i8, ptr %e, i64 120
  %mx = load i32, ptr %mxp
  %hd64 = zext i32 %hd to i64
  %tr64 = zext i32 %tr to i64
  %mx64 = zext i32 %mx to i64
  %fits = icmp slt i64 %n, %mx64
  %chunk = select i1 %fits, i64 %n, i64 %mx64
  %t1 = add i64 %hd64, %chunk
  %total = add i64 %t1, %tr64
  %rec = call ptr @malloc(i64 %total)
  %nr = icmp eq ptr %rec, null
  br i1 %nr, label %nomem, label %fill
fill:
  %dat = getelementptr i8, ptr %rec, i64 %hd64
  call void @llvm.memcpy.p0.p0.i64(ptr %dat, ptr %p, i64 %chunk, i1 false)
  %trl = getelementptr i8, ptr %rec, i64 %t1
  call void @llvm.memset.p0.i64(ptr %bufs, i8 0, i64 64, i1 false)
  store i32 %hd, ptr %bufs
  %t0 = getelementptr i8, ptr %bufs, i64 4
  ; SECBUFFER_STREAM_HEADER, DATA, STREAM_TRAILER, EMPTY.
  store i32 7, ptr %t0
  %p0 = getelementptr i8, ptr %bufs, i64 8
  store ptr %rec, ptr %p0
  %c1 = getelementptr i8, ptr %bufs, i64 16
  %ch32 = trunc i64 %chunk to i32
  store i32 %ch32, ptr %c1
  %tt1 = getelementptr i8, ptr %bufs, i64 20
  store i32 1, ptr %tt1
  %p1 = getelementptr i8, ptr %bufs, i64 24
  store ptr %dat, ptr %p1
  %c2 = getelementptr i8, ptr %bufs, i64 32
  store i32 %tr, ptr %c2
  %tt2 = getelementptr i8, ptr %bufs, i64 36
  store i32 6, ptr %tt2
  %p2 = getelementptr i8, ptr %bufs, i64 40
  store ptr %trl, ptr %p2
  store i32 0, ptr %desc
  %dc = getelementptr i8, ptr %desc, i64 4
  store i32 4, ptr %dc
  %dbp = getelementptr i8, ptr %desc, i64 8
  store ptr %bufs, ptr %dbp
  %ctx = getelementptr i8, ptr %e, i64 8
  %r = call i32 @EncryptMessage(ptr %ctx, i32 0, ptr %desc, i32 0)
  %ok = icmp eq i32 %r, 0
  br i1 %ok, label %send, label %refused
send:
  %h0 = load i32, ptr %bufs
  %d0 = load i32, ptr %c1
  %r0 = load i32, ptr %c2
  %h064 = zext i32 %h0 to i64
  %d064 = zext i32 %d0 to i64
  %r064 = zext i32 %r0 to i64
  %s1 = add i64 %h064, %d064
  %s2 = add i64 %s1, %r064
  %ob = getelementptr i8, ptr %e, i64 56
  %aok = call i1 @nz_tls_append(ptr %ob, ptr %rec, i64 %s2)
  call void @free(ptr %rec)
  br i1 %aok, label %took, label %nomem
took:
  ret i64 %chunk
refused:
  call void @free(ptr %rec)
  %pf = call i64 @nz_tls_fail(ptr %e, i64 -65604, ptr @nz_tls_why_protocol)
  ret i64 %pf
nomem:
  ret i64 -12
}

; The end of session `e`: once established, SCHANNEL_SHUTDOWN applied and the close_notify SSPI
; then writes, into `p`; then everything it holds freed and its configuration let go. A null `p`
; frees without writing.
define hidden i64 @nz_tls_close(ptr %e, ptr %p, i64 %n) {
entry:
  %token = alloca i32, align 4
  %tb = alloca [16 x i8], align 8
  %td = alloca [16 x i8], align 8
  %outb = alloca [32 x i8], align 8
  %outd = alloca [16 x i8], align 8
  %attr = alloca i32, align 4
  %sp = getelementptr i8, ptr %e, i64 24
  %state = load i64, ptr %sp
  %c = load ptr, ptr %e
  %ctx = getelementptr i8, ptr %e, i64 8
  %up = icmp eq i64 %state, 2
  %np = icmp eq ptr %p, null
  %write = xor i1 %np, true
  %notify = and i1 %up, %write
  br i1 %notify, label %shutdown, label %drop
shutdown:
  store i32 1, ptr %token
  store i32 4, ptr %tb
  %tt = getelementptr i8, ptr %tb, i64 4
  store i32 2, ptr %tt
  %tp = getelementptr i8, ptr %tb, i64 8
  store ptr %token, ptr %tp
  store i32 0, ptr %td
  %tc = getelementptr i8, ptr %td, i64 4
  store i32 1, ptr %tc
  %tbp = getelementptr i8, ptr %td, i64 8
  store ptr %tb, ptr %tbp
  %a = call i32 @ApplyControlToken(ptr %ctx, ptr %td)
  call void @llvm.memset.p0.i64(ptr %outb, i8 0, i64 32, i1 false)
  %to = getelementptr i8, ptr %outb, i64 4
  store i32 2, ptr %to
  %ta = getelementptr i8, ptr %outb, i64 20
  store i32 17, ptr %ta
  store i32 0, ptr %outd
  %oc = getelementptr i8, ptr %outd, i64 4
  store i32 2, ptr %oc
  %obp = getelementptr i8, ptr %outd, i64 8
  store ptr %outb, ptr %obp
  %cred = getelementptr i8, ptr %c, i64 8
  %kp = getelementptr i8, ptr %c, i64 24
  %kind = load i64, ptr %kp
  %client = icmp eq i64 %kind, 1
  br i1 %client, label %initiate, label %accept
initiate:
  %hp = getelementptr i8, ptr %e, i64 136
  %host = load ptr, ptr %hp
  %ri = call i32 @InitializeSecurityContextA(ptr %cred, ptr %ctx, ptr %host, i32 573724, i32 0, i32 0, ptr null, i32 0, ptr null, ptr %outd, ptr %attr, ptr null)
  br label %emitted
accept:
  %ra = call i32 @AcceptSecurityContext(ptr %cred, ptr %ctx, ptr null, i32 98588, i32 0, ptr null, ptr %outd, ptr %attr, ptr null)
  br label %emitted
emitted:
  call void @nz_tls_emit(ptr %e, ptr %outb)
  %r = call i64 @nz_tls_pull(ptr %e, ptr %p, i64 %n)
  br label %drop
drop:
  %out = phi i64 [ 0, %entry ], [ %r, %emitted ]
  %started = icmp ne i64 %state, 0
  br i1 %started, label %delete, label %rest
delete:
  %d = call i32 @DeleteSecurityContext(ptr %ctx)
  br label %rest
rest:
  %ip = getelementptr i8, ptr %e, i64 32
  %i = load ptr, ptr %ip
  call void @free(ptr %i)
  %opp = getelementptr i8, ptr %e, i64 56
  %o = load ptr, ptr %opp
  call void @free(ptr %o)
  %pp = getelementptr i8, ptr %e, i64 80
  %pl = load ptr, ptr %pp
  call void @free(ptr %pl)
  %hp2 = getelementptr i8, ptr %e, i64 136
  %h = load ptr, ptr %hp2
  call void @free(ptr %h)
  call void @nz_tls_config_free(ptr %c)
  call void @free(ptr %e)
  ret i64 %out
}

; A fact about session `e`, into `p`: 0 the ALPN name agreed, 1 the version, 2 the peer
; certificate's common name as `/CN=…`, 3 why it failed. Its length; empty when there is none.
define hidden i64 @nz_tls_info(ptr %e, i64 %what, ptr %p, i64 %cap) {
entry:
  %ap = alloca [264 x i8], align 8
  %ci = alloca [28 x i8], align 8
  %certp = alloca ptr, align 8
  %name = alloca [256 x i8], align 1
  %sp = getelementptr i8, ptr %e, i64 24
  %state = load i64, ptr %sp
  %ctx = getelementptr i8, ptr %e, i64 8
  %up = icmp eq i64 %state, 2
  %detail = icmp eq i64 %what, 3
  %known = icmp ult i64 %what, 4
  br i1 %known, label %which, label %invalid
which:
  br i1 %detail, label %why, label %live
live:
  br i1 %up, label %pick, label %empty
pick:
  switch i64 %what, label %empty [ i64 0, label %alpn
                                   i64 1, label %version
                                   i64 2, label %peer ]
alpn:
  call void @llvm.memset.p0.i64(ptr %ap, i8 0, i64 264, i1 false)
  ; SECPKG_ATTR_APPLICATION_PROTOCOL; SecApplicationProtocolNegotiationStatus_Success.
  %qa = call i32 @QueryContextAttributesA(ptr %ctx, i32 35, ptr %ap)
  %st = load i32, ptr %ap
  %qok = icmp eq i32 %qa, 0
  %agreed = icmp eq i32 %st, 1
  %both = and i1 %qok, %agreed
  br i1 %both, label %give_alpn, label %empty
give_alpn:
  %szp = getelementptr i8, ptr %ap, i64 8
  %sz = load i8, ptr %szp
  %sz64 = zext i8 %sz to i64
  %idp = getelementptr i8, ptr %ap, i64 9
  %ga = call i64 @nz_tls_give(ptr %idp, i64 %sz64, ptr %p, i64 %cap)
  ret i64 %ga
version:
  call void @llvm.memset.p0.i64(ptr %ci, i8 0, i64 28, i1 false)
  ; SECPKG_ATTR_CONNECTION_INFO: SP_PROT_TLS1_2 (0xC00), SP_PROT_TLS1_3 (0x3000).
  %qc = call i32 @QueryContextAttributesA(ptr %ctx, i32 90, ptr %ci)
  %proto = load i32, ptr %ci
  %v12b = and i32 %proto, 3072
  %v13b = and i32 %proto, 12288
  %is12 = icmp ne i32 %v12b, 0
  %is13 = icmp ne i32 %v13b, 0
  %t12 = select i1 %is12, ptr @nz_tls_v12, ptr @nz_tls_vtls
  %t = select i1 %is13, ptr @nz_tls_v13, ptr %t12
  %tl = call i64 @strlen(ptr %t)
  %gv = call i64 @nz_tls_give(ptr %t, i64 %tl, ptr %p, i64 %cap)
  ret i64 %gv
peer:
  store ptr null, ptr %certp
  %qp = call i32 @QueryContextAttributesA(ptr %ctx, i32 83, ptr %certp)
  %cert = load ptr, ptr %certp
  %pok = icmp eq i32 %qp, 0
  %has = icmp ne ptr %cert, null
  %got = and i1 %pok, %has
  br i1 %got, label %cn, label %empty
cn:
  ; CERT_NAME_ATTR_TYPE with the common name's OID.
  %nl = call i32 @CertGetNameStringA(ptr %cert, i32 3, i32 0, ptr @nz_tls_cn, ptr %name, i32 256)
  %fc = call i32 @CertFreeCertificateContext(ptr %cert)
  %sl = call i64 @strlen(ptr %name)
  %g1 = call i64 @nz_tls_give(ptr @nz_tls_slash_cn, i64 4, ptr %p, i64 %cap)
  %rest = getelementptr i8, ptr %p, i64 %g1
  %room = sub i64 %cap, %g1
  %g2 = call i64 @nz_tls_give(ptr %name, i64 %sl, ptr %rest, i64 %room)
  %gp = add i64 %g1, %g2
  ret i64 %gp
why:
  %dp = getelementptr i8, ptr %e, i64 160
  %dl = call i64 @strlen(ptr %dp)
  %gd = call i64 @nz_tls_give(ptr %dp, i64 %dl, ptr %p, i64 %cap)
  ret i64 %gd
empty:
  ret i64 0
invalid:
  ret i64 -22
}
"#;
