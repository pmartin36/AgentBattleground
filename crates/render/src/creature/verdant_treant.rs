//! The bundled "Verdant Treant" creature: a small animate tree/plant
//! guardian with a spindly-limbed silhouette, with a real idle animation
//! decoded from compiled-in GIF bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Verdant Treant idle loop. The GIF's own
/// frame delays are ignored by `from_gif`; this is the uniform playback rate.
const VERDANT_TREANT_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Verdant Treant" creature: name `"Verdant Treant"`, idle
/// animation registered under [`crate::AnimationKind::Idle`], decoded from a
/// real multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn verdant_treant() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/verdant_treant_idle.gif"),
        VERDANT_TREANT_FRAME_DUR,
    )
    .expect("bundled verdant_treant_idle.gif must decode");
    Creature::new("Verdant Treant").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::verdant_treant;
    use crate::AnimationKind;

    /// The bundled Verdant Treant creature is named correctly and carries a
    /// real, multi-frame idle animation decoded from the bundled GIF bytes
    /// (not a synthetic single-frame stand-in).
    #[test]
    fn verdant_treant_has_named_idle_animation() {
        let c = verdant_treant();
        assert_eq!(c.name(), "Verdant Treant");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Verdant Treant must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
