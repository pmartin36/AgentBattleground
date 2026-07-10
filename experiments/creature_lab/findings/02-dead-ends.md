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

## Hardcoded effects

**Authored cyan "slash-arc" effect.** Sword-specific → **not generalizable** (a telepath has no slash).
User flagged this directly. *Revisit as:* a *pluggable effect library* (slash/burst/pulse/projectile),
not one hardcoded arc.
