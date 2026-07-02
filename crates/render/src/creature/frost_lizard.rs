//! The bundled "Frost Lizard" creature: a crystalline ice lizard, with a
//! real idle animation decoded from compiled-in GIF bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Frost Lizard idle loop. The GIF's own frame
/// delays are ignored by `from_gif`; this is the uniform playback rate.
const FROST_LIZARD_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Frost Lizard" creature: name `"Frost Lizard"`, idle
/// animation registered under [`crate::AnimationKind::Idle`], decoded from a
/// real multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn frost_lizard() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/frost_lizard_idle.gif"),
        FROST_LIZARD_FRAME_DUR,
    )
    .expect("bundled frost_lizard_idle.gif must decode");
    Creature::new("Frost Lizard").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::frost_lizard;
    use crate::AnimationKind;

    /// The bundled Frost Lizard creature is named correctly and carries a
    /// real, multi-frame idle animation decoded from the bundled GIF bytes
    /// (not a synthetic single-frame stand-in).
    #[test]
    fn frost_lizard_has_named_idle_animation() {
        let c = frost_lizard();
        assert_eq!(c.name(), "Frost Lizard");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Frost Lizard must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
