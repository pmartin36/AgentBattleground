//! Deterministic starting-attack construction: turns a `StartingArchetype`
//! plus allocated `Stats` into a single `Ability`, keyed on the archetype's
//! stat so a higher keyed stat lands higher in the archetype's declared
//! amount band.

use crate::ability::{Ability, AbilityType, DamageClass, Element, StatusKind};
use crate::construction::allocate::{StartingArchetype, STAT_CAP, STAT_FLOOR};
use crate::stats::Stats;

/// Damage band for `Melee` (keyed on strength). Tunable.
pub const MELEE_DAMAGE_RANGE: (u32, u32) = (18, 34);

/// Damage band for `Ranged` (keyed on the larger of dexterity/intelligence).
/// Tunable.
pub const RANGED_DAMAGE_RANGE: (u32, u32) = (12, 26);

/// Magnitude band for `Buff` (keyed on intelligence). Tunable.
pub const BUFF_MAGNITUDE_RANGE: (u32, u32) = (4, 12);

/// Magnitude band for `Debuff` (keyed on intelligence). Tunable.
pub const DEBUFF_MAGNITUDE_RANGE: (u32, u32) = (4, 12);

/// `Melee`'s attack range. Tunable.
pub const MELEE_RANGE: u8 = 1;
/// `Ranged`'s attack range. Tunable.
pub const RANGED_RANGE: u8 = 2;
/// `Debuff`'s range. Tunable.
pub const DEBUFF_RANGE: u8 = 2;

/// `Melee`'s cost. Tunable.
pub const MELEE_COST: u8 = 3;
/// `Ranged`'s cost. Tunable.
pub const RANGED_COST: u8 = 2;
/// `Debuff`'s cost. Tunable.
pub const DEBUFF_COST: u8 = 2;
/// `Buff`'s cost. Tunable.
pub const BUFF_COST: u8 = 2;

/// Every band must be well-formed (`lo <= hi`, `lo >= 1`) and the keyed-stat
/// scale (`allocate::STAT_FLOOR`/`STAT_CAP`) must have positive span, or a
/// retune fails to compile rather than silently dividing by zero.
const _: () = assert!(
    MELEE_DAMAGE_RANGE.0 <= MELEE_DAMAGE_RANGE.1
        && MELEE_DAMAGE_RANGE.0 >= 1
        && RANGED_DAMAGE_RANGE.0 <= RANGED_DAMAGE_RANGE.1
        && RANGED_DAMAGE_RANGE.0 >= 1
        && BUFF_MAGNITUDE_RANGE.0 <= BUFF_MAGNITUDE_RANGE.1
        && BUFF_MAGNITUDE_RANGE.0 >= 1
        && DEBUFF_MAGNITUDE_RANGE.0 <= DEBUFF_MAGNITUDE_RANGE.1
        && DEBUFF_MAGNITUDE_RANGE.0 >= 1
        && STAT_CAP > STAT_FLOOR
);

/// Maps `keyed` (a stat on the `STAT_FLOOR..=STAT_CAP` scale) into `band`,
/// monotonically non-decreasing and bounded within `band`. `keyed` below
/// `STAT_FLOOR` clamps to `band`'s low end; above `STAT_CAP` clamps to its
/// high end.
fn map_amount(keyed: u32, band: (u32, u32)) -> u32 {
    let (lo, hi) = band;
    let span = STAT_CAP - STAT_FLOOR;
    let pos = keyed.clamp(STAT_FLOOR, STAT_CAP) - STAT_FLOOR;
    lo + ((hi - lo) * pos) / span
}

/// The negative `StatusKind` a `Debuff` attaches for `element` — `None` for
/// `Normal`, which has no negative affinity. Exhaustive: a future `Element`
/// variant fails to compile here until handled.
fn negative_status_for(element: Element) -> Option<StatusKind> {
    match element {
        Element::Fire => Some(StatusKind::Burn),
        Element::Ice => Some(StatusKind::Frozen),
        Element::Lightning => Some(StatusKind::Shocked),
        Element::Earth => Some(StatusKind::Rooted),
        Element::Normal => None,
    }
}

/// Which stat keys `Ranged`'s damage and the `DamageClass` that follows from
/// it: dexterity keys `Physical`, intelligence keys `Magic`. On an exact tie
/// `seed` breaks it (`seed % 2 == 0` favors dexterity/Physical) — the keyed
/// value is identical either way, so this only decides the class.
fn ranged_keyed(stats: &Stats, seed: u64) -> (u32, DamageClass) {
    if stats.dexterity > stats.intelligence {
        (stats.dexterity, DamageClass::Physical)
    } else if stats.intelligence > stats.dexterity {
        (stats.intelligence, DamageClass::Magic)
    } else if seed.is_multiple_of(2) {
        (stats.dexterity, DamageClass::Physical)
    } else {
        (stats.intelligence, DamageClass::Magic)
    }
}

/// Builds the single starting `Ability` for `archetype`, deriving its
/// damage/magnitude from the archetype's keyed stat (mapped monotonically
/// into the archetype's declared band), and setting `element` to the passed
/// egg element on every archetype. Deterministic: the same
/// `(archetype, stats, element, seed)` always yields an equal `Ability`.
pub fn build_starting_attack(
    archetype: StartingArchetype,
    stats: &Stats,
    element: Element,
    seed: u64,
) -> Ability {
    match archetype {
        StartingArchetype::Melee => {
            let amount = map_amount(stats.strength, MELEE_DAMAGE_RANGE);
            Ability::new(format!("{} melee strike", element.label()), Vec::new())
                .with_ability_type(AbilityType::Attack)
                .with_element(element)
                .with_class(DamageClass::Physical)
                .with_cost(MELEE_COST)
                .with_damage(amount)
                .with_range(MELEE_RANGE)
        }
        StartingArchetype::Ranged => {
            let (keyed, class) = ranged_keyed(stats, seed);
            let amount = map_amount(keyed, RANGED_DAMAGE_RANGE);
            Ability::new(format!("{} ranged strike", element.label()), Vec::new())
                .with_ability_type(AbilityType::Attack)
                .with_element(element)
                .with_class(class)
                .with_cost(RANGED_COST)
                .with_damage(amount)
                .with_range(RANGED_RANGE)
        }
        StartingArchetype::Debuff => {
            let amount = map_amount(stats.intelligence, DEBUFF_MAGNITUDE_RANGE);
            let status_effects = negative_status_for(element).into_iter().collect();
            Ability::new(format!("{} debuff", element.label()), Vec::new())
                .with_ability_type(AbilityType::Debuff)
                .with_element(element)
                .with_class(DamageClass::Magic)
                .with_cost(DEBUFF_COST)
                .with_damage(amount)
                .with_range(DEBUFF_RANGE)
                .with_status_effects(status_effects)
        }
        StartingArchetype::Buff => {
            let amount = map_amount(stats.intelligence, BUFF_MAGNITUDE_RANGE);
            Ability::new(format!("{} buff", element.label()), Vec::new())
                .with_ability_type(AbilityType::Buff)
                .with_element(element)
                .with_cost(BUFF_COST)
                .with_damage(amount)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn even_stats(value: u32) -> Stats {
        Stats { strength: value, dexterity: value, intelligence: value, vitality: value }
    }

    /// `element()` returns the passed `Element` for every archetype,
    /// including `Buff` (which has no damage class of its own).
    #[test]
    fn element_is_passed_element_for_every_archetype() {
        let stats = even_stats(20);
        for archetype in StartingArchetype::ALL {
            let ability = build_starting_attack(archetype, &stats, Element::Fire, 1);
            assert_eq!(
                ability.element(),
                Some(Element::Fire),
                "{archetype:?} did not set the passed element"
            );
        }
    }

    /// The derived amount, read via `damage()`, always lands inside the
    /// archetype's declared band, across the floor/mid/cap of the keyed
    /// stat's scale.
    #[test]
    fn amount_within_band_for_every_archetype() {
        let cases = [
            (StartingArchetype::Melee, MELEE_DAMAGE_RANGE),
            (StartingArchetype::Ranged, RANGED_DAMAGE_RANGE),
            (StartingArchetype::Buff, BUFF_MAGNITUDE_RANGE),
            (StartingArchetype::Debuff, DEBUFF_MAGNITUDE_RANGE),
        ];
        for (archetype, (lo, hi)) in cases {
            for keyed in [STAT_FLOOR, (STAT_FLOOR + STAT_CAP) / 2, STAT_CAP] {
                let stats = even_stats(keyed);
                let ability = build_starting_attack(archetype, &stats, Element::Normal, 1);
                let amount = ability
                    .damage()
                    .unwrap_or_else(|| panic!("{archetype:?} produced no damage/magnitude"));
                assert!(
                    (lo..=hi).contains(&amount),
                    "{archetype:?} amount {amount} outside band [{lo}, {hi}]"
                );
            }
        }
    }

    /// `Ranged` gets a range greater than 1; `Melee` gets exactly 1.
    #[test]
    fn ranged_range_gt_1_and_melee_range_eq_1() {
        let stats = even_stats(20);

        let melee = build_starting_attack(StartingArchetype::Melee, &stats, Element::Normal, 1);
        assert_eq!(melee.range(), Some(1));

        let ranged = build_starting_attack(StartingArchetype::Ranged, &stats, Element::Normal, 1);
        assert!(
            ranged.range().unwrap_or(0) > 1,
            "ranged range should exceed 1, got {:?}",
            ranged.range()
        );
    }

    /// `ability_type()` follows the archetype: Attack for Ranged/Melee, Buff
    /// for Buff, Debuff for Debuff.
    #[test]
    fn ability_type_matches_archetype() {
        let stats = even_stats(20);
        let expected = [
            (StartingArchetype::Ranged, AbilityType::Attack),
            (StartingArchetype::Melee, AbilityType::Attack),
            (StartingArchetype::Buff, AbilityType::Buff),
            (StartingArchetype::Debuff, AbilityType::Debuff),
        ];
        for (archetype, ability_type) in expected {
            let ability = build_starting_attack(archetype, &stats, Element::Normal, 1);
            assert_eq!(
                ability.ability_type(),
                Some(ability_type),
                "{archetype:?} got the wrong ability_type"
            );
        }
    }

    /// A strictly higher strength yields a Melee damage `>=` a lower one,
    /// all else equal.
    #[test]
    fn higher_strength_increases_melee_damage() {
        let low = even_stats(STAT_FLOOR);
        let high = Stats { strength: STAT_CAP, ..low };
        let low_ability = build_starting_attack(StartingArchetype::Melee, &low, Element::Normal, 1);
        let high_ability =
            build_starting_attack(StartingArchetype::Melee, &high, Element::Normal, 1);
        assert!(
            high_ability.damage().unwrap() >= low_ability.damage().unwrap(),
            "higher strength should not yield less Melee damage"
        );
    }

    /// A strictly higher keyed stat (dexterity, kept the larger of
    /// dexterity/intelligence) yields a Ranged damage `>=` a lower one.
    #[test]
    fn higher_keyed_stat_increases_ranged_damage() {
        let low = even_stats(STAT_FLOOR);
        let high = Stats { dexterity: STAT_CAP, ..low };
        let low_ability =
            build_starting_attack(StartingArchetype::Ranged, &low, Element::Normal, 1);
        let high_ability =
            build_starting_attack(StartingArchetype::Ranged, &high, Element::Normal, 1);
        assert!(
            high_ability.damage().unwrap() >= low_ability.damage().unwrap(),
            "higher keyed stat should not yield less Ranged damage"
        );
    }

    /// A strictly higher intelligence yields a Buff magnitude `>=` a lower
    /// one.
    #[test]
    fn higher_intelligence_increases_buff_magnitude() {
        let low = even_stats(STAT_FLOOR);
        let high = Stats { intelligence: STAT_CAP, ..low };
        let low_ability = build_starting_attack(StartingArchetype::Buff, &low, Element::Normal, 1);
        let high_ability =
            build_starting_attack(StartingArchetype::Buff, &high, Element::Normal, 1);
        assert!(
            high_ability.damage().unwrap() >= low_ability.damage().unwrap(),
            "higher intelligence should not yield less Buff magnitude"
        );
    }

    /// A strictly higher intelligence yields a Debuff magnitude `>=` a
    /// lower one.
    #[test]
    fn higher_intelligence_increases_debuff_magnitude() {
        let low = even_stats(STAT_FLOOR);
        let high = Stats { intelligence: STAT_CAP, ..low };
        let low_ability =
            build_starting_attack(StartingArchetype::Debuff, &low, Element::Normal, 1);
        let high_ability =
            build_starting_attack(StartingArchetype::Debuff, &high, Element::Normal, 1);
        assert!(
            high_ability.damage().unwrap() >= low_ability.damage().unwrap(),
            "higher intelligence should not yield less Debuff magnitude"
        );
    }

    /// The same `(archetype, stats, element, seed)` yields an equal
    /// `Ability` across two calls, for every archetype.
    #[test]
    fn reproducible_same_inputs() {
        let stats = Stats { strength: 15, dexterity: 25, intelligence: 10, vitality: 12 };
        for archetype in StartingArchetype::ALL {
            let first = build_starting_attack(archetype, &stats, Element::Ice, 99);
            let second = build_starting_attack(archetype, &stats, Element::Ice, 99);
            assert_eq!(first, second, "{archetype:?} was not reproducible for identical inputs");
        }
    }

    /// `Debuff` attaches the element's negative `StatusKind`: Fire->Burn,
    /// Ice->Frozen, Lightning->Shocked, Earth->Rooted, Normal->none.
    #[test]
    fn debuff_attaches_negative_status_by_element() {
        let stats = even_stats(20);
        let cases: [(Element, &[StatusKind]); 5] = [
            (Element::Fire, &[StatusKind::Burn]),
            (Element::Ice, &[StatusKind::Frozen]),
            (Element::Lightning, &[StatusKind::Shocked]),
            (Element::Earth, &[StatusKind::Rooted]),
            (Element::Normal, &[]),
        ];
        for (element, expected) in cases {
            let ability = build_starting_attack(StartingArchetype::Debuff, &stats, element, 1);
            assert_eq!(
                ability.status_effects(),
                expected,
                "Debuff with {element:?} attached the wrong status_effects"
            );
        }
    }

    /// `Ranged`'s damage class follows whichever of dexterity/intelligence
    /// is the larger, keyed stat: dexterity-keyed is Physical,
    /// intelligence-keyed is Magic.
    #[test]
    fn ranged_class_splits_on_keyed_stat() {
        let dex_keyed = Stats { strength: 10, dexterity: 30, intelligence: 10, vitality: 10 };
        let int_keyed = Stats { strength: 10, dexterity: 10, intelligence: 30, vitality: 10 };

        let dex_ability =
            build_starting_attack(StartingArchetype::Ranged, &dex_keyed, Element::Normal, 1);
        assert_eq!(
            dex_ability.class(),
            Some(DamageClass::Physical),
            "dexterity-keyed Ranged should be Physical"
        );

        let int_ability =
            build_starting_attack(StartingArchetype::Ranged, &int_keyed, Element::Normal, 1);
        assert_eq!(
            int_ability.class(),
            Some(DamageClass::Magic),
            "intelligence-keyed Ranged should be Magic"
        );
    }
}
