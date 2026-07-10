# Promising — go deeper here

Ranked-ish by how much I'd bet on them. The core unsolved problem: **a lively, controllable,
identity-preserving ATTACK for an arbitrary creature/action, in braille.**

## 1. Reference-/trajectory-driven motion — the properly-untried path
Research (findings summarized below) is unanimous: **specific motion is controlled by trajectories or
reference motion, NOT text.** We only ever fed VACE a *crude synthetic* control. We never tried a **real
reference motion**:
- **VACE V2V with a REAL depth/pose sequence** of an actual slash (from a stock clip or a posed
  reference), retargeted onto the creature. Runs in sd.cpp. The extraction (DepthAnything / DWPose) is a
  **dev-time** tool (build a motion library once), NOT a runtime dependency — so it doesn't hurt shipping.
- **ATI (Any Trajectory Instruction)** — draw motion paths, model follows. Identity-preserving (sparse
  trajectory, unlike VACE's dense control). Lives in **ComfyUI + Wan 14B** (not sd.cpp yet, tight on 16 GB).
  ComfyUI is a *dev/validation* tool only; anything shipped must run in sd.cpp.
- Key idea: **author a motion library ONCE** (as depth/pose/trajectory clips), retarget to ANY creature.
  Generic + general. This is the "motion library + per-creature retarget" architecture.

## 2. The weaponless-body + attached-weapon decomposition (right idea, redo the execution)
Validated: weaponless body motion generates **clean**; the weapon is the only thing that melts. So:
generate weaponless motion → **attach a crisp weapon sprite bound to the hand**. What the failed `attach`
attempt was missing:
- **A real swing body-motion** (the `wl_swing` gen was a dud). Get it via #1 (reference/trajectory), or by
  authoring the *sword's* swing (angle keyframes) over a coherent body clip — decouple body vs weapon.
- **Auto hand-detection** per frame (paw color/blob or a light pose model) instead of hand-authored
  anchors — this is what makes it *general* (no manual work per creature).
- Better anchoring math (orientation from the arm, not guessed).
`attach.rs` and `mkslash2.rs` (silhouette derivation) are starting points.

## 3. LLM → motion representation → generation (the prompt-first general vision)
Player types free-text action → an **LLM maps it to a motion representation** (pick/compose a library
motion, or synthesize trajectory strokes) → trajectory/reference-driven gen executes it, identity kept.
Prompt-first for the user, general across creatures/actions, tweakable (edit the trajectory). Untested;
depends on #1 being solid.

## 4. Motion-prior / liveliness techniques (research-stage, not in sd.cpp)
- **Go-with-the-Flow** (warp noise along optical flow → real motion prior instead of lerp) — CONFIRMED
  relevant.
- **VideoJAM / MoGAN** (joint appearance+motion, motion-adversarial finetune) — makes I2V motion livelier.
- Root cause of lifeless motion: diffusion MSE objective is ~invariant to temporal coherence. These add a
  motion signal. *Revisit when* one lands in a local/GGUF-runnable form.

## 5. Effects library + animation principles (the reliable floor)
- **Pluggable braille effects** (slash / burst / projectile / pulse / heal) authored by us, anchored to a
  creature point — general, crisp, zero diffusion gamble. NOT one hardcoded slash.
- **Frame-timing/easing** (`--ease`, proven "much improved") + **squash/stretch** — but only *on top of
  real body motion*, never as a substitute for it (that reads as puppetry).
- Combine: reliable body-motion clip + easing + attached weapon + effect + target recoil.

## 6. Wildcards worth a look
- **Wan 2.2/2.1 VACE 14B** quantized on 16 GB — might manifest a solid prop where 1.3B couldn't (slow).
- **Sprite-sheet / rig from a single image** — auto-segment creature into parts, rig, animate with a
  shared skeleton. Hard to generalize (arbitrary morphology) but the most classic-game-correct.
- **Higher frame counts** for big motion (more frames = smaller per-frame jumps = less melt) — cheap to
  test more aggressively.
- **Different base video models** as they appear (LTX-2 fits 16 GB; check its control ecosystem).

## Research sources (2026, cited during the session)
Generalizable Motion Generation (arXiv 2510.26794) · VideoJAM (2502.02492) · Go-with-the-Flow (2501.08331)
· MoGAN (2511.21592) · ATI (2505.22944, anytraj.github.io, docs.comfy.org/tutorials/video/wan/wan-ati) ·
Custom Motion Transfer (2312.04966) · sd.cpp Wan/VACE docs (github.com/leejet/stable-diffusion.cpp/docs).
