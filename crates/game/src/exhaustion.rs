//! `Exhaustion`: percent 0..=100, clamped, plus pure additive transitions.
//! Reaching 100 enters the injured state (`injured_until` becomes `Some(_)`);
//! below 100 it stays `None`. No live combat trigger this round — exercised
//! only by unit tests (see spec `34-creature-attributes-data-model.md`).

use std::time::Duration;

/// Max exhaustion percent; reaching it enters the injured state.
pub const MAX_PERCENT: u8 = 100;

/// Non-canonical placeholder recovery duration (spec gives no concrete
/// number — see `34-creature-attributes-data-model.md` Open Questions).
/// 24h, chosen to echo the "one battle per day" pacing. Not balanced design.
pub const RECOVERY_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exhaustion {
    percent: u8,
    injured_until: Option<Duration>,
}

impl Exhaustion {
    pub fn percent(&self) -> u8 {
        self.percent
    }

    pub fn injured_until(&self) -> Option<Duration> {
        self.injured_until
    }

    pub fn is_injured(&self) -> bool {
        self.injured_until.is_some()
    }

    pub fn apply_damage_exhaustion(&self, amount: u8) -> Self {
        self.with_added(amount)
    }

    pub fn apply_ability_use_exhaustion(&self, cost: u8) -> Self {
        self.with_added(cost)
    }

    /// Single source of truth for the clamp + injured-entry logic shared by
    /// both public transitions.
    fn with_added(&self, amount: u8) -> Self {
        let percent = self.percent.saturating_add(amount).min(MAX_PERCENT);
        let injured_until = if percent >= MAX_PERCENT {
            Some(self.injured_until.unwrap_or(RECOVERY_DURATION))
        } else {
            None
        };
        Self {
            percent,
            injured_until,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_rested_and_not_injured() {
        let e = Exhaustion::default();
        assert_eq!(e.percent(), 0);
        assert_eq!(e.injured_until(), None);
        assert!(!e.is_injured());
    }

    #[test]
    fn damage_below_threshold_leaves_not_injured() {
        let e = Exhaustion::default().apply_damage_exhaustion(99);
        assert_eq!(e.percent(), 99);
        assert_eq!(e.injured_until(), None);
        assert!(!e.is_injured());
    }

    #[test]
    fn reaching_exactly_max_percent_becomes_injured() {
        let e = Exhaustion::default()
            .apply_damage_exhaustion(60)
            .apply_damage_exhaustion(40);
        assert_eq!(e.percent(), MAX_PERCENT);
        assert_eq!(e.injured_until(), Some(RECOVERY_DURATION));
        assert!(e.is_injured());
    }

    #[test]
    fn one_below_max_percent_stays_not_injured() {
        let e = Exhaustion::default()
            .apply_damage_exhaustion(60)
            .apply_damage_exhaustion(39);
        assert_eq!(e.percent(), 99);
        assert_eq!(e.injured_until(), None);
        assert!(!e.is_injured());
    }

    #[test]
    fn percent_clamps_at_max_and_never_exceeds_it() {
        let e = Exhaustion::default().apply_damage_exhaustion(250);
        assert_eq!(e.percent(), MAX_PERCENT);
        assert!(e.percent() <= MAX_PERCENT);
    }

    #[test]
    fn ability_use_exhaustion_is_a_distinct_working_entry_point() {
        let e = Exhaustion::default()
            .apply_ability_use_exhaustion(60)
            .apply_ability_use_exhaustion(40);
        assert_eq!(e.percent(), MAX_PERCENT);
        assert!(e.is_injured());
    }

    #[test]
    fn transitions_are_pure_original_unchanged() {
        let a = Exhaustion::default();
        let b = a.apply_damage_exhaustion(10);
        assert_eq!(a.percent(), 0);
        assert_eq!(b.percent(), 10);
    }

    #[test]
    fn already_injured_stays_injured_on_further_damage() {
        let e = Exhaustion::default()
            .apply_damage_exhaustion(100)
            .apply_damage_exhaustion(10);
        assert_eq!(e.percent(), MAX_PERCENT);
        assert!(e.is_injured());
    }
}
