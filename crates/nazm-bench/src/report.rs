//! Aggregation across attempts.
//!
//! This computes **point estimates only**. It deliberately does not decide the gate.
//!
//! `docs/evaluation.md` §6 requires uncertainty estimated on the difference and the ratio
//! themselves, with task as a random effect and model as a second grouping factor —
//! repeated runs of one task are not independent samples. That is a mixed-effects model,
//! it belongs in R or Python, and writing a hand-rolled approximation here would produce a
//! number that looks like the real one and is not. So the harness emits records and point
//! estimates; the decision is made by the analysis, under the registered rules.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::cost::{self, CostPerAccepted, Total};
use crate::record::Attempt;
use crate::task::{Arm, Family};

/// Per-arm aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmSummary {
    pub arm: Arm,
    /// Attempts that count: everything except void ones.
    pub attempts: usize,
    /// Excluded as evaluator defects or protocol violations.
    pub void: usize,
    /// Void attempts as a fraction of all attempts by this arm.
    ///
    /// Reported per arm because a high void rate is itself a finding: if one arm voids far
    /// more often than another, the comparison is not between two arms, it is between one
    /// arm and a partly-broken measurement of another.
    pub void_rate: Option<f64>,
    /// What the void attempts cost, on its own line.
    pub void_cost: Total,
    pub accepted: usize,
    /// Every attempt's cost, void included. The numerator of the primary metric.
    pub cost: Total,
    /// Acceptance rate over counted attempts. `None` when nothing counted.
    pub acceptance_rate: Option<f64>,
    /// **The primary metric.** All attributable spend — failures, retries, and void
    /// attempts — divided by acceptances. `docs/evaluation.md` §5 defines it as "total cost
    /// of all attempts", and this is that.
    ///
    /// An earlier version excluded void spend here and reported it separately. That was a
    /// silent change to the agreed metric, and it could invert the gate: two arms each
    /// accepting 10, A2 spending $10 with no voids and A3 spending $8 plus $12 of void,
    /// reported as $1.00 vs $0.80 — passing the `R_C ≤ 0.80` condition on what was really
    /// a 2× cost regression. Reporting void spend on its own line makes that visible to a
    /// careful reader and does nothing to stop the gate deciding on the wrong number.
    pub total_cost_per_accepted: CostPerAccepted,
    /// **Secondary, diagnostic only.** The same figure with evaluator-fault spend removed.
    ///
    /// Useful for separating "this arm is expensive" from "the infrastructure was flaky
    /// while this arm happened to run". Never used by the gate.
    pub cost_per_accepted_excluding_evaluator_faults: CostPerAccepted,
    /// Acceptance by family, for the "no category collapse" condition.
    pub by_family: IndexMap<Family, FamilyCount>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct FamilyCount {
    pub attempts: usize,
    pub accepted: usize,
}

/// Summarise one arm's attempts.
///
/// Takes every attempt including failures: `docs/evaluation.md` §5 puts failed attempts in
/// the denominator, because an arm that fails cheaply and often is not cheap.
#[must_use]
pub fn summarise(arm: Arm, attempts: &[Attempt]) -> ArmSummary {
    let mine: Vec<&Attempt> = attempts.iter().filter(|a| a.arm == arm).collect();

    // Every attempt's cost, taken before the partition: the primary metric's numerator is
    // all spend, and computing it from `counted` is the defect this ordering prevents.
    let all_costs: Vec<&crate::cost::Cost> = mine.iter().map(|a| &a.cost).collect();
    let all_total = cost::total(all_costs);

    let (void, counted): (Vec<&Attempt>, Vec<&Attempt>) =
        mine.into_iter().partition(|a| a.is_void());

    let accepted = counted.iter().filter(|a| a.is_accepted()).count();
    let costs: Vec<&crate::cost::Cost> = counted.iter().map(|a| &a.cost).collect();
    let total = cost::total(costs);

    let mut by_family: IndexMap<Family, FamilyCount> = IndexMap::new();
    for a in &counted {
        let entry = by_family.entry(a.family).or_default();
        entry.attempts += 1;
        if a.is_accepted() {
            entry.accepted += 1;
        }
    }

    // Acceptance rate is a proportion, so f64 is right here — unlike money, which stays
    // exact. Guarded against an empty denominator rather than producing NaN, which would
    // propagate silently through any downstream comparison.
    let acceptance_rate = (!counted.is_empty()).then(|| {
        #[allow(clippy::cast_precision_loss)]
        {
            accepted as f64 / counted.len() as f64
        }
    });

    let void_costs: Vec<&crate::cost::Cost> = void.iter().map(|a| &a.cost).collect();
    let void_total = cost::total(void_costs);

    #[allow(clippy::cast_precision_loss)]
    let void_rate = {
        let all = counted.len() + void.len();
        (all > 0).then(|| void.len() as f64 / all as f64)
    };

    ArmSummary {
        arm,
        attempts: counted.len(),
        void: void.len(),
        void_rate,
        void_cost: void_total,
        accepted,
        total_cost_per_accepted: all_total.per_accepted(accepted),
        cost_per_accepted_excluding_evaluator_faults: total.per_accepted(accepted),
        cost: all_total,
        acceptance_rate,
        by_family,
    }
}

/// The A3-vs-A2 comparison, as point estimates.
///
/// Named `Comparison`, not `GateResult`, on purpose: it carries no verdict. `delta_p` and
/// `cost_ratio` are the estimands from `docs/evaluation.md` §6, but the thresholds there
/// are stated on confidence bounds, and this struct has none.
/// The cost ratio, or an explicit reason it could not be formed.
///
/// An `Option<f64>` here would fold four different situations into one `None`, and one of
/// them — a comparator that genuinely cost nothing — is a real finding rather than a gap
/// in the data. Division by zero was already guarded; what was missing was saying so.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CostRatio {
    Value(f64),
    /// The comparator's cost per accepted solution was exactly zero. Not an error, not
    /// infinity — a fact that has to be looked at.
    ComparatorCostZero {
        subject_cost: crate::cost::Money,
    },
    SubjectFigureUnavailable,
    ComparatorFigureUnavailable,
}

impl CostRatio {
    #[must_use]
    pub fn value(self) -> Option<f64> {
        match self {
            Self::Value(r) => Some(r),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub subject: Arm,
    pub comparator: Arm,
    /// Δp = p₃ − p₂.
    pub delta_p: Option<f64>,
    /// R_C = C₃ / C₂, or why it could not be formed.
    pub cost_ratio: CostRatio,
    /// Families where the subject fell below 70% of the comparator's acceptance.
    pub collapsed_families: Vec<Family>,
    /// Always true: the gate decision needs confidence bounds this does not compute.
    pub requires_external_analysis: bool,
}

/// Compare an arm against its **gate comparator**, which for Nazm is the tooled TypeScript
/// arm and never the baseline. Returns `None` for arms that have no gate comparator.
#[must_use]
pub fn compare(subject: &ArmSummary, all: &[ArmSummary]) -> Option<Comparison> {
    let comparator_arm = subject.arm.gate_comparator()?;
    let comparator = all.iter().find(|s| s.arm == comparator_arm)?;

    let delta_p = match (subject.acceptance_rate, comparator.acceptance_rate) {
        (Some(p3), Some(p2)) => Some(p3 - p2),
        _ => None,
    };

    // Built from the PRIMARY metric. Using the evaluator-fault-excluding figure here would
    // let an arm that voids expensively look cheap to the gate.
    let cost_ratio = match (
        subject.total_cost_per_accepted.value(),
        comparator.total_cost_per_accepted.value(),
    ) {
        (Some(c3), Some(c2)) if c2.as_micros() != 0 =>
        {
            #[allow(clippy::cast_precision_loss)]
            CostRatio::Value(c3.as_micros() as f64 / c2.as_micros() as f64)
        }
        // Complete data, and the comparator genuinely cost nothing. The ratio is not
        // large, it is undefined — and it is a distinct situation from having no figure,
        // so it gets its own variant rather than falling into a catch-all `None`.
        (Some(c3), Some(_)) => CostRatio::ComparatorCostZero { subject_cost: c3 },
        (None, _) => CostRatio::SubjectFigureUnavailable,
        (_, None) => CostRatio::ComparatorFigureUnavailable,
    };

    let mut collapsed_families = Vec::new();
    for (family, comp) in &comparator.by_family {
        if comp.attempts == 0 {
            continue;
        }
        let subj = subject.by_family.get(family).copied().unwrap_or_default();
        #[allow(clippy::cast_precision_loss)]
        let (subj_rate, comp_rate) = (
            if subj.attempts == 0 {
                0.0
            } else {
                subj.accepted as f64 / subj.attempts as f64
            },
            comp.accepted as f64 / comp.attempts as f64,
        );
        if comp_rate > 0.0 && subj_rate < 0.7 * comp_rate {
            collapsed_families.push(*family);
        }
    }

    Some(Comparison {
        subject: subject.arm,
        comparator: comparator_arm,
        delta_p,
        cost_ratio,
        collapsed_families,
        requires_external_analysis: true,
    })
}
