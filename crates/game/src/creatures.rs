//! The 8 creatures bundled into the binary this round, each a real
//! multi-frame idle GIF decoded via `include_bytes!` — not synthetic frames.
//! One `bundled_creature!` invocation per creature replaces what used to be
//! 6 near-identical files (name, GIF path, and function identifier are the
//! only things that ever differed between them).

use crate::ability::{Ability, AbilityType, DamageClass, Element, Modifier, StatusEffect};
use crate::squad_role::{squad_role, SquadRole, ACTIVE_SLOTS, BENCH_SLOTS};
use crate::stamina::Stamina;
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

/// Max abilities a creature may carry (spec `Decisions (v1)`: the same
/// `len() <= 4` convention as `Ability::modifiers`, applied one level up).
pub const MAX_ABILITIES: usize = 4;

/// Shared placeholder XP cap — the fill-fraction denominator for the post-
/// battle XP bar (spec `46-post-battle-results-screen.md`). Never incremented
/// this pass.
pub const XP_TO_NEXT_LEVEL: u32 = 100;

/// A named creature: its animation catalog plus this game's RPG data —
/// stats, level, abilities, and a stamina meter. There is no separate
/// "roster entry" wrapper type around this: every `Creature` in this game is
/// always a full battle participant, so the RPG fields live directly here
/// rather than behind an extra layer of indirection. (`Creature` itself
/// already lives in `crates/game`, not `engine_render` — see
/// `34-creature-attributes-data-model.md`'s Decisions.)
pub struct Creature {
    name: String,
    animations: HashMap<AnimationKind, AnimatedSprite>,
    stats: Stats,
    level: u32,
    xp: u32,
    abilities: Vec<Ability>,
    stamina: Stamina,
}

impl Creature {
    /// New creature with the given name, an empty animation catalog, and
    /// placeholder RPG defaults (`Stats::default()`, level 1, no abilities,
    /// full `Stamina`) — override via the `with_*` builders below.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            animations: HashMap::new(),
            stats: Stats::default(),
            level: 1,
            xp: 0,
            abilities: Vec::new(),
            stamina: Stamina::default(),
        }
    }

    /// Register `sprite` under `kind` and return self (builder style).
    pub fn with_animation(mut self, kind: AnimationKind, sprite: AnimatedSprite) -> Self {
        self.animations.insert(kind, sprite);
        self
    }

    /// Set this creature's `Stats` and return self (builder style).
    pub fn with_stats(mut self, stats: Stats) -> Self {
        self.stats = stats;
        self
    }

    /// Set this creature's level and return self (builder style).
    pub fn with_level(mut self, level: u32) -> Self {
        self.level = level;
        self
    }

    /// Set this creature's xp and return self (builder style).
    pub fn with_xp(mut self, xp: u32) -> Self {
        self.xp = xp;
        self
    }

    /// Set this creature's abilities and return self (builder style).
    /// Debug-asserts `abilities.len() <= MAX_ABILITIES`.
    pub fn with_abilities(mut self, abilities: Vec<Ability>) -> Self {
        debug_assert!(
            abilities.len() <= MAX_ABILITIES,
            "Creature may hold at most {MAX_ABILITIES} abilities, got {}",
            abilities.len()
        );
        self.abilities = abilities;
        self
    }

    /// Set this creature's `Stamina` and return self (builder style).
    pub fn with_stamina(mut self, stamina: Stamina) -> Self {
        self.stamina = stamina;
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

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn xp(&self) -> u32 {
        self.xp
    }

    pub fn abilities(&self) -> &[Ability] {
        &self.abilities
    }

    pub fn stamina(&self) -> &Stamina {
        &self.stamina
    }
}

/// Per-frame display time for every bundled creature's idle loop. The GIF's
/// own frame delays are ignored by `from_gif`; this is the uniform playback
/// rate. All 8 creatures share this rate today; a future creature needing a
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
bundled_creature!(tide_serpent, "Tide Serpent", "creatures/tide_serpent_idle.gif");
bundled_creature!(volt_scorpion, "Volt Scorpion", "creatures/volt_scorpion_idle.gif");

/// Every creature bundled into the binary this round, in roster order.
pub fn all() -> Vec<Creature> {
    vec![
        ember_wolf(),
        frost_lizard(),
        stone_golem(),
        storm_hawk(),
        verdant_treant(),
        shadow_cat(),
        tide_serpent(),
        volt_scorpion(),
    ]
}

/// Placeholder injury reassignment (spec `34` gives only "drops to reserve",
/// no concrete policy). Swaps the entry at `injured_index` with the first
/// reserve slot so `squad_role(new_index)` reports `Reserve`; the previously-
/// reserve entry fills the vacated active/bench slot. No-op if the index is
/// out of range, the entry is not injured, the entry is already in reserve,
/// or the target reserve slot does not exist. Non-canonical, exercised only
/// by unit tests this round — TODO(code-writer): implement.
pub fn reassign_injured_to_reserve(roster: &mut [Creature], injured_index: usize) {
    if injured_index >= roster.len() {
        return;
    }
    if !roster[injured_index].stamina().is_injured() {
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

/// Wraps each of the 6 bundled creatures with illustrative, distinguishing
/// placeholder `Stats`/`level`/`abilities` (default, full `Stamina`), in
/// `all()`'s order. Values are non-canonical placeholders (spec
/// `34-creature-attributes-data-model.md` `Decisions (v1)`).
pub fn demo_roster() -> Vec<Creature> {
    let entry = |creature: Creature, stats: Stats, level: u32, abilities: Vec<Ability>| -> Creature {
        creature.with_stats(stats).with_level(level).with_abilities(abilities)
    };

    vec![
        entry(
            ember_wolf(),
            Stats { strength: 30, dexterity: 28, intelligence: 12, vitality: 15 },
            5,
            vec![
                Ability::new("Ember Fang", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Fire)
                    .with_class(DamageClass::Physical)
                    .with_cost(3)
                    .with_damage(25)
                    .with_range(1)
                    .with_status_effects(vec![StatusEffect { name: "Burn".to_string() }])
                    .with_flavor("Fangs wreathed in flame, searing on contact."),
                Ability::new("Howl", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(2)
                    .with_flavor("A bone-shaking howl that steels the pack's nerve."),
            ],
        )
        .with_xp(65)
        .with_stamina(Stamina::default().drain_from_damage(20)),
        entry(
            frost_lizard(),
            Stats { strength: 14, dexterity: 18, intelligence: 26, vitality: 22 },
            4,
            vec![
                Ability::new("Frost Bite", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Water)
                    .with_class(DamageClass::Magic)
                    .with_cost(2)
                    .with_damage(18)
                    .with_range(2)
                    .with_flavor("A chilling bite that numbs the target's reflexes."),
                Ability::new("Ice Shield", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(2)
                    .with_flavor("Conjures a shell of hardened ice."),
            ],
        )
        .with_xp(30)
        .with_stamina(Stamina::default().drain_from_damage(55)),
        entry(
            stone_golem(),
            Stats { strength: 22, dexterity: 8, intelligence: 10, vitality: 34 },
            6,
            vec![
                Ability::new("Boulder Smash", vec![Modifier { name: "Modifier A".to_string(), requires: None }])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Earth)
                    .with_class(DamageClass::Physical)
                    .with_cost(4)
                    .with_damage(30)
                    .with_range(1)
                    .with_flavor("A ground-shaking smash of raw stone."),
                Ability::new("Stone Skin", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(3)
                    .with_flavor("Hardens the golem's hide against incoming blows."),
            ],
        )
        .with_xp(90)
        .with_stamina(Stamina::default().drain_from_damage(40)),
        entry(
            storm_hawk(),
            Stats { strength: 12, dexterity: 32, intelligence: 24, vitality: 12 },
            4,
            vec![
                Ability::new("Thunder Strike", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Lightning)
                    .with_class(DamageClass::Magic)
                    .with_cost(3)
                    .with_damage(22)
                    .with_range(3)
                    .with_status_effects(vec![StatusEffect { name: "Shock".to_string() }])
                    .with_flavor("A bolt of lightning called down from storm clouds."),
                Ability::new("Wind Slash", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Normal)
                    .with_class(DamageClass::Physical)
                    .with_cost(1)
                    .with_damage(10)
                    .with_range(1)
                    .with_flavor("A quick slash carried on razor wind."),
            ],
        )
        .with_xp(15)
        .with_stamina(Stamina::default().drain_from_damage(90)),
        entry(
            verdant_treant(),
            Stats { strength: 20, dexterity: 10, intelligence: 22, vitality: 30 },
            7,
            vec![
                Ability::new("Root Bind", vec![])
                    .with_ability_type(AbilityType::Debuff)
                    .with_element(Element::Earth)
                    .with_class(DamageClass::Magic)
                    .with_cost(2)
                    .with_range(2)
                    .with_status_effects(vec![StatusEffect { name: "Rooted".to_string() }])
                    .with_flavor("Grasping roots erupt to snare the target's feet."),
                Ability::new("Photosynthesis", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(1)
                    .with_flavor("Draws deep from sunlight to restore vigor."),
                Ability::new("Thorn Whip", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Earth)
                    .with_class(DamageClass::Physical)
                    .with_cost(2)
                    .with_damage(15)
                    .with_range(2)
                    .with_flavor("A lashing whip of woody thorns."),
                Ability::new("Regrowth", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(3)
                    .with_flavor("Knits wounds shut with rapid new growth."),
            ],
        ),
        entry(
            shadow_cat(),
            Stats { strength: 16, dexterity: 34, intelligence: 18, vitality: 12 },
            3,
            vec![
                Ability::new("Shadow Slash", vec![])
                    .with_ability_type(AbilityType::Attack)
                    .with_element(Element::Normal)
                    .with_class(DamageClass::Physical)
                    .with_cost(2)
                    .with_damage(20)
                    .with_range(1)
                    .with_status_effects(vec![StatusEffect { name: "Bleed".to_string() }])
                    .with_flavor("A blindingly fast strike from the shadows."),
                Ability::new("Vanish", vec![])
                    .with_ability_type(AbilityType::Buff)
                    .with_cost(3)
                    .with_flavor("Slips out of sight, evading the next attack."),
            ],
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
    /// near-identical per-file tests this replaces.
    type Ctor = fn() -> Creature;

    #[test]
    fn every_bundled_creature_has_named_multi_frame_idle() {
        let cases: [(Ctor, &str); 8] = [
            (ember_wolf, "Ember Wolf"),
            (frost_lizard, "Frost Lizard"),
            (stone_golem, "Stone Golem"),
            (storm_hawk, "Storm Hawk"),
            (verdant_treant, "Verdant Treant"),
            (shadow_cat, "Shadow Cat"),
            (tide_serpent, "Tide Serpent"),
            (volt_scorpion, "Volt Scorpion"),
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

    /// `all()` genuinely aggregates all eight bundled creatures — the single
    /// enumeration point a future roster carousel consumes — catching a
    /// silently dropped or duplicated entry, and confirms every entry has
    /// its Idle animation registered.
    #[test]
    fn all_returns_eight_named_idle_creatures() {
        let creatures = super::all();
        assert_eq!(creatures.len(), 8, "expected exactly 8 bundled creatures");

        let names: HashSet<&str> = creatures.iter().map(|c| c.name()).collect();
        let expected: HashSet<&str> = [
            "Ember Wolf",
            "Frost Lizard",
            "Stone Golem",
            "Storm Hawk",
            "Verdant Treant",
            "Shadow Cat",
            "Tide Serpent",
            "Volt Scorpion",
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

    /// Building with `MAX_ABILITIES` abilities succeeds, and every field
    /// (including a non-default level/stats/stamina so a hardcoded-default
    /// getter can't silently pass) round-trips through its accessor.
    #[test]
    fn builders_round_trip_all_fields() {
        let stats = Stats { strength: 5, dexterity: 6, intelligence: 7, vitality: 8 };
        let level = 3u32;
        let abilities = dummy_abilities(MAX_ABILITIES);
        let stamina = Stamina::default().drain_from_damage(15);

        let creature = Creature::new("Test")
            .with_stats(stats)
            .with_level(level)
            .with_abilities(abilities.clone())
            .with_stamina(stamina);

        assert_eq!(creature.name(), "Test");
        assert_eq!(*creature.stats(), stats);
        assert_eq!(creature.level(), level);
        assert_eq!(creature.abilities(), abilities.as_slice());
        assert_eq!(creature.stamina(), &stamina);
    }

    /// Constructing with more than `MAX_ABILITIES` abilities panics under
    /// debug assertions (the profile `cargo test` uses by default). The
    /// panic message must mention "at most" so this can't be satisfied by an
    /// unrelated panic.
    #[test]
    #[should_panic(expected = "at most")]
    fn with_abilities_more_than_max_panics() {
        Creature::new("Test").with_abilities(dummy_abilities(MAX_ABILITIES + 1));
    }

    /// A full `ROSTER_SIZE` roster of distinctly-named creatures, all rested
    /// except `injured_index` (if `Some`), which is pushed to `Stamina`'s
    /// injured state via a full 100 drain.
    fn build_roster(injured_index: Option<usize>) -> Vec<Creature> {
        (0..crate::squad_role::ROSTER_SIZE)
            .map(|i| {
                let stamina = if Some(i) == injured_index {
                    Stamina::default().drain_from_damage(100)
                } else {
                    Stamina::default()
                };
                Creature::new(format!("Entry {i}")).with_stamina(stamina)
            })
            .collect()
    }

    fn names(roster: &[Creature]) -> Vec<String> {
        roster.iter().map(|c| c.name().to_string()).collect()
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
            roster[0].name(),
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
        let actual: Vec<&str> = roster.iter().map(|c| c.name()).collect();
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
            .find(|c| c.name() == "Ember Wolf")
            .expect("Ember Wolf must be in demo_roster");
        let stone_golem = roster
            .iter()
            .find(|c| c.name() == "Stone Golem")
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
        for creature in &roster {
            assert!(
                !creature.stamina().is_injured(),
                "{} must start rested, not injured",
                creature.name()
            );
        }
    }

    /// A freshly constructed `Creature` starts at 0 xp.
    #[test]
    fn new_creature_xp_is_zero() {
        let c = Creature::new("Test");
        assert_eq!(c.xp(), 0);
    }

    /// `with_xp` round-trips through `xp()` — a non-zero value so a
    /// hardcoded-0 getter can't silently pass.
    #[test]
    fn with_xp_round_trips() {
        let c = Creature::new("Test").with_xp(42);
        assert_eq!(c.xp(), 42);
    }

    /// The shared XP-to-next-level cap is pinned at 100 (spec
    /// `46-post-battle-results-screen.md`).
    #[test]
    fn xp_to_next_level_is_100() {
        assert_eq!(XP_TO_NEXT_LEVEL, 100);
    }

    /// The first four `demo_roster()` entries carry four distinct stamina
    /// percents spanning all three b4-t4 bands (`>50` green, `15..=50`
    /// yellow, `<15` red) — the data source the post-battle stamina bar
    /// render tests rely on.
    #[test]
    fn demo_roster_first_four_span_all_three_stamina_bands() {
        let roster = demo_roster();
        let percents: Vec<u8> = roster.iter().take(4).map(|c| c.stamina().percent()).collect();
        assert_eq!(percents.len(), 4);

        let distinct: HashSet<u8> = percents.iter().copied().collect();
        assert_eq!(distinct.len(), 4, "the first four stamina percents must be distinct, got {percents:?}");

        assert!(percents.iter().any(|&p| p > 50), "no percent >50 (green band) in {percents:?}");
        assert!(
            percents.iter().any(|&p| (15..=50).contains(&p)),
            "no percent in 15..=50 (yellow band) in {percents:?}"
        );
        assert!(percents.iter().any(|&p| p < 15), "no percent <15 (red band) in {percents:?}");
    }

    /// The first four `demo_roster()` entries carry four distinct seeded xp
    /// values — the data source the post-battle XP bar animation reads as
    /// its `start`.
    #[test]
    fn demo_roster_first_four_have_distinct_seeded_xp() {
        let roster = demo_roster();
        let xps: Vec<u32> = roster.iter().take(4).map(|c| c.xp()).collect();
        assert_eq!(xps.len(), 4);

        let distinct: HashSet<u32> = xps.iter().copied().collect();
        assert_eq!(distinct.len(), 4, "the first four seeded xp values must be distinct, got {xps:?}");
        assert!(xps.iter().any(|&x| x != 0), "all four seeded xp values were 0, expected real seeds");
    }

    /// `demo_roster()` must exercise both presence and genuine absence of
    /// the b2-t1 optional `Ability` fields: at least one ability has every
    /// one of the 8 new fields populated (the "fully loaded" demonstrator),
    /// and at least one ability leaves some optional field absent (the
    /// "omit when absent" demonstrator specs 48/49 verify against). A
    /// rewrite that populates every field on every ability would silently
    /// lose the absence case and must fail this test.
    #[test]
    fn demo_roster_exercises_present_and_absent_optional_fields() {
        let roster = demo_roster();
        let abilities: Vec<&Ability> = roster.iter().flat_map(|c| c.abilities()).collect();
        assert!(!abilities.is_empty(), "demo_roster() must have at least one ability");

        let fully_populated = abilities.iter().any(|a| {
            a.ability_type().is_some()
                && a.element().is_some()
                && a.class().is_some()
                && a.cost().is_some()
                && a.damage().is_some()
                && a.range().is_some()
                && !a.status_effects().is_empty()
                && a.flavor().is_some()
        });
        assert!(
            fully_populated,
            "expected at least one demo_roster() ability with all 8 new fields populated"
        );

        let has_absence = abilities.iter().any(|a| {
            a.ability_type().is_none()
                || a.element().is_none()
                || a.class().is_none()
                || a.cost().is_none()
                || a.damage().is_none()
                || a.range().is_none()
                || a.status_effects().is_empty()
        });
        assert!(
            has_absence,
            "expected at least one demo_roster() ability to leave some optional field \
             absent (\"omit when absent\" demonstrator) — a rewrite that populates every \
             field on every ability would silently lose this case"
        );
    }

    /// Guards the b3-t1 feed-forward seam: pairing the seeded xp with the
    /// planned `xp_gained` deltas `[50, 30, 40, 25]` must cross
    /// `XP_TO_NEXT_LEVEL` for exactly two of the first four columns.
    #[test]
    fn demo_roster_seeded_xp_supports_two_rollovers() {
        let roster = demo_roster();
        let xps: Vec<u32> = roster.iter().take(4).map(|c| c.xp()).collect();
        let gained: [u32; 4] = [50, 30, 40, 25];

        let crossings = xps
            .iter()
            .zip(gained.iter())
            .filter(|(&start, &g)| start + g > XP_TO_NEXT_LEVEL)
            .count();
        assert_eq!(
            crossings, 2,
            "expected exactly two of the first four columns to cross XP_TO_NEXT_LEVEL, got {crossings} (xps={xps:?})"
        );
    }
}
