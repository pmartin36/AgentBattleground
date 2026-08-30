//! Two-way mapping between the runtime `Creature` (holds decoded
//! `AnimatedSprite`s, not itself `Serialize`) and the persisted
//! `PersistedCreature` (RPG data plus art handles, no decoded sprites).
//! Save drops sprites and writes handles; load reads handles and resolves
//! the idle clip back into a runtime `AnimatedSprite`.

use crate::asset_gen::types::ClipAsset;
use crate::creatures::{AnimationKind, Creature, FRAME_DUR};
use crate::player_data::schema::PersistedCreature;
use engine_render::AnimatedSprite;

/// Builds a `PersistedCreature` from a runtime `Creature`: copies the RPG
/// data and the three art handles verbatim, dropping any decoded sprites.
pub fn creature_to_persisted(creature: &Creature) -> PersistedCreature {
    PersistedCreature::new(
        creature.name().to_string(),
        creature.element(),
        *creature.stats(),
        creature.level(),
        creature.xp(),
        creature.abilities().to_vec(),
        *creature.stamina(),
        creature.still_handle().cloned(),
        creature.idle_handle().cloned(),
        creature.attack_handle().cloned(),
    )
}

/// Overlays a `PersistedCreature`'s RPG fields and art handles onto `base`,
/// returning the updated creature. The single builder chain shared by
/// `creature_from_persisted` and roster hydration's bundled-sprite overlay
/// (which starts from a bundled `Creature`, not `Creature::new`, so the
/// bundled sprite is moved rather than discarded).
pub fn apply_persisted_rpg(base: Creature, p: &PersistedCreature) -> Creature {
    base.with_stats(p.stats)
        .with_level(p.level)
        .with_xp(p.xp)
        .with_abilities(p.abilities.clone())
        .with_stamina(p.stamina)
        .with_element(p.element)
        .with_art_handles(p.still.clone(), p.idle.clone(), p.attack.clone())
}

/// Builds a runtime `Creature` from a `PersistedCreature`: restores the RPG
/// data and the three art handles, and — when an idle handle is present —
/// decodes its frames into an `AnimationKind::Idle` sprite.
pub fn creature_from_persisted(persisted: &PersistedCreature) -> Creature {
    let mut creature = apply_persisted_rpg(Creature::new(persisted.name.clone()), persisted);

    if let Some(idle) = &persisted.idle {
        if let Some(sprite) = resolve_clip(idle) {
            creature = creature.with_animation(AnimationKind::Idle, sprite);
        }
    }

    creature
}

/// Decodes each frame path in `clip` into a `DynamicImage` and assembles an
/// `AnimatedSprite` at the shared bundled-idle frame rate. Returns `None` for
/// an empty clip or when any frame fails to decode (logged via
/// `tracing::warn!`) — the caller leaves the animation unresolved rather than
/// panicking.
pub(crate) fn resolve_clip(clip: &ClipAsset) -> Option<AnimatedSprite> {
    if clip.frames.is_empty() {
        return None;
    }

    let mut frames = Vec::with_capacity(clip.frames.len());
    for path in &clip.frames {
        match image::open(path) {
            Ok(frame) => frames.push(frame),
            Err(e) => {
                tracing::warn!("failed to decode clip frame at {path:?}: {e}");
                return None;
            }
        }
    }

    Some(AnimatedSprite::new(frames, FRAME_DUR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::{AbilityType, DamageClass, Element, Modifier, StatusKind};
    use crate::asset_gen::types::{ClipAsset, ImageAsset};
    use crate::stamina::Stamina;
    use crate::stats::Stats;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;
    use std::time::Duration;

    fn sample_ability() -> crate::ability::Ability {
        crate::ability::Ability::new(
            "Ember Claw",
            vec![Modifier { name: "Overheat".to_string(), requires: None }],
        )
        .with_ability_type(AbilityType::Attack)
        .with_element(Element::Fire)
        .with_class(DamageClass::Magic)
        .with_cost(4)
        .with_damage(22)
        .with_range(2)
        .with_status_effects(vec![StatusKind::Burn])
        .with_flavor("Scorches the target.")
    }

    fn sample_persisted(
        still: Option<ImageAsset>,
        idle: Option<ClipAsset>,
        attack: Option<ClipAsset>,
    ) -> PersistedCreature {
        PersistedCreature::new(
            "Emberling",
            Element::Fire,
            Stats { strength: 12, dexterity: 9, intelligence: 18, vitality: 14 },
            5,
            340,
            vec![sample_ability()],
            Stamina::new(40, 60),
            still,
            idle,
            attack,
        )
    }

    /// A full `Creature` -> `PersistedCreature` -> `Creature` round trip
    /// preserves every RPG field by equality against the source persisted
    /// data (name/element/stats/level/xp/abilities/stamina).
    #[test]
    fn rpg_fields_round_trip_through_conversion() {
        let source = sample_persisted(None, None, None);

        let creature = creature_from_persisted(&source);
        let round_tripped = creature_to_persisted(&creature);

        assert_eq!(round_tripped.name, source.name);
        assert_eq!(round_tripped.element, source.element);
        assert_eq!(round_tripped.stats, source.stats);
        assert_eq!(round_tripped.level, source.level);
        assert_eq!(round_tripped.xp, source.xp);
        assert_eq!(round_tripped.abilities, source.abilities);
        assert_eq!(round_tripped.stamina, source.stamina);
    }

    /// When the source carries real art handles, a round trip through the
    /// runtime `Creature` preserves exactly those handles — no fabricated
    /// or dropped handle.
    #[test]
    fn art_handles_round_trip_through_conversion() {
        let still = ImageAsset { path: PathBuf::from("emberling/still.png") };
        let idle = ClipAsset {
            frames: vec![PathBuf::from("emberling/idle_0.png"), PathBuf::from("emberling/idle_1.png")],
        };
        let attack = ClipAsset { frames: vec![PathBuf::from("emberling/attack_0.png")] };
        let source = sample_persisted(Some(still.clone()), Some(idle.clone()), Some(attack.clone()));

        let creature = creature_from_persisted(&source);
        let round_tripped = creature_to_persisted(&creature);

        assert_eq!(round_tripped.still, Some(still));
        assert_eq!(round_tripped.idle, Some(idle));
        assert_eq!(round_tripped.attack, Some(attack));
    }

    /// Writes a 2-frame PNG clip (frame 0 solid red, frame 1 solid green) to
    /// a unique temp dir and returns it as a `ClipAsset`.
    fn two_frame_clip(tag: &str) -> ClipAsset {
        let dir = std::env::temp_dir().join(format!("abg_player_data_convert_test_{tag}"));
        std::fs::create_dir_all(&dir).unwrap();

        let frame0 = dir.join("idle_0.png");
        let mut img0 = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        img0.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img0.save(&frame0).unwrap();

        let frame1 = dir.join("idle_1.png");
        let img1 = RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255]));
        img1.save(&frame1).unwrap();

        ClipAsset { frames: vec![frame0, frame1] }
    }

    /// Loading a `PersistedCreature` with a real idle clip resolves it into
    /// an `AnimationKind::Idle` sprite whose frame count matches the clip
    /// and whose first frame decodes to the actual red pixel data on disk —
    /// not merely "a sprite exists".
    #[test]
    fn idle_clip_resolves_into_matching_sprite() {
        let idle = two_frame_clip("sprite_resolution");
        let source = sample_persisted(None, Some(idle.clone()), None);

        let creature = creature_from_persisted(&source);

        let sprite = creature
            .animation(AnimationKind::Idle)
            .expect("idle handle present must resolve into an Idle animation");
        assert_eq!(sprite.frame_count(), idle.frames.len());

        let first_frame = sprite.frame_at(Duration::ZERO);
        let pixel = first_frame.to_rgba8().get_pixel(0, 0).0;
        assert_eq!(pixel, [255, 0, 0, 255], "first frame must decode to the red pixel written to disk");
    }

    /// A handle-less `PersistedCreature` (the bundled/first-run-seed shape)
    /// converts without panicking, keeps its RPG data intact, and produces
    /// no Idle animation (there is nothing to resolve).
    #[test]
    fn handle_less_persisted_creature_converts_without_panic() {
        let source = sample_persisted(None, None, None);

        let creature = creature_from_persisted(&source);

        assert_eq!(creature.name(), source.name);
        assert!(creature.animation(AnimationKind::Idle).is_none());
    }

    /// Converting a bundled, handle-less runtime `Creature` (built with the
    /// default `Creature::new`, no art handles ever set) to persisted form
    /// yields all-`None` handles — conversion never fabricates a handle
    /// that was never assigned.
    #[test]
    fn handle_less_runtime_creature_persists_with_no_handles() {
        let creature = Creature::new("Handleless");

        let persisted = creature_to_persisted(&creature);

        assert_eq!(persisted.still, None);
        assert_eq!(persisted.idle, None);
        assert_eq!(persisted.attack, None);
    }
}
