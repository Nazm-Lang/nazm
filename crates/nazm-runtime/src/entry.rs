//! The platform entry (N43): the process's `main`, which stores the arguments, mints the root
//! capabilities, calls the program's `main`, and ends the process — the only code that prints a
//! result or a failure. `docs/architecture.md` §7.45, *Startup and shutdown*.

use std::fmt::Write as _;

use super::core::runtime_declarations;
use super::os::declarations;
use crate::{
    escape, preamble, Needs, MEMORY_FORMAT, MEMORY_VARIABLE, SCHED_FORMAT, SCHED_VARIABLE,
};

/// The scheduler a program runs when the environment does not say (N106, `docs/architecture.md`
/// §7.107): 2 the pool, 1 a thread per task for a program that calls C.
#[must_use]
pub const fn policy(needs: Needs) -> i64 {
    if needs.foreign {
        1
    } else {
        2
    }
}

/// Emit the platform entry point: one per executable, and the only thing that prints.
///
/// # Why this is its own unit
///
/// It is the one place where the language's world meets the operating system's. Nazm's
/// `main` is an ordinary definition with an ordinary symbol derived from its `DefKey`;
/// `@main` is a C entry point with a C signature. Keeping them in separate artefacts is
/// what makes that separation visible rather than implied — and it is what stops an
/// imported module's function named `main` from ever being a process entry, because the
/// symbol this calls is the **root** module's, decided by [`crate::lower::plan`].
///
/// `roots` is how many capability parameters `main` takes (N37). Each is handed a `0`:
/// authority was checked when the program was, and the word is all that is left of it.
#[must_use]
pub fn emit_entry(entry: &str, roots: usize, needs: Needs) -> String {
    emit_entry_for(entry, roots, needs, None, None)
}

/// [`emit_entry`] for a target whose arguments are not `main`'s as they stand (Gate 3): `args`,
/// when given, is the platform function (`platform::args_hook`) that `main` hands `argc` and
/// `argv` to, with where `@nz.argc` and `@nz.argv` are, and that stores them itself.
#[must_use]
///
/// `stack`, when given (R1b, L14: `nazm build --stack-size`), is the bytes of stack the program
/// runs on: stored for the runtime's threads, and `main` itself run on a thread that size.
pub fn emit_entry_for(
    entry: &str,
    roots: usize,
    needs: Needs,
    args: Option<&str>,
    stack: Option<u64>,
) -> String {
    let needs = needs.settled();
    let mut out = String::new();
    preamble(&mut out, "<nazm entry>");
    declarations(&mut out);
    runtime_declarations(&mut out);
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    out.push('\n');
    // `%lld\n`: the full signed 64-bit value in decimal. Encoding the result in the exit
    // status instead would truncate it to 8 bits, which for a language whose `Int` is
    // 64-bit would be a lie in the common case.
    let _ = writeln!(
        out,
        "@.fmt = private unnamed_addr constant [6 x i8] c\"%lld\\0A\\00\""
    );
    // Unconditional since N8. Three classes of storage are reclaimable now and a program
    // with none of them is still a program whose accounting can be asked for — and an
    // instrument that is absent exactly where there is nothing to report is an instrument
    // that cannot distinguish "nothing" from "not measured".
    //
    // Lengths are computed, never counted by hand: a constant whose declared size
    // disagrees with its bytes is rejected by clang at best and truncates at worst.
    let name = MEMORY_VARIABLE;
    let _ = writeln!(
        out,
        "@.memvar = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
        name.len() + 1,
        escape(name)
    );
    let _ = writeln!(
        out,
        "@.memfmt = private unnamed_addr constant [{} x i8] c\"{}\\0A\\00\"",
        MEMORY_FORMAT.len() + 2,
        escape(MEMORY_FORMAT)
    );
    let _ = writeln!(out, "@.membuf = private global [512 x i8] zeroinitializer");
    // The scheduler's counters (N54), on their own variable: asking for one report is not
    // asking for the other.
    let _ = writeln!(
        out,
        "@.schedvar = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
        SCHED_VARIABLE.len() + 1,
        escape(SCHED_VARIABLE)
    );
    let _ = writeln!(
        out,
        "@.schedfmt = private unnamed_addr constant [{} x i8] c\"{}\\0A\\00\"",
        SCHED_FORMAT.len() + 2,
        escape(SCHED_FORMAT)
    );
    out.push('\n');

    if needs.tasks || needs.channels {
        let _ = writeln!(out, "declare void @nz.pool_report()");
    }
    if let (true, Some(hook)) = (needs.io, args) {
        let _ = writeln!(out, "declare void {hook}(i32, ptr, ptr, ptr)");
    }
    // `main` on a thread of the stack asked for (R1b, L14): what it returns is read back after
    // the join — and so is how it failed, because the failure state is thread-local and the
    // report below reads this thread's. Without the hand-back a `main` that failed would end
    // the process with success and its stand-in value.
    if stack.is_some() {
        let _ = writeln!(out, "@nz.main_result = internal global i64 0");
        let _ = writeln!(out, "@nz.main_failed = internal global i1 false");
        let _ = writeln!(out, "@nz.main_errmsg = internal global ptr null");
        let _ = writeln!(out, "@nz.main_errlen = internal global i64 0");
        let _ = writeln!(out, "define internal ptr @nz.main_thread(ptr %unused) {{");
        let _ = writeln!(out, "entry:");
        let _ = writeln!(out, "  %r = call i64 {entry}({root_args})");
        let _ = writeln!(out, "  store i64 %r, ptr @nz.main_result");
        let _ = writeln!(out, "  %tf = load i1, ptr @nz.failed");
        let _ = writeln!(out, "  store i1 %tf, ptr @nz.main_failed");
        let _ = writeln!(out, "  %tm = load ptr, ptr @nz.errmsg");
        let _ = writeln!(out, "  store ptr %tm, ptr @nz.main_errmsg");
        let _ = writeln!(out, "  %tl = load i64, ptr @nz.errlen");
        let _ = writeln!(out, "  store i64 %tl, ptr @nz.main_errlen");
        let _ = writeln!(out, "  ret ptr null");
        let _ = writeln!(out, "}}");
        out.push('\n');
    }
    let _ = writeln!(out, "define i32 @main(i32 %argc, ptr %argv) {{");
    let _ = writeln!(out, "entry:");
    // The scheduler's default for this program (N106, §7.107): the runtime's is the pool; a
    // program that calls C runs a thread per task, where a blocked call would hold a worker.
    // Stored before anything can start a task.
    if (needs.tasks || needs.channels) && policy(needs) == 1 {
        let _ = writeln!(out, "  store i64 1, ptr @nz.pool_policy");
    }
    if let (true, Some(hook)) = (needs.io, args) {
        let _ = writeln!(
            out,
            "  call void {hook}(i32 %argc, ptr %argv, ptr @nz.argc, ptr @nz.argv)"
        );
    } else if needs.io {
        // One fewer than `argc`, because `argv[0]` is the program's own name and is
        // deliberately not visible.
        let _ = writeln!(out, "  %argc64 = sext i32 %argc to i64");
        let _ = writeln!(out, "  %nargs = sub i64 %argc64, 1");
        let _ = writeln!(out, "  store i64 %nargs, ptr @nz.argc");
        let _ = writeln!(out, "  store ptr %argv, ptr @nz.argv");
    }
    match stack {
        None => {
            let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
        }
        // `main` on a thread with the stack asked for (L14), the result read back once it is
        // joined. A thread that cannot start runs `main` here, on the stack the process has.
        Some(bytes) => {
            let _ = writeln!(out, "  store i64 {bytes}, ptr @nz.stack_bytes");
            let _ = writeln!(out, "  %mattr = alloca [128 x i8], align 16");
            let _ = writeln!(out, "  %mtid = alloca ptr, align 8");
            let _ = writeln!(out, "  %mai = call i32 @pthread_attr_init(ptr %mattr)");
            let _ = writeln!(
                out,
                "  %mas = call i32 @pthread_attr_setstacksize(ptr %mattr, i64 {bytes})"
            );
            let _ = writeln!(
                out,
                "  %mrc = call i32 @pthread_create(ptr %mtid, ptr %mattr, ptr @nz.main_thread, ptr null)"
            );
            let _ = writeln!(out, "  %mad = call i32 @pthread_attr_destroy(ptr %mattr)");
            let _ = writeln!(out, "  %mbad = icmp ne i32 %mrc, 0");
            let _ = writeln!(out, "  br i1 %mbad, label %nz.inline, label %nz.join");
            let _ = writeln!(out, "nz.inline:");
            let _ = writeln!(out, "  %minline = call i64 {entry}({root_args})");
            let _ = writeln!(out, "  store i64 %minline, ptr @nz.main_result");
            let _ = writeln!(out, "  br label %nz.ran");
            let _ = writeln!(out, "nz.join:");
            let _ = writeln!(out, "  %mt = load ptr, ptr %mtid");
            let _ = writeln!(out, "  %mjn = call i32 @pthread_join(ptr %mt, ptr null)");
            let _ = writeln!(out, "  %jf = load i1, ptr @nz.main_failed");
            let _ = writeln!(out, "  store i1 %jf, ptr @nz.failed");
            let _ = writeln!(out, "  %jm = load ptr, ptr @nz.main_errmsg");
            let _ = writeln!(out, "  store ptr %jm, ptr @nz.errmsg");
            let _ = writeln!(out, "  %jl = load i64, ptr @nz.main_errlen");
            let _ = writeln!(out, "  store i64 %jl, ptr @nz.errlen");
            let _ = writeln!(out, "  br label %nz.ran");
            let _ = writeln!(out, "nz.ran:");
            let _ = writeln!(out, "  %result = load i64, ptr @nz.main_result");
        }
    }
    {
        // Before the failure is reported, not after: `@nz.report` ends the process and
        // never returns, so a report emitted on the success path alone would say nothing
        // about exactly the runs where reclamation is hardest to get right. `main` has
        // already returned by here, so the counts are final either way.
        //
        // It is a runtime diagnostic, off unless asked for. It is not a built-in because
        // no Nazm program should be able to observe how its own storage is managed:
        // `docs/spec.md` states *when* storage is reclaimed and deliberately not *how*.
        let _ = writeln!(out, "  %want = call ptr @getenv(ptr @.memvar)");
        let _ = writeln!(out, "  %quiet = icmp eq ptr %want, null");
        let _ = writeln!(
            out,
            "  br i1 %quiet, label %nz.checked, label %nz.memreport"
        );
        let _ = writeln!(out, "nz.memreport:");
        let mut fields = Vec::new();
        for (kind, allocated, reclaimed) in [
            ("seq", "@nz.seq_allocated", "@nz.seq_reclaimed"),
            ("str", "@nz.str_allocated", "@nz.str_reclaimed"),
            ("chan", "@nz.chan_allocated", "@nz.chan_reclaimed"),
            ("clo", "@nz.clo_allocated", "@nz.clo_reclaimed"),
        ] {
            let _ = writeln!(out, "  %{kind}a = load i64, ptr {allocated}");
            let _ = writeln!(out, "  %{kind}r = load i64, ptr {reclaimed}");
            let _ = writeln!(out, "  %{kind}l = sub i64 %{kind}a, %{kind}r");
            fields.push(format!("i64 %{kind}a, i64 %{kind}r, i64 %{kind}l"));
        }
        let _ = writeln!(
            out,
            "  %memout = call i32 (ptr, i64, ptr, ...) @snprintf(ptr @.membuf, i64 512, \
             ptr @.memfmt, {})",
            fields.join(", ")
        );
        let _ = writeln!(out, "  %memlen = sext i32 %memout to i64");
        let _ = writeln!(
            out,
            "  %memwrote = call i64 @write(i32 2, ptr @.membuf, i64 %memlen)"
        );
        let _ = writeln!(out, "  br label %nz.checked");
        let _ = writeln!(out, "nz.checked:");
        let _ = writeln!(out, "  %swant = call ptr @getenv(ptr @.schedvar)");
        let _ = writeln!(out, "  %squiet = icmp eq ptr %swant, null");
        let _ = writeln!(
            out,
            "  br i1 %squiet, label %nz.scheduled, label %nz.schedreport"
        );
        let _ = writeln!(out, "nz.schedreport:");
        let mut counts = Vec::new();
        for c in ["spawned", "joined", "selects", "timeouts"] {
            let _ = writeln!(out, "  %sch{c} = load i64, ptr @nz.sched_{c}");
            counts.push(format!("i64 %sch{c}"));
        }
        let _ = writeln!(
            out,
            "  %schout = call i32 (ptr, i64, ptr, ...) @snprintf(ptr @.membuf, i64 512, \
             ptr @.schedfmt, {})",
            counts.join(", ")
        );
        let _ = writeln!(out, "  %schlen = sext i32 %schout to i64");
        let _ = writeln!(
            out,
            "  %schwrote = call i64 @write(i32 2, ptr @.membuf, i64 %schlen)"
        );
        // The pool's own line (N83), only where it ran.
        if needs.tasks || needs.channels {
            let _ = writeln!(out, "  call void @nz.pool_report()");
        }
        let _ = writeln!(out, "  br label %nz.scheduled");
        let _ = writeln!(out, "nz.scheduled:");
    }
    // The single place a diagnostic is written. By the time control is here every scope
    // `main` opened has been joined, so nothing is still running when the process ends —
    // which is the difference between reporting a failure and abandoning the program
    // where it happened.
    let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
    let _ = writeln!(
        out,
        "  br i1 %failed, label %nz.reported, label %nz.finished"
    );
    let _ = writeln!(out, "nz.reported:");
    let _ = writeln!(out, "  call void @nz.report()");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "nz.finished:");
    let _ = writeln!(
        out,
        "  %print = call i32 (ptr, ...) @printf(ptr @.fmt, i64 %result)"
    );
    let _ = writeln!(out, "  ret i32 0");
    let _ = writeln!(out, "}}");
    out
}
