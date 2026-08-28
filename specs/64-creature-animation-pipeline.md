# Creature Animation Pipeline

> **Status: draft (not started).** How short animated creature clips (attack animations, and other generated action clips like a hatching sequence) are produced. Sibling to `17-creature-art-asset-pipeline`, which covers the two static image fidelities; this covers the animated path the still-image models cannot produce. Feeds `13-rendering`'s existing generic frame-sequence player.

## Purpose
Defines how a creature's animated action clips are generated, from a static source image to a frame-sequence asset the renderer can play. This is the animation recipe behind `66-asset-generation-api`'s `generate_animation` operation; *which* actions get generated for *which* features (attack animations, `65-hatchery`'s crack/reveal sequence, etc.) is each calling feature's own concern via `66`, not this spec's.

## Scope
- Local, GPU-gated generation of short (2-6s) action clips from a single starting image + text prompt.
- The background/shadow cleanup step required before an image is usable as generation input.
- The async job lifecycle: submit, poll, success/error/timeout handling — no silent stalls.
- Producing the final frame-sequence asset `13-rendering`'s player consumes.
- The reusable service through which every calling feature requests clips.

Out of scope: which specific creature/action combinations get generated, when, or by what trigger (feature-specific); the static two-fidelity image pipeline (`17`).

## Generation Engine
**MiniMax H3**, an open-weight image-to-video model, run through **`stable-diffusion.cpp`** — the same pure-C/C++, GGUF-quantized, ggml-family engine as `17`'s still-image path. No Python, ComfyUI, or PyTorch. The engine is a single native binary (`sd-cli`) driven as a sandboxed subprocess, the same sibling-binary pattern the game uses for the inspector (`current_exe().parent()`).

H3's four components are passed to `sd-cli -M vid_gen` as separate files:
- **Diffusion transformer** — MiniMax H3 FL2VA, pruned, GGUF Q4_K_M (~11GB).
- **Text encoder** — Qwen3-VL-32B (the MiniMax-H3 variant, including its vision tower), GGUF (~13-18GB).
- **Video VAE** and **audio VAE** (safetensors). H3 is a joint audio+video model; the audio stream decodes even when only the video frames are consumed.
- **Turbo LoRA** — the distilled few-step sampling adapter, applied at strength 1.0.

Weights must be in `stable-diffusion.cpp`'s GGUF tensor layout; ComfyUI-ecosystem GGUFs will not load.

GPU-gating: same framing as `17`. Requires a local GPU with ~16GB VRAM (reference config peaks ~14.6GB). Devices without a suitable GPU cannot use in-app animation generation; the fallback for that case belongs to whichever feature depends on this.

## Reference Configuration
The generation config the pipeline runs. Expect ~3 min/clip on a 16GB GPU (Vulkan backend).

- Sampling: `euler`, 8 steps, Turbo LoRA at 1.0, `--cfg-scale 1.0` (the distilled LoRA is guidance-free).
- `--flow-shift 12.0`, `--strength 1.0`. Full keyframe denoise is required for motion range; sd.cpp's `--strength` default of 0.75 damps how far the animation moves from the still.
- Canvas: 512px-class, 56 frames at 24fps (~2.3s). Resolution is not a quality lever: output is down-rezzed to braille, so the canvas is sized to fit VRAM, not for pixel detail. Larger canvases or longer clips fit a 16GB card only via graph-cut streaming (`--stream-layers --max-vram`), at roughly 20× the wall-clock; not worth it for braille output.
- Memory flags: `--offload-to-cpu --clip-on-cpu --vae-tiling --temporal-tiling`. The 32B text encoder runs on CPU (`--clip-on-cpu`) or it exceeds 16GB during graph build.
- Backend: Vulkan (cross-vendor, and the working path when the installed CUDA toolkit predates the GPU).

An out-of-VRAM condition is recoverable: retry with graph-cut streaming or a smaller canvas rather than hard-failing.

## Role in the generation API
This spec is the animation *recipe* behind `66-asset-generation-api`'s `generate_animation` operation. `66` owns the API surface, the async job lifecycle, caching, GPU-gating, and placement (`crates/game/`); this spec supplies what that operation runs — the H3 engine, the reference config above, and the prompt convention below. Callers never invoke this recipe directly; they go through `66`. The still it animates is produced by `66`'s `generate_image` (spec `17`) and background-cleaned first.

## Pipeline Steps
1. **Background/shadow cleanup.** The source still's flat background and any baked-in drop shadow are stripped and replaced with a uniform flat color before generation — the model does not reliably reproduce a shadow's exact shape through generated motion, leaving visible bleed. A native chroma-key + shadow-strip step, required, not optional.
   - If the creature's own dominant color is in the same hue family as the default background key color, generation must use a different key color (already true of `17`'s pipeline for green creatures on a green background) or this step cannot distinguish character from background.
2. **Submit an image-to-video job** to the `sd-cli` subprocess: the cleaned image as the starting frame (`--init-img`), plus a text prompt describing the action.
   - **Prompt convention (load-bearing, not stylistic):** style-preservation language (art style, outline weight, color/proportion consistency, static camera, flat background color) plus the action described as specific, concrete physical beats (windup, the motion itself, impact/extension, brief follow-through). A vague verb ("attacks fiercely") does not reliably produce the intended motion; a beat-by-beat physical description does. The style being preserved is whatever `17`'s generation already produced — including its cartoony-not-realistic, avoid-heavy-dark-color guidance — this step never introduces a style shift of its own.
3. **Track the job to completion** on a fixed interval, with an explicit timeout. On success, error, or timeout, surface a real status to whatever triggered the generation — never leave a caller (or the player) with no signal for an indefinite period.
4. **Extract frames** from the completed clip into a frame-sequence asset (the same PNG-sequence shape `13-rendering`'s player already consumes).
5. **Cache the result**, keyed by (subject, action). A one-time bake at creation/definition time, not repeated on every view or battle.

## Behavior Notes
- A held prop (e.g. a weapon) stays attached to the creature throughout a generated motion, including fast committed attack swings.
- Motion magnitude is driven by the prompt and by `--flow-shift` / `--strength`. A pose whose limbs are load-bearing (e.g. a handstand) keeps those limbs planted by physical plausibility; whole-body dynamism comes from beat-by-beat action language, not from motion-adjective padding.

## Open Questions / TBDs
- **Redistribution licensing** — whether the H3 weights, their GGUF re-quantizations, and the Qwen3-VL encoder may be bundled/redistributed inside a shipped game. Gates shipping, not development.
- **No-GPU fallback** for animation — belongs to whichever feature depends on this (`65-hatchery`, battle viewer).
- **Cross-platform / cross-hardware** — the Vulkan backend is the cross-vendor target; Windows, AMD, and Mac (Metal/MoltenVK) still need validation.

## Dependencies
- Backend of `66-asset-generation-api` — implements its `generate_animation` operation.
- Sibling to `17-creature-art-asset-pipeline` — shares the background-removed source-image convention and the same native `stable-diffusion.cpp` engine; adds the animated-clip capability `17`'s still-image models cannot produce.
- Feeds `13-rendering` — produces the frame-sequence assets the existing player already plays.
- Consumed (via `66`) by `65-hatchery` (hatching sequence, starting-attack animation) and any future spec needing a generated creature action clip.
