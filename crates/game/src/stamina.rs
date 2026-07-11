//! `Stamina`: `current`/`max` stamina points with a derived `percent()`
//! (`current`/`max`, rounded). Reaching 0 `current` enters the injured state
//! (`injured_until` becomes `Some(_)`); above 0 it stays `None`. Draining
//! transitions are pure. No live combat trigger this round — exercised only
//! by unit tests (see spec `34-creature-attributes-data-model.md`).

use std::time::Duration;

/// The `percent()` ceiling.
pub const MAX_PERCENT: u8 = 100;

/// The `max` capacity that maps to a full-width stamina bar track (the
/// renderer's 100% reference) and the default `max` for `Stamina::default()`.
pub const STAMINA_MAX_CAP: u16 = 100;

/// Non-canonical placeholder recovery duration (spec gives no concrete
/// number — see `34-creature-attributes-data-model.md` Open Questions).
/// 24h, chosen to echo the "one battle per day" pacing. Not balanced design.
pub const RECOVERY_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamina {
    current: u16,
    max: u16,
    injured_until: Option<Duration>,
}

impl Default for Stamina {
    /// A fresh `Stamina` is full at `STAMINA_MAX_CAP` (`current == max`) and
    /// not injured.
    fn default() -> Self {
        Self::full(STAMINA_MAX_CAP)
    }
}

impl Stamina {
    /// Builds a `Stamina` at `current` out of `max` (`current` clamped to
    /// `max`). Injured when `current` is 0.
    pub fn new(current: u16, max: u16) -> Self {
        let current = current.min(max);
        let injured_until = if current == 0 { Some(RECOVERY_DURATION) } else { None };
        Self { current, max, injured_until }
    }

    /// A full creature at `max` capacity (`current == max`).
    pub fn full(max: u16) -> Self {
        Self::new(max, max)
    }

    pub fn current(&self) -> u16 {
        self.current
    }

    pub fn max(&self) -> u16 {
        self.max
    }

    /// Stamina remaining as a percent `0..=100`, derived from `current`/`max`
    /// (round half up). `max == 0` reads 0.
    pub fn percent(&self) -> u8 {
        if self.max == 0 {
            return 0;
        }
        let pct = (self.current as u32 * 100 + self.max as u32 / 2) / self.max as u32;
        pct.min(MAX_PERCENT as u32) as u8
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
    /// both public transitions. Subtracts `amount` from `current` (clamp at
    /// 0) and enters the injured state when `current` reaches 0, preserving
    /// any existing recovery timer.
    fn with_drained(&self, amount: u8) -> Self {
        let current = self.current.saturating_sub(amount as u16);
        let injured_until = if current == 0 {
            Some(self.injured_until.unwrap_or(RECOVERY_DURATION))
        } else {
            None
        };
        Self { current, max: self.max, injured_until }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_full_and_not_injured() {
        let s = Stamina::default();
        assert_eq!(s.percent(), MAX_PERCENT);
        assert_eq!(s.current(), STAMINA_MAX_CAP);
        assert_eq!(s.max(), STAMINA_MAX_CAP);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn full_is_100_percent_at_its_max() {
        let s = Stamina::full(60);
        assert_eq!(s.current(), 60);
        assert_eq!(s.max(), 60);
        assert_eq!(s.percent(), 100);
        assert!(!s.is_injured());
    }

    #[test]
    fn percent_is_derived_from_current_over_max() {
        assert_eq!(Stamina::new(30, 60).percent(), 50);
        assert_eq!(Stamina::new(48, 60).percent(), 80);
        assert_eq!(Stamina::new(57, 95).percent(), 60);
    }

    #[test]
    fn percent_rounds_half_up() {
        // 1/8 = 12.5% -> 13; 3/8 = 37.5% -> 38.
        assert_eq!(Stamina::new(1, 8).percent(), 13);
        assert_eq!(Stamina::new(3, 8).percent(), 38);
    }

    #[test]
    fn percent_with_zero_max_is_zero() {
        assert_eq!(Stamina::new(0, 0).percent(), 0);
    }

    #[test]
    fn new_clamps_current_to_max() {
        let s = Stamina::new(50, 30);
        assert_eq!(s.current(), 30);
        assert_eq!(s.max(), 30);
        assert_eq!(s.percent(), 100);
    }

    #[test]
    fn new_at_zero_current_is_injured() {
        let s = Stamina::new(0, 40);
        assert_eq!(s.percent(), 0);
        assert!(s.is_injured());
    }

    #[test]
    fn drain_from_full_by_42_reads_58_and_not_injured() {
        let s = Stamina::default().drain_from_damage(42);
        assert_eq!(s.percent(), 58);
        assert_eq!(s.current(), 58);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn draining_to_zero_becomes_injured() {
        let s = Stamina::default().drain_from_damage(60).drain_from_damage(40);
        assert_eq!(s.percent(), 0);
        assert_eq!(s.current(), 0);
        assert_eq!(s.injured_until(), Some(RECOVERY_DURATION));
        assert!(s.is_injured());
    }

    #[test]
    fn one_above_zero_stays_not_injured() {
        let s = Stamina::default().drain_from_damage(60).drain_from_damage(39);
        assert_eq!(s.current(), 1);
        assert_eq!(s.percent(), 1);
        assert_eq!(s.injured_until(), None);
        assert!(!s.is_injured());
    }

    #[test]
    fn over_drain_saturates_at_zero_and_injured() {
        let s = Stamina::default().drain_from_damage(250);
        assert_eq!(s.current(), 0);
        assert!(s.is_injured());
    }

    #[test]
    fn drain_subtracts_from_current_not_max() {
        let s = Stamina::new(50, 80).drain_from_damage(20);
        assert_eq!(s.current(), 30);
        assert_eq!(s.max(), 80);
    }

    #[test]
    fn ability_use_drain_is_a_distinct_working_entry_point() {
        let s = Stamina::default()
            .drain_from_ability_use(60)
            .drain_from_ability_use(40);
        assert_eq!(s.current(), 0);
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
        assert_eq!(s.current(), 0);
        assert!(s.is_injured());
    }
}
