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

/// What that report says. One line, three numbers, read back by the tests.
pub const MEMORY_FORMAT: &str = "nazm-memory: sequences allocated=%lld reclaimed=%lld \
                                 live=%lld strings allocated=%lld reclaimed=%lld live=%lld \
                                 channels allocated=%lld reclaimed=%lld live=%lld";
use crate::emit::{preamble, Needs};

/// The runtime's own surface, as a unit that does not define it sees it.
///
/// The globals are `external`: one definition lives in the runtime unit and every other
/// object binds to it. Before N5 they were `internal` and there was only ever one module,
/// so the question did not arise.
pub(crate) fn runtime_declarations(out: &mut String) {
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

    if needs.sequences {
        out.push_str(SEQUENCE_RUNTIME);
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
    if needs.tasks {
        out.push_str(TASK_RUNTIME);
    }
    if needs.channels {
        out.push_str(CHANNEL_RUNTIME);
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
