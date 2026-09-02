//! `generate_creature`: this game's convenience wrapper over
//! `AssetGen::generate_image_with_animations`, supplying the default
//! idle/attack/hatch action set and this game's creature-specific prompt
//! conventions. The shared flat-cartoon style language is owned once by the
//! backends (`STYLE_GUIDANCE`/`STYLE_PRESERVATION`) and appended downstream
//! to every prompt; this preset does not re-embed it, so it only supplies
//! the subject framing and the beat-by-beat action text.

use std::path::PathBuf;

use super::backend_image::key_color_for;
use super::compose::AnimationSetHandle;
use super::operations::AssetGen;
use super::types::{AnimationRequest, ClipParams, Fidelity, ImageRequest};

/// Caller-facing parameters for generating a full creature asset set (still
/// plus the default idle/attack/hatch clips).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatureSpec {
    pub description: String,
    pub seed: u64,
    pub fidelity: Fidelity,
    pub dominant_color: [u8; 3],
    pub import_path: Option<PathBuf>,
}

/// Subject-framing clause the backends do not supply on their own: a single
/// full-body, centered creature subject.
pub const CREATURE_FRAMING: &str =
    "a single creature character, full body, centered, isolated subject";

/// The default action set every creature generates, in this order.
pub const DEFAULT_CREATURE_ACTIONS: [&str; 3] = ["idle", "attack", "hatch"];

struct ActionTemplate {
    action: &'static str,
    beats: &'static str,
}

const ACTION_TEMPLATES: [ActionTemplate; 3] = [
    ActionTemplate {
        action: "idle",
        beats: "settles in place and breathes: its body slowly rises and falls, it shifts its weight and gives a small bob, then eases back to rest",
    },
    ActionTemplate {
        action: "attack",
        beats: "winds up by drawing back and coiling, lunges forward into a committed strike, connects at full extension on impact, then recoils into a brief follow-through",
    },
    ActionTemplate {
        action: "hatch",
        beats: "trembles as a crack spreads across it, breaks open as the creature emerges and pushes upright, gives a quick shake, then settles into a resting pose",
    },
];

const DEFAULT_CLIP_PARAMS: ClipParams = ClipParams { frames: 56, fps: 24 };

/// Builds the one `ImageRequest` plus the three default-action
/// `AnimationRequest`s for a creature spec: the image carries the
/// description, the creature-framing clause, and the key color selected via
/// `key_color_for`; each animation carries the description and its
/// action's beat-by-beat body.
pub(in crate::asset_gen) fn creature_requests(
    spec: &CreatureSpec,
) -> (ImageRequest, Vec<AnimationRequest>) {
    let image = ImageRequest {
        prompt: format!("{}, {CREATURE_FRAMING}", spec.description),
        fidelity: spec.fidelity.clone(),
        seed: spec.seed,
        background_key: key_color_for(spec.dominant_color),
        import_path: spec.import_path.clone(),
    };

    let anims = ACTION_TEMPLATES
        .iter()
        .map(|template| {
            animation_request(template.action, &spec.description)
                .expect("action comes from ACTION_TEMPLATES itself")
        })
        .collect();

    (image, anims)
}

/// Looks up `action` in `ACTION_TEMPLATES` and builds the `AnimationRequest`
/// for it: `description`'s beats, prompt, and the shared default clip
/// params. `None` for an action with no template. The single authoring
/// site for per-action beats + params, so `creature_requests` and any other
/// caller build every `AnimationRequest` through here rather than
/// re-authoring beats inline.
pub fn animation_request(action: &str, description: &str) -> Option<AnimationRequest> {
    let template = ACTION_TEMPLATES.iter().find(|t| t.action == action)?;
    Some(AnimationRequest {
        action: template.action.to_string(),
        prompt: format!("A {} {}", description, template.beats),
        params: DEFAULT_CLIP_PARAMS,
    })
}

impl AssetGen {
    /// Generates a creature's still plus its default idle/attack/hatch
    /// clips via `generate_image_with_animations`.
    pub fn generate_creature(&self, spec: CreatureSpec) -> AnimationSetHandle {
        let (image, anims) = creature_requests(&spec);
        self.generate_image_with_animations(image, anims)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::asset_gen::backend_image::{key_color_for, ZImageBackend};
    use crate::asset_gen::capability::GpuCapability;
    use crate::asset_gen::recipe::SdCliInvocation;
    use crate::asset_gen::runner::{CancelFlag, JobError, JobRunner, RunOutput};

    fn spec() -> CreatureSpec {
        CreatureSpec {
            description: "a tiny fox".to_string(),
            seed: 7777,
            fidelity: Fidelity::Draft,
            dominant_color: [200, 60, 40],
            import_path: None,
        }
    }

    /// The three animation requests carry exactly the default action set,
    /// in order.
    #[test]
    fn creature_requests_uses_default_action_set() {
        let (_, anims) = creature_requests(&spec());
        let actions: Vec<&str> = anims.iter().map(|a| a.action.as_str()).collect();
        assert_eq!(actions, vec!["idle", "attack", "hatch"]);
    }

    /// Each animation prompt carries the caller's description plus its own
    /// beat keyword; the image prompt carries the description plus the
    /// creature-framing clause.
    #[test]
    fn creature_requests_prompts_carry_description_and_beats() {
        let s = spec();
        let (image, anims) = creature_requests(&s);

        assert!(image.prompt.contains(&s.description), "got: {}", image.prompt);
        assert!(image.prompt.contains(CREATURE_FRAMING), "got: {}", image.prompt);

        let by_action = |action: &str| {
            anims
                .iter()
                .find(|a| a.action == action)
                .unwrap_or_else(|| panic!("missing action {action}"))
        };
        assert!(by_action("idle").prompt.contains(&s.description));
        assert!(by_action("idle").prompt.contains("bob"), "got: {}", by_action("idle").prompt);
        assert!(by_action("attack").prompt.contains(&s.description));
        assert!(
            by_action("attack").prompt.contains("follow-through"),
            "got: {}",
            by_action("attack").prompt
        );
        assert!(by_action("hatch").prompt.contains(&s.description));
        assert!(by_action("hatch").prompt.contains("emerges"), "got: {}", by_action("hatch").prompt);
    }

    /// Neither the image nor any animation request re-embeds the backend-
    /// owned flat-cartoon style language: the backends append it once
    /// downstream, so the request itself must not carry it too.
    #[test]
    fn creature_requests_does_not_duplicate_backend_style() {
        let (image, anims) = creature_requests(&spec());
        assert!(!image.prompt.contains("cel-shaded"), "got: {}", image.prompt);
        for anim in &anims {
            assert!(!anim.prompt.contains("cel-shaded"), "got: {}", anim.prompt);
        }
    }

    /// The image request's key color is selected through the one-place
    /// `key_color_for` rule off `dominant_color`, and the seed passes
    /// through unchanged.
    #[test]
    fn creature_requests_selects_key_via_rule() {
        let mut s = spec();
        s.dominant_color = [200, 60, 40]; // not green-family
        let (image, _) = creature_requests(&s);
        assert_eq!(image.background_key, key_color_for(s.dominant_color));
        assert_eq!(image.seed, s.seed);

        let mut green = spec();
        green.dominant_color = [40, 200, 60]; // green-family
        let (image, _) = creature_requests(&green);
        assert_eq!(image.background_key, key_color_for(green.dominant_color));
    }

    /// One fake `JobRunner` branching on the invocation's mode arg,
    /// mirroring `compose.rs`'s `CompositeRunner`: writes a flat-key still
    /// for the image backend's mode and a single flat-key frame for the
    /// animation backend's mode.
    struct CreatureRunner {
        anim_calls: Arc<AtomicUsize>,
    }

    impl JobRunner for CreatureRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let out_path = PathBuf::from(&invocation.args[o_idx + 1]);

            if invocation.args.windows(2).any(|w| w == ["-M", "img_gen"]) {
                std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
                let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
                img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
                img.save(&out_path).unwrap();
                return Ok(RunOutput { stdout: String::new() });
            }

            self.anim_calls.fetch_add(1, Ordering::SeqCst);
            let dir = out_path.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&dir).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(dir.join("f_000.png")).unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    /// `generate_creature` produces the still plus all three default clips,
    /// each resolving `Ok`.
    #[test]
    fn generate_creature_produces_still_and_three_clips() {
        let anim_calls = Arc::new(AtomicUsize::new(0));
        let gen = AssetGen::new(
            Arc::new(CreatureRunner { anim_calls: anim_calls.clone() }),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
        );

        let result = gen.generate_creature(spec()).wait();

        assert!(result.image.is_ok(), "still must resolve Ok, got {:?}", result.image);
        assert_eq!(result.clips.len(), 3, "one clip per default action");
        assert!(result.clips.iter().all(|c| c.is_ok()), "every clip must resolve Ok: {:?}", result.clips);
        assert_eq!(anim_calls.load(Ordering::SeqCst), 3, "one runner call per default action");
    }

    /// `animation_request` carries the caller's description, the action's
    /// beats, and the shared default clip params; an unknown action yields
    /// `None`.
    #[test]
    fn animation_request_owns_beats_and_params_or_none_for_unknown() {
        let idle = animation_request("idle", "a shy fox").expect("idle has a template");
        assert_eq!(idle.action, "idle");
        assert!(idle.prompt.contains("a shy fox"), "got: {}", idle.prompt);
        assert!(idle.prompt.contains("bob"), "got: {}", idle.prompt);
        assert_eq!(idle.params, DEFAULT_CLIP_PARAMS);

        let attack = animation_request("attack", "a shy fox").expect("attack has a template");
        assert_eq!(attack.action, "attack");
        assert!(attack.prompt.contains("follow-through"), "got: {}", attack.prompt);
        assert_eq!(attack.params, DEFAULT_CLIP_PARAMS);

        assert!(
            animation_request("bogus-action", "a shy fox").is_none(),
            "an action with no template must yield None"
        );
    }
}
