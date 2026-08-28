//! Asset-generation orchestration API: request/response contracts, the
//! recipe-backend dispatch seam, the async job lifecycle, and the keyed
//! result cache behind the game's `generate_image`/`generate_animation`
//! operations.

pub mod backend_animation;
pub mod backend_image;
pub mod bg_removal;
pub mod cache;
pub mod capability;
pub mod compose;
pub mod job;
pub mod operations;
pub mod preset;
pub mod recipe;
pub mod runner;
pub mod types;

pub use backend_animation::{MiniMaxH3Backend, CONSISTENCY_CLAUSE, H3Job, LORA_TAG, STYLE_PRESERVATION};
pub use backend_image::{key_color_for, ZImageBackend, STYLE_GUIDANCE};
pub use bg_removal::{
    remove_frame_background, remove_still_background, BackgroundRemover, ChromaDespill,
};
pub use cache::AssetCache;
pub use capability::{capability, GpuCapability};
pub use compose::{AnimationSetHandle, ImageWithAnimations};
pub use job::{JobHandle, JobQueue, JobStatus};
pub use operations::{resolve_status, AssetError, AssetGen, DEFAULT_JOB_TIMEOUT};
pub use preset::{CreatureSpec, CREATURE_FRAMING, DEFAULT_CREATURE_ACTIONS};
pub use recipe::{RecipeBackend, SdCliInvocation};
pub use runner::{CancelFlag, JobError, JobRunner, RunOutput, SdCliRunner, POLL_INTERVAL};
pub use types::{
    AnimationRequest, ClipAsset, ClipParams, Fidelity, ImageAsset, ImageRequest, KeyColor,
};
