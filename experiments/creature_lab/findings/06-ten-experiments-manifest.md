# Ten Animation Experiments — Manifest (2026-07-04 session)

Extensive-research pass into unresolved attack-animation problem (see `00-overview.md` through
`05-proven-pipeline.md`). Four parallel web-research forks investigated: trajectory-control models,
real pose/depth VACE retargeting, offline hand-tracking for stylized sprites, and prop-melt fixes —
see the "research findings" section below for what they turned up. From that, 10 concrete approaches
were designed and actually run on this machine. Results below are reported honestly — several are
negative/melted, which is real signal, not a failure to hide.

**Load and judge every clip in braille motion, not stills** — this is the #1 meta-learning from the
prior session and it held again here (see per-clip notes below).

```bash
cd experiments/creature_lab
PLAY=../ascii_test/target/release/playframes
$PLAY out/<dir> --chroma auto --pingpong --fps 12          # loop
$PLAY out/<dir> --chroma auto --fps 12                       # one-shot forward
$PLAY out/<dir> --chroma auto --fps 12 --ease anticipate      # windup/snap/settle timing
```

## The 10 experiments

### 1. Higher frame-count direct attack — `out/exp1_highframe_attack` (49 frames)
`greenmouse_field_raw.png` (sworded mouse) → Wan 2.2 I2V, 49 frames instead of the usual 17, prompt
pushed toward a real strike ("quick decisive downward sword strike").
**Result: FAILED.** Fully melted into an indistinct yellow-green blob by frame 12 of 49, and stayed
melted/frozen for the rest of the clip. **More frames did not reduce prop-melt — if anything this was
worse and faster than prior 17-frame attempts.** Frame count is not a fix for the magnitude wall.

### 2. Chained low-magnitude hops — `out/exp2_chained_attack` (34 frames, 2×17-frame hops concatenated)
`greenmouse_field_raw.png` → hop 1 (subtle raise/anticipation) → hop 1's last frame → hop 2 (told to
"swing down and strike").
**Result: MIXED.** Zero melting in either hop — the sword stayed crisp and undistorted throughout.
But there is also next to **zero visible motion** — frames 0/8/16 of each hop are nearly identical
even with an assertive strike prompt in hop 2. Chaining fully avoided the magnitude wall, but the
model would rather sit still than move a held prop. Real trade-off, not a win.

### 3. Weaponless small-magnitude thrust — `out/exp3_thrust` (25 frames)
`swordless_field_raw.png` → Wan I2V, prompt: "quick forward lunge thrust... snappy energetic motion."
**Result: FAILED.** Melted into an unrecognizable smear by frame 24, despite having no held prop at
all. Confirms the magnitude wall applies to body motion generally — stacking intensifier words
("quick", "sharply", "snappy", "energetic") was enough to blow past it even weaponless.

### 4. Auto-anchor tracking retrofit on `wl_lunge` — `out/exp4_wllunge_autoattach_fx` (25 frames)
New classical-CV tool `tools/mkanchors.py` (silhouette + mid-height-band leftmost/rightmost point +
temporal continuity — no ML pose model, generalizes to any body plan) replaces the old hand-authored
anchors that were previously called "awful." Applied to the already-clean `wl_lunge` clip, sword
attached via `attach`, slash-effect layered via new `tools/slasheffect.py`.
**Result: MECHANIC VALIDATED, motion still not attack-shaped.** The sword tracks the paw smoothly
with no melting or lag — a real improvement on the old manual-anchor "awful" result. But `wl_lunge`'s
actual body motion is an arms-out "dab," not a swing, so it reads as "mouse holding a sword out to the
side" rather than attacking. The auto-tracking is solved; a real swing body-motion still isn't.

### 5. Effects-library reliable floor — `out/exp5_effects_floor` (25 frames)
`wl_bounce` (re-verified clean this session, not just trusted from old docs) + auto-tracked sword
attach + generalized (non-hardcoded) motion-trail effect + timing via `--ease anticipate` at playback.
**Result: WORKS as a non-generative baseline.** Sword tracks cleanly, no melting. The motion-trail
effect didn't fire (its angular-velocity trigger never crossed threshold on this gentle bounce) —
tunable via `--vel-thresh`, not a blocker. This is the safe fallback: zero diffusion gamble, at the
cost of not being a real attack pose either.

### 6. Generalization test on a different creature — `out/exp6_wolf_attack` (17 frames)
`ember_wolf_field_raw.png` (naturally weaponless quadruped) → Wan I2V, "forward pounce lunge, baring
teeth, snapping bite."
**Result: MIXED, informative.** The wolf's body itself (legs, tail, torso) stayed fully coherent — no
melting — supporting the hypothesis that melting is concentrated on complex articulated/prop regions
rather than being universal. But two disconnected floating yellow flame-blob artifacts were
hallucinated next to the wolf in frames 8 and 16 — a distinct new failure mode (spurious detached
debris) not seen in the mouse experiments.

### 7. VACE with a proportioned 2-bone arm control — `out/exp7_vace_arm` (25 frames)
New `mkslash3` (replaces `mkslash2`'s single straight bar with a tapered shoulder→elbow→wrist arm,
proportioned to the mouse's own limb length — same-topology, not an imported human skeleton, per the
research finding that cross-morphology retargeting is unvalidated/risky). Run against the **swordless**
identity (no baked original sword this time) at `--vace-strength 0.85`.
**Result: PARTIAL.** Identity preservation is excellent — crisp, undistorted mouse, and critically
**no double-sword artifact**, confirming the hypothesis that the old double-sword failure came from the
baked original weapon, not from using a real/structural control. But the new arm itself only manifests
as a faint thin line, not a solid limb.

### 8. VACE strength sweep — `out/exp8_vace_strength95` (25 frames)
Same control as #7, `--vace-strength 0.97`.
**Result: Confirms #7's limitation is model capacity, not strength.** Nearly identical to the 0.85 run
— the arm is still just a thin line. Wan 2.1 VACE **1.3B** appears unable to manifest new solid limb
structure from a control video regardless of strength in the 0.85–0.97 range (consistent with the
older sword-control dead-end finding, now generalized beyond swords). The 14B VACE checkpoint is the
next thing to test, but is heavy for 16GB and unproven here.

### 9. FLF2V redux with real distinct keyframes — `out/exp9_flf2v` (17 frames)
Old FLF2V attempts were blocked because img2img couldn't produce genuinely distinct windup/strike
keyframes (`kf_windup.png`/`kf_strike.png` are nearly pixel-identical — confirmed again this session).
This time: `exp7_vace_arm`'s frame 0 (rest) and frame 24 (arm extended) — genuinely different poses —
fed as `--init-img`/`--end-img` to Wan 2.2 TI2V-5B (not an official FLF2V model, but it accepted the
flags and ran).
**Result: MECHANISM WORKS, payoff limited by inputs.** It genuinely interpolated toward the end
keyframe's pose (previously impossible) — a real unblock of the old dead-end. But since the source
keyframes' arm was only ever a faint line (inherited from #7/#8's VACE-capacity ceiling), the result
inherits that same weakness. Worth revisiting once a stronger keyframe source exists. Minor color
drift frame-to-frame also visible.

### 10. LTX-2 trial — `out/exp10_ltx2`
Newly-discovered lever from research: **stable-diffusion.cpp added LTX-2 support since the last
findings pass.** Downloaded the distilled-1.1 Q4_K_M quant (~14GB) + matching VAE/audio-VAE/embeddings
connector + gemma-3-12b-it text encoder (~7GB) — ~25GB total, all fit on disk, loaded successfully
with `--offload-to-cpu --vae-on-cpu` on the 16GB card. Ran on `ember_wolf_field_raw.png` with the same
attack prompt as #6, for a same-creature comparison against Wan.
**Result: FAILED — not practically usable on this hardware.** Three attempts:
1. Plain run: OOM'd — the 22B model staged its full 14.35GB to VRAM at once (no per-layer streaming
   the way Wan does it) and left no room for the compute buffer.
2. Added `--max-vram -2` (forces graph-cut segmented execution): this fixed the OOM — sampling
   actually **completed successfully** in 153s. But it then crashed during **audio** VAE decode
   (`GGML_ASSERT(src0->type == GGML_TYPE_F16) failed`) — LTX-2 is an audio-video model and tries to
   synthesize an (unwanted, unneeded) audio track alongside the frames.
3. Dropped `--audio-vae` to skip the audio path entirely: got further, but graph-cut segment reloads
   from disk got progressively slower (final segment took **1040 seconds / 17+ minutes** just to
   reload ~12GB of tensors from disk, likely disk-cache thrashing from repeatedly swapping segments
   in and out under `--offload-to-cpu`), and the process died before completing — no frames written.

**Verdict:** LTX-2 (this quant, this hardware) is technically loadable and can sample once VRAM is
segmented correctly, but the repeated full-segment disk reloads make it impractically slow — a single
clip attempt burned over 20 minutes without finishing, versus ~5-20 min for a complete Wan clip. Not
recommended to retry as-is; would need either a smaller quant, faster storage, or an engine-side fix
to keep segments resident in RAM across the graph cut rather than re-reading from disk each time.

## Research findings that shaped this list (full detail in the 4 fork transcripts, summarized here)
- **No trajectory-control model (ATI, DragAnything, MotionCtrl, Tora) is portable to our sd.cpp/GGUF/
  16GB stack today** — all are ComfyUI + 14B-class only. Not chased.
- **Cross-morphology pose retargeting (real human motion → non-human subject) is an unvalidated,
  actively-researched problem, not a solved VACE use case** — this is why experiment #7 used a
  same-topology proportioned arm rather than importing real human mocap/pose data.
- **Classical CV (silhouette + convex-hull/extreme-point + temporal continuity) beats ML pose models**
  for stylized/non-human sprites — MediaPipe/OpenPose/DWPose are trained tightly on real human photos
  and don't transfer. This directly produced `tools/mkanchors.py`.
- **Chained short low-magnitude clips** had the strongest practitioner evidence for avoiding the
  magnitude wall — confirmed here (#2), though the "avoids melting" win came with a "barely moves"
  cost that the research didn't fully anticipate.
- A **Civitai Wan2.2 pixel-sprite-attack LoRA** exists (trained on ~30 melee slash sprite clips,
  matching our aesthetic) but targets the 14B I2V model — heavier than our 5B, not attempted this
  session; a candidate for a future pass if 14B becomes feasible.

## New tools added this session
- `experiments/ascii_test/src/mkslash3.rs` (bin `mkslash3`) — proportioned 2-bone arm VACE control,
  replaces the single-bar `mkslash2` for arm/limb motion.
- `experiments/creature_lab/tools/mkanchors.py` — classical-CV auto hand/paw tracker (no ML), outputs
  `attach`-format anchors.txt from a green-screen frame sequence + one seed point.
- `experiments/creature_lab/tools/slasheffect.py` — generalized (non-hardcoded) motion-trail effect,
  anchored to whatever `mkanchors.py` tracked; fires when angular velocity crosses a tunable threshold.
- `experiments/creature_lab/.venv_tools/` — Python venv (opencv-python-headless, numpy, Pillow) for the
  above dev-time tools. Not part of the shipped game; same dev/runtime split as the rest of the motion
  tooling in this project.
- `experiments/creature_lab/models/ltx2/` — LTX-2.3 distilled-1.1 Q4_K_M + companion VAE/audio-VAE/
  embeddings-connector/gemma-3-12b-it-Q4_K_M text encoder.
