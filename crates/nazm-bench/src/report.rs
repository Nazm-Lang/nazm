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

use crate::cost::{self, Money, Total};
use crate::record::Attempt;
use crate::task::{Arm, Family};

/// Per-arm aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmSummary {
    pub arm: Arm,
    /// Attempts that count: everything except void ones.
    pub attempts: usize,
    /// Excluded as instrument defects or protocol violations.
    pub void: usize,
    pub accepted: usize,
    pub cost: Total,
    /// Acceptance rate over counted attempts. `None` when nothing counted.
    pub acceptance_rate: Option<f64>,
    /// Cost per accepted solution. `None` when any cost was missing, the sum overflowed,
    /// or nothing was accepted — never a stand-in figure.
    pub cost_per_accepted: Option<Money>,
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

    ArmSummary {
        arm,
        attempts: counted.len(),
        void: void.len(),
        accepted,
        cost_per_accepted: total.per_accepted(accepted),
        cost: total,
        acceptance_rate,
        by_family,
    }
}

/// The A3-vs-A2 comparison, as point estimates.
///
/// Named `Comparison`, not `GateResult`, on purpose: it carries no verdict. `delta_p` and
/// `cost_ratio` are the estimands from `docs/evaluation.md` §6, but the thresholds there
/// are stated on confidence bounds, and this struct has none.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub subject: Arm,
    pub comparator: Arm,
    /// Δp = p₃ − p₂.
    pub delta_p: Option<f64>,
    /// R_C = C₃ / C₂.
    pub cost_ratio: Option<f64>,
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

    let cost_ratio = match (subject.cost_per_accepted, comparator.cost_per_accepted) {
        (Some(c3), Some(c2)) if c2.as_micros() != 0 => {
            #[allow(clippy::cast_precision_loss)]
            {
                Some(c3.as_micros() as f64 / c2.as_micros() as f64)
            }
        }
        _ => None,
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
