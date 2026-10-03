//! The platform entry (N43): the process's `main`, which stores the arguments, mints the root
//! capabilities, calls the program's `main`, and ends the process — the only code that prints a
//! result or a failure. `docs/architecture.md` §7.45, *Startup and shutdown*.

use std::fmt::Write as _;

use super::core::runtime_declarations;
use super::os::declarations;
use crate::{
    escape, preamble, Needs, MEMORY_FORMAT, MEMORY_VARIABLE, SCHED_FORMAT, SCHED_VARIABLE,
};

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
    let _ = writeln!(out, "define i32 @main(i32 %argc, ptr %argv) {{");
    let _ = writeln!(out, "entry:");
    if needs.io {
        // One fewer than `argc`, because `argv[0]` is the program's own name and is
        // deliberately not visible.
        let _ = writeln!(out, "  %argc64 = sext i32 %argc to i64");
        let _ = writeln!(out, "  %nargs = sub i64 %argc64, 1");
        let _ = writeln!(out, "  store i64 %nargs, ptr @nz.argc");
        let _ = writeln!(out, "  store ptr %argv, ptr @nz.argv");
    }
    let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
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
