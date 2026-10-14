//! Where an evaluation thread's stack ends (R1, 4.9): the depth limit is an estimate of what a
//! call costs, and this is the bound that holds whatever it costs.

use super::STACK_BASE_BYTES;

std::thread_local! {
    /// The lowest stack address a call may begin at on this evaluation thread, `0` where none is
    /// known (R1, 4.9): what is left below it is the margin the passes that recurse with the
    /// program's shape need, so a call past it reports `N0403` rather than crashing.
    static STACK_FLOOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Record, at the start of a thread whose stack is `size` bytes, where its calls must stop.
pub(crate) fn mark_stack(size: usize) {
    let here = 0u8;
    let top = std::ptr::addr_of!(here) as usize;
    let floor = top
        .saturating_sub(size)
        .saturating_add(STACK_BASE_BYTES.min(size >> 1));
    STACK_FLOOR.with(|f| f.set(floor));
}

/// Whether this thread's stack is too far used for another call (R1, 4.9). The per-call budget
/// is an estimate, and a call nested in an expression costs several evaluator frames on a debug
/// build; this is the bound that holds whatever a call costs.
pub(crate) fn stack_exhausted() -> bool {
    let here = 0u8;
    let at = std::ptr::addr_of!(here) as usize;
    STACK_FLOOR.with(|f| f.get() != 0 && at < f.get())
}
