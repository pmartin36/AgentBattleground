# Exhaustive Options Search — Plan & Status (2026-07-08)

Requested by the project owner after rejecting both the hand-measured skeletal rig
(`08-skeletal-rig-approach.md`) and (provisionally) leaning into effects-only
(`07-effects-scene-investigation.md`): before committing to a direction, search every option
available, broadly, with real citations — not pattern-matched recall. This doc tracks that search:
what's been found, what's still running, what's buildable without the owner, and what needs them.

**Status: all 7 research forks complete. Owner asked to proceed on independent work — vision-LLM
pointing validation done (see below), local Qwen2-VL-7B-GGUF comparison done (see below). Still
awaiting owner decision on which direction to actually build.**

## Vision-LLM pointing — widened validation (done)

Tested blind (no grid overlay) on 4 more diverse creatures beyond the original hawk/wolf: a slime
(blob, minimal limb definition — the hardest case), a stone golem (humanoid, clear joints), a verdant
treant (asymmetric tree-branch limbs), and a shadow cat (quadruped, but in a front-facing crouch with
foreshortened/overlapping legs — a harder pose than the wolf's clean profile stance).

- **Slime**: all 4 points (2 arm nubs, 2 feet) landed correctly or very close, despite this being the
  least anatomically defined creature tested.
- **Stone golem**: excellent — 6 of 8 points (shoulders, elbows, fists, feet) landed almost exactly on
  the joint.
- **Verdant treant**: hands and feet landed almost exactly right; shoulders consistently landed a bit
  low/into the torso rather than precisely at the joint — a mild, repeatable bias, not a failure.
- **Shadow cat**: the weak point. In a foreshortened frontal crouch with legs overlapping, points
  clustered in the right general leg/paw zone but weren't reliably assigned to the *correct specific*
  leg (front vs. back) — genuinely hard even for a human eyeballing the same image. Head point also
  landed slightly high (between the eyes rather than at the chin).

**Conclusion: strong and consistent across very different body plans (biped, quadruped, blob,
asymmetric/branching), with one real, specific, disclosed weakness — ambiguous/foreshortened poses
where limbs visually overlap.** Six creatures tested total now (hawk, wolf, slime, golem, treant, cat),
zero wildly-wrong points, one class of pose that's genuinely harder. This is real signal, not proof at
scale, but meaningfully de-risks the idea beyond the original 2-image test.

## Local vision model comparison — Qwen2-VL-7B-GGUF (done, negative result)

Built llama.cpp from source with Vulkan (same backend choice as this project's stable-diffusion.cpp,
for the same reason — this GPU's Blackwell architecture needs a newer CUDA toolkit than is installed),
downloaded Qwen2-VL-7B-Instruct-Q4_K_M + its mmproj vision-projector file (~6GB total), and ran it via
`llama-mtmd-cli` (the current, non-deprecated multimodal CLI) on the same wolf image already tested
with Claude's own vision.

**Result: does not perform real visual grounding in this configuration.** Three attempts:
1. First run (default image tokens): all 8 requested joint coordinates fell on a suspiciously regular
   grid (multiples of ~128px) — not real per-point analysis.
2. Second run (`--image-min-tokens 1024`, per a warning the tool itself printed about grounding
   accuracy): coordinates were still suspiciously grid-like; visually overlaying them showed half the
   points (4 paws) roughly in the right zone, but the other half (neck/head, hip) landed in empty
   background, nowhere near the actual creature.
3. Third run (simplest possible ask — single point, "where does the front-left paw touch the ground",
   `--image-min-tokens 2048`): answered `(384, 765)` — exactly image-center-x and the bottom edge. Not
   a real localization, a generic/default-shaped guess.

**Bottom line**: this specific local setup (Q4_K_M quantization, llama.cpp's GGUF/mtmd inference path)
does not reliably localize points on our creature art, in clear contrast to Claude's own consistently
precise pointing across 6 creatures. This could be the quantization, the llama.cpp mtmd path being
labeled experimental, insufficient image-token allocation even at 2048, or Qwen2-VL genuinely needing
its native (non-GGUF) inference stack with matching image preprocessing to ground well — not
determined which, and not worth further debugging time without a stronger reason to believe it'd pay
off. **Real, disclosed negative result: the "local vision model" half of this lead does not currently
hold up; the online-model (Claude/GPT-4V) half still does, per the 6-creature validation above.**

Spot-checked a second creature (stone golem, clearest joints of anything tested) to rule out a
wolf-specific fluke: result was even more clearly degenerate — coordinates formed an obvious diagonal
arithmetic progression (128,128 → 192,192 → 256,256 for one arm, mirrored for the other) and both feet
landed at literal image corners. Confirms this is template pattern-completion, not visual analysis,
across at least 2 of the 6 test creatures — not worth testing the remaining 4.

## Round 1 findings (complete)

### Mesh-based rigging / auto-rig-from-single-image
- Mesh deformation (vertices weighted to bones, art bends smoothly) is the real, established fix for
  the "rigid cutout part looks bolted on" problem the owner flagged. Spine/DragonBones/Live2D all do
  this — but rig authoring (bone placement + weight painting) is **manual** in all of them.
- Monster Mash (SIGGRAPH Asia 2020, open source, real, [github.com/google/monster-mash](https://github.com/google/monster-mash)) needs a **fresh hand-drawn sketch** as input, not an already-generated flat sprite — wrong shape for our pipeline.
- Meta's Animated Drawings ([facebookresearch/AnimatedDrawings](https://github.com/facebookresearch/AnimatedDrawings)) auto-rigs AND mesh-deforms, but its pose estimator assumes a **human-like bipedal skeleton only** — quadrupeds/many-limbed creatures need manual skeleton config.
- **PixelLab.ai** (commercial, closed, cloud-only) ships automatic skeleton estimation for BOTH bipeds
  and quadrupeds today — proof the whole capability is achievable, just not open/local.
- **Verdict: no open/local tool does this end-to-end for arbitrary morphologies. Real gap, not an oversight.**

### Pose-transfer / motion-retargeting single-image animation models
- FOMM/TPSMM/LIA/MRAA: lightweight (not diffusion, tens-hundreds of MB), but generalize only *within a
  trained category* (faces, human bodies) — no generic "any creature" model exists or could exist
  without per-category training data we don't have.
- AnimateAnyone/MagicAnimate/Champ/MimicMotion: heavier, still human-skeleton-bound, no local/GGUF port.
- **SCAIL-2** (zai-org, June 2026): the one genuinely exciting find — claims true topology-free,
  skeleton-free motion transfer, explicitly including non-human characters, zero-shot. Built on a 14B
  backbone, ComfyUI-only, no stable-diffusion.cpp support found. Same class of wall as our LTX-2
  near-miss. **Round 2 is investigating exact setup feasibility** (see below) since the owner is
  willing to test it directly.
- TopoCap / Motion2Motion: real topology-agnostic retargeting research, but 3D-rigged-character
  targets, no public inference-ready release.

### Spore-style procedural animation / IK
- Correcting an assumption: Spore does NOT auto-detect topology from a finished mesh. Animators author
  ONE generic motion (abstract pose-goals, not literal joint angles), and an IK solver retargets it
  onto each creature's actual (explicitly known, since the player built it part-by-part) skeleton at
  runtime. **Author once, retarget via IK per-creature** — the *principle* ports to 2D even though
  Spore itself is 3D. (Owner's read: this is "limiting" — likely because our creatures are single
  images, not built part-by-part with an explicit skeleton the way Spore's are, so the "skeleton is
  already known" precondition doesn't hold for us.)
- **FABRIK** (Aristidou & Lasenby) is the modern, simple, ~50-line 2D/3D IK algorithm actually used in
  shipping games for this — chosen over CCD for speed and "feel."
- Sorceress ("Procedural Walk", 2026, 3D): auto-detects a *variable* number of limb-like protrusions
  from a shape (spider → 8 IK chains, quadruped → 4) — no fixed bone count. Real, shipped-style
  precedent for "detect however many limbs, IK each one," even though it's 3D-mesh-based.

### Indie gamedev prior art
- PixelLab.ai's skeleton estimator (see above) most likely works because it's a keypoint model trained
  specifically on **stylized game art**, not real photos — independently confirmed elsewhere that
  photo-trained pose models (MediaPipe/OpenPose/DWPose) fail on illustrated/cartoon characters. This
  reframes the missing piece as "a stylized-art-specific keypoint estimator," not an unsolved research
  problem.
- Sorceress.games advertises an end-to-end match to our exact ambition (generate → animate → sprite
  sheet, arbitrary/non-humanoid) — no technical writeup found, proprietary.
- Confirms (doesn't overturn) our own finding: "Sprite Sheet Diffusion" (CMU project) tried
  ControlNet/OpenPose-driven keyframe generation and hit the same wall we did — pose estimators don't
  generalize to illustrated characters, plus overfitting artifacts even with per-character fine-tuning.

## New finding this session — NOT from web research, from a direct hands-on test
While drafting this doc, tested directly (using the assistant's own vision) whether a general-purpose
vision-capable LLM can simply **point at** joint locations on an arbitrary creature image — sidestepping
the entire "train/find a keypoint detector" problem. Two creatures never measured before (a hawk with
spread wings, a wolf in profile), coordinates estimated BLIND (no grid overlay), then checked by
overlay:
- **Hawk**: both wingtips, both wing roots, and the tail base landed exactly on the correct anatomical
  spot; only the "head" point was slightly off (in the gap above the body, near but not on the beak).
- **Wolf**: all four paws landed exactly on the ground-contact point; front shoulder and neck/head
  landed close-but-slightly-off (plausible nearby territory, not wrong); back hip was a bit forward of
  the true hip but still on the body.
- **Read: strong, consistent hit rate across two very different body plans (flying biped-with-wings,
  quadruped), zero wildly-wrong points.** This is a genuinely promising, previously-untried lead.
- Open questions this raises (round 2 fork investigating): is this a documented/validated technique
  elsewhere (or did we just get lucky twice)? Does a LOCAL vision model (not just Claude/GPT-4V, which
  would only be available to players who opt into online models per this project's existing
  local+online model support) do comparably well — check Molmo (AllenAI, known for precise pointing,
  some sizes small enough to be locally plausible) and Qwen2.5-VL specifically. How consistent is this
  across MANY images, not just 2?

## Round 2 findings (complete)

### Vision-LLM pointing/grounding for rigging
- **This is a real, established technique, not something we stumbled into by luck.** Molmo (Allen
  Institute for AI) made "pointing" a first-class VLM capability; MolmoPoint-8B scores 70.7% on
  PointBench / 89.2 F1 on PixMo-Points. The **PointArena** leaderboard shows this is a general
  frontier-model capability (Gemini-2.5-Pro is competitive too), not one lab's trick. GPT-4V/4o can do
  it but is shakier without fine-tuning/templates than purpose-trained pointing models.
- **Stylized/cartoon generalization is NOT benchmarked anywhere.** All the standard pointing benchmarks
  are photo/real-scene based. This means our own two-image hands-on test (hawk + wolf, both landed
  well) is genuinely uncharted validation, not something backed by an existing paper — real signal,
  not yet proof. Worth deliberately expanding before trusting it (more creatures, more diverse body
  plans, compare against hand-measured ground truth).
- **Local/offline candidate exists**: Qwen2-VL-7B-Instruct has a working GGUF quantization running on
  **mainline llama.cpp** (`llama-qwen2vl-cli`, model + a separate `mmproj` vision-projector file) —
  confirmed, shippable, no exotic fork. Qwen2.5-VL (likely better grounding) needs a non-mainline
  llama.cpp fork today. Molmo itself has no confirmed GGUF port — PyTorch/HF-stack only for now.
- Separately: this project's own design already supports online models (Claude, OpenAI, etc.) as a
  player option alongside a required local model — so gating a vision-LLM rig step behind "you opted
  into an online model" is already a legitimate path, independent of whatever the local-only path uses.
- Run-to-run **consistency** (same creature, repeated calls) is not addressed by any benchmark found —
  open question, would need our own testing.

### Build-it-ourselves pipeline pieces (skeleton extraction, mesh deformation, IK)
Risk ranking from the research, easiest to hardest:
- **IK — solved, no need to build from scratch.** [`fabrik` on crates.io](https://crates.io/crates/fabrik) is a ready-to-use Rust FABRIK implementation. FABRIK itself works fine in 2D (confirmed via existing embedded/Arduino ports).
- **Mesh deformation — lower risk than round 1 assessed.** ARAP (Igarashi et al., SIGGRAPH 2005) is a real, implementable-from-the-paper 20-year-old algorithm, not unpublished/proprietary — the earlier "no open tool" finding was about *authoring software* (Spine/Live2D), not the underlying math. **Linear Blend Skinning (LBS)** is simpler and is what most game engines (including Spine/Unity 2D) actually use under the hood — recommended as the practical default over full ARAP. SSDR gives a method for *automatically* computing skin weights instead of hand-painting them.
- **Skeleton extraction — the real remaining risk, but not open research.** Standard technique: morphological skeletonize (scikit-image `skeletonize()`) → endpoints (degree-1 pixels) = limb tips, branch points (degree-3+) = joints. This is standard image-analysis practice and, critically, **naturally detects however many limbs exist** — a real generalization improvement over `mkanchors.py`'s current hardcoded single-limb assumption. Raw skeletonization is noisy on organic shapes though; a documented pruning algorithm exists (Bai et al., "Skeleton pruning by contour approximation and the integer medial axis transform") but tuning it well across wildly different creature silhouettes (blob vs. bird vs. many-limbed alien) is real engineering work, not a drop-in library call.

### SCAIL-2 + ComfyUI feasibility — meaningfully better news than expected
- **Official first-party ComfyUI support exists** (`WanSCAILToVideo`/`SCAIL2ColoredMask` nodes) — not a same-day community hack, an actual maintained workflow (`docs.comfy.org/tutorials/video/zai/scail2`).
- **A GGUF quant sized for exactly our situation**: Q4_K_M is 10.9GB — comparable to how Wan 2.2 5B
  comfortably fit this 16GB card. (Full size table: Q2_K 6GB → Q8_0 17.7GB, so there's headroom to size
  down further if 10.9GB is still tight alongside the compute buffer.)
- **The CUDA blocker that forced our own stable-diffusion.cpp build onto Vulkan does NOT apply to
  ComfyUI.** That was about the *system apt CUDA toolkit* being too old for this GPU (Blackwell sm_120,
  needs 12.8+). ComfyUI/PyTorch gets CUDA via a pip wheel that bundles its own runtime — confirmed
  community reports of RTX 5070 Ti working via the cu128/cu130 wheel index, independent of the system
  toolkit. Expected: a normal `git clone` + `pip install` + correct-wheel torch install, no
  installation-level blocker.
- No native stable-diffusion.cpp support exists or is in progress — ComfyUI is the only path today.
- **Concrete setup**: clone ComfyUI, install torch via the cu128/cu130 wheel index (not default pip),
  install `city96/ComfyUI-GGUF` custom node, download the SCAIL-2 Q4_K_M GGUF + Wan2.1 VAE + umt5-xxl
  text encoder + CLIP vision (all formats this project already has direct experience with), load the
  official SCAIL-2 workflow and point its loader at the GGUF quant.
- **Risk assessment: meaningfully lower-risk than the LTX-2 attempt** — first-party support, a
  comfortably-sized quant, no CUDA-toolkit blocker. The one real unknown left is actual generation
  speed/VRAM headroom for real video-length output — no direct "ran this on a 16GB card" report found,
  just quant-size math implying it should fit. Worth an actual test, not a guaranteed smooth run, but
  meaningfully more promising odds than the LTX-2 disk-thrashing experience.

## Options inventory — final

| Option | Generalizes to any creature? | Runs local/offline? | Effort | Status |
|---|---|---|---|---|
| Hand-measured rig (tried) | No | Yes | Done | **Rejected by owner** |
| Effects library (tried) | Yes (anchor-only) | Yes | Done | Working; owner wants breadth explored before committing |
| Mesh/Live2D/Spine-style rig | Only with manual authoring | Yes | High (manual per creature) | Ruled out — defeats "type a prompt" goal |
| FOMM/TPSMM-family | No (needs per-category training data) | Yes (lightweight) | N/A — blocked | Ruled out |
| SCAIL-2 (via ComfyUI) | Claims yes, unconfirmed on stylized art | ComfyUI only, 10.9GB quant fits 16GB on paper | Setup well-documented, first-party support, no CUDA blocker | **Owner testing directly; meaningfully lower-risk than LTX-2 was** |
| Spore-style generic-motion+IK | Yes in principle, needs a known skeleton first | Yes | Medium-high | Owner: "seems limiting" — deprioritized |
| From-scratch silhouette→skeleton→IK→mesh-warp pipeline | Yes (that's the point) | Yes | Medium — IK is solved (crate), mesh deform has real 20yr-old algorithms (LBS simplest), skeleton-extraction/pruning is the one real remaining engineering risk | Buildable; not yet started |
| Vision-LLM pointing for auto-rig | Yes — strong hands-on signal, technique is established generally, but stylized-art accuracy is genuinely unbenchmarked anywhere | Online models (Claude/GPT-4V) already supported by this project; Qwen2-VL-7B-GGUF is a real local candidate on mainline llama.cpp | Low-medium if it holds up under wider testing | **New lead; needs broader validation before trusting** |

## Next steps — what I (the assistant) can do without the owner
- **Validate vision-LLM pointing properly**: run the blind-pointing test across a much wider, more
  diverse set of this project's own creatures (bipeds, quadrupeds, blobs, many-limbed, asymmetric),
  compare against hand-measured ground truth, and separately test Qwen2-VL-7B-GGUF locally (not just
  Claude) to see how the offline path compares. This is cheap (no new downloads beyond one GGUF vision
  model) and directly de-risks the most promising new lead.
- **Prototype the from-scratch pipeline** (skeleton extraction → FABRIK IK via the existing crate →
  linear-blend-skinning mesh deformation) on 3-5 diverse creatures as a feasibility demo, once vision-
  pointing (for the joint-detection step) or classical skeletonization (as the alternative/fallback
  joint-detection step) has been validated enough to pick one.
- Either of the above is real build work — **will ask before starting**, per the explicit
  "confirm before implementing" preference, rather than treating this research as a green light.

## What needs the owner
- **Testing SCAIL-2 via ComfyUI themselves** (as offered) — setup instructions above are ready to go.
  Flag if you'd rather I attempt it in this same sandbox instead — I likely can, headlessly, via
  ComfyUI's API without needing a GUI/browser — your call.
- **A decision on where to actually invest build time**: vision-LLM-pointing pipeline, from-scratch
  classical pipeline, SCAIL-2 (pending your test), effects-only, or some combination. This doc is
  inputs to that decision, not the decision itself.
- **Subjective quality judgment** on whatever gets prototyped next — as with the rig and effects
  results, only real braille playback in front of your own eyes is the actual bar.
