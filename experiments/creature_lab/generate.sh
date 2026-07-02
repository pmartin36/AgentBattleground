#!/usr/bin/env bash
# creature_lab/generate.sh
# End-to-end creature bake-off on stable-diffusion.cpp (the engine the game will ship).
#
# Pipeline:
#   1. txt2img          -> high-detail creature  (creature-viewer source)
#   2. img2img simplify -> low-detail creature   (battlefield source, identity carried via low denoise)
#   3. rembg            -> cut background from BOTH (busy backgrounds wreck the braille read)
#   4. downrez/preview  -> both fidelities in braille, in the terminal
#
# Config via env (defaults assume the layout from README.md):
#   SD_CLI     path to sd-cli binary           (default: ./stable-diffusion.cpp/build/bin/sd-cli)
#   MODELS     dir holding the model files      (default: ./models)
#   OUT        output dir                        (default: ./out)
#   DIFFUSION  Z-Image Turbo gguf filename       (in $MODELS)
#   VAE        vae filename                       (in $MODELS)
#   LLM        Qwen3-4B text-encoder gguf         (in $MODELS)
#   STRENGTH   img2img denoise (0.45-0.55 good)   (default: 0.5)
#   BG         background instruction appended to both prompts (override to taste)
#   HI_W       creature-viewer braille width      (default: 100)
#   FIELD_W    battlefield braille width          (default: 36)
#
# Usage: ./generate.sh "<creature prompt>" [name]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
SD_CLI="${SD_CLI:-$HERE/stable-diffusion.cpp/build/bin/sd-cli}"
MODELS="${MODELS:-$HERE/models}"
OUT="${OUT:-$HERE/out}"
DOWNREZ="$HERE/../ascii_test/target/release/downrez"
PREVIEW="$HERE/../ascii_test/preview.sh"

DIFFUSION="${DIFFUSION:-$MODELS/z_image_turbo-Q4_K.gguf}"
VAE="${VAE:-$MODELS/ae.safetensors}"
LLM="${LLM:-$MODELS/Qwen3-4B-Instruct-2507-Q4_K_M.gguf}"
STRENGTH="${STRENGTH:-0.5}"

# Background suppression. Z-Image Turbo runs at cfg 1.0 (negative prompts are inert there),
# so an isolated subject has to be pushed through the POSITIVE prompt. A flat solid backdrop
# also makes the rembg cutout trivial.
# Green screen: key on a saturated color the subject won't contain (survives motion-blur + bg drift
# far better than white). For green creatures (frogs), override BG+CHROMA to magenta.
BG="${BG:-isolated subject, full body, centered, solid flat vivid chroma-key green background, uniform bright green screen backdrop, no scenery, no environment, no foreground objects, no ground detail, no shadow}"

PROMPT="${1:?usage: generate.sh \"<creature prompt>\" [name]}"
NAME="${2:-creature}"

# --- sanity checks (fail loud, point at the README) ---
miss=0
for f in "$SD_CLI" "$DIFFUSION" "$VAE" "$LLM"; do
  [ -e "$f" ] || { echo "MISSING: $f" >&2; miss=1; }
done
[ "$miss" = 0 ] || { echo "-> see creature_lab/README.md for build + model downloads" >&2; exit 1; }
[ -x "$DOWNREZ" ] || { echo "build downrez:  (cd ascii_test && cargo build --release --bin downrez)" >&2; exit 1; }
[ -x "$PREVIEW" ] || { echo "missing $PREVIEW" >&2; exit 1; }
mkdir -p "$OUT"

HI_RAW="$OUT/${NAME}_hi_raw.png"
HI="$OUT/${NAME}_hi.png"
FIELD_RAW="$OUT/${NAME}_field_raw.png"
FIELD="$OUT/${NAME}_field.png"

# Resolve rembg up front: prefer the isolated local venv (no PATH pollution, no activation —
# we call its binary directly), else any rembg on PATH. Warn early if neither is present.
REMBG=""
if   [ -x "$HERE/.venv/bin/rembg" ];     then REMBG="$HERE/.venv/bin/rembg"
elif command -v rembg >/dev/null 2>&1;   then REMBG="rembg"; fi
[ -n "$REMBG" ] || echo "   note: rembg not found -> backgrounds will NOT be removed (braille will look busy). See README." >&2

# cutout <in> <out>: rembg if available and it succeeds, else passthrough copy.
cutout() {
  local in="$1" out="$2"
  if [ -n "$REMBG" ] && "$REMBG" i "$in" "$out"; then return 0; fi
  cp "$in" "$out"
}

COMMON=(--diffusion-model "$DIFFUSION" --vae "$VAE" --llm "$LLM"
        --cfg-scale 1.0 --steps 8 --diffusion-fa -v)

echo ">> [1/4] high-res creature  ->  $HI_RAW"
"$SD_CLI" "${COMMON[@]}" --width 768 --height 768 \
  -p "$PROMPT, detailed creature concept art, vivid colors, $BG" \
  -o "$HI_RAW"

echo ">> [2/4] simplify to battlefield form (img2img, strength=$STRENGTH)  ->  $FIELD_RAW"
"$SD_CLI" "${COMMON[@]}" --width 768 --height 768 \
  --init-img "$HI_RAW" --strength "$STRENGTH" \
  -p "$PROMPT, simplified flat-color sprite, bold shapes, minimal detail, thick clean silhouette, $BG" \
  -o "$FIELD_RAW"

echo ">> [3/4] cut backgrounds (both)  ->  $HI , $FIELD"
cutout "$HI_RAW" "$HI"
cutout "$FIELD_RAW" "$FIELD"

echo ">> [4/4] braille preview  (creature viewer vs battlefield)"
HI_W="${HI_W:-100}" FIELD_W="${FIELD_W:-36}" "$PREVIEW" "$HI" "$FIELD"
