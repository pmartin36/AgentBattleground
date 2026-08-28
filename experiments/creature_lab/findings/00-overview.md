# Creature Animation — Findings & State

Read this first, then `12-attack-animation-pipeline.md` for the current working pipeline. The rest
(`01-what-works` through `11`) is prior research/dead-end history, referenced from here where relevant.

## The goal (non-negotiable framing)
A **generalized** creature-animation system: the player generates *any* creature ("an alien on a
pogostick that attacks via telepathy") and we produce good animations for *any* action. Ideal =
**type a prompt → great animation**. All **local** (stable-diffusion.cpp, no cloud, shippable in the
game binary). Output is **downscaled to braille**, so motion quality / silhouette / temporal
coherence matter far more than fine pixel detail.

## Where we are
**The attack-animation pipeline is locked in — see `12-attack-animation-pipeline.md` for the complete,
current, runnable spec** (exact commands, prompt template, known gotchas). Summary: generate a creature
still image → clean its background/shadow → animate via MiniMax H3 (image-to-video, Turbo LoRA, ~3
min/clip on native `sd-cli`) → extract frames. No driving video, no per-creature rigging, a single detailed text prompt
drives specific committed attack motion with identity/style held up. Verified end-to-end, including on
a creature generated fresh through the whole pipeline (an alien on a pogostick, bounce + gunfire).

**Chroma-key/shadow bleed is fixed.** `tools/cleanbg.py` strips both the background and any baked-in
drop shadow before the image reaches H3 (H3 doesn't reliably reproduce a shadow's exact shape through
generated motion, otherwise leaving patchy bleed). Verified clean across full clips on three creatures
spanning both painterly-shaded and bold flat-vector art styles.

**Background-color selection matters**: creatures whose own color matches the default green-screen hue
family (e.g. a green-skinned alien) must be generated on a different key color (magenta, per the
existing `generate.sh` convention for green creatures) or the background/shadow-removal step cannot
distinguish character from background at all.

**Prop coherence** (a held weapon staying solid through motion — historically the hardest failure mode
in this project): tested on a mouse with a sword. Clean, full success — no issues.

**Runtime**: the pipeline runs natively on `stable-diffusion.cpp` (`sd-cli`), no Python/ComfyUI —
local generation shippable inside the game binary, the project's standing goal. See
`12-attack-animation-pipeline.md` for the exact commands.

Setup: native `stable-diffusion.cpp` (`sd-cli`), local, free/open-weight models only. Hailuo 2.3 and
Krea 2 are out of scope — paid/cloud-API only, this project doesn't use paid models.

**SCAIL-2** (topology-free motion transfer from a driving video onto a target image) works — real
motion quality, identity/green-screen held up — but requires a driving video showing the desired
motion, which doesn't generalize to arbitrary player-invented actions (no stock footage exists for
"an alien on a pogostick that attacks via telepathy"). Also renders the target creature visibly
thinner and with softer shading than the source, consistently across every driving video tried.
Given H3 doesn't have either limitation, H3 is the primary path; SCAIL-2 is a fallback only.

**Skeletal/rig approaches (hand-drawn capsule rig and FABRIK IK + real-texture cutout rig) are
CLOSED** — cutting a sprite into rigid pieces leaves nothing actually rendered at the joint, which
reads as unnatural motion regardless of resolution or polish. See `02-dead-ends` for the full
technical record and revisit-if conditions.

Idle animation (separate from attack animation) is unreliable via diffusion I2V — treat any specific
idle clip as unverified until its own frames/playback are checked, don't assume the category works.

## Meta-learnings
1. **Judge braille playback, across the whole arc, not stills — and not sparse frame sampling either.**
   A clip can look unchanged across widely-spaced still frames while containing a real, well-formed
   motion concentrated in a narrower window between them. Dense sampling or real playback only.
2. **The magnitude wall** (smaller video models, e.g. Wan 5B): single-image I2V only stays coherent for
   small motion; large/fast motion melts the whole creature, not just a prop.
3. **The prop wall** (smaller video models, e.g. Wan-family): a rigid weapon melts, doubles, or goes
   floppy when moved, even inside otherwise-coherent body motion. H3 does not have this problem.
4. **Larger models with real language understanding (H3) can follow a specific prompted action** where
   smaller video models (Wan-family) cannot — text-driven specific motion is model-capacity-dependent,
   not fundamentally impossible.
5. **Generalization is the whole point.** No hardcoded per-weapon/per-creature effects — solutions must
   be creature- and action-agnostic.

## How to use `02-dead-ends`
Dead ends are recorded with a **"revisit if"** note. They are *not* permanently closed — most failed
for a specific reason, and a **sufficiently different approach** may get past it. Don't blindly rerun
the exact same thing; do reconsider if you have a new angle on the root cause.

## Note
`../FAILED_EXPERIMENTS.md` is an earlier, partial version of this; this `findings/` folder supersedes it.
