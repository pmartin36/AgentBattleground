//! The 6 creatures bundled into the binary this round, each a real
//! multi-frame idle GIF decoded via `include_bytes!` — not synthetic frames.
//! One `bundled_creature!` invocation per creature replaces what used to be
//! 6 near-identical files (name, GIF path, and function identifier are the
//! only things that ever differed between them).

use crate::{AnimatedSprite, AnimationKind, Creature};
use std::time::Duration;

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

bundled_creature!(ember_wolf, "Ember Wolf", "../assets/creatures/ember_wolf_idle.gif");
bundled_creature!(frost_lizard, "Frost Lizard", "../assets/creatures/frost_lizard_idle.gif");
bundled_creature!(stone_golem, "Stone Golem", "../assets/creatures/stone_golem_idle.gif");
bundled_creature!(storm_hawk, "Storm Hawk", "../assets/creatures/storm_hawk_idle.gif");
bundled_creature!(verdant_treant, "Verdant Treant", "../assets/creatures/verdant_treant_idle.gif");
bundled_creature!(shadow_cat, "Shadow Cat", "../assets/creatures/shadow_cat_idle.gif");

#[cfg(test)]
mod tests {
    use super::*;

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
}
