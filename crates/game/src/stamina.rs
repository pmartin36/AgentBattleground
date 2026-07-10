//! `Stamina`: percent 0..=100 REMAINING, clamped, plus pure draining
//! transitions. Reaching 0 enters the injured state (`injured_until` becomes
//! `Some(_)`); above 0 it stays `None`. No live combat trigger this round —
//! exercised only by unit tests (see spec `34-creature-attributes-data-model.md`).

use std::time::Duration;

/// Max stamina percent — a fresh/rested creature's stamina.
pub const MAX_PERCENT: u8 = 100;

/// Non-canonical placeholder recovery duration (spec gives no concrete
/// number — see `34-creature-attributes-data-model.md` Open Questions).
/// 24h, chosen to echo the "one battle per day" pacing. Not balanced design.
pub const RECOVERY_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamina {
    percent: u8,
    injured_until: Option<Duration>,
}

impl Default for Stamina {
    /// A fresh `Stamina` is full (`MAX_PERCENT`) and not injured. Trivial,
    /// non-branching scaffolding (not the semantic-invert logic under test —
    /// see `with_drained`), implemented directly so every `Creature::new()`
    /// call elsewhere in the crate keeps compiling and passing.
    fn default() -> Self {
        Self { percent: MAX_PERCENT, injured_until: None }
    }
}

impl Stamina {
    pub fn percent(&self) -> u8 {
        self.percent
    }

    pub fn injured_until(&self) -> Option<Duration> {
        self.injured_until
    }

    pub fn is_injured(&self) -> bool {
        self.injured_until.is_some()
    }

    pub fn drain_from_damage(&self, amount: u8) -> Self {
        self.with_drained(amount)
    }

    pub fn drain_from_ability_use(&self, cost: u8) -> Self {
        self.with_drained(cost)
    }

    /// Single source of truth for the clamp + injured-entry logic shared by
    /// both public transitions. Subtracts (not adds), clamps at 0, and enters
    /// the injured state when `percent` reaches 0 (not `MAX_PERCENT`).
    fn with_drained(&self, amount: u8) -> Self {
        let percent = self.percent.saturating_sub(amount);
        let injured_until = if percent == 0 {
            Some(self.injured_until.unwrap_or(RECOVERY_DURATION))
        } else {
            None
        };
        Self { percent, injured_until }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_full_and_not_injured() {
        let s = Stamina::default();
        assert_eq!(s.percent(), MAX_PERCENT);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn drain_from_full_by_42_reads_58_and_not_injured() {
        let s = Stamina::default().drain_from_damage(42);
        assert_eq!(s.percent(), 58);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn draining_to_zero_becomes_injured() {
        let s = Stamina::default().drain_from_damage(60).drain_from_damage(40);
        assert_eq!(s.percent(), 0);
        assert_eq!(s.injured_until(), Some(RECOVERY_DURATION));
        assert!(s.is_injured());
    }

    #[test]
    fn one_above_zero_stays_not_injured() {
        let s = Stamina::default().drain_from_damage(60).drain_from_damage(39);
        assert_eq!(s.percent(), 1);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn over_drain_saturates_at_zero_and_injured() {
        let s = Stamina::default().drain_from_damage(250);
        assert_eq!(s.percent(), 0);
        assert!(s.is_injured());
    }

    #[test]
    fn ability_use_drain_is_a_distinct_working_entry_point() {
        let s = Stamina::default()
            .drain_from_ability_use(60)
            .drain_from_ability_use(40);
        assert_eq!(s.percent(), 0);
        assert!(s.is_injured());
    }

    #[test]
    fn transitions_are_pure_original_unchanged() {
        let a = Stamina::default();
        let b = a.drain_from_damage(10);
        assert_eq!(a.percent(), MAX_PERCENT);
        assert_eq!(b.percent(), 90);
    }

    #[test]
    fn already_injured_stays_injured_on_further_drain() {
        let s = Stamina::default().drain_from_damage(100).drain_from_damage(10);
        assert_eq!(s.percent(), 0);
        assert!(s.is_injured());
    }
}
