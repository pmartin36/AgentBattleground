# Proven Pipeline (facts only)

Everything here was **either run and its output confirmed** (by the user's eyes and/or observed frames),
or **established by web research**. Speculation and "next ideas" live in `03-promising` — not here.
Confidence is tagged: **[OBSERVED]** = ran, output seen/confirmed. **[RESEARCH]** = from web search
(see the caveat at the bottom — the research pass was truncated and is incomplete).

## The one end-to-end pipeline that produces a good result: creature → idle → braille

This is the *only* fully-working, user-confirmed loop. Run from `experiments/creature_lab/`.

### 1. Generate a creature — `generate.sh` [OBSERVED]
```bash
./generate.sh "a round chunky slime monster, flat colors, bold outlines, low detail" slime
```
- Runs **Z-Image Turbo** txt2img (high-detail) + **img2img-simplify** (battlefield form) + rembg cutout +
  braille preview. Background defaults to **green screen**.
- Produces in `out/`: `<name>_hi_raw.png`, `<name>_hi.png`, `<name>_field_raw.png`, `<name>_field.png`.
- Confirmed to produce clean low-detail creatures: `greenmouse`, `slime`, `swordless` all came from this.
- Env knobs actually used: `BG` (background prompt), `STRENGTH` (img2img denoise, default 0.5),
  `FIELD_W`/`HI_W` (braille preview widths).

### 2. Animate its idle — Wan 2.2 TI2V-5B I2V [OBSERVED]
`animate.sh` exists (`./animate.sh out/<name>_field_raw.png "idle breathing, tail sway" <name>`) but it
ends by launching the interactive player. For headless generation the **exact command that worked** was
`sd-cli` directly (this produced `greenmouse_anim` and `slime_anim`, which the user confirmed as a good idle):
```bash
stable-diffusion.cpp/build/bin/sd-cli -M vid_gen \
  --diffusion-model models/Wan2.2-TI2V-5B-Q4_K_M.gguf \
  --vae models/wan2.2_vae.safetensors \
  --t5xxl models/umt5-xxl-encoder-Q8_0.gguf \
  -i out/<name>_field_raw.png \
  -p "idle breathing, subtle sway, gentle motion, static locked camera, subject centered, solid flat vivid green screen background, simple flat-color low-detail creature" \
  -n "lowres, blurry, extra limbs, deformed, flickering, changing background, camera motion, zoom, pan, watermark, text" \
  --cfg-scale 6.0 --sampling-method euler --flow-shift 3.0 \
  -W 512 -H 512 --video-frames 17 --diffusion-fa --offload-to-cpu --vae-on-cpu -v \
  -o out/<name>_anim/frame_%03d.png
```
Load-bearing, verified by failure/success:
- **`--vae-on-cpu`** — without it, Wan VAE decode OOMs on 16 GB → exit 0, empty output dir. [OBSERVED]
- **`--offload-to-cpu`** — keeps diffusion weights off the GPU. [OBSERVED]
- **`--video-frames` must be `4n+1`** (17, 25, 33). [OBSERVED — non-4n+1 rejected]
- Green background in the prompt → keyed later with `--chroma auto`. [OBSERVED]
- Runtime ~2–4 min/clip; **no frames are written until the final VAE decode** (dir stays empty mid-run —
  don't kill it). [OBSERVED]

### 3. View in braille — `playframes` [OBSERVED]
```bash
../ascii_test/target/release/playframes out/<name>_anim --chroma auto --pingpong --fps 12
```
- `--chroma auto` samples the green per frame (handles frame-to-frame drift). [OBSERVED — better than
  white, but NOT solid: green flashes/leaks still occur. Keying is an open problem, not a finished win.]
- `--ease anticipate` redistributes playback timing (slow→snap→settle); user confirmed **"much improved"**
  on lifeless motion. [OBSERVED]
- `--pingpong` for loops; omit for one-shot forward.

## Other confirmed-working commands
- **Still → braille:** `downrez <img.png> --width N --chroma auto` [OBSERVED]
- **GIF/threshold A/B:** `anim` — `t` cycles adaptive `>`/`>=`/global/bayer; side-by-side original. [OBSERVED built+runs]
- **Crowd/parallax:** `flow` [OBSERVED]

## Confirmed engine/hardware facts [OBSERVED]
- Built `stable-diffusion.cpp` with **Vulkan** (`cmake .. -DSD_VULKAN=ON`) — CUDA build failed because the
  5070 Ti (Blackwell `sm_120`) needs CUDA toolkit ≥12.8 and the apt one was older. Vulkan worked.
- On 16 GB: Wan 2.2 TI2V-5B, Z-Image Turbo, and Wan 2.1 VACE 1.3B all run (with the offload flags above).
- VACE: the **GGUF** 1.3B failed to load (`vace_patch_embedding` dropped — a 5-D-tensor read bug); the
  **safetensors** version loaded and ran. VACE 1.3B requires the **wan 2.1** VAE (not the 2.2 one).
- `img2img` (Z-Image, `--init-img ... --strength ...`) at strength ~0.55 **preserves composition** — it
  did NOT move a held sword into a new pose. [OBSERVED — this is why the keyframe approach failed]

## Confirmed RESEARCH facts [RESEARCH]
From the deep-research pass (sources in `03-promising`):
- Open models produce **implausible/generic motion for out-of-domain/novel actions** — text can't reliably
  specify an arbitrary action (arXiv 2510.26794).
- Specific motion is controlled via **trajectories (ATI, Motion Prompting) or reference motion (VACE,
  Go-with-the-Flow)**, not text.
- The diffusion MSE objective is ~invariant to temporal coherence → base I2V motion is lifeless unless a
  motion prior is added (VideoJAM, MoGAN, Go-with-the-Flow).
- ATI trajectory control is demonstrated on **Wan 2.1-14B** (bears on 16 GB feasibility).

## ⚠️ Research-completeness caveat (deepen this)
The deep-research workflow **hit a session limit mid-run**: 5 search angles + ~21 sources fetched, but only
**4 of 25 claims got full adversarial verification** and the **synthesis step failed**. So the research is
**partial**, skewed toward what the first searches surfaced (heavy on ATI/VACE/Wan). Likely gaps to go
deeper on next time:
- Hands-on results/quality of **VACE V2V with a *real* depth/pose reference** (we only reasoned about it).
- Whether **ATI or an equivalent trajectory control runs in stable-diffusion.cpp** (vs ComfyUI-only) — this
  gates the whole trajectory path for shipping.
- **Auto hand/pose detection** options that run locally (DWPose, DepthAnything, MediaPipe) for the attach path.
- Newer local video models / motion-control methods past the first-page search results.
Re-run a full (non-truncated) research pass focused on those before committing to an approach.
