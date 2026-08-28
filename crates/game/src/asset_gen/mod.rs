//! Asset-generation orchestration API: request/response contracts, the
//! recipe-backend dispatch seam, the async job lifecycle, and the keyed
//! result cache behind the game's `generate_image`/`generate_animation`
//! operations.

pub mod cache;
pub mod capability;
pub mod job;
pub mod recipe;
pub mod runner;
pub mod types;

pub use cache::AssetCache;
pub use capability::{capability, GpuCapability};
pub use job::{JobHandle, JobQueue, JobStatus};
pub use recipe::{RecipeBackend, SdCliInvocation};
pub use runner::{CancelFlag, JobError, JobRunner, RunOutput, SdCliRunner, POLL_INTERVAL};
pub use types::{
    AnimationRequest, ClipAsset, ClipParams, Fidelity, ImageAsset, ImageRequest, KeyColor,
};
