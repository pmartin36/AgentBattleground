//! Request/response data contracts for the asset-generation API. These are
//! the shapes callers build to ask for a generated image or animation clip,
//! and the filesystem handles they get back.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A request to generate (or import) a single still image asset.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    pub fidelity: Fidelity,
    pub seed: u64,
    pub background_key: KeyColor,
    pub import_path: Option<PathBuf>,
}

/// Generation fidelity: a quick draft pass or a higher-quality final pass.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Fidelity {
    Draft,
    High,
}

/// A background key color used for chroma-key style background removal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A request to generate an animation clip for a given action, anchored to
/// an already-generated still image.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimationRequest {
    pub action: String,
    pub prompt: String,
    pub params: ClipParams,
}

/// Per-clip generation parameters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClipParams {
    pub frames: u32,
    pub fps: u32,
}

/// A handle to a cached, background-removed still image asset on disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImageAsset {
    pub path: PathBuf,
}

/// A handle to a cached PNG frame-sequence on disk, in playback order.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClipAsset {
    pub frames: Vec<PathBuf>,
}
