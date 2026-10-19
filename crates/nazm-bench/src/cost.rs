//! Cost accounting, built so that absent data cannot masquerade as zero.
//!
//! `docs/internal/research/evaluation.md` §8 names this as the failure mode that can manufacture an
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

    /// Checked, not saturating and not wrapping.
    ///
    /// An earlier version saturated. That is the wrong failure mode for an accounting
    /// figure: it silently caps and the total keeps looking like a total. Overflow here
    /// would take ~9.2 trillion dollars and so means a unit or parsing bug, which is
    /// exactly the thing that must surface rather than be absorbed.
    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
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

/// Summing can overflow, so the sum is fallible. `Sum` is implemented over `Option` rather
/// than `Money` to make that unavoidable at the call site.
impl Sum<Money> for Option<Money> {
    fn sum<I: Iterator<Item = Money>>(mut iter: I) -> Self {
        iter.try_fold(Money::ZERO, Money::checked_add)
    }
}

/// One line of the cost model, which is either recorded or explicitly absent.
///
/// There is deliberately no `Default`, and no `From<Money>` that could be reached by
/// accident. Constructing one requires saying which case it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CostComponent {
    // A struct variant, not `Recorded(Money)`. serde's internally-tagged representation
    // cannot serialize a newtype variant wrapping an integer, and the newtype form failed
    // at runtime with "cannot serialize tagged newtype variant" — meaning no run record
    // could ever have been written to disk. `cost_round_trips_through_json` pins it.
    Recorded {
        amount: Money,
    },
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
        Self::Recorded {
            amount: Money::from_micros(micros),
        }
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
            Self::Recorded { amount } => Some(*amount),
            Self::Missing { .. } => None,
        }
    }
}

/// The three cost lines from `docs/internal/research/evaluation.md` §5, kept separate on purpose: they are
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
    /// Set when summation overflowed. Invalidates the total exactly as a gap does: a
    /// figure that overflowed is not a smaller figure, it is not a figure.
    overflowed: bool,
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
        self.gaps.is_empty() && !self.overflowed
    }

    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
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
    /// case: zero acceptances is not an infinitely good score, and `docs/internal/research/evaluation.md` §5
    /// requires this figure to be read alongside the acceptance rate for exactly that
    /// reason. An arm accepting 40/100 at $40 beats one accepting 90/100 at $100 on this
    /// number alone, which is why this number alone decides nothing.
    // Truncating division, deliberately — but the justification matters more than the
    // choice, and an earlier comment here got it wrong.
    //
    // It claimed the bias "is uniform across arms, so it cannot favour one". That is false
    // in general: with per-record truncation, an arm with many small records loses more
    // than an arm with few large ones, because floor(a) + floor(b) != floor(a + b). Two
    // records of 0.6 truncate to 0 while one record of 1.0 truncates to 1, and the cheaper
    // arm reads as more expensive.
    //
    // What makes it safe here is structural, not statistical: amounts are exact i64
    // micro-dollars end to end, `total` sums them with no rounding at all, and this is the
    // *only* division in the aggregation path — one, at the presentation boundary. Total
    // residue is therefore under one micro-dollar for an entire arm, not per record.
    // `regrouping_records_cannot_change_the_total` pins that property so a future change
    // that introduces per-record rounding fails the build rather than the review.
    #[allow(clippy::integer_division)]
    #[must_use]
    pub fn per_accepted(&self, accepted: usize) -> CostPerAccepted {
        let Some(total) = self.believable() else {
            return CostPerAccepted::Incomplete {
                gaps: self.gaps.len() + usize::from(self.overflowed),
                accepted,
            };
        };
        let Ok(accepted_i64) = i64::try_from(accepted) else {
            return CostPerAccepted::Incomplete { gaps: 0, accepted };
        };
        let accepted = accepted_i64;
        if accepted == 0 {
            // Not zero cost, and not missing data. Money was spent and nothing was
            // accepted, which is a result about the arm and must read as one.
            return CostPerAccepted::NoAcceptedSolutions { spend: total };
        }
        CostPerAccepted::Value {
            amount: Money::from_micros(total.as_micros() / accepted),
        }
    }
}

/// The outcome of dividing spend by acceptances.
///
/// An enum rather than `Option<Money>` because the previous `Option` conflated two
/// different facts — "cost data is missing" and "nothing was accepted" — and a caller that
/// cannot tell them apart will eventually treat one as the other. An arm that accepted
/// nothing did not have an unmeasurable cost; it failed everything, at a price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CostPerAccepted {
    // A struct variant, not `Value(Money)`: serde's internally-tagged representation
    // cannot serialize a newtype variant wrapping an integer. That defect already shipped
    // once in this file (see `CostComponent::Recorded`) and is not repeating.
    Value {
        amount: Money,
    },
    /// Spend occurred and nothing was accepted. Never rendered as zero.
    NoAcceptedSolutions {
        spend: Money,
    },
    /// A cost component was missing, or the sum overflowed.
    ///
    /// Carries `accepted` because the two facts are independent and a report needs both:
    /// an arm with unusable cost data that also accepted nothing is in a different
    /// situation from one with unusable cost data that accepted everything. An earlier
    /// version returned early on incompleteness and dropped the acceptance count.
    Incomplete {
        gaps: usize,
        accepted: usize,
    },
}

impl CostPerAccepted {
    /// The figure, when there is one. `None` for both other cases — but callers must
    /// handle those cases explicitly rather than reaching for this first.
    #[must_use]
    pub fn value(self) -> Option<Money> {
        match self {
            Self::Value { amount } => Some(amount),
            Self::NoAcceptedSolutions { .. } | Self::Incomplete { .. } => None,
        }
    }

    #[must_use]
    pub fn is_value(self) -> bool {
        matches!(self, Self::Value { .. })
    }

    /// How many solutions were accepted, in every case.
    ///
    /// Available even when the cost figure is not, because "the data is unusable" and
    /// "nothing was accepted" are independent facts that a report needs both of.
    #[must_use]
    pub fn accepted(self) -> usize {
        match self {
            Self::Value { .. } => 1, // at least one; the exact count lives on ArmSummary
            Self::NoAcceptedSolutions { .. } => 0,
            Self::Incomplete { accepted, .. } => accepted,
        }
    }
}

/// Aggregate every attempt's cost, carrying missing-data facts forward.
///
/// Takes all attempts, not just successful ones: `docs/internal/research/evaluation.md` §5 requires failed
/// attempts in the denominator, because an arm that fails cheaply and often is not cheap.
pub fn total<'a>(costs: impl IntoIterator<Item = &'a Cost>) -> Total {
    let mut sum = Some(Money::ZERO);
    let mut attempts = 0usize;
    let mut gaps = Vec::new();

    for cost in costs {
        attempts += 1;
        for (line, component) in cost.components() {
            match component {
                // No rounding here, ever. Exact micro-dollars accumulate, and the single
                // division in the whole path happens in `per_accepted`.
                CostComponent::Recorded { amount } => {
                    sum = sum.and_then(|s| s.checked_add(*amount));
                }
                CostComponent::Missing { reason } => {
                    gaps.push((line.to_owned(), reason.clone()));
                }
            }
        }
    }

    Total {
        total: sum.unwrap_or(Money::ZERO),
        attempts,
        gaps,
        overflowed: sum.is_none(),
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
            !t.per_accepted(2).is_value(),
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
        assert_eq!(
            t.per_accepted(2).value(),
            Some(Money::from_micros(10_000_000))
        );
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
        assert_eq!(
            t.per_accepted(1).value(),
            Some(Money::from_micros(4_000_000))
        );
    }

    #[test]
    fn zero_acceptances_is_not_an_infinitely_good_score() {
        let t = total(&[complete(50_000_000)]);
        assert!(matches!(
            t.per_accepted(0),
            CostPerAccepted::NoAcceptedSolutions { .. }
        ));
    }

    #[test]
    fn regrouping_records_cannot_change_the_total() {
        // The property the single-division design exists to hold. If anyone introduces
        // per-record rounding, this fails: floor(a) + floor(b) != floor(a + b).
        //
        // Same money, three groupings.
        let one_record = [complete(1_500_001)];
        let two_records = [complete(500_000), complete(1_000_001)];
        let many_records = [complete(1), complete(500_000), complete(1_000_000)];

        let a = total(&one_record).believable();
        let b = total(&two_records).believable();
        let c = total(&many_records).believable();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, Some(Money::from_micros(1_500_001)));
    }

    #[test]
    fn the_reviewers_truncation_counterexample_does_not_bite() {
        // Stated against an earlier, wrong justification ("the bias is uniform across arms
        // so it cannot favour one"). With per-record truncation to whole units, an arm of
        // two 0.6 records would read as 0 while an arm of one 1.0 record reads as 1 — the
        // cheaper arm appearing more expensive.
        //
        // Scaled into micro-dollars: A2 spends 1.2 over two records, A3 spends 1.0 over
        // one. A2 must remain the more expensive arm.
        let arm_a2 = [complete(600_000), complete(600_000)];
        let arm_a3 = [complete(1_000_000)];

        let a2 = total(&arm_a2).believable().unwrap();
        let a3 = total(&arm_a3).believable().unwrap();
        assert_eq!(a2, Money::from_micros(1_200_000));
        assert_eq!(a3, Money::from_micros(1_000_000));
        assert!(
            a2 > a3,
            "record count must not invert the ordering of two arms"
        );
    }

    #[test]
    fn overflow_invalidates_the_total_instead_of_capping() {
        // Saturating arithmetic would report i64::MAX as though it were a total. An
        // overflowed figure is not a large figure; it is not a figure.
        let huge = [complete(i64::MAX), complete(1)];
        let t = total(&huge);
        assert!(t.overflowed());
        assert!(!t.is_complete());
        assert!(t.believable().is_none());
        assert!(!t.per_accepted(1).is_value());
    }

    #[test]
    fn cost_round_trips_through_json() {
        // This test did not exist, and its absence hid a defect that would have broken
        // every run record: the original `Recorded(Money)` newtype variant could not be
        // serialized under `#[serde(tag = "kind")]` at all.
        let c = Cost::new(
            CostComponent::recorded(1_500_000),
            CostComponent::recorded(0),
            CostComponent::missing("no operator time logged"),
        );
        let json = serde_json::to_string(&c).expect("a cost record must be writable");
        assert!(json.contains(r#""kind":"recorded""#));
        assert!(json.contains(r#""amount":1500000"#));
        assert_eq!(serde_json::from_str::<Cost>(&json).unwrap(), c);
    }

    #[test]
    fn every_cost_line_is_structurally_required() {
        // Propagating an explicit `Missing` does not help against a component that was
        // never inserted. `Cost` has three non-optional fields and no `serde(default)`, so
        // a record omitting one fails to deserialise rather than defaulting to zero.
        let missing_a_line = r#"{"model":{"kind":"recorded","amount":1},
                                 "infrastructure":{"kind":"recorded","amount":1}}"#;
        assert!(
            serde_json::from_str::<Cost>(missing_a_line).is_err(),
            "a record without every cost line must be rejected, not defaulted"
        );

        let complete_record = r#"{"model":{"kind":"recorded","amount":1},
                                  "infrastructure":{"kind":"recorded","amount":1},
                                  "human":{"kind":"missing","reason":"none logged"}}"#;
        let parsed: Cost = serde_json::from_str(complete_record).unwrap();
        assert!(!parsed.is_complete());
    }

    #[test]
    fn money_sums_exactly_regardless_of_order() {
        // Integer arithmetic, so no associativity surprise. Guards the choice against a
        // well-meaning future change to f64.
        let a: Option<Money> = [1_i64, 2, 3].into_iter().map(Money::from_micros).sum();
        let b: Option<Money> = [3_i64, 2, 1].into_iter().map(Money::from_micros).sum();
        assert_eq!(a, b);
        assert_eq!(a.map(Money::as_micros), Some(6));
    }

    #[test]
    fn display_renders_micros_without_losing_precision() {
        assert_eq!(Money::from_micros(1_234_567).to_string(), "$1.234567");
    }
}
