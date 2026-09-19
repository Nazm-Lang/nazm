//! Cost accounting, built so that absent data cannot masquerade as zero.
//!
//! `docs/evaluation.md` §8 names this as the failure mode that can manufacture an
//! arbitrary winner: an arm whose usage records go missing looks cheap. The defence is
//! that absence is *representable* — [`CostComponent::Missing`] — and that every
//! aggregation carries the fact forward, so a total is never a bare number.
//!
//! Money is integer micro-USD. Floating point in a figure the project is judged on is a
//! quiet source of disagreement between runs, and `f64` addition is not associative, so
//! summing the same records in a different order could change the published result.

use std::fmt;
use std::iter::Sum;

use serde::{Deserialize, Serialize};

/// Micro-US-dollars. `1_000_000` is one dollar.
///
/// Integer because these are summed across thousands of attempts and then divided, and
/// the result is published. Exactness matters more than convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    #[must_use]
    pub const fn as_micros(self) -> i64 {
        self.0
    }

    /// Saturating rather than wrapping: a cost that silently wraps negative would be worse
    /// than one that is implausibly large, because the second is visible in review.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl fmt::Display for Money {
    // Integer division is the intent, not an oversight: this splits an exact micro-dollar
    // count into its dollar and fractional parts, and both halves are printed.
    #[allow(clippy::integer_division)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 1_000_000;
        let frac = (self.0 % 1_000_000).abs();
        write!(f, "${whole}.{frac:06}")
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Self::saturating_add)
    }
}

/// One line of the cost model, which is either recorded or explicitly absent.
///
/// There is deliberately no `Default`, and no `From<Money>` that could be reached by
/// accident. Constructing one requires saying which case it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CostComponent {
    Recorded(Money),
    /// The provider returned no usage, the run was interrupted, the meter was unavailable.
    /// The reason is retained because it is the difference between a harness bug and a
    /// provider outage, and those have different consequences for the result.
    Missing {
        reason: String,
    },
}

impl CostComponent {
    #[must_use]
    pub fn recorded(micros: i64) -> Self {
        Self::Recorded(Money::from_micros(micros))
    }

    #[must_use]
    pub fn missing(reason: impl Into<String>) -> Self {
        Self::Missing {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing { .. })
    }

    /// Deliberately returns `Option`, not `Money`. There is no "unwrap or zero" here,
    /// because that convenience is exactly the defect this module exists to prevent.
    #[must_use]
    pub fn value(&self) -> Option<Money> {
        match self {
            Self::Recorded(m) => Some(*m),
            Self::Missing { .. } => None,
        }
    }
}

/// The three cost lines from `docs/evaluation.md` §5, kept separate on purpose: they are
/// reported individually and are not interchangeable when interpreting a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    /// Provider charges for model usage.
    pub model: CostComponent,
    /// Compute for builds, test runs, and containers.
    pub infrastructure: CostComponent,
    /// Logged operator time, priced by the rate fixed in the pre-registration.
    pub human: CostComponent,
}

impl Cost {
    #[must_use]
    pub fn new(model: CostComponent, infrastructure: CostComponent, human: CostComponent) -> Self {
        Self {
            model,
            infrastructure,
            human,
        }
    }

    /// An attempt that cost nothing in every line — a cache hit, a task skipped by design.
    /// Named so that it can never be confused with "we failed to record the cost".
    #[must_use]
    pub fn known_free() -> Self {
        Self::new(
            CostComponent::recorded(0),
            CostComponent::recorded(0),
            CostComponent::recorded(0),
        )
    }

    #[must_use]
    pub fn components(&self) -> [(&'static str, &CostComponent); 3] {
        [
            ("model", &self.model),
            ("infrastructure", &self.infrastructure),
            ("human", &self.human),
        ]
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.components().iter().all(|(_, c)| !c.is_missing())
    }
}

/// The result of aggregating costs: a number, plus whether that number can be believed.
///
/// [`Total::total`] is intentionally not reachable without also seeing [`Total::gaps`].
/// A caller that wants the bare figure has to ask for it through [`Total::believable`],
/// which returns `None` when anything was missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Total {
    /// Sum of the components that *were* recorded. A lower bound, never the whole truth
    /// when `gaps` is non-empty.
    total: Money,
    /// How many attempts contributed.
    attempts: usize,
    /// One entry per missing component, as `(line, reason)`.
    gaps: Vec<(String, String)>,
}

impl Total {
    #[must_use]
    pub fn attempts(&self) -> usize {
        self.attempts
    }

    #[must_use]
    pub fn gaps(&self) -> &[(String, String)] {
        &self.gaps
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.gaps.is_empty()
    }

    /// The figure, but only when nothing was missing.
    ///
    /// `None` means the protocol's "invalidate or explicitly qualify" rule applies: the
    /// caller must either drop the result or publish it with the gaps attached. It must
    /// not substitute zero.
    #[must_use]
    pub fn believable(&self) -> Option<Money> {
        self.is_complete().then_some(self.total)
    }

    /// The sum of what was actually recorded, explicitly acknowledged as a lower bound.
    /// Named to be uncomfortable to use without thinking.
    #[must_use]
    pub fn lower_bound_with_gaps(&self) -> Money {
        self.total
    }

    /// Cost per accepted solution — the protocol's primary metric.
    ///
    /// Returns `None` if any cost was missing, or if nothing was accepted. Note the second
    /// case: zero acceptances is not an infinitely good score, and `docs/evaluation.md` §5
    /// requires this figure to be read alongside the acceptance rate for exactly that
    /// reason. An arm accepting 40/100 at $40 beats one accepting 90/100 at $100 on this
    /// number alone, which is why this number alone decides nothing.
    // Truncating division, deliberately. The residue is under one micro-dollar per
    // computation — far below the noise in any agent run — and truncation is
    // order-independent, where rounding would make the published figure depend on how the
    // records happened to be grouped. The bias is downward and uniform across arms, so it
    // cannot favour one.
    #[allow(clippy::integer_division)]
    #[must_use]
    pub fn per_accepted(&self, accepted: usize) -> Option<Money> {
        let total = self.believable()?;
        let accepted = i64::try_from(accepted).ok()?;
        if accepted == 0 {
            return None;
        }
        Some(Money::from_micros(total.as_micros() / accepted))
    }
}

/// Aggregate every attempt's cost, carrying missing-data facts forward.
///
/// Takes all attempts, not just successful ones: `docs/evaluation.md` §5 requires failed
/// attempts in the denominator, because an arm that fails cheaply and often is not cheap.
pub fn total<'a>(costs: impl IntoIterator<Item = &'a Cost>) -> Total {
    let mut sum = Money::ZERO;
    let mut attempts = 0usize;
    let mut gaps = Vec::new();

    for cost in costs {
        attempts += 1;
        for (line, component) in cost.components() {
            match component {
                CostComponent::Recorded(m) => sum = sum.saturating_add(*m),
                CostComponent::Missing { reason } => {
                    gaps.push((line.to_owned(), reason.clone()));
                }
            }
        }
    }

    Total {
        total: sum,
        attempts,
        gaps,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn complete(micros: i64) -> Cost {
        Cost::new(
            CostComponent::recorded(micros),
            CostComponent::recorded(0),
            CostComponent::recorded(0),
        )
    }

    #[test]
    fn a_missing_component_does_not_become_zero() {
        // The defect this module exists to prevent: an arm whose usage records vanish
        // must not be reported as cheap.
        let attempts = [
            complete(5_000_000),
            Cost::new(
                CostComponent::missing("provider returned no usage block"),
                CostComponent::recorded(0),
                CostComponent::recorded(0),
            ),
        ];

        let t = total(&attempts);
        assert_eq!(t.attempts(), 2);
        assert!(!t.is_complete());
        assert_eq!(t.gaps().len(), 1);
        assert!(
            t.believable().is_none(),
            "an incomplete total must not yield a figure"
        );
        assert!(
            t.per_accepted(2).is_none(),
            "and must not yield a derived figure either"
        );
    }

    #[test]
    fn the_gap_reason_survives_aggregation() {
        // A harness bug and a provider outage have different consequences for the result,
        // so the distinction has to reach whoever reads the run.
        let attempts = [Cost::new(
            CostComponent::missing("run interrupted at turn 7"),
            CostComponent::recorded(1),
            CostComponent::recorded(0),
        )];
        let t = total(&attempts);
        let (line, reason) = t
            .gaps()
            .first()
            .expect("the missing component must be recorded");
        assert_eq!(line, "model");
        assert!(reason.contains("interrupted"));
    }

    #[test]
    fn known_free_is_complete_and_distinct_from_missing() {
        let t = total(&[Cost::known_free()]);
        assert!(t.is_complete());
        assert_eq!(t.believable(), Some(Money::ZERO));
    }

    #[test]
    fn per_accepted_divides_total_by_acceptances() {
        let attempts = [complete(10_000_000), complete(10_000_000)];
        let t = total(&attempts);
        // $20 spent, 2 accepted -> $10 each.
        assert_eq!(t.per_accepted(2), Some(Money::from_micros(10_000_000)));
    }

    #[test]
    fn failed_attempts_stay_in_the_denominator() {
        // Four attempts, one acceptance: the three failures are not free.
        let attempts = [
            complete(1_000_000),
            complete(1_000_000),
            complete(1_000_000),
            complete(1_000_000),
        ];
        let t = total(&attempts);
        assert_eq!(t.per_accepted(1), Some(Money::from_micros(4_000_000)));
    }

    #[test]
    fn zero_acceptances_is_not_an_infinitely_good_score() {
        let t = total(&[complete(50_000_000)]);
        assert!(t.per_accepted(0).is_none());
    }

    #[test]
    fn money_sums_exactly_regardless_of_order() {
        // Integer arithmetic, so no associativity surprise. Guards the choice against a
        // well-meaning future change to f64.
        let a: Money = [1_i64, 2, 3].into_iter().map(Money::from_micros).sum();
        let b: Money = [3_i64, 2, 1].into_iter().map(Money::from_micros).sum();
        assert_eq!(a, b);
        assert_eq!(a.as_micros(), 6);
    }

    #[test]
    fn display_renders_micros_without_losing_precision() {
        assert_eq!(Money::from_micros(1_234_567).to_string(), "$1.234567");
    }
}
