//! Persisted player save schema: the serializable root (`PlayerData`) and
//! its members, embedded directly from the domain data types rather than
//! duplicating them.

pub mod convert;
pub mod schema;
pub mod store;

pub use convert::{apply_persisted_rpg, creature_from_persisted, creature_to_persisted};
pub use schema::{Egg, EggState, PersistedCreature, PlayerData};
pub use store::{Loaded, PlayerStore};
