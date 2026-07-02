# creature_lab

CLI experiment rig for the creature art pipeline. Generates a high-detail creature and a
simplified battlefield form, then renders both as braille — the two fidelities the game uses.

Runs on **stable-diffusion.cpp** (the ggml/GGUF engine the game will ship with — same family as
the llama.cpp text stack, no Python, sandboxable as a subprocess).

## One-time setup (on a CUDA machine — e.g. the 5070 Ti)

### 1. Build the engine
```bash
cd creature_lab
# --recurse-submodules is required: sd.cpp vendors ggml as a submodule
git clone --depth 1 --recurse-submodules https://github.com/leejet/stable-diffusion.cpp
cd stable-diffusion.cpp
mkdir build && cd build
cmake .. -DSD_CUDA=ON        # NVIDIA. AMD: -DSD_HIPBLAS=ON  cross-vendor: -DSD_VULKAN=ON  Mac: -DSD_METAL=ON  none: omit
cmake --build . --config Release
# -> produces creature_lab/stable-diffusion.cpp/build/bin/sd-cli
```

> Already cloned without submodules? Run `git submodule update --init --recursive` inside
> `stable-diffusion.cpp` before re-running cmake.

### 2. Download the models into `creature_lab/models/`
| File | Source (Hugging Face) | Goes to env var |
|---|---|---|
| `z_image_turbo-Q4_K.gguf` (or the quant you pick) | `leejet/Z-Image-Turbo-GGUF` | `DIFFUSION` |
| `ae.safetensors` (the FLUX VAE) | `ffxvs/vae-flux` (ungated mirror; official `black-forest-labs/FLUX.1-schnell` is gated) | `VAE` |
| `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | `unsloth/Qwen3-4B-Instruct-2507-GGUF` | `LLM` |

On 16 GB (5070 Ti) the Q4 Turbo model is comfortable. If you hit OOM, add `--offload-to-cpu`
to the `COMMON` array in `generate.sh`, or pick a smaller quant (Q3_K).

### 3. Build the down-rezzer + get rembg
```bash
(cd ../ascii_test && cargo build --release --bin downrez)   # braille converter
pip install rembg                                            # background removal (optional but recommended)
```

## Run

```bash
./generate.sh "a fierce crystalline lizard with glowing veins" frostlizard
```

Output lands in `creature_lab/out/`:
- `frostlizard_hi.png` — high-detail creature (creature-viewer source)
- `frostlizard_field.png` — simplified, background removed (battlefield source)

…and the terminal shows both braille fidelities: the big creature-viewer render and the small
battlefield render.

## The knobs that matter

| Env | Default | What it does |
|---|---|---|
| `STRENGTH` | `0.5` | img2img denoise. Lower = battlefield form stays closer to the high-res (same creature); higher = drifts. The dial for "is it still the same creature?" |
| `FIELD_W` | `36` | battlefield braille width (columns) |
| `HI_W` | `100` | creature-viewer braille width |

## What you're judging

1. Does the **high-res** make a gorgeous big braille creature?
2. Does the **simplified** form still read as the *same creature* when crushed to ~36 columns?
3. Is the img2img-simplified battlefield sprite meaningfully cleaner than just shrinking the
   high-res? (Compare against `FIELD_W=36 ../ascii_test/preview.sh out/NAME_hi.png` — same source,
   no simplification.) That comparison is the whole point: it tells you whether the two-image
   approach earns its keep.

## Animation (Wan 2.2 I2V) — `animate.sh`

Turns a **low-detail** sprite into an animated braille loop. Uses the *same `sd-cli` binary* via
`-M vid_gen` — no new engine.

Feed the **simplified** sprite (`out/NAME_field_raw.png`), not the high-detail creature. Low detail
wins twice: it downrezzes cleanly, and the video model has less fine detail to drift on frame-to-frame.

### Extra models into `creature_lab/models/`
| File | Source (Hugging Face) | Env |
|---|---|---|
| `Wan2.2-TI2V-5B-Q4_K_M.gguf` (3.4 GB) | `QuantStack/Wan2.2-TI2V-5B-GGUF` | `WAN` |
| `wan2.2_vae.safetensors` (5B needs its OWN vae) | `Comfy-Org/Wan_2.2_ComfyUI_Repackaged` → `split_files/vae/` | `WAN_VAE` |
| `umt5-xxl-encoder-Q8_0.gguf` (6 GB) | `city96/umt5-xxl-encoder-gguf` | `T5` |

```bash
(cd ../ascii_test && cargo build --release --bin playframes)   # braille frame-sequence player

# animate the low-detail battlefield sprite from a generate.sh run:
./animate.sh out/frostlizard_field_raw.png "idle breathing, tail sway" frostlizard
```

That runs Wan I2V → PNG frame sequence in `out/NAME_anim/` → braille playback (chroma-keyed,
ping-pong loop, `q` to quit).

### Animation knobs
| Env | Default | What it does |
|---|---|---|
| `FRAMES` | `17` | video frames (must be 4n+1). Ping-pong ~doubles the effective loop |
| `RES` | `512` | square gen resolution (Wan also likes 480/832 — tune if quality is poor) |
| `CHROMA` | `auto` | background key color; `auto` samples the bg per-frame (robust to drift). Or `R,G,B` |
| `CHROMA_THRESH` | `95` | key tolerance — raise if bg speckles remain, lower if the creature gets eaten |

**It takes a few minutes and looks idle while working.** Wan samples all frames, then does one big
VAE decode at the end — *nothing is written to disk until decode finishes*, so the output dir stays
empty the whole time. Watch the sampling step counter (`-v`) to confirm it's alive; don't kill it.

Wan's VAE decode is VRAM-hungry and OOMs on 16 GB (`vae decode compute failed` → exit-0-but-empty).
`animate.sh` runs it with `--vae-on-cpu` (decode in system RAM — slower but reliable). That's the fix
if you ever adapt the command by hand.

A clip is an **offline content step** (once per creature), never a battle-time cost. Looping is
handled by ping-pong playback; for a true seamless loop later, sd.cpp supports Wan FLF2V
(first frame = last frame) via `--init-img`/`--end-img`.

`downrez` also gained `--chroma R,G,B|auto [--chroma-thresh N]` for keying a single still (same keyer
as `playframes`), e.g. `downrez out/greenmouse_field_raw.png --chroma auto --width 54`.

## Background keying (green screen)

Video diffusion can't emit alpha, so the background is removed by **chroma-keying** a solid backdrop.
The pipeline generates creatures on a **green screen** and keys it out.

**Why green, not white:** the key color must be far from every color the creature contains. White
fails badly — a motion-blurred silver blade blurs to near-white and gets eaten, and the "white" bg
itself drifts darker frame-to-frame and leaks through. Green sits far from typical creature colors,
so it survives both. `--chroma auto` samples the bg per-frame, absorbing the drift.

**Green creatures (the frog problem):** a green frog on a green screen keys *itself* out. Immediate
fix: override to magenta — `BG="...solid magenta screen background..." CHROMA=255,0,255`.

**Productionize plan (adaptive key color) — not built yet:** don't summarize the creature to one
color (mean loses the distribution; mode ignores secondaries like a sword). Instead: rembg-isolate
the creature, histogram its colors, and pick the key color that **maximizes the minimum distance to
any creature color** (max-min) — pragmatically, score a small palette (green/magenta/blue/orange)
and take the best-separated. Inject it at the **composite step** (generate → isolate → analyze → pick
→ composite creature onto the chosen solid bg → I2V → key it); no chicken-and-egg since the bg is
chosen *after* seeing the creature.

**Color-agnostic fallback:** per-frame **segmentation** (rembg on each frame) cuts by shape, not
color — *no* frog problem. Its costs (edge flicker, dropping thin motion-blurred bits) matter much
less in chunky braille. Keep it as the escape hatch for creatures that collide with every screen color.

## Notes
- `models/`, `out/`, `stable-diffusion.cpp/`, and `.venv/` are git-ignored (large / machine-specific).
- For the *evolution/edit* path later, sd.cpp also ships `docs/qwen_image_edit.md` (a reference-image
  edit model) — a stronger but heavier option than img2img if identity drift becomes a problem.
