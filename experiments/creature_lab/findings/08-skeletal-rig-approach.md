# Skeletal Rig Approach — the actual breakthrough (2026-07-04, post-mortem of the 10 experiments)

After 10 diffusion-based attempts (`06-ten-experiments-manifest.md`) ranged from melted to frozen to
actively disturbing (a hallucinated bloody mouth, a floating disconnected sword, a flickering
wound-like line for an arm) — all confirmed by the user watching real braille playback, not stills —
we stopped asking video diffusion models to generate the attack motion at all.

## The idea
We don't need to *track* where the arm is if we're the ones drawing it. A fully code-driven 2-bone
(shoulder→elbow→wrist) arm, rendered directly onto the static creature image and rotated through a
deterministic windup→strike timeline, has **zero melting risk** (nothing is diffusion-generated),
**zero floating-prop risk** (the weapon's exact position is computed geometrically, not estimated),
and **zero hallucination risk** (no video model is involved in the motion at all). Diffusion is only
ever used to make the *static* creature art; every frame of motion is ordinary 2D compositing.

## What was built
- `tools/rig_arm.py` — takes a static creature image + the resting-paw region to erase, draws a
  tapered 2-bone arm (fur-colored capsule segments with an outline stroke, matching the flat-cartoon
  art style) through a windup→strike arc (reusing `mkslash3`'s timing/easing), and emits an
  `attach`-format anchors.txt for the wrist position/angle each frame.
- The erase step samples the LOCAL background color from a ring just outside the erased region (not
  a single fixed corner color) and feathers the mask with a Gaussian blur, so the patch blends into
  the green screen's own subtle vignette instead of leaving a hard rectangle seam.
- The existing `attach` binary (no changes needed) composites the crisp `sword_sprite.png` onto the
  computed wrist position/angle per frame — reused exactly as designed, just fed geometry instead of
  a video-derived anchor track.

## Result — `out/rig_final` (25 frames)
```bash
cd experiments/creature_lab
../ascii_test/target/release/playframes out/rig_final --chroma auto --pingpong --fps 12
../ascii_test/target/release/playframes out/rig_final --chroma auto --fps 12 --ease anticipate
```
**Clean across the whole arc, frame-by-frame verified (0, 5, 10, 12, 17, 20, 24):** the sword stays
rigidly bound to the hand at every single frame, the arm swings through a real windup→raised→strike→
follow-through arc, no melting, no gaps, no artifacts. This is a genuinely usable attack-swing result —
the first one across two full sessions.

## Known rough edges (all fixable, none fundamental)
- The sword's blade runs off the bottom of the 768×768 canvas at the extremes of the swing — a
  framing/composition issue (character positioned high in frame), not a technique flaw. Fix: give the
  swing more clearance (reposition the character, or scale the sword down slightly).
- Only the arm is rigged; body/legs/ears/tail are entirely static during the swing. A tiny secondary
  motion (a subtle body lean or weight-shift, timed to the swing) would sell it further, and could
  itself be code-driven (procedural squash/lean) rather than diffusion-generated, keeping the same
  zero-risk property.
- Currently one arm, one creature (the mouse), hand-measured shoulder pivot and bone lengths. For
  generalization, the pivot/bone-length measurement would need to be automatic per creature (e.g.
  from the silhouette's bounding box, similar to `mkanchors.py`'s approach) rather than hand-tuned —
  tractable, not yet done.

## Why this is the right direction going forward
Every generative approach in `06-ten-experiments-manifest.md` failed in a DIFFERENT way (magnitude
wall, model capacity ceiling, near-zero motion, hallucination) — that diversity of failure modes across
10 genuinely different techniques, informed by real research, is itself strong evidence that the
generative-attack-motion approach has a low ceiling with the models available on this hardware. The
rig approach sidesteps the entire failure class by construction. Recommend: build this out (auto
pivot/bone detection, secondary body motion, a small library of swing/thrust/overhead timelines) as
the primary attack-animation path, rather than continuing to chase bigger/different video models.
