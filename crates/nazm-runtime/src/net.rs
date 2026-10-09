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
//! the handle's (`os_set_timeout`), as an absolute time the timed wait gives up at; a close marks
//! the handle closing and wakes its waiters the same way, and each answers `Closed`.
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

; The wall clock in nanoseconds: what `nz.ctimedwait`'s deadline is measured against.
define hidden i64 @nz.net_now() {
entry:
  %ts = alloca [16 x i8], align 8
  %r = call i32 @clock_gettime(i32 0, ptr %ts)
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
  %r = call i64 @read(i32 %fd, ptr %buf, i64 %n)
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
  %b = call i32 @bind(i32 %fd, ptr %sa, i32 %l)
  %bfail = icmp slt i32 %b, 0
  br i1 %bfail, label %undo, label %listen
listen:
  %li = call i32 @listen(i32 %fd, i32 128)
  %lfail = icmp slt i32 %li, 0
  br i1 %lfail, label %undo, label %adopt
undo:
  %ue = call i64 @nz.os_errno()
  %c = call i32 @close(i32 %fd)
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
  %c = call i32 @accept(i32 %fd, ptr null, ptr null)
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
  %b = call i32 @bind(i32 %fd, ptr %sa, i32 %l)
  %bfail = icmp slt i32 %b, 0
  br i1 %bfail, label %undo, label %adopt
undo:
  %ue = call i64 @nz.os_errno()
  %c = call i32 @close(i32 %fd)
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
  %r = call i64 @recvfrom(i32 %fd, ptr %buf, i64 %n, i32 0, ptr %sa, ptr %slen)
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
  %r = call i32 @shutdown(i32 %fd, i32 %w)
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
  %rp = call i32 @getpeername(i32 %fd, ptr %sa, ptr %slen)
  br label %asked
own:
  %ro = call i32 @getsockname(i32 %fd, ptr %sa, ptr %slen)
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
