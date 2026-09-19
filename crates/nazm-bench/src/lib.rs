//! Agent-cost benchmark harness.
//!
//! This crate exists before any Nazm compiler does, and that ordering is the point: the
//! project's whole justification rests on a comparative measurement, so the instrument has
//! to work — and be shown to work — before the thing it measures is built.
//!
//! The protocol this implements is [`docs/evaluation.md`]. Two of its rules shape almost
//! every type here:
//!
//! 1. **Missing cost data must never silently become zero cost.** A harness that drops a
//!    usage record and quietly reports `$0` can manufacture an arbitrary winner. See
//!    [`cost`], where absence is representable and propagates through every aggregation.
//! 2. **All attempts count, including failures.** Post-success activity is categorised to
//!    explain where cost went, never to exclude it from the denominator. See [`outcome`].
//!
//! [`docs/evaluation.md`]: https://github.com/mdferdousalam/nazm/blob/main/docs/evaluation.md

pub mod cost;
pub mod outcome;
pub mod prereg;
pub mod record;
pub mod report;
pub mod selftest;
pub mod task;

pub use cost::{Cost, CostComponent, Money, Total};
pub use outcome::{Outcome, PostSuccessCategory};
pub use prereg::PreRegistration;
pub use record::Attempt;
pub use report::{ArmSummary, Comparison};
pub use task::{Arm, TaskSet, TaskSpec};
