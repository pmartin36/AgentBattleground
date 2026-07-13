//! `Modifier` + `StatRequirement`: the structural mechanism for gating a
//! modifier on a stat threshold (e.g. "requires strength >= 30"). No concrete
//! modifier catalog lives here — shape + gating logic only.
//!
//! `is_met`/`is_available` delegate to `Stats::value`, never re-matching on
//! `StatKind` themselves, so `StatKind` stays the single source of truth.

use crate::stats::{StatKind, Stats};

/// A stat threshold a `Modifier` may require to unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatRequirement {
    pub stat: StatKind,
    pub threshold: u32,
}

impl StatRequirement {
    /// True when `stats.value(self.stat) >= self.threshold` (>=, not >) —
    /// exactly-equal-to-threshold counts as met.
    pub fn is_met(&self, stats: &Stats) -> bool {
        stats.value(self.stat) >= self.threshold
    }
}

/// A single modifier: a name plus an optional gating `StatRequirement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modifier {
    pub name: String,
    pub requires: Option<StatRequirement>,
}

impl Modifier {
    /// True unconditionally when `requires` is `None`; otherwise delegates
    /// to the requirement's `is_met`.
    pub fn is_available(&self, stats: &Stats) -> bool {
        self.requires.as_ref().is_none_or(|r| r.is_met(stats))
    }
}

/// Max modifier tags an `Ability` may carry (spec `Decisions (v1)`).
pub const MAX_MODIFIERS: usize = 4;

/// A single ability: a description plus up to `MAX_MODIFIERS` modifier tags.
/// Fields are private — the `modifiers.len() <= MAX_MODIFIERS` invariant is
/// only guaranteed via `Ability::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ability {
    description: String,
    modifiers: Vec<Modifier>,
    ability_type: Option<AbilityType>,
    element: Option<Element>,
    class: Option<DamageClass>,
    cost: Option<u8>,
    damage: Option<u32>,
    range: Option<u8>,
    status_effects: Vec<StatusKind>,
    flavor: Option<String>,
}

impl Ability {
    /// Debug-asserts `modifiers.len() <= MAX_MODIFIERS`. Matches the
    /// `impl Into<String>` name/desc pattern of `Creature::new`.
    pub fn new(description: impl Into<String>, modifiers: Vec<Modifier>) -> Self {
        debug_assert!(
            modifiers.len() <= MAX_MODIFIERS,
            "Ability may hold at most {MAX_MODIFIERS} modifiers, got {}",
            modifiers.len()
        );
        Self {
            description: description.into(),
            modifiers,
            ability_type: None,
            element: None,
            class: None,
            cost: None,
            damage: None,
            range: None,
            status_effects: Vec::new(),
            flavor: None,
        }
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    pub fn with_ability_type(mut self, ability_type: AbilityType) -> Self {
        self.ability_type = Some(ability_type);
        self
    }

    pub fn with_element(mut self, element: Element) -> Self {
        self.element = Some(element);
        self
    }

    pub fn with_class(mut self, class: DamageClass) -> Self {
        self.class = Some(class);
        self
    }

    pub fn with_cost(mut self, cost: u8) -> Self {
        self.cost = Some(cost);
        self
    }

    pub fn with_damage(mut self, damage: u32) -> Self {
        self.damage = Some(damage);
        self
    }

    pub fn with_range(mut self, range: u8) -> Self {
        self.range = Some(range);
        self
    }

    pub fn with_status_effects(mut self, status_effects: Vec<StatusKind>) -> Self {
        self.status_effects = status_effects;
        self
    }

    pub fn with_flavor(mut self, flavor: impl Into<String>) -> Self {
        self.flavor = Some(flavor.into());
        self
    }

    pub fn ability_type(&self) -> Option<AbilityType> {
        self.ability_type
    }

    pub fn element(&self) -> Option<Element> {
        self.element
    }

    pub fn class(&self) -> Option<DamageClass> {
        self.class
    }

    pub fn cost(&self) -> Option<u8> {
        self.cost
    }

    pub fn damage(&self) -> Option<u32> {
        self.damage
    }

    pub fn range(&self) -> Option<u8> {
        self.range
    }

    pub fn status_effects(&self) -> &[StatusKind] {
        &self.status_effects
    }

    pub fn flavor(&self) -> Option<&str> {
        self.flavor.as_deref()
    }
}

/// The elemental affinity of an ability. `label()` is the single source of
/// display text — callers must never re-match on the variant themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Normal,
    Fire,
    Ice,
    Earth,
    Lightning,
}

impl Element {
    pub fn label(&self) -> &'static str {
        match self {
            Element::Normal => "Normal",
            Element::Fire => "Fire",
            Element::Ice => "Ice",
            Element::Earth => "Earth",
            Element::Lightning => "Lightning",
        }
    }
}

/// Whether an ability attacks, buffs, or debuffs. `label()` is the single
/// source of display text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityType {
    Attack,
    Buff,
    Debuff,
}

impl AbilityType {
    pub fn label(&self) -> &'static str {
        match self {
            AbilityType::Attack => "Attack",
            AbilityType::Buff => "Buff",
            AbilityType::Debuff => "Debuff",
        }
    }
}

/// Physical vs. magic damage. `label()` is the single source of display
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageClass {
    Physical,
    Magic,
}

impl DamageClass {
    pub fn label(&self) -> &'static str {
        match self {
            DamageClass::Physical => "Physical",
            DamageClass::Magic => "Magic",
        }
    }
}

/// A combat status kind an ability may inflict. `label()` is the single
/// source of display text — callers must never re-match on the variant.
/// Kind only; the applied-with-duration instance is spec 10's (reserved
/// name `StatusEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Burn,
    Frozen,
    Shocked,
    Rooted,
}

impl StatusKind {
    pub fn label(&self) -> &'static str {
        match self {
            StatusKind::Burn => "Burn",
            StatusKind::Frozen => "Frozen",
            StatusKind::Shocked => "Shocked",
            StatusKind::Rooted => "Rooted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Stats {
        // Distinct per-field values so a requirement reading the wrong stat
        // can't accidentally pass.
        Stats { strength: 30, dexterity: 10, intelligence: 20, vitality: 5 }
    }

    #[test]
    fn is_met_false_when_value_below_threshold() {
        let req = StatRequirement { stat: StatKind::Dexterity, threshold: 30 };
        assert!(!req.is_met(&stats()));
    }

    #[test]
    fn is_met_true_when_value_exactly_at_threshold() {
        let req = StatRequirement { stat: StatKind::Strength, threshold: 30 };
        assert!(req.is_met(&stats()));
    }

    #[test]
    fn is_met_true_when_value_above_threshold() {
        let req = StatRequirement { stat: StatKind::Strength, threshold: 20 };
        assert!(req.is_met(&stats()));
    }

    #[test]
    fn is_met_reads_the_targeted_stat_not_another_field() {
        // Vitality is the lowest of the 4 distinct values (5); a requirement
        // on Vitality with threshold 10 must read `vitality` (5, unmet), not
        // silently pass by reading a different, larger field.
        let req = StatRequirement { stat: StatKind::Vitality, threshold: 10 };
        assert!(!req.is_met(&stats()));
    }

    #[test]
    fn modifier_available_when_requires_is_none() {
        let modifier = Modifier { name: "Free".to_string(), requires: None };
        // stats() satisfies nothing relevant; availability must still be true.
        let empty_stats = Stats { strength: 0, dexterity: 0, intelligence: 0, vitality: 0 };
        assert!(modifier.is_available(&empty_stats));
    }

    #[test]
    fn modifier_unavailable_when_requirement_unmet() {
        let modifier = Modifier {
            name: "Gated".to_string(),
            requires: Some(StatRequirement { stat: StatKind::Intelligence, threshold: 50 }),
        };
        assert!(!modifier.is_available(&stats()));
    }

    fn dummy_modifiers(n: usize) -> Vec<Modifier> {
        (0..n).map(|i| Modifier { name: format!("Modifier {i}"), requires: None }).collect()
    }

    #[test]
    fn new_with_max_modifiers_succeeds() {
        let ability = Ability::new("Roar", dummy_modifiers(MAX_MODIFIERS));
        assert_eq!(ability.modifiers().len(), MAX_MODIFIERS);
    }

    #[test]
    fn new_with_zero_modifiers_succeeds() {
        let ability = Ability::new("Roar", dummy_modifiers(0));
        assert_eq!(ability.modifiers().len(), 0);
    }

    #[test]
    #[should_panic]
    fn new_with_more_than_max_modifiers_panics() {
        Ability::new("Roar", dummy_modifiers(MAX_MODIFIERS + 1));
    }

    #[test]
    fn accessors_round_trip_description_and_modifiers() {
        let mods = dummy_modifiers(2);
        let ability = Ability::new("Roar", mods.clone());
        assert_eq!(ability.description(), "Roar");
        assert_eq!(ability.modifiers(), mods.as_slice());
    }

    #[test]
    fn element_label_returns_display_string() {
        assert_eq!(Element::Fire.label(), "Fire");
        assert_eq!(Element::Ice.label(), "Ice");
    }

    #[test]
    fn ability_type_label_returns_display_string() {
        assert_eq!(AbilityType::Attack.label(), "Attack");
    }

    #[test]
    fn damage_class_label_returns_display_string() {
        assert_eq!(DamageClass::Physical.label(), "Physical");
    }

    #[test]
    fn status_kind_label_returns_display_string() {
        assert_eq!(StatusKind::Burn.label(), "Burn");
        assert_eq!(StatusKind::Frozen.label(), "Frozen");
        assert_eq!(StatusKind::Shocked.label(), "Shocked");
        assert_eq!(StatusKind::Rooted.label(), "Rooted");
    }

    #[test]
    fn builder_round_trip_sets_every_new_field() {
        let ability = Ability::new("Roar", vec![])
            .with_ability_type(AbilityType::Debuff)
            .with_element(Element::Lightning)
            .with_class(DamageClass::Magic)
            .with_cost(3)
            .with_damage(42)
            .with_range(5)
            .with_status_effects(vec![StatusKind::Shocked])
            .with_flavor("A crackling roar.");

        assert_eq!(ability.ability_type(), Some(AbilityType::Debuff));
        assert_eq!(ability.element(), Some(Element::Lightning));
        assert_eq!(ability.class(), Some(DamageClass::Magic));
        assert_eq!(ability.cost(), Some(3));
        assert_eq!(ability.damage(), Some(42));
        assert_eq!(ability.range(), Some(5));
        assert_eq!(ability.status_effects(), &[StatusKind::Shocked]);
        assert_eq!(ability.flavor(), Some("A crackling roar."));
    }

    #[test]
    fn fresh_ability_new_leaves_new_fields_none_and_empty() {
        let ability = Ability::new("Roar", vec![]);

        assert_eq!(ability.ability_type(), None);
        assert_eq!(ability.element(), None);
        assert_eq!(ability.class(), None);
        assert_eq!(ability.cost(), None);
        assert_eq!(ability.damage(), None);
        assert_eq!(ability.range(), None);
        assert!(ability.status_effects().is_empty());
        assert_eq!(ability.flavor(), None);
    }
}
