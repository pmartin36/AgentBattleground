//! Deterministic stat allocation for constructed creatures: distributes the
//! fixed `STAT_BUDGET` across `StatKind::ALL` in proportion to a
//! `StatWeighting`, enforcing `STAT_FLOOR`/`STAT_CAP`, with byte-identical
//! output for the same `(weighting, seed)`.

use crate::ability::Element;
use crate::stats::{StatKind, Stats};

/// Total points distributed across all 4 stats by `allocate_stats`.
/// Tunable — existing hand-authored creatures total ~74-85.
pub const STAT_BUDGET: u32 = 80;

/// Minimum value any single stat can land at after allocation. Tunable.
pub const STAT_FLOOR: u32 = 8;

/// Maximum value any single stat can land at after allocation. Tunable.
pub const STAT_CAP: u32 = 40;

/// Feasibility relation the 3 constants above must satisfy for
/// `allocate_stats` to be able to reserve every stat's floor and still have
/// cap headroom to redistribute into. Fails to compile if a future retune
/// breaks it.
const _: () = assert!(
    4 * STAT_FLOOR <= STAT_BUDGET
        && STAT_CAP <= STAT_BUDGET - 3 * STAT_FLOOR
        && STAT_BUDGET - STAT_CAP >= 3 * STAT_FLOOR
        && STAT_FLOOR >= 1
);

/// Per-stat weight driving `allocate_stats`'s proportional split. Weights
/// are not required to be pre-normalized.
#[derive(Debug, Clone, PartialEq)]
pub struct StatWeighting {
    pub strength: f32,
    pub dexterity: f32,
    pub intelligence: f32,
    pub vitality: f32,
}

impl StatWeighting {
    /// An even split across all 4 stats.
    pub fn uniform() -> Self {
        Self { strength: 1.0, dexterity: 1.0, intelligence: 1.0, vitality: 1.0 }
    }

    /// The weight for `kind`, resolved through the single `kind -> field`
    /// mapping — mirrors `Stats::value` (stats.rs).
    pub fn weight(&self, kind: StatKind) -> f32 {
        match kind {
            StatKind::Strength => self.strength,
            StatKind::Dexterity => self.dexterity,
            StatKind::Intelligence => self.intelligence,
            StatKind::Vitality => self.vitality,
        }
    }
}

/// The starting combat role a constructed creature is built around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartingArchetype {
    Ranged,
    Melee,
    Debuff,
    Buff,
}

impl StartingArchetype {
    /// All variants, in a fixed order — for iterating without re-hardcoding
    /// the list a second time.
    pub const ALL: [StartingArchetype; 4] = [
        StartingArchetype::Ranged,
        StartingArchetype::Melee,
        StartingArchetype::Debuff,
        StartingArchetype::Buff,
    ];
}

/// The full request driving one creature's construction: identity, the stat
/// split to allocate, the starting archetype and element to build an
/// opening attack around, and the seed that makes the whole pipeline
/// reproducible.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionRequest {
    name: String,
    description: String,
    weighting: StatWeighting,
    archetype: StartingArchetype,
    element: Element,
    seed: u64,
}

impl ConstructionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        weighting: StatWeighting,
        archetype: StartingArchetype,
        element: Element,
        seed: u64,
    ) -> Self {
        Self { name: name.into(), description: description.into(), weighting, archetype, element, seed }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn weighting(&self) -> &StatWeighting {
        &self.weighting
    }

    pub fn archetype(&self) -> StartingArchetype {
        self.archetype
    }

    pub fn element(&self) -> Element {
        self.element
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }
}

/// Distributes `STAT_BUDGET` across the 4 stats in proportion to
/// `weighting`, enforcing `STAT_FLOOR` and `STAT_CAP`. Deterministic: the
/// same `(weighting, seed)` always yields byte-identical `Stats`.
pub fn allocate_stats(weighting: &StatWeighting, seed: u64) -> Stats {
    let raw_weights: Vec<f64> =
        StatKind::ALL.iter().map(|&kind| weighting.weight(kind).max(0.0) as f64).collect();
    let weight_sum: f64 = raw_weights.iter().sum();
    let weights: Vec<f64> =
        if weight_sum > 0.0 { raw_weights } else { vec![1.0; StatKind::ALL.len()] };

    let reserve = STAT_FLOOR * StatKind::ALL.len() as u32;
    let distributable = STAT_BUDGET - reserve;

    let base = apportion(distributable, &weights, seed);
    let mut alloc: Vec<u32> = base.iter().map(|&b| STAT_FLOOR + b).collect();

    loop {
        let excess: u32 = alloc.iter().map(|&a| a.saturating_sub(STAT_CAP)).sum();
        if excess == 0 {
            break;
        }
        for a in alloc.iter_mut() {
            *a = (*a).min(STAT_CAP);
        }
        let headroom: Vec<f64> = alloc.iter().map(|&a| (STAT_CAP - a) as f64).collect();
        if headroom.iter().sum::<f64>() <= 0.0 {
            break;
        }
        let redistributed = apportion(excess, &headroom, seed);
        for (a, add) in alloc.iter_mut().zip(redistributed.iter()) {
            *a += add;
        }
    }

    let mut stats = Stats::default();
    for (kind, value) in StatKind::ALL.into_iter().zip(alloc) {
        match kind {
            StatKind::Strength => stats.strength = value,
            StatKind::Dexterity => stats.dexterity = value,
            StatKind::Intelligence => stats.intelligence = value,
            StatKind::Vitality => stats.vitality = value,
        }
    }
    stats
}

/// Largest-remainder proportional split of `total` across `weights`,
/// summing EXACTLY to `total` (never over/under by rounding drift), every
/// entry `>= 0`. Ties in the fractional remainder are broken by rotating
/// through `seed` (`(index + seed) % len`, ascending) so the same seed
/// always resolves ties the same way — no `rand` dependency needed for
/// reproducibility. Returns all zeros if `weights` is empty, `total == 0`,
/// or every weight is `<= 0`.
fn apportion(total: u32, weights: &[f64], seed: u64) -> Vec<u32> {
    let n = weights.len();
    if n == 0 || total == 0 {
        return vec![0; n];
    }
    let sum: f64 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 {
        return vec![0; n];
    }

    let total_f = total as f64;
    let raw: Vec<f64> = weights.iter().map(|w| total_f * w.max(0.0) / sum).collect();
    let mut out: Vec<u32> = raw.iter().map(|r| r.floor() as u32).collect();
    let floor_sum: u32 = out.iter().sum();
    let mut remainder = total.saturating_sub(floor_sum);

    let rotation = (seed as usize) % n;
    let mut fracs: Vec<(usize, f64)> =
        raw.iter().zip(&out).enumerate().map(|(i, (r, &f))| (i, r - f as f64)).collect();
    fracs.sort_by(|a, b| {
        b.1.total_cmp(&a.1).then_with(|| ((a.0 + rotation) % n).cmp(&((b.0 + rotation) % n)))
    });

    let mut idx = 0;
    while remainder > 0 && idx < fracs.len() {
        out[fracs[idx].0] += 1;
        remainder -= 1;
        idx += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spread of weighting shapes: even, lopsided, one stat near-zero,
    /// and fully one-sided (all budget on a single stat).
    fn weighting_spread() -> Vec<StatWeighting> {
        vec![
            StatWeighting { strength: 1.0, dexterity: 1.0, intelligence: 1.0, vitality: 1.0 },
            StatWeighting { strength: 10.0, dexterity: 1.0, intelligence: 1.0, vitality: 1.0 },
            StatWeighting { strength: 1.0, dexterity: 1.0, intelligence: 1.0, vitality: 0.0001 },
            StatWeighting { strength: 1.0, dexterity: 0.0, intelligence: 0.0, vitality: 0.0 },
        ]
    }

    /// For any weighting shape, the four allocated stats sum to exactly
    /// `STAT_BUDGET` — rounding must reconcile to the whole budget, never
    /// leave a remainder or overshoot.
    #[test]
    fn sum_equals_budget_over_weighting_spread() {
        for w in weighting_spread() {
            let stats = allocate_stats(&w, 7);
            let sum = stats.strength + stats.dexterity + stats.intelligence + stats.vitality;
            assert_eq!(sum, STAT_BUDGET, "weighting {w:?} summed to {sum}, not STAT_BUDGET");
        }
    }

    /// No stat ever lands below `STAT_FLOOR`, even when its weight is zero
    /// or near-zero.
    #[test]
    fn every_stat_at_least_floor_over_weighting_spread() {
        for w in weighting_spread() {
            let stats = allocate_stats(&w, 7);
            for kind in StatKind::ALL {
                assert!(
                    stats.value(kind) >= STAT_FLOOR,
                    "{kind:?} = {} fell below STAT_FLOOR for weighting {w:?}",
                    stats.value(kind)
                );
            }
        }
    }

    /// No stat ever exceeds `STAT_CAP`, even when a weighting tries to dump
    /// the whole budget onto one stat.
    #[test]
    fn no_stat_exceeds_cap_over_weighting_spread() {
        for w in weighting_spread() {
            let stats = allocate_stats(&w, 7);
            for kind in StatKind::ALL {
                assert!(
                    stats.value(kind) <= STAT_CAP,
                    "{kind:?} = {} exceeded STAT_CAP for weighting {w:?}",
                    stats.value(kind)
                );
            }
        }
    }

    /// A fully one-sided weighting clamps the weighted stat at exactly
    /// `STAT_CAP`, while the other three still meet `STAT_FLOOR`.
    #[test]
    fn fully_one_sided_clamps_weighted_stat_and_floors_the_rest() {
        let w = StatWeighting { strength: 1.0, dexterity: 0.0, intelligence: 0.0, vitality: 0.0 };
        let stats = allocate_stats(&w, 7);
        assert_eq!(stats.strength, STAT_CAP, "fully-weighted stat should clamp to STAT_CAP");
        assert!(stats.dexterity >= STAT_FLOOR);
        assert!(stats.intelligence >= STAT_FLOOR);
        assert!(stats.vitality >= STAT_FLOOR);
    }

    /// The same `(weighting, seed)` pair always produces byte-identical
    /// `Stats` — the reproducibility guarantee `construct_creature`
    /// depends on downstream.
    #[test]
    fn same_weighting_and_seed_reproduces_identical_stats() {
        let w = StatWeighting { strength: 3.0, dexterity: 1.0, intelligence: 5.0, vitality: 2.0 };
        let first = allocate_stats(&w, 42);
        let second = allocate_stats(&w, 42);
        assert_eq!(first, second, "same (weighting, seed) must yield byte-identical Stats");
    }

    /// A stat with a strictly higher weight lands at or above one with a
    /// strictly lower weight — the weighting must actually shape the split,
    /// not just produce an even distribution.
    #[test]
    fn higher_weight_yields_at_least_as_much_as_lower_weight() {
        let w = StatWeighting { strength: 5.0, dexterity: 1.0, intelligence: 1.0, vitality: 1.0 };
        let stats = allocate_stats(&w, 7);
        assert!(
            stats.strength >= stats.dexterity,
            "higher-weighted strength ({}) should be >= lower-weighted dexterity ({})",
            stats.strength,
            stats.dexterity
        );
    }
}
