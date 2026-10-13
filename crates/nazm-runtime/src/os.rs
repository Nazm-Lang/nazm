//! The operating-system adapter (N43): every entry point of the C library and POSIX threads the
//! generated code and the runtime call, and nothing else of the platform.
//!
//! `docs/architecture.md` §7.45. The runtime's semantics are portable LLVM text
//! ([`super::core`]); what it needs of the operating system is this list, which is the same on
//! every v1 target (macOS and Linux, 64-bit ARM and x86: all POSIX with a C library whose
//! `int64_t`-sized calls these declarations spell). A target without them is refused by
//! [`crate::backend::Target`] before anything is generated, never built against a guess.
//! `a_runtime_calls_the_operating_system_only_through_the_adapter` holds the runtime text to it.

use std::fmt::Write as _;

/// Everything a Nazm unit may refer to and does not define.
///
/// Emitted into **every** unit, unconditionally. A `declare` costs nothing — LLVM emits no
/// symbol reference for one that is never called — and the alternative is deciding, per
/// unit, which declarations it happens to need, which is a class of bug ("the IR was
/// rejected because something was not declared") for no benefit.
pub fn declarations(out: &mut String) {
    for d in [
        "declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.uadd.with.overflow.i64(i64, i64)",
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
        "declare i32 @printf(ptr noundef, ...)",
        "declare i32 @snprintf(ptr noundef, i64, ptr noundef, ...)",
        "declare ptr @malloc(i64)",
        "declare ptr @realloc(ptr, i64)",
        "declare void @free(ptr)",
        "declare ptr @getenv(ptr)",
        "declare i32 @memcmp(ptr, ptr, i64)",
        "declare i64 @write(i32, ptr, i64)",
        "declare void @exit(i32) noreturn",
        "declare i64 @strlen(ptr)",
        "declare ptr @fopen(ptr, ptr)",
        "declare i32 @fseek(ptr, i64, i32)",
        "declare i64 @ftell(ptr)",
        "declare i64 @fread(ptr, i64, i64, ptr)",
        "declare i64 @fwrite(ptr, i64, i64, ptr)",
        "declare i32 @fclose(ptr)",
        "declare i32 @pthread_create(ptr, ptr, ptr, ptr)",
        "declare i32 @pthread_join(ptr, ptr)",
        "declare i32 @pthread_mutex_init(ptr, ptr)",
        "declare i32 @pthread_mutex_lock(ptr)",
        "declare i32 @pthread_mutex_unlock(ptr)",
        "declare i32 @pthread_cond_init(ptr, ptr)",
        "declare i32 @pthread_cond_wait(ptr, ptr)",
        "declare i32 @pthread_cond_broadcast(ptr)",
        "declare i32 @pthread_mutex_destroy(ptr)",
        "declare i32 @pthread_cond_destroy(ptr)",
        "declare {i64, i1} @llvm.umul.with.overflow.i64(i64, i64)",
        "declare ptr @llvm.frameaddress.p0(i32)",
        "declare i32 @pthread_attr_init(ptr)",
        "declare i32 @pthread_attr_setstacksize(ptr, i64)",
        "declare i32 @pthread_attr_destroy(ptr)",
        "declare i32 @pthread_cond_timedwait(ptr, ptr, ptr)",
        "declare i32 @nz_os_clock(i32, ptr)",
        "declare i64 @atoll(ptr)",
        "declare i64 @nz_os_page_size()",
        "declare ptr @nz_os_stack_alloc(i64, i64)",
        "declare void @nz_os_stack_free(ptr, i64, i64)",
        "declare i64 @nz_os_stack_size()",
        "declare i32 @nz_os_close(i32)",
        "declare i64 @nz_os_read(i32, ptr, i64)",
        "declare i64 @nz_os_write(i32, ptr, i64)",
        "declare i64 @nz_os_lseek(i32, i64, i32)",
        "declare i32 @nz_os_fsync(i32)",
        "declare i32 @nz_os_unlink(ptr)",
        "declare i32 @nz_os_rmdir(ptr)",
        "declare i32 @nz_os_rename(ptr, ptr)",
        "declare i32 @nz_os_dirfd(ptr)",
        "declare i32 @nz_os_closedir(ptr)",
        "declare i32 @nz_os_entropy(ptr, i64)",
        "declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)",
        "declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)",
        "declare double @strtod(ptr, ptr)",
        "declare float @strtof(ptr, ptr)",
        "declare double @llvm.fabs.f64(double)",
        // 128-bit integers (R1b, L13b): the wide part's checked product and float conversions.
        "declare {i128, i1} @llvm.umul.with.overflow.i128(i128, i128)",
        "declare i128 @llvm.fptosi.sat.i128.f64(double)",
        "declare i128 @llvm.fptoui.sat.i128.f64(double)",
    ] {
        let _ = writeln!(out, "{d}");
    }
}
