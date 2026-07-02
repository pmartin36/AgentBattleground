//! The bundled "Ember Wolf" creature: a wolf wreathed in low flame, with a
//! real idle animation decoded from compiled-in GIF bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Ember Wolf idle loop. The GIF's own frame
/// delays are ignored by `from_gif`; this is the uniform playback rate.
const EMBER_WOLF_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Ember Wolf" creature: name `"Ember Wolf"`, idle animation
/// registered under [`crate::AnimationKind::Idle`], decoded from a real
/// multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn ember_wolf() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/ember_wolf_idle.gif"),
        EMBER_WOLF_FRAME_DUR,
    )
    .expect("bundled ember_wolf_idle.gif must decode");
    Creature::new("Ember Wolf").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::ember_wolf;
    use crate::AnimationKind;

    /// The bundled Ember Wolf creature is named correctly and carries a real,
    /// multi-frame idle animation decoded from the bundled GIF bytes (not a
    /// synthetic single-frame stand-in).
    #[test]
    fn ember_wolf_has_named_idle_animation() {
        let c = ember_wolf();
        assert_eq!(c.name(), "Ember Wolf");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Ember Wolf must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
