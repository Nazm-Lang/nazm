//! What the interpreter allocated, reclaimed and scheduled: the counters behind the memory and
//! scheduler reports a run prints when asked.

pub(super) static ALLOCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) static RECLAIMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) static STRINGS_ALLOCATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static STRINGS_RECLAIMED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static CHANNELS_ALLOCATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static CHANNELS_RECLAIMED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static CLOSURES_ALLOCATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static CLOSURES_RECLAIMED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static SCHED_SPAWNED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static SCHED_JOINED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) static SCHED_SELECTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static SCHED_TIMEOUTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// What the scheduler did (N54): tasks started and joined, selects, and selects that timed
/// out. The same four counts a compiled program reports under `NAZM_SCHED_REPORT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedCounts {
    pub spawned: u64,
    pub joined: u64,
    pub selects: u64,
    pub timeouts: u64,
}

/// This process's scheduler counts.
#[must_use]
pub fn sched_counts() -> SchedCounts {
    let n = |c: &std::sync::atomic::AtomicU64| c.load(std::sync::atomic::Ordering::Relaxed);
    SchedCounts {
        spawned: n(&SCHED_SPAWNED),
        joined: n(&SCHED_JOINED),
        selects: n(&SCHED_SELECTS),
        timeouts: n(&SCHED_TIMEOUTS),
    }
}

/// What one class of storage did: created, reclaimed.
///
/// Three classes rather than one total, because they answer different questions and N8
/// makes them independently reclaimable. A slice is **not** an allocation: it takes a
/// reference to a backing that already exists, and counting it as one would report a leak
/// where nothing was allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub sequences: (u64, u64),
    pub strings: (u64, u64),
    pub channels: (u64, u64),
    /// Function values (N50): one object a closure or a named function's value makes.
    pub closures: (u64, u64),
}

/// What this process allocated and reclaimed, by class.
///
/// For the driver's `NAZM_MEMORY_REPORT` line and for the tests that compare the two
/// implementations. Not reachable from a Nazm program, for the reason the compiled
/// counters are not: the specification states *when* storage is reclaimed and deliberately
/// not *how*.
#[must_use]
pub fn memory_counts() -> Counts {
    let n = |c: &std::sync::atomic::AtomicU64| c.load(std::sync::atomic::Ordering::Relaxed);
    Counts {
        sequences: (n(&ALLOCATED), n(&RECLAIMED)),
        strings: (n(&STRINGS_ALLOCATED), n(&STRINGS_RECLAIMED)),
        channels: (n(&CHANNELS_ALLOCATED), n(&CHANNELS_RECLAIMED)),
        closures: (n(&CLOSURES_ALLOCATED), n(&CLOSURES_RECLAIMED)),
    }
}
