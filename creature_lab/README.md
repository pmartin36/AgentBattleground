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

## Notes
- `models/`, `out/`, and `stable-diffusion.cpp/` are git-ignored (large / machine-specific).
- For the *evolution/edit* path later, sd.cpp also ships `docs/qwen_image_edit.md` (a reference-image
  edit model) — a stronger but heavier option than img2img if identity drift becomes a problem.
