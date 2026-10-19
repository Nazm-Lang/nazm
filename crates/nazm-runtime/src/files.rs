//! Files and handles (Gate 2, `docs/user/spec.md`, *Files and handles*): the `os_` built-ins' entries,
//! as target-independent LLVM text over the per-target unit `crate::platform::emit_os` defines.
//!
//! # The handle table
//!
//! A handle is a word, `generation << 32 | descriptor`. The table, indexed by descriptor, holds
//! each descriptor's current generation, how many operations are using it, its state — a file
//! (1), a directory (2), standard input (3), standard output or errors (10, R1d), or nothing (0),
//! with 256 added once a close is waiting — and, for a directory, its `DIR *`. Every operation **pins** its handle first: the
//! generation must be the slot's and the state live, and the use count goes up; it unpins after.
//! A close of a pinned handle marks it closing and the last unpin closes the descriptor, so no
//! operation ever runs on a descriptor the system has since given to another open. A failed open
//! answers its status as the handle: negative, never a live handle's word. A handle that is closed
//! answers -65536, a number no system gives an error, which `@std/ioerror` calls `Closed`.
//!
//! The table is guarded by a spin lock — one word, no initialisation, held only for a few loads
//! and stores and a `realloc` — and by nothing while the system call runs. A directory's
//! `readdir` is the exception: the lock is held across it, since one `DIR *` is not safe to read
//! from two threads, and it is a read of a local directory.

/// The runtime text: a slot is 64 bytes — generation (0), use count (8), state (16), `DIR *`
/// (24), and for a socket its wait record (32) and deadline in milliseconds (40).
pub(crate) const FILES_RUNTIME: &str = r#"
@nz.os_lock = hidden global i32 0
@nz.os_slots = hidden global ptr null
@nz.os_cap = hidden global i64 0
@nz.os_stdin_h = hidden global i64 0
@nz.os_in_buf = hidden global ptr null
@nz.os_in_at = hidden global i64 0
@nz.os_in_end = hidden global i64 0
@nz.os_in_lock = hidden global i32 0
@nz.os_stdout_h = hidden global i64 0
@nz.os_stderr_h = hidden global i64 0

declare i32 @nz_os_open(ptr, i64)
declare i32 @nz_os_mkdir(ptr)
declare i32 @nz_os_stat(ptr, ptr)
declare ptr @nz_os_opendir(ptr)
declare i64 @nz_os_readdir(ptr, ptr, i64)
declare i64 @nz_os_family()
declare i64 @nz_os_error_text(i64, ptr, i64)

define hidden void @nz.os_acquire() noinline {
entry:
  br label %spin
spin:
  %old = atomicrmw xchg ptr @nz.os_lock, i32 1 acquire
  %free = icmp eq i32 %old, 0
  br i1 %free, label %held, label %spin
held:
  ret void
}

define hidden void @nz.os_release() {
entry:
  store atomic i32 0, ptr @nz.os_lock release, align 4
  ret void
}

; -errno, read the moment a system call has failed.
define hidden i64 @nz.os_errno() {
entry:
  %p = call ptr @nz_errno_location()
  %e = load i32, ptr %p
  %w = sext i32 %e to i64
  %neg = sub i64 0, %w
  ret i64 %neg
}

; The slot for descriptor `fd`, growing the table to hold it; null when memory is refused. The
; lock is held.
define hidden ptr @nz.os_slot(i64 %fd) {
entry:
  %cap = load i64, ptr @nz.os_cap
  %fits = icmp slt i64 %fd, %cap
  br i1 %fits, label %at, label %grow
grow:
  %twice = shl i64 %cap, 1
  %need = add i64 %fd, 1
  %more = icmp sgt i64 %need, %twice
  %want = select i1 %more, i64 %need, i64 %twice
  %small = icmp slt i64 %want, 64
  %new = select i1 %small, i64 64, i64 %want
  %bytes = mul i64 %new, 64
  %old = load ptr, ptr @nz.os_slots
  %p = call ptr @realloc(ptr %old, i64 %bytes)
  %none = icmp eq ptr %p, null
  br i1 %none, label %refused, label %zero
refused:
  ret ptr null
zero:
  %from = mul i64 %cap, 64
  %start = getelementptr i8, ptr %p, i64 %from
  %rest = sub i64 %bytes, %from
  call void @llvm.memset.p0.i64(ptr %start, i8 0, i64 %rest, i1 false)
  store ptr %p, ptr @nz.os_slots
  store i64 %new, ptr @nz.os_cap
  br label %at
at:
  %slots = load ptr, ptr @nz.os_slots
  %off = mul i64 %fd, 64
  %slot = getelementptr i8, ptr %slots, i64 %off
  ret ptr %slot
}

; A new handle for `fd`, of state `kind`, holding `obj`; -ENOMEM when the table cannot grow. The
; lock is held.
define hidden i64 @nz.os_register(i64 %fd, i64 %kind, ptr %obj) {
entry:
  %slot = call ptr @nz.os_slot(i64 %fd)
  %none = icmp eq ptr %slot, null
  br i1 %none, label %refused, label %fill
refused:
  ret i64 -12
fill:
  %g = load i64, ptr %slot
  %gen = add i64 %g, 1
  store i64 %gen, ptr %slot
  %usersp = getelementptr i8, ptr %slot, i64 8
  store i64 0, ptr %usersp
  %statep = getelementptr i8, ptr %slot, i64 16
  store i64 %kind, ptr %statep
  %objp = getelementptr i8, ptr %slot, i64 24
  store ptr %obj, ptr %objp
  %deadlinep = getelementptr i8, ptr %slot, i64 40
  store i64 0, ptr %deadlinep
  %hi = shl i64 %gen, 32
  %h = or i64 %hi, %fd
  ret i64 %h
}

; The live slot `h` names, or null. The lock is held.
define hidden ptr @nz.os_live(i64 %h) {
entry:
  %neg = icmp slt i64 %h, 0
  br i1 %neg, label %dead, label %look
look:
  %fd = and i64 %h, 4294967295
  %gen = lshr i64 %h, 32
  %cap = load i64, ptr @nz.os_cap
  %fits = icmp slt i64 %fd, %cap
  br i1 %fits, label %slot, label %dead
slot:
  %slots = load ptr, ptr @nz.os_slots
  %off = mul i64 %fd, 64
  %s = getelementptr i8, ptr %slots, i64 %off
  %g = load i64, ptr %s
  %same = icmp eq i64 %g, %gen
  %statep = getelementptr i8, ptr %s, i64 16
  %state = load i64, ptr %statep
  %open = icmp ne i64 %state, 0
  %closing = and i64 %state, 256
  %staying = icmp eq i64 %closing, 0
  %a = and i1 %same, %open
  %live = and i1 %a, %staying
  br i1 %live, label %found, label %dead
found:
  ret ptr %s
dead:
  ret ptr null
}

; Pin `h`: its state (1 a file, 2 a directory, 3 standard input), or its status — -65536 for a
; handle that is closed, a number no system gives an error.
define hidden i64 @nz.os_pin(i64 %h) noinline {
entry:
  %neg = icmp slt i64 %h, 0
  br i1 %neg, label %failed, label %lock
failed:
  ret i64 %h
lock:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %closed, label %pin
closed:
  call void @nz.os_release()
  ret i64 -65536
pin:
  %usersp = getelementptr i8, ptr %s, i64 8
  %u = load i64, ptr %usersp
  %u1 = add i64 %u, 1
  store i64 %u1, ptr %usersp
  %statep = getelementptr i8, ptr %s, i64 16
  %kind = load i64, ptr %statep
  call void @nz.os_release()
  ret i64 %kind
}

; Close what `fd` or `obj` holds: 0, or -errno. Standard output and errors (R1d) stay open: the
; handle is closed, the stream is not, and `print` keeps writing.
define hidden i64 @nz.os_shut(i64 %fd, i64 %kind, ptr %obj) {
entry:
  %stream = icmp eq i64 %kind, 10
  br i1 %stream, label %ok, label %which
which:
  %dir = icmp eq i64 %kind, 2
  br i1 %dir, label %closedir, label %close
closedir:
  %rd = call i32 @nz_os_closedir(ptr %obj)
  br label %done
close:
  %d = trunc i64 %fd to i32
  %rf = call i32 @nz_os_close(i32 %d)
  br label %done
done:
  %r = phi i32 [ %rd, %closedir ], [ %rf, %close ]
  %bad = icmp slt i32 %r, 0
  br i1 %bad, label %fail, label %ok
fail:
  %e = call i64 @nz.os_errno()
  ret i64 %e
ok:
  ret i64 0
}

; Unpin `h`; the last unpin of a handle closed meanwhile closes it.
define hidden void @nz.os_unpin(i64 %h) noinline {
entry:
  call void @nz.os_acquire()
  %fd = and i64 %h, 4294967295
  %slots = load ptr, ptr @nz.os_slots
  %off = mul i64 %fd, 64
  %s = getelementptr i8, ptr %slots, i64 %off
  %usersp = getelementptr i8, ptr %s, i64 8
  %u = load i64, ptr %usersp
  %u1 = sub i64 %u, 1
  store i64 %u1, ptr %usersp
  %statep = getelementptr i8, ptr %s, i64 16
  %state = load i64, ptr %statep
  %closing = and i64 %state, 256
  %waiting = icmp ne i64 %closing, 0
  %last = icmp eq i64 %u1, 0
  %shut = and i1 %waiting, %last
  br i1 %shut, label %close, label %keep
keep:
  call void @nz.os_release()
  ret void
close:
  %kind = and i64 %state, 255
  %objp = getelementptr i8, ptr %s, i64 24
  %obj = load ptr, ptr %objp
  store i64 0, ptr %statep
  call void @nz.os_release()
  %r = call i64 @nz.os_shut(i64 %fd, i64 %kind, ptr %obj)
  ret void
}

define hidden i64 @nz.os_close(i64 %h) noinline {
entry:
  %neg = icmp slt i64 %h, 0
  br i1 %neg, label %failed, label %lock
failed:
  ret i64 %h
lock:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %closed, label %live
closed:
  call void @nz.os_release()
  ret i64 -65536
live:
  %usersp = getelementptr i8, ptr %s, i64 8
  %u = load i64, ptr %usersp
  %statep = getelementptr i8, ptr %s, i64 16
  %state = load i64, ptr %statep
  %busy = icmp sgt i64 %u, 0
  br i1 %busy, label %later, label %now
later:
  %marked = or i64 %state, 256
  store i64 %marked, ptr %statep
  ; A socket's waiters wake, find the handle closing, and answer `Closed`; the last of them
  ; to let go closes the descriptor.
  %waitp = getelementptr i8, ptr %s, i64 32
  %wait = load ptr, ptr %waitp
  call void @nz.os_release()
  call void @nz.net_wake(ptr %wait)
  ret i64 0
now:
  %objp = getelementptr i8, ptr %s, i64 24
  %obj = load ptr, ptr %objp
  store i64 0, ptr %statep
  call void @nz.os_release()
  %fd = and i64 %h, 4294967295
  %r = call i64 @nz.os_shut(i64 %fd, i64 %state, ptr %obj)
  ret i64 %r
}

; A handle for descriptor `fd` just opened as `kind`; on a table that cannot grow, the
; descriptor is closed and the status answered.
define hidden i64 @nz.os_adopt(i32 %fd, i64 %kind, ptr %obj) noinline {
entry:
  %w = sext i32 %fd to i64
  call void @nz.os_acquire()
  %h = call i64 @nz.os_register(i64 %w, i64 %kind, ptr %obj)
  call void @nz.os_release()
  %bad = icmp slt i64 %h, 0
  br i1 %bad, label %undo, label %ok
undo:
  %r = call i64 @nz.os_shut(i64 %w, i64 %kind, ptr %obj)
  ret i64 %h
ok:
  ret i64 %h
}

define hidden i64 @nz.os_open_read(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %open
oom:
  ret i64 -12
open:
  %fd = call i32 @nz_os_open(ptr %path, i64 -1)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %bad = icmp slt i32 %fd, 0
  br i1 %bad, label %fail, label %ok
fail:
  ret i64 %e
ok:
  %h = call i64 @nz.os_adopt(i32 %fd, i64 1, ptr null)
  ret i64 %h
}

define hidden i64 @nz.os_open_write(ptr %p, i64 %n, i64 %mode) {
entry:
  %valid = icmp ult i64 %mode, 4
  br i1 %valid, label %copy, label %invalid
invalid:
  ret i64 -22
copy:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %open
oom:
  ret i64 -12
open:
  %fd = call i32 @nz_os_open(ptr %path, i64 %mode)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %bad = icmp slt i32 %fd, 0
  br i1 %bad, label %fail, label %ok
fail:
  ret i64 %e
ok:
  %h = call i64 @nz.os_adopt(i32 %fd, i64 1, ptr null)
  ret i64 %h
}

define hidden i64 @nz.os_dir_open(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %open
oom:
  ret i64 -12
open:
  %d = call ptr @nz_os_opendir(ptr %path)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %bad = icmp eq ptr %d, null
  br i1 %bad, label %fail, label %ok
fail:
  ret i64 %e
ok:
  %fd = call i32 @nz_os_dirfd(ptr %d)
  %h = call i64 @nz.os_adopt(i32 %fd, i64 2, ptr %d)
  ret i64 %h
}

define hidden i64 @nz.os_stdin() noinline {
entry:
  call void @nz.os_acquire()
  %had = load i64, ptr @nz.os_stdin_h
  %known = icmp ne i64 %had, 0
  br i1 %known, label %old, label %new
old:
  call void @nz.os_release()
  ret i64 %had
new:
  %h = call i64 @nz.os_register(i64 0, i64 3, ptr null)
  %ok = icmp sgt i64 %h, 0
  br i1 %ok, label %keep, label %done
keep:
  store i64 %h, ptr @nz.os_stdin_h
  br label %done
done:
  call void @nz.os_release()
  ret i64 %h
}

; Standard output (`which` 1) or errors (2), R1d: one handle each, made the first time.
define hidden i64 @nz.os_std_stream(i64 %which) noinline {
entry:
  %errors = icmp eq i64 %which, 2
  %slot = select i1 %errors, ptr @nz.os_stderr_h, ptr @nz.os_stdout_h
  %fd = select i1 %errors, i64 2, i64 1
  call void @nz.os_acquire()
  %had = load i64, ptr %slot
  %known = icmp ne i64 %had, 0
  br i1 %known, label %old, label %new
old:
  call void @nz.os_release()
  ret i64 %had
new:
  %h = call i64 @nz.os_register(i64 %fd, i64 10, ptr null)
  %ok = icmp sgt i64 %h, 0
  br i1 %ok, label %keep, label %done
keep:
  store i64 %h, ptr %slot
  br label %done
done:
  call void @nz.os_release()
  ret i64 %h
}

define hidden i64 @nz.os_status(i64 %h) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %ok
fail:
  ret i64 %k
ok:
  call void @nz.os_unpin(i64 %h)
  ret i64 0
}

define hidden i64 @nz.os_read(i64 %h, ptr %buf, i64 %n) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %pinned
fail:
  ret i64 %k
pinned:
  switch i64 %k, label %read [
    i64 2, label %isdir
    i64 3, label %stdin
    i64 4, label %socket
    i64 5, label %einval
    i64 6, label %einval
    i64 7, label %socket
    i64 8, label %einval
    i64 9, label %einval
    i64 10, label %writeonly
  ]
isdir:
  call void @nz.os_unpin(i64 %h)
  ret i64 -21
writeonly:
  call void @nz.os_unpin(i64 %h)
  ret i64 -9
einval:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
stdin:
  %ir = call i64 @nz.os_in_read(ptr %buf, i64 %n)
  call void @nz.os_unpin(i64 %h)
  ret i64 %ir
socket:
  %sr = call i64 @nz.net_recv(i64 %h, ptr %buf, i64 %n)
  call void @nz.os_unpin(i64 %h)
  ret i64 %sr
read:
  %fd = trunc i64 %h to i32
  %r = call i64 @nz_os_read(i32 %fd, ptr %buf, i64 %n)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %failed = icmp slt i64 %r, 0
  %out = select i1 %failed, i64 %e, i64 %r
  ret i64 %out
}

; A line of standard input (R1d, P3) into the caller's buffer: `nz.os_in_line`'s answer. Any other
; handle is -EINVAL, a directory -EISDIR, as a read of one is.
define hidden i64 @nz.os_read_line(i64 %h, ptr %buf, i64 %n) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %pinned
fail:
  ret i64 %k
pinned:
  %stdin = icmp eq i64 %k, 3
  br i1 %stdin, label %line, label %other
other:
  %dir = icmp eq i64 %k, 2
  %code = select i1 %dir, i64 -21, i64 -22
  call void @nz.os_unpin(i64 %h)
  ret i64 %code
line:
  %r = call i64 @nz.os_in_line(ptr %buf, i64 %n)
  call void @nz.os_unpin(i64 %h)
  ret i64 %r
}

; Standard input's buffer (R1d, P3), 16 KiB: bytes read from descriptor 0 and not yet handed
; out, shared by `os_read` and `os_read_line`, so a line read never loses what follows its line.
; Made on the first line read; its lock is held across the system call, so standard input has
; one reader at a time.
define hidden void @nz.os_in_acquire() noinline {
entry:
  br label %spin
spin:
  %old = atomicrmw xchg ptr @nz.os_in_lock, i32 1 acquire
  %free = icmp eq i32 %old, 0
  br i1 %free, label %held, label %spin
held:
  ret void
}

define hidden void @nz.os_in_release() {
entry:
  store atomic i32 0, ptr @nz.os_in_lock release, align 4
  ret void
}

; At most `n` bytes of standard input into `buf`: what the buffer holds first, and only when it
; holds nothing one read of the descriptor straight into `buf`. A count, 0 at the end, or -errno.
define hidden i64 @nz.os_in_read(ptr %buf, i64 %n) {
entry:
  call void @nz.os_in_acquire()
  %at = load i64, ptr @nz.os_in_at
  %end = load i64, ptr @nz.os_in_end
  %held = sub i64 %end, %at
  %some = icmp sgt i64 %held, 0
  br i1 %some, label %take, label %direct
take:
  %less = icmp slt i64 %n, %held
  %k = select i1 %less, i64 %n, i64 %held
  %base = load ptr, ptr @nz.os_in_buf
  %from = getelementptr i8, ptr %base, i64 %at
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %from, i64 %k, i1 false)
  %next = add i64 %at, %k
  store i64 %next, ptr @nz.os_in_at
  call void @nz.os_in_release()
  ret i64 %k
direct:
  %r = call i64 @nz_os_read(i32 0, ptr %buf, i64 %n)
  %e = call i64 @nz.os_errno()
  call void @nz.os_in_release()
  %failed = icmp slt i64 %r, 0
  %out = select i1 %failed, i64 %e, i64 %r
  ret i64 %out
}

; Standard input up to and including its next newline into `buf`, stopping sooner at `n` bytes or
; at the end; what follows stays in the buffer. The count, 0 at the end; a failure of the system
; (or -ENOMEM for the buffer) is answered only when nothing was read, and otherwise met again by
; the next read.
define hidden i64 @nz.os_in_line(ptr %buf, i64 %n) {
entry:
  %gotp = alloca i64, align 8
  store i64 0, ptr %gotp
  call void @nz.os_in_acquire()
  br label %loop
loop:
  %got = load i64, ptr %gotp
  %full = icmp sge i64 %got, %n
  br i1 %full, label %done, label %have
have:
  %at = load i64, ptr @nz.os_in_at
  %end = load i64, ptr @nz.os_in_end
  %empty = icmp sge i64 %at, %end
  br i1 %empty, label %fill, label %scan
fill:
  %old = load ptr, ptr @nz.os_in_buf
  %none = icmp eq ptr %old, null
  br i1 %none, label %make, label %refill
make:
  %fresh = call ptr @malloc(i64 16384)
  %oom = icmp eq ptr %fresh, null
  br i1 %oom, label %nomem, label %keep
keep:
  store ptr %fresh, ptr @nz.os_in_buf
  br label %refill
refill:
  %into = load ptr, ptr @nz.os_in_buf
  %r = call i64 @nz_os_read(i32 0, ptr %into, i64 16384)
  %e = call i64 @nz.os_errno()
  %bad = icmp slt i64 %r, 0
  br i1 %bad, label %failed, label %read
read:
  store i64 0, ptr @nz.os_in_at
  store i64 %r, ptr @nz.os_in_end
  %eof = icmp eq i64 %r, 0
  br i1 %eof, label %done, label %loop
scan:
  %base = load ptr, ptr @nz.os_in_buf
  %held = sub i64 %end, %at
  %room = sub i64 %n, %got
  %less = icmp slt i64 %room, %held
  %k = select i1 %less, i64 %room, i64 %held
  br label %look
look:
  %j = phi i64 [ 0, %scan ], [ %j1, %next ]
  %stop = icmp sge i64 %j, %k
  br i1 %stop, label %copy, label %test
test:
  %pos = add i64 %at, %j
  %bp = getelementptr i8, ptr %base, i64 %pos
  %b = load i8, ptr %bp
  %nl = icmp eq i8 %b, 10
  %j1 = add i64 %j, 1
  br i1 %nl, label %copy, label %next
next:
  br label %look
copy:
  %len = phi i64 [ %k, %look ], [ %j1, %test ]
  %ended = phi i1 [ false, %look ], [ true, %test ]
  %from = getelementptr i8, ptr %base, i64 %at
  %to = getelementptr i8, ptr %buf, i64 %got
  call void @llvm.memcpy.p0.p0.i64(ptr %to, ptr %from, i64 %len, i1 false)
  %after = add i64 %at, %len
  store i64 %after, ptr @nz.os_in_at
  %more = add i64 %got, %len
  store i64 %more, ptr %gotp
  br i1 %ended, label %done, label %loop
nomem:
  br label %failed
failed:
  %status = phi i64 [ -12, %nomem ], [ %e, %refill ]
  %some = icmp sgt i64 %got, 0
  br i1 %some, label %done, label %refused
refused:
  call void @nz.os_in_release()
  ret i64 %status
done:
  call void @nz.os_in_release()
  %total = load i64, ptr %gotp
  ret i64 %total
}

define hidden i64 @nz.os_write(i64 %h, ptr %buf, i64 %n) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %pinned
fail:
  ret i64 %k
pinned:
  switch i64 %k, label %write [
    i64 2, label %isdir
    i64 3, label %isstdin
    i64 4, label %socket
    i64 5, label %einval
    i64 6, label %einval
    i64 7, label %einval
    i64 8, label %einval
    i64 9, label %pipe
  ]
isdir:
  call void @nz.os_unpin(i64 %h)
  ret i64 -21
isstdin:
  call void @nz.os_unpin(i64 %h)
  ret i64 -9
einval:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
socket:
  %sw = call i64 @nz.net_send(i64 %h, ptr %buf, i64 %n)
  call void @nz.os_unpin(i64 %h)
  ret i64 %sw
pipe:
  %pw = call i64 @nz.net_pipe_write(i64 %h, ptr %buf, i64 %n)
  call void @nz.os_unpin(i64 %h)
  ret i64 %pw
write:
  %fd = trunc i64 %h to i32
  %r = call i64 @nz_os_write(i32 %fd, ptr %buf, i64 %n)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %failed = icmp slt i64 %r, 0
  %out = select i1 %failed, i64 %e, i64 %r
  ret i64 %out
}

define hidden i64 @nz.os_seek(i64 %h, i64 %offset, i64 %whence) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %pinned
fail:
  ret i64 %k
pinned:
  switch i64 %k, label %check [
    i64 2, label %isdir
    i64 3, label %isstdin
    i64 4, label %isstdin
    i64 5, label %isstdin
    i64 6, label %isstdin
    i64 7, label %isstdin
    i64 8, label %isstdin
    i64 9, label %isstdin
    i64 10, label %isstdin
  ]
isdir:
  call void @nz.os_unpin(i64 %h)
  ret i64 -21
isstdin:
  call void @nz.os_unpin(i64 %h)
  ret i64 -29
check:
  %range = icmp ugt i64 %whence, 2
  %start = icmp eq i64 %whence, 0
  %before = icmp slt i64 %offset, 0
  %negstart = and i1 %start, %before
  %invalid = or i1 %range, %negstart
  br i1 %invalid, label %einval, label %seek
einval:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
seek:
  %fd = trunc i64 %h to i32
  %w = trunc i64 %whence to i32
  %r = call i64 @nz_os_lseek(i32 %fd, i64 %offset, i32 %w)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %failed = icmp slt i64 %r, 0
  %out = select i1 %failed, i64 %e, i64 %r
  ret i64 %out
}

define hidden i64 @nz.os_sync(i64 %h) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %fail, label %pinned
fail:
  ret i64 %k
pinned:
  switch i64 %k, label %sync [
    i64 2, label %isdir
    i64 3, label %isstdin
    i64 4, label %isstdin
    i64 5, label %isstdin
    i64 6, label %isstdin
    i64 7, label %isstdin
    i64 8, label %isstdin
    i64 9, label %isstdin
    i64 10, label %isstdin
  ]
isdir:
  call void @nz.os_unpin(i64 %h)
  ret i64 -21
isstdin:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
sync:
  %fd = trunc i64 %h to i32
  %r = call i32 @nz_os_fsync(i32 %fd)
  %e = call i64 @nz.os_errno()
  call void @nz.os_unpin(i64 %h)
  %failed = icmp slt i32 %r, 0
  %out = select i1 %failed, i64 %e, i64 0
  ret i64 %out
}

define hidden i64 @nz.os_dir_next(i64 %h, ptr %buf, i64 %n) noinline {
entry:
  %neg = icmp slt i64 %h, 0
  br i1 %neg, label %failed, label %lock
failed:
  ret i64 %h
lock:
  call void @nz.os_acquire()
  %s = call ptr @nz.os_live(i64 %h)
  %dead = icmp eq ptr %s, null
  br i1 %dead, label %closed, label %live
closed:
  call void @nz.os_release()
  ret i64 -65536
live:
  %statep = getelementptr i8, ptr %s, i64 16
  %state = load i64, ptr %statep
  %dir = icmp eq i64 %state, 2
  br i1 %dir, label %list, label %notdir
notdir:
  call void @nz.os_release()
  ret i64 -20
list:
  %objp = getelementptr i8, ptr %s, i64 24
  %d = load ptr, ptr %objp
  %r = call i64 @nz_os_readdir(ptr %d, ptr %buf, i64 %n)
  call void @nz.os_release()
  ret i64 %r
}

define hidden i64 @nz.os_stat(ptr %p, i64 %n, ptr %out) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %stat
oom:
  ret i64 -12
stat:
  %r = call i32 @nz_os_stat(ptr %path, ptr %out)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %failed = icmp slt i32 %r, 0
  %res = select i1 %failed, i64 %e, i64 0
  ret i64 %res
}

; `op` 0 makes a directory, 1 removes one, 2 removes a file.
define hidden i64 @nz.os_path_op(ptr %p, i64 %n, i64 %op) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %go
oom:
  ret i64 -12
go:
  switch i64 %op, label %unlink [
    i64 0, label %mkdir
    i64 1, label %rmdir
  ]
mkdir:
  %rm = call i32 @nz_os_mkdir(ptr %path)
  br label %done
rmdir:
  %rr = call i32 @nz_os_rmdir(ptr %path)
  br label %done
unlink:
  %ru = call i32 @nz_os_unlink(ptr %path)
  br label %done
done:
  %r = phi i32 [ %rm, %mkdir ], [ %rr, %rmdir ], [ %ru, %unlink ]
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %failed = icmp slt i32 %r, 0
  %res = select i1 %failed, i64 %e, i64 0
  ret i64 %res
}

define hidden i64 @nz.os_rename(ptr %ap, i64 %an, ptr %bp, i64 %bn) {
entry:
  %a = call ptr @nz.cstr(ptr %ap, i64 %an)
  %noa = icmp eq ptr %a, null
  br i1 %noa, label %oom, label %second
oom:
  ret i64 -12
second:
  %b = call ptr @nz.cstr(ptr %bp, i64 %bn)
  %nob = icmp eq ptr %b, null
  br i1 %nob, label %oomb, label %go
oomb:
  call void @free(ptr %a)
  ret i64 -12
go:
  %r = call i32 @nz_os_rename(ptr %a, ptr %b)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %a)
  call void @free(ptr %b)
  %failed = icmp slt i32 %r, 0
  %res = select i1 %failed, i64 %e, i64 0
  ret i64 %res
}

define hidden i64 @nz.os_family() {
entry:
  %f = call i64 @nz_os_family()
  ret i64 %f
}

@.closed = private unnamed_addr constant [21 x i8] c"the handle is closed\00"
@.unnamed = private unnamed_addr constant [23 x i8] c"the name was not found\00"

define hidden i64 @nz.os_error_text(i64 %status, ptr %buf, i64 %cap) {
entry:
  %closed = icmp eq i64 %status, -65536
  br i1 %closed, label %ours, label %name
ours:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr @.closed, i64 20, i1 false)
  ret i64 20
name:
  %unknown = icmp eq i64 %status, -65537
  br i1 %unknown, label %unnamed, label %system
unnamed:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr @.unnamed, i64 22, i1 false)
  ret i64 22
system:
  %n = call i64 @nz_os_error_text(i64 %status, ptr %buf, i64 %cap)
  ret i64 %n
}

declare ptr @nz_os_environ()

; Whether entry `e` is one: an `=` after its first byte. Its name's length then, or -1.
define hidden i64 @nz.os_env_split(ptr %e) {
entry:
  %n = call i64 @strlen(ptr %e)
  br label %scan
scan:
  %i = phi i64 [ 1, %entry ], [ %i1, %next ]
  %end = icmp sge i64 %i, %n
  br i1 %end, label %none, label %look
look:
  %cp = getelementptr i8, ptr %e, i64 %i
  %c = load i8, ptr %cp
  %eq = icmp eq i8 %c, 61
  br i1 %eq, label %found, label %next
next:
  %i1 = add i64 %i, 1
  br label %scan
found:
  ret i64 %i
none:
  ret i64 -1
}

; The `i`th entry of the environment that is one, `NAME=value`, its length through `lenp`; null
; past the last. `i` -1 counts them instead, returning null and the count through `lenp`.
define hidden ptr @nz.os_env_nth(i64 %want, ptr %lenp) {
entry:
  %env = call ptr @nz_os_environ()
  br label %walk
walk:
  %k = phi i64 [ 0, %entry ], [ %k1, %skip ], [ %k2, %counted ]
  %seen = phi i64 [ 0, %entry ], [ %seen, %skip ], [ %seen1, %counted ]
  %ep = getelementptr ptr, ptr %env, i64 %k
  %e = load ptr, ptr %ep
  %end = icmp eq ptr %e, null
  br i1 %end, label %done, label %test
test:
  %at = call i64 @nz.os_env_split(ptr %e)
  %is = icmp sgt i64 %at, 0
  %k1 = add i64 %k, 1
  br i1 %is, label %valid, label %skip
skip:
  br label %walk
valid:
  %this = icmp eq i64 %seen, %want
  br i1 %this, label %hit, label %counted
counted:
  %seen1 = add i64 %seen, 1
  %k2 = add i64 %k, 1
  br label %walk
hit:
  %len = call i64 @strlen(ptr %e)
  store i64 %len, ptr %lenp
  ret ptr %e
done:
  store i64 %seen, ptr %lenp
  ret ptr null
}

define hidden i64 @nz.os_env_count() {
entry:
  %n = alloca i64, align 8
  %p = call ptr @nz.os_env_nth(i64 -1, ptr %n)
  %c = load i64, ptr %n
  ret i64 %c
}

define hidden ptr @nz.os_env_at(i64 %i, ptr %lenp) {
entry:
  %p = call ptr @nz.os_env_nth(i64 %i, ptr %lenp)
  ret ptr %p
}

; `n` bytes of the system's entropy into `buf`, 256 at a time as `getentropy` allows: `n`, or -errno.
define hidden i64 @nz.os_random_bytes(ptr %buf, i64 %n) {
entry:
  br label %loop
loop:
  %at = phi i64 [ 0, %entry ], [ %at1, %got ]
  %left = sub i64 %n, %at
  %done = icmp sle i64 %left, 0
  br i1 %done, label %finish, label %chunk
chunk:
  %big = icmp sgt i64 %left, 256
  %take = select i1 %big, i64 256, i64 %left
  %p = getelementptr i8, ptr %buf, i64 %at
  %r = call i32 @nz_os_entropy(ptr %p, i64 %take)
  %bad = icmp ne i32 %r, 0
  br i1 %bad, label %fail, label %got
got:
  %at1 = add i64 %at, %take
  br label %loop
fail:
  %e = call i64 @nz.os_errno()
  ret i64 %e
finish:
  ret i64 %n
}

; The value of the first entry named `name` (`n` bytes), its length through `lenp`; when there is
; none, an empty string and -1.
define hidden ptr @nz.os_env_find(ptr %name, i64 %n, ptr %lenp) {
entry:
  %env = call ptr @nz_os_environ()
  br label %walk
walk:
  %k = phi i64 [ 0, %entry ], [ %k1, %next ]
  %ep = getelementptr ptr, ptr %env, i64 %k
  %e = load ptr, ptr %ep
  %end = icmp eq ptr %e, null
  br i1 %end, label %none, label %test
test:
  %k1 = add i64 %k, 1
  %at = call i64 @nz.os_env_split(ptr %e)
  %same = icmp eq i64 %at, %n
  br i1 %same, label %compare, label %next
compare:
  %d = call i32 @memcmp(ptr %e, ptr %name, i64 %n)
  %eq = icmp eq i32 %d, 0
  br i1 %eq, label %hit, label %next
next:
  br label %walk
hit:
  %start = add i64 %n, 1
  %v = getelementptr i8, ptr %e, i64 %start
  %len = call i64 @strlen(ptr %v)
  store i64 %len, ptr %lenp
  ret ptr %v
none:
  store i64 -1, ptr %lenp
  ret ptr @.nz.empty
}

@.nz.empty = private unnamed_addr constant [1 x i8] c"\00"
"#;

/// The file system, further (R1d, `docs/user/spec.md`, *The platform, further*): a path's own facts, its
/// permissions, its canonical form, a new private directory, and advisory whole-file locks, over the
/// per-target unit's `nz_os_lstat`, `nz_os_chmod`, `nz_os_realpath` and `nz_os_flock` — which WASI's
/// unit answers with `path_filestat_get` and, for the three it has not got, ENOSYS, as it answers
/// listing a directory. A temporary directory is private where the system keeps permissions: one
/// whose `chmod` is ENOSYS (WASI) keeps the directory as made.
pub(crate) const FS_RUNTIME: &str = r#"
declare i32 @nz_os_lstat(ptr, ptr)
declare i32 @nz_os_chmod(ptr, i64)
declare i64 @nz_os_realpath(ptr, ptr, i64)
declare i32 @nz_os_flock(i32, i32)

define hidden i64 @nz.os_lstat(ptr %p, i64 %n, ptr %out) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %stat
oom:
  ret i64 -12
stat:
  %r = call i32 @nz_os_lstat(ptr %path, ptr %out)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %failed = icmp slt i32 %r, 0
  %res = select i1 %failed, i64 %e, i64 0
  ret i64 %res
}

; Permission bits `mode`, `0..=0o7777`; 0 or -errno.
define hidden i64 @nz.os_chmod(ptr %p, i64 %n, i64 %mode) {
entry:
  %bits = icmp ult i64 %mode, 4096
  br i1 %bits, label %copy, label %invalid
invalid:
  ret i64 -22
copy:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %go
oom:
  ret i64 -12
go:
  %r = call i32 @nz_os_chmod(ptr %path, i64 %mode)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %failed = icmp slt i32 %r, 0
  %res = select i1 %failed, i64 %e, i64 0
  ret i64 %res
}

; The canonical path into the caller's `cap` bytes: its length, -EINVAL when it does not fit, or
; -errno.
define hidden i64 @nz.os_realpath(ptr %p, i64 %n, ptr %out, i64 %cap) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %oom, label %go
oom:
  ret i64 -12
go:
  %r = call i64 @nz_os_realpath(ptr %path, ptr %out, i64 %cap)
  %e = call i64 @nz.os_errno()
  call void @free(ptr %path)
  %failed = icmp slt i64 %r, 0
  br i1 %failed, label %fail, label %got
fail:
  ret i64 %e
got:
  %long = icmp sgt i64 %r, %cap
  %res = select i1 %long, i64 -22, i64 %r
  ret i64 %res
}

@.nz.tmpname = private unnamed_addr constant [5 x i8] c"nazm-"

; A new directory in `parent` named `nazm-` and twelve random lowercase letters and digits, made
; by this call, its permissions 0o700; its path into the caller's `cap` bytes and the path's
; length. -EINVAL for an empty parent or a path that would not fit, before anything is made;
; -EEXIST after 64 names were all taken; or -errno.
define hidden i64 @nz.os_mkdtemp(ptr %p, i64 %n, ptr %out, i64 %cap) {
entry:
  %rand = alloca [16 x i8], align 8
  %empty = icmp eq i64 %n, 0
  br i1 %empty, label %invalid, label %measure
invalid:
  ret i64 -22
measure:
  %lastp = getelementptr i8, ptr %p, i64 %n
  %lastq = getelementptr i8, ptr %lastp, i64 -1
  %last = load i8, ptr %lastq
  %ends = icmp eq i8 %last, 47
  %sep = select i1 %ends, i64 0, i64 1
  %head = add i64 %n, %sep
  %total = add i64 %head, 17
  %long = icmp sgt i64 %total, %cap
  br i1 %long, label %invalid, label %alloc
alloc:
  %room = add i64 %total, 1
  %buf = call ptr @malloc(i64 %room)
  %none = icmp eq ptr %buf, null
  br i1 %none, label %oom, label %fill
oom:
  ret i64 -12
fill:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %p, i64 %n, i1 false)
  %slashp = getelementptr i8, ptr %buf, i64 %n
  store i8 47, ptr %slashp
  %prefixp = getelementptr i8, ptr %buf, i64 %head
  call void @llvm.memcpy.p0.p0.i64(ptr %prefixp, ptr @.nz.tmpname, i64 5, i1 false)
  %endp = getelementptr i8, ptr %buf, i64 %total
  store i8 0, ptr %endp
  %namep = getelementptr i8, ptr %prefixp, i64 5
  br label %attempt
attempt:
  %tries = phi i64 [ 0, %fill ], [ %tries1, %taken ]
  %more = icmp slt i64 %tries, 64
  br i1 %more, label %draw, label %exhausted
exhausted:
  call void @free(ptr %buf)
  ret i64 -17
draw:
  %got = call i32 @nz_os_entropy(ptr %rand, i64 12)
  %noent = icmp ne i32 %got, 0
  br i1 %noent, label %fail, label %letters
letters:
  %i = phi i64 [ 0, %draw ], [ %i1, %letters ]
  %rp = getelementptr i8, ptr %rand, i64 %i
  %rb = load i8, ptr %rp
  %rw = zext i8 %rb to i64
  %c = urem i64 %rw, 36
  %digit = icmp ult i64 %c, 10
  %dch = add i64 %c, 48
  %lch = add i64 %c, 87
  %ch = select i1 %digit, i64 %dch, i64 %lch
  %ch8 = trunc i64 %ch to i8
  %np = getelementptr i8, ptr %namep, i64 %i
  store i8 %ch8, ptr %np
  %i1 = add i64 %i, 1
  %named = icmp eq i64 %i1, 12
  br i1 %named, label %make, label %letters
make:
  %m = call i32 @nz_os_mkdir(ptr %buf)
  %made = icmp eq i32 %m, 0
  br i1 %made, label %private, label %why
why:
  %e = call i64 @nz.os_errno()
  %exists = icmp eq i64 %e, -17
  br i1 %exists, label %taken, label %give
taken:
  %tries1 = add i64 %tries, 1
  br label %attempt
give:
  call void @free(ptr %buf)
  ret i64 %e
private:
  %pm = call i32 @nz_os_chmod(ptr %buf, i64 448)
  %pe = call i64 @nz.os_errno()
  %refused = icmp ne i32 %pm, 0
  %kept = icmp ne i64 %pe, -38
  %open = and i1 %refused, %kept
  br i1 %open, label %undo, label %done
undo:
  %u = call i32 @nz_os_rmdir(ptr %buf)
  call void @free(ptr %buf)
  ret i64 %pe
done:
  call void @llvm.memcpy.p0.p0.i64(ptr %out, ptr %buf, i64 %total, i1 false)
  call void @free(ptr %buf)
  ret i64 %total
fail:
  %fe = call i64 @nz.os_errno()
  call void @free(ptr %buf)
  ret i64 %fe
}

; An advisory whole-file lock on file `h`, exclusive or shared, waited for when `wait` is 1, or
; tried once; 0, or a status: -EISDIR for a directory, -EINVAL for any handle but a file's, and
; the system's -EWOULDBLOCK while another holds it.
define hidden i64 @nz.os_file_lock(i64 %h, i1 %exclusive, i64 %wait) {
entry:
  %kind = select i1 %exclusive, i32 2, i32 1
  %waits = icmp ne i64 %wait, 0
  %nb = select i1 %waits, i32 0, i32 4
  %op = or i32 %kind, %nb
  %r = call i64 @nz.os_flock_handle(i64 %h, i32 %op, i1 %waits)
  ret i64 %r
}

define hidden i64 @nz.os_file_unlock(i64 %h) {
entry:
  %r = call i64 @nz.os_flock_handle(i64 %h, i32 8, i1 false)
  ret i64 %r
}

; `flock` operation `op` on file `h`, pinned while it runs; a wait the system interrupts is
; resumed when `resume` is set.
define hidden i64 @nz.os_flock_handle(i64 %h, i32 %op, i1 %resume) {
entry:
  %k = call i64 @nz.os_pin(i64 %h)
  %bad = icmp slt i64 %k, 0
  br i1 %bad, label %failed, label %pinned
failed:
  ret i64 %k
pinned:
  switch i64 %k, label %einval [
    i64 1, label %lock
    i64 2, label %isdir
  ]
isdir:
  call void @nz.os_unpin(i64 %h)
  ret i64 -21
einval:
  call void @nz.os_unpin(i64 %h)
  ret i64 -22
lock:
  %fd = trunc i64 %h to i32
  %r = call i32 @nz_os_flock(i32 %fd, i32 %op)
  %ok = icmp eq i32 %r, 0
  br i1 %ok, label %held, label %refused
held:
  call void @nz.os_unpin(i64 %h)
  ret i64 0
refused:
  %e = call i64 @nz.os_errno()
  %intr = icmp eq i64 %e, -4
  %again = and i1 %intr, %resume
  br i1 %again, label %lock, label %give
give:
  call void @nz.os_unpin(i64 %h)
  ret i64 %e
}
"#;

/// Where a program reaches files and not the network, the four entries the files part calls for a
/// socket or a pipe, which no handle of such a program can be.
pub(crate) const NET_STUBS: &str = r"
define hidden i64 @nz.net_recv(i64 %h, ptr %buf, i64 %n) {
entry:
  ret i64 -22
}

define hidden i64 @nz.net_send(i64 %h, ptr %buf, i64 %n) {
entry:
  ret i64 -22
}

define hidden void @nz.net_wake(ptr %w) noinline {
entry:
  ret void
}

define hidden i64 @nz.net_pipe_write(i64 %h, ptr %buf, i64 %n) {
entry:
  ret i64 -22
}
";
