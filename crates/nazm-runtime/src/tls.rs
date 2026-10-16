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
//! SChannel binding and every target without one are refused by name (`N0830`). Verification is
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
define hidden ptr @nz.tls_table() {
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
define hidden ptr @nz.tls_hold(ptr %s, i64 %id, i64 %kind) {
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
        SwitchTarget::X86_64Windows | SwitchTarget::Aarch64Android => None,
    }
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
