//! The MiniMax H3 image-to-video `RecipeBackend`: builds the locked
//! generation config (model/vae/llm/lora selection, cfg-scale, flow-shift,
//! strength, sampling, canvas, and CPU-offload flags) plus the
//! style-preservation + beat-driven prompt for a resolved `H3Job`. The
//! flat-background prompt clause here must always agree with the pre-clean
//! flatten color and the per-frame removal key the operation derives, since
//! all three consume the one `KeyColor` the operation resolves.

use std::path::PathBuf;

use super::model_paths::{ModelPathError, ModelPaths};
use super::recipe::{RecipeBackend, SdCliInvocation};
use super::types::KeyColor;

/// Style-preservation language every animation prompt carries so the shot
/// keeps the still's flat 2D cartoon look rather than drifting toward a
/// generic video-model style.
pub const STYLE_PRESERVATION: &str = "flat 2D cartoon illustration style with thick black outlines and cel-shaded coloring, camera locked static straight-on";

/// Consistency clause reinforcing that proportions, colors, and style stay
/// fixed across the whole shot.
pub const CONSISTENCY_CLAUSE: &str = "The creature's proportions, colors, and flat cel-shaded illustration style stay fully consistent and unchanged throughout the entire shot";

/// The Turbo LoRA prompt tag (a prompt tag, not a CLI flag), locked at
/// strength 1.0.
pub const LORA_TAG: &str = "<lora:minimax_h3_turbo_v4_step600_ema:1.0>";

/// The diffusion model filename resolved under the configured models dir.
const DIFFUSION_MODEL: &str = "minimax_h3_fl2va_pruned-Q4_K_M.gguf";
/// The VAE filename resolved under the configured models dir.
const VAE_MODEL: &str = "minimax_h3_video_vae_fp16.safetensors";
/// The audio VAE filename resolved under the configured models dir.
const AUDIO_VAE_MODEL: &str = "minimax_h3_audio_vae_fp32.safetensors";
/// The LLM (text encoder) filename resolved under the configured models dir.
const LLM_MODEL: &str = "qwen3vl_32b_minimax_h3-Q4_K_M.gguf";

/// The resolved per-clip inputs `generate_animation` hands the backend: the
/// pre-cleaned flat-background init image, the output path, the caller's
/// beat-by-beat prompt body, the key color the flat background and the
/// per-frame removal must agree on, and the request's frame count/fps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H3Job {
    pub init_img: PathBuf,
    pub output: PathBuf,
    pub prompt: String,
    pub key: KeyColor,
    pub frames: u32,
    pub fps: u32,
}

/// The MiniMax H3 image-to-video backend. Owns no execution; only the locked
/// config and prompt construction.
pub struct MiniMaxH3Backend;

impl RecipeBackend for MiniMaxH3Backend {
    type Request = H3Job;

    fn invocation(&self, job: &H3Job, models: &ModelPaths) -> Result<SdCliInvocation, ModelPathError> {
        let diffusion_model = models.resolve(DIFFUSION_MODEL)?;
        let vae = models.resolve(VAE_MODEL)?;
        let audio_vae = models.resolve(AUDIO_VAE_MODEL)?;
        let llm = models.resolve(LLM_MODEL)?;
        let loras_dir = models.resolve_loras_dir()?;

        let prompt = format!(
            "{}, {STYLE_PRESERVATION}. {CONSISTENCY_CLAUSE}. {}. {LORA_TAG}",
            job.prompt,
            h3_background_clause(&job.key)
        );

        Ok(SdCliInvocation {
            args: vec![
                "-M".to_string(),
                "vid_gen".to_string(),
                "--diffusion-model".to_string(),
                diffusion_model.to_string_lossy().into_owned(),
                "--vae".to_string(),
                vae.to_string_lossy().into_owned(),
                "--audio-vae".to_string(),
                audio_vae.to_string_lossy().into_owned(),
                "--llm".to_string(),
                llm.to_string_lossy().into_owned(),
                "--lora-model-dir".to_string(),
                loras_dir.to_string_lossy().into_owned(),
                "--cfg-scale".to_string(),
                "1.0".to_string(),
                "--flow-shift".to_string(),
                "12.0".to_string(),
                "--strength".to_string(),
                "1.0".to_string(),
                "--sampling-method".to_string(),
                "euler".to_string(),
                "--seed".to_string(),
                "42".to_string(),
                "-W".to_string(),
                "512".to_string(),
                "-H".to_string(),
                "512".to_string(),
                "--diffusion-fa".to_string(),
                "--offload-to-cpu".to_string(),
                "--rng".to_string(),
                "cpu".to_string(),
                "--clip-on-cpu".to_string(),
                "--vae-tiling".to_string(),
                "--temporal-tiling".to_string(),
                "--steps".to_string(),
                "8".to_string(),
                "--video-frames".to_string(),
                job.frames.to_string(),
                "--fps".to_string(),
                job.fps.to_string(),
                "--init-img".to_string(),
                job.init_img.to_string_lossy().into_owned(),
                "-p".to_string(),
                prompt,
                "-o".to_string(),
                job.output.to_string_lossy().into_owned(),
            ],
        })
    }
}

/// The flat-background prompt clause matching `key`: a green key gets the
/// green-screen phrase, anything else (magenta) gets the magenta phrase. Uses
/// the same strict-max-green rule `key_color_for`/`is_green_key` use, so this
/// clause can never disagree with the pre-clean flatten color or the
/// per-frame removal key.
fn h3_background_clause(key: &KeyColor) -> &'static str {
    if key.g > key.r && key.g > key.b {
        "The background remains a solid flat green screen color the whole time, no scenery, no camera movement"
    } else {
        "The background remains a solid flat magenta screen color the whole time, no scenery, no camera movement"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREEN: KeyColor = KeyColor { r: 0, g: 255, b: 0 };
    const MAGENTA: KeyColor = KeyColor {
        r: 255,
        g: 0,
        b: 255,
    };

    fn job(key: KeyColor, frames: u32, fps: u32) -> H3Job {
        H3Job {
            init_img: PathBuf::from("/tmp/abg_h3_init.png"),
            output: PathBuf::from("/tmp/abg_h3_out/anim.png"),
            prompt: "winds up by pulling its fist back, throws a punch, impact, brief follow-through"
                .to_string(),
            key,
            frames,
            fps,
        }
    }

    fn prompt_arg(inv: &SdCliInvocation) -> String {
        let i = inv.args.iter().position(|a| a == "-p").expect("-p arg present");
        inv.args[i + 1].clone()
    }

    /// The invocation carries the explicit `-M vid_gen` mode; every model
    /// flag (`--diffusion-model`/`--vae`/`--audio-vae`/`--llm`) followed by
    /// an absolute path under the resolver's configured dir;
    /// `--lora-model-dir` followed by the resolved `loras` dir; every other
    /// locked flag/value pair from the verified config; and `--init-img`/
    /// `-o` matching the job's paths.
    #[test]
    fn h3_invocation_has_mode_and_resolved_paths() {
        let backend = MiniMaxH3Backend;
        let j = job(GREEN, 56, 24);
        let models = ModelPaths::unchecked("/abs/models");
        let inv = backend.invocation(&j, &models).expect("resolver has every file");

        assert!(
            inv.args.windows(2).any(|w| w == ["-M", "vid_gen"]),
            "got args: {:?}",
            inv.args
        );

        let path_for = |flag: &str, filename: &str| {
            let idx = inv.args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("missing {flag} in {:?}", inv.args));
            let path = std::path::PathBuf::from(&inv.args[idx + 1]);
            assert!(path.is_absolute(), "{flag} value {path:?} must be absolute");
            assert_eq!(path, std::path::PathBuf::from("/abs/models").join(filename));
        };
        path_for("--diffusion-model", "minimax_h3_fl2va_pruned-Q4_K_M.gguf");
        path_for("--vae", "minimax_h3_video_vae_fp16.safetensors");
        path_for("--audio-vae", "minimax_h3_audio_vae_fp32.safetensors");
        path_for("--llm", "qwen3vl_32b_minimax_h3-Q4_K_M.gguf");
        path_for("--lora-model-dir", "loras");

        for pair in [
            ["--cfg-scale", "1.0"],
            ["--flow-shift", "12.0"],
            ["--strength", "1.0"],
            ["--sampling-method", "euler"],
            ["--seed", "42"],
            ["-W", "512"],
            ["-H", "512"],
            ["--steps", "8"],
            ["--rng", "cpu"],
        ] {
            assert!(
                inv.args.windows(2).any(|w| w == pair),
                "missing {pair:?} in {:?}",
                inv.args
            );
        }

        for flag in [
            "--diffusion-fa",
            "--offload-to-cpu",
            "--clip-on-cpu",
            "--vae-tiling",
            "--temporal-tiling",
        ] {
            assert!(
                inv.args.iter().any(|a| a == flag),
                "missing {flag} in {:?}",
                inv.args
            );
        }

        let init_idx = inv
            .args
            .iter()
            .position(|a| a == "--init-img")
            .expect("--init-img arg present");
        assert_eq!(inv.args[init_idx + 1], j.init_img.to_string_lossy());

        let o_idx = inv.args.iter().position(|a| a == "-o").expect("-o arg present");
        assert_eq!(inv.args[o_idx + 1], j.output.to_string_lossy());
    }

    /// A missing model file under the configured dir surfaces the typed
    /// resolver error naming it, never a bad argv.
    #[test]
    fn h3_invocation_missing_model_errors() {
        let dir = std::env::temp_dir().join(format!(
            "abg-test-h3-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let models = ModelPaths::from_dir_str(Some(dir.to_str().unwrap())).expect("dir exists");

        let backend = MiniMaxH3Backend;
        let err = backend
            .invocation(&job(GREEN, 56, 24), &models)
            .expect_err("no model files present");
        assert!(
            matches!(err, ModelPathError::MissingFile { .. } | ModelPathError::MissingDir { .. }),
            "expected a missing-file/dir resolver error, got {err:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `--video-frames`/`--fps` come from the job's `frames`/`fps`, not a
    /// hard-locked value.
    #[test]
    fn h3_video_frames_and_fps_from_request() {
        let backend = MiniMaxH3Backend;
        let models = ModelPaths::unchecked("/abs/models");
        let inv = backend.invocation(&job(GREEN, 56, 24), &models).expect("resolver has every file");
        assert!(
            inv.args.windows(2).any(|w| w == ["--video-frames", "56"]),
            "got args: {:?}",
            inv.args
        );
        assert!(
            inv.args.windows(2).any(|w| w == ["--fps", "24"]),
            "got args: {:?}",
            inv.args
        );
    }

    /// The prompt carries the style-preservation scaffold, the consistency
    /// clause, the LoRA tag, and the caller's beat-by-beat action text
    /// verbatim.
    #[test]
    fn h3_prompt_has_style_preservation_and_beats() {
        let backend = MiniMaxH3Backend;
        let j = job(GREEN, 56, 24);
        let models = ModelPaths::unchecked("/abs/models");
        let prompt = prompt_arg(&backend.invocation(&j, &models).expect("resolver has every file"));
        assert!(prompt.contains("flat 2D cartoon"), "got prompt: {prompt}");
        assert!(prompt.contains("camera locked static"), "got prompt: {prompt}");
        assert!(prompt.contains("thick black outlines"), "got prompt: {prompt}");
        assert!(prompt.contains("stay fully consistent"), "got prompt: {prompt}");
        assert!(
            prompt.contains("minimax_h3_turbo_v4_step600_ema:1.0"),
            "got prompt: {prompt}"
        );
        assert!(prompt.contains(&j.prompt), "got prompt: {prompt}");
    }

    /// A green key produces the green background clause and never mentions
    /// magenta; a magenta key produces the magenta clause.
    #[test]
    fn h3_prompt_background_clause_matches_key() {
        let backend = MiniMaxH3Backend;
        let models = ModelPaths::unchecked("/abs/models");

        let green_prompt = prompt_arg(&backend.invocation(&job(GREEN, 56, 24), &models).expect("ok"));
        assert!(
            green_prompt.contains("solid flat green screen"),
            "got prompt: {green_prompt}"
        );
        assert!(!green_prompt.contains("magenta"), "got prompt: {green_prompt}");

        let magenta_prompt = prompt_arg(&backend.invocation(&job(MAGENTA, 56, 24), &models).expect("ok"));
        assert!(
            magenta_prompt.contains("solid flat magenta screen"),
            "got prompt: {magenta_prompt}"
        );
    }
}
