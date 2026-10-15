//! Networking (Gate 2, `docs/spec.md`, *Networking*): the socket entries and the reactor, as
//! target-independent LLVM text over the handle table (`crate::files`) and the per-target unit's
//! `nz_net_*` (`crate::platform`).
//!
//! # Waiting without holding a worker
//!
//! Every socket is non-blocking. An operation that would block **waits on its handle's wait
//! record** — a mutex, a condition, a sequence number and the directions asked for, allocated the
//! first time a handle on that descriptor waits and kept for the descriptor's life — through the
//! language's existing waits, `nz.cwait` and `nz.ctimedwait`: on the task pool those park the task
//! and its worker runs others; with a thread per task the thread waits. Before waiting it arms the
//! descriptor, once, with the **reactor**: one runtime thread blocked in `kqueue` or `epoll` whose
//! every event names a handle. The reactor bumps that handle's sequence number and wakes its
//! waiters with `nz.cwake`; a waiter that sees the number move retries its operation. A deadline is
//! the handle's (`os_set_timeout`), as an absolute time on the monotonic clock (Q1-M-12) the timed
//! wait gives up at; a close marks the handle closing and wakes its waiters the same way, and each
//! answers `Closed`.
//!
//! The reactor holds no lock while it waits, and takes the handle table's only to find a wait
//! record; a waiter holds its record's mutex and takes the table's to ask whether its handle is
//! closing; a close takes the table's and lets it go before it wakes anyone. So no two locks are
//! ever taken in opposite orders.

/// The runtime text.
pub(crate) const NET_RUNTIME: &str = r#"
@nz.net_state = hidden global i64 0
@nz.net_poller = hidden global i64 0
@nz.net_thread = hidden global i64 0

declare i32 @nz_net_poller()
declare i32 @nz_net_arm(i32, i32, i64, i64)
declare i64 @nz_net_poll(i32, ptr)
declare i64 @nz_net_fill(ptr, i64, i64, ptr)
declare i64 @nz_net_format(ptr, ptr, i64)
declare i32 @nz_net_prepare(i32)
declare i32 @nz_net_open(ptr, i64, i64)
declare i64 @nz_net_connect(i32, ptr, i64)
declare i64 @nz_net_error(i32)
declare i64 @nz_net_send(i32, ptr, i64)
declare i64 @nz_net_sendto(i32, ptr, i64, ptr, i64)
declare i64 @nz_net_eagain()
declare i64 @nz_net_etimedout()
declare i64 @nz_net_resolve(ptr, i64, ptr, i64)
declare i64 @nz_net_read(i32, ptr, i64)
declare i32 @nz_net_bind(i32, ptr, i32)
declare i32 @nz_net_listen(i32)
declare i32 @nz_net_accept(i32)
declare i64 @nz_net_recvfrom(i32, ptr, i64, ptr, ptr)
declare i32 @nz_net_shutdown(i32, i32)
declare i32 @nz_net_name(i32, i32, ptr, ptr)
declare i64 @nz_net_write(i32, ptr, i64)
declare i32 @nz_os_spawn_in(ptr, ptr, ptr, ptr, ptr, i64, ptr)
declare i64 @nz_os_reap(i64, i32, ptr)
declare i32 @nz_os_terminate(i64)
declare i32 @nz_os_key()
declare i64 @nz_os_child_id(i64)
declare i64 @nz_os_pid()
declare i32 @nz_os_signal_group(i64, i64)
declare void @nz_os_nap(i64)

; The monotonic clock in nanoseconds: what `nz.ctimedwait`'s deadline is measured against
; (Q1-M-12). A wall-clock deadline there is decades ahead of the monotonic clock: a thread waited
; out the nap's 100 ms cap on every wait however little was left, and a pool task never timed out.
define hidden i64 @nz.net_now() {
entry:
  %ts = alloca [16 x i8], align 8
  call void @llvm.memset.p0.i64(ptr %ts, i8 0, i64 16, i1 false)
  %r = call i32 @nz_os_clock(i32 4, ptr %ts)
  %s = load i64, ptr %ts
  %np = getelementptr i8, ptr %ts, i64 8
  %n = load i64, ptr %np
  %sn = mul i64 %s, 1000000000
  %t = add i64 %sn, %n
  ret i64 %t
}

; Start the reactor once: the first caller makes the poller and the thread, the rest wait for it.
define hidden void @nz.net_start() {
entry:
  %st = load atomic i64, ptr @nz.net_state acquire, align 8
  %ready = icmp eq i64 %st, 2
  br i1 %ready, label %done, label %race
race:
  %pair = cmpxchg ptr @nz.net_state, i64 0, i64 1 acq_rel acquire
  %won = extractvalue { i64, i1 } %pair, 1
  br i1 %won, label %make, label %spin
make:
  %q = call i32 @nz_net_poller()
  %qw = sext i32 %q to i64
  store i64 %qw, ptr @nz.net_poller
  %c = call i32 @pthread_create(ptr @nz.net_thread, ptr null, ptr @nz.net_reactor, ptr null)
  store atomic i64 2, ptr @nz.net_state release, align 8
  br label %done
spin:
  %again = load atomic i64, ptr @nz.net_state acquire, align 8
  %up = icmp eq i64 %again, 2
  br i1 %up, label %done, label %spin
done:
  ret void
}

; The reactor: forever, wait for events and wake each one's handle.
define hidden ptr @nz.net_reactor(ptr %arg) {
entry:
  %buf = alloca [64 x i64], align 8
  %qw = load i64, ptr @nz.net_poller
  %q = trunc i64 %qw to i32
  br label %wait
wait:
  %n = call i64 @nz_net_poll(i32 %q, ptr %buf)
  br label %each
each:
  %i = phi i64 [ 0, %wait ], [ %i1, %woke ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %one, label %wait
one:
  %hp = getelementptr i64, ptr %buf, i64 %i
  %h = load i64, ptr %hp
  call void @nz.net_event(i64 %h)
  br label %woke
woke:
  %i1 = add i64 %i, 1
  br label %each
}

; An event for handle `h`: wake its waiters, if its descriptor is still that generation's.
define hidden void @nz.net_event(i64 %h) {
entry:
  %fd = and i64 %h, 4294967295
  %gen = lshr i64 %h, 32
  call void @nz.os_acquire()
  %cap = load i64, ptr @nz.os_cap
  %fits = icmp slt i64 %fd, %cap
  br i1 %fits, label %slot, label %none
slot:
  %slots = load ptr, ptr @nz.os_slots
  %off = mul i64 %fd, 64
  %s = getelementptr i8, ptr %slots, i64 %off
  %g = load i64, ptr %s
  %same = icmp eq i64 %g, %gen
  br i1 %same, label %found, label %none
found:
  %wp = getelementptr i8, ptr %s, i64 32
  %w = load ptr, ptr %wp
  call void @nz.os_release()
  call void @nz.net_wake(ptr %w)
  ret void
none:
  call void @nz.os_release()
  ret void
}

; Wake everyone waiting on record `w` (null: nobody ever has).
define hidden void @nz.net_wake(ptr %w) {
entry:
  %none = icmp eq ptr %w, null
  br i1 %none, label %done, label %wake
wake:
  %l = call i32 @pthread_mutex_lock(ptr %w)
  %seqp = getelementptr i8, ptr %w, i64 128
  %seq = load i64, ptr %seqp
  %seq1 = add i64 %seq, 1
  store i64 %seq1, ptr %seqp
  %interestp = getelementptr i8, ptr %w, i64 136
  store i64 0, ptr %interestp
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  %cv = getelementptr i8, ptr %w, i64 64
  call void @nz.cwake(ptr %cv)
  br label %done
done:
  ret void
}

; The wait record of live handle `h`, made the first time; null for a handle that is not live.
define hidden ptr @nz.net_waitrec(i64 %h) {
entry:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %none, label %live
none:
  call void @nz.os_release()
  ret ptr null
live:
  %wp = getelementptr i8, ptr %s, i64 32
  %w = load ptr, ptr %wp
  %fresh = icmp eq ptr %w, null
  br i1 %fresh, label %make, label %have
have:
  call void @nz.os_release()
  ret ptr %w
make:
  %m = call ptr @malloc(i64 256)
  %oom = icmp eq ptr %m, null
  br i1 %oom, label %none, label %init
init:
  call void @llvm.memset.p0.i64(ptr %m, i8 0, i64 256, i1 false)
  %mi = call i32 @pthread_mutex_init(ptr %m, ptr null)
  %cv = getelementptr i8, ptr %m, i64 64
  %ci = call i32 @pthread_cond_init(ptr %cv, ptr null)
  store ptr %m, ptr %wp
  call void @nz.os_release()
  ret ptr %m
}

; Whether `h` is no longer live — closed, or closing.
define hidden i1 @nz.net_closing(i64 %h) {
entry:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  call void @nz.os_release()
  %gone = icmp eq ptr %s, null
  ret i1 %gone
}

; When a wait on `h` gives up, as the wall clock's nanoseconds; 0 for never.
define hidden i64 @nz.net_deadline(i64 %h) {
entry:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %never, label %read
read:
  %mp = getelementptr i8, ptr %s, i64 40
  %ms = load i64, ptr %mp
  call void @nz.os_release()
  %some = icmp sgt i64 %ms, 0
  br i1 %some, label %at, label %none
at:
  %now = call i64 @nz.net_now()
  %ns = mul i64 %ms, 1000000
  %d = add i64 %now, %ns
  ret i64 %d
never:
  call void @nz.os_release()
  br label %none
none:
  ret i64 0
}

; Wait until `h` may be ready in direction `dir` (1 reading, 2 writing): 0 to try again, the
; status of a deadline that passed (-ETIMEDOUT), or -65536 for a handle closed meanwhile.
define hidden i64 @nz.net_wait(i64 %h, i64 %dir, i64 %deadline) {
entry:
  %ts = alloca [16 x i8], align 8
  %w = call ptr @nz.net_waitrec(i64 %h)
  %none = icmp eq ptr %w, null
  br i1 %none, label %gone, label %arm
gone:
  ret i64 -65536
arm:
  call void @nz.net_start()
  %l = call i32 @pthread_mutex_lock(ptr %w)
  %seqp = getelementptr i8, ptr %w, i64 128
  %seq0 = load i64, ptr %seqp
  %interestp = getelementptr i8, ptr %w, i64 136
  %was = load i64, ptr %interestp
  %interest = or i64 %was, %dir
  store i64 %interest, ptr %interestp
  %qw = load i64, ptr @nz.net_poller
  %q = trunc i64 %qw to i32
  %fd = trunc i64 %h to i32
  %a = call i32 @nz_net_arm(i32 %q, i32 %fd, i64 %interest, i64 %h)
  %cv = getelementptr i8, ptr %w, i64 64
  br label %check
check:
  %seq = load i64, ptr %seqp
  %moved = icmp ne i64 %seq, %seq0
  br i1 %moved, label %ready, label %open
open:
  %closing = call i1 @nz.net_closing(i64 %h)
  br i1 %closing, label %closed, label %clock
clock:
  %bounded = icmp ne i64 %deadline, 0
  br i1 %bounded, label %timed, label %forever
forever:
  %r0 = call i32 @nz.cwait(ptr %cv, ptr %w)
  br label %check
timed:
  %now = call i64 @nz.net_now()
  %past = icmp sge i64 %now, %deadline
  br i1 %past, label %expired, label %sleep
sleep:
  %sec = sdiv i64 %deadline, 1000000000
  %nsec = srem i64 %deadline, 1000000000
  store i64 %sec, ptr %ts
  %np = getelementptr i8, ptr %ts, i64 8
  store i64 %nsec, ptr %np
  %r1 = call i32 @nz.ctimedwait(ptr %cv, ptr %w, ptr %ts)
  br label %check
ready:
  %u0 = call i32 @pthread_mutex_unlock(ptr %w)
  ret i64 0
closed:
  %u1 = call i32 @pthread_mutex_unlock(ptr %w)
  ret i64 -65536
expired:
  %u2 = call i32 @pthread_mutex_unlock(ptr %w)
  %t = call i64 @nz_net_etimedout()
  %neg = sub i64 0, %t
  ret i64 %neg
}

; Whether the failure just reported is "would block".
define hidden i1 @nz.net_would_block() {
entry:
  %ep = call ptr @nz_errno_location()
  %e = load i32, ptr %ep
  %w = sext i32 %e to i64
  %again = call i64 @nz_net_eagain()
  %is = icmp eq i64 %w, %again
  ret i1 %is
}

; A read from connected socket `h`, pinned by the caller, waiting at most its deadline.
define hidden i64 @nz.net_recv(i64 %h, ptr %buf, i64 %n) {
entry:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  %r = call i64 @nz_net_read(i32 %fd, ptr %buf, i64 %n)
  %ok = icmp sge i64 %r, 0
  br i1 %ok, label %done, label %failed
done:
  ret i64 %r
failed:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 1, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  ret i64 %w
}

; A write to connected socket `h`, pinned by the caller: the bytes the system took.
define hidden i64 @nz.net_send(i64 %h, ptr %buf, i64 %n) {
entry:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  %r = call i64 @nz_net_send(i32 %fd, ptr %buf, i64 %n)
  %ok = icmp sge i64 %r, 0
  br i1 %ok, label %done, label %failed
done:
  ret i64 %r
failed:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 2, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  ret i64 %w
}

; `HOST:PORT` (IPv6 bracketed) into the caller's 128 bytes: its length, or -22.
define hidden i64 @nz.net_parse(ptr %p, i64 %n, ptr %out) {
entry:
  br label %scan
scan:
  %i = phi i64 [ %n, %entry ], [ %i1, %step ]
  %any = icmp sgt i64 %i, 0
  br i1 %any, label %look, label %bad
look:
  %i1 = sub i64 %i, 1
  %cp = getelementptr i8, ptr %p, i64 %i1
  %c = load i8, ptr %cp
  %colon = icmp eq i8 %c, 58
  br i1 %colon, label %port, label %step
step:
  br label %scan
port:
  %first = add i64 %i1, 1
  %plen = sub i64 %n, %first
  %some = icmp sgt i64 %plen, 0
  %few = icmp sle i64 %plen, 5
  %shape = and i1 %some, %few
  br i1 %shape, label %digits, label %bad
digits:
  %j = phi i64 [ %first, %port ], [ %j1, %digit ]
  %v = phi i64 [ 0, %port ], [ %v1, %digit ]
  %end = icmp eq i64 %j, %n
  br i1 %end, label %ranged, label %next
next:
  %dp = getelementptr i8, ptr %p, i64 %j
  %d = load i8, ptr %dp
  %lo = icmp uge i8 %d, 48
  %hi = icmp ule i8 %d, 57
  %isd = and i1 %lo, %hi
  br i1 %isd, label %digit, label %bad
digit:
  %dz = zext i8 %d to i64
  %dv = sub i64 %dz, 48
  %v10 = mul i64 %v, 10
  %v1 = add i64 %v10, %dv
  %j1 = add i64 %j, 1
  br label %digits
ranged:
  %fits = icmp ule i64 %v, 65535
  br i1 %fits, label %host, label %bad
host:
  %c0 = load i8, ptr %p
  %bracket = icmp eq i8 %c0, 91
  br i1 %bracket, label %six, label %four
six:
  %close = sub i64 %i1, 1
  %long = icmp sgt i64 %close, 1
  br i1 %long, label %six2, label %bad
six2:
  %cbp = getelementptr i8, ptr %p, i64 %close
  %cb = load i8, ptr %cbp
  %closed = icmp eq i8 %cb, 93
  br i1 %closed, label %six3, label %bad
six3:
  %hp6 = getelementptr i8, ptr %p, i64 1
  %hn6 = sub i64 %close, 1
  br label %fill
four:
  %nonempty = icmp sgt i64 %i1, 0
  br i1 %nonempty, label %fill, label %bad
fill:
  %hp = phi ptr [ %hp6, %six3 ], [ %p, %four ]
  %hn = phi i64 [ %hn6, %six3 ], [ %i1, %four ]
  %v6 = phi i64 [ 1, %six3 ], [ 0, %four ]
  %cs = call ptr @nz.cstr(ptr %hp, i64 %hn)
  %nocopy = icmp eq ptr %cs, null
  br i1 %nocopy, label %bad, label %filled
filled:
  %len = call i64 @nz_net_fill(ptr %cs, i64 %v, i64 %v6, ptr %out)
  call void @free(ptr %cs)
  %ok = icmp sgt i64 %len, 0
  br i1 %ok, label %good, label %bad
good:
  ret i64 %len
bad:
  ret i64 -22
}

define hidden i64 @nz.os_tcp_listen(ptr %p, i64 %n) {
entry:
  %sa = alloca [128 x i8], align 8
  %len = call i64 @nz.net_parse(ptr %p, i64 %n, ptr %sa)
  %bad = icmp slt i64 %len, 0
  br i1 %bad, label %invalid, label %open
invalid:
  ret i64 %len
open:
  %fd = call i32 @nz_net_open(ptr %sa, i64 1, i64 1)
  %nofd = icmp slt i32 %fd, 0
  br i1 %nofd, label %fail, label %bind
fail:
  %e = call i64 @nz.os_errno()
  ret i64 %e
bind:
  %l = trunc i64 %len to i32
  %b = call i32 @nz_net_bind(i32 %fd, ptr %sa, i32 %l)
  %bfail = icmp slt i32 %b, 0
  br i1 %bfail, label %undo, label %listen
listen:
  %li = call i32 @nz_net_listen(i32 %fd)
  %lfail = icmp slt i32 %li, 0
  br i1 %lfail, label %undo, label %adopt
undo:
  %ue = call i64 @nz.os_errno()
  %c = call i32 @nz_os_close(i32 %fd)
  ret i64 %ue
adopt:
  %h = call i64 @nz.os_adopt(i32 %fd, i64 5, ptr null)
  ret i64 %h
}

define hidden i64 @nz.os_tcp_accept(i64 %h) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %failed, label %pinned
failed:
  ret i64 %k
pinned:
  %listener = icmp eq i64 %k, 5
  br i1 %listener, label %start, label %invalid
invalid:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
start:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  %c = call i32 @nz_net_accept(i32 %fd)
  %got = icmp sge i32 %c, 0
  br i1 %got, label %adopt, label %none
adopt:
  %p = call i32 @nz_net_prepare(i32 %c)
  call void @nz.os_unpin(i64 %h)
  %nh = call i64 @nz.os_adopt(i32 %c, i64 4, ptr null)
  ret i64 %nh
none:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  call void @nz.os_unpin(i64 %h)
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 1, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  call void @nz.os_unpin(i64 %h)
  ret i64 %w
}

define hidden i64 @nz.os_tcp_connect(ptr %p, i64 %n, i64 %ms) {
entry:
  %sa = alloca [128 x i8], align 8
  %len = call i64 @nz.net_parse(ptr %p, i64 %n, ptr %sa)
  %bad = icmp slt i64 %len, 0
  br i1 %bad, label %invalid, label %open
invalid:
  ret i64 %len
open:
  %fd = call i32 @nz_net_open(ptr %sa, i64 1, i64 0)
  %nofd = icmp slt i32 %fd, 0
  br i1 %nofd, label %fail, label %adopt
fail:
  %e = call i64 @nz.os_errno()
  ret i64 %e
adopt:
  %h = call i64 @nz.os_adopt(i32 %fd, i64 4, ptr null)
  %noh = icmp slt i64 %h, 0
  br i1 %noh, label %nohandle, label %connect
nohandle:
  ret i64 %h
connect:
  %r = call i64 @nz_net_connect(i32 %fd, ptr %sa, i64 %len)
  switch i64 %r, label %refused [
    i64 0, label %connected
    i64 1, label %under
  ]
connected:
  ret i64 %h
refused:
  %c0 = call i64 @nz.os_close(i64 %h)
  ret i64 %r
under:
  %bounded = icmp sgt i64 %ms, 0
  %now = call i64 @nz.net_now()
  %ns = mul i64 %ms, 1000000
  %at = add i64 %now, %ns
  %deadline = select i1 %bounded, i64 %at, i64 0
  br label %wait
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 2, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %settled, label %gaveup
gaveup:
  %c1 = call i64 @nz.os_close(i64 %h)
  ret i64 %w
settled:
  %err = call i64 @nz_net_error(i32 %fd)
  %ok = icmp eq i64 %err, 0
  br i1 %ok, label %connected, label %failed
failed:
  %c2 = call i64 @nz.os_close(i64 %h)
  %neg = sub i64 0, %err
  ret i64 %neg
}

define hidden i64 @nz.os_udp_bind(ptr %p, i64 %n) {
entry:
  %sa = alloca [128 x i8], align 8
  %len = call i64 @nz.net_parse(ptr %p, i64 %n, ptr %sa)
  %bad = icmp slt i64 %len, 0
  br i1 %bad, label %invalid, label %open
invalid:
  ret i64 %len
open:
  %fd = call i32 @nz_net_open(ptr %sa, i64 2, i64 0)
  %nofd = icmp slt i32 %fd, 0
  br i1 %nofd, label %fail, label %bind
fail:
  %e = call i64 @nz.os_errno()
  ret i64 %e
bind:
  %l = trunc i64 %len to i32
  %b = call i32 @nz_net_bind(i32 %fd, ptr %sa, i32 %l)
  %bfail = icmp slt i32 %b, 0
  br i1 %bfail, label %undo, label %adopt
undo:
  %ue = call i64 @nz.os_errno()
  %c = call i32 @nz_os_close(i32 %fd)
  ret i64 %ue
adopt:
  %h = call i64 @nz.os_adopt(i32 %fd, i64 6, ptr null)
  ret i64 %h
}

define hidden i64 @nz.os_udp_send_to(i64 %h, ptr %buf, i64 %n, ptr %ap, i64 %an) {
entry:
  %sa = alloca [128 x i8], align 8
  %len = call i64 @nz.net_parse(ptr %ap, i64 %an, ptr %sa)
  %bad = icmp slt i64 %len, 0
  br i1 %bad, label %invalid, label %pin
invalid:
  ret i64 %len
pin:
  %k = call i64 @nz.os_pin(i64 %h)
  %dead = icmp slt i64 %k, 0
  br i1 %dead, label %failed, label %pinned
failed:
  ret i64 %k
pinned:
  %udp = icmp eq i64 %k, 6
  br i1 %udp, label %start, label %wrong
wrong:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
start:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  %r = call i64 @nz_net_sendto(i32 %fd, ptr %buf, i64 %n, ptr %sa, i64 %len)
  %ok = icmp sge i64 %r, 0
  br i1 %ok, label %done, label %none
done:
  call void @nz.os_unpin(i64 %h)
  ret i64 %r
none:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  call void @nz.os_unpin(i64 %h)
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 2, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  call void @nz.os_unpin(i64 %h)
  ret i64 %w
}

define hidden i64 @nz.os_udp_recv_from(i64 %h, ptr %buf, i64 %n, ptr %from, i64 %fn) {
entry:
  %sa = alloca [128 x i8], align 8
  %slen = alloca i32, align 4
  %txt = alloca [80 x i8], align 8
  %k = call i64 @nz.os_pin(i64 %h)
  %dead = icmp slt i64 %k, 0
  br i1 %dead, label %failed, label %pinned
failed:
  ret i64 %k
pinned:
  %udp = icmp eq i64 %k, 6
  br i1 %udp, label %start, label %wrong
wrong:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
start:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  store i32 128, ptr %slen
  %r = call i64 @nz_net_recvfrom(i32 %fd, ptr %buf, i64 %n, ptr %sa, ptr %slen)
  %ok = icmp sge i64 %r, 0
  br i1 %ok, label %got, label %none
got:
  call void @llvm.memset.p0.i64(ptr %from, i8 0, i64 %fn, i1 false)
  %tn = call i64 @nz_net_format(ptr %sa, ptr %txt, i64 80)
  %short = icmp slt i64 %fn, %tn
  %cn = select i1 %short, i64 %fn, i64 %tn
  call void @llvm.memcpy.p0.p0.i64(ptr %from, ptr %txt, i64 %cn, i1 false)
  call void @nz.os_unpin(i64 %h)
  ret i64 %r
none:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  call void @nz.os_unpin(i64 %h)
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 1, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  call void @nz.os_unpin(i64 %h)
  ret i64 %w
}

define hidden i64 @nz.os_set_timeout(i64 %h, i64 %ms) {
entry:
  %neg = icmp slt i64 %h, 0
  br i1 %neg, label %failed, label %range
failed:
  ret i64 %h
range:
  %bad = icmp slt i64 %ms, 0
  br i1 %bad, label %invalid, label %lock
invalid:
  ret i64 -22
lock:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %closed, label %set
closed:
  call void @nz.os_release()
  ret i64 -65536
set:
  %mp = getelementptr i8, ptr %s, i64 40
  store i64 %ms, ptr %mp
  call void @nz.os_release()
  ret i64 0
}

define hidden i64 @nz.os_shutdown(i64 %h, i64 %how) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %dead = icmp slt i64 %k, 0
  br i1 %dead, label %failed, label %pinned
failed:
  ret i64 %k
pinned:
  %tcp = icmp eq i64 %k, 4
  %range = icmp ult i64 %how, 3
  %ok = and i1 %tcp, %range
  br i1 %ok, label %shut, label %invalid
invalid:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
shut:
  %fd = trunc i64 %h to i32
  %w = trunc i64 %how to i32
  %r = call i32 @nz_net_shutdown(i32 %fd, i32 %w)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %bad = icmp slt i32 %r, 0
  %out = select i1 %bad, i64 %e, i64 0
  ret i64 %out
}

; A socket's own address (`peer` 0) or its peer's (1) into `buf`: the length, 0 for none.
define hidden i64 @nz.os_net_addr(i64 %h, i64 %peer, ptr %buf, i64 %cap) {
entry:
  %sa = alloca [128 x i8], align 8
  %slen = alloca i32, align 4
  %k = call i64 @nz.os_pin(i64 %h)
  %dead = icmp slt i64 %k, 0
  br i1 %dead, label %none, label %pinned
none:
  ret i64 0
pinned:
  %tcp = icmp eq i64 %k, 4
  %listener = icmp eq i64 %k, 5
  %udp = icmp eq i64 %k, 6
  %socket0 = or i1 %tcp, %listener
  %socket = or i1 %socket0, %udp
  %wantpeer = icmp ne i64 %peer, 0
  %ok = select i1 %wantpeer, i1 %tcp, i1 %socket
  br i1 %ok, label %ask, label %unpinned
unpinned:
  call void @nz.os_unpin(i64 %h)
  ret i64 0
ask:
  store i32 128, ptr %slen
  %fd = trunc i64 %h to i32
  br i1 %wantpeer, label %peerof, label %own
peerof:
  %rp = call i32 @nz_net_name(i32 %fd, i32 1, ptr %sa, ptr %slen)
  br label %asked
own:
  %ro = call i32 @nz_net_name(i32 %fd, i32 0, ptr %sa, ptr %slen)
  br label %asked
asked:
  %r = phi i32 [ %rp, %peerof ], [ %ro, %own ]
  call void @nz.os_unpin(i64 %h)
  %bad = icmp slt i32 %r, 0
  br i1 %bad, label %none, label %format
format:
  %n = call i64 @nz_net_format(ptr %sa, ptr %buf, i64 %cap)
  ret i64 %n
}

define hidden i64 @nz.os_resolve(ptr %p, i64 %n, i64 %port, ptr %out, i64 %cap) {
entry:
  %range = icmp ult i64 %port, 65536
  br i1 %range, label %copy, label %invalid
invalid:
  ret i64 -22
copy:
  %c = call ptr @nz.cstr(ptr %p, i64 %n)
  %none = icmp eq ptr %c, null
  br i1 %none, label %oom, label %ask
oom:
  ret i64 -12
ask:
  %r = call i64 @nz_net_resolve(ptr %c, i64 %port, ptr %out, i64 %cap)
  call void @free(ptr %c)
  ret i64 %r
}
"#;

/// Child processes (Gate 2, `docs/spec.md`, *Process and environment*): `posix_spawnp` with three
/// pipes whose parent ends are non-blocking handles that wait as sockets do. A child's handle is
/// keyed by a descriptor it holds on `/dev/null` and names a record — its process reference (0),
/// its status once waited for (8, -1 before), its three pipes' handles (16, 24, 32), and (R1d) its
/// process id (40) and its peak resident memory once waited for (48). A program that runs another
/// ignores `SIGPIPE` from then on, as the interpreter's host always does, so writing to a child that
/// has gone is `BrokenPipe` and never a signal.
///
/// R1d (`docs/spec.md`, *The platform, further*) adds a child's directory and environment — the
/// environment ordered by name, so every implementation hands a child the same one — a new process
/// group and a null standard input, bounded waits that look again at most every 10 ms, a signal to
/// a group, and this process's id.
pub(crate) const PROCESS_RUNTIME: &str = r#"
; A write to a pipe, pinned by the caller: the bytes the system took, waiting at most its deadline.
define hidden i64 @nz.net_pipe_write(i64 %h, ptr %buf, i64 %n) {
entry:
  %deadline = call i64 @nz.net_deadline(i64 %h)
  %fd = trunc i64 %h to i32
  br label %try
try:
  %r = call i64 @nz_net_write(i32 %fd, ptr %buf, i64 %n)
  %ok = icmp sge i64 %r, 0
  br i1 %ok, label %done, label %failed
done:
  ret i64 %r
failed:
  %e = call i64 @nz.os_errno()
  %block = call i1 @nz.net_would_block()
  br i1 %block, label %wait, label %error
error:
  ret i64 %e
wait:
  %w = call i64 @nz.net_wait(i64 %h, i64 2, i64 %deadline)
  %go = icmp eq i64 %w, 0
  br i1 %go, label %try, label %stop
stop:
  ret i64 %w
}

; Run `program` with the arguments in `args` (a `Strs`): the child's handle, or -errno.
define hidden i64 @nz.os_spawn(ptr %pp, i64 %pn, ptr %args) {
entry:
  %h = call i64 @nz.os_start(ptr %pp, i64 %pn, ptr %args, ptr null, ptr null, i64 0)
  ret i64 %h
}

; R1d: `program` in directory `dir` (empty: this process's) with exactly the environment `env` (a
; `Strs` of `NAME=value`), and `flags` — 1 a new process group, 2 a null standard input. The
; child's handle, or -errno; -EINVAL, nothing started, for another flag, an entry that is not one,
; a name given twice, or a zero byte in `dir` or an entry. A relative program named with a `/` is
; made absolute beside the canonical `dir` first: systems differ on whether a child's directory is
; changed before its program is looked for, and this way none has to choose.
define hidden i64 @nz.os_spawn_in(ptr %pp, i64 %pn, ptr %args, ptr %dp, i64 %dn, ptr %env, i64 %flags) {
entry:
  %status = alloca i64, align 8
  %extra = and i64 %flags, -4
  %unknown = icmp ne i64 %extra, 0
  br i1 %unknown, label %invalid, label %dircheck
invalid:
  ret i64 -22
dircheck:
  %nuldir = call i1 @nz.os_has_byte(ptr %dp, i64 %dn, i8 0)
  br i1 %nuldir, label %invalid, label %environment
environment:
  store i64 0, ptr %status
  %envp = call ptr @nz.os_envp(ptr %env, ptr %status)
  %noenv = icmp eq ptr %envp, null
  br i1 %noenv, label %refused, label %where
refused:
  %why = load i64, ptr %status
  ret i64 %why
where:
  %here = icmp eq i64 %dn, 0
  br i1 %here, label %start, label %copydir
copydir:
  %d = call ptr @nz.cstr(ptr %dp, i64 %dn)
  %nod = icmp eq ptr %d, null
  br i1 %nod, label %oom, label %isdir
oom:
  call void @nz.os_envp_free(ptr %envp)
  ret i64 -12
isdir:
  ; The directory is asked about first, so a missing one is `NotFound` and a file
  ; `NotADirectory` on every system, whatever its `posix_spawnp` folds them into.
  %facts = alloca [32 x i8], align 8
  %st = call i32 @nz_os_stat(ptr %d, ptr %facts)
  %ste = call i64 @nz.os_errno()
  %nostat = icmp slt i32 %st, 0
  br i1 %nostat, label %baddir, label %kindcheck
kindcheck:
  %kp = getelementptr i8, ptr %facts, i64 8
  %k = load i64, ptr %kp
  %dirkind = icmp eq i64 %k, 1
  br i1 %dirkind, label %start, label %notdir
notdir:
  br label %baddir
baddir:
  %why2 = phi i64 [ %ste, %isdir ], [ -20, %notdir ]
  call void @free(ptr %d)
  call void @nz.os_envp_free(ptr %envp)
  ret i64 %why2
start:
  %dir = phi ptr [ null, %where ], [ %d, %kindcheck ]
  %slash = call i1 @nz.os_has_byte(ptr %pp, i64 %pn, i8 47)
  %elsewhere = icmp ne ptr %dir, null
  %maybe = and i1 %slash, %elsewhere
  br i1 %maybe, label %rootcheck, label %go
rootcheck:
  ; A `/` was found, so the program has a first byte.
  %first = load i8, ptr %pp
  %rooted = icmp eq i8 %first, 47
  br i1 %rooted, label %go, label %join
join:
  %joined = call ptr @nz.os_beside(ptr %dir, ptr %pp, i64 %pn, ptr %status)
  %nojoin = icmp eq ptr %joined, null
  %jl = load i64, ptr %status
  br i1 %nojoin, label %unjoined, label %go
unjoined:
  call void @free(ptr %dir)
  call void @nz.os_envp_free(ptr %envp)
  ret i64 %jl
go:
  %prog = phi ptr [ %pp, %start ], [ %pp, %rootcheck ], [ %joined, %join ]
  %progn = phi i64 [ %pn, %start ], [ %pn, %rootcheck ], [ %jl, %join ]
  %beside = phi i1 [ false, %start ], [ false, %rootcheck ], [ true, %join ]
  %h = call i64 @nz.os_start(ptr %prog, i64 %progn, ptr %args, ptr %dir, ptr %envp, i64 %flags)
  br i1 %beside, label %freejoin, label %done
freejoin:
  call void @free(ptr %prog)
  br label %done
done:
  call void @free(ptr %dir)
  call void @nz.os_envp_free(ptr %envp)
  ret i64 %h
}

; Program `pp` (`pn` bytes) beside directory `dir` (a C string), made absolute: the canonical
; `dir`, a `/` and the program, in a new buffer; its length through `lenp`. Null with -errno there
; when `dir` has no canonical path.
define hidden ptr @nz.os_beside(ptr %dir, ptr %pp, i64 %pn, ptr %lenp) {
entry:
  %room = add i64 %pn, 4098
  %buf = call ptr @malloc(i64 %room)
  %none = icmp eq ptr %buf, null
  br i1 %none, label %oom, label %ask
oom:
  store i64 -12, ptr %lenp
  ret ptr null
ask:
  %n = call i64 @nz_os_realpath(ptr %dir, ptr %buf, i64 4096)
  %e = call i64 @nz.os_errno()
  %failed = icmp slt i64 %n, 0
  br i1 %failed, label %fail, label %fits
fail:
  call void @free(ptr %buf)
  store i64 %e, ptr %lenp
  ret ptr null
fits:
  %long = icmp sgt i64 %n, 4096
  br i1 %long, label %toolong, label %put
toolong:
  call void @free(ptr %buf)
  store i64 -22, ptr %lenp
  ret ptr null
put:
  %sp = getelementptr i8, ptr %buf, i64 %n
  store i8 47, ptr %sp
  %at = add i64 %n, 1
  %dst = getelementptr i8, ptr %buf, i64 %at
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %pp, i64 %pn, i1 false)
  %len = add i64 %at, %pn
  store i64 %len, ptr %lenp
  ret ptr %buf
}

; Whether `n` bytes at `p` hold byte `b`.
define hidden i1 @nz.os_has_byte(ptr %p, i64 %n, i8 %b) {
entry:
  br label %scan
scan:
  %i = phi i64 [ 0, %entry ], [ %i1, %next ]
  %end = icmp sge i64 %i, %n
  br i1 %end, label %none, label %look
look:
  %cp = getelementptr i8, ptr %p, i64 %i
  %c = load i8, ptr %cp
  %hit = icmp eq i8 %c, %b
  br i1 %hit, label %found, label %next
next:
  %i1 = add i64 %i, 1
  br label %scan
found:
  ret i1 true
none:
  ret i1 false
}

; Two environment entries by name, byte by byte, a name before every longer one it begins:
; negative, zero (the same name) or positive.
define hidden i64 @nz.os_env_order(ptr %a, ptr %b) {
entry:
  %na = call i64 @nz.os_env_split(ptr %a)
  %nb = call i64 @nz.os_env_split(ptr %b)
  %shorter = icmp slt i64 %na, %nb
  %m = select i1 %shorter, i64 %na, i64 %nb
  %d = call i32 @memcmp(ptr %a, ptr %b, i64 %m)
  %differ = icmp ne i32 %d, 0
  %dw = sext i32 %d to i64
  %bylen = sub i64 %na, %nb
  %r = select i1 %differ, i64 %dw, i64 %bylen
  ret i64 %r
}

; `env` (a `Strs`) as a null-terminated array of C strings ordered by name, each checked to be an
; entry with no zero byte and no name repeated; null, with -EINVAL or -ENOMEM at `status`, when
; one is not.
define hidden ptr @nz.os_envp(ptr %env, ptr %status) {
entry:
  %count = load i64, ptr %env
  %slots = add i64 %count, 1
  %bytes = mul i64 %slots, 8
  %arr = call ptr @malloc(i64 %bytes)
  %noarr = icmp eq ptr %arr, null
  br i1 %noarr, label %oom, label %start
oom:
  store i64 -12, ptr %status
  ret ptr null
start:
  store ptr null, ptr %arr
  %data = getelementptr i8, ptr %env, i64 16
  %elems = load ptr, ptr %data
  br label %each
each:
  %i = phi i64 [ 0, %start ], [ %i1, %placed ]
  %more = icmp slt i64 %i, %count
  br i1 %more, label %one, label %done
one:
  %off = mul i64 %i, 24
  %ep = getelementptr i8, ptr %elems, i64 %off
  %sp = load ptr, ptr %ep
  %lp = getelementptr i8, ptr %ep, i64 8
  %sl = load i64, ptr %lp
  %nul = call i1 @nz.os_has_byte(ptr %sp, i64 %sl, i8 0)
  br i1 %nul, label %bad, label %copy
copy:
  %c = call ptr @nz.cstr(ptr %sp, i64 %sl)
  %noc = icmp eq ptr %c, null
  br i1 %noc, label %nomem, label %entry1
nomem:
  store i64 -12, ptr %status
  br label %undo
entry1:
  %at = call i64 @nz.os_env_split(ptr %c)
  %isentry = icmp sgt i64 %at, 0
  br i1 %isentry, label %insert, label %badc
badc:
  call void @free(ptr %c)
  br label %bad
bad:
  store i64 -22, ptr %status
  br label %undo
insert:
  br label %shift
shift:
  %j = phi i64 [ %i, %insert ], [ %jm, %moved ]
  %top = icmp eq i64 %j, 0
  br i1 %top, label %put, label %compare
compare:
  %jm = sub i64 %j, 1
  %prevp = getelementptr ptr, ptr %arr, i64 %jm
  %prev = load ptr, ptr %prevp
  %o = call i64 @nz.os_env_order(ptr %prev, ptr %c)
  %same = icmp eq i64 %o, 0
  br i1 %same, label %twice, label %after
twice:
  ; The name is there already: undo the shift so far, then refuse.
  call void @nz.os_env_unshift(ptr %arr, i64 %j, i64 %i)
  call void @free(ptr %c)
  br label %bad
after:
  %later = icmp sgt i64 %o, 0
  br i1 %later, label %move, label %put
move:
  %dst = getelementptr ptr, ptr %arr, i64 %j
  store ptr %prev, ptr %dst
  br label %moved
moved:
  br label %shift
put:
  %slot = getelementptr ptr, ptr %arr, i64 %j
  store ptr %c, ptr %slot
  %i1 = add i64 %i, 1
  %endp = getelementptr ptr, ptr %arr, i64 %i1
  store ptr null, ptr %endp
  br label %placed
placed:
  br label %each
undo:
  call void @nz.os_envp_free(ptr %arr)
  ret ptr null
done:
  ret ptr %arr
}

; Entries `from + 1 ..= to` of `arr` moved back down one place: an insertion's shift undone, so the
; array is again its first `to` entries, ordered, ending in null.
define hidden void @nz.os_env_unshift(ptr %arr, i64 %from, i64 %to) {
entry:
  br label %loop
loop:
  %k = phi i64 [ %from, %entry ], [ %k1, %step ]
  %more = icmp slt i64 %k, %to
  br i1 %more, label %step, label %end
step:
  %k1 = add i64 %k, 1
  %srcp = getelementptr ptr, ptr %arr, i64 %k1
  %v = load ptr, ptr %srcp
  %dstp = getelementptr ptr, ptr %arr, i64 %k
  store ptr %v, ptr %dstp
  br label %loop
end:
  %lastp = getelementptr ptr, ptr %arr, i64 %to
  store ptr null, ptr %lastp
  ret void
}

; An array `nz.os_envp` made, and every string in it, freed; null is a no-op.
define hidden void @nz.os_envp_free(ptr %arr) {
entry:
  %none = icmp eq ptr %arr, null
  br i1 %none, label %done, label %walk
walk:
  %k = phi i64 [ 0, %entry ], [ %k1, %one ]
  %p = getelementptr ptr, ptr %arr, i64 %k
  %s = load ptr, ptr %p
  %end = icmp eq ptr %s, null
  br i1 %end, label %all, label %one
one:
  call void @free(ptr %s)
  %k1 = add i64 %k, 1
  br label %walk
all:
  call void @free(ptr %arr)
  br label %done
done:
  ret void
}

; Start `program` with `args`, in `dir` (null: here), with `envp` (null: this process's), by
; `flags`: the child's handle, or -errno. A child's record: its process reference (0), its status
; once waited for (8, -1 before), its three pipes' handles (16, 24, 32), its process id (40) and its
; peak resident memory once waited for (48).
define hidden i64 @nz.os_start(ptr %pp, i64 %pn, ptr %args, ptr %dir, ptr %envp, i64 %flags) {
entry:
  %ends = alloca [3 x i32], align 4
  %pid = alloca i64, align 8
  %prog = call ptr @nz.cstr(ptr %pp, i64 %pn)
  %noprog = icmp eq ptr %prog, null
  br i1 %noprog, label %oom, label %argv
oom:
  ret i64 -12
argv:
  %count = load i64, ptr %args
  %slots = add i64 %count, 2
  %bytes = mul i64 %slots, 8
  %av = call ptr @malloc(i64 %bytes)
  store ptr %prog, ptr %av
  %data = getelementptr i8, ptr %args, i64 16
  %elems = load ptr, ptr %data
  br label %copy
copy:
  %i = phi i64 [ 0, %argv ], [ %i1, %copied ]
  %more = icmp slt i64 %i, %count
  br i1 %more, label %one, label %terminate
one:
  %off = mul i64 %i, 24
  %ep = getelementptr i8, ptr %elems, i64 %off
  %sp = load ptr, ptr %ep
  %lp = getelementptr i8, ptr %ep, i64 8
  %sl = load i64, ptr %lp
  %c = call ptr @nz.cstr(ptr %sp, i64 %sl)
  %i1 = add i64 %i, 1
  %slot = getelementptr ptr, ptr %av, i64 %i1
  store ptr %c, ptr %slot
  br label %copied
copied:
  br label %copy
terminate:
  %last = add i64 %count, 1
  %endp = getelementptr ptr, ptr %av, i64 %last
  store ptr null, ptr %endp
  %r = call i32 @nz_os_spawn_in(ptr %pid, ptr %prog, ptr %av, ptr %envp, ptr %dir, i64 %flags, ptr %ends)
  br label %free
free:
  %k = phi i64 [ 0, %terminate ], [ %k1, %free ]
  %fsp = getelementptr ptr, ptr %av, i64 %k
  %fs = load ptr, ptr %fsp
  call void @free(ptr %fs)
  %k1 = add i64 %k, 1
  %freemore = icmp sle i64 %k1, %count
  br i1 %freemore, label %free, label %freed
freed:
  call void @free(ptr %av)
  %failed = icmp ne i32 %r, 0
  br i1 %failed, label %undo, label %started
undo:
  %rw = sext i32 %r to i64
  %neg = sub i64 0, %rw
  ret i64 %neg
started:
  %in1p = getelementptr i32, ptr %ends, i64 0
  %in1 = load i32, ptr %in1p
  %out0p = getelementptr i32, ptr %ends, i64 1
  %out0 = load i32, ptr %out0p
  %err0p = getelementptr i32, ptr %ends, i64 2
  %err0 = load i32, ptr %err0p
  %q1 = call i32 @nz_net_prepare(i32 %out0)
  %q2 = call i32 @nz_net_prepare(i32 %err0)
  %rec = call ptr @malloc(i64 64)
  %pidw = load i64, ptr %pid
  store i64 %pidw, ptr %rec
  %stp = getelementptr i8, ptr %rec, i64 8
  store i64 -1, ptr %stp
  %idp = getelementptr i8, ptr %rec, i64 40
  %id = call i64 @nz_os_child_id(i64 %pidw)
  store i64 %id, ptr %idp
  %rssp = getelementptr i8, ptr %rec, i64 48
  store i64 -1, ptr %rssp
  ; A null standard input (R1d) has no pipe: its handle is one already closed.
  %nullin = icmp slt i32 %in1, 0
  br i1 %nullin, label %noinput, label %input
noinput:
  br label %outputs
input:
  %q0 = call i32 @nz_net_prepare(i32 %in1)
  %hin1 = call i64 @nz.os_adopt(i32 %in1, i64 9, ptr null)
  br label %outputs
outputs:
  %hin = phi i64 [ -65536, %noinput ], [ %hin1, %input ]
  %hout = call i64 @nz.os_adopt(i32 %out0, i64 7, ptr null)
  %herr = call i64 @nz.os_adopt(i32 %err0, i64 7, ptr null)
  %hinp = getelementptr i8, ptr %rec, i64 16
  store i64 %hin, ptr %hinp
  %houtp = getelementptr i8, ptr %rec, i64 24
  store i64 %hout, ptr %houtp
  %herrp = getelementptr i8, ptr %rec, i64 32
  store i64 %herr, ptr %herrp
  %key = call i32 @nz_os_key()
  %h = call i64 @nz.os_adopt(i32 %key, i64 8, ptr %rec)
  ret i64 %h
}

; The record of child `h`, pinned; null (and unpinned) for a handle that is not a child.
define hidden ptr @nz.os_child(i64 %h) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %child = icmp eq i64 %k, 8
  br i1 %child, label %find, label %other
other:
  %pinned = icmp sgt i64 %k, 0
  br i1 %pinned, label %unpin, label %none
unpin:
  call void @nz.os_unpin(i64 %h)
  br label %none
none:
  ret ptr null
find:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %objp = getelementptr i8, ptr %s, i64 24
  %rec = load ptr, ptr %objp
  call void @nz.os_release()
  ret ptr %rec
}

define hidden i64 @nz.os_child_pipe(i64 %h, i64 %which) {
entry:
  %live = call i64 @nz.os_status(i64 %h)
  %dead = icmp slt i64 %live, 0
  br i1 %dead, label %failed, label %look
failed:
  ret i64 %live
look:
  %rec = call ptr @nz.os_child(i64 %h)
  %none = icmp eq ptr %rec, null
  br i1 %none, label %invalid, label %range
invalid:
  ret i64 -22
range:
  %ok = icmp ult i64 %which, 3
  br i1 %ok, label %pick, label %wrong
wrong:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
pick:
  %idx = add i64 %which, 2
  %off = mul i64 %idx, 8
  %pp = getelementptr i8, ptr %rec, i64 %off
  %p = load i64, ptr %pp
  call void @nz.os_unpin(i64 %h)
  ret i64 %p
}

; The child's exit: its code, or 256 and the signal that ended it; the same every time.
define hidden i64 @nz.os_wait(i64 %h) {
entry:
  %r = call i64 @nz.os_wait_for(i64 %h, i64 0, i64 0)
  ret i64 %r
}

; R1d: `os_wait`'s answer if the child has ended, -EWOULDBLOCK at once if it has not.
define hidden i64 @nz.os_try_wait(i64 %h) {
entry:
  %r = call i64 @nz.os_wait_for(i64 %h, i64 1, i64 0)
  ret i64 %r
}

; R1d: `os_wait`'s answer if the child ends before the monotonic clock reads `deadline`
; milliseconds, -ETIMEDOUT then, the child left running.
define hidden i64 @nz.os_wait_until(i64 %h, i64 %deadline) {
entry:
  %r = call i64 @nz.os_wait_for(i64 %h, i64 2, i64 %deadline)
  ret i64 %r
}

; The monotonic clock `time_now_ms` reads, in milliseconds.
define hidden i64 @nz.os_mono_ms() {
entry:
  %tp = alloca [2 x i64], align 8
  %r = call i32 @nz_os_clock(i32 4, ptr %tp)
  %sec = load i64, ptr %tp
  %tpn = getelementptr i8, ptr %tp, i64 8
  %nsec = load i64, ptr %tpn
  %msec = mul i64 %sec, 1000
  %frac = sdiv i64 %nsec, 1000000
  %t = add i64 %msec, %frac
  ret i64 %t
}

; A wait for child `h`: `mode` 0 until it ends, 1 not at all, 2 until `deadline`, looking again at
; most every 10 ms. Its end is kept, with its peak memory, so every later wait answers the same.
define hidden i64 @nz.os_wait_for(i64 %h, i64 %mode, i64 %deadline) {
entry:
  %live = call i64 @nz.os_status(i64 %h)
  %dead = icmp slt i64 %live, 0
  br i1 %dead, label %failed, label %look
failed:
  ret i64 %live
look:
  %rec = call ptr @nz.os_child(i64 %h)
  %none = icmp eq ptr %rec, null
  br i1 %none, label %invalid, label %known
invalid:
  ret i64 -22
known:
  %stp = getelementptr i8, ptr %rec, i64 8
  %was = load i64, ptr %stp
  %done = icmp sge i64 %was, 0
  br i1 %done, label %again, label %waitkind
again:
  call void @nz.os_unpin(i64 %h)
  ret i64 %was
waitkind:
  %pid = load i64, ptr %rec
  %rssp = getelementptr i8, ptr %rec, i64 48
  %blocks = icmp eq i64 %mode, 0
  br i1 %blocks, label %block, label %poll
block:
  %out = call i64 @nz_os_reap(i64 %pid, i32 1, ptr %rssp)
  br label %ended
poll:
  %got = call i64 @nz_os_reap(i64 %pid, i32 0, ptr %rssp)
  %eagain = call i64 @nz_net_eagain()
  %notyet = sub i64 0, %eagain
  %running = icmp eq i64 %got, %notyet
  br i1 %running, label %pending, label %ended
pending:
  %once = icmp eq i64 %mode, 1
  br i1 %once, label %wouldblock, label %clock
wouldblock:
  call void @nz.os_unpin(i64 %h)
  ret i64 %notyet
clock:
  %now = call i64 @nz.os_mono_ms()
  %passed = icmp sge i64 %now, %deadline
  br i1 %passed, label %timedout, label %nap
timedout:
  call void @nz.os_unpin(i64 %h)
  %et = call i64 @nz_net_etimedout()
  %net = sub i64 0, %et
  ret i64 %net
nap:
  %left = sub i64 %deadline, %now
  %long = icmp sgt i64 %left, 10
  %ms = select i1 %long, i64 10, i64 %left
  call void @nz_os_nap(i64 %ms)
  br label %poll
ended:
  %r = phi i64 [ %out, %block ], [ %got, %poll ]
  %bad = icmp slt i64 %r, 0
  br i1 %bad, label %give, label %decode
give:
  call void @nz.os_unpin(i64 %h)
  ret i64 %r
decode:
  store i64 %r, ptr %stp
  call void @nz.os_unpin(i64 %h)
  ret i64 %r
}

; R1d: a child's process id, kept from its start.
define hidden i64 @nz.os_child_pid(i64 %h) {
entry:
  %live = call i64 @nz.os_status(i64 %h)
  %dead = icmp slt i64 %live, 0
  br i1 %dead, label %failed, label %look
failed:
  ret i64 %live
look:
  %rec = call ptr @nz.os_child(i64 %h)
  %none = icmp eq ptr %rec, null
  br i1 %none, label %invalid, label %known
invalid:
  ret i64 -22
known:
  %idp = getelementptr i8, ptr %rec, i64 40
  %id = load i64, ptr %idp
  call void @nz.os_unpin(i64 %h)
  ret i64 %id
}

; R1d: a waited child's peak resident memory in bytes; -EINVAL before it has ended.
define hidden i64 @nz.os_child_rusage(i64 %h) {
entry:
  %live = call i64 @nz.os_status(i64 %h)
  %dead = icmp slt i64 %live, 0
  br i1 %dead, label %failed, label %look
failed:
  ret i64 %live
look:
  %rec = call ptr @nz.os_child(i64 %h)
  %none = icmp eq ptr %rec, null
  br i1 %none, label %invalid, label %known
invalid:
  ret i64 -22
known:
  %stp = getelementptr i8, ptr %rec, i64 8
  %was = load i64, ptr %stp
  %running = icmp slt i64 %was, 0
  %rssp = getelementptr i8, ptr %rec, i64 48
  %rss = load i64, ptr %rssp
  call void @nz.os_unpin(i64 %h)
  %out = select i1 %running, i64 -22, i64 %rss
  ret i64 %out
}

; R1d: this process's id.
define hidden i64 @nz.os_pid() {
entry:
  %p = call i64 @nz_os_pid()
  ret i64 %p
}

; R1d: signal `sig` to every process of group `pgid`: 0, or -errno; -EINVAL, never sent, for a
; group below 2 or a signal outside 0..=31.
define hidden i64 @nz.os_kill_group(i64 %pgid, i64 %sig) {
entry:
  %group = icmp sgt i64 %pgid, 1
  %small = icmp slt i64 %pgid, 2147483648
  %sigok = icmp ult i64 %sig, 32
  %g = and i1 %group, %small
  %ok = and i1 %g, %sigok
  br i1 %ok, label %send, label %invalid
invalid:
  ret i64 -22
send:
  %r = call i32 @nz_os_signal_group(i64 %pgid, i64 %sig)
  %e = call i64 @nz.os_errno()
  %bad = icmp slt i32 %r, 0
  %out = select i1 %bad, i64 %e, i64 0
  ret i64 %out
}

define hidden i64 @nz.os_kill(i64 %h) {
entry:
  %live = call i64 @nz.os_status(i64 %h)
  %dead = icmp slt i64 %live, 0
  br i1 %dead, label %failed, label %look
failed:
  ret i64 %live
look:
  %rec = call ptr @nz.os_child(i64 %h)
  %none = icmp eq ptr %rec, null
  br i1 %none, label %invalid, label %known
invalid:
  ret i64 -22
known:
  %stp = getelementptr i8, ptr %rec, i64 8
  %was = load i64, ptr %stp
  %done = icmp sge i64 %was, 0
  br i1 %done, label %already, label %kill
already:
  call void @nz.os_unpin(i64 %h)
  ret i64 0
kill:
  %pid = load i64, ptr %rec
  %r = call i32 @nz_os_terminate(i64 %pid)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %bad = icmp slt i32 %r, 0
  %out = select i1 %bad, i64 %e, i64 0
  ret i64 %out
}
"#;
