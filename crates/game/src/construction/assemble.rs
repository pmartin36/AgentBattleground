//! Assembles a runtime `Creature` from a `ConstructionRequest`, composing
//! the sibling `allocate_stats`/`build_starting_attack` builders plus
//! (optional) art handles into the full creature.

use crate::asset_gen::types::{ClipAsset, ImageAsset};
use crate::construction::allocate::{allocate_stats, ConstructionRequest};
use crate::construction::attack::build_starting_attack;
use crate::creatures::Creature;

/// Builds a level-1 `Creature` from `request`: stats via `allocate_stats`,
/// exactly one ability via `build_starting_attack`, the request's name,
/// description, and element, default `Stamina`, and the passed art handles
/// (each independently optional — absent at definition time, filled in
/// after incubation). Deterministic: the same `request` and handles always
/// produce an equal `Creature`.
pub fn construct_creature(
    request: &ConstructionRequest,
    still: Option<ImageAsset>,
    idle: Option<ClipAsset>,
    attack_clip: Option<ClipAsset>,
) -> Creature {
    let stats = allocate_stats(request.weighting(), request.seed());
    let ability =
        build_starting_attack(request.archetype(), &stats, request.element(), request.seed());
    Creature::new(request.name())
        .with_description(request.description())
        .with_level(1)
        .with_stats(stats)
        .with_abilities(vec![ability])
        .with_element(request.element())
        .with_art_handles(still, idle, attack_clip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::Element;
    use crate::construction::allocate::{allocate_stats, StartingArchetype, StatWeighting};
    use crate::construction::attack::build_starting_attack;
    use crate::stamina::Stamina;
    use std::path::PathBuf;

    fn lopsided_request() -> ConstructionRequest {
        ConstructionRequest::new(
            "Emberling",
            "a small ember spirit",
            StatWeighting { strength: 5.0, dexterity: 1.0, intelligence: 1.0, vitality: 1.0 },
            StartingArchetype::Melee,
            Element::Fire,
            42,
        )
    }

    fn art_handles() -> (ImageAsset, ClipAsset, ClipAsset) {
        (
            ImageAsset { path: PathBuf::from("emberling/still.png") },
            ClipAsset { frames: vec![PathBuf::from("emberling/idle_0.png")] },
            ClipAsset { frames: vec![PathBuf::from("emberling/attack_0.png")] },
        )
    }

    /// The assembled creature is level 1, carries the request's element, and
    /// has exactly one ability.
    #[test]
    fn construct_sets_level_1_element_and_single_ability() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        assert_eq!(creature.level(), 1);
        assert_eq!(creature.element(), Element::Fire);
        assert_eq!(creature.abilities().len(), 1);
    }

    /// The assembled ability equals a direct `build_starting_attack` call
    /// on the same request's allocated stats — the ability is the sibling
    /// builder's output, not hand-rolled.
    #[test]
    fn construct_ability_equals_build_starting_attack() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        let stats = allocate_stats(request.weighting(), request.seed());
        let expected =
            build_starting_attack(request.archetype(), &stats, request.element(), request.seed());
        assert_eq!(creature.abilities()[0], expected);
    }

    /// The assembled stats equal a direct `allocate_stats` call on the same
    /// request's weighting and seed.
    #[test]
    fn construct_stats_equal_allocate_stats() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        let expected = allocate_stats(request.weighting(), request.seed());
        assert_eq!(*creature.stats(), expected);
    }

    /// The assembled creature's name and description match the request.
    #[test]
    fn construct_name_and_description_from_request() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        assert_eq!(creature.name(), "Emberling");
        assert_eq!(creature.description(), "a small ember spirit");
    }

    /// The assembled creature starts with a full, non-injured `Stamina`.
    #[test]
    fn construct_default_stamina_full_not_injured() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        assert_eq!(*creature.stamina(), Stamina::default());
        assert!(!creature.stamina().is_injured());
    }

    /// Passed `Some` art handles are threaded onto the creature's three
    /// accessors.
    #[test]
    fn construct_threads_some_art_handles() {
        let request = lopsided_request();
        let (still, idle, attack) = art_handles();
        let creature =
            construct_creature(&request, Some(still.clone()), Some(idle.clone()), Some(attack.clone()));
        assert_eq!(creature.still_handle(), Some(&still));
        assert_eq!(creature.idle_handle(), Some(&idle));
        assert_eq!(creature.attack_handle(), Some(&attack));
    }

    /// Passing `None` for every art handle leaves all three accessors
    /// `None` (the definition-time state, before incubation generates art).
    #[test]
    fn construct_threads_none_art_handles() {
        let request = lopsided_request();
        let creature = construct_creature(&request, None, None, None);
        assert_eq!(creature.still_handle(), None);
        assert_eq!(creature.idle_handle(), None);
        assert_eq!(creature.attack_handle(), None);
    }

    /// Two calls with the same request and same handles produce a creature
    /// equal on every accessor (the spec's central reproducibility
    /// guarantee). `Creature` does not derive `PartialEq`, so this compares
    /// field-by-field through the accessors.
    #[test]
    fn construct_is_reproducible() {
        let request = lopsided_request();
        let (still, idle, attack) = art_handles();
        let a = construct_creature(&request, Some(still.clone()), Some(idle.clone()), Some(attack.clone()));
        let b = construct_creature(&request, Some(still.clone()), Some(idle.clone()), Some(attack.clone()));

        assert_eq!(a.name(), b.name());
        assert_eq!(a.description(), b.description());
        assert_eq!(a.level(), b.level());
        assert_eq!(a.stats(), b.stats());
        assert_eq!(a.abilities(), b.abilities());
        assert_eq!(a.element(), b.element());
        assert_eq!(a.stamina(), b.stamina());
        assert_eq!(a.still_handle(), b.still_handle());
        assert_eq!(a.idle_handle(), b.idle_handle());
        assert_eq!(a.attack_handle(), b.attack_handle());
    }
}
