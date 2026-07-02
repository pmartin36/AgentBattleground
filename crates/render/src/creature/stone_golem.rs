//! The bundled "Stone Golem" creature: a hulking rock-construct humanoid,
//! with a real idle animation decoded from compiled-in GIF bytes.

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

/// Per-frame display time for the Stone Golem idle loop. The GIF's own frame
/// delays are ignored by `from_gif`; this is the uniform playback rate.
const STONE_GOLEM_FRAME_DUR: Duration = Duration::from_millis(80);

/// The bundled "Stone Golem" creature: name `"Stone Golem"`, idle
/// animation registered under [`crate::AnimationKind::Idle`], decoded from a
/// real multi-frame GIF bundled via `include_bytes!` (not synthetic frames).
pub fn stone_golem() -> Creature {
    let sprite = AnimatedSprite::from_gif(
        include_bytes!("../assets/creatures/stone_golem_idle.gif"),
        STONE_GOLEM_FRAME_DUR,
    )
    .expect("bundled stone_golem_idle.gif must decode");
    Creature::new("Stone Golem").with_animation(AnimationKind::Idle, sprite)
}

#[cfg(test)]
mod tests {
    use super::stone_golem;
    use crate::AnimationKind;

    /// The bundled Stone Golem creature is named correctly and carries a
    /// real, multi-frame idle animation decoded from the bundled GIF bytes
    /// (not a synthetic single-frame stand-in).
    #[test]
    fn stone_golem_has_named_idle_animation() {
        let c = stone_golem();
        assert_eq!(c.name(), "Stone Golem");
        let sprite = c
            .animation(AnimationKind::Idle)
            .expect("Stone Golem must have an Idle animation registered");
        assert!(
            sprite.frame_count() >= 2,
            "idle animation must be a real animated loop (>= 2 frames), got {}",
            sprite.frame_count()
        );
    }
}
