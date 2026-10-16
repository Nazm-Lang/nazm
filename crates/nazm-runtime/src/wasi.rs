//! The WASI runtime and entry (Gate 3, `docs/spec.md`, *WASI*): `wasm32-wasip1` — WebAssembly with
//! the WASI preview-1 system interface, one thread, and the outside reached only through its
//! imports from `wasi_snapshot_preview1`.
//!
//! Where `wasm32-unknown-unknown` has no heap (N64), this one has: a bump arena in linear memory
//! from the linker's `__heap_base`, grown by `memory.grow` and never returned, over which the parts
//! a board with a heap takes — strings, sequences, joins, closures — run unchanged. Standard
//! streams are `fd_write` and `fd_read`; arguments `args_get`; clocks `clock_time_get`; files the
//! files part over this unit's `nz_os_*`, each `path_open`ed beneath the first preopened directory,
//! descriptor 3, so a program reaches only the directories its host grants — the capability rule a
//! WASI host enforces. Tasks, channels and the network need threads or sockets WASI preview 1 has
//! not got: a program reaching them is refused by name, as on `wasm32-unknown-unknown`.
//!
//! Every C function the runtime parts call is defined here, with the signature the parts declare:
//! `malloc`, `free`, `realloc`, `memcpy`, `memmove`, `memset`, `memcmp`, `strlen`, `write`,
//! `exit`. The module is built with bulk memory, so LLVM's own copies are `memory.copy` and
//! `memory.fill`, never a call that would meet these at another signature.

use std::fmt::Write as _;

use crate::{escape, preamble, Needs, RESULT_STATUS, RESULT_VARIABLE};

/// The module every import comes from.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

/// The shadow stack the linker reserves: 1 MiB.
pub const STACK: u64 = 0x10_0000;

/// What `nz.stack_check` keeps in reserve (N49's quarter).
pub const STACK_HEADROOM: u64 = STACK >> 2;

/// The `wasm-ld` line a WASI module is linked with, after the objects: the command `_start`, the
/// memory exported, as WASI hosts expect.
pub const LINK: &str = "wasm-ld --stack-first -z stack-size=1048576 --export=_start -o module.wasm";

/// Every WASI function the runtime imports, with its signature.
const IMPORTS: &[(&str, &str, &str)] = &[
    ("fd_write", "i32", "i32, ptr, i32, ptr"),
    ("fd_read", "i32", "i32, ptr, i32, ptr"),
    ("fd_close", "i32", "i32"),
    ("fd_seek", "i32", "i32, i64, i32, ptr"),
    ("fd_sync", "i32", "i32"),
    (
        "path_open",
        "i32",
        "i32, i32, ptr, i32, i32, i64, i64, i32, ptr",
    ),
    ("path_filestat_get", "i32", "i32, i32, ptr, i32, ptr"),
    ("path_create_directory", "i32", "i32, ptr, i32"),
    ("path_remove_directory", "i32", "i32, ptr, i32"),
    ("path_unlink_file", "i32", "i32, ptr, i32"),
    ("path_rename", "i32", "i32, ptr, i32, i32, ptr, i32"),
    ("args_sizes_get", "i32", "ptr, ptr"),
    ("args_get", "i32", "ptr, ptr"),
    ("clock_time_get", "i32", "i32, i64, ptr"),
    ("random_get", "i32", "ptr, i32"),
    ("proc_exit", "void", "i32"),
    ("poll_oneoff", "i32", "ptr, ptr, i32, ptr"),
];

/// The runtime for a program that reaches `needs`.
#[must_use]
pub fn emit_runtime(needs: &Needs) -> String {
    let mut out = String::new();
    preamble(&mut out, "<nazm WASI runtime>");
    for (k, (name, ret, params)) in IMPORTS.iter().enumerate() {
        let _ = writeln!(out, "declare {ret} @wasi.{name}({params}) #{k}");
    }
    out.push_str(BASE);
    out.push_str(&crate::wasm::multi3());
    out.push_str(&stack_check());
    // The parts a heap admits, as a board with a heap takes them.
    out.push_str(&crate::core::board_heap_parts(needs));
    out.push_str(&crate::core::heap_shims(needs));
    if needs.io || needs.files {
        out.push_str(IO);
    }
    if needs.files {
        out.push_str(crate::files::FILES_RUNTIME);
        out.push_str(crate::files::FS_RUNTIME);
        out.push_str(crate::files::NET_STUBS);
        out.push_str(OS);
    }
    let mut out = without_declarations_of_definitions(&out);
    for (k, (name, _, _)) in IMPORTS.iter().enumerate() {
        let _ = writeln!(
            out,
            "attributes #{k} = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"{name}\" }}"
        );
    }
    out
}

/// `text` without a `declare` of a function `text` also defines: the parts written for a hosted
/// runtime declare the platform functions this module defines itself.
fn without_declarations_of_definitions(text: &str) -> String {
    let name = |line: &str| -> Option<String> {
        let (_, rest) = line.split_once('@')?;
        Some(rest.chars().take_while(|c| *c != '(').collect())
    };
    let defined: std::collections::BTreeSet<String> = text
        .lines()
        .filter(|l| l.starts_with("define "))
        .filter_map(name)
        .collect();
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let declares_a_definition =
            line.starts_with("declare ") && name(line).is_some_and(|n| defined.contains(&n));
        if !declares_a_definition {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn stack_check() -> String {
    format!(
        r"
@__stack_low = external global i8
declare ptr @llvm.frameaddress.p0(i32)

define hidden i32 @nz.stack_check() {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %low = ptrtoint ptr @__stack_low to i64
  %limit = add i64 %low, {STACK_HEADROOM}
  %past = icmp ult i64 %sp, %limit
  %r = zext i1 %past to i32
  ret i32 %r
}}

; A function whose own frame is large asks this instead (Q1-A1-01): past the limit, or with less
; than `need` bytes between this frame and the limit.
define hidden i32 @nz.stack_check_frame(i64 %need) {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %low = ptrtoint ptr @__stack_low to i64
  %limit = add i64 %low, {STACK_HEADROOM}
  %past = icmp ult i64 %sp, %limit
  %room = sub i64 %sp, %limit
  %short = icmp ult i64 %room, %need
  %either = or i1 %past, %short
  %r = zext i1 %either to i32
  ret i32 %r
}}

; The stack the linker reserved, for the parts that budget against it.
define hidden i64 @nz_os_stack_size() {{
entry:
  ret i64 {STACK}
}}

; The preemption poll's flag and slow path (6.3): one thread and no pool, so never asked.
@nz.preempt_req = hidden global i64 0

define hidden void @nz.preempt() {{
entry:
  ret void
}}
"
    )
}

/// The failure state, the heap, the C library's few functions, standard output, the clocks and the
/// arguments' globals.
const BASE: &str = r#"
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
declare i32 @llvm.wasm.memory.size.i32(i32)
declare i32 @llvm.wasm.memory.grow.i32(i32, i32)

@nz.failed = hidden global i1 false
@nz.errmsg = hidden global ptr null
@nz.errlen = hidden global i64 0
@nz.argc = hidden global i64 0
@nz.argv = hidden global ptr null
@__heap_base = external global i8
@nz.heap_next = internal global i32 0

define hidden void @nz.fail(ptr %m, i64 %n) {
entry:
  %had = load i1, ptr @nz.failed
  br i1 %had, label %done, label %record
record:
  store i1 true, ptr @nz.failed
  store ptr %m, ptr @nz.errmsg
  store i64 %n, ptr @nz.errlen
  br label %done
done:
  ret void
}

define hidden void @nz.errno_save() {
entry:
  ret void
}

define hidden i32 @nz.failed_now() {
entry:
  %f = load i1, ptr @nz.failed
  %r = zext i1 %f to i32
  ret i32 %r
}

; The failure written to standard error, and the process ended with status 2.
define hidden void @nz.report() noreturn {
entry:
  %m = load ptr, ptr @nz.errmsg
  %n = load i64, ptr @nz.errlen
  %w = call i64 @write(i32 2, ptr %m, i64 %n)
  call void @wasi.proc_exit(i32 2)
  unreachable
}

; The arena: a 16-byte header holding the size, so `realloc` copies what there was, every block
; 16-aligned; memory grown a page at a time where the next block passes its end. Never returned.
define ptr @malloc(i64 %n) "no-builtins" {
entry:
  %huge = icmp ugt i64 %n, 2147483648
  br i1 %huge, label %none, label %size
size:
  %seen = load i32, ptr @nz.heap_next
  %fresh = icmp eq i32 %seen, 0
  %base = ptrtoint ptr @__heap_base to i32
  %b15 = add i32 %base, 15
  %balign = and i32 %b15, -16
  %cur = select i1 %fresh, i32 %balign, i32 %seen
  %n32 = trunc i64 %n to i32
  %r15 = add i32 %n32, 15
  %ralign = and i32 %r15, -16
  %need = add i32 %ralign, 16
  %next = add i32 %cur, %need
  %next64 = zext i32 %next to i64
  %pages = call i32 @llvm.wasm.memory.size.i32(i32 0)
  %pages64 = zext i32 %pages to i64
  %limit = shl i64 %pages64, 16
  %over = icmp ugt i64 %next64, %limit
  br i1 %over, label %grow, label %take
grow:
  %short = sub i64 %next64, %limit
  %round = add i64 %short, 65535
  %more64 = lshr i64 %round, 16
  %more = trunc i64 %more64 to i32
  %got = call i32 @llvm.wasm.memory.grow.i32(i32 0, i32 %more)
  %failed = icmp eq i32 %got, -1
  br i1 %failed, label %none, label %take
take:
  store i32 %next, ptr @nz.heap_next
  %h = inttoptr i32 %cur to ptr
  store i64 %n, ptr %h
  %p = getelementptr i8, ptr %h, i32 16
  ret ptr %p
none:
  ret ptr null
}

define void @free(ptr %p) {
entry:
  ret void
}

define ptr @realloc(ptr %p, i64 %n) "no-builtins" {
entry:
  %q = call ptr @malloc(i64 %n)
  %absent = icmp eq ptr %p, null
  %failed = icmp eq ptr %q, null
  %skip = or i1 %absent, %failed
  br i1 %skip, label %done, label %copy
copy:
  %h = getelementptr i8, ptr %p, i32 -16
  %old = load i64, ptr %h
  %less = icmp ult i64 %old, %n
  %m = select i1 %less, i64 %old, i64 %n
  call void @llvm.memcpy.p0.p0.i64(ptr %q, ptr %p, i64 %m, i1 false)
  br label %done
done:
  ret ptr %q
}

define ptr @calloc(i64 %k, i64 %n) "no-builtins" {
entry:
  %size = mul i64 %k, %n
  %p = call ptr @malloc(i64 %size)
  %none = icmp eq ptr %p, null
  br i1 %none, label %done, label %zero
zero:
  call void @llvm.memset.p0.i64(ptr %p, i8 0, i64 %size, i1 false)
  br label %done
done:
  ret ptr %p
}

; With bulk memory, each of these is one instruction: none can lower to a call to itself.
define ptr @memcpy(ptr %d, ptr %s, i64 %n) "no-builtins" {
entry:
  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)
  ret ptr %d
}

define ptr @memmove(ptr %d, ptr %s, i64 %n) "no-builtins" {
entry:
  call void @llvm.memmove.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)
  ret ptr %d
}

define ptr @memset(ptr %d, i32 %v, i64 %n) "no-builtins" {
entry:
  %b = trunc i32 %v to i8
  call void @llvm.memset.p0.i64(ptr %d, i8 %b, i64 %n, i1 false)
  ret ptr %d
}

define i32 @memcmp(ptr %a, ptr %b, i64 %n) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %j, %same ]
  %more = icmp ult i64 %i, %n
  br i1 %more, label %body, label %equal
body:
  %ap = getelementptr i8, ptr %a, i64 %i
  %x = load i8, ptr %ap
  %bp = getelementptr i8, ptr %b, i64 %i
  %y = load i8, ptr %bp
  %eq = icmp eq i8 %x, %y
  br i1 %eq, label %same, label %differ
same:
  %j = add i64 %i, 1
  br label %loop
differ:
  %xw = zext i8 %x to i32
  %yw = zext i8 %y to i32
  %d = sub i32 %xw, %yw
  ret i32 %d
equal:
  ret i32 0
}

; `bcmp`, as LLVM may emit for an equality test of `memcmp`, at wasm32's size.
define i32 @bcmp(ptr %a, ptr %b, i32 %n) "no-builtins" {
entry:
  %w = zext i32 %n to i64
  %r = call i32 @memcmp(ptr %a, ptr %b, i64 %w)
  ret i32 %r
}

define i64 @strlen(ptr %s) "no-builtins" {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %j, %next ]
  %p = getelementptr i8, ptr %s, i64 %i
  %c = load i8, ptr %p
  %end = icmp eq i8 %c, 0
  br i1 %end, label %done, label %next
next:
  %j = add i64 %i, 1
  br label %loop
done:
  ret i64 %i
}

; The C library's `write` over `fd_write`: every byte, or -1. One I/O vector: a pointer and a
; 32-bit length.
define i64 @write(i32 %fd, ptr %p, i64 %n) {
entry:
  %iov = alloca [2 x i32], align 4
  %done = alloca i32, align 4
  br label %loop
loop:
  %at = phi i64 [ 0, %entry ], [ %next, %wrote ]
  %left = sub i64 %n, %at
  %more = icmp sgt i64 %left, 0
  br i1 %more, label %send, label %finished
send:
  %from = getelementptr i8, ptr %p, i64 %at
  %fromi = ptrtoint ptr %from to i32
  store i32 %fromi, ptr %iov
  %lenp = getelementptr i32, ptr %iov, i32 1
  %len = trunc i64 %left to i32
  store i32 %len, ptr %lenp
  %e = call i32 @wasi.fd_write(i32 %fd, ptr %iov, i32 1, ptr %done)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %failed, label %wrote
wrote:
  %d = load i32, ptr %done
  %d64 = zext i32 %d to i64
  %next = add i64 %at, %d64
  br label %loop
failed:
  ret i64 -1
finished:
  ret i64 %n
}

define void @exit(i32 %code) noreturn {
entry:
  call void @wasi.proc_exit(i32 %code)
  unreachable
}

; An Int's decimal digits into the caller's 21 bytes: how many.
define hidden i64 @nz.format_int(ptr %buf, i64 %v) {
entry:
  %tmp = alloca [24 x i8], align 1
  %neg = icmp slt i64 %v, 0
  br label %digits
digits:
  %x = phi i64 [ %v, %entry ], [ %q, %digits ]
  %k = phi i64 [ 0, %entry ], [ %k1, %digits ]
  %q = sdiv i64 %x, 10
  %r = srem i64 %x, 10
  %ra = call i64 @llvm.abs.i64(i64 %r, i1 false)
  %ch = add i64 %ra, 48
  %c8 = trunc i64 %ch to i8
  %slot = getelementptr i8, ptr %tmp, i64 %k
  store i8 %c8, ptr %slot
  %k1 = add i64 %k, 1
  %more = icmp ne i64 %q, 0
  br i1 %more, label %digits, label %sign
sign:
  br i1 %neg, label %minus, label %copy
minus:
  %ms = getelementptr i8, ptr %tmp, i64 %k1
  store i8 45, ptr %ms
  %k2 = add i64 %k1, 1
  br label %copy
copy:
  %len = phi i64 [ %k1, %sign ], [ %k2, %minus ]
  br label %rev
rev:
  %i = phi i64 [ 0, %copy ], [ %i1, %rev ]
  %src = sub i64 %len, 1
  %si = sub i64 %src, %i
  %sp = getelementptr i8, ptr %tmp, i64 %si
  %c = load i8, ptr %sp
  %dp = getelementptr i8, ptr %buf, i64 %i
  store i8 %c, ptr %dp
  %i1 = add i64 %i, 1
  %again = icmp ult i64 %i1, %len
  br i1 %again, label %rev, label %out
out:
  ret i64 %len
}
declare i64 @llvm.abs.i64(i64, i1)

; Clock 4 is a monotonic clock and 0 the wall clock, as on Linux and macOS; a `timespec` is two
; 64-bit words. WASI's clock 1 is its monotonic one, 0 its realtime one.
define hidden i32 @nz_os_clock(i32 %which, ptr %ts) {
entry:
  %ns = alloca i64, align 8
  %wall = icmp eq i32 %which, 0
  %id = select i1 %wall, i32 0, i32 1
  %e = call i32 @wasi.clock_time_get(i32 %id, i64 1, ptr %ns)
  %t = load i64, ptr %ns
  %sec = udiv i64 %t, 1000000000
  %nsec = urem i64 %t, 1000000000
  store i64 %sec, ptr %ts
  %np = getelementptr i8, ptr %ts, i32 8
  store i64 %nsec, ptr %np
  %bad = icmp ne i32 %e, 0
  %r = select i1 %bad, i32 -1, i32 0
  ret i32 %r
}

define hidden i64 @nz.time_now_ns() {
entry:
  %ns = alloca i64, align 8
  %e = call i32 @wasi.clock_time_get(i32 1, i64 1, ptr %ns)
  %t = load i64, ptr %ns
  ret i64 %t
}

define hidden i64 @nz.time_now_ms() {
entry:
  %t = call i64 @nz.time_now_ns()
  %ms = udiv i64 %t, 1000000
  ret i64 %ms
}

; A sleep (Gate 3): one thread and no task to run meanwhile, so the module waits — `poll_oneoff` on a
; single relative subscription to the monotonic clock. 0.
define hidden i64 @nz.time_sleep_ms(i64 %ms) {
entry:
  %sub = alloca [48 x i8], align 8
  %ev = alloca [32 x i8], align 8
  %got = alloca i32, align 4
  call void @llvm.memset.p0.i64(ptr %sub, i8 0, i64 48, i1 false)
  %pos = icmp sgt i64 %ms, 0
  br i1 %pos, label %wait, label %done
wait:
  %idp = getelementptr i8, ptr %sub, i32 16
  store i32 1, ptr %idp
  %ns = mul i64 %ms, 1000000
  %tp = getelementptr i8, ptr %sub, i32 24
  store i64 %ns, ptr %tp
  %e = call i32 @wasi.poll_oneoff(ptr %sub, ptr %ev, i32 1, ptr %got)
  br label %done
done:
  ret i64 0
}

define hidden i64 @nz.time_wall_ns() {
entry:
  %ns = alloca i64, align 8
  %e = call i32 @wasi.clock_time_get(i32 0, i64 1, ptr %ns)
  %t = load i64, ptr %ns
  ret i64 %t
}
"#;

/// The io part's file functions over WASI — a whole file read, written or asked after, beneath
/// the first preopened directory; and the `path` every one of them and the files part opens.
const IO: &str = r#"
; WASI's rights for a file or directory: every one preview 1 defines but bit 28, `sock_shutdown`,
; which a host grants no file — asked for, Node's refuses the open (`ENOTCAPABLE`).
@nz.wasi_rights = internal constant i64 268435455

; `n` bytes as a NUL-terminated copy the caller frees, as the hosted runtime's.
define hidden ptr @nz.cstr(ptr %p, i64 %n) {
entry:
  %room = add i64 %n, 1
  %buf = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %buf, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %p, i64 %n, i1 false)
  %end = getelementptr i8, ptr %buf, i64 %n
  store i8 0, ptr %end
  ret ptr %buf
}

; `path`'s `n` bytes made relative to descriptor 3, the first preopened directory: a leading `./`
; dropped. A path WASI cannot reach beneath it fails as WASI says.
define internal i32 @nz.wasi_open(ptr %path, i64 %n, i32 %oflags, i32 %fdflags, ptr %fd) {
entry:
  %two = icmp sge i64 %n, 2
  %c0 = load i8, ptr %path
  %p1 = getelementptr i8, ptr %path, i32 1
  %c1 = load i8, ptr %p1
  %dot = icmp eq i8 %c0, 46
  %slash = icmp eq i8 %c1, 47
  %ds = and i1 %dot, %slash
  %skip = and i1 %two, %ds
  %off = select i1 %skip, i64 2, i64 0
  %rest = sub i64 %n, %off
  %start = getelementptr i8, ptr %path, i64 %off
  %len = trunc i64 %rest to i32
  %rights = load i64, ptr @nz.wasi_rights
  %e = call i32 @wasi.path_open(i32 3, i32 1, ptr %start, i32 %len, i32 %oflags, i64 %rights, i64 %rights, i32 %fdflags, ptr %fd)
  ret i32 %e
}

; A whole file as a string the caller owns — its data, length and owner, through `out` — or three
; zeros if it cannot be read.
define hidden void @nz.read_file_to(ptr %out, ptr %p, i64 %n) {
entry:
  %fdp = alloca i32, align 4
  store { ptr, i64, ptr } zeroinitializer, ptr %out
  %e = call i32 @nz.wasi_open(ptr %p, i64 %n, i32 0, i32 0, ptr %fdp)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %done, label %opened
opened:
  %fd = load i32, ptr %fdp
  %endp = alloca i64, align 8
  %se = call i32 @wasi.fd_seek(i32 %fd, i64 0, i32 2, ptr %endp)
  %size = load i64, ptr %endp
  %back = call i32 @wasi.fd_seek(i32 %fd, i64 0, i32 0, ptr %endp)
  ; One byte more than the file, so an empty file still owns storage, as the hosted runtime's.
  %room = add i64 %size, 1
  %owner = call ptr @nz.str_alloc(i64 %room)
  %none = icmp eq ptr %owner, null
  br i1 %none, label %close, label %read
read:
  %data = getelementptr i8, ptr %owner, i32 16
  %iov = alloca [2 x i32], align 4
  %got = alloca i32, align 4
  br label %loop
loop:
  %at = phi i64 [ 0, %read ], [ %next, %more ]
  %left = sub i64 %size, %at
  %rem = icmp sgt i64 %left, 0
  br i1 %rem, label %chunk, label %whole
chunk:
  %to = getelementptr i8, ptr %data, i64 %at
  %toi = ptrtoint ptr %to to i32
  store i32 %toi, ptr %iov
  %lenp = getelementptr i32, ptr %iov, i32 1
  %len = trunc i64 %left to i32
  store i32 %len, ptr %lenp
  %re = call i32 @wasi.fd_read(i32 %fd, ptr %iov, i32 1, ptr %got)
  %g = load i32, ptr %got
  %rbad = icmp ne i32 %re, 0
  %eof = icmp eq i32 %g, 0
  %stop = or i1 %rbad, %eof
  br i1 %stop, label %short, label %more
more:
  %g64 = zext i32 %g to i64
  %next = add i64 %at, %g64
  br label %loop
short:
  call void @nz.str_release(ptr %owner)
  br label %close
whole:
  %t0 = insertvalue { ptr, i64, ptr } undef, ptr %data, 0
  %t1 = insertvalue { ptr, i64, ptr } %t0, i64 %size, 1
  %t2 = insertvalue { ptr, i64, ptr } %t1, ptr %owner, 2
  store { ptr, i64, ptr } %t2, ptr %out
  br label %close
close:
  %ce = call i32 @wasi.fd_close(i32 %fd)
  br label %done
done:
  ret void
}

define hidden i1 @nz.file_exists(ptr %p, i64 %n) {
entry:
  %fdp = alloca i32, align 4
  %e = call i32 @nz.wasi_open(ptr %p, i64 %n, i32 0, i32 0, ptr %fdp)
  %ok = icmp eq i32 %e, 0
  br i1 %ok, label %close, label %done
close:
  %fd = load i32, ptr %fdp
  %ce = call i32 @wasi.fd_close(i32 %fd)
  br label %done
done:
  ret i1 %ok
}

; `data` written to the file at `pp`, created or truncated: the bytes written, or -1.
define hidden i64 @nz.write_file(ptr %pp, i64 %pn, ptr %dp, i64 %dn) {
entry:
  %fdp = alloca i32, align 4
  %e = call i32 @nz.wasi_open(ptr %pp, i64 %pn, i32 9, i32 0, ptr %fdp)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %opened
opened:
  %fd = load i32, ptr %fdp
  %w = call i64 @write(i32 %fd, ptr %dp, i64 %dn)
  %ce = call i32 @wasi.fd_close(i32 %fd)
  ret i64 %w
fail:
  ret i64 -1
}
"#;

/// The files part's `nz_os_*` over WASI (Gate 3): paths beneath descriptor 3, WASI's error numbers
/// turned into Linux's — `nz_os_family` says 1 — through an `errno` of this unit's own.
const OS: &str = r#"
@nz.wasi_errno = internal global i32 0

define hidden ptr @nz_errno_location() {
entry:
  ret ptr @nz.wasi_errno
}

; WASI's error number as Linux's, set as `errno`: -1.
define internal i32 @nz.wasi_fail(i32 %e) {
entry:
  switch i32 %e, label %other [
    i32 44, label %noent
    i32 2, label %acces
    i32 76, label %acces
    i32 20, label %exist
    i32 31, label %isdir
    i32 54, label %notdir
    i32 8, label %badf
    i32 28, label %inval
    i32 55, label %notempty
    i32 63, label %perm
    i32 52, label %nosys
  ]
noent:
  store i32 2, ptr @nz.wasi_errno
  ret i32 -1
acces:
  store i32 13, ptr @nz.wasi_errno
  ret i32 -1
exist:
  store i32 17, ptr @nz.wasi_errno
  ret i32 -1
isdir:
  store i32 21, ptr @nz.wasi_errno
  ret i32 -1
notdir:
  store i32 20, ptr @nz.wasi_errno
  ret i32 -1
badf:
  store i32 9, ptr @nz.wasi_errno
  ret i32 -1
inval:
  store i32 22, ptr @nz.wasi_errno
  ret i32 -1
notempty:
  store i32 39, ptr @nz.wasi_errno
  ret i32 -1
perm:
  store i32 1, ptr @nz.wasi_errno
  ret i32 -1
nosys:
  store i32 38, ptr @nz.wasi_errno
  ret i32 -1
other:
  store i32 5, ptr @nz.wasi_errno
  ret i32 -1
}

; `mode` -1 opens to read; 0 creates or truncates, 1 creates or appends, 2 creates a new file,
; 3 reads and writes an existing one — WASI's `oflags` (creat 1, excl 4, trunc 8) and `fdflags`
; (append 1).
define hidden i32 @nz_os_open(ptr %path, i64 %mode) {
entry:
  %n = call i64 @strlen(ptr %path)
  %fdp = alloca i32, align 4
  %o0 = icmp eq i64 %mode, 0
  %o1 = icmp eq i64 %mode, 1
  %o2 = icmp eq i64 %mode, 2
  %of0 = select i1 %o0, i32 9, i32 0
  %of1 = select i1 %o1, i32 1, i32 %of0
  %oflags = select i1 %o2, i32 5, i32 %of1
  %fdflags = select i1 %o1, i32 1, i32 0
  %e = call i32 @nz.wasi_open(ptr %path, i64 %n, i32 %oflags, i32 %fdflags, ptr %fdp)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  %fd = load i32, ptr %fdp
  ret i32 %fd
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
}

define hidden i32 @nz_os_close(i32 %fd) {
entry:
  %e = call i32 @wasi.fd_close(i32 %fd)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  ret i32 0
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
}

define hidden i64 @nz_os_read(i32 %fd, ptr %buf, i64 %n) {
entry:
  %iov = alloca [2 x i32], align 4
  %got = alloca i32, align 4
  %bi = ptrtoint ptr %buf to i32
  store i32 %bi, ptr %iov
  %lenp = getelementptr i32, ptr %iov, i32 1
  %len = trunc i64 %n to i32
  store i32 %len, ptr %lenp
  %e = call i32 @wasi.fd_read(i32 %fd, ptr %iov, i32 1, ptr %got)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  %g = load i32, ptr %got
  %g64 = zext i32 %g to i64
  ret i64 %g64
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i64 -1
}

define hidden i64 @nz_os_write(i32 %fd, ptr %buf, i64 %n) {
entry:
  %w = call i64 @write(i32 %fd, ptr %buf, i64 %n)
  %bad = icmp slt i64 %w, 0
  br i1 %bad, label %fail, label %ok
ok:
  ret i64 %w
fail:
  store i32 5, ptr @nz.wasi_errno
  ret i64 -1
}

define hidden i64 @nz_os_lseek(i32 %fd, i64 %off, i32 %whence) {
entry:
  %at = alloca i64, align 8
  %e = call i32 @wasi.fd_seek(i32 %fd, i64 %off, i32 %whence, ptr %at)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  %p = load i64, ptr %at
  ret i64 %p
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i64 -1
}

define hidden i32 @nz_os_fsync(i32 %fd) {
entry:
  %e = call i32 @wasi.fd_sync(i32 %fd)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  ret i32 0
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
}

; One path operation beneath descriptor 3, its result as POSIX's: 0, or -1 with `errno`.
define internal i32 @nz.wasi_path(i32 %e) {
entry:
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
ok:
  ret i32 0
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
}

define hidden i32 @nz_os_mkdir(ptr %path) {
entry:
  %n = call i64 @strlen(ptr %path)
  %len = trunc i64 %n to i32
  %e = call i32 @wasi.path_create_directory(i32 3, ptr %path, i32 %len)
  %r = call i32 @nz.wasi_path(i32 %e)
  ret i32 %r
}

define hidden i32 @nz_os_rmdir(ptr %path) {
entry:
  %n = call i64 @strlen(ptr %path)
  %len = trunc i64 %n to i32
  %e = call i32 @wasi.path_remove_directory(i32 3, ptr %path, i32 %len)
  %r = call i32 @nz.wasi_path(i32 %e)
  ret i32 %r
}

define hidden i32 @nz_os_unlink(ptr %path) {
entry:
  %n = call i64 @strlen(ptr %path)
  %len = trunc i64 %n to i32
  %e = call i32 @wasi.path_unlink_file(i32 3, ptr %path, i32 %len)
  %r = call i32 @nz.wasi_path(i32 %e)
  ret i32 %r
}

define hidden i32 @nz_os_rename(ptr %from, ptr %to) {
entry:
  %fn = call i64 @strlen(ptr %from)
  %fl = trunc i64 %fn to i32
  %tn = call i64 @strlen(ptr %to)
  %tl = trunc i64 %tn to i32
  %e = call i32 @wasi.path_rename(i32 3, ptr %from, i32 %fl, i32 3, ptr %to, i32 %tl)
  %r = call i32 @nz.wasi_path(i32 %e)
  ret i32 %r
}

; Size, kind (0 a file, 1 a directory, 2 anything else), modification time in nanoseconds and
; permission bits — which WASI does not keep, so 0o644 — as four i64s at `out`.
define hidden i32 @nz_os_stat(ptr %path, ptr %out) {
entry:
  %n = call i64 @strlen(ptr %path)
  %len = trunc i64 %n to i32
  %st = alloca [64 x i8], align 8
  %e = call i32 @wasi.path_filestat_get(i32 3, i32 1, ptr %path, i32 %len, ptr %st)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
ok:
  %sizep = getelementptr i8, ptr %st, i32 32
  %size = load i64, ptr %sizep
  store i64 %size, ptr %out
  %tp = getelementptr i8, ptr %st, i32 16
  %t = load i8, ptr %tp
  %isfile = icmp eq i8 %t, 4
  %isdir = icmp eq i8 %t, 3
  %k1 = select i1 %isdir, i64 1, i64 2
  %kind = select i1 %isfile, i64 0, i64 %k1
  %kindp = getelementptr i8, ptr %out, i32 8
  store i64 %kind, ptr %kindp
  %mp = getelementptr i8, ptr %st, i32 48
  %mtime = load i64, ptr %mp
  %mtimep = getelementptr i8, ptr %out, i32 16
  store i64 %mtime, ptr %mtimep
  %permp = getelementptr i8, ptr %out, i32 24
  store i64 420, ptr %permp
  ret i32 0
}

; R1d: a path's own facts, a final link not followed (lookup flags 0): `os_stat`'s four — WASI's
; filetype 4 a file, 3 a directory, 7 a symbolic link — and its device and inode.
define hidden i32 @nz_os_lstat(ptr %path, ptr %out) {
entry:
  %n = call i64 @strlen(ptr %path)
  %len = trunc i64 %n to i32
  %st = alloca [64 x i8], align 8
  %e = call i32 @wasi.path_filestat_get(i32 3, i32 0, ptr %path, i32 %len, ptr %st)
  %bad = icmp ne i32 %e, 0
  br i1 %bad, label %fail, label %ok
fail:
  %r = call i32 @nz.wasi_fail(i32 %e)
  ret i32 %r
ok:
  %sizep = getelementptr i8, ptr %st, i32 32
  %size = load i64, ptr %sizep
  store i64 %size, ptr %out
  %tp = getelementptr i8, ptr %st, i32 16
  %t = load i8, ptr %tp
  %isfile = icmp eq i8 %t, 4
  %isdir = icmp eq i8 %t, 3
  %islink = icmp eq i8 %t, 7
  %k2 = select i1 %islink, i64 3, i64 2
  %k1 = select i1 %isdir, i64 1, i64 %k2
  %kind = select i1 %isfile, i64 0, i64 %k1
  %kindp = getelementptr i8, ptr %out, i32 8
  store i64 %kind, ptr %kindp
  %mp = getelementptr i8, ptr %st, i32 48
  %mtime = load i64, ptr %mp
  %mtimep = getelementptr i8, ptr %out, i32 16
  store i64 %mtime, ptr %mtimep
  %permp = getelementptr i8, ptr %out, i32 24
  store i64 420, ptr %permp
  %dev = load i64, ptr %st
  %devp = getelementptr i8, ptr %out, i32 32
  store i64 %dev, ptr %devp
  %inop = getelementptr i8, ptr %st, i32 8
  %ino = load i64, ptr %inop
  %inoout = getelementptr i8, ptr %out, i32 40
  store i64 %ino, ptr %inoout
  ret i32 0
}

; R1d: WASI preview 1 keeps no permissions, resolves no canonical path and takes no lock: each is
; ENOSYS, as listing a directory is.
define hidden i32 @nz_os_chmod(ptr %path, i64 %mode) {
entry:
  store i32 38, ptr @nz.wasi_errno
  ret i32 -1
}

define hidden i64 @nz_os_realpath(ptr %path, ptr %out, i64 %cap) {
entry:
  store i32 38, ptr @nz.wasi_errno
  ret i64 -1
}

define hidden i32 @nz_os_flock(i32 %fd, i32 %op) {
entry:
  store i32 38, ptr @nz.wasi_errno
  ret i32 -1
}

; Directories are not listed on WASI here: opening one to list is ENOSYS.
define hidden ptr @nz_os_opendir(ptr %path) {
entry:
  store i32 38, ptr @nz.wasi_errno
  ret ptr null
}

define hidden i64 @nz_os_readdir(ptr %dir, ptr %buf, i64 %cap) {
entry:
  ret i64 -38
}

define hidden i32 @nz_os_dirfd(ptr %dir) {
entry:
  ret i32 -1
}

define hidden i32 @nz_os_closedir(ptr %dir) {
entry:
  ret i32 0
}

define hidden i32 @nz_os_entropy(ptr %buf, i64 %n) {
entry:
  %len = trunc i64 %n to i32
  %e = call i32 @wasi.random_get(ptr %buf, i32 %len)
  %r = call i32 @nz.wasi_path(i32 %e)
  ret i32 %r
}

; WASI preview 1 has an environment, and this runtime does not read it: none.
@nz.wasi_environ = internal global [1 x ptr] zeroinitializer

define hidden ptr @nz_os_environ() {
entry:
  ret ptr @nz.wasi_environ
}

define hidden i64 @nz_os_family() {
entry:
  ret i64 1
}

; The message for a status: "error N", WASI's hosts keeping no table of their own.
define hidden i64 @nz_os_error_text(i64 %status, ptr %buf, i64 %cap) {
entry:
  %neg = icmp slt i64 %status, 0
  br i1 %neg, label %text, label %none
none:
  ret i64 0
text:
  %small = icmp slt i64 %cap, 28
  br i1 %small, label %none, label %write
write:
  store i8 101, ptr %buf
  %p1 = getelementptr i8, ptr %buf, i32 1
  store i8 114, ptr %p1
  %p2 = getelementptr i8, ptr %buf, i32 2
  store i8 114, ptr %p2
  %p3 = getelementptr i8, ptr %buf, i32 3
  store i8 111, ptr %p3
  %p4 = getelementptr i8, ptr %buf, i32 4
  store i8 114, ptr %p4
  %p5 = getelementptr i8, ptr %buf, i32 5
  store i8 32, ptr %p5
  %p6 = getelementptr i8, ptr %buf, i32 6
  %e = sub i64 0, %status
  %n = call i64 @nz.format_int(ptr %p6, i64 %e)
  %total = add i64 %n, 6
  ret i64 %total
}
"#;

/// The entry: WASI's `_start` — the arguments read into `nz.argc` and `nz.argv`, `main` run, and
/// its result written to standard output as a hosted program's is; or the failure reported and the
/// process ended with status 2.
#[must_use]
pub fn emit_entry(entry: &str, roots: usize) -> String {
    let mut out = String::new();
    preamble(&mut out, "<nazm WASI entry>");
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    let _ = writeln!(
        out,
        "@nz.failed = external global i1\n@nz.argc = external global i64\n@nz.argv = external global ptr\n\
         declare void @nz.report()\ndeclare ptr @malloc(i64)\ndeclare i64 @write(i32, ptr, i64)\n\
         declare i64 @nz.format_int(ptr, i64)\n\
         declare i32 @wasi.args_sizes_get(ptr, ptr) #0\ndeclare i32 @wasi.args_get(ptr, ptr) #1\n\
         declare void @wasi.proc_exit(i32) #2\n\
         declare i32 @wasi.environ_sizes_get(ptr, ptr) #4\ndeclare i32 @wasi.environ_get(ptr, ptr) #5\n\
         declare i32 @memcmp(ptr, ptr, i64)\n"
    );
    // `main`'s result (R1, 5b.3): the exit status, unless the environment holds the result
    // report's variable, looked for as `NAME=` at the start of an entry.
    let wanted = format!("{RESULT_VARIABLE}=");
    let _ = writeln!(
        out,
        "@.resultvar = private unnamed_addr constant [{} x i8] c\"{}\"\n\
         @.resultmsg = private unnamed_addr constant [{} x i8] c\"{}\\0A\"",
        wanted.len(),
        escape(&wanted),
        RESULT_STATUS.len() + 1,
        escape(RESULT_STATUS)
    );
    let wanted_len = wanted.len();
    let msg_len = RESULT_STATUS.len() + 1;
    let _ = write!(
        out,
        r"define void @_start() #3 {{
entry:
  %countp = alloca i32, align 4
  %sizep = alloca i32, align 4
  %e = call i32 @wasi.args_sizes_get(ptr %countp, ptr %sizep)
  %count = load i32, ptr %countp
  %size = load i32, ptr %sizep
  %count64 = zext i32 %count to i64
  %slots = add i64 %count64, 1
  %vbytes = shl i64 %slots, 2
  %argv = call ptr @malloc(i64 %vbytes)
  %size64 = zext i32 %size to i64
  %buf = call ptr @malloc(i64 %size64)
  %e2 = call i32 @wasi.args_get(ptr %argv, ptr %buf)
  ; One fewer than the count, because `argv[0]` is the program's own name and is not visible.
  %has = icmp sgt i64 %count64, 0
  %visible = sub i64 %count64, 1
  %n = select i1 %has, i64 %visible, i64 0
  store i64 %n, ptr @nz.argc
  store ptr %argv, ptr @nz.argv
  %result = call i64 {entry}({root_args})
  %failed = load i1, ptr @nz.failed
  br i1 %failed, label %reported, label %finished
reported:
  call void @nz.report()
  unreachable
finished:
  %ecountp = alloca i32, align 4
  %esizep = alloca i32, align 4
  %ee = call i32 @wasi.environ_sizes_get(ptr %ecountp, ptr %esizep)
  %ecount = load i32, ptr %ecountp
  %esize = load i32, ptr %esizep
  %ecount64 = zext i32 %ecount to i64
  %eslots = add i64 %ecount64, 1
  %evbytes = shl i64 %eslots, 2
  %environ = call ptr @malloc(i64 %evbytes)
  %esize64 = zext i32 %esize to i64
  %ebytes = add i64 %esize64, 1
  %ebuf = call ptr @malloc(i64 %ebytes)
  %ee2 = call i32 @wasi.environ_get(ptr %environ, ptr %ebuf)
  %ei = alloca i64, align 8
  store i64 0, ptr %ei
  br label %scan
scan:
  %i = load i64, ptr %ei
  %more = icmp ult i64 %i, %ecount64
  br i1 %more, label %entry_check, label %status
entry_check:
  %slot = getelementptr i32, ptr %environ, i64 %i
  %addr = load i32, ptr %slot
  %eaddr = zext i32 %addr to i64
  %entryp = inttoptr i64 %eaddr to ptr
  %cmp = call i32 @memcmp(ptr %entryp, ptr @.resultvar, i64 {wanted_len})
  %hit = icmp eq i32 %cmp, 0
  %next = add i64 %i, 1
  store i64 %next, ptr %ei
  br i1 %hit, label %print, label %scan
print:
  %digits = alloca [24 x i8], align 1
  %len = call i64 @nz.format_int(ptr %digits, i64 %result)
  %nl = getelementptr i8, ptr %digits, i64 %len
  store i8 10, ptr %nl
  %all = add i64 %len, 1
  %w = call i64 @write(i32 1, ptr %digits, i64 %all)
  call void @wasi.proc_exit(i32 0)
  unreachable
status:
  %inside = icmp ule i64 %result, 255
  br i1 %inside, label %exit_status, label %bad_status
exit_status:
  %code = trunc i64 %result to i32
  call void @wasi.proc_exit(i32 %code)
  unreachable
bad_status:
  %bw = call i64 @write(i32 2, ptr @.resultmsg, i64 {msg_len})
  call void @wasi.proc_exit(i32 2)
  unreachable
}}
"
    );
    let _ = writeln!(
        out,
        "attributes #0 = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"args_sizes_get\" }}\n\
         attributes #1 = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"args_get\" }}\n\
         attributes #2 = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"proc_exit\" }}\n\
         attributes #3 = {{ \"wasm-export-name\"=\"_start\" }}\n\
         attributes #4 = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"environ_sizes_get\" }}\n\
         attributes #5 = {{ \"wasm-import-module\"=\"{WASI_MODULE}\" \"wasm-import-name\"=\"environ_get\" }}"
    );
    out
}

/// The reference host for Node's WASI (`DIR/wasi.mjs`): `node wasi.mjs module.wasm DIR [ARGS…]`
/// runs the module with DIR preopened as `.` — the only directory it may reach — and the
/// arguments after it; the module's own status is the process's.
pub const HOST: &str = r#"// Written by nazm build for wasm32-wasip1 (Gate 3, docs/spec.md *WASI*).
// The reference host: `node wasi.mjs module.wasm DIR [ARGS...]`. It preopens DIR as `.`, and
// grants nothing else: no environment, no other directory.
import { readFileSync } from 'node:fs';
import { WASI } from 'node:wasi';
const [file, dir, ...args] = process.argv.slice(2);
const wasi = new WASI({ version: 'preview1', args: ['module', ...args], env: {}, preopens: { '.': dir ?? '.' }, returnOnExit: true });
const module = new WebAssembly.Module(readFileSync(file ?? 'module.wasm'));
for (const i of WebAssembly.Module.imports(module)) {
  if (i.module !== 'wasi_snapshot_preview1') {
    process.stderr.write(`nazm host: the module asks for ${i.module}.${i.name}, which WASI does not grant\n`);
    process.exit(3);
  }
}
const instance = new WebAssembly.Instance(module, wasi.getImportObject());
process.exitCode = wasi.start(instance);
"#;
