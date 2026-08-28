# Sprite-sheet / pose-grid keyframe generation — research parked for later (2026-08-13)

Not built or tested yet. Saved so this angle isn't lost — revisit after the current H3 push.

## The idea
Same mechanism as the Krea 2 grid-generation idea (`09-exhaustive-search-plan.md`), but free/local
instead of a paid API: generate several consistent poses of the same character in a single still-image
call (no video model, no melting risk by construction), then treat those poses as animation keyframes.

## What's actually usable (free, local, ComfyUI-native)
**FLUX.1 (`schnell` variant is Apache-2.0) + a character-turnaround LoRA + OpenPose-skeleton-grid
ControlNet conditioning.** Well-documented, community-proven pattern: feed a grid of OpenPose skeletons
(each panel a different pose/angle) as ControlNet input, write a short character-description prompt,
get back a multi-panel sheet with consistent identity across panels.

## What's NOT usable
- Krea 2 — ruled out, paid API only (see `00-overview.md`).
- "Sprite Sheet Diffusion" (arXiv 2412.03685) — the actual purpose-built research for this exact task
  (reference image + pose sequence → consistent animated frames), but **no released code or weights** —
  a paper contribution only, not something we can run. Built on SD v1.5 + Animate Anyone, reportedly
  needed 30GB+ VRAM to train — likely not viable on this hardware even if it were released.
- Most of the GitHub "sprite sheet generator" tools found (`falsprite`, `sprite-sheet-creator`) route
  through `fal.ai`, a paid inference host — same problem as Krea, not actually free/local despite being
  open-source *tooling*.

## Open question, not yet tested
The community FLUX+LoRA workflows are proven for **turnarounds** (same pose, different camera angles).
Whether the same mechanism extends cleanly to **different action poses** (windup/strike/follow-through)
of the same character is untested — plausible (swap the ControlNet grid's skeletons from different
angles to different action poses) but not verified. This project already has real experience computing
skeleton/joint poses (the FABRIK IK work, now closed per `02-dead-ends.md`) that could feed directly
into this as the ControlNet conditioning input.
