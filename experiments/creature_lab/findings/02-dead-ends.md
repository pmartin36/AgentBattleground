# Dead Ends (tried here, failed) — with revisit notes

Each is failed *as executed*. The **revisit-if** note says what a *sufficiently different* approach
would need to change. Don't blindly rerun; do reconsider with a new angle. User feedback quoted where given.

## Attack-motion generation

**I2V, text prompt "sword slash" (Wan 5B, 17f).** The whole **mouse melted** into a blur mid-swing;
sword vanished. → *Root:* magnitude wall — single-image I2V invents big motion from one frame and smears
everything. *Revisit if:* a model/technique with a real motion prior (see 03), or much smaller motion.

**I2V, contained prompt (33f).** Coherent but only a **wind-up/brandish, never a strike**. Constraints
that stop melting also kill the decisive motion. *Revisit if:* motion specified by trajectory/reference,
not text.

**FLF2V (first→last frame interpolation).** Coherent raise→strike but **lifeless/linear** — no
anticipation (a windup needs direction reversal, impossible from 2 monotonic keyframes). End keyframe
softer. *Revisit if:* 3+ keyframes with a real windup pose (but see next) + easing.

**Multi-keyframe FLF2V chain (windup).** Blocked at keyframe generation: **img2img (strength 0.55) would
not move the sword to distinct poses** — windup ≈ strike, so the chain barely moved. *Revisit if:* a
reliable way to generate distinct action-pose keyframes (higher img2img strength risks identity; pose
control?).

**Dynamic body-motion gen from the SWORDED mouse (bounce/fightbob/lunge, 25f).** Body coherent, but the
**sword-arm melts in transition frames** (`mo_lunge/frame_016` = mush hilt). User: *"mo_lunge has some
really fucked up frames lol."* → Confirms the prop wall inside body motion. *Revisit:* don't — generate
weaponless instead (that works; see 03).

**Weaponless "overhead swing" gen (`wl_swing`).** A dud — the **mouse turned around** (faced away, soft),
didn't do the prompted swing. Text didn't produce the specific motion. *Revisit if:* reference/trajectory
control drives the specific arc instead of text.

## VACE (reference/control video → retarget), Wan 2.1 VACE 1.3B

**Crude synthetic control (ellipse + bar as dense frames).** Produced a **green egg + stick** — VACE V2V
treats control as *dense structure* and overwrote identity with the control's shape. *Revisit if:* the
control structurally matches the subject (real depth/pose), OR a sparse-trajectory control (ATI-style,
not dense) — see 03.

**Low vace-strength (0.5) + synthetic control.** Identity returned but sword = **floppy energy streak**,
pose spun around. Strength trades identity vs control adherence.

**Silhouette-matched control + baked windup (strength 0.8–0.95).** *Stills looked great* (real sword,
identity!) but braille motion = **DOUBLE SWORD** — the control silhouette (derived from the mouse image)
baked in the mouse's *original* sword AND the animated bar. Classic "stills lie." *Revisit if:* control
silhouette excludes the original weapon.

**Swordless reference + control-sword.** Double-sword gone, but **VACE-1.3B can't manifest a solid weapon**
from a control bar → **ghost streak**. *Revisit if:* Wan 2.1/2.2 VACE **14B** (bigger; might manifest a
solid prop; tight on 16 GB, slow, unproven).

## Compositing approaches

**`combat` v1 — static sprites translated.** User: *"those are static sprites that are just being moved…
the only interesting part is the drawn slash."* Dead — paper-doll slide, no body animation.

**`combat` v2 — living idle frames + procedural squash/stretch.** User: *"much improved… but still
generally boring because the sprites are not moving."* Squash/stretch on a subtle idle still reads puppet-y.

**`combat` v3 — lunge-clip state machine.** Coupled the `mo_lunge` clip in, but that clip had the melted
sword frames, and my extra squash/stretch was *"weird condense-y motions."*

**`attach` — composite sword sprite onto hand-authored anchors on `wl_lunge`.** The latest. User:
*"to be honest, this is awful. Maybe the worst yet lol."* The crisp sword *did* bind to the hand and not
melt (mechanic works), but on the arms-out **roar** body pose it read badly, and hand-authored anchors
are loose/manual. *Revisit:* the **decomposition is right** (see 03) — the execution needs a real swing
body-motion + auto hand-detection, not this.

## Skeletal/rig approaches

**Hand-authored 2-bone capsule rig (`rig_arm.py`).** Zero-melt, zero-hallucination by construction
(fully code-driven, no diffusion in the motion path) — but User: *"the rig version sucks, to be
honest... the rig doesn't work. plainly. it's not an approach we can use."* Looked bolted-on (a
synthetic drawn capsule, not the creature's own art style) and required hand-measured pivots/bone
lengths per creature — doesn't generalize. See `08-skeletal-rig-approach.md`. *Revisit if:* never as
a hand-drawn capsule; see the IK+cutout attempt below for the direct successor.

**FABRIK IK + real-texture cutout rig (`ikrig.py`).** The direct fix for the rig's two named flaws:
drove the limb via inverse kinematics against a Cartesian target (not hand-authored angles), and
rendered by cutting the actual creature texture out of the source art (two rigid pieces hinged at the
joint) instead of drawing a synthetic shape. Built a validation gate (catches bad joint placement
before rendering — proven to catch real errors, e.g. a root point floating in background), auto-
measured limb thickness from the creature's own silhouette (with real, fixed biases: measuring
exactly at a socket or a round fist/paw is wrong by construction), and a silhouette-conforming cutout
mask (fixes capsules not fitting claws/branches). Tested clean across 3 diverse body plans (rock-
golem arm, canine leg, branch-limbed humanoid) with every bug found diagnosed to a specific root
cause and mostly fixed — chin-fusion measurement error, oversized auto-thickness blob, floating-twig
debris, pale-ghost compositing, a shadow thread. Static PNG frames looked coherent throughout. **Still
failed in real braille playback** — User: *"that doesn't work. they all look pretty rough."* **Not a
braille-resolution problem** — user confirmed the actual cause: *"it's a problem of unnatural
movement... taking a sprite and breaking it apart so that there's nothing in the joints."* Cutting a
sprite into rigid pieces and hinging them leaves the joint itself unrendered — no material actually
bends there, two flat textures just pivot against each other — and that reads as unnatural motion at
any resolution, independent of every bug above being fixed. *Revisit if:* a renderer that actually
deforms material across the joint (soft blend-skinning/mesh warp, not a rigid hinge between separately
cut pieces) — the rigid-cutout-and-hinge technique itself is the dead end, not its bugs.

## Hardcoded effects

**Authored cyan "slash-arc" effect.** Sword-specific → **not generalizable** (a telepath has no slash).
User flagged this directly. *Revisit as:* a *pluggable effect library* (slash/burst/pulse/projectile),
not one hardcoded arc.
