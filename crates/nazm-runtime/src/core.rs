//! The runtime's portable core (N43): its state, its failure and reporting helpers, and each
//! helper set a program may reach, as LLVM text defined once per executable.
//!
//! `docs/architecture.md` §7.45 is the contract; [`super::INVENTORY`] lists every symbol this
//! text defines, with its signature, ownership, failure behaviour and thread safety, and the
//! tests in [`super`] hold the two to each other. It reaches the operating system only through
//! [`super::os`]'s declarations.

use std::fmt::Write as _;

use super::os::declarations;

/// The environment variable that asks a compiled program to report what it reclaimed.
///
/// A diagnostic and not a language feature: it is read by `getenv` in the entry wrapper,
/// written to standard error, and invisible to every Nazm program. `docs/spec.md` states
/// *when* storage is reclaimed and deliberately not *how*, so a counter a program could
/// read would put the how into the language.
pub const MEMORY_VARIABLE: &str = "NAZM_MEMORY_REPORT";

/// What that report says. One line, three numbers a class, read back by the tests. The
/// closure objects' class came last, in N50.
pub const MEMORY_FORMAT: &str = "nazm-memory: sequences allocated=%lld reclaimed=%lld \
                                 live=%lld strings allocated=%lld reclaimed=%lld live=%lld \
                                 channels allocated=%lld reclaimed=%lld live=%lld \
                                 closures allocated=%lld reclaimed=%lld live=%lld";

/// The environment variable that asks a compiled program for its scheduler's counters (N54).
/// A diagnostic, like [`MEMORY_VARIABLE`]: written to standard error at exit, invisible to the
/// program.
pub const SCHED_VARIABLE: &str = "NAZM_SCHED_REPORT";

/// What that report says: four counts of what happened, never samples.
pub const SCHED_FORMAT: &str = "nazm-sched: spawned=%lld joined=%lld selects=%lld timeouts=%lld";
use crate::{preamble, Needs};

/// The runtime's own surface, as a unit that does not define it sees it.
///
/// The globals are `external`: one definition lives in the runtime unit and every other
/// object binds to it. Before N5 they were `internal` and there was only ever one module,
/// so the question did not arise.
pub fn runtime_declarations(out: &mut String) {
    // Generated from the inventory, so a declaration cannot disagree with it (N43).
    for g in super::GLOBALS {
        let _ = writeln!(out, "{}", g.declaration());
    }
    for f in super::INVENTORY.iter().filter(|f| f.declared) {
        let _ = writeln!(out, "{}", f.declaration());
    }
}

/// Emit the program's runtime support: defined once, for the whole executable.
///
/// # Ownership, as a rule rather than as a sequence of renamings
///
/// Everything with a definition that must exist **exactly once per program** lives here
/// and nowhere else: the failure state, the failure and reporting helpers, and each helper
/// set the program reached. They are `hidden` rather than `internal` so the linker can
/// bind another object's call to them, and `hidden` rather than external so the finished
/// executable exports none of them — a Nazm program has no ABI and this is where that is
/// enforced.
#[must_use]
pub fn emit_runtime(needs: Needs) -> String {
    let needs = needs.settled();
    let mut out = String::new();
    preamble(&mut out, "<nazm runtime>");
    declarations(&mut out);
    out.push('\n');

    // The process's arguments, stashed by the entry wrapper so that `args_count` and
    // `arg` need no parameter threaded through every Nazm function. Only where the
    // program reads them: a program that carries scaffolding it never reaches is the easy
    // mistake here, and `tests/build.rs` has checked for it since before there were units.
    if needs.io {
        let _ = writeln!(out, "@nz.argc = hidden global i64 0");
        let _ = writeln!(out, "@nz.argv = hidden global ptr null");
        let _ = writeln!(
            out,
            "@.rb = private unnamed_addr constant [3 x i8] c\"rb\\00\""
        );
        let _ = writeln!(
            out,
            "@.wb = private unnamed_addr constant [3 x i8] c\"wb\\00\""
        );
    }

    // The failure state is **thread-local**: two tasks can fail at once and neither may
    // see the other's message. Only the *first* failure on a thread is kept, so a second
    // one caused by continuing with the stand-in value of the first cannot overwrite it.
    let _ = writeln!(out, "@nz.failed = hidden thread_local global i1 false");
    let _ = writeln!(out, "@nz.errmsg = hidden thread_local global ptr null");
    let _ = writeln!(out, "@nz.errlen = hidden thread_local global i64 0");
    // The lowest frame address this thread may reach before `nz.stack_check` reports
    // (N49); zero until the thread's first check sets it. Only where something checks.
    if needs.stack {
        let _ = writeln!(out, "@nz.stack_limit = hidden thread_local global i64 0");
    }
    out.push('\n');

    // The accounting, unconditionally. Six counters cost six words in the data segment
    // and buy one thing: **every** program can be asked what it allocated and what it
    // reclaimed, including one that allocates nothing. Until N8 these lived inside the
    // sequence runtime, so a program with no sequence printed no report at all — which
    // reads as "nothing to say" and means "the instrument was not built in".
    for c in [
        "@nz.seq_allocated",
        "@nz.seq_reclaimed",
        "@nz.str_allocated",
        "@nz.str_reclaimed",
        "@nz.chan_allocated",
        "@nz.chan_reclaimed",
        "@nz.clo_allocated",
        "@nz.clo_reclaimed",
        "@nz.sched_spawned",
        "@nz.sched_joined",
        "@nz.sched_selects",
        "@nz.sched_timeouts",
    ] {
        let _ = writeln!(out, "{c} = hidden global i64 0");
    }
    out.push('\n');

    // Not `llvm.trap`: a process killed by SIGILL is not "stops with a diagnostic".
    //
    // **It records and returns; it does not exit.** `docs/spec.md` says a failure inside a
    // `scope` is reported at the closing brace, after every task has been joined — so the
    // thread that fails has to be able to walk back out to that brace.
    let _ = writeln!(out, "define hidden void @nz.fail(ptr %m, i64 %n) {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %had = load i1, ptr @nz.failed");
    let _ = writeln!(out, "  br i1 %had, label %done, label %record");
    let _ = writeln!(out, "record:");
    let _ = writeln!(out, "  store i1 true, ptr @nz.failed");
    let _ = writeln!(out, "  store ptr %m, ptr @nz.errmsg");
    let _ = writeln!(out, "  store i64 %n, ptr @nz.errlen");
    let _ = writeln!(out, "  br label %done");
    let _ = writeln!(out, "done:");
    let _ = writeln!(out, "  ret void");
    let _ = writeln!(out, "}}");
    out.push('\n');
    // The one place a diagnostic is written and the process ends: reached from the entry
    // wrapper, once every task has been joined.
    let _ = writeln!(out, "define hidden void @nz.report() noreturn {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %m = load ptr, ptr @nz.errmsg");
    let _ = writeln!(out, "  %n = load i64, ptr @nz.errlen");
    let _ = writeln!(out, "  %w = call i64 @write(i32 2, ptr %m, i64 %n)");
    let _ = writeln!(out, "  call void @exit(i32 2)");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "}}");
    if needs.stack {
        out.push_str(STACK_RUNTIME);
    }
    // The device-register built-ins (N62): unreachable in a hosted program, which a hosted build
    // refuses for using them, and defined here so one inventory describes every runtime.
    out.push_str(crate::board::MMIO_RUNTIME);

    if needs.sequences {
        out.push_str(SEQUENCE_RUNTIME);
        out.push_str(SUM_RUNTIME);
        out.push_str(SOA_RUNTIME);
    }
    if needs.strings {
        out.push_str(STRING_RUNTIME);
    }
    if needs.io {
        out.push_str(IO_RUNTIME);
    }
    if needs.join {
        out.push_str(JOIN_RUNTIME);
    }
    if needs.tasks || needs.channels {
        let (restore, save, set) = if needs.stack {
            (
                "  %slp = getelementptr i8, ptr %t, i64 104\n  %sl = load i64, ptr %slp\n  store i64 %sl, ptr @nz.stack_limit",
                "  %sl = load i64, ptr @nz.stack_limit\n  %slp = getelementptr i8, ptr %t, i64 104\n  store i64 %sl, ptr %slp\n  store i64 0, ptr @nz.stack_limit",
                "  %quarter = lshr i64 %size, 2\n  %lo = getelementptr i8, ptr %base, i64 %pg\n  %loi = ptrtoint ptr %lo to i64\n  %lim = add i64 %loi, %quarter\n  %limp = getelementptr i8, ptr %t, i64 104\n  store i64 %lim, ptr %limp",
            )
        } else {
            ("", "", "")
        };
        out.push_str(
            &POOL_RUNTIME
                .replace("{RESTORE_LIMIT}", restore)
                .replace("{SAVE_LIMIT}", save)
                .replace("{SET_LIMIT}", set),
        );
    }
    // With the pool's text present, every wait, wake and join goes through it (N83). The texts
    // themselves are the thread model's, unchanged — `compiler/emit.nz` carries them line by line
    // (the `runtime parity` gate) — and the pool is joined to them here, as they are emitted.
    let pooled = needs.tasks || needs.channels;
    let text = |t: &str| if pooled { pool_hooked(t) } else { t.to_owned() };
    if needs.tasks {
        out.push_str(&text(TASK_RUNTIME));
        out.push_str(SCHED_RUNTIME);
    }
    if needs.channels {
        out.push_str(&text(CHANNEL_RUNTIME));
        out.push_str(&text(GENERIC_CHANNEL_RUNTIME));
        out.push_str(&text(SELECT_RUNTIME));
    }
    if needs.strings {
        out.push_str(FOREIGN_STRING_RUNTIME);
        out.push_str(TEXT_RUNTIME);
    }
    if needs.foreign {
        out.push_str(ERRNO_RUNTIME);
    }
    if needs.numbers {
        out.push_str(NUMBER_RUNTIME);
    }
    if needs.closures {
        out.push_str(CLOSURE_RUNTIME);
    }
    if needs.shims {
        out.push_str(SHIM_RUNTIME);
        if needs.sequences {
            out.push_str(SHIM_SEQUENCE_RUNTIME);
        }
        if needs.io {
            out.push_str(SHIM_IO_RUNTIME);
        }
        if needs.join {
            out.push_str(SHIM_JOIN_RUNTIME);
        }
        if needs.tasks {
            out.push_str(SHIM_TASK_RUNTIME);
        }
    }
    out.push('\n');
    out
}

/// Closure objects (N50, `docs/architecture.md` §7.52): `{ i64 count, ptr code, ptr release,
/// captures… }`. `nz.closure_new` allocates one with a count of one; the caller stores the
/// captures after the header. The count is atomic, as a string backing's is. The last release
/// calls the object's release function — generated per closure, it releases each managed
/// capture — and frees the object.
const CLOSURE_RUNTIME: &str = r"
define hidden ptr @nz.closure_new(i64 %size, ptr %code, ptr %release) {
entry:
  %p = call ptr @malloc(i64 %size)
  %oom = icmp eq ptr %p, null
  br i1 %oom, label %none, label %fill
none:
  ret ptr null
fill:
  %n = atomicrmw add ptr @nz.clo_allocated, i64 1 monotonic
  store i64 1, ptr %p
  %cp = getelementptr i8, ptr %p, i64 8
  store ptr %code, ptr %cp
  %rp = getelementptr i8, ptr %p, i64 16
  store ptr %release, ptr %rp
  ret ptr %p
}

define hidden void @nz.closure_retain(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %bump
bump:
  %old = atomicrmw add ptr %c, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden void @nz.closure_release(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %drop
drop:
  %old = atomicrmw sub ptr %c, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %free, label %done
free:
  fence acquire
  %rp = getelementptr i8, ptr %c, i64 16
  %rel = load ptr, ptr %rp
  %has = icmp ne ptr %rel, null
  br i1 %has, label %captures, label %object
captures:
  call void %rel(ptr %c)
  br label %object
object:
  call void @free(ptr %c)
  %n = atomicrmw add ptr @nz.clo_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}
";

/// The stack guard (N49, `docs/spec.md` *Native recursion*), called first in every function in
/// a call cycle.
///
/// Nonzero when this frame is past the thread's limit. The limit is set on the thread's first
/// check: this frame's address minus three quarters of the smaller of `RLIMIT_STACK` and 8 MiB.
/// Task threads are started with 8 MiB (`nz.spawn`), and the main thread's stack is
/// `RLIMIT_STACK`, so no thread's budget exceeds the stack it has; the quarter left over is the
/// headroom for the non-recursive calls between two checks. It records nothing: the call site
/// names the function entered and fails with `N0408` itself.
/// `nz.soa_grow` (N68, `docs/architecture.md` §7.70): room for one more element of a `Vec` laid
/// out one 8-byte array per field. The arrays share one allocation, field `k` at `k · cap`; a
/// full vector moves to twice the capacity, each field's `len` elements copied to its new place.
const SOA_RUNTIME: &str = r"
define hidden ptr @nz.soa_grow(ptr %h, i64 %nf) {
entry:
  %len = load i64, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  %cap = load i64, ptr %capp
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %grow, label %have
have:
  ret ptr %data
grow:
  %empty = icmp eq i64 %cap, 0
  %dbl = shl i64 %cap, 1
  %nc = select i1 %empty, i64 4, i64 %dbl
  %row = mul i64 %nc, 8
  %bytes = mul i64 %row, %nf
  %new = call ptr @malloc(i64 %bytes)
  %oom = icmp eq ptr %new, null
  br i1 %oom, label %fail, label %copy
fail:
  ret ptr null
copy:
  %n = mul i64 %len, 8
  br label %field
field:
  %k = phi i64 [ 0, %copy ], [ %k.next, %field ]
  %to.off = mul i64 %k, %row
  %to = getelementptr i8, ptr %new, i64 %to.off
  %from.row = mul i64 %cap, 8
  %from.off = mul i64 %k, %from.row
  %from = getelementptr i8, ptr %data, i64 %from.off
  call void @llvm.memcpy.p0.p0.i64(ptr %to, ptr %from, i64 %n, i1 false)
  %k.next = add i64 %k, 1
  %more = icmp slt i64 %k.next, %nf
  br i1 %more, label %field, label %done
done:
  call void @free(ptr %data)
  store ptr %new, ptr %dp
  store i64 %nc, ptr %capp
  ret ptr %new
}
";

/// `nz.ints_sum` (N66, `docs/architecture.md` §7.68): the ordered checked sum, vectorisable.
const SUM_RUNTIME: &str = r"

; `ints_sum_from(start, v)` (N66): `start` plus every element of `v`, left to right, with the checked
; addition's exact failure — `*ovf` set to 1 at the first running sum that overflows. A block of 64
; elements whose magnitudes are all at most 2^56 is added without checks while the running sum's
; magnitude is below 2^62: then no partial sum inside it can leave `Int`, so the block's total is
; exactly what the checked additions would give, and the check-free loop is the one the code
; generator vectorises. Any other block is added one checked element at a time.
define hidden i64 @nz.ints_sum(ptr %h, i64 %start, ptr %ovf) {
entry:
  %len = load i64, ptr %h
  %df = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %df
  store i32 0, ptr %ovf
  br label %block

block:
  %i = phi i64 [ 0, %entry ], [ %i.next, %fast.done ], [ %i.slow, %slow.done ]
  %s = phi i64 [ %start, %entry ], [ %s.fast, %fast.done ], [ %s.slowout, %slow.done ]
  %rest = sub i64 %len, %i
  %whole = icmp sge i64 %rest, 64
  br i1 %whole, label %scan, label %tail

; The block's largest magnitude, as unsigned (so `i64::MIN`'s is 2^63).
scan:
  br label %scan.loop
scan.loop:
  %k = phi i64 [ 0, %scan ], [ %k.next, %scan.loop ]
  %m = phi i64 [ 0, %scan ], [ %m.next, %scan.loop ]
  %at = add i64 %i, %k
  %p = getelementptr i64, ptr %data, i64 %at
  %x = load i64, ptr %p
  %neg = sub i64 0, %x
  %isneg = icmp slt i64 %x, 0
  %mag = select i1 %isneg, i64 %neg, i64 %x
  %bigger = icmp ugt i64 %mag, %m
  %m.next = select i1 %bigger, i64 %mag, i64 %m
  %k.next = add i64 %k, 1
  %scanned = icmp eq i64 %k.next, 64
  br i1 %scanned, label %decide, label %scan.loop

decide:
  %small = icmp ule i64 %m.next, 72057594037927936
  %sneg = sub i64 0, %s
  %sisneg = icmp slt i64 %s, 0
  %smag = select i1 %sisneg, i64 %sneg, i64 %s
  %room = icmp ult i64 %smag, 4611686018427387904
  %safe = and i1 %small, %room
  br i1 %safe, label %fast, label %slow

fast:
  br label %fast.loop
fast.loop:
  %j = phi i64 [ 0, %fast ], [ %j.next, %fast.loop ]
  %acc = phi i64 [ 0, %fast ], [ %acc.next, %fast.loop ]
  %fat = add i64 %i, %j
  %fp = getelementptr i64, ptr %data, i64 %fat
  %fx = load i64, ptr %fp
  %acc.next = add i64 %acc, %fx
  %j.next = add i64 %j, 1
  %fdone = icmp eq i64 %j.next, 64
  br i1 %fdone, label %fast.done, label %fast.loop
fast.done:
  %s.fast = add i64 %s, %acc.next
  %i.next = add i64 %i, 64
  br label %block

; One block of 64, checked element by element.
slow:
  br label %slow.loop
slow.loop:
  %q = phi i64 [ 0, %slow ], [ %q.next, %slow.step ]
  %ss = phi i64 [ %s, %slow ], [ %ss.next, %slow.step ]
  %sat = add i64 %i, %q
  %sp = getelementptr i64, ptr %data, i64 %sat
  %sx = load i64, ptr %sp
  %r = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %ss, i64 %sx)
  %ss.next = extractvalue { i64, i1 } %r, 0
  %o = extractvalue { i64, i1 } %r, 1
  br i1 %o, label %overflow, label %slow.step
slow.step:
  %q.next = add i64 %q, 1
  %sdone = icmp eq i64 %q.next, 64
  br i1 %sdone, label %slow.done, label %slow.loop
slow.done:
  %s.slowout = phi i64 [ %ss.next, %slow.step ]
  %i.slow = add i64 %i, 64
  br label %block

; Fewer than 64 left: checked, one at a time.
tail:
  %t = phi i64 [ %i, %block ], [ %t.next, %tail.step ]
  %ts = phi i64 [ %s, %block ], [ %ts.next, %tail.step ]
  %tend = icmp sge i64 %t, %len
  br i1 %tend, label %done, label %tail.body
tail.body:
  %tp = getelementptr i64, ptr %data, i64 %t
  %tx = load i64, ptr %tp
  %tr = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %ts, i64 %tx)
  %ts.next = extractvalue { i64, i1 } %tr, 0
  %to = extractvalue { i64, i1 } %tr, 1
  br i1 %to, label %overflow, label %tail.step
tail.step:
  %t.next = add i64 %t, 1
  br label %tail

overflow:
  store i32 1, ptr %ovf
  ret i64 0
done:
  ret i64 %ts
}
";

const STACK_RUNTIME: &str = r"
define hidden i32 @nz.stack_check() {
entry:
  %rl = alloca [2 x i64], align 8
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %lim = load i64, ptr @nz.stack_limit
  %unset = icmp eq i64 %lim, 0
  br i1 %unset, label %init, label %compare
init:
  ; RLIMIT_STACK is 3 on every v1 target, and struct rlimit is two 64-bit words there.
  %rc = call i32 @getrlimit(i32 3, ptr %rl)
  %cur = load i64, ptr %rl
  %ok = icmp eq i32 %rc, 0
  %cap = select i1 %ok, i64 %cur, i64 8388608
  %small = icmp ult i64 %cap, 8388608
  %size = select i1 %small, i64 %cap, i64 8388608
  %quarter = lshr i64 %size, 2
  %budget = sub i64 %size, %quarter
  %under = icmp ult i64 %sp, %budget
  %raw = sub i64 %sp, %budget
  %limit = select i1 %under, i64 1, i64 %raw
  store i64 %limit, ptr @nz.stack_limit
  br label %compare
compare:
  %l = phi i64 [ %lim, %entry ], [ %limit, %init ]
  %past = icmp ult i64 %sp, %l
  %r = zext i1 %past to i32
  ret i32 %r
}
";

/// The registry's accessors and pointer-taking entries (`crate::runtime`, N41), for code a
/// second compiler generated. Each is a thin wrapper over an entry the LLVM text calls
/// directly; none allocates, frees or counts anything the wrapped one does not.
const SHIM_RUNTIME: &str = r#"
@.dec.rt = private unnamed_addr constant [5 x i8] c"%lld\00"

define hidden i32 @nz.failed_now() {
entry:
  %f = load i1, ptr @nz.failed
  %r = zext i1 %f to i32
  ret i32 %r
}

define hidden i64 @nz.format_int(ptr %buf, i64 %v) {
entry:
  %w = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %buf, i64 21, ptr @.dec.rt, i64 %v)
  %n = sext i32 %w to i64
  ret i64 %n
}
"#;

/// `nz.vec_release`: the per-element release `crate::emit`'s `vec_helper` writes for each
/// element type, once, with the element's release passed in — the same decrement, walk, frees
/// and count.
const SHIM_SEQUENCE_RUNTIME: &str = r"
define hidden void @nz.vec_release(ptr %h, ptr %f, i64 %stride) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %done, label %drop
drop:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = sub i64 %rc, 1
  store i64 %rc2, ptr %rcp
  %last = icmp sle i64 %rc2, 0
  br i1 %last, label %contents, label %done
contents:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %contents ], [ %next, %body ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %storage
body:
  %off = mul i64 %i, %stride
  %e = getelementptr i8, ptr %data, i64 %off
  call void %f(ptr %e)
  %next = add i64 %i, 1
  br label %test
storage:
  call void @free(ptr %data)
  call void @free(ptr %h)
  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}
";

const SHIM_IO_RUNTIME: &str = r"
define hidden void @nz.read_file_to(ptr %out, ptr %p, i64 %n) {
entry:
  %r = call { ptr, i64, ptr } @nz.read_file(ptr %p, i64 %n)
  store { ptr, i64, ptr } %r, ptr %out
  ret void
}
";

const SHIM_JOIN_RUNTIME: &str = r"
define hidden void @nz.str_join_to(ptr %out, ptr %h, ptr %sp, i64 %sn) {
entry:
  %r = call { ptr, i64, ptr } @nz.str_join(ptr %h, ptr %sp, i64 %sn)
  store { ptr, i64, ptr } %r, ptr %out
  ret void
}
";

const SHIM_TASK_RUNTIME: &str = r"
define hidden void @nz.scope_join_to(ptr %h, ptr %out) {
entry:
  %r = call { ptr, i64 } @nz.scope_join(ptr %h)
  store { ptr, i64 } %r, ptr %out
  ret void
}

define hidden void @nz.task_publish(ptr %a) {
entry:
  %failed = load i1, ptr @nz.failed
  br i1 %failed, label %publish, label %done
publish:
  %em = load ptr, ptr @nz.errmsg
  %el = load i64, ptr @nz.errlen
  %emp = getelementptr i8, ptr %a, i64 8
  store ptr %em, ptr %emp
  %elp = getelementptr i8, ptr %a, i64 16
  store i64 %el, ptr %elp
  br label %done
done:
  ret void
}
";

/// `str_join`, as generated LLVM.
///
/// A loop, so it cannot be inlined at the call site the way the other built-ins are: it
/// measures every part, allocates **once**, and copies. That is the whole point — the
/// same result built with `str_concat` in a loop is quadratic in time and memory, and
/// with nothing ever freed the compiler written in Nazm exhausted memory assembling its
/// own output before this existed.
///
/// Signals allocation failure by returning a null pointer, which the call site turns
/// into `N0406` at the right span.
const JOIN_RUNTIME: &str = r"
define hidden { ptr, i64, ptr } @nz.str_join(ptr %h, ptr %sp, i64 %sn) {
entry:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %sum
sum:
  %i = phi i64 [ 0, %entry ], [ %i2, %sumbody ]
  %acc = phi i64 [ 0, %entry ], [ %acc2, %sumbody ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %sumbody, label %alloc
sumbody:
  %e = getelementptr { ptr, i64, ptr }, ptr %data, i64 %i
  %lp = getelementptr i8, ptr %e, i64 8
  %li = load i64, ptr %lp
  %acc2 = add i64 %acc, %li
  %i2 = add i64 %i, 1
  br label %sum
alloc:
  ; One separator between each pair, so n-1 of them, and none for an empty sequence.
  %less = sub i64 %n, 1
  %neg = icmp slt i64 %less, 0
  %gaps = select i1 %neg, i64 0, i64 %less
  %sepbytes = mul i64 %gaps, %sn
  %total = add i64 %acc, %sepbytes
  ; One byte more, so an empty result still owns storage and its pointer stays valid.
  %room = add i64 %total, 1
  %own = call ptr @nz.str_alloc(i64 %room)
  %bad = icmp eq ptr %own, null
  br i1 %bad, label %fail, label %copy
copy:
  %buf = getelementptr i8, ptr %own, i64 16
  br label %loop
loop:
  %j = phi i64 [ 0, %copy ], [ %j2, %tail ]
  %off = phi i64 [ 0, %copy ], [ %off3, %tail ]
  %more2 = icmp slt i64 %j, %n
  br i1 %more2, label %body, label %done
body:
  %first = icmp eq i64 %j, 0
  br i1 %first, label %tail, label %gap
gap:
  %d1 = getelementptr i8, ptr %buf, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %d1, ptr %sp, i64 %sn, i1 false)
  %off1 = add i64 %off, %sn
  br label %tail
tail:
  %off2 = phi i64 [ %off, %body ], [ %off1, %gap ]
  %e2 = getelementptr { ptr, i64, ptr }, ptr %data, i64 %j
  %p2 = load ptr, ptr %e2
  %l2p = getelementptr i8, ptr %e2, i64 8
  %l2 = load i64, ptr %l2p
  %d2 = getelementptr i8, ptr %buf, i64 %off2
  call void @llvm.memcpy.p0.p0.i64(ptr %d2, ptr %p2, i64 %l2, i1 false)
  %off3 = add i64 %off2, %l2
  %j2 = add i64 %j, 1
  br label %loop
done:
  %a = insertvalue { ptr, i64, ptr } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64, ptr } %a, i64 %total, 1
  %b2 = insertvalue { ptr, i64, ptr } %b, ptr %own, 2
  ret { ptr, i64, ptr } %b2
fail:
  %z = insertvalue { ptr, i64, ptr } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64, ptr } %z, i64 0, 1
  %z3 = insertvalue { ptr, i64, ptr } %z2, ptr null, 2
  ret { ptr, i64, ptr } %z3
}
";

/// The file-system helpers, as generated LLVM.
///
/// Here rather than at the call sites because each is a dozen basic blocks and none of
/// them needs a source position: every failure is signalled by a null pointer or a
/// negative count, and the *call site* turns that into `N0407` at the right span. That
/// split is the same one the sequence helpers use, for the same reason.
///
/// A Nazm `Str` is not NUL-terminated — the specification makes an embedded zero an
/// ordinary byte — so `@nz.cstr` copies the path into a terminated buffer before any
/// libc call sees it. Passing the bytes directly would truncate a path at the first
/// zero, or read past the end of one that has none.
const IO_RUNTIME: &str = r"
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

define hidden i1 @nz.file_exists(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %no, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  ; The terminated copy has done its job the moment `fopen` returns. Until N8 every file
  ; operation leaked one of these.
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %no, label %yes
yes:
  %ignored = call i32 @fclose(ptr %f)
  ret i1 true
no:
  ret i1 false
}

define hidden { ptr, i64, ptr } @nz.read_file(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %measure
measure:
  ; SEEK_END is 2 and SEEK_SET is 0 on every target this is verified against.
  %e = call i32 @fseek(ptr %f, i64 0, i32 2)
  %len = call i64 @ftell(ptr %f)
  %unmeasurable = icmp slt i64 %len, 0
  br i1 %unmeasurable, label %closefail, label %reserve
reserve:
  %s = call i32 @fseek(ptr %f, i64 0, i32 0)
  ; One byte more than the file, so an empty file still owns storage and its pointer
  ; stays valid to compare.
  %room = add i64 %len, 1
  %own = call ptr @nz.str_alloc(i64 %room)
  %nobuf = icmp eq ptr %own, null
  br i1 %nobuf, label %closefail, label %read
read:
  %buf = getelementptr i8, ptr %own, i64 16
  %got = call i64 @fread(ptr %buf, i64 1, i64 %len, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %len
  br i1 %short, label %partial, label %done
partial:
  ; A short read is a failure, and the storage it was going to fill is released rather
  ; than abandoned.
  call void @nz.str_release(ptr %own)
  br label %fail
done:
  %a = insertvalue { ptr, i64, ptr } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64, ptr } %a, i64 %len, 1
  %b2 = insertvalue { ptr, i64, ptr } %b, ptr %own, 2
  ret { ptr, i64, ptr } %b2
closefail:
  %c2 = call i32 @fclose(ptr %f)
  br label %fail
fail:
  %z = insertvalue { ptr, i64, ptr } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64, ptr } %z, i64 0, 1
  %z3 = insertvalue { ptr, i64, ptr } %z2, ptr null, 2
  ret { ptr, i64, ptr } %z3
}

define hidden i64 @nz.write_file(ptr %pp, i64 %pn, ptr %dp, i64 %dn) {
entry:
  %path = call ptr @nz.cstr(ptr %pp, i64 %pn)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.wb)
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %write
write:
  %got = call i64 @fwrite(ptr %dp, i64 1, i64 %dn, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %dn
  br i1 %short, label %fail, label %done
done:
  ret i64 %got
fail:
  ret i64 -1
}
";

/// The two sequence helpers, as generated LLVM rather than a linked runtime crate.
///
/// Generated because that is the smallest thing that works: `docs/architecture.md` keeps
/// `nazm-rt` for when a runtime genuinely needs stack switching and a custom allocator,
/// and neither of these does. They are emitted only into programs that use a sequence.
///
/// **Bounds checks are not here.** They live at each call site, because the diagnostic
/// has to name that call's source position and a shared helper has only one. What is
/// here is allocation and growth, which have no position to report — both signal failure
/// by returning null, and the call site turns that into `N0406`.
///
/// The header is `{ i64 len, i64 cap, ptr data, i64 rc }`, 32 bytes. The handle is its
/// address and never moves, which is what makes a sequence a reference: growth rewrites
/// `data` and `cap` in place and every holder sees it.
///
/// # The reference count, and why it needs no atomic
///
/// `rc` is the number of live references, as `docs/spec.md`'s *memory constitution* counts
/// them: the bindings holding this handle, plus the one it is travelling in. When it
/// reaches zero the data and the header are freed, which is the whole of reclamation.
///
/// It is an ordinary `add`, not an atomic one, and that is a consequence of a rule the
/// language already had rather than an optimisation taken on trust. `N0321` refuses to let
/// a sequence cross into a task, so **no two threads can ever hold the same handle**, so
/// no two threads can ever touch the same count. A rule written for data-race safety pays
/// for itself a second time here; if it were ever relaxed, this would have to become an
/// atomic in the same commit.
///
/// # The counters
///
/// `@nz.seq_allocated` and `@nz.seq_reclaimed` exist so that a test can assert reclamation
/// happened rather than infer it from resident memory, which is an allocator's opinion. The
/// entry wrapper prints them when `NAZM_MEMORY_REPORT` is set in the environment and says
/// nothing otherwise. They are a runtime diagnostic, not a language feature: no Nazm
/// program can read them, and `docs/spec.md` does not mention them.
const SEQUENCE_RUNTIME: &str = r"
define hidden ptr @nz.arr_new() {
entry:
  %h = call ptr @malloc(i64 32)
  %bad = icmp eq ptr %h, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  store i64 0, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  store i64 0, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  store ptr null, ptr %datap
  ; Born with one reference: the expression that produced it. Whoever consumes that
  ; expression either takes the reference over or releases it.
  %rcp = getelementptr i8, ptr %h, i64 24
  store i64 1, ptr %rcp
  ret ptr %h
}

define hidden void @nz.arr_retain(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %done, label %bump
bump:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = add i64 %rc, 1
  store i64 %rc2, ptr %rcp
  br label %done
done:
  ret void
}

define hidden i1 @nz.arr_release(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %kept, label %drop
drop:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = sub i64 %rc, 1
  store i64 %rc2, ptr %rcp
  %last = icmp sle i64 %rc2, 0
  br i1 %last, label %free, label %kept
free:
  ; The data buffer first, then the header: after the header is freed its `data` field
  ; may not be read, and reading it is how this would become a use-after-free of its own.
  %datap = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %datap
  call void @free(ptr %data)
  call void @free(ptr %h)
  ret i1 true
kept:
  ret i1 false
}

; The counted pair. `@nz.arr_new` and `@nz.arr_release` are the allocator; these two are
; the allocator plus the bookkeeping that makes reclamation checkable. The scope machinery
; uses the uncounted pair for its list of task ids, which is not a Nazm value and must not
; appear in a report about the program's sequences.
define hidden ptr @nz.seq_new() {
entry:
  %h = call ptr @nz.arr_new()
  %bad = icmp eq ptr %h, null
  br i1 %bad, label %done, label %count
count:
  ; Atomic, unlike the reference count beside it: a *task* may create a sequence of its
  ; own, so two threads can reach this counter even though `N0321` stops them reaching
  ; one sequence. Until N8 this was an ordinary load-add-store and a concurrent program
  ; could under-report what it had allocated.
  %c = atomicrmw add ptr @nz.seq_allocated, i64 1 monotonic
  br label %done
done:
  ret ptr %h
}

define hidden void @nz.seq_release(ptr %h) {
entry:
  %freed = call i1 @nz.arr_release(ptr %h)
  br i1 %freed, label %count, label %done
count:
  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden ptr @nz.arr_grow(ptr %h, i64 %elem) {
entry:
  %len = load i64, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  %cap = load i64, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %grow, label %done
grow:
  ; Doubling, with a floor of four. `realloc(null, n)` is `malloc(n)`, so the first
  ; growth needs no special case. Both products are checked: an element may be a record
  ; or an enum of any size since N11, and a wrapped size would allocate less than the
  ; caller is about to write.
  %twice.r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %cap, i64 2)
  %twice = extractvalue {i64, i1} %twice.r, 0
  %twice.o = extractvalue {i64, i1} %twice.r, 1
  %tiny = icmp slt i64 %twice, 4
  %want = select i1 %tiny, i64 4, i64 %twice
  %bytes.r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %want, i64 %elem)
  %bytes = extractvalue {i64, i1} %bytes.r, 0
  %bytes.o = extractvalue {i64, i1} %bytes.r, 1
  %over = or i1 %twice.o, %bytes.o
  br i1 %over, label %oom, label %sized
sized:
  %old = load ptr, ptr %datap
  %new = call ptr @realloc(ptr %old, i64 %bytes)
  %failed = icmp eq ptr %new, null
  br i1 %failed, label %oom, label %kept
kept:
  ; The header is updated only after the allocation succeeded, so a failed growth
  ; leaves the sequence exactly as it was rather than half-resized.
  store ptr %new, ptr %datap
  store i64 %want, ptr %capp
  br label %done
oom:
  ret ptr null
done:
  %data = load ptr, ptr %datap
  ret ptr %data
}
";

/// A string's backing allocation, as generated LLVM.
///
/// The layout is `{ i64 rc, i64 length, bytes… }`, and the **owner** field of a `Str` is
/// this block's address while its **pointer** field is `owner + 16` or anywhere after it.
/// That separation is the whole of N8's string model: the bytes a value names and the
/// allocation that keeps them valid are different addresses, so a slice can move one
/// without losing the other.
///
/// # Null is a backing
///
/// A literal's owner and a process argument's owner are both null, and every operation
/// here starts by testing for it. So *immortal* and *external* need no tag of their own
/// and no branch anywhere else in the compiler: they are simply strings that own nothing,
/// and retaining or releasing one costs a comparison.
///
/// # Why this count is atomic and a sequence's is not
///
/// `docs/spec.md` lets a `Str` cross into a task and refuses to let a sequence cross
/// (`N0321`). A task can therefore be handed a string, slice it, bind the slice — and two
/// sibling tasks doing that to the same backing really are two threads adjusting one
/// count. A sequence's count cannot be reached by two threads at all, which is why that
/// one stays an ordinary `add`. Neither choice is a guess: each follows from a rule the
/// language already had.
///
/// The decrement is `release` and the free is preceded by an `acquire` fence, which is the
/// standard pairing: it is what makes every other thread's writes to the buffer visible to
/// the thread that frees it.
const STRING_RUNTIME: &str = r"
define hidden ptr @nz.str_alloc(i64 %n) {
entry:
  ; Checked, because a length that overflows here would allocate less than the caller is
  ; about to write. `docs/spec.md` reports it as N0406 at the call site.
  %r = call {i64, i1} @llvm.uadd.with.overflow.i64(i64 %n, i64 16)
  %over = extractvalue {i64, i1} %r, 1
  br i1 %over, label %fail, label %ask
ask:
  %room = extractvalue {i64, i1} %r, 0
  %b = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %b, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  store i64 1, ptr %b
  %lenp = getelementptr i8, ptr %b, i64 8
  store i64 %n, ptr %lenp
  %c = atomicrmw add ptr @nz.str_allocated, i64 1 monotonic
  ret ptr %b
}

define hidden void @nz.str_retain(ptr %b) {
entry:
  ; Null is immortal or external storage: a literal, or a process argument. Nothing to
  ; count, and nothing this program may ever free.
  %none = icmp eq ptr %b, null
  br i1 %none, label %done, label %bump
bump:
  %old = atomicrmw add ptr %b, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden void @nz.str_release(ptr %b) {
entry:
  %none = icmp eq ptr %b, null
  br i1 %none, label %done, label %drop
drop:
  %old = atomicrmw sub ptr %b, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %free, label %done
free:
  fence acquire
  call void @free(ptr %b)
  %c = atomicrmw add ptr @nz.str_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

; A `Strs` owns a reference to every string in it, so its storage cannot be released by
; the helper an `Ints` uses. Elements first, then the array, then the header — the order
; matters, because reading an element out of a buffer that has already been freed is the
; use-after-free this whole milestone exists to make impossible.
define hidden void @nz.strs_release(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %done, label %drop
drop:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = sub i64 %rc, 1
  store i64 %rc2, ptr %rcp
  %last = icmp sle i64 %rc2, 0
  br i1 %last, label %contents, label %done
contents:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %contents ], [ %next, %body ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %storage
body:
  %e = getelementptr { ptr, i64, ptr }, ptr %data, i64 %i
  %op = getelementptr i8, ptr %e, i64 16
  %owner = load ptr, ptr %op
  call void @nz.str_release(ptr %owner)
  %next = add i64 %i, 1
  br label %test
storage:
  call void @free(ptr %data)
  call void @free(ptr %h)
  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}
";

/// Starting a task, and waiting for one, as generated LLVM.
///
/// # The slot is reserved before the thread starts
///
/// `@nz.spawn` records the task **first** and undoes the record if `pthread_create`
/// fails. A thread that started and could not be recorded would never be joined, and a
/// scope that cannot join a task it started is not a scope — so the failure that must not
/// happen is "started but untracked", and this is the order that makes it impossible.
///
/// # What is stored
///
/// One pointer per task: the argument block, whose first eight bytes `pthread_create`
/// writes the thread handle into. The block is allocated by the spawn site anyway, so
/// using its head as the handle's storage costs nothing and keeps the list to one
/// allocation.
///
/// Returns `0` on success, `-1` when the list could not grow, and `1` when the thread
/// could not be started. The call site turns those into `N0406` and `N0404`.
const TASK_RUNTIME: &str = r"
define hidden i32 @nz.spawn(ptr %h, ptr %fn, ptr %blk) {
entry:
  ; Every task thread has 8 MiB of stack (N49): the size `nz.stack_check` budgets from, and
  ; Linux's default already. The attribute object is opaque; 128 bytes covers every v1 target.
  %attr = alloca [128 x i8], align 16
  %data = call ptr @nz.arr_grow(ptr %h, i64 8)
  %oom = icmp eq ptr %data, null
  br i1 %oom, label %nomem, label %record
nomem:
  ret i32 -1
record:
  %len = load i64, ptr %h
  %slot = getelementptr i64, ptr %data, i64 %len
  %raw = ptrtoint ptr %blk to i64
  store i64 %raw, ptr %slot
  %next = add i64 %len, 1
  store i64 %next, ptr %h
  %ai = call i32 @pthread_attr_init(ptr %attr)
  %as = call i32 @pthread_attr_setstacksize(ptr %attr, i64 8388608)
  %rc = call i32 @pthread_create(ptr %blk, ptr %attr, ptr %fn, ptr %blk)
  %ad = call i32 @pthread_attr_destroy(ptr %attr)
  %bad = icmp ne i32 %rc, 0
  br i1 %bad, label %undo, label %ok
undo:
  store i64 %len, ptr %h
  ret i32 1
ok:
  ret i32 0
}

; Joins every task and reports the first failure any of them had, as { message, length },
; or { null, 0 } if none did.
;
; **Every** task is joined before anything is reported, including after one has been found
; to have failed — returning at the first would leave the rest running, and a scope that
; does not contain its tasks is not a scope. The failure taken is the earliest by position
; in the list, which is the order the tasks were spawned in, so which one is reported does
; not depend on which thread happened to finish first.
;
; No lock is needed. A task writes its own diagnostic into its own argument block before
; its thread ends, and this reads that block only after `pthread_join` has returned for
; it, which is a happens-before edge.
define hidden { ptr, i64 } @nz.scope_join(ptr %h) {
entry:
  ; The count is read once. Nothing can be added while this runs: the scope's body has
  ; finished by every path that reaches here, and a task cannot spawn into its parent.
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %entry ], [ %next, %after ]
  %msg = phi ptr [ null, %entry ], [ %msg2, %after ]
  %len = phi i64 [ 0, %entry ], [ %len2, %after ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %ep = getelementptr i64, ptr %data, i64 %i
  %raw = load i64, ptr %ep
  %blk = inttoptr i64 %raw to ptr
  %tid = load ptr, ptr %blk
  %rc = call i32 @pthread_join(ptr %tid, ptr null)
  ; Joined, so the thread has ended and nothing will read this block again. Its arguments
  ; were released by the trampoline on its way out; this is the block itself, one
  ; `malloc` per `spawn` that until N8 nothing ever gave back.
  ; The task's own diagnostic, at a fixed offset in the block it was given.
  %emp = getelementptr i8, ptr %blk, i64 8
  %em = load ptr, ptr %emp
  %elp = getelementptr i8, ptr %blk, i64 16
  %el = load i64, ptr %elp
  %none = icmp eq ptr %msg, null
  %this = icmp ne ptr %em, null
  %take = and i1 %none, %this
  ; Read before the block is freed, and the message itself is a constant in the object's
  ; read-only data rather than anything the block owns — so what is carried out stays
  ; valid after the free below.
  call void @free(ptr %blk)
  br i1 %take, label %first, label %after
first:
  br label %after
after:
  %msg2 = phi ptr [ %em, %first ], [ %msg, %body ]
  %len2 = phi i64 [ %el, %first ], [ %len, %body ]
  %next = add i64 %i, 1
  br label %test
done:
  %r0 = insertvalue { ptr, i64 } undef, ptr %msg, 0
  %r1 = insertvalue { ptr, i64 } %r0, i64 %len, 1
  ret { ptr, i64 } %r1
}
";

/// A bounded channel, as generated LLVM: one mutex, one condition variable, one ring.
///
/// The header is
/// `{ i64 cap, i64 len, i64 head, i64 closed, ptr data, ptr mutex, ptr cond, i64 rc }`.
///
/// # When a channel may be destroyed, and why no waiter can be left
///
/// At `rc == 0`, and the argument is structural rather than hopeful. A reference is a
/// binding that holds the handle, a value on its way out of a `return`, or the argument
/// block of a task that has not been joined. A thread blocked in `chan_send` or
/// `chan_recv` reached it **from a frame that holds the handle**, because a built-in
/// borrows its argument and the caller keeps the reference across the call — so a waiter
/// implies a live reference, and `rc == 0` implies no waiter. The count is also why
/// closing is not destruction: `chan_close` changes what the channel does and touches no
/// storage at all.
///
/// The count is atomic for the reason a string backing's is: `docs/spec.md` lets a `Chan`
/// cross into a task, so two tasks really can bind the same channel at once.
///
/// # One condition variable, broadcast every time
///
/// Senders and receivers wait on the same variable and every change wakes all of them.
/// Not the fastest arrangement and the obviously correct one — `docs/spec.md` promises no
/// fairness, and a signal-one scheme has to be right about *which* one, which is where
/// the lost-wakeup bugs live. The interpreter's `Channel` is built the same way, so the
/// two agree by construction rather than by testing.
///
/// # The waits are loops
///
/// `pthread_cond_wait` may return without the condition holding. Every wait here is
/// inside a loop that re-reads the state, which is the only correct way to use one.
/// A generic channel, `Chan[T]` (N53): the `Int` channel's header, mutex, condition variable
/// and waits exactly — so `nz.chan_retain` and `nz.chan_close` serve both — with a ring of
/// fixed-size slots instead of `i64`s. The header goes on past the `Int` channel's:
/// `… i64 rc @56, i64 stride @64, ptr release @72`. Send and receive copy one slot under the
/// lock; the last release calls `release` on each slot still queued, then frees what
/// `nz.chan_release` frees. The ownership of a slot's contents is the caller's to arrange: a
/// sender hands over a reference of its own, a receiver takes it.
/// Select, deadlines and the clock (N54, `docs/architecture.md` §7.56).
///
/// A select cannot wait on several channels' condition variables, so every typed channel's
/// send and close advances one process-wide event count, `@nz.sel_gen`, and wakes whoever is
/// selecting. A selector reads the count, scans its channels in index order under each one's
/// lock, and waits only while the count has not moved since — under the count's own lock, so a
/// wake-up between its scan and its wait is never lost. `@nz.sel_waiters` lets a send skip the
/// lock when nobody selects. The lock and its condition variable are made once, on first use,
/// and live as long as the process.
const SELECT_RUNTIME: &str = r"
@nz.sel_gen = hidden global i64 0
@nz.sel_waiters = hidden global i64 0
@nz.sel_sync = hidden global ptr null

define hidden ptr @nz.sel_sync_get() {
entry:
  %s = load atomic ptr, ptr @nz.sel_sync acquire, align 8
  %unmade = icmp eq ptr %s, null
  br i1 %unmade, label %make, label %have
have:
  ret ptr %s
make:
  %fresh = call ptr @malloc(i64 256)
  %oom = icmp eq ptr %fresh, null
  br i1 %oom, label %nomem, label %init
nomem:
  ret ptr null
init:
  %mi = call i32 @pthread_mutex_init(ptr %fresh, ptr null)
  %fcv = getelementptr i8, ptr %fresh, i64 128
  %ci = call i32 @pthread_cond_init(ptr %fcv, ptr null)
  %pair = cmpxchg ptr @nz.sel_sync, ptr null, ptr %fresh acq_rel acquire
  %won = extractvalue {ptr, i1} %pair, 1
  br i1 %won, label %mine, label %theirs
mine:
  ret ptr %fresh
theirs:
  %md = call i32 @pthread_mutex_destroy(ptr %fresh)
  %cd = call i32 @pthread_cond_destroy(ptr %fcv)
  call void @free(ptr %fresh)
  %other = extractvalue {ptr, i1} %pair, 0
  ret ptr %other
}

; Called after a typed channel changed in a way a selector waits for: a value queued, or a close.
define hidden void @nz.sel_signal() {
entry:
  %gen = atomicrmw add ptr @nz.sel_gen, i64 1 seq_cst
  %who = load atomic i64, ptr @nz.sel_waiters seq_cst, align 8
  %any = icmp sgt i64 %who, 0
  br i1 %any, label %wake, label %quiet
wake:
  %s = call ptr @nz.sel_sync_get()
  %lost = icmp eq ptr %s, null
  br i1 %lost, label %quiet, label %tell
tell:
  %l = call i32 @pthread_mutex_lock(ptr %s)
  %scv = getelementptr i8, ptr %s, i64 128
  %bc = call i32 @pthread_cond_broadcast(ptr %scv)
  %u = call i32 @pthread_mutex_unlock(ptr %s)
  br label %quiet
quiet:
  ret void
}

; `chan_close_of`: the `Int` channel's close, then the event a selector waits for.
define hidden void @nz.chanv_close(ptr %c) {
entry:
  call void @nz.chan_close(ptr %c)
  call void @nz.sel_signal()
  ret void
}

; One look at one channel, without waiting: 1 and the oldest slot copied out to `%dst` (its
; reference handed over), 2 when closed and drained, 0 when neither.
define hidden i64 @nz.chanv_try(ptr %c, ptr %dst) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %sp = getelementptr i8, ptr %c, i64 64
  %stride = load i64, ptr %sp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  %queued = load i64, ptr %lenp
  %has = icmp sgt i64 %queued, 0
  br i1 %has, label %take, label %ask
ask:
  %shut = load i64, ptr %closedp
  %isshut = icmp ne i64 %shut, 0
  %why = select i1 %isshut, i64 2, i64 0
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i64 %why
take:
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %data = load ptr, ptr %datap
  %off = mul i64 %head, %stride
  %slot = getelementptr i8, ptr %data, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %slot, i64 %stride, i1 false)
  %h1 = add i64 %head, 1
  %nh = urem i64 %h1, %cap
  store i64 %nh, ptr %headp
  %left = sub i64 %queued, 1
  store i64 %left, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i64 1
}

; `%h` is a `Vec[Chan[T]]`; `%timed` nonzero gives up after `%ms` milliseconds. The result:
; `i` a value from channel `i` in `%dst`, `-3 - i` channel `i` closed and drained, `-1` no
; channels, `-2` timed out, and the most negative `i64` out of memory — which no `-3 - i`
; can be. The lowest ready index wins.
define hidden i64 @nz.chanv_select(ptr %h, ptr %dst, i64 %ms, i64 %timed) {
entry:
  %ts = alloca [2 x i64], align 8
  %now = alloca [2 x i64], align 8
  %count = load i64, ptr %h
  %nothing = icmp sle i64 %count, 0
  br i1 %nothing, label %none, label %setup
none:
  ret i64 -1
setup:
  %s = call ptr @nz.sel_sync_get()
  %oom = icmp eq ptr %s, null
  br i1 %oom, label %nomem, label %counted
nomem:
  ret i64 -9223372036854775808
counted:
  %sel = atomicrmw add ptr @nz.sched_selects, i64 1 monotonic
  %in = atomicrmw add ptr @nz.sel_waiters, i64 1 seq_cst
  %istimed = icmp ne i64 %timed, 0
  br i1 %istimed, label %deadline, label %look
deadline:
  ; CLOCK_REALTIME is 0 on every v1 target, and it is what pthread_cond_timedwait measures.
  %r0 = call i32 @clock_gettime(i32 0, ptr %ts)
  %sec0 = load i64, ptr %ts
  %tsn = getelementptr i8, ptr %ts, i64 8
  %nsec0 = load i64, ptr %tsn
  %pos = icmp sgt i64 %ms, 0
  %wait_ms = select i1 %pos, i64 %ms, i64 0
  %addsec = sdiv i64 %wait_ms, 1000
  %addms = srem i64 %wait_ms, 1000
  %addns = mul i64 %addms, 1000000
  %ns1 = add i64 %nsec0, %addns
  %carry = icmp sge i64 %ns1, 1000000000
  %ns2 = sub i64 %ns1, 1000000000
  %nsf = select i1 %carry, i64 %ns2, i64 %ns1
  %sec1 = add i64 %sec0, %addsec
  %sec2 = add i64 %sec1, 1
  %secf = select i1 %carry, i64 %sec2, i64 %sec1
  store i64 %secf, ptr %ts
  store i64 %nsf, ptr %tsn
  br label %look
look:
  %g0 = load atomic i64, ptr @nz.sel_gen seq_cst, align 8
  %cdp = getelementptr i8, ptr %h, i64 16
  %cdata = load ptr, ptr %cdp
  br label %scan
scan:
  %i = phi i64 [ 0, %look ], [ %inext, %onward ]
  %more = icmp slt i64 %i, %count
  br i1 %more, label %probe, label %idle
probe:
  %coff = mul i64 %i, 8
  %cpp = getelementptr i8, ptr %cdata, i64 %coff
  %ch = load ptr, ptr %cpp
  %got = call i64 @nz.chanv_try(ptr %ch, ptr %dst)
  %took = icmp eq i64 %got, 1
  br i1 %took, label %value, label %closedq
closedq:
  %drained = icmp eq i64 %got, 2
  br i1 %drained, label %ended, label %onward
onward:
  %inext = add i64 %i, 1
  br label %scan
value:
  %o1 = atomicrmw sub ptr @nz.sel_waiters, i64 1 seq_cst
  ret i64 %i
ended:
  %o2 = atomicrmw sub ptr @nz.sel_waiters, i64 1 seq_cst
  %enc = sub i64 -3, %i
  ret i64 %enc
idle:
  br i1 %istimed, label %check, label %wait
check:
  %r1 = call i32 @clock_gettime(i32 0, ptr %now)
  %nsec = load i64, ptr %now
  %nwn = getelementptr i8, ptr %now, i64 8
  %nnsec = load i64, ptr %nwn
  %dsec = load i64, ptr %ts
  %dtn = getelementptr i8, ptr %ts, i64 8
  %dnsec = load i64, ptr %dtn
  %later = icmp sgt i64 %nsec, %dsec
  %samesec = icmp eq i64 %nsec, %dsec
  %nslate = icmp sge i64 %nnsec, %dnsec
  %atlast = and i1 %samesec, %nslate
  %past = or i1 %later, %atlast
  br i1 %past, label %timeout, label %wait
timeout:
  %o3 = atomicrmw sub ptr @nz.sel_waiters, i64 1 seq_cst
  %to = atomicrmw add ptr @nz.sched_timeouts, i64 1 monotonic
  ret i64 -2
wait:
  %wl = call i32 @pthread_mutex_lock(ptr %s)
  %wcv = getelementptr i8, ptr %s, i64 128
  br label %recheck
recheck:
  %g1 = load atomic i64, ptr @nz.sel_gen seq_cst, align 8
  %still = icmp eq i64 %g1, %g0
  br i1 %still, label %sleep, label %woke
sleep:
  br i1 %istimed, label %tsleep, label %fsleep
tsleep:
  ; Woken, timed out or spurious alike: look again, and the deadline check decides.
  %tw = call i32 @pthread_cond_timedwait(ptr %wcv, ptr %s, ptr %ts)
  br label %woke
fsleep:
  %fw = call i32 @pthread_cond_wait(ptr %wcv, ptr %s)
  br label %recheck
woke:
  %wu = call i32 @pthread_mutex_unlock(ptr %s)
  br label %look
}

; A monotonic clock in milliseconds. Clock 4 is CLOCK_MONOTONIC_RAW on both Linux and macOS.
define hidden i64 @nz.time_now_ms() {
entry:
  %tp = alloca [2 x i64], align 8
  %r = call i32 @clock_gettime(i32 4, ptr %tp)
  %sec = load i64, ptr %tp
  %tpn = getelementptr i8, ptr %tp, i64 8
  %nsec = load i64, ptr %tpn
  %msec = mul i64 %sec, 1000
  %frac = sdiv i64 %nsec, 1000000
  %t = add i64 %msec, %frac
  ret i64 %t
}
";

/// A `Str` argument to a foreign function (N55, §7.57). `nz.cstr_has_nul` says whether the bytes
/// hold a NUL, which no C string can; `nz.cstr_borrow` makes the NUL-terminated copy C borrows for
/// the call, or null out of memory, and the caller frees it after the call. Their own part,
/// outside the string runtime the self-hosted compiler's parity gate holds.
/// Bytes and text (Gate 2, `docs/general-purpose.md` §5–§6): UTF-8 validation, counting,
/// decoding and encoding, and zeroing and moving a buffer's bytes. Its own text rather than
/// `STRING_RUNTIME`'s, which `compiler/emit.nz` carries line by line and needs none of these.
const TEXT_RUNTIME: &str = r"
define hidden i1 @nz.utf8_valid(ptr %p, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %end, %ok ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %lead, label %yes
lead:
  %bp = getelementptr i8, ptr %p, i64 %i
  %b8 = load i8, ptr %bp
  %b = zext i8 %b8 to i32
  %is1 = icmp ult i32 %b, 128
  %ge2 = icmp uge i32 %b, 194
  %le2 = icmp ule i32 %b, 223
  %is2 = and i1 %ge2, %le2
  %ge3 = icmp uge i32 %b, 224
  %le3 = icmp ule i32 %b, 239
  %is3 = and i1 %ge3, %le3
  %ge4 = icmp uge i32 %b, 240
  %le4 = icmp ule i32 %b, 244
  %is4 = and i1 %ge4, %le4
  %l4 = select i1 %is4, i32 4, i32 0
  %l3 = select i1 %is3, i32 3, i32 %l4
  %l2 = select i1 %is2, i32 2, i32 %l3
  %len = select i1 %is1, i32 1, i32 %l2
  %e0 = icmp eq i32 %b, 224
  %f0 = icmp eq i32 %b, 240
  %lo1 = select i1 %f0, i32 144, i32 128
  %lo = select i1 %e0, i32 160, i32 %lo1
  %ed = icmp eq i32 %b, 237
  %f4 = icmp eq i32 %b, 244
  %hi1 = select i1 %f4, i32 143, i32 191
  %hi = select i1 %ed, i32 159, i32 %hi1
  %none = icmp eq i32 %len, 0
  %len64 = zext i32 %len to i64
  %end = add i64 %i, %len64
  %over = icmp sgt i64 %end, %n
  %bad = or i1 %none, %over
  br i1 %bad, label %no, label %check
check:
  %need1 = icmp uge i32 %len, 2
  br i1 %need1, label %c1, label %ok
c1:
  %p1 = getelementptr i8, ptr %bp, i64 1
  %x1 = load i8, ptr %p1
  %y1 = zext i8 %x1 to i32
  %a1 = icmp uge i32 %y1, %lo
  %z1 = icmp ule i32 %y1, %hi
  %in1 = and i1 %a1, %z1
  %need2 = icmp uge i32 %len, 3
  br i1 %in1, label %t2, label %no
t2:
  br i1 %need2, label %c2, label %ok
c2:
  %p2 = getelementptr i8, ptr %bp, i64 2
  %x2 = load i8, ptr %p2
  %y2 = zext i8 %x2 to i32
  %m2 = and i32 %y2, 192
  %in2 = icmp eq i32 %m2, 128
  %need3 = icmp uge i32 %len, 4
  br i1 %in2, label %t3, label %no
t3:
  br i1 %need3, label %c3, label %ok
c3:
  %p3 = getelementptr i8, ptr %bp, i64 3
  %x3 = load i8, ptr %p3
  %y3 = zext i8 %x3 to i32
  %m3 = and i32 %y3, 192
  %in3 = icmp eq i32 %m3, 128
  br i1 %in3, label %ok, label %no
ok:
  br label %loop
yes:
  ret i1 true
no:
  ret i1 false
}

define hidden i64 @nz.utf8_count(ptr %p, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %step ]
  %c = phi i64 [ 0, %entry ], [ %c1, %step ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %step, label %done
step:
  %bp = getelementptr i8, ptr %p, i64 %i
  %b8 = load i8, ptr %bp
  %b = zext i8 %b8 to i32
  %m = and i32 %b, 192
  %cont = icmp eq i32 %m, 128
  %add = select i1 %cont, i64 0, i64 1
  %c1 = add i64 %c, %add
  %i1 = add i64 %i, 1
  br label %loop
done:
  ret i64 %c
}

define hidden i64 @nz.utf8_decode(ptr %p, i64 %n, i64 %i) {
entry:
  %bp = getelementptr i8, ptr %p, i64 %i
  %b8 = load i8, ptr %bp
  %b = zext i8 %b8 to i64
  %one = icmp ult i64 %b, 128
  br i1 %one, label %r1, label %multi
r1:
  ret i64 %b
multi:
  %p1 = getelementptr i8, ptr %bp, i64 1
  %x1 = load i8, ptr %p1
  %y1 = zext i8 %x1 to i64
  %k1 = and i64 %y1, 63
  %two = icmp ult i64 %b, 224
  br i1 %two, label %r2, label %more
r2:
  %h2 = and i64 %b, 31
  %s2 = shl i64 %h2, 6
  %v2 = or i64 %s2, %k1
  ret i64 %v2
more:
  %p2 = getelementptr i8, ptr %bp, i64 2
  %x2 = load i8, ptr %p2
  %y2 = zext i8 %x2 to i64
  %k2 = and i64 %y2, 63
  %three = icmp ult i64 %b, 240
  br i1 %three, label %r3, label %r4
r3:
  %h3 = and i64 %b, 15
  %s3 = shl i64 %h3, 12
  %t3 = shl i64 %k1, 6
  %u3 = or i64 %s3, %t3
  %v3 = or i64 %u3, %k2
  ret i64 %v3
r4:
  %p3 = getelementptr i8, ptr %bp, i64 3
  %x3 = load i8, ptr %p3
  %y3 = zext i8 %x3 to i64
  %k3 = and i64 %y3, 63
  %h4 = and i64 %b, 7
  %s4 = shl i64 %h4, 18
  %t4 = shl i64 %k1, 12
  %w4 = shl i64 %k2, 6
  %u4 = or i64 %s4, %t4
  %z4 = or i64 %u4, %w4
  %v4 = or i64 %z4, %k3
  ret i64 %v4
}

define hidden i64 @nz.utf8_encode(ptr %buf, i64 %c) {
entry:
  %one = icmp ult i64 %c, 128
  br i1 %one, label %e1, label %m2
e1:
  %b1 = trunc i64 %c to i8
  store i8 %b1, ptr %buf
  ret i64 1
m2:
  %lo6 = and i64 %c, 63
  %cont0 = or i64 %lo6, 128
  %c0 = trunc i64 %cont0 to i8
  %sh6 = lshr i64 %c, 6
  %two = icmp ult i64 %c, 2048
  br i1 %two, label %e2, label %m3
e2:
  %l2 = or i64 %sh6, 192
  %a2 = trunc i64 %l2 to i8
  store i8 %a2, ptr %buf
  %q2 = getelementptr i8, ptr %buf, i64 1
  store i8 %c0, ptr %q2
  ret i64 2
m3:
  %mid = and i64 %sh6, 63
  %cont1 = or i64 %mid, 128
  %c1 = trunc i64 %cont1 to i8
  %sh12 = lshr i64 %c, 12
  %three = icmp ult i64 %c, 65536
  br i1 %three, label %e3, label %e4
e3:
  %l3 = or i64 %sh12, 224
  %a3 = trunc i64 %l3 to i8
  store i8 %a3, ptr %buf
  %q31 = getelementptr i8, ptr %buf, i64 1
  store i8 %c1, ptr %q31
  %q32 = getelementptr i8, ptr %buf, i64 2
  store i8 %c0, ptr %q32
  ret i64 3
e4:
  %hi = and i64 %sh12, 63
  %cont2 = or i64 %hi, 128
  %c2 = trunc i64 %cont2 to i8
  %sh18 = lshr i64 %c, 18
  %l4 = or i64 %sh18, 240
  %a4 = trunc i64 %l4 to i8
  store i8 %a4, ptr %buf
  %q41 = getelementptr i8, ptr %buf, i64 1
  store i8 %c2, ptr %q41
  %q42 = getelementptr i8, ptr %buf, i64 2
  store i8 %c1, ptr %q42
  %q43 = getelementptr i8, ptr %buf, i64 3
  store i8 %c0, ptr %q43
  ret i64 4
}

define hidden void @nz.mem_zero(ptr %p, i64 %n) {
entry:
  call void @llvm.memset.p0.i64(ptr %p, i8 0, i64 %n, i1 false)
  ret void
}

define hidden void @nz.mem_move(ptr %d, ptr %s, i64 %n) {
entry:
  call void @llvm.memmove.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)
  ret void
}
";

const FOREIGN_STRING_RUNTIME: &str = r"
define hidden i1 @nz.cstr_has_nul(ptr %p, i64 %n) {
entry:
  br label %scan
scan:
  %i = phi i64 [ 0, %entry ], [ %inext, %look ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %look_at, label %clean
look_at:
  %at = getelementptr i8, ptr %p, i64 %i
  %byte = load i8, ptr %at
  %nul = icmp eq i8 %byte, 0
  br i1 %nul, label %found, label %look
look:
  %inext = add i64 %i, 1
  br label %scan
found:
  ret i1 true
clean:
  ret i1 false
}

define hidden i64 @nz.cstr_len(ptr %p) {
entry:
  br label %scan
scan:
  %i = phi i64 [ 0, %entry ], [ %inext, %more ]
  %at = getelementptr i8, ptr %p, i64 %i
  %byte = load i8, ptr %at
  %nul = icmp eq i8 %byte, 0
  br i1 %nul, label %done, label %more
more:
  %inext = add i64 %i, 1
  br label %scan
done:
  ret i64 %i
}

define hidden ptr @nz.cstr_borrow(ptr %p, i64 %n) {
entry:
  %room = add i64 %n, 1
  %buf = call ptr @malloc(i64 %room)
  %oom = icmp eq ptr %buf, null
  br i1 %oom, label %nomem, label %fill
nomem:
  ret ptr null
fill:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %p, i64 %n, i1 false)
  %stop = getelementptr i8, ptr %buf, i64 %n
  store i8 0, ptr %stop
  ret ptr %buf
}
";

/// `errno` across a foreign call (N85, §7.86): generated code calls `nz.errno_save` the moment a
/// foreign call returns, and `c_errno()` reads what it saved. The location of `errno` is the
/// target's (`__error` on macOS, `__errno_location` on Linux), so it is asked of
/// `nz_errno_location`, which a per-target unit of its own defines (`crate::switch::emit_errno`)
/// and this text stays the same on every target.
const ERRNO_RUNTIME: &str = r"
@nz.errno_saved = hidden thread_local global i64 0

declare ptr @nz_errno_location()

define hidden void @nz.errno_save() {
entry:
  %p = call ptr @nz_errno_location()
  %e = load i32, ptr %p
  %w = sext i32 %e to i64
  store i64 %w, ptr @nz.errno_saved
  ret void
}

define hidden i64 @nz.errno_last() {
entry:
  %v = load i64, ptr @nz.errno_saved
  ret i64 %v
}
";

/// The scheduler's counters (N54): called by generated code after a task starts and before a
/// scope joins, so the runtime's task text — under the self-hosted parity gate — is untouched.
/// The task pool (N83, `docs/architecture.md` §7.84): an opt-in M:N scheduler beside one OS
/// thread per task. `{RESTORE_LIMIT}`, `{SAVE_LIMIT}` and `{SET_LIMIT}` become the stack limit's
/// save, restore and first value where the stack check is reached, and nothing otherwise.
const POOL_RUNTIME: &str = r#"
@nz.pool_mode = hidden global i64 0
@nz.pool_workers = hidden global ptr null
@nz.pool_count = hidden global i64 0
@nz.pool_next = hidden global i64 0
@nz.pool_stack = hidden global i64 0
@nz.pool_page = hidden global i64 0
@nz.pool_live = hidden global i64 0
@nz.pool_peak = hidden global i64 0
@nz.pool_wait = hidden global ptr null
@nz.pool_buckets = hidden global ptr null
@nz.pool_join = hidden global ptr null
@nz.cur_task = hidden thread_local global ptr null
@.pool.env.sched = private unnamed_addr constant [15 x i8] c"NAZM_SCHEDULER\00"
@.pool.env.workers = private unnamed_addr constant [13 x i8] c"NAZM_WORKERS\00"
@.pool.env.stack = private unnamed_addr constant [16 x i8] c"NAZM_TASK_STACK\00"
@.pool.word = private unnamed_addr constant [5 x i8] c"pool\00"
@.pool.threads = private unnamed_addr constant [8 x i8] c"threads\00"
; The program's policy (N106, §7.107): 2 the pool, 1 a thread per task. The platform entry of a
; program that calls C stores 1 before `main` runs; a library, which has no entry, keeps the pool.
@nz.pool_policy = hidden global i64 2
@.pool.fmt = private unnamed_addr constant [46 x i8] c"nazm-pool: workers=%lld peak=%lld stack=%lld\0A\00"

declare void @nz_ctx_switch(ptr, ptr)
declare ptr @nz_ctx_prepare(ptr, ptr)

; The scheduler this process runs (N83, §7.84; the default N106's, §7.107): 1 one OS thread per
; task, 2 the pool, 4 a pool that could not start or a NAZM_SCHEDULER that names neither — no
; task starts. Decided once, at the first question; 3 while deciding.
define hidden i64 @nz.pool_mode_get() {
entry:
  br label %top
top:
  %m = load atomic i64, ptr @nz.pool_mode acquire, align 8
  %settled = icmp eq i64 %m, 0
  br i1 %settled, label %decide, label %known
known:
  %deciding = icmp eq i64 %m, 3
  br i1 %deciding, label %spin, label %done
done:
  ret i64 %m
spin:
  br label %top
decide:
  %pair = cmpxchg ptr @nz.pool_mode, i64 0, i64 3 acq_rel acquire
  %won = extractvalue {i64, i1} %pair, 1
  br i1 %won, label %ask, label %top
ask:
  %v = call ptr @getenv(ptr @.pool.env.sched)
  %unset = icmp eq ptr %v, null
  br i1 %unset, label %policy, label %len
policy:
  ; Unset: the program's policy.
  %pol = load i64, ptr @nz.pool_policy
  %onthreads = icmp eq i64 %pol, 1
  br i1 %onthreads, label %threads, label %start
len:
  %n = call i64 @strlen(ptr %v)
  %four = icmp eq i64 %n, 4
  br i1 %four, label %cmp, label %seven
cmp:
  %c = call i32 @memcmp(ptr %v, ptr @.pool.word, i64 4)
  %is = icmp eq i32 %c, 0
  br i1 %is, label %start, label %refused
seven:
  %sev = icmp eq i64 %n, 7
  br i1 %sev, label %cmpt, label %refused
cmpt:
  %ct = call i32 @memcmp(ptr %v, ptr @.pool.threads, i64 7)
  %ist = icmp eq i32 %ct, 0
  br i1 %ist, label %threads, label %refused
refused:
  store atomic i64 4, ptr @nz.pool_mode release, align 8
  ret i64 4
threads:
  store atomic i64 1, ptr @nz.pool_mode release, align 8
  ret i64 1
start:
  %ok = call i1 @nz.pool_start()
  %mode = select i1 %ok, i64 2, i64 4
  store atomic i64 %mode, ptr @nz.pool_mode release, align 8
  ret i64 %mode
}

; A positive count from the environment variable at `%v`, or `%dflt`.
define hidden i64 @nz.pool_env(ptr %name, i64 %dflt) {
entry:
  %v = call ptr @getenv(ptr %name)
  %unset = icmp eq ptr %v, null
  br i1 %unset, label %default, label %parse
parse:
  %n = call i64 @atoll(ptr %v)
  %pos = icmp sgt i64 %n, 0
  br i1 %pos, label %have, label %default
have:
  ret i64 %n
default:
  ret i64 %dflt
}

; A mutex at 0 and a condition variable at 128, in 256 bytes, or null.
define hidden ptr @nz.pool_sync() {
entry:
  %s = call ptr @malloc(i64 256)
  %oom = icmp eq ptr %s, null
  br i1 %oom, label %none, label %init
none:
  ret ptr null
init:
  %mi = call i32 @pthread_mutex_init(ptr %s, ptr null)
  %cv = getelementptr i8, ptr %s, i64 128
  %ci = call i32 @pthread_cond_init(ptr %cv, ptr null)
  ret ptr %s
}

; Start the pool: its waiter table, its join lock, and its workers. False if any of it could not be
; made; the workers already started then wait for tasks that never come.
define hidden i1 @nz.pool_start() {
entry:
  %attr = alloca [128 x i8], align 16
  %pg32 = call i32 @getpagesize()
  %pg = sext i32 %pg32 to i64
  store i64 %pg, ptr @nz.pool_page
  %wanted = call i64 @nz.pool_env(ptr @.pool.env.workers, i64 4)
  %big = icmp sgt i64 %wanted, 64
  %count = select i1 %big, i64 64, i64 %wanted
  %asked = call i64 @nz.pool_env(ptr @.pool.env.stack, i64 262144)
  %min = mul i64 %pg, 4
  %small = icmp slt i64 %asked, %min
  %raw = select i1 %small, i64 %min, i64 %asked
  %pm1 = sub i64 %pg, 1
  %up = add i64 %raw, %pm1
  %pages = udiv i64 %up, %pg
  %stack = mul i64 %pages, %pg
  store i64 %stack, ptr @nz.pool_stack
  %wl = call ptr @nz.pool_sync()
  %bk = call ptr @malloc(i64 2048)
  %js = call ptr @nz.pool_sync()
  %wsz = mul i64 %count, 8
  %arr = call ptr @malloc(i64 %wsz)
  %n1 = icmp eq ptr %wl, null
  %n2 = icmp eq ptr %bk, null
  %n3 = icmp eq ptr %js, null
  %n4 = icmp eq ptr %arr, null
  %o1 = or i1 %n1, %n2
  %o2 = or i1 %n3, %n4
  %any = or i1 %o1, %o2
  br i1 %any, label %fail, label %tables
fail:
  ret i1 false
tables:
  call void @llvm.memset.p0.i64(ptr %bk, i8 0, i64 2048, i1 false)
  store ptr %wl, ptr @nz.pool_wait
  store ptr %bk, ptr @nz.pool_buckets
  store ptr %js, ptr @nz.pool_join
  br label %make
make:
  %i = phi i64 [ 0, %tables ], [ %inext, %made ]
  %more = icmp slt i64 %i, %count
  br i1 %more, label %one, label %publish
one:
  %w = call ptr @malloc(i64 320)
  %wnull = icmp eq ptr %w, null
  br i1 %wnull, label %fail, label %winit
winit:
  call void @llvm.memset.p0.i64(ptr %w, i8 0, i64 320, i1 false)
  %wmi = call i32 @pthread_mutex_init(ptr %w, ptr null)
  %wcv = getelementptr i8, ptr %w, i64 128
  %wci = call i32 @pthread_cond_init(ptr %wcv, ptr null)
  %ip = getelementptr i8, ptr %w, i64 288
  store i64 %i, ptr %ip
  %slot = getelementptr ptr, ptr %arr, i64 %i
  store ptr %w, ptr %slot
  br label %made
made:
  %inext = add i64 %i, 1
  br label %make
publish:
  store ptr %arr, ptr @nz.pool_workers
  store i64 %count, ptr @nz.pool_count
  br label %spawnw
spawnw:
  %j = phi i64 [ 0, %publish ], [ %jnext, %started ]
  %jmore = icmp slt i64 %j, %count
  br i1 %jmore, label %thread, label %ok
thread:
  %wp = getelementptr ptr, ptr %arr, i64 %j
  %ww = load ptr, ptr %wp
  %th = getelementptr i8, ptr %ww, i64 296
  %ai = call i32 @pthread_attr_init(ptr %attr)
  %as = call i32 @pthread_attr_setstacksize(ptr %attr, i64 1048576)
  %rc = call i32 @pthread_create(ptr %th, ptr %attr, ptr @nz.worker_main, ptr %ww)
  %ad = call i32 @pthread_attr_destroy(ptr %attr)
  %bad = icmp ne i32 %rc, 0
  br i1 %bad, label %fail, label %started
started:
  %jnext = add i64 %j, 1
  br label %spawnw
ok:
  ret i1 true
}

; Queue a runnable task at its worker's tail and wake the worker. The worker's lock is held.
define hidden void @nz.pool_push(ptr %w, ptr %t) {
entry:
  %np = getelementptr i8, ptr %t, i64 48
  store ptr null, ptr %np
  %hp = getelementptr i8, ptr %w, i64 256
  %tp = getelementptr i8, ptr %w, i64 264
  %tail = load ptr, ptr %tp
  %empty = icmp eq ptr %tail, null
  br i1 %empty, label %first, label %append
first:
  store ptr %t, ptr %hp
  br label %linked
append:
  %tn = getelementptr i8, ptr %tail, i64 48
  store ptr %t, ptr %tn
  br label %linked
linked:
  store ptr %t, ptr %tp
  %cv = getelementptr i8, ptr %w, i64 128
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  ret void
}

; The wall clock in nanoseconds, as `pthread_cond_timedwait` measures it.
define hidden i64 @nz.pool_now() {
entry:
  %ts = alloca [2 x i64], align 8
  %r = call i32 @clock_gettime(i32 0, ptr %ts)
  %s = load i64, ptr %ts
  %np = getelementptr i8, ptr %ts, i64 8
  %n = load i64, ptr %np
  %sn = mul i64 %s, 1000000000
  %t = add i64 %sn, %n
  ret i64 %t
}

; Every parked task on this worker whose deadline has passed is queued again; entries for tasks no
; longer parked with a deadline leave the list. The worker's lock is held. Writes the soonest
; remaining deadline into `%ts` and says whether there is one.
define hidden i1 @nz.pool_timers(ptr %w, ptr %ts) {
entry:
  %now = call i64 @nz.pool_now()
  %headp = getelementptr i8, ptr %w, i64 280
  %first = load ptr, ptr %headp
  store ptr null, ptr %headp
  br label %walk
walk:
  %t = phi ptr [ %first, %entry ], [ %next, %keep ], [ %next, %drop ]
  %soon = phi i64 [ 0, %entry ], [ %soon2, %keep ], [ %soon, %drop ]
  %atend = icmp eq ptr %t, null
  br i1 %atend, label %finish, label %look
look:
  %nxp = getelementptr i8, ptr %t, i64 136
  %next = load ptr, ptr %nxp
  %stp = getelementptr i8, ptr %t, i64 24
  %st = load i64, ptr %stp
  %dp = getelementptr i8, ptr %t, i64 112
  %d = load i64, ptr %dp
  %parked = icmp eq i64 %st, 3
  %timed = icmp ne i64 %d, 0
  %live = and i1 %parked, %timed
  br i1 %live, label %due, label %drop
due:
  %passed = icmp sle i64 %d, %now
  br i1 %passed, label %fire, label %keep
fire:
  store i64 0, ptr %dp
  store i64 0, ptr %stp
  call void @nz.pool_push(ptr %w, ptr %t)
  br label %drop
drop:
  %inp = getelementptr i8, ptr %t, i64 144
  store i64 0, ptr %inp
  store ptr null, ptr %nxp
  br label %walk
keep:
  %h = load ptr, ptr %headp
  store ptr %h, ptr %nxp
  store ptr %t, ptr %headp
  %none = icmp eq i64 %soon, 0
  %earlier = icmp slt i64 %d, %soon
  %take = or i1 %none, %earlier
  %soon2 = select i1 %take, i64 %d, i64 %soon
  br label %walk
finish:
  %have = icmp ne i64 %soon, 0
  br i1 %have, label %fill, label %out
fill:
  %sec = sdiv i64 %soon, 1000000000
  %nsec = srem i64 %soon, 1000000000
  store i64 %sec, ptr %ts
  %tn = getelementptr i8, ptr %ts, i64 8
  store i64 %nsec, ptr %tn
  br label %out
out:
  ret i1 %have
}

; Into a task: it becomes this thread's current task, with its own failure state and stack limit.
define hidden void @nz.task_enter(ptr %t) {
entry:
  store ptr %t, ptr @nz.cur_task
  %fp = getelementptr i8, ptr %t, i64 80
  %f = load i64, ptr %fp
  %fb = trunc i64 %f to i1
  store i1 %fb, ptr @nz.failed
  %mp = getelementptr i8, ptr %t, i64 88
  %m = load ptr, ptr %mp
  store ptr %m, ptr @nz.errmsg
  %lp = getelementptr i8, ptr %t, i64 96
  %l = load i64, ptr %lp
  store i64 %l, ptr @nz.errlen
{RESTORE_LIMIT}
  ret void
}

; Out of a task: its failure state and stack limit are saved into it, and the worker's are clean.
define hidden void @nz.task_leave(ptr %t) {
entry:
  %fb = load i1, ptr @nz.failed
  %f = zext i1 %fb to i64
  %fp = getelementptr i8, ptr %t, i64 80
  store i64 %f, ptr %fp
  %m = load ptr, ptr @nz.errmsg
  %mp = getelementptr i8, ptr %t, i64 88
  store ptr %m, ptr %mp
  %l = load i64, ptr @nz.errlen
  %lp = getelementptr i8, ptr %t, i64 96
  store i64 %l, ptr %lp
{SAVE_LIMIT}
  store i1 false, ptr @nz.failed
  store ptr null, ptr @nz.errmsg
  store i64 0, ptr @nz.errlen
  store ptr null, ptr @nz.cur_task
  ret void
}

; A worker: run its tasks first in, first out, each until it parks, yields or ends.
define hidden ptr @nz.worker_main(ptr %w) {
entry:
  %ts = alloca [2 x i64], align 8
  %l0 = call i32 @pthread_mutex_lock(ptr %w)
  br label %look
look:
  %timed = call i1 @nz.pool_timers(ptr %w, ptr %ts)
  %hp = getelementptr i8, ptr %w, i64 256
  %h = load ptr, ptr %hp
  %empty = icmp eq ptr %h, null
  br i1 %empty, label %idle, label %take
idle:
  %cv = getelementptr i8, ptr %w, i64 128
  br i1 %timed, label %tw, label %fw
tw:
  %a = call i32 @pthread_cond_timedwait(ptr %cv, ptr %w, ptr %ts)
  br label %look
fw:
  %b = call i32 @pthread_cond_wait(ptr %cv, ptr %w)
  br label %look
take:
  %np = getelementptr i8, ptr %h, i64 48
  %n = load ptr, ptr %np
  store ptr %n, ptr %hp
  %last = icmp eq ptr %n, null
  br i1 %last, label %cleartail, label %go
cleartail:
  %tp = getelementptr i8, ptr %w, i64 264
  store ptr null, ptr %tp
  br label %go
go:
  store ptr null, ptr %np
  %sp = getelementptr i8, ptr %h, i64 24
  store i64 1, ptr %sp
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  call void @nz.task_enter(ptr %h)
  %wsp = getelementptr i8, ptr %w, i64 272
  %tsp = load ptr, ptr %h
  call void @nz_ctx_switch(ptr %wsp, ptr %tsp)
  call void @nz.task_leave(ptr %h)
  %st = load i64, ptr %sp
  %ended = icmp eq i64 %st, 4
  br i1 %ended, label %finish, label %relock
relock:
  %l2 = call i32 @pthread_mutex_lock(ptr %w)
  %parking = icmp eq i64 %st, 2
  br i1 %parking, label %park, label %yielded
park:
  %wk = getelementptr i8, ptr %h, i64 32
  %woken = load i64, ptr %wk
  %was = icmp ne i64 %woken, 0
  br i1 %was, label %requeue, label %rest
requeue:
  store i64 0, ptr %wk
  store i64 0, ptr %sp
  call void @nz.pool_push(ptr %w, ptr %h)
  br label %look
rest:
  store i64 3, ptr %sp
  %dp = getelementptr i8, ptr %h, i64 112
  %d = load i64, ptr %dp
  %inp = getelementptr i8, ptr %h, i64 144
  %in = load i64, ptr %inp
  %hasd = icmp ne i64 %d, 0
  %notin = icmp eq i64 %in, 0
  %add = and i1 %hasd, %notin
  br i1 %add, label %addtimer, label %look
addtimer:
  store i64 1, ptr %inp
  %th = getelementptr i8, ptr %w, i64 280
  %old = load ptr, ptr %th
  %tnx = getelementptr i8, ptr %h, i64 136
  store ptr %old, ptr %tnx
  store ptr %h, ptr %th
  br label %look
yielded:
  store i64 0, ptr %sp
  call void @nz.pool_push(ptr %w, ptr %h)
  br label %look
finish:
  ; The task's stack goes, then it is marked done, last: a joiner may free its record at once.
  %base = getelementptr i8, ptr %h, i64 8
  %stk = load ptr, ptr %base
  %pg = load i64, ptr @nz.pool_page
  %rw = call i32 @mprotect(ptr %stk, i64 %pg, i32 3)
  call void @free(ptr %stk)
  %js = load ptr, ptr @nz.pool_join
  %jl = call i32 @pthread_mutex_lock(ptr %js)
  %donep = getelementptr i8, ptr %h, i64 72
  store i64 1, ptr %donep
  %jcv = getelementptr i8, ptr %js, i64 128
  call void @nz.cwake(ptr %jcv)
  %ju = call i32 @pthread_mutex_unlock(ptr %js)
  %l3 = call i32 @pthread_mutex_lock(ptr %w)
  br label %look
}

; Start a task on the pool: a record, a guarded stack, a first frame, a worker by round robin, and
; its place in the argument block's head, where a thread's handle goes otherwise. 0, or 1 when no
; stack could be had.
define hidden i32 @nz.task_start(ptr %fn, ptr %blk) {
entry:
  %slot = alloca ptr, align 8
  %t = call ptr @malloc(i64 192)
  %tnull = icmp eq ptr %t, null
  br i1 %tnull, label %nothread, label %zero
nothread:
  ret i32 1
zero:
  call void @llvm.memset.p0.i64(ptr %t, i8 0, i64 192, i1 false)
  %pg = load i64, ptr @nz.pool_page
  %size = load i64, ptr @nz.pool_stack
  %total = add i64 %size, %pg
  %rc = call i32 @posix_memalign(ptr %slot, i64 %pg, i64 %total)
  %bad = icmp ne i32 %rc, 0
  br i1 %bad, label %nostack, label %guard
nostack:
  call void @free(ptr %t)
  ret i32 1
guard:
  %base = load ptr, ptr %slot
  %g = call i32 @mprotect(ptr %base, i64 %pg, i32 0)
  %bp = getelementptr i8, ptr %t, i64 8
  store ptr %base, ptr %bp
  %tot = getelementptr i8, ptr %t, i64 16
  store i64 %total, ptr %tot
  %top = getelementptr i8, ptr %base, i64 %total
  %sp = call ptr @nz_ctx_prepare(ptr %top, ptr %t)
  store ptr %sp, ptr %t
  %fp = getelementptr i8, ptr %t, i64 56
  store ptr %fn, ptr %fp
  %blp = getelementptr i8, ptr %t, i64 64
  store ptr %blk, ptr %blp
{SET_LIMIT}
  %k = atomicrmw add ptr @nz.pool_next, i64 1 monotonic
  %count = load i64, ptr @nz.pool_count
  %idx = urem i64 %k, %count
  %arr = load ptr, ptr @nz.pool_workers
  %wp = getelementptr ptr, ptr %arr, i64 %idx
  %w = load ptr, ptr %wp
  %wfield = getelementptr i8, ptr %t, i64 40
  store ptr %w, ptr %wfield
  store ptr %t, ptr %blk
  %live = atomicrmw add ptr @nz.pool_live, i64 1 monotonic
  %now = add i64 %live, 1
  %pk = atomicrmw max ptr @nz.pool_peak, i64 %now monotonic
  %l = call i32 @pthread_mutex_lock(ptr %w)
  call void @nz.pool_push(ptr %w, ptr %t)
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  ret i32 0
}

; What the entry stub calls, on the task's own stack: the task's start routine, then the end.
define hidden void @nz_task_main(ptr %t) {
entry:
  %fp = getelementptr i8, ptr %t, i64 56
  %fn = load ptr, ptr %fp
  %bp = getelementptr i8, ptr %t, i64 64
  %blk = load ptr, ptr %bp
  %r = call ptr %fn(ptr %blk)
  %live = atomicrmw sub ptr @nz.pool_live, i64 1 monotonic
  %wp = getelementptr i8, ptr %t, i64 40
  %w = load ptr, ptr %wp
  %l = call i32 @pthread_mutex_lock(ptr %w)
  %sp = getelementptr i8, ptr %t, i64 24
  store i64 4, ptr %sp
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  %wsp = getelementptr i8, ptr %w, i64 272
  %back = load ptr, ptr %wsp
  call void @nz_ctx_switch(ptr %t, ptr %back)
  unreachable
}

; Back to the worker, the task's state already set under its worker's lock.
define hidden void @nz.task_suspend(ptr %t) {
entry:
  %wp = getelementptr i8, ptr %t, i64 40
  %w = load ptr, ptr %wp
  %wsp = getelementptr i8, ptr %w, i64 272
  %back = load ptr, ptr %wsp
  call void @nz_ctx_switch(ptr %t, ptr %back)
  ret void
}

; Make a parked task runnable, or note that it was woken while still running or parking.
define hidden void @nz.task_wake(ptr %t) {
entry:
  %wp = getelementptr i8, ptr %t, i64 40
  %w = load ptr, ptr %wp
  %l = call i32 @pthread_mutex_lock(ptr %w)
  %sp = getelementptr i8, ptr %t, i64 24
  %st = load i64, ptr %sp
  %parked = icmp eq i64 %st, 3
  br i1 %parked, label %run, label %busy
run:
  %dp = getelementptr i8, ptr %t, i64 112
  store i64 0, ptr %dp
  store i64 0, ptr %sp
  call void @nz.pool_push(ptr %w, ptr %t)
  br label %out
busy:
  %running = icmp eq i64 %st, 1
  %parking = icmp eq i64 %st, 2
  %either = or i1 %running, %parking
  br i1 %either, label %note, label %out
note:
  %wk = getelementptr i8, ptr %t, i64 32
  store i64 1, ptr %wk
  br label %out
out:
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  ret void
}

define hidden ptr @nz.pool_bucket(ptr %cv) {
entry:
  %a = ptrtoint ptr %cv to i64
  %h = lshr i64 %a, 4
  %i = and i64 %h, 255
  %bk = load ptr, ptr @nz.pool_buckets
  %slot = getelementptr ptr, ptr %bk, i64 %i
  ret ptr %slot
}

; Take `%t` off the waiters of whatever it waits on, if it is still on them.
define hidden void @nz.pool_unwait(ptr %t) {
entry:
  %wl = load ptr, ptr @nz.pool_wait
  %l = call i32 @pthread_mutex_lock(ptr %wl)
  %cvp = getelementptr i8, ptr %t, i64 128
  %cv = load ptr, ptr %cvp
  %none = icmp eq ptr %cv, null
  br i1 %none, label %out, label %find
find:
  %slot = call ptr @nz.pool_bucket(ptr %cv)
  br label %walk
walk:
  %link = phi ptr [ %slot, %find ], [ %nlink, %next ]
  %cur = load ptr, ptr %link
  %end = icmp eq ptr %cur, null
  br i1 %end, label %clear, label %test
test:
  %hit = icmp eq ptr %cur, %t
  %nlink = getelementptr i8, ptr %cur, i64 120
  br i1 %hit, label %unlink, label %next
next:
  br label %walk
unlink:
  %after = load ptr, ptr %nlink
  store ptr %after, ptr %link
  br label %clear
clear:
  store ptr null, ptr %cvp
  %np = getelementptr i8, ptr %t, i64 120
  store ptr null, ptr %np
  br label %out
out:
  %u = call i32 @pthread_mutex_unlock(ptr %wl)
  ret void
}

; Wait on a condition variable, its mutex held. On a pool task the task parks and its worker runs
; others; anywhere else the thread waits. Either way the mutex is held again on return, and the
; caller looks at its condition again, as with any condition variable.
define hidden i32 @nz.cwait(ptr %cv, ptr %m) {
entry:
  %t = load ptr, ptr @nz.cur_task
  %isthread = icmp eq ptr %t, null
  br i1 %isthread, label %thread, label %task
thread:
  %r = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  ret i32 %r
task:
  call void @nz.pool_park(ptr %t, ptr %cv, ptr %m, i64 0)
  ret i32 0
}

; As `nz.cwait`, giving up at the wall-clock time `%ts`.
define hidden i32 @nz.ctimedwait(ptr %cv, ptr %m, ptr %ts) {
entry:
  %t = load ptr, ptr @nz.cur_task
  %isthread = icmp eq ptr %t, null
  br i1 %isthread, label %thread, label %task
thread:
  %r = call i32 @pthread_cond_timedwait(ptr %cv, ptr %m, ptr %ts)
  ret i32 %r
task:
  %s = load i64, ptr %ts
  %np = getelementptr i8, ptr %ts, i64 8
  %n = load i64, ptr %np
  %sn = mul i64 %s, 1000000000
  %d0 = add i64 %sn, %n
  %zero = icmp eq i64 %d0, 0
  %d = select i1 %zero, i64 1, i64 %d0
  call void @nz.pool_park(ptr %t, ptr %cv, ptr %m, i64 %d)
  ret i32 0
}

; Register as a waiter on `%cv`, mark the task parking under its worker's lock, let go of `%m`,
; and switch to the worker; on resuming, take `%m` again and leave the waiters if still there.
; The order makes a wake-up between the caller's last look and the switch impossible to lose: a
; waker needs `%m` to change anything, and finds the task registered and parking.
define hidden void @nz.pool_park(ptr %t, ptr %cv, ptr %m, i64 %deadline) {
entry:
  %wl = load ptr, ptr @nz.pool_wait
  %l = call i32 @pthread_mutex_lock(ptr %wl)
  %slot = call ptr @nz.pool_bucket(ptr %cv)
  %head = load ptr, ptr %slot
  %np = getelementptr i8, ptr %t, i64 120
  store ptr %head, ptr %np
  %cvp = getelementptr i8, ptr %t, i64 128
  store ptr %cv, ptr %cvp
  store ptr %t, ptr %slot
  %u = call i32 @pthread_mutex_unlock(ptr %wl)
  %wp = getelementptr i8, ptr %t, i64 40
  %w = load ptr, ptr %wp
  %wlk = call i32 @pthread_mutex_lock(ptr %w)
  %dp = getelementptr i8, ptr %t, i64 112
  store i64 %deadline, ptr %dp
  %sp = getelementptr i8, ptr %t, i64 24
  store i64 2, ptr %sp
  %wu = call i32 @pthread_mutex_unlock(ptr %w)
  %mu = call i32 @pthread_mutex_unlock(ptr %m)
  call void @nz.task_suspend(ptr %t)
  call void @nz.pool_unwait(ptr %t)
  %ml = call i32 @pthread_mutex_lock(ptr %m)
  ret void
}

; Wake every waiter on a condition variable: threads, by the variable, and pool tasks registered
; on it.
define hidden void @nz.cwake(ptr %cv) {
entry:
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %mode = load atomic i64, ptr @nz.pool_mode acquire, align 8
  %pool = icmp eq i64 %mode, 2
  br i1 %pool, label %tasks, label %out
tasks:
  %wl = load ptr, ptr @nz.pool_wait
  %l = call i32 @pthread_mutex_lock(ptr %wl)
  %slot = call ptr @nz.pool_bucket(ptr %cv)
  br label %walk
walk:
  %link = phi ptr [ %slot, %tasks ], [ %nlink, %skip ], [ %link, %woke ]
  %cur = load ptr, ptr %link
  %end = icmp eq ptr %cur, null
  br i1 %end, label %done, label %test
test:
  %cvp = getelementptr i8, ptr %cur, i64 128
  %on = load ptr, ptr %cvp
  %mine = icmp eq ptr %on, %cv
  %nlink = getelementptr i8, ptr %cur, i64 120
  br i1 %mine, label %take, label %skip
skip:
  br label %walk
take:
  %after = load ptr, ptr %nlink
  store ptr %after, ptr %link
  store ptr null, ptr %nlink
  store ptr null, ptr %cvp
  call void @nz.task_wake(ptr %cur)
  br label %woke
woke:
  br label %walk
done:
  %u = call i32 @pthread_mutex_unlock(ptr %wl)
  br label %out
out:
  ret void
}

; A pool task that looked and did not wait lets the rest of its worker's tasks run first.
define hidden void @nz.task_yield() {
entry:
  %t = load ptr, ptr @nz.cur_task
  %none = icmp eq ptr %t, null
  br i1 %none, label %out, label %yield
yield:
  %wp = getelementptr i8, ptr %t, i64 40
  %w = load ptr, ptr %wp
  %l = call i32 @pthread_mutex_lock(ptr %w)
  %sp = getelementptr i8, ptr %t, i64 24
  store i64 5, ptr %sp
  %u = call i32 @pthread_mutex_unlock(ptr %w)
  call void @nz.task_suspend(ptr %t)
  br label %out
out:
  ret void
}

; With the schedule report asked for, the pool's line: its workers, the most tasks alive at once,
; and each task's stack — only where the pool ran.
define hidden void @nz.pool_report() {
entry:
  %buf = alloca [128 x i8], align 8
  %mode = load atomic i64, ptr @nz.pool_mode acquire, align 8
  %pool = icmp eq i64 %mode, 2
  br i1 %pool, label %say, label %out
say:
  %w = load i64, ptr @nz.pool_count
  %p = load i64, ptr @nz.pool_peak
  %s = load i64, ptr @nz.pool_stack
  %n = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %buf, i64 128, ptr @.pool.fmt, i64 %w, i64 %p, i64 %s)
  %len = sext i32 %n to i64
  %wr = call i64 @write(i32 2, ptr %buf, i64 %len)
  br label %out
out:
  ret void
}

; Wait for one task to end: its thread joined, or its pool record marked done and freed.
define hidden i32 @nz.join_one(ptr %tid) {
entry:
  %mode = load atomic i64, ptr @nz.pool_mode acquire, align 8
  %pool = icmp eq i64 %mode, 2
  br i1 %pool, label %wait, label %thread
thread:
  %rc = call i32 @pthread_join(ptr %tid, ptr null)
  ret i32 %rc
wait:
  %js = load ptr, ptr @nz.pool_join
  %jcv = getelementptr i8, ptr %js, i64 128
  %l = call i32 @pthread_mutex_lock(ptr %js)
  br label %check
check:
  %dp = getelementptr i8, ptr %tid, i64 72
  %d = load i64, ptr %dp
  %done = icmp ne i64 %d, 0
  br i1 %done, label %free, label %sleep
sleep:
  %w = call i32 @nz.cwait(ptr %jcv, ptr %js)
  br label %check
free:
  %u = call i32 @pthread_mutex_unlock(ptr %js)
  call void @free(ptr %tid)
  ret i32 0
}
"#;

/// The thread model's task, channel and select texts joined to the pool (N83, §7.84): every
/// condition wait, timed wait and broadcast through `nz.cwait`, `nz.ctimedwait` and `nz.cwake`; a
/// scope's join through `nz.join_one`; `nz.spawn` starting a pool task when the pool runs; and a
/// select that does not wait yielding its worker. Applied as the text is emitted, so the
/// constants stay the thread model's, line for line.
fn pool_hooked(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 1024);
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .split_once(" = call i32 @pthread_cond_broadcast(")
            .map(|(_, r)| r)
        {
            out.push_str("  call void @nz.cwake(");
            out.push_str(rest);
        } else if line.contains("call i32 @pthread_cond_wait(") {
            out.push_str(&line.replace("@pthread_cond_wait(", "@nz.cwait("));
        } else if line.contains("call i32 @pthread_cond_timedwait(") {
            out.push_str(&line.replace("@pthread_cond_timedwait(", "@nz.ctimedwait("));
        } else if line == "  %rc = call i32 @pthread_join(ptr %tid, ptr null)" {
            out.push_str("  %rc = call i32 @nz.join_one(ptr %tid)");
        } else if line == "  %ai = call i32 @pthread_attr_init(ptr %attr)" {
            out.push_str(POOL_SPAWN);
            out.push_str(line);
        } else if line == "  ret i64 -2" {
            out.push_str(POOL_YIELD);
            out.push_str(line);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// What `nz.spawn` does before creating a thread, when the pool's text is present: start a
/// pool task if the pool runs, refuse if it was asked for and could not start.
const POOL_SPAWN: &str =
    "  ; The pool (N83, §7.84), when the environment asked for it; a pool that could not start
  ; starts nothing, as a thread that could not be created does.
  %mode = call i64 @nz.pool_mode_get()
  %pooled = icmp eq i64 %mode, 2
  br i1 %pooled, label %pool, label %nopool
pool:
  %prc = call i32 @nz.task_start(ptr %fn, ptr %blk)
  %pbad = icmp ne i32 %prc, 0
  br i1 %pbad, label %undo, label %ok
nopool:
  %broken = icmp eq i64 %mode, 4
  br i1 %broken, label %undo, label %thread
thread:
";

/// What a select's timeout does first, when the pool's text is present: a pool task that did not
/// wait lets its worker's other tasks run (N83, §7.84).
const POOL_YIELD: &str = "  call void @nz.task_yield()
";

const SCHED_RUNTIME: &str = r"
define hidden void @nz.sched_note_spawn() {
entry:
  %n = atomicrmw add ptr @nz.sched_spawned, i64 1 monotonic
  ret void
}

; `%h` is the scope's task list; every task in it is about to be joined.
define hidden void @nz.sched_note_join(ptr %h) {
entry:
  %tasks = load i64, ptr %h
  %n = atomicrmw add ptr @nz.sched_joined, i64 %tasks monotonic
  ret void
}
";

const GENERIC_CHANNEL_RUNTIME: &str = r"
define hidden ptr @nz.chanv_new(i64 %cap, i64 %stride, ptr %release) {
entry:
  %c = call ptr @malloc(i64 80)
  %bad = icmp eq ptr %c, null
  br i1 %bad, label %fail, label %ring
fail:
  ret ptr null
freec:
  call void @free(ptr %c)
  ret ptr null
freed:
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
freem:
  call void @free(ptr %m)
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
ring:
  %r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %cap, i64 %stride)
  %over = extractvalue {i64, i1} %r, 1
  br i1 %over, label %freec, label %ask
ask:
  %bytes = extractvalue {i64, i1} %r, 0
  %room = add i64 %bytes, 8
  %d = call ptr @malloc(i64 %room)
  %badd = icmp eq ptr %d, null
  br i1 %badd, label %freec, label %lock
lock:
  %m = call ptr @malloc(i64 128)
  %badm = icmp eq ptr %m, null
  br i1 %badm, label %freed, label %cond
cond:
  %v = call ptr @malloc(i64 128)
  %badv = icmp eq ptr %v, null
  br i1 %badv, label %freem, label %init
init:
  %mi = call i32 @pthread_mutex_init(ptr %m, ptr null)
  %vi = call i32 @pthread_cond_init(ptr %v, ptr null)
  store i64 %cap, ptr %c
  %lenp = getelementptr i8, ptr %c, i64 8
  store i64 0, ptr %lenp
  %headp = getelementptr i8, ptr %c, i64 16
  store i64 0, ptr %headp
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 0, ptr %closedp
  %datap = getelementptr i8, ptr %c, i64 32
  store ptr %d, ptr %datap
  %mp = getelementptr i8, ptr %c, i64 40
  store ptr %m, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  store ptr %v, ptr %vp
  %refs = getelementptr i8, ptr %c, i64 56
  store i64 1, ptr %refs
  %sp = getelementptr i8, ptr %c, i64 64
  store i64 %stride, ptr %sp
  %relp = getelementptr i8, ptr %c, i64 72
  store ptr %release, ptr %relp
  %n = atomicrmw add ptr @nz.chan_allocated, i64 1 monotonic
  ret ptr %c
}

; The last reference releases what is still queued, slot by slot, by the element's own release
; function — then everything `nz.chan_release` frees for an `Int` channel.
define hidden void @nz.chanv_release(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %drop
drop:
  %refs = getelementptr i8, ptr %c, i64 56
  %old = atomicrmw sub ptr %refs, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %contents, label %done
contents:
  fence acquire
  %relp = getelementptr i8, ptr %c, i64 72
  %rel = load ptr, ptr %relp
  %norel = icmp eq ptr %rel, null
  br i1 %norel, label %destroy, label %walk
walk:
  %lenp = getelementptr i8, ptr %c, i64 8
  %len = load i64, ptr %lenp
  %headp = getelementptr i8, ptr %c, i64 16
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %sp = getelementptr i8, ptr %c, i64 64
  %stride = load i64, ptr %sp
  %datap0 = getelementptr i8, ptr %c, i64 32
  %data0 = load ptr, ptr %datap0
  br label %test
test:
  %i = phi i64 [ 0, %walk ], [ %next, %body ]
  %more = icmp slt i64 %i, %len
  br i1 %more, label %body, label %destroy
body:
  %sum = add i64 %head, %i
  %idx = urem i64 %sum, %cap
  %off = mul i64 %idx, %stride
  %slot = getelementptr i8, ptr %data0, i64 %off
  call void %rel(ptr %slot)
  %next = add i64 %i, 1
  br label %test
destroy:
  %vp = getelementptr i8, ptr %c, i64 48
  %v = load ptr, ptr %vp
  %dv = call i32 @pthread_cond_destroy(ptr %v)
  call void @free(ptr %v)
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %dm = call i32 @pthread_mutex_destroy(ptr %m)
  call void @free(ptr %m)
  %datap = getelementptr i8, ptr %c, i64 32
  %data = load ptr, ptr %datap
  call void @free(ptr %data)
  call void @free(ptr %c)
  %n = atomicrmw add ptr @nz.chan_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

; Copies one slot in from `%src` — the sender's own reference, which the channel now holds —
; or refuses, copying nothing, when closed.
define hidden i1 @nz.chanv_send(ptr %c, ptr %src) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %sp = getelementptr i8, ptr %c, i64 64
  %stride = load i64, ptr %sp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %refuse, label %space
space:
  %len = load i64, ptr %lenp
  %cap = load i64, ptr %c
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %wait, label %store
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
refuse:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
store:
  %head = load i64, ptr %headp
  %sum = add i64 %head, %len
  %idx = urem i64 %sum, %cap
  %data = load ptr, ptr %datap
  %off = mul i64 %idx, %stride
  %slot = getelementptr i8, ptr %data, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %slot, ptr %src, i64 %stride, i1 false)
  %grown = add i64 %len, 1
  store i64 %grown, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  call void @nz.sel_signal()
  ret i1 true
}

; Copies the oldest slot out to `%dst` — the reference moves to the receiver — or reports the
; channel closed and drained.
define hidden i1 @nz.chanv_recv(ptr %c, ptr %dst) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %sp = getelementptr i8, ptr %c, i64 64
  %stride = load i64, ptr %sp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  %len = load i64, ptr %lenp
  %nothing = icmp sle i64 %len, 0
  br i1 %nothing, label %maybe, label %take
maybe:
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %drained, label %wait
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
drained:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
take:
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %data = load ptr, ptr %datap
  %off = mul i64 %head, %stride
  %slot = getelementptr i8, ptr %data, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %slot, i64 %stride, i1 false)
  %h1 = add i64 %head, 1
  %nh = urem i64 %h1, %cap
  store i64 %nh, ptr %headp
  %grown = sub i64 %len, 1
  store i64 %grown, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 true
}
";

const CHANNEL_RUNTIME: &str = r"
define hidden ptr @nz.chan_new(i64 %cap) {
entry:
  %c = call ptr @malloc(i64 64)
  %bad = icmp eq ptr %c, null
  br i1 %bad, label %fail, label %ring
fail:
  ret ptr null
; A channel is four allocations, and a failure part-way through must not leave the earlier
; ones behind. One label per stage, each freeing exactly what has been taken by then —
; which is `docs/spec.md`'s failure law applied to the runtime's own storage, not only to
; the program's.
freec:
  call void @free(ptr %c)
  ret ptr null
freed:
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
freem:
  call void @free(ptr %m)
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
ring:
  ; Checked: a capacity large enough to overflow this product would allocate a ring
  ; smaller than the channel then believes it has.
  %r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %cap, i64 8)
  %over = extractvalue {i64, i1} %r, 1
  br i1 %over, label %freec, label %ask
ask:
  %bytes = extractvalue {i64, i1} %r, 0
  %d = call ptr @malloc(i64 %bytes)
  %badd = icmp eq ptr %d, null
  br i1 %badd, label %freec, label %lock
lock:
  ; Sized generously on purpose: pthread_mutex_t and pthread_cond_t are opaque and
  ; differently sized on every platform, and there is no header to ask from here.
  ; Over-allocating is what makes this portable without a size to get wrong.
  %m = call ptr @malloc(i64 128)
  %badm = icmp eq ptr %m, null
  br i1 %badm, label %freed, label %cond
cond:
  %v = call ptr @malloc(i64 128)
  %badv = icmp eq ptr %v, null
  br i1 %badv, label %freem, label %init
init:
  %mi = call i32 @pthread_mutex_init(ptr %m, ptr null)
  %vi = call i32 @pthread_cond_init(ptr %v, ptr null)
  store i64 %cap, ptr %c
  %lenp = getelementptr i8, ptr %c, i64 8
  store i64 0, ptr %lenp
  %headp = getelementptr i8, ptr %c, i64 16
  store i64 0, ptr %headp
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 0, ptr %closedp
  %datap = getelementptr i8, ptr %c, i64 32
  store ptr %d, ptr %datap
  %mp = getelementptr i8, ptr %c, i64 40
  store ptr %m, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  store ptr %v, ptr %vp
  ; Born with one reference, held by the expression that created it.
  %rcp = getelementptr i8, ptr %c, i64 56
  store i64 1, ptr %rcp
  %n = atomicrmw add ptr @nz.chan_allocated, i64 1 monotonic
  ret ptr %c
}

define hidden void @nz.chan_retain(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %bump
bump:
  %rcp = getelementptr i8, ptr %c, i64 56
  %old = atomicrmw add ptr %rcp, i64 1 monotonic
  br label %done
done:
  ret void
}

; The whole of a channel goes at once: the condition variable, the mutex, the ring and the
; header. Destroying the two primitives is not optional — `pthread_mutex_destroy` releases
; what the platform allocated behind an opaque handle, and freeing the memory without it
; leaks whatever that was. Neither can be in use: nothing holds a reference, so no thread
; is inside a send, a receive or a close.
define hidden void @nz.chan_release(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %drop
drop:
  %rcp = getelementptr i8, ptr %c, i64 56
  %old = atomicrmw sub ptr %rcp, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %destroy, label %done
destroy:
  fence acquire
  %vp = getelementptr i8, ptr %c, i64 48
  %v = load ptr, ptr %vp
  %dv = call i32 @pthread_cond_destroy(ptr %v)
  call void @free(ptr %v)
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %dm = call i32 @pthread_mutex_destroy(ptr %m)
  call void @free(ptr %m)
  %datap = getelementptr i8, ptr %c, i64 32
  %data = load ptr, ptr %datap
  call void @free(ptr %data)
  call void @free(ptr %c)
  %n = atomicrmw add ptr @nz.chan_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden i1 @nz.chan_send(ptr %c, i64 %v) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; Closed is tested first and on every pass, so a channel closed while this sender
  ; waited refuses rather than queueing into something nobody will drain.
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %refuse, label %space
space:
  %len = load i64, ptr %lenp
  %cap = load i64, ptr %c
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %wait, label %store
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
refuse:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
store:
  %head = load i64, ptr %headp
  %sum = add i64 %head, %len
  %idx = urem i64 %sum, %cap
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %idx
  store i64 %v, ptr %slot
  %n1 = add i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 true
}

define hidden i1 @nz.chan_recv(ptr %c, ptr %seq, ptr %msg, i64 %mn) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; A queued value wins over the close flag, which is what makes closing end a channel
  ; rather than discard what is in it.
  %len = load i64, ptr %lenp
  %empty = icmp sle i64 %len, 0
  br i1 %empty, label %maybe, label %take
maybe:
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %drained, label %wait
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
drained:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
take:
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %head
  %val = load i64, ptr %slot
  %h1 = add i64 %head, 1
  %nh = urem i64 %h1, %cap
  store i64 %nh, ptr %headp
  %n1 = sub i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ; The push happens after the unlock. Growing a sequence may call `realloc`, and holding
  ; a channel's mutex across an allocation would make every other task on it wait for the
  ; allocator.
  %grown = call ptr @nz.arr_grow(ptr %seq, i64 8)
  %oom = icmp eq ptr %grown, null
  br i1 %oom, label %nomem, label %push
nomem:
  ; Records and returns, like every other failure. `false` is the stand-in `chan_recv`
  ; hands back while unwinding; the caller checks the flag before it reads it.
  call void @nz.fail(ptr %msg, i64 %mn)
  ret i1 false
push:
  %sn = load i64, ptr %seq
  %sslot = getelementptr i64, ptr %grown, i64 %sn
  store i64 %val, ptr %sslot
  %sn1 = add i64 %sn, 1
  store i64 %sn1, ptr %seq
  ret i1 true
}

define hidden void @nz.chan_close(ptr %c) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 1, ptr %closedp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u = call i32 @pthread_mutex_unlock(ptr %m)
  ret void
}
";

/// Gate 2's numbers: a float's text and a decimal float's value, the same in every build as
/// `nazm_sema::numeric` is in the interpreter. A float is written by ECMAScript's
/// `Number::toString` layout over the shortest digits that read back to the same value in its
/// own precision: the shortest is found as the first `%.*e` precision whose text `strtod` (or
/// `strtof`, for a `Float32`) reads back exactly — the correctly rounded digits at that length,
/// which is what the interpreter's shortest digits are too.
const NUMBER_RUNTIME: &str = r#"
@.fmt.e = private unnamed_addr constant [5 x i8] c"%.*e\00"
@.fmt.lld = private unnamed_addr constant [5 x i8] c"%lld\00"
@.fmt.llu = private unnamed_addr constant [5 x i8] c"%llu\00"

define hidden i64 @nz.format_u64(ptr %buf, i64 %v) {
entry:
  %w = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %buf, i64 21, ptr @.fmt.llu, i64 %v)
  %n = sext i32 %w to i64
  ret i64 %n
}

define hidden i64 @nz.format_float(ptr %buf, double %x, i1 %single) {
entry:
  %tmp = alloca [48 x i8]
  %dig = alloca [32 x i8]
  %isnan = fcmp uno double %x, %x
  br i1 %isnan, label %nan, label %number
nan:
  store i8 110, ptr %buf
  %n1 = getelementptr i8, ptr %buf, i64 1
  store i8 97, ptr %n1
  %n2 = getelementptr i8, ptr %buf, i64 2
  store i8 110, ptr %n2
  ret i64 3
number:
  %bits = bitcast double %x to i64
  %neg = icmp slt i64 %bits, 0
  %signlen = zext i1 %neg to i64
  br i1 %neg, label %minus, label %magnitude
minus:
  store i8 45, ptr %buf
  br label %magnitude
magnitude:
  %out = getelementptr i8, ptr %buf, i64 %signlen
  %ax = call double @llvm.fabs.f64(double %x)
  %isinf = fcmp oeq double %ax, 0x7FF0000000000000
  br i1 %isinf, label %inf, label %finite
inf:
  store i8 105, ptr %out
  %i1 = getelementptr i8, ptr %out, i64 1
  store i8 110, ptr %i1
  %i2 = getelementptr i8, ptr %out, i64 2
  store i8 102, ptr %i2
  %li = add i64 %signlen, 3
  ret i64 %li
finite:
  %iszero = fcmp oeq double %ax, 0.0
  br i1 %iszero, label %zero, label %try
zero:
  store i8 48, ptr %out
  %lz = add i64 %signlen, 1
  ret i64 %lz
try:
  %p = phi i32 [ 0, %finite ], [ %p1, %try.next ]
  %w = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %tmp, i64 48, ptr @.fmt.e, i32 %p, double %ax)
  br i1 %single, label %try.f, label %try.d
try.f:
  %vf = call float @strtof(ptr %tmp, ptr null)
  %xf = fptrunc double %ax to float
  %okf = fcmp oeq float %vf, %xf
  br label %try.check
try.d:
  %vd = call double @strtod(ptr %tmp, ptr null)
  %okd = fcmp oeq double %vd, %ax
  br label %try.check
try.check:
  %ok = phi i1 [ %okf, %try.f ], [ %okd, %try.d ]
  %last = icmp sge i32 %p, 17
  %stop = or i1 %ok, %last
  %p1 = add i32 %p, 1
  br i1 %stop, label %scan, label %try.next
try.next:
  br label %try
scan:
  %i = phi i64 [ 0, %try.check ], [ %i.next, %scan.keep ]
  %k = phi i64 [ 0, %try.check ], [ %k.next, %scan.keep ]
  %cp = getelementptr i8, ptr %tmp, i64 %i
  %c = load i8, ptr %cp
  %ise = icmp eq i8 %c, 101
  br i1 %ise, label %exponent, label %scan.keep
scan.keep:
  %isdot = icmp eq i8 %c, 46
  %dp = getelementptr i8, ptr %dig, i64 %k
  store i8 %c, ptr %dp
  %step = select i1 %isdot, i64 0, i64 1
  %k.next = add i64 %k, %step
  %i.next = add i64 %i, 1
  br label %scan
exponent:
  %ep = getelementptr i8, ptr %tmp, i64 %i
  %ep1 = getelementptr i8, ptr %ep, i64 1
  %e = call i64 @atoll(ptr %ep1)
  %n = add i64 %e, 1
  %len = call i64 @nz.fmt_layout(ptr %out, ptr %dig, i64 %k, i64 %n)
  %total = add i64 %len, %signlen
  ret i64 %total
}

define hidden i64 @nz.fmt_layout(ptr %out, ptr %dig, i64 %k, i64 %n) {
entry:
  %le21 = icmp sle i64 %n, 21
  %kle = icmp sle i64 %k, %n
  %whole = and i1 %kle, %le21
  br i1 %whole, label %int, label %t2
int:
  call void @llvm.memcpy.p0.p0.i64(ptr %out, ptr %dig, i64 %k, i1 false)
  %zeros = sub i64 %n, %k
  %zp = getelementptr i8, ptr %out, i64 %k
  call void @llvm.memset.p0.i64(ptr %zp, i8 48, i64 %zeros, i1 false)
  ret i64 %n
t2:
  %pos = icmp sgt i64 %n, 0
  %mid = and i1 %pos, %le21
  br i1 %mid, label %frac, label %t3
frac:
  call void @llvm.memcpy.p0.p0.i64(ptr %out, ptr %dig, i64 %n, i1 false)
  %dot = getelementptr i8, ptr %out, i64 %n
  store i8 46, ptr %dot
  %rest = getelementptr i8, ptr %dig, i64 %n
  %rl = sub i64 %k, %n
  %after = getelementptr i8, ptr %dot, i64 1
  call void @llvm.memcpy.p0.p0.i64(ptr %after, ptr %rest, i64 %rl, i1 false)
  %lf = add i64 %k, 1
  ret i64 %lf
t3:
  %gtm6 = icmp sgt i64 %n, -6
  %le0 = icmp sle i64 %n, 0
  %small = and i1 %gtm6, %le0
  br i1 %small, label %tiny, label %sci
tiny:
  store i8 48, ptr %out
  %o1 = getelementptr i8, ptr %out, i64 1
  store i8 46, ptr %o1
  %nz = sub i64 0, %n
  %o2 = getelementptr i8, ptr %out, i64 2
  call void @llvm.memset.p0.i64(ptr %o2, i8 48, i64 %nz, i1 false)
  %o3 = getelementptr i8, ptr %o2, i64 %nz
  call void @llvm.memcpy.p0.p0.i64(ptr %o3, ptr %dig, i64 %k, i1 false)
  %lt0 = add i64 %nz, 2
  %lt = add i64 %lt0, %k
  ret i64 %lt
sci:
  %d0 = load i8, ptr %dig
  store i8 %d0, ptr %out
  %one = icmp eq i64 %k, 1
  br i1 %one, label %sci.e, label %sci.frac
sci.frac:
  %s1 = getelementptr i8, ptr %out, i64 1
  store i8 46, ptr %s1
  %s2 = getelementptr i8, ptr %out, i64 2
  %d1 = getelementptr i8, ptr %dig, i64 1
  %km1 = sub i64 %k, 1
  call void @llvm.memcpy.p0.p0.i64(ptr %s2, ptr %d1, i64 %km1, i1 false)
  %kp1 = add i64 %k, 1
  br label %sci.e
sci.e:
  %at = phi i64 [ 1, %sci ], [ %kp1, %sci.frac ]
  %ech = getelementptr i8, ptr %out, i64 %at
  store i8 101, ptr %ech
  %e1 = sub i64 %n, 1
  %eneg = icmp slt i64 %e1, 0
  %sign = select i1 %eneg, i8 45, i8 43
  %sch = getelementptr i8, ptr %ech, i64 1
  store i8 %sign, ptr %sch
  %eabs.n = sub i64 0, %e1
  %eabs = select i1 %eneg, i64 %eabs.n, i64 %e1
  %edig = getelementptr i8, ptr %sch, i64 1
  %ew = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %edig, i64 8, ptr @.fmt.lld, i64 %eabs)
  %ewl = sext i32 %ew to i64
  %ls0 = add i64 %at, 2
  %ls = add i64 %ls0, %ewl
  ret i64 %ls
}

define hidden i64 @nz.digits_from(ptr %p, i64 %i, i64 %n) {
entry:
  br label %loop
loop:
  %j = phi i64 [ %i, %entry ], [ %j1, %more ]
  %inside = icmp slt i64 %j, %n
  br i1 %inside, label %look, label %done
look:
  %cp = getelementptr i8, ptr %p, i64 %j
  %c = load i8, ptr %cp
  %ge0 = icmp uge i8 %c, 48
  %le9 = icmp ule i8 %c, 57
  %d = and i1 %ge0, %le9
  br i1 %d, label %more, label %done
more:
  %j1 = add i64 %j, 1
  br label %loop
done:
  ret i64 %j
}

define hidden i1 @nz.float_parses(ptr %p, i64 %n) {
entry:
  %has = icmp sgt i64 %n, 0
  br i1 %has, label %first, label %no
first:
  %c0 = load i8, ptr %p
  %plus = icmp eq i8 %c0, 43
  %minus = icmp eq i8 %c0, 45
  %signed = or i1 %plus, %minus
  %start = zext i1 %signed to i64
  %left = sub i64 %n, %start
  %three = icmp eq i64 %left, 3
  br i1 %three, label %word, label %digits
word:
  %wp = getelementptr i8, ptr %p, i64 %start
  %w0 = load i8, ptr %wp
  %wp1 = getelementptr i8, ptr %wp, i64 1
  %w1 = load i8, ptr %wp1
  %wp2 = getelementptr i8, ptr %wp, i64 2
  %w2 = load i8, ptr %wp2
  %i0 = icmp eq i8 %w0, 105
  %i1 = icmp eq i8 %w1, 110
  %i2 = icmp eq i8 %w2, 102
  %ia = and i1 %i0, %i1
  %inf = and i1 %ia, %i2
  %a0 = icmp eq i8 %w0, 110
  %a1 = icmp eq i8 %w1, 97
  %a2 = icmp eq i8 %w2, 110
  %aa = and i1 %a0, %a1
  %nan = and i1 %aa, %a2
  %named = or i1 %inf, %nan
  br i1 %named, label %yes, label %digits
digits:
  %w.end = call i64 @nz.digits_from(ptr %p, i64 %start, i64 %n)
  %whole = sub i64 %w.end, %start
  %in1 = icmp slt i64 %w.end, %n
  br i1 %in1, label %dot.look, label %after.frac
dot.look:
  %dcp = getelementptr i8, ptr %p, i64 %w.end
  %dc = load i8, ptr %dcp
  %isdot = icmp eq i8 %dc, 46
  br i1 %isdot, label %fraction, label %after.frac
fraction:
  %f.start = add i64 %w.end, 1
  %f.end = call i64 @nz.digits_from(ptr %p, i64 %f.start, i64 %n)
  %frac.n = sub i64 %f.end, %f.start
  br label %after.frac
after.frac:
  %at = phi i64 [ %w.end, %digits ], [ %w.end, %dot.look ], [ %f.end, %fraction ]
  %fr = phi i64 [ 0, %digits ], [ 0, %dot.look ], [ %frac.n, %fraction ]
  %mant = add i64 %whole, %fr
  %some = icmp sgt i64 %mant, 0
  br i1 %some, label %exp.look, label %no
exp.look:
  %in2 = icmp slt i64 %at, %n
  br i1 %in2, label %exp.char, label %end
exp.char:
  %ecp = getelementptr i8, ptr %p, i64 %at
  %ec = load i8, ptr %ecp
  %e.lo = icmp eq i8 %ec, 101
  %e.up = icmp eq i8 %ec, 69
  %ise = or i1 %e.lo, %e.up
  br i1 %ise, label %exp.sign, label %end
exp.sign:
  %e1 = add i64 %at, 1
  %in3 = icmp slt i64 %e1, %n
  br i1 %in3, label %exp.sign.look, label %no
exp.sign.look:
  %scp = getelementptr i8, ptr %p, i64 %e1
  %sc = load i8, ptr %scp
  %sp = icmp eq i8 %sc, 43
  %sm = icmp eq i8 %sc, 45
  %ss = or i1 %sp, %sm
  %skip = zext i1 %ss to i64
  %e.start = add i64 %e1, %skip
  %e.end = call i64 @nz.digits_from(ptr %p, i64 %e.start, i64 %n)
  %e.n = sub i64 %e.end, %e.start
  %e.some = icmp sgt i64 %e.n, 0
  br i1 %e.some, label %end.exp, label %no
end.exp:
  %all.e = icmp eq i64 %e.end, %n
  ret i1 %all.e
end:
  %all = icmp eq i64 %at, %n
  ret i1 %all
yes:
  ret i1 true
no:
  ret i1 false
}

define hidden double @nz.float_parse(ptr %p, i64 %n) {
entry:
  %ok = call i1 @nz.float_parses(ptr %p, i64 %n)
  br i1 %ok, label %copy, label %nan
copy:
  %room = add i64 %n, 1
  %c = call ptr @malloc(i64 %room)
  %none = icmp eq ptr %c, null
  br i1 %none, label %nan, label %read
read:
  call void @llvm.memcpy.p0.p0.i64(ptr %c, ptr %p, i64 %n, i1 false)
  %end = getelementptr i8, ptr %c, i64 %n
  store i8 0, ptr %end
  %v = call double @strtod(ptr %c, ptr null)
  call void @free(ptr %c)
  ret double %v
nan:
  ret double 0x7FF8000000000000
}
"#;
