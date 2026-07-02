//! The bundled "Storm Hawk" creature: a lightning-crackling hawk with a
//! winged silhouette, with a real idle animation decoded from compiled-in
//! GIF bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Storm Hawk idle loop. The GIF's own frame
/// delays are ignored by `from_gif`; this is the uniform playback rate.
const STORM_HAWK_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Storm Hawk" creature: name `"Storm Hawk"`, idle animation
/// registered under [`crate::AnimationKind::Idle`], decoded from a real
/// multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn storm_hawk() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/storm_hawk_idle.gif"),
        STORM_HAWK_FRAME_DUR,
    )
    .expect("bundled storm_hawk_idle.gif must decode");
    Creature::new("Storm Hawk").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::storm_hawk;
    use crate::AnimationKind;

    /// The bundled Storm Hawk creature is named correctly and carries a
    /// real, multi-frame idle animation decoded from the bundled GIF bytes
    /// (not a synthetic single-frame stand-in).
    #[test]
    fn storm_hawk_has_named_idle_animation() {
        let c = storm_hawk();
        assert_eq!(c.name(), "Storm Hawk");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Storm Hawk must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
