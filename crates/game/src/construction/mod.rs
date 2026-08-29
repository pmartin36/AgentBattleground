//! Deterministic creature construction: turns a `ConstructionRequest`
//! (weighting, archetype, element, seed) into a fully-assembled runtime
//! `Creature` via stat allocation and starting-attack building.

pub mod allocate;
pub mod assemble;
pub mod attack;

pub use allocate::{
    allocate_stats, ConstructionRequest, StartingArchetype, StatWeighting, STAT_BUDGET, STAT_CAP,
    STAT_FLOOR,
};
pub use assemble::construct_creature;
pub use attack::{
    build_starting_attack, BUFF_MAGNITUDE_RANGE, DEBUFF_MAGNITUDE_RANGE, MELEE_DAMAGE_RANGE,
    RANGED_DAMAGE_RANGE,
};
