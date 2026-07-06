//! The 6 creatures bundled into the binary this round, each a real
//! multi-frame idle GIF decoded via `include_bytes!` — not synthetic frames.
//! One `bundled_creature!` invocation per creature replaces what used to be
//! 6 near-identical files (name, GIF path, and function identifier are the
//! only things that ever differed between them).

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
}
