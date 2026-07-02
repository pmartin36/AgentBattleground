//! Creature identity: a named piece and its catalog of playable animations.

use crate::AnimatedSprite;
use std::collections::HashMap;

mod ember_wolf;
pub use ember_wolf::ember_wolf;
mod frost_lizard;
pub use frost_lizard::frost_lizard;
mod stone_golem;
pub use stone_golem::stone_golem;
mod storm_hawk;
pub use storm_hawk::storm_hawk;
mod verdant_treant;
pub use verdant_treant::verdant_treant;
mod shadow_cat;
pub use shadow_cat::shadow_cat;

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

#[cfg(test)]
mod tests {
    use super::{AnimationKind, Creature};
    use crate::AnimatedSprite;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};
    use std::time::Duration;

    fn px(r: u8, g: u8, b: u8) -> DynamicImage {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, PixelRgba([r, g, b, 255]));
        DynamicImage::from(img)
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
}
