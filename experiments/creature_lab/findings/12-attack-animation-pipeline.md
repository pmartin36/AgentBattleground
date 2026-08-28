# Attack Animation Pipeline — locked in

The working end-to-end pipeline: a creature prompt in, a green/magenta-screen attack-animation frame
sequence out. Runs entirely on native `stable-diffusion.cpp` (`sd-cli`) — no Python, ComfyUI, or
PyTorch. Four steps, each in order.

## Step 1 — generate the creature still image

```bash
cd experiments/creature_lab
./generate.sh "<creature prompt>, flat 2D cartoon illustration style, thick black outlines, cel-shaded flat colors, simple low-detail creature" <name>
```

Produces `out/<name>_field_raw.png` (green-screen background) via Z-Image Turbo (`sd-cli`).

**Background color**: default key is green. If the creature's own dominant color is in the green
family (e.g. a green-skinned alien), override to magenta so the chroma-key step can separate character
from background:
```bash
BG="isolated subject, full body, centered, solid flat vivid chroma-key magenta background, uniform bright magenta screen backdrop, no scenery, no environment, no foreground objects, no ground detail, no shadow" \
./generate.sh "<prompt>" <name>
```

## Step 2 — clean the background

```bash
.venv_tools/bin/python tools/cleanbg.py out/<name>_field_raw.png out/<name>_field_clean_bg.png
```

Strips both the flat background and any baked-in drop shadow (hue-direction match + magnitude-ratio
gate, so it doesn't misfire on the character's own black outlines/white highlights or dark shading),
replacing both with a uniform flat color. Always run this before Step 3 — feeding H3 the raw image
directly leaves visible background/shadow bleed in the animated output. (Chroma-key utility; the one
remaining Python touch in the pipeline, a candidate to port to native.)

## Step 3 — animate via MiniMax H3 (native `sd-cli`)

MiniMax H3 image-to-video runs directly through `stable-diffusion.cpp`'s `vid_gen` mode. Four
components passed separately, plus the distilled Turbo LoRA (8 steps, ~3 min/clip):

```bash
cd experiments/creature_lab
stable-diffusion.cpp/build-vk/bin/sd-cli -M vid_gen \
  --diffusion-model models_sdcpp/minimax_h3_fl2va_pruned-Q4_K_M.gguf \
  --vae ComfyUI/models/vae/minimax_h3_video_vae_fp16.safetensors \
  --audio-vae ComfyUI/models/vae/minimax_h3_audio_vae_fp32.safetensors \
  --llm models_sdcpp/qwen3vl_32b_minimax_h3-Q4_K_M.gguf \
  --lora-model-dir ComfyUI/models/loras \
  --init-img out/<name>_field_clean_bg.png \
  -p "<prompt, see template> <lora:minimax_h3_turbo_v4_step600_ema:1.0>" \
  --cfg-scale 1.0 --flow-shift 12.0 --strength 1.0 --sampling-method euler --seed 42 \
  -W 512 -H 512 --diffusion-fa --offload-to-cpu --rng cpu \
  --clip-on-cpu --vae-tiling --temporal-tiling \
  --fps 24 --video-frames 56 --steps 8 \
  -o out/<name>_<action>/anim.png
```

Notes:
- Weights are the sd.cpp-format GGUFs from `leejet/MiniMax-H3-GGUF` (`models_sdcpp/`). ComfyUI-ecosystem
  GGUFs use a different tensor layout and will not load (sd.cpp reads the DiT hidden size as 1 and fails
  metadata validation).
- `--clip-on-cpu` is required — the 32B Qwen3-VL text encoder exceeds 16GB VRAM if placed on the GPU.
- `--strength 1.0` matters: sd.cpp's default of 0.75 attenuates motion range.
- Resolution is sized to fit VRAM, not for detail (output is down-rezzed to braille). 512² fits without
  streaming; the full 768²×124 source recipe fits a 16GB card only via `--stream-layers --max-vram`, at
  ~20× the wall-clock.
- Backend is Vulkan (the build under `stable-diffusion.cpp/build-vk/`). It is the cross-vendor path and
  the working one on Blackwell cards, where the installed CUDA toolkit predates the GPU.

**Prompt template** (the style-preservation language is load-bearing — it's what keeps identity/art
style consistent through the generated motion):
```
A <creature description>, flat 2D cartoon illustration style with thick black outlines and cel-shaded
coloring, camera locked static straight-on. <Specific action, described beat-by-beat: windup/anticipation,
the main motion with concrete physical detail, impact/extension, brief follow-through>. The creature's
proportions, colors, and flat cel-shaded illustration style stay fully consistent and unchanged
throughout the entire shot. The background remains a solid flat <green/magenta> screen color the whole
time, no scenery, no camera movement.
```

Write the action description as specific physical beats, not a vague verb ("attacks fiercely" fails;
"winds up by pulling its fist back, rotates its hips, throws a punch with the arm fully extending at
impact, holds a brief follow-through" works). Motion magnitude comes from beat-by-beat action language,
not from motion-adjective padding ("extreme", "maximum dynamic"). A load-bearing pose (e.g. a handstand)
keeps those limbs planted by physical plausibility.

## Step 4 — extract frames and view

```bash
ffmpeg -i out/<name>_<action>/anim.avi -vf fps=24 out/<name>_<action>/frames/f_%03d.png
../ascii_test/target/release/playframes out/<name>_<action>/frames --chroma auto --fps 24
```

`playframes` also accepts multiple clip directories for side-by-side comparison — Left/Right arrows
switch between them, `q` quits.

## Verified working

Golem (punch, ground-slam), wolf (lunge/bite), treant (claw-swipe), mouse-with-sword (held prop —
historically the hardest failure mode in this project — a clean, full success in real playback),
alien-on-a-pogostick (novel creature, bounce + gunfire), dragon-with-katana (leaping slash, held prop
solid through the swing). Reproduced natively via `sd-cli`, judged by real braille playback.

## Runtime notes

- **~3 min/clip** on a 16GB GPU (Vulkan), reliable across repeated runs with VRAM returning to idle
  baseline between each; peak ~14.6GB. One-time bake per creature/action at creation time, not a
  live/real-time step.
- An out-of-VRAM condition is recoverable: retry with graph-cut streaming (`--stream-layers --max-vram`)
  or a smaller canvas.
- A separate, coarser background grain was seen during earlier generation runs — cosmetic, not chased,
  and not confirmed on the sd.cpp path.
