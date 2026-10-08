//! Channels between tasks, and `select` over several of them.

use super::counters::{CHANNELS_ALLOCATED, CHANNELS_RECLAIMED, SCHED_SELECTS, SCHED_TIMEOUTS};
use super::value::Value;
use super::*;

/// A bounded queue with a close flag.
///
/// One mutex and one condition variable. Both senders and receivers wait on the same
/// variable and every change notifies all of them — which is not the fastest arrangement
/// and is the obviously correct one, and `docs/spec.md` promises no fairness anyway.
#[derive(Debug)]
pub struct Channel {
    state: Mutex<ChannelState>,
    changed: std::sync::Condvar,
    capacity: usize,
}

#[derive(Debug, Default)]
struct ChannelState {
    /// What is queued: `Int`s for a `Chan`, any value for a `Chan[T]` (N53). One channel for
    /// both, since a value carries no static type here and the semantics are one.
    queue: std::collections::VecDeque<Value>,
    closed: bool,
}

impl Channel {
    pub(super) fn new(capacity: usize) -> Self {
        CHANNELS_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            state: Mutex::new(ChannelState::default()),
            changed: std::sync::Condvar::new(),
            capacity,
        }
    }

    /// Queue a value, or report that the channel is closed — in which case **the value
    /// was not sent**, which is what `docs/spec.md` promises.
    pub(super) fn send(&self, value: Value) -> Option<bool> {
        let mut state = self.state.lock().ok()?;
        while !state.closed && state.queue.len() >= self.capacity {
            state = self.changed.wait(state).ok()?;
        }
        if state.closed {
            return Some(false);
        }
        state.queue.push_back(value);
        drop(state);
        self.changed.notify_all();
        select_event();
        Some(true)
    }

    /// One look without waiting (N54): a value taken, the channel closed and drained, or
    /// neither.
    fn try_take(&self) -> Option<Ready> {
        let mut state = self.state.lock().ok()?;
        let ready = match state.queue.pop_front() {
            Some(v) => Ready::Value(v),
            None if state.closed => Ready::Closed,
            None => Ready::Empty,
        };
        drop(state);
        if matches!(ready, Ready::Value(_)) {
            self.changed.notify_all();
        }
        Some(ready)
    }

    /// Take a value, or report that the channel is closed **and drained**. Values queued
    /// before a close are still delivered: closing ends a channel, it does not discard
    /// what is in it.
    pub(super) fn recv(&self) -> Option<Option<Value>> {
        let mut state = self.state.lock().ok()?;
        while state.queue.is_empty() && !state.closed {
            state = self.changed.wait(state).ok()?;
        }
        let taken = state.queue.pop_front();
        drop(state);
        self.changed.notify_all();
        Some(taken)
    }

    /// Idempotent, and wakes everyone: a blocked sender and a blocked receiver both have
    /// to find out, and neither can be left waiting on a channel nobody will touch again.
    pub(super) fn close(&self) -> Option<()> {
        let mut state = self.state.lock().ok()?;
        state.closed = true;
        drop(state);
        self.changed.notify_all();
        select_event();
        Some(())
    }
}

/// What one look at a channel found (N54).
enum Ready {
    Value(Value),
    Closed,
    Empty,
}

/// The select event count (N54, §7.56): every send and close advances it, and a select waits
/// only while it has not moved since the select last looked — under its own lock, so no
/// wake-up between a look and a wait is lost. One for every channel: a select cannot wait on
/// several condition variables.
static SELECT_EVENTS: Mutex<u64> = Mutex::new(0);

static SELECT_WOKEN: std::sync::Condvar = std::sync::Condvar::new();

fn select_event() {
    if let Ok(mut g) = SELECT_EVENTS.lock() {
        *g = g.wrapping_add(1);
    }
    SELECT_WOKEN.notify_all();
}

/// What a select found: the lowest ready index, with the value it took if any; `-1` for no
/// channels; `-2` when the deadline passed first.
pub(super) fn select(
    chans: &[Arc<Channel>],
    deadline: Option<std::time::Instant>,
) -> Option<(i64, Option<Value>)> {
    if chans.is_empty() {
        return Some((-1, None));
    }
    SCHED_SELECTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    loop {
        let seen = *SELECT_EVENTS.lock().ok()?;
        for (i, c) in chans.iter().enumerate() {
            let at = i64::try_from(i).unwrap_or(i64::MAX);
            match c.try_take()? {
                Ready::Value(v) => return Some((at, Some(v))),
                Ready::Closed => return Some((at, None)),
                Ready::Empty => {}
            }
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            SCHED_TIMEOUTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some((-2, None));
        }
        let mut g = SELECT_EVENTS.lock().ok()?;
        while *g == seen {
            match deadline {
                Some(d) => {
                    let now = std::time::Instant::now();
                    if now >= d {
                        break;
                    }
                    g = SELECT_WOKEN.wait_timeout(g, d - now).ok()?.0;
                }
                None => g = SELECT_WOKEN.wait(g).ok()?,
            }
        }
    }
}

/// Counted for the same reason [`Seq`] is: the host reclaims the mutex, the condition
/// variable and the queue when the last `Arc` goes, and *"the host does the right thing"*
/// is not evidence that the **language** reclaims a channel. The compiled program counts
/// the same event at the same moment, and `crates/nazm-cli/tests/memory.rs` compares them.
impl Drop for Channel {
    fn drop(&mut self) {
        CHANNELS_RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
