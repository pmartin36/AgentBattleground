//! Inspector spawn / sibling-discovery — relocated to `scene_core::net::inspect`
//! (spec 31 Phase B bucket b5). This shim keeps `crate::inspect::*` resolving
//! for existing game-space callers until Bucket b6 retargets them directly.
pub use scene_core::net::inspect::*;
