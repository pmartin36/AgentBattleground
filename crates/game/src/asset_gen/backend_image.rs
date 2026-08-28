//! The Z-Image Turbo text-to-image `RecipeBackend`: builds the model choice,
//! locked generation flags, and cartoony/flat-saturated style-guided prompt
//! (including the chroma-key screen clause matching the request's key color).
//! Also owns the ONE place the key-color selection rule lives,
//! `key_color_for`, so callers cannot bypass it.

use super::recipe::{RecipeBackend, SdCliInvocation};
use super::types::{Fidelity, ImageRequest, KeyColor};

/// Style guidance appended to every generated-image prompt: legible at
/// braille resolution, flat/saturated, avoiding heavy dark low-contrast
/// fields.
pub const STYLE_GUIDANCE: &str = "flat 2D cartoon illustration style, thick black outlines, cel-shaded flat colors, vivid saturated colors, simple low-detail creature, high-contrast clear silhouette, no dark low-contrast fields";

/// The Z-Image Turbo backend: fixed model, locked cfg/steps flags, and a
/// style + key-color-steered prompt. Owns no execution.
pub struct ZImageBackend;

impl RecipeBackend for ZImageBackend {
    type Request = ImageRequest;

    fn invocation(&self, request: &ImageRequest) -> SdCliInvocation {
        let (width, height) = fidelity_dims(&request.fidelity);
        let prompt = format!(
            "{}, {}, {}",
            request.prompt,
            STYLE_GUIDANCE,
            key_screen_clause(&request.background_key)
        );
        let out_path = super::operations::image_raw_path(request);
        SdCliInvocation {
            model: "z_image_turbo".to_string(),
            args: vec![
                "--cfg-scale".to_string(),
                "1.0".to_string(),
                "--steps".to_string(),
                "8".to_string(),
                "--diffusion-fa".to_string(),
                "--seed".to_string(),
                request.seed.to_string(),
                "--width".to_string(),
                width.to_string(),
                "--height".to_string(),
                height.to_string(),
                "-p".to_string(),
                prompt,
                "-o".to_string(),
                out_path.to_string_lossy().into_owned(),
            ],
        }
    }
}

/// Render dimensions for a given generation fidelity: a quick low-res draft
/// pass, or a higher-resolution final pass.
fn fidelity_dims(fidelity: &Fidelity) -> (u32, u32) {
    match fidelity {
        Fidelity::Draft => (512, 512),
        Fidelity::High => (768, 768),
    }
}

/// Whether `key`'s green channel is the strict-max channel, i.e. `key` is
/// itself a green key rather than a magenta one. The same strong-channel
/// shape `bg_removal` uses, so the prompt's screen phrase and the removal
/// key can never disagree.
fn is_green_key(key: &KeyColor) -> bool {
    key.g > key.r && key.g > key.b
}

/// The chroma-key screen clause matching `key`: a green key gets the green
/// screen phrase, anything else (magenta) gets the magenta phrase.
fn key_screen_clause(key: &KeyColor) -> &'static str {
    if is_green_key(key) {
        "solid flat vivid chroma-key green background, uniform bright green screen backdrop, no scenery, no shadow"
    } else {
        "solid flat vivid chroma-key magenta background, uniform bright magenta screen backdrop, no scenery, no shadow"
    }
}

/// The one place the key-color selection rule lives: green by default,
/// magenta when `dominant` is green-family (green is the strict-max
/// channel), so keying can still separate a green subject from its screen.
/// `dominant` is a required argument so a caller cannot silently skip the
/// rule.
pub fn key_color_for(dominant: [u8; 3]) -> KeyColor {
    let [r, g, b] = dominant;
    if g > r && g > b {
        KeyColor { r: 255, g: 0, b: 255 }
    } else {
        KeyColor { r: 0, g: 255, b: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_gen::operations::image_raw_path;
    use crate::asset_gen::types::Fidelity;

    const GREEN: KeyColor = KeyColor { r: 0, g: 255, b: 0 };
    const MAGENTA: KeyColor = KeyColor {
        r: 255,
        g: 0,
        b: 255,
    };

    fn req(background_key: KeyColor) -> ImageRequest {
        ImageRequest {
            prompt: "a small dragon".to_string(),
            fidelity: Fidelity::Draft,
            seed: 7,
            background_key,
            import_path: None,
        }
    }

    fn prompt_arg(inv: &SdCliInvocation) -> String {
        let i = inv.args.iter().position(|a| a == "-p").expect("-p arg present");
        inv.args[i + 1].clone()
    }

    /// The invocation's prompt carries the style-guidance language, so the
    /// generated art stays legible and flat/saturated for braille render.
    #[test]
    fn zimage_prompt_has_style_guidance() {
        let backend = ZImageBackend;
        let inv = backend.invocation(&req(GREEN));
        let prompt = prompt_arg(&inv);
        assert!(prompt.contains("flat 2D cartoon"), "got prompt: {prompt}");
        assert!(prompt.contains("cel-shaded"), "got prompt: {prompt}");
        assert!(prompt.contains("thick black outlines"), "got prompt: {prompt}");
    }

    /// A green-key request produces a green screen clause, never a magenta
    /// one.
    #[test]
    fn zimage_prompt_green_key_default() {
        let backend = ZImageBackend;
        let inv = backend.invocation(&req(GREEN));
        let prompt = prompt_arg(&inv);
        assert!(prompt.contains("green"), "got prompt: {prompt}");
        assert!(!prompt.contains("magenta"), "got prompt: {prompt}");
    }

    /// A magenta-key request produces a magenta screen clause.
    #[test]
    fn zimage_prompt_magenta_for_magenta_key() {
        let backend = ZImageBackend;
        let inv = backend.invocation(&req(MAGENTA));
        let prompt = prompt_arg(&inv);
        assert!(prompt.contains("magenta"), "got prompt: {prompt}");
    }

    /// `key_color_for` selects green by default and magenta for a
    /// green-family dominant color.
    #[test]
    fn key_color_for_defaults_green() {
        assert_eq!(key_color_for([200, 60, 40]), GREEN);
    }

    #[test]
    fn key_color_for_green_family_selects_magenta() {
        assert_eq!(key_color_for([40, 200, 60]), MAGENTA);
    }

    /// The model name, locked cfg/steps flags, and the shared deterministic
    /// output-path helper all appear in the invocation.
    #[test]
    fn zimage_invocation_locks_model_and_flags() {
        let backend = ZImageBackend;
        let request = req(GREEN);
        let inv = backend.invocation(&request);
        assert_eq!(inv.model, "z_image_turbo");
        assert!(
            inv.args.windows(2).any(|w| w == ["--cfg-scale", "1.0"]),
            "got args: {:?}",
            inv.args
        );
        assert!(
            inv.args.windows(2).any(|w| w == ["--steps", "8"]),
            "got args: {:?}",
            inv.args
        );
        let o_idx = inv.args.iter().position(|a| a == "-o").expect("-o arg present");
        assert_eq!(
            inv.args[o_idx + 1],
            image_raw_path(&request).to_string_lossy(),
            "-o must match the shared deterministic raw-output path"
        );
    }
}
