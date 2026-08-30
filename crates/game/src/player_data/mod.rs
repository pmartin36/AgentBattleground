//! Persisted player save schema: the serializable root (`PlayerData`) and
//! its members, embedded directly from the domain data types rather than
//! duplicating them.

pub mod convert;
pub mod schema;
pub mod store;

pub use convert::{apply_persisted_rpg, creature_from_persisted, creature_to_persisted};
pub(crate) use convert::resolve_clip;
pub use schema::{Egg, EggState, PersistedCreature, PlayerData};
pub use store::{Loaded, PlayerStore};

/// The canonical first-run save: the demo roster plus a handful of starter
/// eggs. Both the roster and hatchery load paths seed from this identical
/// value, so the two scenes can never write disagreeing saves to the shared
/// file (whichever loads first now writes the same content).
pub fn default_seed() -> PlayerData {
    PlayerData {
        roster: crate::creatures::demo_roster()
            .iter()
            .map(creature_to_persisted)
            .collect(),
        eggs: starter_eggs(),
    }
}

/// A few undefined starter eggs of varied elements, so a fresh Hatchery tray
/// has eggs to define rather than showing nothing. How a player acquires
/// further eggs is a separate concern (see `65-hatchery`).
fn starter_eggs() -> Vec<Egg> {
    use crate::ability::Element;
    [Element::Fire, Element::Ice, Element::Earth, Element::Normal]
        .into_iter()
        .map(|element| Egg {
            element,
            state: EggState::Undefined,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        })
        .collect()
}
