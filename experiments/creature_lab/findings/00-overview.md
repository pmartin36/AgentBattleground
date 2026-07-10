# Creature Animation — Findings & State

Knowledge dump from a long broad-exploration session (through early July 2026). Read this first,
then `01-what-works`, `02-dead-ends`, `03-promising-next`, `04-inventory`, `05-proven-pipeline`,
`06-ten-experiments-manifest` (2026-07-04 session: 10 concrete approaches designed from deep web
research and actually run — most were negative/partial results, which is real signal, not noise).

## The goal (non-negotiable framing)
A **generalized** creature-animation system: the player generates *any* creature ("an alien on a
pogostick that attacks via telepathy") and we produce good animations for *any* action. Ideal =
**type a prompt → great animation**. All **local** (stable-diffusion.cpp, no cloud, shippable in the
game binary). Output is **downscaled to braille**, so motion quality / silhouette / temporal
coherence matter far more than fine pixel detail.

## Where we are
**Broad exploration phase.** We've been *close* a few times but have NOT cracked it. Specifically:
- **Idle animation is hit-or-miss, not reliable.** Correction (user, 2026-07-04): it "sometimes works
  and sometimes produces basically nothing." A few clips (`greenmouse_anim`, `slime_anim`, `wl_bounce`,
  `wl_lunge`) have been individually re-verified as coherent, but don't treat "idle/weaponless motion"
  as a trusted default — re-check any specific clip's frames before building on it.
- **Attack animation is the unsolved core problem** — a lively, controllable, identity-preserving
  attack for an arbitrary creature. Every approach so far either melts, goes lifeless, or reads badly
  in braille. This is where to "go much deeper — try many more things." The 2026-07-04 session
  (`06-ten-experiments-manifest`) went considerably deeper and still didn't crack it, but sharpened
  the picture: the magnitude wall applies to body motion generally (not just props), VACE 1.3B cannot
  manifest new solid limb structure regardless of control shape or strength, and chaining low-magnitude
  clips avoids melting at the cost of the model barely moving at all.

## Meta-learnings (the expensive ones — internalize these)
1. **STILLS LIE. Judge braille playback, across the WHOLE arc.** Repeatedly a clip looked clean on 3
   spot-frames and fell apart in motion (double-sword, melted mid-frames). Screen every few frames AND
   watch it move in braille before believing anything.
2. **The magnitude wall.** Single-image I2V (Wan 5B) only stays coherent for *small* motion. Large/fast
   motion **melts the whole creature** (not just a prop). Idle/bounce/lunge = OK; fast swing = mush.
3. **The prop wall (separate from #2).** A rigid weapon (sword) melts, doubles, or goes floppy when
   moved — even inside an otherwise-coherent body motion. This is *additional* to the magnitude wall.
   → **Validated fix direction: generate weaponless, attach the weapon after the fact** (see 03).
4. **Text can't specify a specific/novel action.** Prompting "sword slash" / "overhead swing" gives
   melt or the-wrong-motion (a spin). Confirmed by research (arXiv 2510.26794) and empirically. Specific
   motion in the field is controlled by **trajectories or reference motion**, not text.
5. **Generalization is the whole point.** No hardcoded "sword slash" effects — a telepath/pogostick has
   no sword. Solutions must be creature- and action-agnostic.

## How to use `02-dead-ends`
Dead ends are recorded with a **"revisit if"** note. They are *not* permanently closed — most failed
for a specific reason, and a **sufficiently different approach** may get past it. Don't blindly rerun
the exact same thing; do reconsider if you have a new angle on the root cause.

## Note
`../FAILED_EXPERIMENTS.md` is an earlier, partial version of this; this `findings/` folder supersedes it.
