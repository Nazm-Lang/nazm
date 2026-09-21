//! The gate every generated compiler stage passes through before it is allowed to run.
//!
//! # Why this exists
//!
//! On 2026-09-21 `cargo test --test selfhost` stopped the machine three times. The suite
//! builds `nzlex`, `nzparse`, `nzcheck` and `nzemit` and runs them natively with no
//! ceiling; the drivers accumulated their output by copying, so each grew without bound,
//! and `cargo test` runs one test thread per core — ten of them at once on a 32 GiB
//! machine. The kernel's userspace watchdog panicked because nothing could be scheduled.
//!
//! The accumulation bug is fixed (see `tests/resource_regression.rs`). This gate is the
//! other half: a bug of that shape must not be able to reach the host again. macOS cannot
//! impose a hard memory ceiling on a child — `RLIMIT_AS` is an alias of `RLIMIT_RSS`
//! there and `setrlimit(2)` documents it as advisory — so the ceiling has to come from
//! outside the process, and the refusal has to be explicit rather than a silent skip.

use std::sync::Once;

/// Set inside the bounded environment, where a real ceiling already applies.
const CONTAINED: &str = "NAZM_CONTAINED";
/// Deliberate, documented waiver. Loud on purpose.
const WAIVER: &str = "NAZM_ALLOW_UNCONTAINED";

/// Refuses to let a generated compiler stage run without a resource ceiling.
///
/// Call this from every function that produces a stage executable, so no launch site can
/// bypass it by construction.
pub(crate) fn require_containment(stage: &str) {
    if std::env::var(CONTAINED).as_deref() == Ok("1") {
        return;
    }
    if std::env::var(WAIVER).as_deref() == Ok("1") {
        static WARNED: Once = Once::new();
        WARNED.call_once(|| {
            eprintln!(
                "warning: {WAIVER}=1 — generated compiler stages are running with no memory \
                 ceiling. This is the configuration that panicked the machine on 2026-09-21."
            );
        });
        return;
    }
    panic!(
        "refusing to build and run `{stage}` with no resource ceiling.\n\
         \n\
         This suite executes compiler stages written in Nazm. A stage that allocates \
         without bound has no ceiling on this host: macOS `RLIMIT_AS` is advisory, so a \
         runaway reaches the machine rather than the process. On 2026-09-21 that panicked \
         the kernel three times.\n\
         \n\
         Run it inside the bounded environment, one workload at a time:\n\
         \n    NAZM_CONTAINED=1 cargo test -p nazm-cli --test selfhost --jobs 1 -- --test-threads 1\n\
         \n\
         and set {CONTAINED}=1 only where a real ceiling applies (a container with \
         --memory and --memory-swap set equal, or a cgroup that enforces one).\n\
         To override deliberately and accept the risk, set {WAIVER}=1."
    );
}

/// The address-space ceiling a generated program runs under, in kilobytes: 512 MiB.
///
/// Chosen against measurement, not taste. The largest legitimate program in the tree
/// peaks at 4.4 MB resident, the self-hosted compiler stages at about 36 MB, and each
/// pthread a concurrency test spawns reserves roughly 8 MB of address space for its
/// stack. 512 MiB clears all of that by two orders of magnitude while sitting well below
/// the container's own 4 GiB ceiling — so a runaway dies as a *test failure*, attributed
/// to the test that caused it, instead of taking the whole run down with an
/// infrastructure OOM.
const ADDRESS_SPACE_KB: u64 = 512 * 1024;

/// A `Command` that runs `exe` under that ceiling.
///
/// # What this bounds, and what it does not
///
/// `ulimit -v` sets `RLIMIT_AS`, which bounds **virtual address space**, not resident
/// memory. A program that maps a great deal and touches little will be refused even
/// though its footprint is small — which is why the value is generous rather than tight.
/// It is the right tool here anyway, because the failure being guarded against is
/// unbounded `malloc`, and that grows the address space.
///
/// **It is effective on Linux and not on Darwin**, where `RLIMIT_AS` is an alias of
/// `RLIMIT_RSS` and `setrlimit(2)` documents it as advisory. That is why it is a second
/// line rather than the first: the container is the ceiling that actually holds. The
/// `2>/dev/null` and the unconditional `exec` keep the wrapper harmless where the limit
/// is not enforced.
///
/// A shell wrapper rather than `pre_exec`, because the workspace sets
/// `unsafe_code = "forbid"` and `pre_exec` is unsafe. The limit is inherited by every
/// descendant, threads included, since they share the address space.
pub(crate) fn bounded_exe(exe: &std::path::Path) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.arg("-c")
        .arg(format!(
            "ulimit -v {ADDRESS_SPACE_KB} 2>/dev/null; exec \"$0\" \"$@\""
        ))
        .arg(exe);
    c
}
