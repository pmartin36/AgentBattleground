# Inventory — tools, assets, commands

## Rust binaries (`experiments/ascii_test/`, `cargo build --release --bin <name>`)
| bin | what |
|---|---|
| `main` (ascii_test) | single-image fidelity: ASCII vs half-block vs braille, face crop |
| `anim` | GIF player + braille A/B; `t` cycles threshold modes (adaptive `>`/`>=`, global, bayer), side-by-side original via Kitty graphics. Plays the two Pikachu gifs + barbarian |
| `flow` | multi-sprite depth-layer crowd ("tidal wave") compositor |
| `downrez` | still image → colored braille (stdout). `--width N --chroma auto|R,G,B --chroma-thresh N --no-color` |
| `playframes` | play a frame dir OR gif as braille. `--chroma auto --pingpong --fps N --ease anticipate|smooth` |
| `combat` | attack-scene state machine: `combat <atk_idle_dir> <atk_attack_dir> <target_idle_dir>` (currently *not* good — see dead-ends) |
| `mkslash` / `mkslash2` | generate synthetic VACE control frames (crude arc / silhouette-matched). Dead-end aids |
| `attach` | composite a weapon sprite onto per-frame hand anchors: `attach <frames_dir> <weapon.png> <anchors.txt> <out_dir> [--handle hx,hy]`. anchors.txt lines: `idx x y angle_deg scale` (sparse, interpolated) |

`preview.sh` = quick two-fidelity braille preview of a still.

## Generation scripts (`experiments/creature_lab/`)
- `generate.sh "<prompt>" <name>` — hi + img2img-simplified creature, green bg default, braille preview.
  Env: `STRENGTH`, `BG`, `FIELD_W`, `HI_W`.
- `animate.sh <field_raw.png> "<motion>" <name>` — Wan I2V idle/motion → braille loop. Env: `FRAMES` (4n+1),
  `RES`, `CHROMA` (default auto), `CHROMA_THRESH`. Uses `--vae-on-cpu` (do not remove — OOM fix).

## Models (`experiments/creature_lab/models/`, all present)
`z_image_turbo-Q4_K.gguf` · `ae.safetensors` (FLUX VAE, from `ffxvs/vae-flux`) · `Qwen3-4B-Instruct-2507-Q4_K_M.gguf`
· `Wan2.2-TI2V-5B-Q4_K_M.gguf` · `wan2.2_vae.safetensors` · `umt5-xxl-encoder-Q8_0.gguf`
· `wan2.1_vace_1.3B_fp16.safetensors` (use safetensors, NOT the gguf) · `wan_2.1_vae.safetensors`.
Engine: `stable-diffusion.cpp/build/bin/sd-cli` (built with Vulkan).

## Key generated assets (`experiments/creature_lab/out/`)
- `greenmouse_field_raw.png` — the sworded mouse (green bg). `greenmouse_anim/` — clean idle (17f). ✅
- `swordless_field_raw.png` — weaponless mouse. `wl_bounce/`, `wl_lunge/` — clean weaponless motion (25f). ✅
- `slime_field_raw.png` + `slime_anim/` — purple slime enemy + idle. ✅
- `sword_sprite.png` — standalone keyable sword (for `attach`).
- `mouse_flf2v_anim/` — lifeless FLF2V slash (good `--ease` test subject).
- Dead-end outputs: `mo_lunge/` (melted sword), `wl_swing/` (turned-around dud), `mouse_sworded/` ("awful"
  attach result), `att_*`/`sl_*`/`mouse_vace*` (VACE experiments).

## Handy commands (from repo root)
```bash
# idle (works well):
./experiments/ascii_test/target/release/playframes experiments/creature_lab/out/greenmouse_anim --chroma auto --pingpong
# renderer threshold A/B:
./experiments/ascii_test/target/release/anim            # t = cycle modes
# generate a creature:
cd experiments/creature_lab && ./generate.sh "a round chunky slime, flat colors, low detail" blob
# sd-cli directly (NOTE: zsh doesn't word-split unquoted var bundles — inline flags or use a bash script):
#   -M vid_gen for video/I2V/FLF2V/VACE ; add --offload-to-cpu --vae-on-cpu on 16 GB ; frames = 4n+1
```

## Gotchas that cost time
- **zsh word-splitting**: `$FLAGS` unquoted does NOT split into args (unlike bash) → "usage" error. Inline
  flags or run inside a `bash` script.
- **`--vae-on-cpu`** required on 16 GB for Wan video, else silent OOM (exit 0, empty dir).
- **VACE**: use the **safetensors** model (gguf drops the vace tensor). VACE 1.3B needs the **wan 2.1** VAE.
- **Screen the whole arc in braille motion**, not spot stills.
