//! The bundled "Shadow Cat" creature: a sleek dark-wisped panther with a low
//! sleek silhouette, with a real idle animation decoded from compiled-in GIF
//! bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Shadow Cat idle loop. The GIF's own frame
/// delays are ignored by `from_gif`; this is the uniform playback rate.
const SHADOW_CAT_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Shadow Cat" creature: name `"Shadow Cat"`, idle animation
/// registered under [`crate::AnimationKind::Idle`], decoded from a real
/// multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn shadow_cat() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/shadow_cat_idle.gif"),
        SHADOW_CAT_FRAME_DUR,
    )
    .expect("bundled shadow_cat_idle.gif must decode");
    Creature::new("Shadow Cat").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::shadow_cat;
    use crate::AnimationKind;

    /// The bundled Shadow Cat creature is named correctly and carries a
    /// real, multi-frame idle animation decoded from the bundled GIF bytes
    /// (not a synthetic single-frame stand-in).
    #[test]
    fn shadow_cat_has_named_idle_animation() {
        let c = shadow_cat();
        assert_eq!(c.name(), "Shadow Cat");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Shadow Cat must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
