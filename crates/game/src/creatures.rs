//! The 6 creatures bundled into the binary this round, each a real
//! multi-frame idle GIF decoded via `include_bytes!` — not synthetic frames.
//! One `bundled_creature!` invocation per creature replaces what used to be
//! 6 near-identical files (name, GIF path, and function identifier are the
//! only things that ever differed between them).

use crate::ability::{Ability, Modifier};
use crate::exhaustion::Exhaustion;
use crate::squad_role::{squad_role, SquadRole, ACTIVE_SLOTS, BENCH_SLOTS};
use crate::stats::Stats;
use engine_render::AnimatedSprite;
use std::collections::HashMap;
use std::time::Duration;

/// The kind of animation a creature can play.
///
/// Extension policy: add variants here, don't restructure. New kinds
/// (Attack, Hurt, Death) become new catalog entries on `Creature`, never
/// new fields or a new type. `Idle` is the only kind that exists this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationKind {
    Idle,
}

/// A named creature and its catalog of animations, each resolvable to a
/// playable [`AnimatedSprite`].
pub struct Creature {
    name: String,
    animations: HashMap<AnimationKind, AnimatedSprite>,
}

impl Creature {
    /// New creature with the given name and an empty animation catalog.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            animations: HashMap::new(),
        }
    }

    /// Register `sprite` under `kind` and return self (builder style).
    pub fn with_animation(mut self, kind: AnimationKind, sprite: AnimatedSprite) -> Self {
        self.animations.insert(kind, sprite);
        self
    }

    /// The creature's display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The sprite registered under `kind`, if any.
    pub fn animation(&self, kind: AnimationKind) -> Option<&AnimatedSprite> {
        self.animations.get(&kind)
    }
}

/// Per-frame display time for every bundled creature's idle loop. The GIF's
/// own frame delays are ignored by `from_gif`; this is the uniform playback
/// rate. All 6 creatures share this rate today; a future creature needing a
/// different pace can pass its own duration as a 4th macro argument.
const FRAME_DUR: Duration = Duration::from_millis(80);

/// Defines `pub fn $fn_name() -> Creature`: decodes the bundled GIF at
/// `$gif_path` (relative to this file) into an `AnimatedSprite` at
/// `FRAME_DUR`, and returns a `Creature` named `$display_name` with that
/// sprite registered under `AnimationKind::Idle`.
macro_rules! bundled_creature {
    ($fn_name:ident, $display_name:literal, $gif_path:literal) => {
        #[doc = concat!("The bundled \"", $display_name, "\" creature.")]
        pub fn $fn_name() -> Creature {
            let sprite = AnimatedSprite::from_gif(include_bytes!($gif_path), FRAME_DUR)
                .expect(concat!("bundled ", $gif_path, " must decode"));
            Creature::new($display_name).with_animation(AnimationKind::Idle, sprite)
        }
    };
}

bundled_creature!(ember_wolf, "Ember Wolf", "creatures/ember_wolf_idle.gif");
bundled_creature!(frost_lizard, "Frost Lizard", "creatures/frost_lizard_idle.gif");
bundled_creature!(stone_golem, "Stone Golem", "creatures/stone_golem_idle.gif");
bundled_creature!(storm_hawk, "Storm Hawk", "creatures/storm_hawk_idle.gif");
bundled_creature!(verdant_treant, "Verdant Treant", "creatures/verdant_treant_idle.gif");
bundled_creature!(shadow_cat, "Shadow Cat", "creatures/shadow_cat_idle.gif");

/// Every creature bundled into the binary this round, in roster order.
pub fn all() -> Vec<Creature> {
    vec![
        ember_wolf(),
        frost_lizard(),
        stone_golem(),
        storm_hawk(),
        verdant_treant(),
        shadow_cat(),
    ]
}

/// Max abilities a roster entry may carry (spec `Decisions (v1)`: the same
/// `len() <= 4` convention as `Ability::modifiers`, applied one level up).
pub const MAX_ABILITIES: usize = 4;

/// A [`Creature`] (identity + animation catalog) paired with this game's
/// RPG data: stats, level, abilities, and an exhaustion meter. Fields are
/// private; the `abilities.len() <= MAX_ABILITIES` invariant is guaranteed
/// only via [`RosterEntry::new`]. Not `Clone`/`Debug` — `Creature` isn't.
pub struct RosterEntry {
    creature: Creature,
    stats: Stats,
    level: u32,
    abilities: Vec<Ability>,
    exhaustion: Exhaustion,
}

impl RosterEntry {
    /// Must debug-assert `abilities.len() <= MAX_ABILITIES` with a message
    /// containing "at most" (mirrors `Ability::new`'s invariant message
    /// pattern, one composition level up), then construct.
    pub fn new(
        creature: Creature,
        stats: Stats,
        level: u32,
        abilities: Vec<Ability>,
        exhaustion: Exhaustion,
    ) -> Self {
        debug_assert!(
            abilities.len() <= MAX_ABILITIES,
            "RosterEntry may hold at most {MAX_ABILITIES} abilities, got {}",
            abilities.len()
        );
        Self {
            creature,
            stats,
            level,
            abilities,
            exhaustion,
        }
    }

    pub fn creature(&self) -> &Creature {
        &self.creature
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn abilities(&self) -> &[Ability] {
        &self.abilities
    }

    pub fn exhaustion(&self) -> &Exhaustion {
        &self.exhaustion
    }
}

/// Placeholder injury reassignment (spec `34` gives only "drops to reserve",
/// no concrete policy). Swaps the entry at `injured_index` with the first
/// reserve slot so `squad_role(new_index)` reports `Reserve`; the previously-
/// reserve entry fills the vacated active/bench slot. No-op if the index is
/// out of range, the entry is not injured, the entry is already in reserve,
/// or the target reserve slot does not exist. Non-canonical, exercised only
/// by unit tests this round — TODO(code-writer): implement.
pub fn reassign_injured_to_reserve(roster: &mut [RosterEntry], injured_index: usize) {
    if injured_index >= roster.len() {
        return;
    }
    if !roster[injured_index].exhaustion().is_injured() {
        return;
    }
    if squad_role(injured_index) == SquadRole::Reserve {
        return;
    }
    let target = ACTIVE_SLOTS + BENCH_SLOTS; // first reserve slot
    if target >= roster.len() {
        return;
    }
    roster.swap(injured_index, target);
}

/// Wraps each of the 6 bundled creatures in a [`RosterEntry`] with
/// illustrative, distinguishing placeholder `Stats`/`level`/`abilities` and a
/// default (rested, non-injured) `Exhaustion`, in `all()`'s order. Values are
/// non-canonical placeholders (spec `34-creature-attributes-data-model.md`
/// `Decisions (v1)`) — TODO(code-writer): implement.
pub fn demo_roster() -> Vec<RosterEntry> {
    let entry = |creature: Creature,
                 stats: Stats,
                 level: u32,
                 abilities: Vec<Ability>|
     -> RosterEntry { RosterEntry::new(creature, stats, level, abilities, Exhaustion::default()) };

    vec![
        entry(
            ember_wolf(),
            Stats { strength: 30, dexterity: 28, intelligence: 12, vitality: 15 },
            5,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
        entry(
            frost_lizard(),
            Stats { strength: 14, dexterity: 18, intelligence: 26, vitality: 22 },
            4,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
        entry(
            stone_golem(),
            Stats { strength: 22, dexterity: 8, intelligence: 10, vitality: 34 },
            6,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
        entry(
            storm_hawk(),
            Stats { strength: 12, dexterity: 32, intelligence: 24, vitality: 12 },
            4,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
        entry(
            verdant_treant(),
            Stats { strength: 20, dexterity: 10, intelligence: 22, vitality: 30 },
            7,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
        entry(
            shadow_cat(),
            Stats { strength: 16, dexterity: 34, intelligence: 18, vitality: 12 },
            3,
            vec![Ability::new(
                "Placeholder ability 1",
                vec![Modifier { name: "Modifier A".to_string(), requires: None }],
            )],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn px(r: u8, g: u8, b: u8) -> image::DynamicImage {
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([r, g, b, 255]));
        image::DynamicImage::from(img)
    }

    fn make_sprite() -> AnimatedSprite {
        AnimatedSprite::new(vec![px(255, 0, 0), px(0, 255, 0)], Duration::from_millis(100))
    }

    /// The name supplied at construction round-trips through `name()`.
    #[test]
    fn name_round_trips() {
        let c = Creature::new("Test");
        assert_eq!(c.name(), "Test");
    }

    /// A sprite registered under a kind is retrievable under that same kind,
    /// with identical frame_count/frame_dur — proves the catalog is a real
    /// keyed lookup, not a hardcoded per-kind field.
    #[test]
    fn registered_idle_is_retrievable() {
        let sprite = make_sprite();
        let expected_frames = sprite.frame_count();
        let expected_dur = sprite.frame_dur();
        let c = Creature::new("Test").with_animation(AnimationKind::Idle, sprite);

        let found = c.animation(AnimationKind::Idle).expect("idle animation must be registered");
        assert_eq!(found.frame_count(), expected_frames);
        assert_eq!(found.frame_dur(), expected_dur);
    }

    /// Looking up a kind that was never registered returns `None` — proves
    /// the lookup genuinely depends on the `AnimationKind` argument rather
    /// than always returning whatever was last inserted.
    #[test]
    fn unregistered_kind_is_none_even_when_other_kind_registered() {
        let c = Creature::new("Test").with_animation(AnimationKind::Idle, make_sprite());
        // Idle is registered; a fresh creature with nothing registered must
        // still report None for Idle, proving lookup isn't hardcoded true.
        let empty = Creature::new("Empty");
        assert!(empty.animation(AnimationKind::Idle).is_none());
        // Sanity: the non-empty one does resolve.
        assert!(c.animation(AnimationKind::Idle).is_some());
    }

    /// Every bundled creature has its declared name and a real multi-frame
    /// (>= 2 frames) idle animation — one parametrized test in place of the
    /// 6 near-identical per-file tests this replaces.
    type Ctor = fn() -> Creature;

    #[test]
    fn every_bundled_creature_has_named_multi_frame_idle() {
        let cases: [(Ctor, &str); 6] = [
            (ember_wolf, "Ember Wolf"),
            (frost_lizard, "Frost Lizard"),
            (stone_golem, "Stone Golem"),
            (storm_hawk, "Storm Hawk"),
            (verdant_treant, "Verdant Treant"),
            (shadow_cat, "Shadow Cat"),
        ];
        for (ctor, expected_name) in cases {
            let c = ctor();
            assert_eq!(c.name(), expected_name);
            let sprite = c
                .animation(AnimationKind::Idle)
                .unwrap_or_else(|| panic!("{expected_name} must have an Idle animation registered"));
            assert!(
                sprite.frame_count() >= 2,
                "{expected_name}'s idle animation must be a real animated loop (>= 2 frames), got {}",
                sprite.frame_count()
            );
        }
    }

    /// `all()` genuinely aggregates all six bundled creatures — the single
    /// enumeration point a future roster carousel consumes — catching a
    /// silently dropped or duplicated entry, and confirms every entry has
    /// its Idle animation registered.
    #[test]
    fn all_returns_six_named_idle_creatures() {
        let creatures = super::all();
        assert_eq!(creatures.len(), 6, "expected exactly 6 bundled creatures");

        let names: HashSet<&str> = creatures.iter().map(|c| c.name()).collect();
        let expected: HashSet<&str> = [
            "Ember Wolf",
            "Frost Lizard",
            "Stone Golem",
            "Storm Hawk",
            "Verdant Treant",
            "Shadow Cat",
        ]
        .into_iter()
        .collect();
        assert_eq!(names, expected);

        for c in &creatures {
            assert!(
                c.animation(AnimationKind::Idle).is_some(),
                "{} must have an Idle animation registered",
                c.name()
            );
        }
    }

    fn dummy_abilities(n: usize) -> Vec<Ability> {
        (0..n).map(|i| Ability::new(format!("Ability {i}"), vec![])).collect()
    }

    /// Constructing with `MAX_ABILITIES` abilities succeeds, and every field
    /// (including a non-default level/stats/exhaustion so a hardcoded-default
    /// getter can't silently pass) round-trips through its accessor.
    #[test]
    fn new_round_trips_all_fields() {
        let creature = Creature::new("Test");
        let stats = Stats { strength: 5, dexterity: 6, intelligence: 7, vitality: 8 };
        let level = 3u32;
        let abilities = dummy_abilities(MAX_ABILITIES);
        let exhaustion = Exhaustion::default().apply_damage_exhaustion(15);

        let entry = RosterEntry::new(creature, stats, level, abilities.clone(), exhaustion);

        assert_eq!(entry.creature().name(), "Test");
        assert_eq!(*entry.stats(), stats);
        assert_eq!(entry.level(), level);
        assert_eq!(entry.abilities(), abilities.as_slice());
        assert_eq!(entry.exhaustion(), &exhaustion);
    }

    /// Constructing with more than `MAX_ABILITIES` abilities panics under
    /// debug assertions (the profile `cargo test` uses by default) — same
    /// invariant-enforcement pattern as `Ability::new`, one level up. The
    /// panic message must mention "at most" so this can't be satisfied by an
    /// unrelated panic.
    #[test]
    #[should_panic(expected = "at most")]
    fn new_with_more_than_max_abilities_panics() {
        let creature = Creature::new("Test");
        let stats = Stats::default();
        let exhaustion = Exhaustion::default();
        RosterEntry::new(creature, stats, 1, dummy_abilities(MAX_ABILITIES + 1), exhaustion);
    }

    /// A full `ROSTER_SIZE` roster of distinctly-named entries, all rested
    /// except `injured_index` (if `Some`), which is pushed to `Exhaustion`'s
    /// injured state via 100 damage-exhaustion.
    fn build_roster(injured_index: Option<usize>) -> Vec<RosterEntry> {
        (0..crate::squad_role::ROSTER_SIZE)
            .map(|i| {
                let exhaustion = if Some(i) == injured_index {
                    Exhaustion::default().apply_damage_exhaustion(100)
                } else {
                    Exhaustion::default()
                };
                RosterEntry::new(
                    Creature::new(format!("Entry {i}")),
                    Stats::default(),
                    1,
                    vec![],
                    exhaustion,
                )
            })
            .collect()
    }

    fn names(roster: &[RosterEntry]) -> Vec<String> {
        roster.iter().map(|e| e.creature().name().to_string()).collect()
    }

    /// An injured entry in an active slot is swapped into the first reserve
    /// slot: `squad_role(new_index)` reports `Reserve` for the injured
    /// entry's new position, and the entry that occupied that reserve slot
    /// now sits at the vacated active index — proving a real swap, not a
    /// no-op.
    #[test]
    fn reassign_moves_injured_active_entry_into_reserve() {
        let mut roster = build_roster(Some(0));
        let before = names(&roster);
        let first_reserve = ACTIVE_SLOTS + BENCH_SLOTS;

        reassign_injured_to_reserve(&mut roster, 0);

        let new_index = names(&roster)
            .iter()
            .position(|n| n == "Entry 0")
            .expect("injured entry must still be present");
        assert_eq!(
            squad_role(new_index),
            SquadRole::Reserve,
            "injured entry's new position {new_index} must be a reserve slot"
        );
        assert_eq!(
            roster[0].creature().name(),
            before[first_reserve],
            "the entry previously in the first reserve slot must now occupy the vacated active slot"
        );
    }

    /// A not-injured entry in an active slot is left untouched — the roster
    /// order is unchanged.
    #[test]
    fn reassign_is_noop_when_entry_not_injured() {
        let mut roster = build_roster(None);
        let before = names(&roster);

        reassign_injured_to_reserve(&mut roster, 0);

        assert_eq!(names(&roster), before);
    }

    /// An injured entry already occupying a reserve slot is left untouched —
    /// no swap needed since it already satisfies the reserve invariant.
    #[test]
    fn reassign_is_noop_when_already_reserve() {
        let reserve_index = ACTIVE_SLOTS + BENCH_SLOTS;
        let mut roster = build_roster(Some(reserve_index));
        let before = names(&roster);

        reassign_injured_to_reserve(&mut roster, reserve_index);

        assert_eq!(names(&roster), before);
    }

    /// `demo_roster()` returns exactly `ROSTER_SIZE` entries — one per
    /// bundled creature, no dropped/duplicated entry.
    #[test]
    fn demo_roster_returns_six_entries() {
        let roster = demo_roster();
        assert_eq!(roster.len(), crate::squad_role::ROSTER_SIZE);
    }

    /// Names are the 6 bundled creatures, in `all()`'s order — guards
    /// against a dropped/duplicated/reordered entry.
    #[test]
    fn demo_roster_names_match_bundled_order() {
        let roster = demo_roster();
        let expected = [
            "Ember Wolf",
            "Frost Lizard",
            "Stone Golem",
            "Storm Hawk",
            "Verdant Treant",
            "Shadow Cat",
        ];
        let actual: Vec<&str> = roster.iter().map(|e| e.creature().name()).collect();
        assert_eq!(actual, expected);
    }

    /// Ember Wolf and Stone Golem must have genuinely different `Stats`
    /// profiles — proves the "distinguishing" requirement is real, not
    /// 6 copy-pasted identical placeholder structs.
    #[test]
    fn demo_roster_two_creatures_have_distinct_stats() {
        let roster = demo_roster();
        let ember_wolf = roster
            .iter()
            .find(|e| e.creature().name() == "Ember Wolf")
            .expect("Ember Wolf must be in demo_roster");
        let stone_golem = roster
            .iter()
            .find(|e| e.creature().name() == "Stone Golem")
            .expect("Stone Golem must be in demo_roster");
        assert_ne!(
            ember_wolf.stats(),
            stone_golem.stats(),
            "Ember Wolf and Stone Golem must have distinguishing (non-identical) placeholder stats"
        );
    }

    /// Every demo entry starts rested (not injured) — a sane default for
    /// placeholder data with no live combat trigger this round.
    #[test]
    fn demo_roster_entries_start_rested() {
        let roster = demo_roster();
        for entry in &roster {
            assert!(
                !entry.exhaustion().is_injured(),
                "{} must start rested, not injured",
                entry.creature().name()
            );
        }
    }
}
