# What Works (solid — build on these)

## Local generation stack
- **stable-diffusion.cpp** (ggml/GGUF, pure C/C++, no Python, sandboxable subprocess) — the engine
  the game ships. Built with **Vulkan** (`-DSD_VULKAN=ON`) on the 5070 Ti (Blackwell `sm_120` needs
  CUDA 12.8+, which the apt toolkit lacked — Vulkan sidestepped it). Backends also: CUDA/HipBLAS/Metal/CPU.
- **Runs on 16 GB** with `--offload-to-cpu --vae-on-cpu`: Wan 2.2 TI2V-5B (image+video), Z-Image Turbo
  (stills), Wan 2.1 VACE 1.3B. (Blog claims "Wan needs 24 GB" are about the 14B models.)
- **VAE decode is the VRAM peak for video** and OOMs on 16 GB → `--vae-on-cpu` (slower but reliable).
  Symptom of the OOM: exit-0 but empty output dir (`vae decode compute failed`). GGUF loader also has a
  5-D-tensor read bug that drops VACE's `vace_patch_embedding` — use the **safetensors** VACE, not GGUF.

## Creature generation
- **Z-Image Turbo** makes great **low-detail** creatures (flat colors, bold outlines) — ideal for braille.
- `generate.sh` = hi-detail creature + img2img-simplified battlefield form, both braille-previewed.
- **Two fidelities**: high-detail (creature viewer) + simplified (battlefield) are *two generated
  images*, not one downscaled — a detailed image shrunk to braille is mush; a simple one is crisp.

## Braille render pipeline
- 2×4 dot cells, 24-bit color, native alpha. Binaries: `downrez` (still→braille), `playframes`
  (frame-dir/gif player), `anim` (gif A/B), `flow` (crowd/parallax), `combat` (scene). See `04-inventory`.
- **Threshold**: per-cell adaptive (dot lit if `luma >= cell_mean`) looks best; `>=` (not `>`) closes
  the worst holes (flat cells were going fully blank). A tunable bias `luma >= mean*s` is being explored
  in `anim`. Global-mean and Bayer-dither are alternatives (less texture / controlled stipple).

## Green-screen keying (BETTER than white — NOT solved)
- **Not in the fully-solid bucket.** Green screen + `--chroma auto` (samples the bg per-frame) is
  clearly **better than white** — a motion-blurred silver blade blurs to near-white and gets eaten by a
  white key; green sits farther from creature colors. BUT **green flashes/leaks still occur** (user
  observed them). So this is an *improvement*, not a finished solution — treat it as partial.
- Frog problem (green creature) → override to **magenta**.
- Open directions (not proven to fully fix it): adaptive key = pick the color that **maximizes
  min-distance to the creature's color histogram**, injected at the composite step; or **rembg** per-frame
  segmentation (color-agnostic, but has its own edge flicker). Keying is an open problem to harden.

## Idle animation — the reliable animation win
- **Wan 2.2 TI2V-5B I2V** with a *subtle* prompt ("idle breathing, gentle sway") produces **coherent,
  alive** idle loops. This is the one motion that Just Works. `animate.sh` wraps it.
- Frames must be **4n+1** (e.g. 17, 25, 33). ~2–4 min/clip on 16 GB.

## Frame timing / easing — a real lever we own
- `playframes --ease anticipate|smooth` redistributes playback time (slow windup → fast snap → settle)
  without changing frames. User feedback: **"much improved"** on the lifeless FLF2V. Timing/easing is a
  *playback* feature we fully control — combine it with ANY motion approach.

## Weaponless body motion generates clean (key validation)
- `wl_bounce`, `wl_lunge` (weaponless mouse, dynamic body motion) are **fully coherent, no melting** —
  including the exact frames where the *sworded* version melted. **This proves the weapon was the only
  thing melting**, and validates the "generate weaponless, attach weapon after" decomposition (see 03).
