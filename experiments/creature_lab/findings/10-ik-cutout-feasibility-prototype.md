# IK + Cutout Feasibility Prototype (2026-07-08)

Built the end-to-end pipeline approved after the rig-approach rejection: joint points (3-point
chain: root/joint/tip) → FABRIK inverse kinematics solving the chain against a moving Cartesian
target each frame → render by cutting the actual limb pixels out of the source art as two rigid
pieces hinged at the joint, rotated into the IK-solved pose. Two differences from the rejected
`rig_arm.py` approach on purpose:
- **Driven by IK against a Cartesian target, not hand-authored joint angles.** `tools/ikrig.py`'s
  `fabrik_solve()` is a straight implementation of Aristidou & Lasenby's FABRIK (forward-reach to
  target, backward-reach to root, repeat) — general to any N-joint chain, not hardcoded to 2 bones.
- **Renders by cutting real art out of the source image, not drawing a synthetic capsule.** Each
  frame is: erase the limb's rest-pose footprint from the body (chroma-key-color fill, feathered),
  then paste two rotated RGBA cutout pieces (upper segment + lower segment, each with its own pivot)
  back on top. The pieces carry the original rock/fur/flame texture, directly targeting the "sucks,
  looks bolted-on" complaint about the hand-drawn capsule.

## What was built
`tools/ikrig.py` — usage: `ikrig.py <body_image> <out_dir> --root x,y --joint x,y --tip x,y --wind x,y
--strike x,y [--thickness a,b,c] [--frames 25]`. Root/joint/tip are the rest-pose 3-point chain
(shoulder/elbow/wrist or hip/knee/paw); wind/strike are Cartesian targets for the tip at the windup
and strike poses — the elbow/knee position is *solved*, never specified.

Tested on two creatures with different body plans, joint points taken from the same manual
measurement approach used to validate `rig_arm.py` (standing in for the vision-LLM pointing step,
already separately validated in `09-exhaustive-search-plan.md` across 6 creatures):

- **Stone golem, right arm, haymaker punch** — `out/ik_golem_punch` (25 frames).
  `--root 175,205 --joint 100,390 --tip 150,550 --wind 30,430 --strike 420,470 --thickness 210,135,180`
- **Ember wolf, front leg, forward paw swipe** — `out/ik_wolf_swipe` (25 frames).
  `--root 310,330 --joint 330,470 --tip 335,610 --wind 280,480 --strike 460,555 --thickness 60,45,70`

```bash
cd experiments/creature_lab
../ascii_test/target/release/playframes out/ik_golem_punch --chroma auto --pingpong --fps 12
../ascii_test/target/release/playframes out/ik_wolf_swipe --chroma auto --pingpong --fps 12
```

## Bugs hit and fixed during the build (real, disclosed)
1. **Capsule thickness far too narrow for the golem's blocky rock arm** (auto-estimated as a
   fraction of bone length, ~124px, vs. the arm's actual ~190-210px width). Erased only a thin
   channel down the middle of the arm and cut equally-narrow pieces — at a glance this reads as
   "the arm didn't move" because most of the original static arm's silhouette (the un-erased rind
   around the edges) stays put, and the new narrow piece is easy to miss overlapping the torso.
   Fixed by measuring the true pixel width off the reference image and passing `--thickness`
   explicitly. **This is a real generalization gap**: thickness can't be auto-derived from bone
   length alone for blocky/irregular creatures — it needs its own estimate (a silhouette-width
   scan at each joint would be the automatic fix, not yet built).
2. **Erase-fill color mismatch left a visible smear.** `rig_arm.py`'s trick of sampling background
   color from a ring near the erase box (built for photo-like art with soft gradients) picks up
   dark body/shadow-edge pixels on these flat chroma-key renders, muddying the fill. Since this
   project's asset pipeline always renders on a flat, uniform chroma-key screen, switched to
   sampling the four image corners directly — exact match, smear gone.

## Result
Both clips show real bend-and-swing motion (windup → cross/swipe → follow-through) with the elbow/
knee genuinely bending via IK, rendered from the actual creature texture rather than a foreign drawn
shape. Static-frame review (0/5/10/15/20/24) looks like a legitimate attack motion on both a biped
arm and a quadruped leg. **Not yet judged in real braille playback** — per this project's own
"stills lie" rule, this is not a verdict, just a readiness report. Needs the owner's own
`playframes` check before it counts as validated.

## Known rough edges (not fundamental, same category as the original rig writeup)
- A faint soft-edged seam is still visible at the erase-hole boundary in a few frames — better than
  the original mismatched-color smear, but the Gaussian feather (6px) could still be tightened.
- Thickness is hand-measured per creature/limb here, same manual-measurement complaint leveled at
  the rejected rig — except now it's ONE number (a width estimate) instead of a full bone-length +
  pivot-angle timeline, and it's a much more automatable quantity (a silhouette cross-section scan
  at the joint coordinates could derive it directly, not yet built).
- Two rigid hinged pieces (not continuous skinning/blending across the joint) — visibly a "cut" at
  the elbow/knee up close, though far less foreign-looking than the old capsule since both pieces
  are real source texture. Linear blend skinning (soft weight blending near the joint) was the
  research-recommended upgrade if the rigid-hinge look reads as a seam in practice.
- Root/joint/tip and wind/strike targets are still hand-entered per limb/motion here; the next
  integration step (not yet built) is feeding these from the already-validated vision-LLM pointing
  step instead of manual pixel coordinates.

## Round 2 (2026-07-08, "last attempt" stress test) — productionization dry run

User called manual measurement "doomed" for an autonomous pipeline and asked for a concrete plan,
then to actually build it. Built the two load-bearing pieces (auto thickness, a validation gate)
and stress-tested joint placement blind on a third, harder creature. Real findings, not a clean
win:

**The wolf clip's "leg attached to face" bug was a genuine, diagnosable measurement error, not a
technique flaw.** Root cause: my hand-picked `--root` (310,330) landed almost exactly on the wolf's
chin, not the shoulder — confirmed by cropping and looking, not guessed. The cutout mask at that
root point necessarily grabbed chin fur, which then swung with the leg. Re-measuring against the
actual leg silhouette (root moved to where the leg visibly separates from the body, ~y=465) fixed
it outright. This is exactly the failure mode the user was worried about — a single bad coordinate
produces a grotesque, not just mediocre, result — which is why validation can't be optional (below).

**Built `auto_thickness_at()`**: measures real cross-section width by ray-casting outward from the
bone at a few points, using a corner-keyed + solid-filled foreground mask (same family of technique
as `mkanchors.py`, with an added largest-external-contour fill since raw chroma-keying punches small
false holes in dark interior texture/shadow, which breaks a straight ray-cast). Needed two rounds of
correction before it was trustworthy:
1. Measuring exactly at the socket (t=0) or exactly at a round terminal mass (t=1) is wrong by
   construction — the socket blends into torso/hip width, and a fist/paw's width isn't captured by
   a single-axis perpendicular slice. Fixed by insetting the sample point and fanning the scan angle
   ±20° to catch round bulges.
2. Even after that, a short bone (the wolf's 42px upper leg) still measured mostly torso — confirmed
   this by rendering it (root auto-measured at 275px, nearly the width of the whole body) before it
   ever got clamped. **Added a hard clamp: no limb segment can be thicker than its own bone length,
   or more than 2x its neighboring joint's width.** This clamp visibly mattered — without it, the
   golem's auto-measured root thickness (307px, wider than the 199.6px bone itself) rendered as an
   oversized blob that visually swallowed the whole swing regardless of the angle underneath.

**Built a validation gate** (`--no-validate` to bypass): rejects any root/joint/tip that doesn't
land on the creature's own foreground silhouette, and warns if `root` sits in the top 15% of the
creature's own bounding-box height (shoulders/hips are essentially never there). **This gate caught
a real error during the stress test below, before any rendering happened** — not a synthetic test.

**Blind stress test, third creature (shadow_cat, not previously hand-tuned this session):**
one-shot joint estimate by eye → overlay check → the validation gate's own "does this touch the
silhouette" logic mirrors what I did by hand: my first `root` guess (270,430) was floating in
background/smoke, off the body entirely. Caught it on inspection, corrected to (278,458), re-checked,
passed. **This is the intended workflow working as designed** — propose, then verify before
rendering, not trust the first guess.

**But auto-thickness then failed completely on this same creature**, and this is the important
negative result: root/joint/tip all measured ~297-300px (nonsense — the leg is nowhere near that
wide). Cause, confirmed by rendering the foreground mask directly: the cat is a near-black creature
with a ground shadow rendered in the same dark tone family, and viewed front-on with legs bunched
close together — the chroma mask fuses the legs, the shadow, and the belly into one undifferentiated
blob with no internal gaps. A perpendicular ray-cast from any point on that leg just keeps hitting
"foreground" in every direction for a long distance. The clamp prevents this from rendering a
catastrophe, but it can't produce a *correct* measurement either — it just falls back to a generic,
not-actually-measured proportion.

## Final assessment
Two of three things needed for autonomy now have real, working implementations with evidence behind
them (validated joint placement via propose-then-verify; a hard-clamped auto-thickness that is
accurate on clean, separated, light-toned silhouettes). The third — auto-thickness on **dark-colored
creatures with a rendered drop shadow and/or front-on bunched limbs** — does not work, and the clamp
only hides the failure rather than fixing it. This is not a rare edge case for player-generated
creatures; dark palettes and shadows are common art choices, not a narrow exception.

Recommendation: **this specific weak point (shadow/dark-silhouette fusion) needs a fix at the asset
generation stage, not downstream** — e.g. the creature generator emitting a clean alpha matte without
a baked-in drop shadow, so limb separation doesn't depend on color-distance-from-background at all.
Absent that, the pipeline needs a pre-flight confidence check per creature (can auto-thickness find
a real local minimum/maximum along the bone, or is it flatlined at the mask's outer bound?) that
falls back to the effects-only track for any creature it isn't confident about, rather than forcing
a guessed proportion through. This isn't proven un-buildable — it's the next concrete gap, not a
dead end, but it is real remaining scope, not a finished pipeline.

## Round 3 (2026-07-08, same session) — fairer stress test + capsule-vs-silhouette mismatch

User corrected the round-2 verdict: shadow_cat was a bad test on two independent counts — it's
sitting on its haunches (foreshortened, hard to animate regardless of technique) and dark palettes
will be disallowed in the creature-generation prompt going forward, which removes the exact
shadow-fusion failure mode found above at the source. Re-tested on `verdant_treant` (standing,
light-toned wood palette, asymmetric branch-like anatomy) instead — a fair test the earlier one wasn't.

**Blind one-shot joint estimate, then the propose-then-verify workflow caught nothing wrong this
time** (root/joint/tip all landed cleanly on the arm on first inspection) — a good sign the earlier
catches weren't lucky. `ikrig.py`'s own mechanical validation gate then rejected it anyway:
`foreground_mask_cv`'s morphological cleanup (`MORPH_OPEN`, meant to strip small chroma-key speckle
noise) was eroding the treant's thin wiry claw-fingers enough to disconnect them from the arm, so the
solid-fill-largest-contour step (added in round 2 to patch internal shadow holes) dropped them
entirely — the validated joints were correct, the *mask* was wrong. Removed `MORPH_OPEN` (kept only
`MORPH_CLOSE`, which fixes small holes without eroding thin protrusions) and the fingers survived
intact.

**With that fixed, the render itself surfaced the real, novel finding**: a capsule is the wrong
cutout shape for non-tapering anatomy. The rendered claw left a small disconnected "severed twig"
each frame — a sliver of the original, un-erased hand that the idealized smooth-taper capsule mask
didn't fully cover, since real claws/branches splay irregularly rather than tapering. **Fixed properly,
not patched**: added `conform_to_silhouette()` — build the capsule as a generous (1.7x) region-of-
interest, then intersect it with the creature's own true silhouette, so the cutout follows the actual
limb shape instead of an idealized taper. This is the correct general fix (it also means auto-
thickness's estimated numbers only need to *localize* the limb, not perfectly describe its shape).

That introduced one new artifact (a translucent "pale ghost" trail) — traced to the intersection's
2px edge feather, which meaningfully desaturates a structure only ~10px wide when composited against
a saturated background; set that feather to 0 by default and the piece rendered fully opaque again.

**Re-running golem/wolf through the updated pipeline (no regressions on the previously-working cases)
surfaced one further real, still-unresolved issue**: a small ambient shadow under the golem's fist —
dark enough to pass the background-distance threshold on its own — survives as a thin thread attached
to (not disconnected from) the fist, so a connected-component filter (added to drop genuinely separate
shadow fragments, which it does) doesn't catch it. Tried shrinking it with a small morphological
erosion: a 3x3 kernel removes the golem's shadow thread but also erodes away most of the treant's
real, similarly-thin fingers — confirmed directly by testing both. **A 2x2 kernel doesn't erode the
treant's fingers, but also doesn't remove the golem's shadow.** There is no single geometry-only
threshold that cleanly separates "thin real anatomy" from "thin shadow artifact" — sampled the
shadow's own pixel colors to check whether a saturation/luminance rule could split them, but ran out
of scope this session to build and validate that rule. Left as a disclosed, understood, not-yet-fixed
minor cosmetic issue (a thin gray line, not a body-fusion-level defect) — golem's clip still ships
with it visible.

**Net result this round**: three distinct body plans (rock-golem arm, canine leg, branch-limbed
humanoid) now render through the same pipeline — validated joints, auto-measured and clamped
thickness, silhouette-conforming cutout — with only one known, minor, disclosed cosmetic defect
remaining (not a structural one). Every new creature tested this session surfaced at least one new,
previously-unseen failure mode; none were technique-breaking, all were diagnosed to a specific,
explainable root cause and mostly fixed outright. That pattern — steady, explainable bugs, not a
widening set of unfixable ones — is itself the most useful evidence for or against production
viability: it says the approach is fixable but not yet exhausted, and a real pipeline should expect
to keep finding this kind of thing on further creatures, not assume round 3 was the last one.
