# Asset Generation — Real sd-cli Invocation & Output

> **Status: draft (not started).** Fixes `66-asset-generation-api`'s generation half so it actually runs against the real `sd-cli` binary: correct the image and animation argv to `sd-cli`'s real interface, resolve model files to absolute paths, and reconcile the animation output contract (`sd-cli` writes a video; the pipeline reads PNG frames). Proves image and animation generation end-to-end with a live RED/GREEN, the way `73` proved the local text model. Bundling, a model download/registry flow, and licensing are explicitly deferred — the goal here is to prove generation works, not to distribute.

## Why
`66`'s orchestration (lifecycle, cache, GPU-gating, background removal, the three operations) is correct and stays. But its two `RecipeBackend` implementations build an argv that does not match the real `sd-cli`, and were only ever unit-tested against a fake runner. A fresh-context evaluation ran the production argv against the real binary + models on this machine and both failed:
- Image: `sd-cli z_image_turbo --cfg-scale 1.0 …` -> `[ERROR] unknown argument: z_image_turbo` (exit 1). No `-M img_gen`; the model is a bare positional; `--vae` and `--llm` are missing.
- Animation: `sd-cli vid_gen --diffusion-model minimax_h3….gguf …` -> `[ERROR] unknown argument: vid_gen`. Missing `-M`; model refs are filename-only; `--lora-model-dir loras` points nowhere.

The models themselves are fine: the corrected commands generate real output (a 512x512 PNG still in ~4s; a completed H3 clip in ~122s). So this is an argv + path-resolution + output-contract fix, not a generation-capability problem.

## The real sd-cli interface (target)
From `sd-cli --help` and `experiments/creature_lab/findings/{05-proven-pipeline,12-attack-animation-pipeline}.md`, mode is `-M/--mode <img_gen|vid_gen|…>` and models are passed by path via `-m`/`--diffusion-model`/`--vae`/`--audio-vae`/`--llm`. There is no positional model or mode. The proven, working forms:
- Image: `sd-cli -M img_gen --diffusion-model <z_image gguf> --vae <ae.safetensors> --llm <Qwen3-4B gguf> --cfg-scale 1.0 --steps 8 --diffusion-fa --seed N --width W --height H -p "<prompt>" -o <out.png>`.
- Animation: `sd-cli -M vid_gen --diffusion-model <minimax_h3 gguf> --vae <video vae> --audio-vae <audio vae> --llm <qwen3vl gguf> --lora-model-dir <loras dir> --cfg-scale 1.0 --flow-shift 12.0 --strength 1.0 --sampling-method euler --seed N -W 512 -H 512 --diffusion-fa --offload-to-cpu --rng cpu --clip-on-cpu --vae-tiling --temporal-tiling --steps 8 --video-frames N --fps N --init-img <init> -p "<prompt>" -o <out>` (writes a video file, not PNG frames).

## Scope
1. **Runner fix.** `SdCliRunner::run` currently does `Command::new(bin).arg(&invocation.model).args(&invocation.args)`, prepending the `model` field as a bare positional — the root of both failures. The runner must run `sd-cli` with the invocation's args verbatim, no prepended positional. The `SdCliInvocation` type changes accordingly (drop the bare-positional `model`; the mode and model-path flags live in the args the backends build).
2. **Image backend (`ZImageBackend::invocation`).** Emit `-M img_gen`, `--diffusion-model <path>`, `--vae <path>`, `--llm <path>` (Z-Image needs the diffusion model, the FLUX-style VAE, and the Qwen text encoder), plus the existing sampling args and `-o`. No bare model positional.
3. **Animation backend (`MiniMaxH3Backend::invocation`).** Emit `-M vid_gen`, `--diffusion-model`/`--vae`/`--audio-vae`/`--llm` by resolved path, `--lora-model-dir <resolved dir>`, and the existing H3 config args.
4. **Model-path resolution.** Both backends resolve their known model filenames to absolute paths under a configured **assets-models directory** (an env var / config value; e.g. `AGENTBATTLEGROUND_SDCLI_MODELS_DIR`). Filename-only refs are replaced by resolved paths. A missing model file resolves to a clear error, never a silent bad argv. Co-locating or downloading those files is a setup/distribution concern deferred out of this spec; for now the directory is configured to where the files already are.
5. **Animation output contract.** `sd-cli` `vid_gen` writes a single video file (`.avi`/`.webm`), but `operations.rs::materialize_clip` reads a PNG frame sequence from the output dir. Reconcile it: after the video is produced, extract its frames to the PNG sequence the pipeline expects (an ffmpeg step, as the findings pipeline uses: `ffmpeg -i <video> … f_%03d.png`), or request a frame-sequence output form `sd-cli` supports. `materialize_clip` then finds the frames it reads.
6. **Verification.** A live `#[ignore]d` test (mirroring `73`'s `text_gen/live_test.rs`) drives the game's real `SdCliRunner` + the operations against the real `sd-cli` + the local models and asserts the artifacts exist: `generate_image` produces a non-empty still PNG, and `generate_animation` produces a PNG frame sequence (more than one frame). This is the RED/GREEN that proves the fix, run manually since the offline gate cannot spawn `sd-cli` over multi-GB models.

Out of scope (deferred, not distributing yet): a download/verify/registry flow for the image/animation models (no equivalent of `73`'s `model_install` exists for `asset_gen`, and it is not needed to prove generation works with local models); bundling the `sd-cli` binary + its runtime; model licensing (H3's redistribution/output terms). Background removal, the cache, GPU-gating, and the operation surfaces from `66` are unchanged.

## Notes / gotchas from the evaluation
- The image and animation model files are not co-located today (diffusion GGUFs under `experiments/creature_lab/models` and `models_sdcpp`, both VAEs only under `ComfyUI/models/vae`, the LoRA under `ComfyUI/models/loras`). The configured assets-models directory must resolve each file's real location; the simplest path for now is one directory the setup step populates (symlinks) with every required file + a `loras` subdir.
- `sd-cli` is invoked once per operation and is GPU-gated exactly as `66` already handles; nothing here changes the capability query or the no-GPU fallback.

## Dependencies
- `66-asset-generation-api` — the orchestration, `RecipeBackend` trait, `SdCliRunner`, operations, and cache this fixes the generation half of; `66` is reopened until this lands and both paths are e2e-verified.
- `experiments/creature_lab/findings/{05-proven-pipeline,12-attack-animation-pipeline}.md` — the proven real `sd-cli` commands this matches.
- `67`/`68` — the hatchery consumers that need real stills + clips for the gated hatch to complete.
- `73-local-text-model` — the pattern for the live `#[ignore]d` end-to-end verification test.

## Open Questions / TBDs
- Whether to extract video frames with an ffmpeg subprocess (another bundled dependency later) or drive `sd-cli` to emit a PNG sequence directly, if it supports one for `vid_gen`.
- The exact shape of the assets-models directory config (single dir with everything vs. per-model-kind paths), to be settled when the deferred download/registry flow is designed.
