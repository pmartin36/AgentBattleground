//! Live end-to-end proof: real `sd-cli` binary + real local models + real
//! `ffmpeg`, driven through the game's own `AssetGen` (real `SdCliRunner`,
//! real `ZImageBackend`, real `ModelPaths` resolver, default `FfmpegExtractor`
//! extraction). `#[ignore]`d because it spawns a multi-GB-model subprocess,
//! so it never runs in the offline gate.
//!
//! Run it with:
//!   cargo test -p game --lib live_asset_gen -- --ignored --nocapture
//!
//! It locates the sd-cli binary from `AGENTBATTLEGROUND_SDCLI_BIN` or an
//! in-repo build, and the models directory from
//! `AGENTBATTLEGROUND_SDCLI_MODELS_DIR` (`ModelPaths::from_env`). It proves
//! the corrected argv + resolver + frame-extraction fix: `generate_image`
//! yields a non-empty still PNG, and `generate_animation` yields a PNG frame
//! sequence with more than one frame.
#![cfg(test)]

use std::path::PathBuf;
use std::sync::Arc;

use super::backend_image::ZImageBackend;
use super::capability::GpuCapability;
use super::job::JobStatus;
use super::model_paths::ModelPaths;
use super::operations::AssetGen;
use super::runner::SdCliRunner;
use super::types::{AnimationRequest, ClipParams, Fidelity, ImageRequest, KeyColor};

/// Env var overriding the sd-cli binary location, for family-consistency
/// with `AGENTBATTLEGROUND_SDCLI_MODELS_DIR`.
const ENV_SDCLI_BIN: &str = "AGENTBATTLEGROUND_SDCLI_BIN";

/// The real sd-cli binary + real configured models dir, from env overrides
/// or in-repo build locations. `None` (skip) if either is absent.
fn binary_and_models() -> Option<(PathBuf, ModelPaths)> {
    let bin = std::env::var(ENV_SDCLI_BIN)
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            Some(PathBuf::from(
                "experiments/creature_lab/stable-diffusion.cpp/build-vk/bin/sd-cli",
            ))
        })
        .filter(|p| p.exists())?;
    let models = ModelPaths::from_env().ok()?;
    Some((bin, models))
}

#[test]
#[ignore = "requires the real sd-cli binary + local models + ffmpeg; run with --ignored"]
fn live_asset_gen_generates_image_and_animation() {
    let Some((bin, models)) = binary_and_models() else {
        panic!(
            "sd-cli binary and/or models directory not found; set {ENV_SDCLI_BIN} and \
             AGENTBATTLEGROUND_SDCLI_MODELS_DIR, or place sd-cli under \
             experiments/creature_lab/stable-diffusion.cpp/build-vk/bin/"
        );
    };

    let asset_gen = AssetGen::new(
        Arc::new(SdCliRunner::with_bin(bin)),
        Box::new(ZImageBackend),
        GpuCapability::Available,
        models,
    );

    let image_request = ImageRequest {
        prompt: "a fierce armored beetle that channels lightning through its horns".to_string(),
        fidelity: Fidelity::Draft,
        seed: 1,
        background_key: KeyColor { r: 0, g: 255, b: 0 },
        import_path: None,
    };

    let still = match asset_gen.generate_image(image_request).wait() {
        JobStatus::Success(asset) => asset,
        other => panic!("image generation failed: {other:?}"),
    };
    let still_len = std::fs::metadata(&still.path)
        .unwrap_or_else(|e| panic!("generated still {:?} must exist: {e}", still.path))
        .len();
    eprintln!("--- generated still: {:?} ({still_len} bytes) ---", still.path);
    assert!(still_len > 0, "generated still PNG must be non-empty");

    let animation_request = AnimationRequest {
        action: "attack".to_string(),
        prompt: "the beetle lunges forward, horns crackling with lightning".to_string(),
        params: ClipParams { frames: 8, fps: 8 },
    };

    let clip = match asset_gen.generate_animation(&still, animation_request).wait() {
        JobStatus::Success(clip) => clip,
        other => panic!("animation generation failed: {other:?}"),
    };
    eprintln!(
        "--- generated clip: {} frames: {:?} ---",
        clip.frames.len(),
        clip.frames
    );
    assert!(
        clip.frames.len() > 1,
        "generated clip must have more than one frame, got {}",
        clip.frames.len()
    );
    for frame in &clip.frames {
        let len = std::fs::metadata(frame)
            .unwrap_or_else(|e| panic!("generated frame {frame:?} must exist: {e}"))
            .len();
        assert!(len > 0, "generated frame {frame:?} must be non-empty");
    }
}
