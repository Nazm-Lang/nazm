//! The counted heap (R1d, D5; `docs/spec.md`, *A bounded heap*): what `nazm build --heap-limit`
//! links, and nothing else does.
//!
//! A program built with a limit calls `nz.heap_malloc`, `nz.heap_realloc` and `nz.heap_free`
//! wherever it would have called the C library's `malloc`, `realloc` and `free` — its own units,
//! the runtime and the platform's units alike ([`count_heap`]; the Cranelift backend renames the
//! symbols in LIR instead). Each block carries a 16-byte header holding the bytes asked for, so a
//! free knows what it gives back, and every block stays 16-aligned. The live total is one atomic
//! word: an allocation reserves its bytes first, and one that would take the total past the limit
//! writes a fixed message and ends the process with status 2 — `N0406`, the allocator's refusal,
//! as every run-time failure ends with that status. One the C library itself refuses still
//! answers null, which the caller reports as `N0406` at its own site, as without a limit.
//!
//! A task's stack is the platform's stack unit's (`posix_memalign`, freed there) and is not heap,
//! so that unit is the one the renaming leaves alone. What C allocates for itself is C's.
//!
//! The renaming is by name, so it holds only if every buffer the runtime frees is one it
//! allocated: a buffer the C library mallocs and hands back to be freed would reach
//! `nz.heap_free`, which reads a header that is not there and gives the C library a pointer it
//! never returned. So the runtime never asks C to allocate on its behalf — no entry of
//! [`C_ALLOCATING`], and no entry of [`C_ALLOCATING_ON_NULL`] given a null buffer — and what C
//! allocates and frees itself (`getaddrinfo` and `freeaddrinfo`, `opendir` and `closedir`) stays
//! C's on both sides.

use std::fmt::Write as _;

/// The C library's allocation entries a program built with a limit reaches only through the
/// counted ones: `(C's, the counted one)`.
pub const COUNTED: [(&str, &str); 3] = [
    ("@malloc(", "@nz.heap_malloc("),
    ("@realloc(", "@nz.heap_realloc("),
    ("@free(", "@nz.heap_free("),
];

/// C library functions that return a buffer of C's `malloc` for the caller to `free`, which the
/// runtime therefore never calls (a program's own foreign calls are its own).
pub const C_ALLOCATING: &[&str] = &[
    "strdup",
    "strndup",
    "wcsdup",
    "_strdup",
    "_wcsdup",
    "asprintf",
    "vasprintf",
    "getline",
    "getdelim",
    "open_memstream",
    "backtrace_symbols",
    "scandir",
    "canonicalize_file_name",
    "get_current_dir_name",
    "tempnam",
];

/// C library functions that allocate the result for the caller when their buffer argument is
/// null, which the runtime therefore always calls with a buffer of its own.
pub const C_ALLOCATING_ON_NULL: &[&str] = &["realpath", "getcwd", "_fullpath", "_wfullpath"];

/// `text` with every call and declaration of C's `malloc`, `realloc` and `free` made one of the
/// counted heap's. A line holding a constant string is left as it is, so a program's own text that
/// happens to spell one is never changed.
#[must_use]
pub fn count_heap(text: &str) -> String {
    renamed(text, &COUNTED)
}

/// `text` with each of `table`'s C entries made its replacement, but never on a line holding a
/// constant string.
fn renamed(text: &str, table: &[(&str, &str); 3]) -> String {
    let mut out = String::with_capacity(text.len() + 256);
    for line in text.split_inclusive('\n') {
        if line.contains(" c\"") || !table.iter().any(|(c, _)| line.contains(c)) {
            out.push_str(line);
            continue;
        }
        let mut l = line.to_owned();
        for (c, to) in table {
            l = l.replace(c, to);
        }
        out.push_str(&l);
    }
    out
}

/// C's allocation entries and the small-block heap's (R1.5 3.1, design D13), which a hosted
/// executable that calls no C reaches in their place: `(C's, the small-block heap's)`.
pub const SMALL: [(&str, &str); 3] = [
    ("@malloc(", "@nz.small_malloc("),
    ("@realloc(", "@nz.small_realloc("),
    ("@free(", "@nz.small_free("),
];

/// Blocks of each size class a thread of a program of several keeps for reuse; one freed past
/// them goes back to C.
pub const SMALL_KEPT: u64 = 64;

/// The bound in a program of one thread: none (`u64::MAX`, compared unsigned, is never reached).
/// The bound exists for a block one thread allocates and another frees; with one thread every
/// freed block is one this thread will ask for again, and a program that frees a tree of a
/// million boxes and builds another (binary-trees) otherwise gave all but 64 of them back to C
/// only to ask C for them again.
pub const SMALL_KEPT_ONE_THREAD: u64 = u64::MAX;

/// `text` with every call and declaration of C's `malloc`, `realloc` and `free` made the
/// small-block heap's, as [`count_heap`] makes them the counted heap's.
#[must_use]
pub fn small_heap(text: &str) -> String {
    renamed(text, &SMALL)
}

/// The small-block heap's unit (R1.5 3.1, design D13; `docs/architecture.md` §7.119). Every block
/// carries a 16-byte header holding its size class: 1 to 32 for a block of up to 512 bytes, whose
/// capacity is sixteen times its class, and 0 for a larger one, which is C's `malloc`'s. A thread
/// keeps up to [`SMALL_KEPT`] freed blocks of each class on a list of its own and hands them out
/// again without calling C; a block freed past that, and every large one, goes back to C. A block
/// freed on another thread than the one that allocated it joins the freeing thread's list, which
/// the bound keeps from growing without limit. In a program of one thread (`single`) the lists
/// are plain globals, unbounded ([`SMALL_KEPT_ONE_THREAD`]); elsewhere they are thread-local.
#[must_use]
pub fn emit_small(single: bool) -> String {
    // `noinline` keeps a list's update out of generated code, where a preempted task could be
    // stopped half-way through it (6.3); a program of one thread is never preempted.
    format!(
        "; Generated by nazm build. Do not edit.\nsource_filename = \"small\"\n\n{}\n",
        SMALL_RUNTIME
            .replace("{TLS}", if single { "" } else { "thread_local " })
            .replace(" noinline {", if single { " {" } else { " noinline {" })
            .replace(
                "{KEPT}",
                &if single {
                    // As LLVM spells the unsigned bound: the same 64 bits, signed.
                    (SMALL_KEPT_ONE_THREAD as i64).to_string()
                } else {
                    SMALL_KEPT.to_string()
                }
            )
    )
}

/// The message a program writes when an allocation would take its live heap past `limit`.
#[must_use]
pub fn message(limit: u64) -> String {
    format!("nazm: error[N0406]: out of memory: the heap limit of {limit} bytes is reached\n")
}

/// The counted heap's unit for a program of at most `limit` live heap bytes.
#[must_use]
pub fn emit_heap(limit: u64) -> String {
    let text = message(limit);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "; Generated by nazm build. Do not edit.\nsource_filename = \"heap\"\n"
    );
    let _ = writeln!(out, "@nz.heap_limit = internal global i64 {limit}");
    let _ = writeln!(out, "@nz.heap_live = internal global i64 0");
    let _ = writeln!(out, "@nz.heap_over = internal global i32 0");
    let _ = writeln!(
        out,
        "@.nz.heap_message = private unnamed_addr constant [{} x i8] c\"{}\"",
        text.len(),
        crate::escape(&text)
    );
    let _ = writeln!(
        out,
        "{}",
        HEAP_RUNTIME.replace("{LEN}", &text.len().to_string())
    );
    out
}

/// The counted entries; `{LEN}` is the message's length.
const HEAP_RUNTIME: &str = r"
declare ptr @malloc(i64)
declare ptr @realloc(ptr, i64)
declare void @free(ptr)
declare i64 @write(i32, ptr, i64)
declare void @exit(i32) noreturn

; Reserve `n` more live bytes, or end the process: the first thread past the limit writes the
; message and exits with status 2; any other waits for that exit.
define hidden void @nz.heap_reserve(i64 %n) {
entry:
  %limit = load i64, ptr @nz.heap_limit
  %huge = icmp ugt i64 %n, %limit
  br i1 %huge, label %over, label %add
add:
  %old = atomicrmw add ptr @nz.heap_live, i64 %n seq_cst
  %new = add i64 %old, %n
  %past = icmp ugt i64 %new, %limit
  br i1 %past, label %over, label %ok
ok:
  ret void
over:
  %first = atomicrmw xchg ptr @nz.heap_over, i32 1 seq_cst
  %alone = icmp eq i32 %first, 0
  br i1 %alone, label %report, label %wait
report:
  %w = call i64 @write(i32 2, ptr @.nz.heap_message, i64 {LEN})
  call void @exit(i32 2)
  unreachable
wait:
  br label %wait
}

; `n` bytes the caller owns behind a 16-byte header holding `n`, counted; null when the C library
; refuses, with nothing counted.
define hidden ptr @nz.heap_malloc(i64 %n) {
entry:
  call void @nz.heap_reserve(i64 %n)
  %total = add i64 %n, 16
  %h = call ptr @malloc(i64 %total)
  %none = icmp eq ptr %h, null
  br i1 %none, label %refused, label %got
refused:
  %undo = atomicrmw sub ptr @nz.heap_live, i64 %n seq_cst
  ret ptr null
got:
  store i64 %n, ptr %h
  %p = getelementptr i8, ptr %h, i64 16
  ret ptr %p
}

; Gives a counted block back; null is a no-op.
define hidden void @nz.heap_free(ptr %p) {
entry:
  %none = icmp eq ptr %p, null
  br i1 %none, label %done, label %give
give:
  %h = getelementptr i8, ptr %p, i64 -16
  %n = load i64, ptr %h
  %undo = atomicrmw sub ptr @nz.heap_live, i64 %n seq_cst
  call void @free(ptr %h)
  br label %done
done:
  ret void
}

; A counted block of `n` bytes holding what `p` held, as C's `realloc`: growth is reserved first,
; and a refusal leaves `p` as it was, counted as it was.
define hidden ptr @nz.heap_realloc(ptr %p, i64 %n) {
entry:
  %fresh = icmp eq ptr %p, null
  br i1 %fresh, label %new, label %resize
new:
  %m = call ptr @nz.heap_malloc(i64 %n)
  ret ptr %m
resize:
  %h = getelementptr i8, ptr %p, i64 -16
  %old = load i64, ptr %h
  %grows = icmp ugt i64 %n, %old
  %more = sub i64 %n, %old
  br i1 %grows, label %reserve, label %move
reserve:
  call void @nz.heap_reserve(i64 %more)
  br label %move
move:
  %total = add i64 %n, 16
  %q = call ptr @realloc(ptr %h, i64 %total)
  %none = icmp eq ptr %q, null
  br i1 %none, label %refused, label %got
refused:
  %back = select i1 %grows, i64 %more, i64 0
  %undo = atomicrmw sub ptr @nz.heap_live, i64 %back seq_cst
  ret ptr null
got:
  store i64 %n, ptr %q
  %less = sub i64 %old, %n
  %freed = select i1 %grows, i64 0, i64 %less
  %give = atomicrmw sub ptr @nz.heap_live, i64 %freed seq_cst
  %r = getelementptr i8, ptr %q, i64 16
  ret ptr %r
}
";

/// The small-block heap's entries; `{TLS}` is `thread_local ` or nothing, `{KEPT}` the bound.
/// `@nz.small_lists` holds each class's list head at its class, and the list's length 33 slots on.
const SMALL_RUNTIME: &str = r"
@nz.small_lists = internal {TLS}global [66 x ptr] zeroinitializer

declare ptr @malloc(i64)
declare ptr @realloc(ptr, i64)
declare void @free(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

; `n` bytes the caller owns behind a 16-byte header holding the block's class, or null when C
; refuses: a block of up to 512 bytes from this thread's list of its class when the list has one,
; else from C.
define hidden ptr @nz.small_malloc(i64 %n) noinline {
entry:
  %big = icmp ugt i64 %n, 512
  br i1 %big, label %large, label %small
small:
  %n15 = add i64 %n, 15
  %c0 = lshr i64 %n15, 4
  %zero = icmp eq i64 %c0, 0
  %c = select i1 %zero, i64 1, i64 %c0
  %slot = getelementptr [66 x ptr], ptr @nz.small_lists, i64 0, i64 %c
  %head = load ptr, ptr %slot
  %empty = icmp eq ptr %head, null
  br i1 %empty, label %fresh, label %pop
pop:
  %next = load ptr, ptr %head
  store ptr %next, ptr %slot
  %ci = add i64 %c, 33
  %cnt = getelementptr [66 x i64], ptr @nz.small_lists, i64 0, i64 %ci
  %k = load i64, ptr %cnt
  %k1 = sub i64 %k, 1
  store i64 %k1, ptr %cnt
  ret ptr %head
fresh:
  %bytes = shl i64 %c, 4
  %total = add i64 %bytes, 16
  %h = call ptr @malloc(i64 %total)
  %none = icmp eq ptr %h, null
  br i1 %none, label %refused, label %mark
mark:
  store i64 %c, ptr %h
  %p = getelementptr i8, ptr %h, i64 16
  ret ptr %p
large:
  %huge = icmp ugt i64 %n, -17
  br i1 %huge, label %refused, label %alloc
alloc:
  %ltotal = add i64 %n, 16
  %lh = call ptr @malloc(i64 %ltotal)
  %lnone = icmp eq ptr %lh, null
  br i1 %lnone, label %refused, label %lmark
lmark:
  store i64 0, ptr %lh
  %lp = getelementptr i8, ptr %lh, i64 16
  ret ptr %lp
refused:
  ret ptr null
}

; Gives a block back: a small one to this thread's list of its class while that holds fewer than
; {KEPT}, any other to C; null is a no-op.
define hidden void @nz.small_free(ptr %p) noinline {
entry:
  %none = icmp eq ptr %p, null
  br i1 %none, label %done, label %give
give:
  %h = getelementptr i8, ptr %p, i64 -16
  %c = load i64, ptr %h
  %large = icmp eq i64 %c, 0
  br i1 %large, label %back, label %keep
keep:
  %ci = add i64 %c, 33
  %cnt = getelementptr [66 x i64], ptr @nz.small_lists, i64 0, i64 %ci
  %k = load i64, ptr %cnt
  %full = icmp uge i64 %k, {KEPT}
  br i1 %full, label %back, label %push
push:
  %slot = getelementptr [66 x ptr], ptr @nz.small_lists, i64 0, i64 %c
  %head = load ptr, ptr %slot
  store ptr %head, ptr %p
  store ptr %p, ptr %slot
  %k1 = add i64 %k, 1
  store i64 %k1, ptr %cnt
  br label %done
back:
  call void @free(ptr %h)
  br label %done
done:
  ret void
}

; A block of `n` bytes holding what `p` held, as C's `realloc`: a small block that already holds
; `n` is kept, a smaller one moved, a large one resized by C; a refusal leaves `p` as it was.
define hidden ptr @nz.small_realloc(ptr %p, i64 %n) noinline {
entry:
  %fresh = icmp eq ptr %p, null
  br i1 %fresh, label %new, label %resize
new:
  %m = call ptr @nz.small_malloc(i64 %n)
  ret ptr %m
resize:
  %h = getelementptr i8, ptr %p, i64 -16
  %c = load i64, ptr %h
  %large = icmp eq i64 %c, 0
  br i1 %large, label %grow, label %small
grow:
  %huge = icmp ugt i64 %n, -17
  br i1 %huge, label %refused, label %re
re:
  %total = add i64 %n, 16
  %q = call ptr @realloc(ptr %h, i64 %total)
  %qnone = icmp eq ptr %q, null
  br i1 %qnone, label %refused, label %qok
qok:
  %qp = getelementptr i8, ptr %q, i64 16
  ret ptr %qp
small:
  %cap = shl i64 %c, 4
  %fits = icmp ule i64 %n, %cap
  br i1 %fits, label %same, label %move
same:
  ret ptr %p
move:
  %r = call ptr @nz.small_malloc(i64 %n)
  %rnone = icmp eq ptr %r, null
  br i1 %rnone, label %refused, label %copy
copy:
  call void @llvm.memcpy.p0.p0.i64(ptr %r, ptr %p, i64 %cap, i1 false)
  call void @nz.small_free(ptr %p)
  ret ptr %r
refused:
  ret ptr null
}
";

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allocator_is_renamed_and_never_inside_a_string() {
        let text = "declare ptr @malloc(i64)\n  %p = call ptr @malloc(i64 8)\n  call void @free(ptr %p)\n  call void @freeaddrinfo(ptr %a)\n  %q = call ptr @realloc(ptr %p, i64 9)\n@.s = private constant [9 x i8] c\"@malloc(\\00\"\n";
        let counted = count_heap(text);
        assert_eq!(
            counted,
            "declare ptr @nz.heap_malloc(i64)\n  %p = call ptr @nz.heap_malloc(i64 8)\n  call void @nz.heap_free(ptr %p)\n  call void @freeaddrinfo(ptr %a)\n  %q = call ptr @nz.heap_realloc(ptr %p, i64 9)\n@.s = private constant [9 x i8] c\"@malloc(\\00\"\n"
        );
    }

    /// Every text the runtime emits for every target, as [`crate::digest`] covers it.
    fn every_runtime_text() -> Vec<String> {
        let mut texts = vec![
            crate::emit_runtime(crate::EVERYTHING),
            crate::emit_entry("@nz_main", 1, crate::EVERYTHING),
        ];
        for t in crate::switch::SwitchTarget::ALL {
            texts.push(crate::switch::emit_switch(t));
            texts.push(crate::switch::emit_errno(t));
            texts.push(crate::platform::emit_os(t));
            texts.push(crate::platform::emit_stack(t));
            texts.push(crate::platform::emit_init(t).unwrap_or_default());
        }
        texts
    }

    #[test]
    fn the_runtime_never_frees_what_the_c_library_allocated() {
        // Under `--heap-limit` every `free` is the counted heap's, so a buffer C allocated and
        // the runtime freed would be given back through a header it does not have: a program
        // built with a limit that called `file_canonical` ended with SIGTRAP, status 133.
        for text in every_runtime_text() {
            for line in text.lines() {
                for f in C_ALLOCATING {
                    assert!(
                        !line.contains(&format!("@{f}(")),
                        "the runtime asks C to allocate: {line}"
                    );
                }
                for f in C_ALLOCATING_ON_NULL {
                    let call = line.split_once(&format!("@{f}(")).map(|(_, args)| args);
                    assert!(
                        !call.is_some_and(|args| args.contains("ptr null")),
                        "the runtime asks C to allocate: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_unit_holds_its_limit_and_its_message() {
        let unit = emit_heap(1 << 20);
        assert!(
            unit.contains("@nz.heap_limit = internal global i64 1048576"),
            "{unit}"
        );
        let text = message(1 << 20);
        assert!(
            unit.contains(&format!("ptr @.nz.heap_message, i64 {})", text.len())),
            "{unit}"
        );
        assert!(!unit.contains("{LEN}"), "{unit}");
    }
}
