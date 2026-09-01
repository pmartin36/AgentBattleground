//! On-disk persisted schema for a player's roster and eggs — the payload
//! `player_data::store` will serialize via bincode.
//!
//! Every enum embedded here is on an append-only contract: bincode encodes
//! a variant by its declared position, not its name, so inserting or
//! reordering a variant silently misdecodes every existing save. New
//! variants may only ever be appended at the end.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::ability::{Ability, Element};
use crate::asset_gen::types::{ClipAsset, ImageAsset};
use crate::creatures::MAX_ABILITIES;
use crate::stamina::Stamina;
use crate::stats::Stats;

/// A player's full save payload. `roster`'s Vec order IS squad position —
/// there is no separate stored position field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerData {
    pub roster: Vec<PersistedCreature>,
    pub eggs: Vec<Egg>,
}

impl PlayerData {
    /// Whether the roster has a free slot for a newly hatched creature.
    pub fn roster_has_open_slot(&self) -> bool {
        self.roster.len() < crate::squad_role::ROSTER_SIZE
    }

    /// Appends `creature` to the end of the roster, preserving the existing
    /// members' order.
    pub fn push_roster(&mut self, creature: PersistedCreature) {
        self.roster.push(creature);
    }

    /// Replaces the roster member at `index` with `incoming`, returning the
    /// displaced creature so the caller decides its fate. Out-of-range
    /// `index` is a no-op returning `None`.
    pub fn replace_roster_slot(&mut self, index: usize, incoming: PersistedCreature) -> Option<PersistedCreature> {
        let slot = self.roster.get_mut(index)?;
        Some(std::mem::replace(slot, incoming))
    }
}

/// A creature's on-disk form: RPG data plus optional art handles. Handles
/// are `None` for a bundled/handle-less creature (e.g. the first-run seed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCreature {
    pub name: String,
    pub element: Element,
    pub stats: Stats,
    pub level: u32,
    pub xp: u32,
    pub abilities: Vec<Ability>,
    pub stamina: Stamina,
    pub still: Option<ImageAsset>,
    pub idle: Option<ClipAsset>,
    pub attack: Option<ClipAsset>,
}

impl PersistedCreature {
    /// Debug-asserts `abilities.len() <= MAX_ABILITIES`, mirroring
    /// `Creature::with_abilities`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        element: Element,
        stats: Stats,
        level: u32,
        xp: u32,
        abilities: Vec<Ability>,
        stamina: Stamina,
        still: Option<ImageAsset>,
        idle: Option<ClipAsset>,
        attack: Option<ClipAsset>,
    ) -> Self {
        debug_assert!(
            abilities.len() <= MAX_ABILITIES,
            "PersistedCreature may hold at most {MAX_ABILITIES} abilities, got {}",
            abilities.len()
        );
        Self {
            name: name.into(),
            element,
            stats,
            level,
            xp,
            abilities,
            stamina,
            still,
            idle,
            attack,
        }
    }
}

/// An egg's incubation state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EggState {
    Undefined,
    Incubating { started_at: SystemTime },
    Ready,
}

/// An unhatched (or freshly-hatched) egg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Egg {
    pub element: Element,
    pub state: EggState,
    pub mad_lib: Option<String>,
    pub egg_art: Option<ImageAsset>,
    pub hatchling: Option<PersistedCreature>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::{AbilityType, DamageClass, Modifier, StatRequirement, StatusKind};
    use crate::creatures::MAX_ABILITIES;
    use crate::squad_role::ROSTER_SIZE;
    use crate::stats::StatKind;
    use std::path::PathBuf;
    use std::time::Duration;

    fn sample_ability() -> Ability {
        Ability::new(
            "Ember Claw",
            vec![Modifier {
                name: "Overheat".to_string(),
                requires: Some(StatRequirement { stat: StatKind::Intelligence, threshold: 15 }),
            }],
        )
        .with_ability_type(AbilityType::Attack)
        .with_element(Element::Fire)
        .with_class(DamageClass::Magic)
        .with_cost(4)
        .with_damage(22)
        .with_range(2)
        .with_status_effects(vec![StatusKind::Burn])
        .with_flavor("Scorches the target.")
    }

    fn sample_persisted_creature_with_handles() -> PersistedCreature {
        PersistedCreature::new(
            "Emberling",
            Element::Fire,
            Stats { strength: 12, dexterity: 9, intelligence: 18, vitality: 14 },
            5,
            340,
            vec![sample_ability()],
            Stamina::new(40, 60),
            Some(ImageAsset { path: PathBuf::from("emberling/still.png") }),
            Some(ClipAsset {
                frames: vec![
                    PathBuf::from("emberling/idle/0.png"),
                    PathBuf::from("emberling/idle/1.png"),
                ],
            }),
            Some(ClipAsset {
                frames: vec![
                    PathBuf::from("emberling/attack/0.png"),
                    PathBuf::from("emberling/attack/1.png"),
                ],
            }),
        )
    }

    fn sample_persisted_creature_handle_less() -> PersistedCreature {
        PersistedCreature::new(
            "Bundled Sprout",
            Element::Normal,
            Stats::default(),
            1,
            0,
            vec![],
            Stamina::default(),
            None,
            None,
            None,
        )
    }

    /// A fully-populated `PlayerData` (nested `Modifier`/`StatRequirement`,
    /// non-default `Stats`/`Stamina`, all art handles `Some`, and one egg
    /// per `EggState` variant with `started_at` set) survives a bincode
    /// round trip byte-for-structural-equality.
    #[test]
    fn player_data_round_trip_preserves_populated_creature_and_all_egg_states() {
        let creature = sample_persisted_creature_with_handles();
        let data = PlayerData {
            roster: vec![creature.clone()],
            eggs: vec![
                Egg {
                    element: Element::Ice,
                    state: EggState::Undefined,
                    mad_lib: Some("a shivering ???".to_string()),
                    egg_art: Some(ImageAsset { path: PathBuf::from("eggs/ice.png") }),
                    hatchling: None,
                },
                Egg {
                    element: Element::Earth,
                    state: EggState::Incubating {
                        started_at: std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
                    },
                    mad_lib: None,
                    egg_art: None,
                    hatchling: Some(creature.clone()),
                },
                Egg {
                    element: Element::Lightning,
                    state: EggState::Ready,
                    mad_lib: Some("a crackling ???".to_string()),
                    egg_art: Some(ImageAsset { path: PathBuf::from("eggs/lightning.png") }),
                    hatchling: Some(creature),
                },
            ],
        };

        let bytes = bincode::serialize(&data).expect("serialize");
        let decoded: PlayerData = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded, data);
    }

    /// A `PlayerData` whose creature has no art handles and whose egg has no
    /// hatchling (the first-run/seed shape) round trips without panicking.
    #[test]
    fn player_data_round_trip_tolerates_handle_less_creature_and_absent_hatchling() {
        let data = PlayerData {
            roster: vec![sample_persisted_creature_handle_less()],
            eggs: vec![Egg {
                element: Element::Normal,
                state: EggState::Undefined,
                mad_lib: None,
                egg_art: None,
                hatchling: None,
            }],
        };

        let bytes = bincode::serialize(&data).expect("serialize");
        let decoded: PlayerData = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded, data);
    }

    /// `PersistedCreature::new` panics when handed more than `MAX_ABILITIES`
    /// abilities.
    #[test]
    #[should_panic(expected = "at most 4 abilities")]
    fn persisted_creature_new_panics_over_max_abilities() {
        let too_many = vec![sample_ability(); MAX_ABILITIES + 1];
        PersistedCreature::new(
            "Overloaded",
            Element::Normal,
            Stats::default(),
            1,
            0,
            too_many,
            Stamina::default(),
            None,
            None,
            None,
        );
    }

    /// A roster member distinguishable only by name, for the roster-op
    /// tests below.
    fn roster_member(name: &str) -> PersistedCreature {
        PersistedCreature::new(
            name,
            Element::Normal,
            Stats::default(),
            1,
            0,
            Vec::new(),
            Stamina::default(),
            None,
            None,
            None,
        )
    }

    /// `push_roster` appends at the end, preserving the existing members'
    /// order.
    #[test]
    fn push_roster_appends_preserving_order() {
        let mut data = PlayerData { roster: vec![roster_member("Emberling")], eggs: Vec::new() };
        data.push_roster(roster_member("Newbie"));
        assert_eq!(data.roster.len(), 2, "roster must grow by one");
        assert_eq!(data.roster[0].name, "Emberling", "existing member's order must be preserved");
        assert_eq!(data.roster[1].name, "Newbie", "the new creature must be appended at the end");
    }

    /// `roster_has_open_slot` is true below `ROSTER_SIZE` and false once
    /// full, computed from the constant rather than a hardcoded number.
    #[test]
    fn roster_has_open_slot_true_below_size_full_at_size() {
        let mut data = PlayerData {
            roster: (0..ROSTER_SIZE - 1).map(|i| roster_member(&format!("M{i}"))).collect(),
            eggs: Vec::new(),
        };
        assert!(data.roster_has_open_slot(), "a roster below ROSTER_SIZE must report an open slot");

        data.roster.push(roster_member("Last"));
        assert!(!data.roster_has_open_slot(), "a full roster must report no open slot");
    }

    /// `replace_roster_slot` returns the displaced creature, leaves the
    /// roster's length unchanged, and installs `incoming` at `index` —
    /// the pick/dispose decoupling: the caller decides what happens to the
    /// returned creature.
    #[test]
    fn replace_roster_slot_returns_bumped_and_keeps_len() {
        let mut data = PlayerData {
            roster: vec![roster_member("A"), roster_member("B"), roster_member("C")],
            eggs: Vec::new(),
        };
        let bumped = data.replace_roster_slot(1, roster_member("Newbie"));
        assert_eq!(bumped.map(|c| c.name), Some("B".to_string()), "must return the displaced creature");
        assert_eq!(data.roster.len(), 3, "roster length must stay unchanged");
        assert_eq!(data.roster[1].name, "Newbie", "the incoming creature must occupy the replaced slot");
    }

    /// An out-of-range `index` is a no-op: returns `None` and mutates
    /// nothing.
    #[test]
    fn replace_roster_slot_out_of_range_is_none_no_mutation() {
        let mut data = PlayerData { roster: vec![roster_member("A"), roster_member("B")], eggs: Vec::new() };
        let before = data.roster.clone();
        let result = data.replace_roster_slot(5, roster_member("Newbie"));
        assert!(result.is_none(), "an out-of-range index must return None");
        assert_eq!(data.roster, before, "an out-of-range index must not mutate the roster");
    }

    fn bincode_variant_index(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes[..4].try_into().expect("at least 4 bytes"))
    }

    /// `EggState::Undefined` is bincode wire index 0 — the append-only
    /// contract's pinned position for this variant.
    #[test]
    fn eggstate_undefined_is_wire_index_0() {
        let bytes = bincode::serialize(&EggState::Undefined).expect("serialize");
        assert_eq!(bincode_variant_index(&bytes), 0);
    }

    /// `EggState::Incubating` is bincode wire index 1.
    #[test]
    fn eggstate_incubating_is_wire_index_1() {
        let bytes = bincode::serialize(&EggState::Incubating { started_at: std::time::UNIX_EPOCH })
            .expect("serialize");
        assert_eq!(bincode_variant_index(&bytes), 1);
    }

    /// `EggState::Ready` is bincode wire index 2.
    #[test]
    fn eggstate_ready_is_wire_index_2() {
        let bytes = bincode::serialize(&EggState::Ready).expect("serialize");
        assert_eq!(bincode_variant_index(&bytes), 2);
    }

    /// `Element` variants are pinned to their declared positions on the
    /// bincode wire — the append-only contract's guard for this enum.
    #[test]
    fn element_variants_are_pinned_to_declared_wire_index() {
        let expected = [
            (Element::Normal, 0),
            (Element::Fire, 1),
            (Element::Ice, 2),
            (Element::Earth, 3),
            (Element::Lightning, 4),
        ];
        for (variant, index) in expected {
            let bytes = bincode::serialize(&variant).expect("serialize");
            assert_eq!(bincode_variant_index(&bytes), index, "{variant:?} wire index");
        }
    }
}
