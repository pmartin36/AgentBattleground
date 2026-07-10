# Effects-Heavy Scene Investigation (2026-07-04, parallel track)

Separate track from the character-rig work happening in parallel. Premise (user's fallback idea,
already flagged as "current leading direction" in `../FAILED_EXPERIMENTS.md`): stop trying to nail
precise character-level attack animation — pair **minimal, reliable creature motion** with a **big,
generalizable environmental/magic effect** to sell the attack instead.

**Load any of these** (from `experiments/creature_lab/`):
```bash
../ascii_test/target/release/playframes out/<dir> --chroma auto --pingpong --fps 12
```

## Verdict up front
**Procedural (code-drawn) effects work well and are the clear winner.** Diffusion-generated abstract
effects do not reliably produce anything clean. Zero melting, zero hallucination risk either way with
procedural — this is the most unambiguously positive result across both animation investigations this
session.

## What was built

### `tools/fxgen.py` — new procedural effect generator (numpy + PIL, no diffusion)
Three modes, each fully parametric (anchor point, scale, frame count, color) — not hardcoded to any
creature or weapon:
- **`fire`** — `out/fx_fire` (25 frames). Layered flame-tongue shapes (numpy distance-field + fire
  color LUT), erupt → sustain/flicker → fade arc. **Result: good.** Reads clearly as fire, flickers
  convincingly frame to frame, bold silhouette should hold up fine at braille resolution.
- **`crack`** — `out/fx_crack` (25 frames). Fractal (midpoint-displacement) jagged crack lines radiate
  from an anchor point, reveal progressively, then the main branches widen into a dark chasm with a
  glowing orange rim. Hit one real bug during dev (the widening gap was accidentally confined to the
  thin line-reveal mask and never visibly opened) — fixed by tracking arc-length-of-nearest-point
  separately from perpendicular distance. **Result: good after the fix**, clearly reads as "ground
  cracking open."
- **`burst`** — `out/fx_burst` (20 frames). Expanding fading rings + a quick bright core flash, color
  configurable (tested purple/magenta for a "magic" feel). **Result: good, simplest and safest of the
  three** — basic radial geometry, unlikely to ever look bad.

### `tools/fxcomposite.py` — new compositor
Green-keys an fx clip and overlays it onto a base creature clip (or under it, via `--behind`),
looping the shorter clip to match. Not diffusion-specific — works on any two green-screen clips.

### Combined scene — `out/fx_scene_mouse_fire` (25 frames)
`wl_bounce` (re-verified this session as coherent, not just trusted from old docs — see main
session's finding) + `fx_fire`, composited with `fxcomposite.py`. **Result: genuinely good.** The
mouse's bounce motion (visible arm movement, an airborne jump with shadow) reads naturally, fire
erupts convincingly in front of its lower body, and — critically — none of the disturbing artifacts
from the character-rig track (hallucinated mouths, floating props) appear here, because the face/ears
are never touched and nothing about the fire depends on tracking a limb. This is the strongest overall
result from either investigation this session. Minor note: the fire fully occludes the legs/lower
body, which reads fine as "conjuring fire" but is worth knowing if a design wants the whole body
visible.

### Diffusion test — `out/fx_ice_diffusion` (17 frames)
Tested the open hypothesis that amorphous phenomena (fire/ice/smoke) might tolerate the "magnitude
wall" better than a rigid prop or precise limb, since visual chaos reads as stylistically fine for
these. Wan 2.2 TI2V-5B **T2V** (no init image), prompt: "a burst of ice crystals and frost shooting
outward, blizzard of snow and ice shards swirling, magical ice attack effect... simple flat-color
low-detail effect."
**Result: does NOT validate the hypothesis, in either direction.** The output isn't melted-into-mush
the way a creature is — there's no coherent subject to melt in the first place — but it's also not a
clean usable effect. All three sampled frames (0, 8, 16) are noisy, glitchy blue/white/black
static-like texture with no discernible shape or progression — closer to visual interference than a
blizzard. **Verdict: not worth pursuing further as-is.** Procedural clearly wins for effects; if
diffusion effects are revisited, they'd need heavy prompt/parameter iteration this session didn't
have budget for, and even then there's no guarantee of a controllable *shape* (a burst vs. a beam vs.
a spreading frost) the way procedural gives for free via parameters.

## Recommendation
Build out the effects-library idea for real: `fxgen.py`'s three modes are a solid starting set.
Natural next additions in the same procedural style: a directional beam (line-anchored instead of
point-anchored, for the "ice beam" idea specifically), a projectile/impact variant, and a heal/buff
pulse (reuse `burst` with a different palette). All combine with `fxcomposite.py` onto any creature's
existing idle/bounce/lunge clip — this generalizes across creatures for free since the effect never
looks at the creature's geometry, only its own anchor point.
